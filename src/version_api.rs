//! `GET /api/v1/version`. See `specs/api-documentation` - "System exposes
//! version and compatibility information".

use crate::http::AppState;
use axum::Json;
use serde::Serialize;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

/// Hand-maintained compatibility constants, bumped manually as the contract
/// evolves. `API_VERSION` is also the generated OpenAPI document's
/// `info.version` (see `openapi.rs`), and `SCHEMA_VERSION` must equal the
/// value the migrations write to `schema_metadata`; `tests/version_consistency.rs`
/// ties the three together so they cannot drift silently. There is no
/// automatic derivation for `minimum_runner_version` - no Python runner
/// exists yet to check compatibility against.
pub const API_VERSION: &str = "1.4";
pub const SCHEMA_VERSION: u32 = 1;
const MINIMUM_RUNNER_VERSION: &str = "0.1.0";

/// API and schema compatibility information.
#[derive(Serialize, ToSchema)]
struct VersionResponse {
    /// The documented OpenAPI contract's version. Bumped by hand when the
    /// contract changes in a way clients should notice.
    #[schema(example = "1.1")]
    api_version: String,
    /// The running server binary's Cargo package version
    /// (`CARGO_PKG_VERSION`), not duplicated by hand.
    #[schema(example = "0.1.0")]
    server_version: String,
    /// The oldest Python benchmark runner version known to be compatible
    /// with this server. Maintained by hand; no runner exists yet to
    /// validate against.
    #[schema(example = "0.1.0")]
    minimum_runner_version: String,
    /// Increments only on a backward-incompatible change to the run/
    /// artifact/model database or contract schema.
    #[schema(example = 1)]
    schema_version: u32,
}

/// Report API and schema compatibility information.
#[utoipa::path(
    get,
    path = "/api/v1/version",
    operation_id = "getVersion",
    tag = "system",
    responses(
        (status = 200, description = "Version and compatibility information.", body = VersionResponse),
    )
)]
async fn get_version() -> Json<VersionResponse> {
    Json(VersionResponse {
        api_version: API_VERSION.to_string(),
        server_version: env!("CARGO_PKG_VERSION").to_string(),
        minimum_runner_version: MINIMUM_RUNNER_VERSION.to_string(),
        schema_version: SCHEMA_VERSION,
    })
}

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(get_version))
}
