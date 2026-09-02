//! `POST /api/v1/artifacts`, `GET /api/v1/artifacts/{id}/metadata`,
//! `GET /api/v1/artifacts/{id}/content`, `GET /api/v1/artifacts/{id}/download`.
//! See `specs/ingestion-service` - "Service exposes artifact upload,
//! metadata, and content retrieval".

use crate::api_error::{ApiError, ApiErrorResponse};
use crate::artifact_store::{
    ArtifactKind, ArtifactRecord, ArtifactStoreError, Compression, artifact_file_exists,
    get_artifact_record, open_artifact_content, store_artifact,
};
use crate::events::{ArtifactCreatedEvent, Event};
use crate::http::AppState;
use crate::extract::{Path as PathParam, Query};
use axum::Json;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use futures_util::TryStreamExt;
use serde::{Deserialize, Serialize};
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, ReadBuf};
use tokio_util::io::{ReaderStream, StreamReader};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(upload_artifact))
        .routes(routes!(artifact_metadata))
        .routes(routes!(artifact_content))
        .routes(routes!(artifact_download))
}

/// OpenAPI-only marker for a raw, unstructured binary body (streamed as
/// `application/octet-stream`, not JSON and not a multipart form).
#[derive(ToSchema)]
#[schema(value_type = String, format = Binary)]
struct BinaryBody(#[allow(dead_code)] Vec<u8>);

/// Marker error so the size-limit failure can be distinguished from any
/// other I/O error further up the call stack (`err.get_ref()` +
/// `is::<UploadSizeLimitExceeded>()`), instead of matching on message text.
#[derive(Debug)]
struct UploadSizeLimitExceeded;

impl std::fmt::Display for UploadSizeLimitExceeded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "artifact upload exceeds the configured maximum size")
    }
}

impl std::error::Error for UploadSizeLimitExceeded {}

fn is_size_limit_error(err: &io::Error) -> bool {
    err.get_ref()
        .is_some_and(|inner| inner.is::<UploadSizeLimitExceeded>())
}

/// Wraps an [`AsyncRead`] and fails once more than `limit` bytes have been
/// read from it, so an upload body can never grow the temp file (or the
/// server's memory, since ingestion streams throughout) without bound.
struct LimitedReader<R> {
    inner: R,
    limit: u64,
    read_so_far: u64,
}

impl<R: AsyncRead + Unpin> AsyncRead for LimitedReader<R> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let before = buf.filled().len();
        let poll = Pin::new(&mut this.inner).poll_read(cx, buf);
        if let Poll::Ready(Ok(())) = &poll {
            let read = buf.filled().len() - before;
            this.read_so_far += read as u64;
            if this.read_so_far > this.limit {
                return Poll::Ready(Err(io::Error::other(UploadSizeLimitExceeded)));
            }
        }
        poll
    }
}

fn store_error_response(err: ArtifactStoreError) -> Response {
    match err {
        ArtifactStoreError::Other(msg) => ApiError::invalid_request(msg).into_response(),
        ArtifactStoreError::FileMissing => {
            ApiError::artifact_file_missing("artifact file is missing on disk").into_response()
        }
        ArtifactStoreError::Io(_) | ArtifactStoreError::Db(_) => {
            ApiError::internal("internal storage error").into_response()
        }
    }
}

#[derive(Deserialize)]
struct UploadParams {
    /// One of the recognized artifact kinds (see `ArtifactKind`). An
    /// unrecognized value is rejected with `invalid_request`.
    kind: String,
    /// Caller-supplied display name, retained only as metadata - never used
    /// to choose a storage path.
    original_name: Option<String>,
}

/// Result of a successful artifact upload.
#[derive(Serialize, ToSchema)]
struct UploadResponse {
    id: Uuid,
    /// Lowercase 64-character hexadecimal SHA-256 digest of the original,
    /// uncompressed content.
    #[schema(example = "b5bb9d8014a0f9b1d61e21e796d78dccdf1352f23cd32812f4850b878ae4944")]
    sha256: String,
    size_bytes: i64,
    /// How the stored bytes relate to the original content. `sha256` and
    /// `size_bytes` always describe the original, uncompressed content
    /// either way.
    #[schema(value_type = Compression)]
    compression: String,
}

/// Stream and store an artifact's content, deduplicating by SHA-256. The
/// body is the raw artifact bytes, streamed directly - **not** a multipart
/// form. `kind` and `original_name` are query parameters.
#[utoipa::path(
    post,
    path = "/api/v1/artifacts",
    operation_id = "uploadArtifact",
    tag = "artifacts",
    params(
        ("kind" = ArtifactKind, Query, description = "The artifact's kind. Determines whether the content is stored Zstandard-compressed (see `ArtifactKind`)."),
        ("original_name" = Option<String>, Query, description = "Caller-supplied display name, retained only as metadata."),
    ),
    request_body(
        content = BinaryBody,
        content_type = "application/octet-stream",
        description = "Raw artifact bytes, streamed directly. Rejected once the stream exceeds the server's configured maximum artifact upload size."
    ),
    responses(
        (status = 201, description = "The artifact was stored, or an identical one (by SHA-256) already existed and was reused.", body = UploadResponse),
        (status = 400, description = "`kind` is not a recognized artifact kind.", body = ApiErrorResponse),
        (status = 413, description = "The upload exceeds the server's configured maximum artifact size.", body = ApiErrorResponse),
        (status = 500, description = "Internal storage error.", body = ApiErrorResponse),
    )
)]
async fn upload_artifact(
    State(state): State<AppState>,
    Query(params): Query<UploadParams>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let kind = match ArtifactKind::try_from(params.kind.as_str()) {
        Ok(kind) => kind,
        Err(_) => {
            return ApiError::invalid_request(format!("invalid artifact kind: {:?}", params.kind))
                .into_response();
        }
    };

    let media_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let stream = body
        .into_data_stream()
        .map_err(|err| io::Error::other(err.to_string()));
    let reader = LimitedReader {
        inner: StreamReader::new(stream),
        limit: state.config.limits.max_artifact_upload_bytes,
        read_so_far: 0,
    };

    match store_artifact(
        &state.pool,
        &state.config.artifact_root,
        &state.config.temporary_dir,
        kind,
        params.original_name.as_deref(),
        media_type.as_deref(),
        reader,
    )
    .await
    {
        Ok(stored) => {
            state.events.publish(Event::ArtifactCreated(ArtifactCreatedEvent {
                id: stored.id,
                kind,
                sha256: stored.sha256.clone(),
                size_bytes: stored.size_bytes,
            }));
            (
                StatusCode::CREATED,
                Json(UploadResponse {
                    id: stored.id,
                    sha256: stored.sha256,
                    size_bytes: stored.size_bytes,
                    compression: stored.compression.to_string(),
                }),
            )
                .into_response()
        }
        Err(ArtifactStoreError::Io(err)) if is_size_limit_error(&err) => {
            ApiError::payload_too_large("artifact exceeds the maximum upload size").into_response()
        }
        Err(err) => store_error_response(err),
    }
}

/// An artifact's full metadata, including where to fetch its content.
#[derive(Serialize, ToSchema)]
struct ArtifactMetadataResponse {
    id: Uuid,
    kind: ArtifactKind,
    original_filename: Option<String>,
    size_bytes: i64,
    media_type: Option<String>,
    compression: Compression,
    /// Lowercase 64-character hexadecimal SHA-256 digest of the original,
    /// uncompressed content.
    #[schema(example = "b5bb9d8014a0f9b1d61e21e796d78dccdf1352f23cd32812f4850b878ae4944")]
    sha256: String,
    created_at: String,
    /// Whether the content-addressed file still exists on disk. `false`
    /// means the database record exists but the file is unavailable - see
    /// the `artifact_file_missing` error on `content`/`download`.
    available: bool,
    content_url: String,
    download_url: String,
}

async fn build_metadata_response(
    state: &AppState,
    record: &ArtifactRecord,
) -> ArtifactMetadataResponse {
    let available = artifact_file_exists(&state.config.artifact_root, record).await;
    ArtifactMetadataResponse {
        id: record.id,
        kind: record.kind,
        original_filename: record.original_filename.clone(),
        size_bytes: record.size_bytes,
        media_type: record.media_type.clone(),
        compression: record.compression,
        sha256: record.sha256.clone(),
        created_at: record.created_at.clone(),
        available,
        content_url: format!("/api/v1/artifacts/{}/content", record.id),
        download_url: format!("/api/v1/artifacts/{}/download", record.id),
    }
}

/// Fetch an artifact's metadata, including whether its content-addressed
/// file is currently available on disk.
#[utoipa::path(
    get,
    path = "/api/v1/artifacts/{id}/metadata",
    operation_id = "getArtifactMetadata",
    tag = "artifacts",
    params(("id" = Uuid, Path, description = "The artifact's identifier.")),
    responses(
        (status = 200, description = "The artifact's metadata.", body = ArtifactMetadataResponse),
        (status = 404, description = "No artifact exists with this ID.", body = ApiErrorResponse),
    )
)]
async fn artifact_metadata(
    State(state): State<AppState>,
    PathParam(id): PathParam<Uuid>,
) -> Response {
    match get_artifact_record(&state.pool, id).await {
        Ok(Some(record)) => Json(build_metadata_response(&state, &record).await).into_response(),
        Ok(None) => ApiError::not_found("artifact not found").into_response(),
        Err(err) => store_error_response(err),
    }
}

/// The last path component of the stored `original_filename`, discarding
/// any directory components - so a name that was never used to choose a
/// filesystem path (see `artifact_store`) can't be turned into a
/// path-traversing `Content-Disposition` filename either. Falls back to a
/// kind-and-hash name when no usable name was recorded.
fn download_basename(record: &ArtifactRecord) -> String {
    record
        .original_filename
        .as_deref()
        .and_then(|name| std::path::Path::new(name).file_name())
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(|name| name.to_string())
        .unwrap_or_else(|| format!("{}-{}.bin", record.kind.as_str(), &record.sha256[..12]))
}

/// Restricts a filename to what is safe inside a quoted `filename="..."`
/// parameter: printable ASCII minus the quote, backslash, and semicolon.
/// Anything else (non-ASCII, control characters) becomes `_`. The header
/// value is therefore always valid, whatever name was uploaded.
fn ascii_filename(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .map(|c| {
            if (c.is_ascii_graphic() || c == ' ') && !matches!(c, '"' | '\\' | ';') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if sanitized.trim_matches('_').trim().is_empty() {
        "download.bin".to_string()
    } else {
        sanitized
    }
}

/// RFC 5987 percent-encoding for the `filename*` parameter: unreserved
/// characters pass through, everything else is `%XX` per UTF-8 byte.
fn percent_encode_utf8(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for byte in name.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// Builds the `Content-Disposition` value for a download: an ASCII-safe
/// `filename` always, plus an RFC 5987 `filename*` carrying the exact
/// original name when it was not plain ASCII.
fn content_disposition(record: &ArtifactRecord) -> String {
    let original = download_basename(record);
    let ascii = ascii_filename(&original);
    if ascii == original {
        format!("attachment; filename=\"{ascii}\"")
    } else {
        format!(
            "attachment; filename=\"{ascii}\"; filename*=UTF-8''{}",
            percent_encode_utf8(&original)
        )
    }
}

async fn stream_artifact_content(state: &AppState, id: Uuid, attachment: bool) -> Response {
    let record = match get_artifact_record(&state.pool, id).await {
        Ok(Some(record)) => record,
        Ok(None) => return ApiError::not_found("artifact not found").into_response(),
        Err(err) => return store_error_response(err),
    };

    let reader = match open_artifact_content(&state.config.artifact_root, &record).await {
        Ok(reader) => reader,
        Err(err) => return store_error_response(err),
    };

    let content_type = record
        .media_type
        .clone()
        .unwrap_or_else(|| "application/octet-stream".to_string());

    let mut response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type);

    if attachment {
        response = response.header(header::CONTENT_DISPOSITION, content_disposition(&record));
    }

    match response.body(Body::from_stream(ReaderStream::new(reader))) {
        Ok(response) => response.into_response(),
        // Every header above is built from validated or sanitized input,
        // so this is unreachable; never panic on a request path regardless.
        Err(_) => ApiError::internal("internal error building artifact response").into_response(),
    }
}

/// Stream an artifact's content, decompressing transparently if it is
/// stored Zstandard-compressed. The response body is the raw content with
/// its stored media type - never a JSON-encoded string.
#[utoipa::path(
    get,
    path = "/api/v1/artifacts/{id}/content",
    operation_id = "getArtifactContent",
    tag = "artifacts",
    params(("id" = Uuid, Path, description = "The artifact's identifier.")),
    responses(
        (status = 200, description = "The artifact's content, streamed with its stored media type (or `application/octet-stream` if none was recorded).", content_type = "application/octet-stream", body = BinaryBody),
        (status = 404, description = "No artifact exists with this ID.", body = ApiErrorResponse),
        (status = 410, description = "The database record exists but the file is missing on disk.", body = ApiErrorResponse),
    )
)]
async fn artifact_content(
    State(state): State<AppState>,
    PathParam(id): PathParam<Uuid>,
) -> Response {
    stream_artifact_content(&state, id, false).await
}

/// Like `content`, but sets `Content-Disposition: attachment` with a safe
/// filename derived from the artifact's kind and content hash.
#[utoipa::path(
    get,
    path = "/api/v1/artifacts/{id}/download",
    operation_id = "downloadArtifact",
    tag = "artifacts",
    params(("id" = Uuid, Path, description = "The artifact's identifier.")),
    responses(
        (status = 200, description = "The artifact's content as an attachment download.", content_type = "application/octet-stream", body = BinaryBody),
        (status = 404, description = "No artifact exists with this ID.", body = ApiErrorResponse),
        (status = 410, description = "The database record exists but the file is missing on disk.", body = ApiErrorResponse),
    )
)]
async fn artifact_download(
    State(state): State<AppState>,
    PathParam(id): PathParam<Uuid>,
) -> Response {
    stream_artifact_content(&state, id, true).await
}
