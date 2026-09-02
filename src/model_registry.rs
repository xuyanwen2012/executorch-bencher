//! Model asset registration and verification.
//!
//! `ExternalModelStorage` is the only implementation shipped in this
//! change: it registers a `.pte` file in place (never copying it) and
//! verifies it cheaply using a cached `(size_bytes, file_modified_at)`
//! comparison, falling back to a full rehash only when either has changed.
//! See `specs/artifact-storage` - "External model assets are registered
//! once without copying" and "External model verification avoids
//! unnecessary rehashing".
//!
//! `ModelStorage` is defined so a future `ManagedModelStorage` - writing a
//! deduplicated copy under `models/sha256/<prefix>/<sha256>`, mirroring
//! `artifact_store`'s content-addressing - can be added later behind the
//! same trait and the same `model_assets` schema, without touching this
//! module's callers. It is intentionally **not implemented** in this
//! change; see `design.md` - "`model_assets` and `ModelStorage` trait:
//! external implemented, managed abstracted".

use chrono::{DateTime, Utc};
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::{Row, SqlitePool};
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tokio::io::AsyncReadExt;
use utoipa::ToSchema;
use uuid::Uuid;

const HASH_CHUNK_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ModelStorageMode {
    External,
    Managed,
}

impl ModelStorageMode {
    pub fn as_str(self) -> &'static str {
        match self {
            ModelStorageMode::External => "external",
            ModelStorageMode::Managed => "managed",
        }
    }
}

impl TryFrom<&str> for ModelStorageMode {
    type Error = ModelRegistryError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "external" => Ok(ModelStorageMode::External),
            "managed" => Ok(ModelStorageMode::Managed),
            other => Err(ModelRegistryError::Other(format!(
                "invalid model storage mode: {other:?}"
            ))),
        }
    }
}

#[derive(Debug)]
pub enum ModelRegistryError {
    Io(std::io::Error),
    Db(sqlx::Error),
    /// The path given for registration doesn't exist or isn't a regular
    /// file.
    NotARegularFile(PathBuf),
    Other(String),
}

impl fmt::Display for ModelRegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModelRegistryError::Io(err) => write!(f, "model filesystem error: {err}"),
            ModelRegistryError::Db(err) => write!(f, "model database error: {err}"),
            ModelRegistryError::NotARegularFile(path) => {
                write!(f, "not a regular file: {}", path.display())
            }
            ModelRegistryError::Other(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for ModelRegistryError {}

impl From<std::io::Error> for ModelRegistryError {
    fn from(err: std::io::Error) -> Self {
        ModelRegistryError::Io(err)
    }
}

impl From<sqlx::Error> for ModelRegistryError {
    fn from(err: sqlx::Error) -> Self {
        ModelRegistryError::Db(err)
    }
}

/// A registered model asset, as read back from `model_assets`.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelAsset {
    pub id: Uuid,
    pub sha256: String,
    pub original_name: String,
    pub size_bytes: i64,
    pub model_format: String,
    pub storage_mode: ModelStorageMode,
    pub external_path: Option<String>,
    pub relative_path: Option<String>,
    pub file_modified_at: Option<DateTime<Utc>>,
    pub registered_at: DateTime<Utc>,
    pub last_verified_at: Option<DateTime<Utc>>,
    pub available: bool,
}

fn now_text() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn row_to_asset(row: sqlx::sqlite::SqliteRow) -> Result<ModelAsset, ModelRegistryError> {
    let id: String = row.try_get("id")?;
    let storage_mode: String = row.try_get("storage_mode")?;
    let available: i64 = row.try_get("available")?;
    Ok(ModelAsset {
        id: Uuid::parse_str(&id)
            .map_err(|err| ModelRegistryError::Other(format!("invalid model asset id: {err}")))?,
        sha256: row.try_get("sha256")?,
        original_name: row.try_get("original_name")?,
        size_bytes: row.try_get("size_bytes")?,
        model_format: row.try_get("model_format")?,
        storage_mode: ModelStorageMode::try_from(storage_mode.as_str())?,
        external_path: row.try_get("external_path")?,
        relative_path: row.try_get("relative_path")?,
        file_modified_at: row.try_get("file_modified_at")?,
        registered_at: row.try_get("registered_at")?,
        last_verified_at: row.try_get("last_verified_at")?,
        available: available != 0,
    })
}

/// Streams `path` once, computing its SHA-256 without buffering the whole
/// file in memory.
async fn hash_file(path: &Path) -> Result<String, ModelRegistryError> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; HASH_CHUNK_BYTES];
    loop {
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex_encode(&hasher.finalize()))
}

fn system_time_to_utc(time: SystemTime) -> DateTime<Utc> {
    DateTime::<Utc>::from(time)
}

pub async fn get_model_asset(
    pool: &SqlitePool,
    id: Uuid,
) -> Result<Option<ModelAsset>, ModelRegistryError> {
    let row = sqlx::query("SELECT * FROM model_assets WHERE id = ?")
        .bind(id.to_string())
        .fetch_optional(pool)
        .await?;
    row.map(row_to_asset).transpose()
}

pub async fn find_by_sha256(
    pool: &SqlitePool,
    sha256: &str,
) -> Result<Option<ModelAsset>, ModelRegistryError> {
    let row = sqlx::query("SELECT * FROM model_assets WHERE sha256 = ?")
        .bind(sha256)
        .fetch_optional(pool)
        .await?;
    row.map(row_to_asset).transpose()
}

pub async fn list_model_assets(pool: &SqlitePool) -> Result<Vec<ModelAsset>, ModelRegistryError> {
    let rows = sqlx::query("SELECT * FROM model_assets ORDER BY registered_at")
        .fetch_all(pool)
        .await?;
    rows.into_iter().map(row_to_asset).collect()
}

/// The behavioral contract a model-storage mode implements: register a
/// model once, verify it cheaply before a run and fully on demand, and
/// resolve where its content currently lives. `ExternalModelStorage`
/// implements this now; a future `ManagedModelStorage` would implement it
/// against `models/sha256/<prefix>/<sha256>` instead.
///
/// Uses `async fn` directly rather than boxed futures: this trait is only
/// ever called from within this crate, never used as a trait object, so the
/// usual "auto trait bounds on the future can't be named" caveat doesn't
/// apply here.
#[allow(async_fn_in_trait)]
pub trait ModelStorage {
    async fn register(
        &self,
        pool: &SqlitePool,
        source_path: &Path,
    ) -> Result<ModelAsset, ModelRegistryError>;

    /// Cheap pre-run verification: reuses the cached SHA-256 when the
    /// file's size and modification time are unchanged, rehashes when
    /// either differs, and marks the asset unavailable when the file is
    /// gone.
    async fn verify(
        &self,
        pool: &SqlitePool,
        asset: &ModelAsset,
    ) -> Result<ModelAsset, ModelRegistryError>;

    /// Always rehashes from the current file content and updates
    /// `last_verified_at`, regardless of cached size/modification time.
    async fn verify_full(
        &self,
        pool: &SqlitePool,
        asset: &ModelAsset,
    ) -> Result<ModelAsset, ModelRegistryError>;

    /// Where this asset's content currently lives on disk.
    fn resolve_content_path(&self, asset: &ModelAsset) -> PathBuf;
}

/// External mode: the model file stays wherever it already is; the
/// registry only ever reads it, never copies it. Default mode for the
/// current multi-gigabyte `.pte` models.
pub struct ExternalModelStorage;

impl ExternalModelStorage {
    /// Marks `asset` unavailable (file missing) without rehashing.
    async fn mark_unavailable(
        pool: &SqlitePool,
        asset: &ModelAsset,
    ) -> Result<ModelAsset, ModelRegistryError> {
        sqlx::query("UPDATE model_assets SET available = 0 WHERE id = ?")
            .bind(asset.id.to_string())
            .execute(pool)
            .await?;
        get_model_asset(pool, asset.id)
            .await?
            .ok_or_else(|| ModelRegistryError::Other("model asset disappeared mid-update".into()))
    }

    /// Rehashes the file at `asset.external_path` and reconciles the
    /// result: updates `asset` in place when its SHA-256 is unique, or
    /// marks `asset` unavailable (superseded by an existing registration)
    /// when the freshly computed hash collides with a *different* already
    /// -registered asset.
    async fn rehash_and_reconcile(
        pool: &SqlitePool,
        asset: &ModelAsset,
        path: &Path,
        size_bytes: i64,
        modified_at: DateTime<Utc>,
    ) -> Result<ModelAsset, ModelRegistryError> {
        let sha256 = hash_file(path).await?;
        let now = now_text();

        let update_result = sqlx::query(
            "UPDATE model_assets
             SET sha256 = ?, size_bytes = ?, file_modified_at = ?, last_verified_at = ?, available = 1
             WHERE id = ?",
        )
        .bind(&sha256)
        .bind(size_bytes)
        .bind(modified_at)
        .bind(&now)
        .bind(asset.id.to_string())
        .execute(pool)
        .await;

        match update_result {
            Ok(_) => get_model_asset(pool, asset.id).await?.ok_or_else(|| {
                ModelRegistryError::Other("model asset disappeared mid-update".into())
            }),
            Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => {
                // The rehashed content matches a different, already
                // -registered asset; this one is a stale duplicate.
                Self::mark_unavailable(pool, asset).await
            }
            Err(err) => Err(err.into()),
        }
    }
}

impl ModelStorage for ExternalModelStorage {
    async fn register(
        &self,
        pool: &SqlitePool,
        source_path: &Path,
    ) -> Result<ModelAsset, ModelRegistryError> {
        let metadata = tokio::fs::metadata(source_path)
            .await
            .map_err(|_| ModelRegistryError::NotARegularFile(source_path.to_path_buf()))?;
        if !metadata.is_file() {
            return Err(ModelRegistryError::NotARegularFile(
                source_path.to_path_buf(),
            ));
        }

        let sha256 = hash_file(source_path).await?;
        let size_bytes = metadata.len() as i64;
        let modified_at = system_time_to_utc(metadata.modified()?);

        if let Some(existing) = find_by_sha256(pool, &sha256).await? {
            return Ok(existing);
        }

        let id = Uuid::now_v7();
        let original_name = source_path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| source_path.to_string_lossy().into_owned());
        let external_path = source_path.to_string_lossy().into_owned();
        let now = now_text();

        let insert_result = sqlx::query(
            "INSERT INTO model_assets (
                id, sha256, original_name, size_bytes, model_format, storage_mode,
                external_path, relative_path, file_modified_at, registered_at,
                last_verified_at, available
            ) VALUES (?, ?, ?, ?, 'pte', 'external', ?, NULL, ?, ?, ?, 1)",
        )
        .bind(id.to_string())
        .bind(&sha256)
        .bind(&original_name)
        .bind(size_bytes)
        .bind(&external_path)
        .bind(modified_at)
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await;

        match insert_result {
            Ok(_) => get_model_asset(pool, id).await?.ok_or_else(|| {
                ModelRegistryError::Other("model asset vanished after insert".into())
            }),
            Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => {
                // Lost a race with a concurrent registration of the same
                // content; reuse the row it inserted.
                find_by_sha256(pool, &sha256).await?.ok_or_else(|| {
                    ModelRegistryError::Other(
                        "unique violation on model insert but no matching row found".into(),
                    )
                })
            }
            Err(err) => Err(err.into()),
        }
    }

    async fn verify(
        &self,
        pool: &SqlitePool,
        asset: &ModelAsset,
    ) -> Result<ModelAsset, ModelRegistryError> {
        let external_path = asset.external_path.as_deref().ok_or_else(|| {
            ModelRegistryError::Other("external verification requires an external_path".into())
        })?;
        let path = Path::new(external_path);

        let metadata = match tokio::fs::metadata(path).await {
            Ok(metadata) if metadata.is_file() => metadata,
            _ => return Self::mark_unavailable(pool, asset).await,
        };

        let size_bytes = metadata.len() as i64;
        let modified_at = system_time_to_utc(metadata.modified()?);

        let unchanged = asset.available
            && asset.size_bytes == size_bytes
            && asset.file_modified_at == Some(modified_at);
        if unchanged {
            return Ok(asset.clone());
        }

        Self::rehash_and_reconcile(pool, asset, path, size_bytes, modified_at).await
    }

    async fn verify_full(
        &self,
        pool: &SqlitePool,
        asset: &ModelAsset,
    ) -> Result<ModelAsset, ModelRegistryError> {
        let external_path = asset.external_path.as_deref().ok_or_else(|| {
            ModelRegistryError::Other("external verification requires an external_path".into())
        })?;
        let path = Path::new(external_path);

        let metadata = match tokio::fs::metadata(path).await {
            Ok(metadata) if metadata.is_file() => metadata,
            _ => return Self::mark_unavailable(pool, asset).await,
        };
        let size_bytes = metadata.len() as i64;
        let modified_at = system_time_to_utc(metadata.modified()?);

        Self::rehash_and_reconcile(pool, asset, path, size_bytes, modified_at).await
    }

    fn resolve_content_path(&self, asset: &ModelAsset) -> PathBuf {
        PathBuf::from(asset.external_path.as_deref().unwrap_or_default())
    }
}

/// Deferred: a future managed mode would copy a model once into
/// `models/sha256/<prefix>/<sha256>` beneath the configured model root,
/// deduplicating by checksum exactly like `artifact_store::store_artifact`.
/// Not implemented in this change - see the module-level docs above and
/// `design.md` - "`model_assets` and `ModelStorage` trait: external
/// implemented, managed abstracted".
#[allow(dead_code)]
pub struct ManagedModelStorage {
    pub model_root: PathBuf,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    async fn test_pool() -> (SqlitePool, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let db_path = dir.path().join("benchmarks.sqlite3");
        let database_url = format!("sqlite://{}", db_path.display());
        let pool = db::connect_and_migrate(&database_url)
            .await
            .expect("failed to connect and migrate");
        (pool, dir)
    }

    fn write_model_file(dir: &Path, name: &str, contents: &[u8]) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, contents).expect("failed to write model file");
        path
    }

    #[tokio::test]
    async fn registering_an_external_model_never_copies_it() {
        let (pool, dir) = test_pool().await;
        let model_root = dir.path().join("models");
        let source = write_model_file(dir.path(), "model.pte", &[7u8; 4096]);

        let asset = ExternalModelStorage
            .register(&pool, &source)
            .await
            .expect("registration should succeed");

        assert_eq!(asset.storage_mode, ModelStorageMode::External);
        assert_eq!(
            asset.external_path.as_deref(),
            Some(source.to_str().unwrap())
        );
        assert_eq!(asset.size_bytes, 4096);
        assert!(
            !model_root.exists(),
            "registration must not create a managed copy"
        );
    }

    #[tokio::test]
    async fn registering_the_same_model_twice_reuses_one_record() {
        let (pool, dir) = test_pool().await;
        let source = write_model_file(dir.path(), "model.pte", b"identical model bytes");

        let first = ExternalModelStorage.register(&pool, &source).await.unwrap();
        let second = ExternalModelStorage.register(&pool, &source).await.unwrap();
        assert_eq!(first.id, second.id);

        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM model_assets")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn registering_a_missing_path_is_rejected() {
        let (pool, dir) = test_pool().await;
        let missing = dir.path().join("does-not-exist.pte");
        let err = ExternalModelStorage
            .register(&pool, &missing)
            .await
            .unwrap_err();
        assert!(matches!(err, ModelRegistryError::NotARegularFile(_)));
    }

    #[tokio::test]
    async fn verifying_an_unchanged_model_reuses_the_cached_checksum() {
        let (pool, dir) = test_pool().await;
        let source = write_model_file(dir.path(), "model.pte", b"stable content");
        let asset = ExternalModelStorage.register(&pool, &source).await.unwrap();

        let verified = ExternalModelStorage.verify(&pool, &asset).await.unwrap();
        assert_eq!(verified.sha256, asset.sha256);
        assert!(verified.available);
        // Cached verification does not advance last_verified_at, since it
        // never re-reads the file content.
        assert_eq!(verified.last_verified_at, asset.last_verified_at);
    }

    #[tokio::test]
    async fn a_resized_model_is_detected_and_rehashed() {
        let (pool, dir) = test_pool().await;
        let source = write_model_file(dir.path(), "model.pte", b"original content");
        let asset = ExternalModelStorage.register(&pool, &source).await.unwrap();

        std::fs::write(&source, b"changed content, different size!!").unwrap();

        let verified = ExternalModelStorage.verify(&pool, &asset).await.unwrap();
        assert_ne!(verified.sha256, asset.sha256);
        assert!(verified.available);
    }

    #[tokio::test]
    async fn a_moved_or_deleted_model_is_marked_unavailable() {
        let (pool, dir) = test_pool().await;
        let source = write_model_file(dir.path(), "model.pte", b"will be deleted");
        let asset = ExternalModelStorage.register(&pool, &source).await.unwrap();

        std::fs::remove_file(&source).unwrap();

        let verified = ExternalModelStorage.verify(&pool, &asset).await.unwrap();
        assert!(!verified.available);
        assert_eq!(
            verified.sha256, asset.sha256,
            "identity is not silently reassigned"
        );
    }

    #[tokio::test]
    async fn full_verification_always_rehashes_and_updates_last_verified_at() {
        let (pool, dir) = test_pool().await;
        let source = write_model_file(dir.path(), "model.pte", b"content for full verify");
        let asset = ExternalModelStorage.register(&pool, &source).await.unwrap();

        // Sleep so a millisecond-resolution timestamp is guaranteed to
        // advance.
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        let verified = ExternalModelStorage
            .verify_full(&pool, &asset)
            .await
            .unwrap();
        assert_eq!(verified.sha256, asset.sha256);
        assert!(verified.last_verified_at.unwrap() > asset.last_verified_at.unwrap());
    }
}
