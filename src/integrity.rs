//! Read-only storage/database reconciliation report. Never deletes or
//! modifies anything - see `specs/artifact-storage` - "Storage integrity is
//! reconciled through a read-only report, not automatic deletion".

use sqlx::{Row, SqlitePool};
use std::collections::HashSet;
use std::fmt;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug)]
pub struct IntegrityError(String);

impl fmt::Display for IntegrityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for IntegrityError {}

impl From<sqlx::Error> for IntegrityError {
    fn from(err: sqlx::Error) -> Self {
        IntegrityError(format!("integrity check database error: {err}"))
    }
}

impl From<std::io::Error> for IntegrityError {
    fn from(err: std::io::Error) -> Self {
        IntegrityError(format!("integrity check filesystem error: {err}"))
    }
}

/// A snapshot of storage/database consistency at the moment it was run.
/// Every field is a *report*: nothing here is modified or deleted.
#[derive(Debug, Default, Clone)]
pub struct IntegrityReport {
    /// Artifact rows no run currently references.
    pub unreferenced_artifacts: Vec<Uuid>,
    /// Artifact rows whose content-addressed file is missing on disk.
    pub artifacts_missing_files: Vec<Uuid>,
    /// Files under the artifact root with no matching `artifacts` row.
    pub orphaned_artifact_files: Vec<PathBuf>,
    /// Files under the model root with no matching `model_assets` row.
    pub orphaned_model_files: Vec<PathBuf>,
    /// External model assets currently marked unavailable.
    pub unavailable_models: Vec<Uuid>,
}

impl IntegrityReport {
    pub fn is_clean(&self) -> bool {
        self.unreferenced_artifacts.is_empty()
            && self.artifacts_missing_files.is_empty()
            && self.orphaned_artifact_files.is_empty()
            && self.orphaned_model_files.is_empty()
            && self.unavailable_models.is_empty()
    }
}

async fn find_unreferenced_artifacts(pool: &SqlitePool) -> Result<Vec<Uuid>, IntegrityError> {
    let rows = sqlx::query(
        "SELECT a.id FROM artifacts a
         WHERE NOT EXISTS (
             SELECT 1 FROM runs r
             WHERE r.stdout_artifact_id = a.id
                OR r.stderr_artifact_id = a.id
                OR r.crash_artifact_id = a.id
                OR r.input_artifact_id = a.id
                OR r.output_artifact_id = a.id
         )",
    )
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            let id: String = row.try_get("id")?;
            Uuid::parse_str(&id)
                .map_err(|err| IntegrityError(format!("invalid stored artifact id: {err}")))
        })
        .collect()
}

async fn find_artifacts_missing_files(
    pool: &SqlitePool,
    artifact_root: &Path,
) -> Result<Vec<Uuid>, IntegrityError> {
    let rows = sqlx::query("SELECT id, storage_path FROM artifacts")
        .fetch_all(pool)
        .await?;

    let mut missing = Vec::new();
    for row in rows {
        let id: String = row.try_get("id")?;
        let storage_path: String = row.try_get("storage_path")?;
        if tokio::fs::metadata(artifact_root.join(&storage_path))
            .await
            .is_err()
        {
            missing.push(
                Uuid::parse_str(&id)
                    .map_err(|err| IntegrityError(format!("invalid stored artifact id: {err}")))?,
            );
        }
    }
    Ok(missing)
}

async fn find_unavailable_models(pool: &SqlitePool) -> Result<Vec<Uuid>, IntegrityError> {
    let rows = sqlx::query("SELECT id FROM model_assets WHERE available = 0")
        .fetch_all(pool)
        .await?;
    rows.into_iter()
        .map(|row| {
            let id: String = row.try_get("id")?;
            Uuid::parse_str(&id)
                .map_err(|err| IntegrityError(format!("invalid stored model asset id: {err}")))
        })
        .collect()
}

/// Walks `root/sha256/**/*`, returning each file's path relative to `root`
/// in the same `sha256/<prefix>/<hash>` form artifacts and model assets
/// store. A missing root (nothing has ever been written there) is not an
/// error - it just means there are no files to report.
async fn walk_content_addressed_files(root: &Path) -> Result<Vec<PathBuf>, IntegrityError> {
    let sha256_dir = root.join("sha256");
    let mut found = Vec::new();

    let mut prefix_entries = match tokio::fs::read_dir(&sha256_dir).await {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(found),
        Err(err) => return Err(err.into()),
    };

    while let Some(prefix_entry) = prefix_entries.next_entry().await? {
        if !prefix_entry.file_type().await?.is_dir() {
            continue;
        }
        let mut file_entries = tokio::fs::read_dir(prefix_entry.path()).await?;
        while let Some(file_entry) = file_entries.next_entry().await? {
            if file_entry.file_type().await?.is_file() {
                let relative = PathBuf::from("sha256")
                    .join(prefix_entry.file_name())
                    .join(file_entry.file_name());
                found.push(relative);
            }
        }
    }
    Ok(found)
}

async fn find_orphaned_artifact_files(
    pool: &SqlitePool,
    artifact_root: &Path,
) -> Result<Vec<PathBuf>, IntegrityError> {
    let on_disk = walk_content_addressed_files(artifact_root).await?;
    let recorded: HashSet<String> = sqlx::query("SELECT storage_path FROM artifacts")
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|row| row.try_get::<String, _>("storage_path"))
        .collect::<Result<_, _>>()?;

    Ok(on_disk
        .into_iter()
        .filter(|path| !recorded.contains(&path.to_string_lossy().replace('\\', "/")))
        .collect())
}

async fn find_orphaned_model_files(
    pool: &SqlitePool,
    model_root: &Path,
) -> Result<Vec<PathBuf>, IntegrityError> {
    let on_disk = walk_content_addressed_files(model_root).await?;
    let recorded: HashSet<String> =
        sqlx::query("SELECT relative_path FROM model_assets WHERE relative_path IS NOT NULL")
            .fetch_all(pool)
            .await?
            .into_iter()
            .map(|row| row.try_get::<String, _>("relative_path"))
            .collect::<Result<_, _>>()?;

    Ok(on_disk
        .into_iter()
        .filter(|path| !recorded.contains(&path.to_string_lossy().replace('\\', "/")))
        .collect())
}

/// Runs every reconciliation check and returns the combined report.
/// Read-only: no row or file is created, modified, or deleted.
pub async fn check(
    pool: &SqlitePool,
    artifact_root: &Path,
    model_root: &Path,
) -> Result<IntegrityReport, IntegrityError> {
    Ok(IntegrityReport {
        unreferenced_artifacts: find_unreferenced_artifacts(pool).await?,
        artifacts_missing_files: find_artifacts_missing_files(pool, artifact_root).await?,
        orphaned_artifact_files: find_orphaned_artifact_files(pool, artifact_root).await?,
        orphaned_model_files: find_orphaned_model_files(pool, model_root).await?,
        unavailable_models: find_unavailable_models(pool).await?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifact_store::{ArtifactKind, store_artifact_bytes};
    use crate::db;
    use crate::model_registry::{ExternalModelStorage, ModelStorage};
    use crate::runs::insert_run;

    async fn test_pool_and_roots() -> (SqlitePool, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let db_path = dir.path().join("benchmarks.sqlite3");
        let database_url = format!("sqlite://{}", db_path.display());
        let pool = db::connect_and_migrate(&database_url)
            .await
            .expect("failed to connect and migrate");
        (pool, dir)
    }

    #[tokio::test]
    async fn a_freshly_migrated_database_reports_clean() {
        let (pool, dir) = test_pool_and_roots().await;
        let report = check(
            &pool,
            &dir.path().join("artifacts"),
            &dir.path().join("models"),
        )
        .await
        .expect("check should succeed");
        assert!(report.is_clean());
    }

    #[tokio::test]
    async fn reports_every_seeded_orphan_condition() {
        let (pool, dir) = test_pool_and_roots().await;
        let artifact_root = dir.path().join("artifacts");
        let temporary_dir = dir.path().join("temporary");
        let model_root = dir.path().join("models");

        // 1. An unreferenced artifact: stored, but no run points at it.
        let unreferenced = store_artifact_bytes(
            &pool,
            &artifact_root,
            &temporary_dir,
            ArtifactKind::Prompt,
            None,
            None,
            b"unreferenced".to_vec(),
        )
        .await
        .expect("store should succeed");

        // 2. An artifact row whose file is missing on disk.
        let missing_file = store_artifact_bytes(
            &pool,
            &artifact_root,
            &temporary_dir,
            ArtifactKind::Output,
            None,
            None,
            b"will lose its file".to_vec(),
        )
        .await
        .expect("store should succeed");
        let record = crate::artifact_store::get_artifact_record(&pool, missing_file.id)
            .await
            .unwrap()
            .unwrap();
        std::fs::remove_file(artifact_root.join(&record.storage_path)).unwrap();
        // Referenced so it doesn't also show up as unreferenced - isolates
        // the "missing file" condition from the "unreferenced" one.
        let model_path = dir.path().join("model.pte");
        std::fs::write(&model_path, b"model bytes").unwrap();
        let model_asset = ExternalModelStorage
            .register(&pool, &model_path)
            .await
            .unwrap();
        let mut new_run =
            crate::runs::test_support::minimal_new_run(uuid::Uuid::now_v7(), model_asset.id);
        new_run.output_artifact_id = Some(missing_file.id);
        insert_run(&pool, &new_run).await.unwrap();

        // 3. A file on disk with no database row.
        std::fs::create_dir_all(artifact_root.join("sha256").join("ab")).unwrap();
        std::fs::write(
            artifact_root.join("sha256").join("ab").join("orphan-file"),
            b"orphan",
        )
        .unwrap();

        // 4. An unavailable external model (file removed after registration).
        std::fs::remove_file(&model_path).unwrap();
        ExternalModelStorage
            .verify(&pool, &model_asset)
            .await
            .unwrap();

        let report = check(&pool, &artifact_root, &model_root)
            .await
            .expect("check should succeed");

        assert_eq!(report.unreferenced_artifacts, vec![unreferenced.id]);
        assert_eq!(report.artifacts_missing_files, vec![missing_file.id]);
        assert_eq!(
            report.orphaned_artifact_files,
            vec![PathBuf::from("sha256/ab/orphan-file")]
        );
        assert_eq!(report.unavailable_models, vec![model_asset.id]);
        assert!(!report.is_clean());
    }
}
