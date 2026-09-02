//! Top-level OpenAPI document metadata (title, description, version, tags).
//! See `specs/api-documentation` - "System exposes a generated OpenAPI
//! document".
//!
//! No `license` or `servers` block is configured: `Cargo.toml` declares no
//! license, and no deployment/base-path configuration exists to derive a
//! `servers` entry from correctly (see the change's design notes). The
//! `system`, `runs`, `models`, `artifacts`, and `events` tags are declared;
//! `analysis` is omitted since no operation uses it yet. The event payload
//! schemas are registered explicitly because the stream operation can only
//! reference them from its description (OpenAPI has no SSE construct).

use utoipa::OpenApi;
use utoipa::openapi::OpenApi as OpenApiDoc;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "ExecuTorch Bencher API",
        description = "Internal LLM benchmark collection and analysis service for Android \
                        phones and Linux hosts. Provides HTTP endpoints to ingest, store, and \
                        retrieve benchmark run results, content-addressed artifacts, and \
                        registered model assets. The service has no authentication and is \
                        meant for a trusted lab network.",
        version = "1.0"
    ),
    tags(
        (name = "system", description = "Service health, version, and API documentation."),
        (name = "runs", description = "Benchmark run submission and retrieval."),
        (name = "models", description = "Model asset registration and verification."),
        (name = "artifacts", description = "Content-addressed artifact upload and retrieval."),
        (name = "events", description = "Live change notifications as Server-Sent Events."),
    ),
    components(schemas(
        crate::events::RunCreatedEvent,
        crate::events::ArtifactCreatedEvent,
        crate::events::ModelRegisteredEvent,
    ))
)]
struct ApiDoc;

/// Builds the OpenAPI document's static metadata, with the Cargo-derived
/// `license` field cleared regardless of `CARGO_PKG_LICENSE` (Cargo sets
/// that env var to an empty string, not unset, when `Cargo.toml` has no
/// `license` field, which utoipa would otherwise render as an empty
/// license object). `info.version` is the same `API_VERSION` that
/// `GET /api/v1/version` reports, so the two cannot disagree.
pub fn base_document() -> OpenApiDoc {
    let mut doc = ApiDoc::openapi();
    doc.info.license = None;
    doc.info.version = crate::version_api::API_VERSION.to_string();
    doc
}
