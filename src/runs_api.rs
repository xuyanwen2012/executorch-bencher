//! `GET /api/v1/runs` and `GET /api/v1/runs/{id}`. See
//! `specs/ingestion-service` - "Service exposes a paginated, filterable
//! run listing", "Single-run responses expose the complete recorded run",
//! and "Run responses expose viewable artifact metadata".

use crate::api_error::{ApiError, ApiErrorResponse};
use crate::artifact_store::{
    ArtifactKind, ArtifactRecord, Compression, artifact_file_exists, get_artifact_record,
};
use crate::domain::{CorrectnessResult, DeviceClass, ExitStatus, Platform, Sha256Hex};
use crate::http::AppState;
use crate::model_registry::get_model_asset;
use crate::runs::{Run, RunCursor, RunListFilter, RunSummary, get_run, list_runs};
use crate::extract::{Path as PathParam, Query};
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
    OpenApiRouter::new()
        .routes(routes!(list_runs_handler))
        .routes(routes!(get_run_handler))
}

const DEFAULT_LIMIT: u32 = 50;
const MAX_LIMIT: u32 = 200;

/// Query parameters for `GET /api/v1/runs`. Every filter is an exact match;
/// set filters combine conjunctively. Together the configuration filters
/// (device through prompt) identify one benchmark configuration, so a
/// results row can link to exactly the runs that produced it.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
struct ListRunsParams {
    /// Page size. Default 50, maximum 200.
    #[param(minimum = 1, maximum = 200, example = 50)]
    limit: Option<u32>,
    /// Opaque cursor from a previous response's `next_cursor`.
    cursor: Option<String>,
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
    /// GPU clock in MHz.
    gpu_clock_mhz: Option<i64>,
    /// MIF clock in MHz.
    mif_clock_mhz: Option<i64>,
    /// INT clock in MHz.
    int_clock_mhz: Option<i64>,
    /// The accelerator as the runtime backend reports it (Vulkan device
    /// name on Linux, GPU on Android).
    host_accelerator: Option<String>,
    /// Lowercase 64-character hexadecimal SHA-256 digest of the prompt.
    prompt_sha256: Option<String>,
    /// One of the stable `ExitStatus` values.
    #[param(value_type = Option<ExitStatus>)]
    exit_status: Option<String>,
    /// One of the stable `CorrectnessResult` values.
    #[param(value_type = Option<CorrectnessResult>)]
    correctness_result: Option<String>,
}

/// The model a run exercised, as referenced from a run summary.
#[derive(Serialize, ToSchema)]
struct ModelAssetRef {
    id: Uuid,
    /// The registered `.pte` file's name.
    original_name: String,
}

/// One run's list-view summary: identity, configuration, outcome, and
/// throughput. Fetch `GET /api/v1/runs/{id}` for the complete record.
#[derive(Serialize, ToSchema)]
struct RunSummaryResponse {
    id: Uuid,
    started_at: DateTime<Utc>,
    /// Absent while the run is still in progress.
    finished_at: Option<DateTime<Utc>>,
    repetition: i64,
    platform: Platform,
    /// `internal`: lab device with the rigorous snapshot. `external`:
    /// retail phone or Linux box recording what it can report.
    device_class: DeviceClass,
    /// Android: device serial. Linux: hostname.
    device_serial: String,
    /// Product/model name as the host reports it. Null when not captured.
    device_model: Option<String>,
    git_commit_sha: String,
    git_dirty: bool,
    /// Absent when the collector did not record it.
    git_branch: Option<String>,
    /// Lab Android devices only; null elsewhere.
    sumd_driver_version: Option<String>,
    /// Lab Android devices only; null elsewhere.
    bsp_version: Option<String>,
    /// The accelerator the backend executed on. Null when not captured.
    host_accelerator: Option<String>,
    model_asset: ModelAssetRef,
    exit_status: ExitStatus,
    correctness_result: CorrectnessResult,
    /// Prefill throughput in tokens per second.
    prefill_tokens_per_sec: f64,
    /// Decode throughput in tokens per second. Null when the run recorded
    /// no decode measurement.
    decode_tokens_per_sec: Option<f64>,
    /// Null when the platform's collector did not capture it.
    thermal_throttling: Option<bool>,
}

impl From<RunSummary> for RunSummaryResponse {
    fn from(s: RunSummary) -> Self {
        RunSummaryResponse {
            id: s.id,
            started_at: s.started_at,
            finished_at: s.finished_at,
            repetition: s.repetition,
            platform: s.platform,
            device_class: s.device_class,
            device_serial: s.device_serial,
            device_model: s.device_model,
            git_commit_sha: s.git_commit_sha,
            git_dirty: s.git_dirty,
            git_branch: s.git_branch,
            sumd_driver_version: s.sumd_driver_version,
            bsp_version: s.bsp_version,
            host_accelerator: s.host_accelerator,
            model_asset: ModelAssetRef {
                id: s.model_asset_id,
                original_name: s.model_original_name,
            },
            exit_status: s.exit_status,
            correctness_result: s.correctness_result,
            prefill_tokens_per_sec: s.prefill_tokens_per_sec,
            decode_tokens_per_sec: s.decode_tokens_per_sec,
            thermal_throttling: s.thermal_throttling,
        }
    }
}

/// One page of run summaries, newest first.
#[derive(Serialize, ToSchema)]
struct RunListResponse {
    items: Vec<RunSummaryResponse>,
    /// Opaque cursor for the next page. Null when no more runs match.
    next_cursor: Option<String>,
}

fn parse_list_params(params: ListRunsParams) -> Result<(RunListFilter, usize, Option<RunCursor>), ApiError> {
    let limit = params.limit.unwrap_or(DEFAULT_LIMIT);
    if limit == 0 || limit > MAX_LIMIT {
        return Err(ApiError::invalid_request(format!(
            "limit must be between 1 and {MAX_LIMIT}"
        )));
    }
    let cursor = params
        .cursor
        .as_deref()
        .map(RunCursor::decode)
        .transpose()
        .map_err(|err| ApiError::invalid_request(err.to_string()))?;
    let exit_status = params
        .exit_status
        .as_deref()
        .map(ExitStatus::try_from)
        .transpose()
        .map_err(|err| ApiError::invalid_request(err.to_string()))?;
    let correctness_result = params
        .correctness_result
        .as_deref()
        .map(CorrectnessResult::try_from)
        .transpose()
        .map_err(|err| ApiError::invalid_request(err.to_string()))?;
    let prompt_sha256 = params
        .prompt_sha256
        .map(Sha256Hex::try_from)
        .transpose()
        .map_err(|err| ApiError::invalid_request(err.to_string()))?;
    let platform = params
        .platform
        .as_deref()
        .map(Platform::try_from)
        .transpose()
        .map_err(|err| ApiError::invalid_request(err.to_string()))?;
    let device_class = params
        .device_class
        .as_deref()
        .map(DeviceClass::try_from)
        .transpose()
        .map_err(|err| ApiError::invalid_request(err.to_string()))?;

    let filter = RunListFilter {
        platform,
        device_class,
        device_serial: params.device_serial,
        model_asset_id: params.model_asset_id,
        git_commit_sha: params.git_commit_sha,
        git_branch: params.git_branch,
        git_dirty: params.git_dirty,
        sumd_driver_version: params.sumd_driver_version,
        bsp_version: params.bsp_version,
        gpu_clock_mhz: params.gpu_clock_mhz,
        mif_clock_mhz: params.mif_clock_mhz,
        int_clock_mhz: params.int_clock_mhz,
        host_accelerator: params.host_accelerator,
        prompt_sha256,
        exit_status,
        correctness_result,
    };
    Ok((filter, limit as usize, cursor))
}

/// List runs newest first, with exact-match filters and opaque keyset
/// pagination. See `specs/ingestion-service` - "Service exposes a
/// paginated, filterable run listing".
#[utoipa::path(
    get,
    path = "/api/v1/runs",
    operation_id = "listRuns",
    tag = "runs",
    params(ListRunsParams),
    responses(
        (status = 200, description = "One page of run summaries, newest first.", body = RunListResponse),
        (status = 400, description = "An unrecognized enum filter value, an out-of-range `limit`, or an undecodable `cursor`.", body = ApiErrorResponse),
        (status = 500, description = "Internal storage error.", body = ApiErrorResponse),
    )
)]
async fn list_runs_handler(
    State(state): State<AppState>,
    Query(params): Query<ListRunsParams>,
) -> Response {
    let (filter, limit, cursor) = match parse_list_params(params) {
        Ok(parsed) => parsed,
        Err(err) => return err.into_response(),
    };
    match list_runs(&state.pool, &filter, limit, cursor.as_ref()).await {
        Ok(page) => Json(RunListResponse {
            items: page.items.into_iter().map(Into::into).collect(),
            next_cursor: page.next_cursor.map(|c| c.encode()),
        })
        .into_response(),
        Err(_) => ApiError::internal("internal error listing runs").into_response(),
    }
}

/// A referenced artifact's viewable metadata: enough to know whether it is
/// present and how to retrieve it, without a separate lookup.
#[derive(Serialize, ToSchema)]
struct ArtifactView {
    id: Uuid,
    kind: ArtifactKind,
    /// Caller-supplied display name at upload time; never used to choose a
    /// storage path. Absent when no name was supplied.
    original_filename: Option<String>,
    size_bytes: i64,
    /// MIME type captured from the upload's `Content-Type` header, if any.
    media_type: Option<String>,
    compression: Compression,
    /// Whether the content-addressed file still exists on disk. The
    /// database record can outlive the file (see `artifact_file_missing`
    /// errors on content/download).
    available: bool,
    content_url: String,
    download_url: String,
}

async fn artifact_view(state: &AppState, id: Uuid) -> Option<ArtifactView> {
    let record: ArtifactRecord = get_artifact_record(&state.pool, id).await.ok()??;
    let available = artifact_file_exists(&state.config.artifact_root, &record).await;
    Some(ArtifactView {
        id: record.id,
        kind: record.kind,
        original_filename: record.original_filename.clone(),
        size_bytes: record.size_bytes,
        media_type: record.media_type.clone(),
        compression: record.compression,
        available,
        content_url: format!("/api/v1/artifacts/{}/content", record.id),
        download_url: format!("/api/v1/artifacts/{}/download", record.id),
    })
}

/// A run's referenced model asset, summarized for display alongside the run.
#[derive(Serialize, ToSchema)]
struct ModelAssetSummary {
    id: Uuid,
    /// Lowercase 64-character hexadecimal SHA-256 digest of the model file.
    #[schema(example = "b5bb9d8014a0f9b1d61e21e796d78dccdf1352f23cd32812f4850b878ae4944")]
    sha256: String,
    original_name: String,
    /// Whether the model is currently available (registered file still
    /// resolves on disk).
    available: bool,
}

/// A run's complete recorded record: metadata, device state, performance
/// configuration, build/workload identity, results, and the artifacts and
/// model it referenced. Flat: every field is top-level, named as recorded,
/// with units in descriptions. See `specs/ingestion-service` - "Single-run
/// responses expose the complete recorded run".
#[derive(Serialize, ToSchema)]
pub(crate) struct RunResponse {
    id: Uuid,
    started_at: DateTime<Utc>,
    /// Absent while the run is still in progress.
    finished_at: Option<DateTime<Utc>>,
    exit_status: ExitStatus,
    /// Independent of `exit_status` - a run can succeed but fail
    /// correctness validation, or vice versa.
    correctness_result: CorrectnessResult,
    /// Absent when the run produced no output or has not finished.
    output_preview: Option<String>,
    model_asset: Option<ModelAssetSummary>,
    input_artifact: Option<ArtifactView>,
    output_artifact: Option<ArtifactView>,
    stdout_artifact: Option<ArtifactView>,
    stderr_artifact: Option<ArtifactView>,
    crash_artifact: Option<ArtifactView>,

    // Run metadata
    /// Zero-based repetition index within the run's batch.
    repetition: i64,
    /// The exact command-line argument array, as a JSON array of strings.
    #[schema(value_type = Vec<String>)]
    command_args: serde_json::Value,
    /// Human-readable command line. Absent when not recorded.
    command_line: Option<String>,
    /// Caller-defined input parameters, as recorded JSON.
    #[schema(value_type = Object)]
    input_parameters: serde_json::Value,
    /// Captured environment-variable allowlist, as a JSON object mapping
    /// variable name to its value (`null` = unset, `""` = empty).
    #[schema(value_type = Object)]
    env_vars: serde_json::Value,
    env_allowlist_version: String,
    collector_version: String,

    // Host identity (every platform)
    platform: Platform,
    /// `internal`: lab device under full control, rigorous Android snapshot
    /// required. `external`: retail phone or Linux box recording what it can
    /// report; unavailable fields are null.
    device_class: DeviceClass,
    /// Android: device serial. Linux: hostname.
    device_serial: String,
    /// Product/model name as the host reports it. Null when not captured.
    device_model: Option<String>,
    /// Host uptime at run start, in seconds. Null when not captured.
    device_uptime_seconds: Option<i64>,
    /// Null when not captured.
    thermal_throttling: Option<bool>,

    // Android lab snapshot (required on internal Android devices; null on
    // Linux and where an external phone could not report it)
    /// Lab Android devices only; null elsewhere.
    bsp_version: Option<String>,
    /// Lab Android devices only; null elsewhere.
    sumd_driver_version: Option<String>,
    /// Android only; null when not captured.
    battery_charging: Option<bool>,
    /// Device temperature at run start, in degrees Celsius. Android only;
    /// null when not captured.
    initial_temperature_celsius: Option<f64>,
    /// Maximum device temperature observed during the run, in degrees
    /// Celsius. Android only; null when not captured.
    max_temperature_celsius: Option<f64>,

    // Android pinned clocks (lab devices only)
    /// GPU clock in MHz. Lab Android devices only; null elsewhere.
    gpu_clock_mhz: Option<i64>,
    /// MIF (memory interface) clock in MHz. Lab Android devices only; null
    /// elsewhere.
    mif_clock_mhz: Option<i64>,
    /// INT (interconnect) clock in MHz. Lab Android devices only; null
    /// elsewhere.
    int_clock_mhz: Option<i64>,

    // Host description (required on Linux; on Android, what the phone
    // reports: build, kernel, SoC, GPU)
    /// OS release: distribution and version on Linux, Android release and
    /// build id on phones. Null when not captured.
    host_os: Option<String>,
    /// Kernel release. Null when not captured.
    host_kernel: Option<String>,
    /// CPU model on Linux, SoC model on Android. Null when not captured.
    host_cpu_model: Option<String>,
    /// Logical CPU count. Null when not captured.
    host_cpu_count: Option<i64>,
    /// Total memory in bytes. Null when not captured.
    host_memory_bytes: Option<i64>,
    /// The accelerator the runtime backend executed on, as the backend
    /// reports it (Vulkan device on Linux, GPU on Android). Null when not
    /// captured.
    host_accelerator: Option<String>,
    /// The accelerator's driver version. Null when not captured.
    host_accelerator_driver: Option<String>,

    // Build and workload identity
    git_commit_sha: String,
    git_dirty: bool,
    /// Git branch the run was made from. Absent when not recorded.
    git_branch: Option<String>,
    /// The commit's timestamp. Absent when not recorded.
    git_commit_timestamp: Option<DateTime<Utc>>,
    /// The commit's subject line. Absent when not recorded.
    git_commit_subject: Option<String>,
    /// Lowercase 64-character hexadecimal SHA-256 digest of the executable.
    /// Null when the binary's identity was not preserved.
    #[schema(example = "b5bb9d8014a0f9b1d61e21e796d78dccdf1352f23cd32812f4850b878ae4944")]
    executable_sha256: Option<String>,
    /// Lowercase 64-character hexadecimal SHA-256 digest of the prompt.
    #[schema(example = "b5bb9d8014a0f9b1d61e21e796d78dccdf1352f23cd32812f4850b878ae4944")]
    prompt_sha256: String,
    input_token_count: i64,
    output_token_count: i64,

    // Results
    /// Prefill throughput in tokens per second.
    prefill_tokens_per_sec: f64,
    /// Decode throughput in tokens per second. Null when the run recorded
    /// no decode measurement.
    decode_tokens_per_sec: Option<f64>,
    /// Short description of the failure. Absent when the run did not fail.
    error_summary: Option<String>,
}

/// Decodes a stored canonical-JSON column. The column is `CHECK
/// (json_valid(...))` so parsing cannot normally fail; if it somehow does,
/// the raw text is returned as a JSON string rather than losing it.
fn stored_json(raw: String) -> serde_json::Value {
    serde_json::from_str(&raw).unwrap_or(serde_json::Value::String(raw))
}

pub(crate) async fn build_run_response(state: &AppState, run: Run) -> RunResponse {
    let model_asset = get_model_asset(&state.pool, run.model_asset_id)
        .await
        .ok()
        .flatten()
        .map(|asset| ModelAssetSummary {
            id: asset.id,
            sha256: asset.sha256,
            original_name: asset.original_name,
            available: asset.available,
        });

    let input_artifact = match run.input_artifact_id {
        Some(id) => artifact_view(state, id).await,
        None => None,
    };
    let output_artifact = match run.output_artifact_id {
        Some(id) => artifact_view(state, id).await,
        None => None,
    };
    let stdout_artifact = match run.stdout_artifact_id {
        Some(id) => artifact_view(state, id).await,
        None => None,
    };
    let stderr_artifact = match run.stderr_artifact_id {
        Some(id) => artifact_view(state, id).await,
        None => None,
    };
    let crash_artifact = match run.crash_artifact_id {
        Some(id) => artifact_view(state, id).await,
        None => None,
    };

    let android = run.host.android();
    let lab = android.and_then(|a| a.lab.as_ref());
    let desc = run.host.description();
    RunResponse {
        id: run.id,
        started_at: run.started_at,
        finished_at: run.finished_at,
        exit_status: run.exit_status,
        correctness_result: run.correctness_result,
        output_preview: run.output_preview,
        model_asset,
        input_artifact,
        output_artifact,
        stdout_artifact,
        stderr_artifact,
        crash_artifact,

        repetition: run.repetition,
        command_args: stored_json(run.command_args_json),
        command_line: run.command_line,
        input_parameters: stored_json(run.input_parameters_json),
        env_vars: stored_json(run.env_vars_json),
        env_allowlist_version: run.env_allowlist_version,
        collector_version: run.collector_version,

        platform: run.host.platform(),
        device_class: run.device_class,
        device_serial: run.device_serial,
        device_model: run.device_model,
        device_uptime_seconds: run.host.uptime_seconds(),
        thermal_throttling: run.host.thermal_throttling(),

        bsp_version: lab.map(|l| l.bsp_version.clone()),
        sumd_driver_version: lab.map(|l| l.sumd_driver_version.clone()),
        battery_charging: android.and_then(|a| a.battery_charging),
        initial_temperature_celsius: android.and_then(|a| a.initial_temperature_celsius),
        max_temperature_celsius: android.and_then(|a| a.max_temperature_celsius),

        gpu_clock_mhz: lab.map(|l| l.gpu_clock_mhz),
        mif_clock_mhz: lab.map(|l| l.mif_clock_mhz),
        int_clock_mhz: lab.map(|l| l.int_clock_mhz),

        host_os: desc.os.map(str::to_string),
        host_kernel: desc.kernel.map(str::to_string),
        host_cpu_model: desc.cpu_model.map(str::to_string),
        host_cpu_count: desc.cpu_count,
        host_memory_bytes: desc.memory_bytes,
        host_accelerator: desc.accelerator.map(str::to_string),
        host_accelerator_driver: desc.accelerator_driver.map(str::to_string),

        git_commit_sha: run.git_commit_sha,
        git_dirty: run.git_dirty,
        git_branch: run.git_branch,
        git_commit_timestamp: run.git_commit_timestamp,
        git_commit_subject: run.git_commit_subject,
        executable_sha256: run.executable_sha256.as_ref().map(|s| s.to_string()),
        prompt_sha256: run.prompt_sha256.to_string(),
        input_token_count: run.input_token_count,
        output_token_count: run.output_token_count,

        prefill_tokens_per_sec: run.prefill_tokens_per_sec,
        decode_tokens_per_sec: run.decode_tokens_per_sec,
        error_summary: run.error_summary,
    }
}

/// Fetch a run by ID, including its outcome and the artifacts/model it
/// referenced. See `specs/ingestion-service` - "Run responses expose
/// viewable artifact metadata".
#[utoipa::path(
    get,
    path = "/api/v1/runs/{id}",
    operation_id = "getRun",
    tag = "runs",
    params(("id" = Uuid, Path, description = "The run's identifier.")),
    responses(
        (status = 200, description = "The run was found.", body = RunResponse),
        (status = 404, description = "No run exists with this ID.", body = ApiErrorResponse),
    )
)]
async fn get_run_handler(
    State(state): State<AppState>,
    PathParam(id): PathParam<Uuid>,
) -> Response {
    match get_run(&state.pool, id).await {
        Ok(Some(run)) => Json(build_run_response(&state, run).await).into_response(),
        Ok(None) => ApiError::not_found("run not found").into_response(),
        Err(_) => ApiError::internal("internal error reading run").into_response(),
    }
}
