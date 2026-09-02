//! `GET /api/v1/results`. See `specs/ingestion-service` - "Service exposes
//! grouped benchmark results".

use crate::api_error::{ApiError, ApiErrorResponse};
use crate::domain::{DeviceClass, Platform, Sha256Hex};
use crate::http::AppState;
use crate::results::{self, Facets, MetricStats, ResultRow, ResultsFilter};
use crate::extract::Query;
use axum::Json;
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(get_results_handler))
}

/// Query parameters for `GET /api/v1/results`. Every filter is an exact
/// match; set filters combine conjunctively. Outcome filters are
/// deliberately absent: every run of a configuration contributes to its
/// row as a statistic or a count.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
struct ResultsParams {
    /// One of the stable `Platform` values.
    #[param(value_type = Option<Platform>)]
    platform: Option<String>,
    /// One of the stable `DeviceClass` values.
    #[param(value_type = Option<DeviceClass>)]
    device_class: Option<String>,
    /// Android: device serial. Linux: hostname.
    device_serial: Option<String>,
    model_asset_id: Option<Uuid>,
    git_commit_sha: Option<String>,
    git_branch: Option<String>,
    git_dirty: Option<bool>,
    sumd_driver_version: Option<String>,
    bsp_version: Option<String>,
    /// The accelerator as the runtime backend reports it.
    host_accelerator: Option<String>,
    /// Lowercase 64-character hexadecimal SHA-256 digest of the prompt.
    prompt_sha256: Option<String>,
}

/// Median, minimum, maximum, and sample count of one throughput metric,
/// in tokens per second, over succeeded runs.
#[derive(Serialize, ToSchema)]
struct MetricStatsResponse {
    /// Median in tokens per second.
    median: f64,
    /// Minimum in tokens per second.
    min: f64,
    /// Maximum in tokens per second.
    max: f64,
    /// Number of succeeded runs the statistic was computed over.
    n: u64,
}

impl From<&MetricStats> for MetricStatsResponse {
    fn from(s: &MetricStats) -> Self {
        MetricStatsResponse {
            median: s.median,
            min: s.min,
            max: s.max,
            n: s.n,
        }
    }
}

/// The model a configuration exercised.
#[derive(Serialize, ToSchema)]
struct ResultModelRef {
    id: Uuid,
    /// The registered `.pte` file's name.
    original_name: String,
}

/// One benchmark configuration and its statistics over all its runs.
#[derive(Serialize, ToSchema)]
struct ResultRowResponse {
    // Configuration key
    platform: Platform,
    /// `internal`: lab device. `external`: retail phone or Linux box.
    device_class: DeviceClass,
    /// Android: device serial. Linux: hostname.
    device_serial: String,
    /// Product/model name as the host reports it. Null when not captured.
    device_model: Option<String>,
    model_asset: ResultModelRef,
    git_commit_sha: String,
    git_dirty: bool,
    /// Lab Android devices only; null elsewhere.
    sumd_driver_version: Option<String>,
    /// Lab Android devices only; null elsewhere.
    bsp_version: Option<String>,
    /// GPU clock in MHz. Lab Android devices only; null elsewhere.
    gpu_clock_mhz: Option<i64>,
    /// MIF (memory interface) clock in MHz. Lab Android devices only; null
    /// elsewhere.
    mif_clock_mhz: Option<i64>,
    /// INT (interconnect) clock in MHz. Lab Android devices only; null
    /// elsewhere.
    int_clock_mhz: Option<i64>,
    /// The accelerator the runtime backend executed on. Null when not
    /// captured.
    host_accelerator: Option<String>,
    /// Lowercase 64-character hexadecimal SHA-256 digest of the prompt.
    #[schema(example = "b5bb9d8014a0f9b1d61e21e796d78dccdf1352f23cd32812f4850b878ae4944")]
    prompt_sha256: String,
    input_token_count: i64,

    // Commit metadata (absent until a collector records it)
    git_branch: Option<String>,
    git_commit_timestamp: Option<DateTime<Utc>>,
    git_commit_subject: Option<String>,

    // Statistics
    /// Prefill throughput statistics in tokens per second over succeeded
    /// runs. Null when no run of this configuration succeeded.
    prefill: Option<MetricStatsResponse>,
    /// Decode throughput statistics in tokens per second over succeeded
    /// runs that recorded a decode measurement. Null when none did.
    decode: Option<MetricStatsResponse>,

    // Counts
    total_runs: u64,
    /// Runs whose exit status is not `succeeded`.
    not_succeeded: u64,
    /// Succeeded runs whose correctness result is `failed`.
    correctness_failed: u64,
    /// Runs that reported thermal throttling.
    throttled: u64,
    first_run_at: DateTime<Utc>,
    last_run_at: DateTime<Utc>,
}

impl From<ResultRow> for ResultRowResponse {
    fn from(r: ResultRow) -> Self {
        ResultRowResponse {
            platform: r.key.platform,
            device_class: r.key.device_class,
            device_serial: r.key.device_serial,
            device_model: r.device_model,
            model_asset: ResultModelRef {
                id: r.key.model_asset_id,
                original_name: r.model_original_name,
            },
            git_commit_sha: r.key.git_commit_sha,
            git_dirty: r.key.git_dirty,
            sumd_driver_version: r.key.sumd_driver_version,
            bsp_version: r.key.bsp_version,
            gpu_clock_mhz: r.key.gpu_clock_mhz,
            mif_clock_mhz: r.key.mif_clock_mhz,
            int_clock_mhz: r.key.int_clock_mhz,
            host_accelerator: r.key.host_accelerator,
            prompt_sha256: r.key.prompt_sha256,
            input_token_count: r.input_token_count,
            git_branch: r.git_branch,
            git_commit_timestamp: r.git_commit_timestamp,
            git_commit_subject: r.git_commit_subject,
            prefill: r.prefill.as_ref().map(Into::into),
            decode: r.decode.as_ref().map(Into::into),
            total_runs: r.total_runs,
            not_succeeded: r.not_succeeded,
            correctness_failed: r.correctness_failed,
            throttled: r.throttled,
            first_run_at: r.first_started_at,
            last_run_at: r.last_started_at,
        }
    }
}

/// Distinct filter values across all runs, ignoring the active filters.
#[derive(Serialize, ToSchema)]
struct FacetsResponse {
    platforms: Vec<Platform>,
    device_classes: Vec<DeviceClass>,
    device_serials: Vec<String>,
    models: Vec<ResultModelRef>,
    git_branches: Vec<String>,
    sumd_driver_versions: Vec<String>,
    bsp_versions: Vec<String>,
    host_accelerators: Vec<String>,
}

impl From<Facets> for FacetsResponse {
    fn from(f: Facets) -> Self {
        FacetsResponse {
            platforms: f.platforms,
            device_classes: f.device_classes,
            device_serials: f.device_serials,
            models: f
                .models
                .into_iter()
                .map(|(id, original_name)| ResultModelRef { id, original_name })
                .collect(),
            git_branches: f.git_branches,
            sumd_driver_versions: f.sumd_driver_versions,
            bsp_versions: f.bsp_versions,
            host_accelerators: f.host_accelerators,
        }
    }
}

/// Grouped results: one row per configuration, newest commit first.
#[derive(Serialize, ToSchema)]
struct ResultsResponse {
    rows: Vec<ResultRowResponse>,
    /// True when more than the maximum number of configurations matched
    /// and only the first 500 are returned.
    truncated: bool,
    facets: FacetsResponse,
}

/// Group runs into benchmark configurations and report each one's
/// median/min/max/n throughput over succeeded runs, plus failure and
/// throttling counts. Rows are ordered by commit timestamp (falling back to
/// first run time) descending. At most 500 rows are returned.
#[utoipa::path(
    get,
    path = "/api/v1/results",
    operation_id = "getResults",
    tag = "runs",
    params(ResultsParams),
    responses(
        (status = 200, description = "Grouped results and filter facets.", body = ResultsResponse),
        (status = 400, description = "An invalid filter value.", body = ApiErrorResponse),
        (status = 500, description = "Internal storage error.", body = ApiErrorResponse),
    )
)]
async fn get_results_handler(
    State(state): State<AppState>,
    Query(params): Query<ResultsParams>,
) -> Response {
    let prompt_sha256 = match params.prompt_sha256.map(Sha256Hex::try_from).transpose() {
        Ok(v) => v,
        Err(err) => return ApiError::invalid_request(err.to_string()).into_response(),
    };
    let platform = match params.platform.as_deref().map(Platform::try_from).transpose() {
        Ok(v) => v,
        Err(err) => return ApiError::invalid_request(err.to_string()).into_response(),
    };
    let device_class = match params.device_class.as_deref().map(DeviceClass::try_from).transpose() {
        Ok(v) => v,
        Err(err) => return ApiError::invalid_request(err.to_string()).into_response(),
    };
    let filter = ResultsFilter {
        platform,
        device_class,
        device_serial: params.device_serial,
        model_asset_id: params.model_asset_id,
        git_commit_sha: params.git_commit_sha,
        git_branch: params.git_branch,
        git_dirty: params.git_dirty,
        sumd_driver_version: params.sumd_driver_version,
        bsp_version: params.bsp_version,
        host_accelerator: params.host_accelerator,
        prompt_sha256,
    };

    let page = match results::results(&state.pool, &filter, results::MAX_ROWS).await {
        Ok(page) => page,
        Err(_) => return ApiError::internal("internal error computing results").into_response(),
    };
    let facets = match results::facets(&state.pool).await {
        Ok(facets) => facets,
        Err(_) => return ApiError::internal("internal error computing facets").into_response(),
    };

    Json(ResultsResponse {
        rows: page.rows.into_iter().map(Into::into).collect(),
        truncated: page.truncated,
        facets: facets.into(),
    })
    .into_response()
}
