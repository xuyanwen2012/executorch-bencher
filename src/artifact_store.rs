use async_compression::tokio::bufread::ZstdDecoder;
use async_compression::tokio::write::ZstdEncoder;
use futures_util::stream;
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::{Row, SqlitePool};
use std::fmt;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::time::{Duration, SystemTime};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use utoipa::ToSchema;
use uuid::Uuid;

const COPY_CHUNK_BYTES: usize = 64 * 1024;

/// The kinds of files the backend manages as content-addressed artifacts.
/// See `specs/benchmark-schema` - "Large inputs and outputs are tracked as
/// content-addressed artifacts".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    Prompt,
    Stdout,
    Stderr,
    Output,
    CrashLog,
    Logcat,
    CorrectnessReport,
}

impl ArtifactKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ArtifactKind::Prompt => "prompt",
            ArtifactKind::Stdout => "stdout",
            ArtifactKind::Stderr => "stderr",
            ArtifactKind::Output => "output",
            ArtifactKind::CrashLog => "crash_log",
            ArtifactKind::Logcat => "logcat",
            ArtifactKind::CorrectnessReport => "correctness_report",
        }
    }

    /// Text-oriented log kinds are stored Zstandard-compressed; `prompt` and
    /// `output` are preserved exactly, uncompressed. See
    /// `specs/artifact-storage` - "Artifact kind determines whether content
    /// is compressed".
    pub fn compresses(self) -> bool {
        matches!(
            self,
            ArtifactKind::Stdout
                | ArtifactKind::Stderr
                | ArtifactKind::CrashLog
                | ArtifactKind::Logcat
                | ArtifactKind::CorrectnessReport
        )
    }
}

impl TryFrom<&str> for ArtifactKind {
    type Error = ArtifactStoreError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "prompt" => Ok(ArtifactKind::Prompt),
            "stdout" => Ok(ArtifactKind::Stdout),
            "stderr" => Ok(ArtifactKind::Stderr),
            "output" => Ok(ArtifactKind::Output),
            "crash_log" => Ok(ArtifactKind::CrashLog),
            "logcat" => Ok(ArtifactKind::Logcat),
            "correctness_report" => Ok(ArtifactKind::CorrectnessReport),
            other => Err(ArtifactStoreError::Other(format!(
                "invalid artifact kind: {other:?}"
            ))),
        }
    }
}

/// How an artifact's stored bytes relate to its recorded content: `None`
/// means the stored file *is* the original content; `Zstd` means the stored
/// file is a Zstandard-compressed encoding of it. The recorded `sha256`
/// always identifies the original, uncompressed content either way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Compression {
    None,
    Zstd,
}

impl Compression {
    pub fn as_str(self) -> &'static str {
        match self {
            Compression::None => "none",
            Compression::Zstd => "zstd",
        }
    }
}

impl TryFrom<&str> for Compression {
    type Error = ArtifactStoreError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "none" => Ok(Compression::None),
            "zstd" => Ok(Compression::Zstd),
            other => Err(ArtifactStoreError::Other(format!(
                "invalid compression mode: {other:?}"
            ))),
        }
    }
}

#[derive(Debug)]
pub enum ArtifactStoreError {
    Io(std::io::Error),
    Db(sqlx::Error),
    /// The artifact's database record exists but its content-addressed file
    /// is absent from disk.
    FileMissing,
    Other(String),
}

impl fmt::Display for ArtifactStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ArtifactStoreError::Io(err) => write!(f, "artifact filesystem error: {err}"),
            ArtifactStoreError::Db(err) => write!(f, "artifact database error: {err}"),
            ArtifactStoreError::FileMissing => {
                write!(f, "artifact record exists but its file is missing on disk")
            }
            ArtifactStoreError::Other(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for ArtifactStoreError {}

impl From<std::io::Error> for ArtifactStoreError {
    fn from(err: std::io::Error) -> Self {
        ArtifactStoreError::Io(err)
    }
}

impl From<sqlx::Error> for ArtifactStoreError {
    fn from(err: sqlx::Error) -> Self {
        ArtifactStoreError::Db(err)
    }
}

/// An artifact's identity and metadata after being stored (or deduplicated
/// against an existing identical artifact). `storage_path` is always
/// relative to the configured artifact root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredArtifact {
    pub id: Uuid,
    pub sha256: String,
    pub size_bytes: i64,
    pub storage_path: String,
    pub compression: &'static str,
}

/// An artifact row as read back from storage, for metadata/viewing purposes.
#[derive(Debug, Clone)]
pub struct ArtifactRecord {
    pub id: Uuid,
    pub sha256: String,
    pub size_bytes: i64,
    pub kind: ArtifactKind,
    pub original_filename: Option<String>,
    pub storage_path: String,
    pub media_type: Option<String>,
    pub compression: Compression,
    pub created_at: String,
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// Derives an artifact's storage path from its content hash alone - never
/// from any caller-supplied field - so no request data can choose an
/// arbitrary destination. See `specs/artifact-storage` - "Backend-managed
/// storage paths cannot be chosen by request data".
fn relative_path_for(sha256_hex: &str) -> PathBuf {
    PathBuf::from("sha256")
        .join(&sha256_hex[0..2])
        .join(sha256_hex)
}

async fn find_existing(
    pool: &SqlitePool,
    sha256_hex: &str,
    size_bytes: i64,
) -> Result<Option<Uuid>, ArtifactStoreError> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT id FROM artifacts WHERE sha256 = ? AND size_bytes = ?")
            .bind(sha256_hex)
            .bind(size_bytes)
            .fetch_optional(pool)
            .await?;
    match row {
        Some((id,)) => Ok(Some(Uuid::parse_str(&id).map_err(|err| {
            ArtifactStoreError::Other(format!("stored artifact id is not a valid UUID: {err}"))
        })?)),
        None => Ok(None),
    }
}

/// Streams `reader` into `writer` in fixed-size chunks, feeding every chunk
/// to a running SHA-256 hasher *before* it reaches `writer` - so the
/// recorded hash always identifies the original bytes read from `reader`,
/// even when `writer` compresses them on the way to disk. Never buffers the
/// complete content in memory.
async fn copy_hashing<R, W>(
    mut reader: R,
    mut writer: W,
) -> Result<(String, u64), ArtifactStoreError>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; COPY_CHUNK_BYTES];
    let mut total: u64 = 0;

    loop {
        let n = reader.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        writer.write_all(&buf[..n]).await?;
        total += n as u64;
    }
    // `shutdown`, not just `flush`: a compressing writer (e.g. `ZstdEncoder`)
    // must write its final frame trailer, which `flush` alone doesn't do.
    writer.shutdown().await?;

    Ok((hex_encode(&hasher.finalize()), total))
}

/// Stores content read from `reader` as a content-addressed artifact under
/// `artifact_root`, streaming it through a temporary file under
/// `temporary_dir` without buffering the complete content in memory.
///
/// Sequence: write to a temp file (compressing on the fly for log-shaped
/// kinds, hashing the original uncompressed bytes as they're read), verify,
/// atomically rename into its final content-addressed location, then insert
/// (or reuse, on a `(sha256, size_bytes)` match) the `artifacts` row - in
/// that order, so no database row is ever created for a file that failed to
/// land at its expected path.
pub async fn store_artifact<R>(
    pool: &SqlitePool,
    artifact_root: &Path,
    temporary_dir: &Path,
    kind: ArtifactKind,
    original_filename: Option<&str>,
    media_type: Option<&str>,
    reader: R,
) -> Result<StoredArtifact, ArtifactStoreError>
where
    R: AsyncRead + Unpin,
{
    tokio::fs::create_dir_all(temporary_dir).await?;
    let tmp_path = temporary_dir.join(Uuid::now_v7().to_string());
    let tmp_file = tokio::fs::File::create(&tmp_path).await?;

    let compress = kind.compresses();
    let (sha256_hex, size_bytes) = if compress {
        copy_hashing(reader, ZstdEncoder::new(tmp_file)).await
    } else {
        copy_hashing(reader, tmp_file).await
    }
    .inspect_err(|_| {
        // Best-effort: don't leave a half-written temp file behind on error.
        let _ = std::fs::remove_file(&tmp_path);
    })?;
    let size_bytes = size_bytes as i64;

    let relative_path = relative_path_for(&sha256_hex);
    let final_path = artifact_root.join(&relative_path);
    if let Some(final_dir) = final_path.parent() {
        tokio::fs::create_dir_all(final_dir).await?;
    }

    if tokio::fs::metadata(&final_path).await.is_ok() {
        // Identical content already stored; drop the redundant temp file.
        tokio::fs::remove_file(&tmp_path).await?;
    } else if let Err(err) = tokio::fs::rename(&tmp_path, &final_path).await {
        // Lost a race with a concurrent writer that just landed the same
        // content-addressed file; if so, reuse it. Otherwise propagate.
        if tokio::fs::metadata(&final_path).await.is_err() {
            return Err(err.into());
        }
        let _ = tokio::fs::remove_file(&tmp_path).await;
    }

    let storage_path = relative_path.to_string_lossy().into_owned();
    let compression = if compress {
        Compression::Zstd
    } else {
        Compression::None
    };

    if let Some(id) = find_existing(pool, &sha256_hex, size_bytes).await? {
        return Ok(StoredArtifact {
            id,
            sha256: sha256_hex,
            size_bytes,
            storage_path,
            compression: compression.as_str(),
        });
    }

    let id = Uuid::now_v7();
    let created_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

    let insert_result = sqlx::query(
        "INSERT INTO artifacts (id, sha256, size_bytes, kind, original_filename, storage_path, media_type, compression, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id.to_string())
    .bind(&sha256_hex)
    .bind(size_bytes)
    .bind(kind.as_str())
    .bind(original_filename)
    .bind(&storage_path)
    .bind(media_type)
    .bind(compression.as_str())
    .bind(&created_at)
    .execute(pool)
    .await;

    match insert_result {
        Ok(_) => Ok(StoredArtifact {
            id,
            sha256: sha256_hex,
            size_bytes,
            storage_path,
            compression: compression.as_str(),
        }),
        Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => {
            // Lost a race with a concurrent writer inserting the same
            // content; reuse the row it inserted instead of erroring.
            let existing_id = find_existing(pool, &sha256_hex, size_bytes)
                .await?
                .ok_or_else(|| {
                    ArtifactStoreError::Other(
                        "unique violation on artifact insert but no matching row found".into(),
                    )
                })?;
            Ok(StoredArtifact {
                id: existing_id,
                sha256: sha256_hex,
                size_bytes,
                storage_path,
                compression: compression.as_str(),
            })
        }
        Err(err) => Err(err.into()),
    }
}

/// Convenience wrapper for callers holding the complete content in memory
/// (small prompts, generated output, tests). Streams the same code path as
/// [`store_artifact`] - no separate, non-streaming implementation.
pub async fn store_artifact_bytes(
    pool: &SqlitePool,
    artifact_root: &Path,
    temporary_dir: &Path,
    kind: ArtifactKind,
    original_filename: Option<&str>,
    media_type: Option<&str>,
    bytes: Vec<u8>,
) -> Result<StoredArtifact, ArtifactStoreError> {
    let byte_stream = Box::pin(stream::once(async move {
        Ok::<_, std::io::Error>(tokio_util::bytes::Bytes::from(bytes))
    }));
    let reader = tokio_util::io::StreamReader::new(byte_stream);
    store_artifact(
        pool,
        artifact_root,
        temporary_dir,
        kind,
        original_filename,
        media_type,
        reader,
    )
    .await
}

fn row_to_record(row: sqlx::sqlite::SqliteRow) -> Result<ArtifactRecord, ArtifactStoreError> {
    let id: String = row.try_get("id")?;
    let kind: String = row.try_get("kind")?;
    let compression: String = row.try_get("compression")?;

    Ok(ArtifactRecord {
        id: Uuid::parse_str(&id)
            .map_err(|err| ArtifactStoreError::Other(format!("invalid artifact id: {err}")))?,
        sha256: row.try_get("sha256")?,
        size_bytes: row.try_get("size_bytes")?,
        kind: ArtifactKind::try_from(kind.as_str())?,
        original_filename: row.try_get("original_filename")?,
        storage_path: row.try_get("storage_path")?,
        media_type: row.try_get("media_type")?,
        compression: Compression::try_from(compression.as_str())?,
        created_at: row.try_get("created_at")?,
    })
}

/// Fetches an artifact's database record. Returns `Ok(None)` when no such
/// artifact is registered; does not check whether its file exists on disk.
pub async fn get_artifact_record(
    pool: &SqlitePool,
    id: Uuid,
) -> Result<Option<ArtifactRecord>, ArtifactStoreError> {
    let row = sqlx::query("SELECT * FROM artifacts WHERE id = ?")
        .bind(id.to_string())
        .fetch_optional(pool)
        .await?;

    row.map(row_to_record).transpose()
}

/// Whether an artifact's content-addressed file is currently present on
/// disk beneath `artifact_root`.
pub async fn artifact_file_exists(artifact_root: &Path, record: &ArtifactRecord) -> bool {
    tokio::fs::metadata(artifact_root.join(&record.storage_path))
        .await
        .is_ok()
}

/// Opens an artifact's content as a stream of the *original, uncompressed*
/// bytes, decompressing transparently and incrementally when the stored
/// file is Zstandard-compressed. Returns [`ArtifactStoreError::FileMissing`]
/// when the database record exists but no file is present at its expected
/// location - never a generic I/O error in that case.
pub async fn open_artifact_content(
    artifact_root: &Path,
    record: &ArtifactRecord,
) -> Result<Pin<Box<dyn AsyncRead + Send>>, ArtifactStoreError> {
    let path = artifact_root.join(&record.storage_path);
    let file = match tokio::fs::File::open(&path).await {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Err(ArtifactStoreError::FileMissing);
        }
        Err(err) => return Err(err.into()),
    };

    match record.compression {
        Compression::None => Ok(Box::pin(file)),
        Compression::Zstd => Ok(Box::pin(ZstdDecoder::new(BufReader::new(file)))),
    }
}

/// Removes temporary-upload files older than `retention` from
/// `temporary_dir`, leaving actively-written (recent) files untouched. Safe
/// to run at startup or as a periodic maintenance operation: a temp file
/// only exists mid-upload or after an interrupted one, and any temp file
/// older than a reasonable upload duration is necessarily abandoned, since a
/// completed ingestion always renames it away.
pub async fn cleanup_abandoned_temp_files(
    temporary_dir: &Path,
    retention: Duration,
) -> Result<usize, ArtifactStoreError> {
    let mut removed = 0usize;
    let mut entries = match tokio::fs::read_dir(temporary_dir).await {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(err) => return Err(err.into()),
    };

    let now = SystemTime::now();
    while let Some(entry) = entries.next_entry().await? {
        let metadata = match entry.metadata().await {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        if !metadata.is_file() {
            continue;
        }
        let modified = match metadata.modified() {
            Ok(modified) => modified,
            Err(_) => continue,
        };
        let age = now.duration_since(modified).unwrap_or(Duration::ZERO);
        if age >= retention && tokio::fs::remove_file(entry.path()).await.is_ok() {
            removed += 1;
        }
    }

    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    async fn test_pool_and_roots() -> (SqlitePool, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let db_path = dir.path().join("benchmarks.sqlite3");
        let database_url = format!("sqlite://{}", db_path.display());
        let pool = db::connect_and_migrate(&database_url)
            .await
            .expect("failed to connect and migrate");
        (pool, dir)
    }

    fn roots(dir: &tempfile::TempDir) -> (PathBuf, PathBuf) {
        (dir.path().join("artifacts"), dir.path().join("temporary"))
    }

    #[tokio::test]
    async fn storing_identical_content_twice_reuses_the_same_artifact() {
        let (pool, dir) = test_pool_and_roots().await;
        let (artifact_root, temporary_dir) = roots(&dir);

        let first = store_artifact_bytes(
            &pool,
            &artifact_root,
            &temporary_dir,
            ArtifactKind::Prompt,
            None,
            None,
            b"hello".to_vec(),
        )
        .await
        .expect("first store should succeed");
        let second = store_artifact_bytes(
            &pool,
            &artifact_root,
            &temporary_dir,
            ArtifactKind::Prompt,
            None,
            None,
            b"hello".to_vec(),
        )
        .await
        .expect("second store should succeed");

        assert_eq!(first.id, second.id);
        assert_eq!(first.storage_path, second.storage_path);

        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM artifacts")
            .fetch_one(&pool)
            .await
            .expect("failed to count artifacts");
        assert_eq!(count, 1);

        let stored_path = artifact_root.join(&first.storage_path);
        assert!(stored_path.is_file());
    }

    #[tokio::test]
    async fn different_content_is_kept_as_distinct_artifacts() {
        let (pool, dir) = test_pool_and_roots().await;
        let (artifact_root, temporary_dir) = roots(&dir);

        let a = store_artifact_bytes(
            &pool,
            &artifact_root,
            &temporary_dir,
            ArtifactKind::Prompt,
            None,
            None,
            b"alpha".to_vec(),
        )
        .await
        .expect("store should succeed");
        let b = store_artifact_bytes(
            &pool,
            &artifact_root,
            &temporary_dir,
            ArtifactKind::Prompt,
            None,
            None,
            b"beta".to_vec(),
        )
        .await
        .expect("store should succeed");

        assert_ne!(a.id, b.id);
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM artifacts")
            .fetch_one(&pool)
            .await
            .expect("failed to count artifacts");
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn storage_path_is_always_relative() {
        let (pool, dir) = test_pool_and_roots().await;
        let (artifact_root, temporary_dir) = roots(&dir);

        let stored = store_artifact_bytes(
            &pool,
            &artifact_root,
            &temporary_dir,
            ArtifactKind::CrashLog,
            None,
            None,
            b"boom".to_vec(),
        )
        .await
        .expect("store should succeed");

        let path = std::path::Path::new(&stored.storage_path);
        assert!(path.is_relative());
        assert!(
            !stored
                .storage_path
                .starts_with(&*artifact_root.to_string_lossy())
        );
    }

    #[tokio::test]
    async fn a_crafted_original_filename_cannot_escape_the_artifact_root() {
        let (pool, dir) = test_pool_and_roots().await;
        let (artifact_root, temporary_dir) = roots(&dir);

        let stored = store_artifact_bytes(
            &pool,
            &artifact_root,
            &temporary_dir,
            ArtifactKind::Prompt,
            Some("../../../../etc/passwd"),
            None,
            b"not actually /etc/passwd".to_vec(),
        )
        .await
        .expect("store should succeed even with a hostile filename");

        let stored_path = artifact_root.join(&stored.storage_path);
        assert!(stored_path.starts_with(&artifact_root));
        assert!(stored_path.is_file());
        // Only one file was ever written: the traversal-laden name was
        // never interpreted as a path, only retained as display metadata.
        let record = get_artifact_record(&pool, stored.id)
            .await
            .expect("lookup should succeed")
            .expect("record should exist");
        assert_eq!(
            record.original_filename.as_deref(),
            Some("../../../../etc/passwd")
        );
        let mut written_files = Vec::new();
        let mut prefix_entries = tokio::fs::read_dir(artifact_root.join("sha256"))
            .await
            .expect("sha256 dir should exist");
        while let Some(prefix_entry) = prefix_entries.next_entry().await.unwrap() {
            let mut inner = tokio::fs::read_dir(prefix_entry.path()).await.unwrap();
            while let Some(file_entry) = inner.next_entry().await.unwrap() {
                written_files.push(file_entry.path());
            }
        }
        assert_eq!(written_files, vec![stored_path]);
    }

    #[tokio::test]
    async fn a_failed_write_never_leaves_a_dangling_database_reference() {
        let (pool, dir) = test_pool_and_roots().await;
        // A path with a regular file as a path component can never be
        // created as a directory, so `create_dir_all` for the final
        // content-addressed location fails deterministically before any
        // database insert is attempted.
        let blocking_file_path = dir.path().join("not-a-directory");
        std::fs::write(&blocking_file_path, b"blocking").expect("failed to write blocking file");
        let artifact_root = blocking_file_path.join("artifacts");
        let temporary_dir = dir.path().join("temporary");

        let result = store_artifact_bytes(
            &pool,
            &artifact_root,
            &temporary_dir,
            ArtifactKind::Stderr,
            None,
            None,
            b"data".to_vec(),
        )
        .await;
        assert!(result.is_err());

        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM artifacts")
            .fetch_one(&pool)
            .await
            .expect("failed to count artifacts");
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn interrupted_upload_leaves_no_artifact_row() {
        let (pool, dir) = test_pool_and_roots().await;
        let (_artifact_root, temporary_dir) = roots(&dir);
        tokio::fs::create_dir_all(&temporary_dir).await.unwrap();

        // Simulate an interrupted upload: a reader that errors partway
        // through, after some bytes have already been written to the temp
        // file.
        struct FlakyReader {
            remaining: Vec<u8>,
            failed: bool,
        }
        impl AsyncRead for FlakyReader {
            fn poll_read(
                mut self: Pin<&mut Self>,
                _cx: &mut std::task::Context<'_>,
                buf: &mut tokio::io::ReadBuf<'_>,
            ) -> std::task::Poll<std::io::Result<()>> {
                if !self.remaining.is_empty() {
                    let take = self.remaining.len().min(buf.remaining());
                    let chunk: Vec<u8> = self.remaining.drain(..take).collect();
                    buf.put_slice(&chunk);
                    return std::task::Poll::Ready(Ok(()));
                }
                if !self.failed {
                    self.failed = true;
                    return std::task::Poll::Ready(Err(std::io::Error::other("simulated failure")));
                }
                std::task::Poll::Ready(Ok(()))
            }
        }

        let reader = FlakyReader {
            remaining: b"partial content".to_vec(),
            failed: false,
        };
        let artifact_root = dir.path().join("artifacts");
        let result = store_artifact(
            &pool,
            &artifact_root,
            &temporary_dir,
            ArtifactKind::Stdout,
            None,
            None,
            reader,
        )
        .await;
        assert!(result.is_err());

        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM artifacts")
            .fetch_one(&pool)
            .await
            .expect("failed to count artifacts");
        assert_eq!(count, 0);

        // No abandoned temp file should remain either.
        let mut entries = tokio::fs::read_dir(&temporary_dir).await.unwrap();
        assert!(entries.next_entry().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn concurrent_ingestion_of_identical_content_converges_on_one_artifact() {
        let (pool, dir) = test_pool_and_roots().await;
        let (artifact_root, temporary_dir) = roots(&dir);

        let tasks: Vec<_> = (0..8)
            .map(|_| {
                let pool = pool.clone();
                let artifact_root = artifact_root.clone();
                let temporary_dir = temporary_dir.clone();
                tokio::spawn(async move {
                    store_artifact_bytes(
                        &pool,
                        &artifact_root,
                        &temporary_dir,
                        ArtifactKind::Logcat,
                        None,
                        None,
                        b"concurrent identical content".to_vec(),
                    )
                    .await
                })
            })
            .collect();

        let mut ids = std::collections::HashSet::new();
        for task in tasks {
            let stored = task.await.unwrap().expect("store should succeed");
            ids.insert(stored.id);
        }
        assert_eq!(
            ids.len(),
            1,
            "all concurrent uploads should resolve to one artifact id"
        );

        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM artifacts")
            .fetch_one(&pool)
            .await
            .expect("failed to count artifacts");
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn a_large_stream_is_ingested_without_full_buffering() {
        let (pool, dir) = test_pool_and_roots().await;
        let (artifact_root, temporary_dir) = roots(&dir);

        // A generated, deterministic 32 MiB source fed in small chunks -
        // large enough to exercise multi-chunk streaming without a real
        // multi-gigabyte fixture.
        const TOTAL: usize = 32 * 1024 * 1024;
        struct Generated {
            remaining: usize,
        }
        impl AsyncRead for Generated {
            fn poll_read(
                mut self: Pin<&mut Self>,
                _cx: &mut std::task::Context<'_>,
                buf: &mut tokio::io::ReadBuf<'_>,
            ) -> std::task::Poll<std::io::Result<()>> {
                if self.remaining == 0 {
                    return std::task::Poll::Ready(Ok(()));
                }
                let take = self.remaining.min(buf.remaining()).min(8192);
                let chunk = vec![b'x'; take];
                buf.put_slice(&chunk);
                self.remaining -= take;
                std::task::Poll::Ready(Ok(()))
            }
        }

        let stored = store_artifact(
            &pool,
            &artifact_root,
            &temporary_dir,
            ArtifactKind::Stdout,
            None,
            None,
            Generated { remaining: TOTAL },
        )
        .await
        .expect("large streamed store should succeed");

        assert_eq!(stored.size_bytes, TOTAL as i64);
        let record = get_artifact_record(&pool, stored.id)
            .await
            .expect("lookup should succeed")
            .expect("record should exist");
        assert_eq!(record.compression, Compression::Zstd);

        let mut content = open_artifact_content(&artifact_root, &record)
            .await
            .expect("content should open");
        let mut buf = Vec::new();
        content
            .read_to_end(&mut buf)
            .await
            .expect("should read decompressed content");
        assert_eq!(buf.len(), TOTAL);
        assert!(buf.iter().all(|&b| b == b'x'));
    }

    #[tokio::test]
    async fn compressed_artifact_identity_matches_its_uncompressed_content() {
        let (pool, dir) = test_pool_and_roots().await;
        let (artifact_root, temporary_dir) = roots(&dir);

        let content = b"repeated repeated repeated log line\n".repeat(100);
        let mut hasher = Sha256::new();
        hasher.update(&content);
        let expected_sha256 = hex_encode(&hasher.finalize());

        let stored = store_artifact_bytes(
            &pool,
            &artifact_root,
            &temporary_dir,
            ArtifactKind::Stdout,
            None,
            Some("text/plain"),
            content.clone(),
        )
        .await
        .expect("store should succeed");

        assert_eq!(stored.sha256, expected_sha256);
        assert_eq!(stored.compression, "zstd");

        let record = get_artifact_record(&pool, stored.id)
            .await
            .expect("lookup should succeed")
            .expect("record should exist");
        assert_eq!(record.compression, Compression::Zstd);
        assert_eq!(record.sha256, expected_sha256);

        // The stored file is smaller than the original repetitive content -
        // proof it was actually compressed, not stored verbatim.
        let stored_path = artifact_root.join(&record.storage_path);
        let on_disk_len = std::fs::metadata(&stored_path).unwrap().len();
        assert!((on_disk_len as usize) < content.len());
    }

    #[tokio::test]
    async fn compressed_content_streams_back_decompressed() {
        let (pool, dir) = test_pool_and_roots().await;
        let (artifact_root, temporary_dir) = roots(&dir);

        let content = b"line one\nline two\nline three\n".repeat(50);
        let stored = store_artifact_bytes(
            &pool,
            &artifact_root,
            &temporary_dir,
            ArtifactKind::CorrectnessReport,
            None,
            None,
            content.clone(),
        )
        .await
        .expect("store should succeed");

        let record = get_artifact_record(&pool, stored.id)
            .await
            .expect("lookup should succeed")
            .expect("record should exist");
        let mut reader = open_artifact_content(&artifact_root, &record)
            .await
            .expect("content should open");
        let mut buf = Vec::new();
        reader
            .read_to_end(&mut buf)
            .await
            .expect("should decompress");
        assert_eq!(buf, content);
    }

    #[tokio::test]
    async fn a_missing_file_is_reported_distinctly_from_other_errors() {
        let (pool, dir) = test_pool_and_roots().await;
        let (artifact_root, temporary_dir) = roots(&dir);

        let stored = store_artifact_bytes(
            &pool,
            &artifact_root,
            &temporary_dir,
            ArtifactKind::Prompt,
            None,
            None,
            b"will be deleted out of band".to_vec(),
        )
        .await
        .expect("store should succeed");
        let record = get_artifact_record(&pool, stored.id)
            .await
            .expect("lookup should succeed")
            .expect("record should exist");

        std::fs::remove_file(artifact_root.join(&record.storage_path))
            .expect("failed to delete stored file out of band");

        let result = open_artifact_content(&artifact_root, &record).await;
        assert!(matches!(result, Err(ArtifactStoreError::FileMissing)));
        assert!(!artifact_file_exists(&artifact_root, &record).await);
    }

    #[tokio::test]
    async fn invalid_kind_strings_are_rejected() {
        assert!(ArtifactKind::try_from("bogus").is_err());
        for kind in [
            "prompt",
            "stdout",
            "stderr",
            "output",
            "crash_log",
            "logcat",
            "correctness_report",
        ] {
            assert!(ArtifactKind::try_from(kind).is_ok());
        }
    }

    #[tokio::test]
    async fn cleanup_removes_only_temp_files_older_than_retention() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let temporary_dir = dir.path().join("temporary");
        tokio::fs::create_dir_all(&temporary_dir).await.unwrap();

        let old_path = temporary_dir.join("abandoned");
        let fresh_path = temporary_dir.join("in-flight");
        std::fs::write(&old_path, b"stale").unwrap();
        std::fs::write(&fresh_path, b"fresh").unwrap();

        // Backdate the "abandoned" file's modified time well past the
        // retention window; leave the "in-flight" one at its natural mtime.
        let old_time = SystemTime::now() - Duration::from_secs(3600);
        filetime_set(&old_path, old_time);

        let removed = cleanup_abandoned_temp_files(&temporary_dir, Duration::from_secs(60))
            .await
            .expect("cleanup should succeed");

        assert_eq!(removed, 1);
        assert!(!old_path.exists());
        assert!(fresh_path.exists());
    }

    /// Minimal mtime-setting helper so the retention test doesn't need an
    /// extra crate: reopens the file for writing (which some platforms bump
    /// mtime for) is not reliable, so we use `std::fs::File::set_times` via
    /// the standard library's `FileTimes` API.
    fn filetime_set(path: &std::path::Path, time: SystemTime) {
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(path)
            .expect("failed to reopen file to set mtime");
        let times = std::fs::FileTimes::new().set_modified(time);
        file.set_times(times).expect("failed to set mtime");
    }
}
