//! `POST /api/v1/models/register`, `GET /api/v1/models`,
//! `GET /api/v1/models/{id}`, `POST /api/v1/models/{id}/verify`. See
//! `specs/ingestion-service` - "Service exposes model registration and
//! verification".

use crate::api_error::{ApiError, ApiErrorResponse};
use crate::domain::Sha256Hex;
use crate::events::{Event, ModelRegisteredEvent};
use crate::http::AppState;
use crate::model_registry::{
    ExternalModelStorage, ModelAsset, ModelRegistryError, ModelStorage, ModelStorageMode,
};
use axum::Json;
use crate::config::Config;
use crate::extract::{Json as JsonBody, Path as PathParam, Query};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(register_model))
        .routes(routes!(list_models))
        .routes(routes!(get_model))
        .routes(routes!(verify_model))
}

fn registry_error_response(err: ModelRegistryError) -> Response {
    match err {
        ModelRegistryError::NotARegularFile(path) => {
            ApiError::invalid_request(format!("not a regular file: {}", path.display()))
                .into_response()
        }
        ModelRegistryError::Other(msg) => ApiError::invalid_request(msg).into_response(),
        ModelRegistryError::Io(_) | ModelRegistryError::Db(_) => {
            ApiError::internal("internal storage error").into_response()
        }
    }
}

/// A registered model asset's metadata.
#[derive(Serialize, ToSchema)]
struct ModelAssetResponse {
    id: Uuid,
    /// Lowercase 64-character hexadecimal SHA-256 digest of the model file.
    #[schema(example = "b5bb9d8014a0f9b1d61e21e796d78dccdf1352f23cd32812f4850b878ae4944")]
    sha256: String,
    original_name: String,
    size_bytes: i64,
    model_format: String,
    storage_mode: ModelStorageMode,
    /// The server-side filesystem path this external model was registered
    /// from. `null` for managed-mode models. Currently returned to every
    /// caller as-is - no authentication exists in this service to restrict
    /// it to authorized clients.
    external_path: Option<String>,
    registered_at: chrono::DateTime<chrono::Utc>,
    /// Absent until the model has been explicitly verified at least once.
    last_verified_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Whether the registered file currently resolves on disk.
    available: bool,
}

impl From<&ModelAsset> for ModelAssetResponse {
    fn from(asset: &ModelAsset) -> Self {
        ModelAssetResponse {
            id: asset.id,
            sha256: asset.sha256.clone(),
            original_name: asset.original_name.clone(),
            size_bytes: asset.size_bytes,
            model_format: asset.model_format.clone(),
            storage_mode: asset.storage_mode,
            external_path: asset.external_path.clone(),
            registered_at: asset.registered_at,
            last_verified_at: asset.last_verified_at,
            available: asset.available,
        }
    }
}

#[derive(Deserialize, ToSchema)]
struct RegisterModelRequest {
    /// Absolute path to an existing `.pte` file on the server's filesystem,
    /// beneath one of the server's registrable model roots
    /// (`MODEL_REGISTER_ROOTS`, default the model root). Symlinks are
    /// resolved before the root check, so neither `..` nor a symlink can
    /// escape a root. External mode never copies this file - see
    /// `specs/artifact-storage` - "External model assets are registered
    /// once without copying". Registering the same file's SHA-256 again
    /// reuses the cached checksum rather than rehashing or copying it.
    #[schema(example = "/data/models/llama-3-8b-instruct.pte")]
    path: String,
}

/// Confines registration to `.pte` files beneath a configured registrable
/// root. Without this, an unauthenticated caller could use registration as
/// a file-existence-and-hash oracle over the whole server filesystem and
/// make the server hash arbitrarily large files. Returns the resolved path.
async fn validate_register_path(config: &Config, raw: &str) -> Result<PathBuf, ApiError> {
    let path = Path::new(raw);
    if !path.is_absolute() {
        return Err(ApiError::invalid_field(
            "path",
            "path: must be an absolute path on the server",
        ));
    }
    if path.extension().and_then(|ext| ext.to_str()) != Some("pte") {
        return Err(ApiError::invalid_field("path", "path: must name a .pte file"));
    }
    let resolved = tokio::fs::canonicalize(path).await.map_err(|_| {
        ApiError::invalid_field(
            "path",
            format!("path: {raw} does not exist or is not readable by the server"),
        )
    })?;
    if !resolved.is_file() {
        return Err(ApiError::invalid_field(
            "path",
            format!("path: {raw} is not a regular file"),
        ));
    }
    for root in &config.model_register_roots {
        if let Ok(root) = tokio::fs::canonicalize(root).await
            && resolved.starts_with(&root)
        {
            return Ok(resolved);
        }
    }
    Err(ApiError::invalid_field(
        "path",
        "path: must be beneath one of the server's registrable model roots (MODEL_REGISTER_ROOTS)",
    ))
}

/// Register an external `.pte` model file by path, without copying it.
/// Registering a file whose SHA-256 already matches a registered model
/// reuses the cached checksum and existing record rather than rehashing or
/// duplicating it.
#[utoipa::path(
    post,
    path = "/api/v1/models/register",
    operation_id = "registerModel",
    tag = "models",
    request_body = RegisterModelRequest,
    responses(
        (status = 201, description = "The model was registered, or an existing record with the same SHA-256 was reused.", body = ModelAssetResponse),
        (status = 400, description = "The body is not valid JSON, or the path is not absolute, does not name a `.pte` file, does not exist, is not a regular file, or lies outside every registrable model root. `details.field` is `path`.", body = ApiErrorResponse),
        (status = 500, description = "Internal storage error.", body = ApiErrorResponse),
    )
)]
async fn register_model(
    State(state): State<AppState>,
    JsonBody(req): JsonBody<RegisterModelRequest>,
) -> Response {
    let path = match validate_register_path(&state.config, &req.path).await {
        Ok(path) => path,
        Err(err) => return err.into_response(),
    };
    match ExternalModelStorage.register(&state.pool, &path).await {
        Ok(asset) => {
            state.events.publish(Event::ModelRegistered(ModelRegisteredEvent {
                id: asset.id,
                original_name: asset.original_name.clone(),
                sha256: asset.sha256.clone(),
            }));
            (StatusCode::CREATED, Json(ModelAssetResponse::from(&asset))).into_response()
        }
        Err(err) => registry_error_response(err),
    }
}

/// Query parameters for `GET /api/v1/models`.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
struct ListModelsParams {
    /// Return only the model asset with this content hash (lowercase
    /// 64-character hexadecimal SHA-256), so a collector that knows a
    /// model file's hash can find its asset ID. An empty list when no
    /// asset matches.
    sha256: Option<String>,
}

/// List registered model assets, optionally narrowed to one content hash.
/// Not paginated - matches the current implementation's real, unpaginated
/// response shape. See `specs/ingestion-service` - "Service exposes model
/// lookup by content hash".
#[utoipa::path(
    get,
    path = "/api/v1/models",
    operation_id = "listModels",
    tag = "models",
    params(ListModelsParams),
    responses(
        (status = 200, description = "The matching model assets (all of them without a filter).", body = Vec<ModelAssetResponse>),
        (status = 400, description = "`sha256` is not a well-formed digest.", body = ApiErrorResponse),
        (status = 500, description = "Internal storage error.", body = ApiErrorResponse),
    )
)]
async fn list_models(
    State(state): State<AppState>,
    Query(params): Query<ListModelsParams>,
) -> Response {
    let result = match params.sha256 {
        Some(raw) => match Sha256Hex::try_from(raw) {
            Ok(sha256) => crate::model_registry::find_by_sha256(&state.pool, sha256.as_str())
                .await
                .map(|asset| asset.into_iter().collect::<Vec<_>>()),
            Err(err) => {
                return ApiError::invalid_field("sha256", err.to_string()).into_response();
            }
        },
        None => crate::model_registry::list_model_assets(&state.pool).await,
    };
    match result {
        Ok(assets) => {
            let body: Vec<ModelAssetResponse> =
                assets.iter().map(ModelAssetResponse::from).collect();
            Json(body).into_response()
        }
        Err(err) => registry_error_response(err),
    }
}

/// Fetch a single registered model asset by ID.
#[utoipa::path(
    get,
    path = "/api/v1/models/{id}",
    operation_id = "getModel",
    tag = "models",
    params(("id" = Uuid, Path, description = "The model asset's identifier.")),
    responses(
        (status = 200, description = "The model asset.", body = ModelAssetResponse),
        (status = 404, description = "No model asset exists with this ID.", body = ApiErrorResponse),
    )
)]
async fn get_model(State(state): State<AppState>, PathParam(id): PathParam<Uuid>) -> Response {
    match crate::model_registry::get_model_asset(&state.pool, id).await {
        Ok(Some(asset)) => Json(ModelAssetResponse::from(&asset)).into_response(),
        Ok(None) => ApiError::not_found("model asset not found").into_response(),
        Err(err) => registry_error_response(err),
    }
}

/// Trigger explicit full verification of a model's checksum: recalculates
/// its SHA-256 from the current file content, updates its last-verified
/// timestamp, and reports whether it still matches the registered value.
/// Only implemented for external-mode models.
#[utoipa::path(
    post,
    path = "/api/v1/models/{id}/verify",
    operation_id = "verifyModel",
    tag = "models",
    params(("id" = Uuid, Path, description = "The model asset's identifier.")),
    responses(
        (status = 200, description = "Verification completed; the asset's cached checksum, availability, and last-verified timestamp were refreshed.", body = ModelAssetResponse),
        (status = 404, description = "No model asset exists with this ID.", body = ApiErrorResponse),
        (status = 501, description = "Verification for managed-mode models is not yet implemented.", body = ApiErrorResponse),
    )
)]
async fn verify_model(State(state): State<AppState>, PathParam(id): PathParam<Uuid>) -> Response {
    let asset = match crate::model_registry::get_model_asset(&state.pool, id).await {
        Ok(Some(asset)) => asset,
        Ok(None) => return ApiError::not_found("model asset not found").into_response(),
        Err(err) => return registry_error_response(err),
    };

    if asset.storage_mode != ModelStorageMode::External {
        // Managed mode is defined but not implemented in this change - see
        // `design.md` - "`model_assets` and `ModelStorage` trait: external
        // implemented, managed abstracted".
        return ApiError::not_implemented(
            "verification for managed-mode models is not yet implemented",
        )
        .into_response();
    }

    match ExternalModelStorage.verify_full(&state.pool, &asset).await {
        Ok(verified) => Json(ModelAssetResponse::from(&verified)).into_response(),
        Err(err) => registry_error_response(err),
    }
}
