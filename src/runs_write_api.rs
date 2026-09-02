//! `POST /api/v1/runs`: a collector submits one complete, immutable run
//! record per repetition. See `specs/ingestion-service` - "Service accepts
//! complete run records over HTTP" and "Run creation validates references
//! and snapshot rules before writing".
//!
//! The request mirrors the flat run response (same field names, units, and
//! enumerations) so a client works against one shape. Enumerated fields are
//! accepted as strings and parsed here, so an unknown value is reported
//! through the error envelope naming the field rather than by the JSON
//! extractor's plain-text rejection.

use crate::api_error::{ApiError, ApiErrorResponse};
use crate::artifact_store::get_artifact_record;
use crate::domain::{
    CorrectnessResult, DeviceClass, ExitStatus, Platform, Sha256Hex, validate_command_args,
    validate_env_vars, validate_json,
};
use crate::events::{Event, RunCreatedEvent};
use crate::http::AppState;
use crate::model_registry::get_model_asset;
use crate::runs::{
    AndroidDeviceState, AndroidLabConfig, HostState, LinuxHostState, NewRun, get_run, insert_run,
    output_preview,
};
use crate::runs_api::{RunResponse, build_run_response};
use axum::body::Bytes;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(create_run))
}

/// One complete run attempt, as a collector submits it. Field names,
/// units, nullability, and enumerations match `RunResponse`; the artifact
/// references are IDs returned by `POST /api/v1/artifacts`, and
/// `model_asset_id` comes from `GET /api/v1/models?sha256=...` or
/// `POST /api/v1/models/register`.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct CreateRunRequest {
    /// Client-assigned run ID (a UUID; v7 recommended so IDs sort by
    /// time). Posting the same ID twice is rejected with `409 conflict`.
    pub id: Uuid,
    pub started_at: DateTime<Utc>,
    /// Null when the run never finished (crash, reboot, timeout).
    pub finished_at: Option<DateTime<Utc>>,
    /// Zero-based repetition index within the run's batch.
    pub repetition: i64,
    /// The exact command-line argument array.
    #[schema(value_type = Vec<String>)]
    pub command_args: serde_json::Value,
    /// Human-readable command line.
    pub command_line: Option<String>,
    /// Caller-defined input parameters (a JSON object).
    #[schema(value_type = Object)]
    pub input_parameters: serde_json::Value,
    /// Captured environment-variable allowlist as a JSON object (`null` =
    /// unset, `""` = empty).
    #[schema(value_type = Object)]
    pub env_vars: serde_json::Value,
    pub env_allowlist_version: String,
    pub collector_version: String,

    // Host identity (every platform)
    /// One of the stable `Platform` values.
    #[schema(value_type = Platform)]
    pub platform: String,
    /// One of the stable `DeviceClass` values. `internal` requires the
    /// complete Android lab snapshot; `external` records what the host can
    /// report.
    #[schema(value_type = DeviceClass)]
    pub device_class: String,
    /// Android: device serial. Linux: hostname.
    pub device_serial: String,
    /// Product/model name as the host reports it.
    pub device_model: Option<String>,
    /// Host uptime at run start, in seconds.
    pub device_uptime_seconds: Option<i64>,
    pub thermal_throttling: Option<bool>,

    // Android lab snapshot (BSP, SUMD, and the three clocks all together
    // or not at all; required on internal devices; forbidden on Linux)
    pub bsp_version: Option<String>,
    pub sumd_driver_version: Option<String>,
    /// Android only.
    pub battery_charging: Option<bool>,
    /// Device temperature at run start, in degrees Celsius. Android only.
    pub initial_temperature_celsius: Option<f64>,
    /// Maximum device temperature observed, in degrees Celsius. Android
    /// only.
    pub max_temperature_celsius: Option<f64>,
    /// GPU clock in MHz.
    pub gpu_clock_mhz: Option<i64>,
    /// MIF (memory interface) clock in MHz.
    pub mif_clock_mhz: Option<i64>,
    /// INT (interconnect) clock in MHz.
    pub int_clock_mhz: Option<i64>,

    // Host description (required on Linux: os, kernel, cpu_model,
    // accelerator; optional on Android)
    /// OS release: distribution and version on Linux, Android release and
    /// build id on phones.
    pub host_os: Option<String>,
    /// Kernel release.
    pub host_kernel: Option<String>,
    /// CPU model on Linux, SoC model on Android.
    pub host_cpu_model: Option<String>,
    /// Logical CPU count.
    pub host_cpu_count: Option<i64>,
    /// Total memory in bytes.
    pub host_memory_bytes: Option<i64>,
    /// The accelerator the runtime backend executed on, as the backend
    /// reports it.
    pub host_accelerator: Option<String>,
    /// The accelerator's driver version.
    pub host_accelerator_driver: Option<String>,

    // Build and workload identity
    pub git_commit_sha: String,
    pub git_dirty: bool,
    pub git_branch: Option<String>,
    pub git_commit_timestamp: Option<DateTime<Utc>>,
    pub git_commit_subject: Option<String>,
    /// Lowercase 64-character hexadecimal SHA-256 digest of the executable.
    /// Null when the binary's identity was not preserved; never a
    /// placeholder.
    pub executable_sha256: Option<String>,
    pub model_asset_id: Uuid,
    /// Lowercase 64-character hexadecimal SHA-256 digest of the prompt.
    pub prompt_sha256: String,
    pub input_token_count: i64,
    pub output_token_count: i64,

    // Results
    /// Prefill throughput in tokens per second (0 when nothing was
    /// measured).
    pub prefill_tokens_per_sec: f64,
    /// Decode throughput in tokens per second. Null when the run recorded
    /// no decode measurement.
    pub decode_tokens_per_sec: Option<f64>,
    /// One of the stable `ExitStatus` values.
    #[schema(value_type = ExitStatus)]
    pub exit_status: String,
    /// One of the stable `CorrectnessResult` values.
    #[schema(value_type = CorrectnessResult)]
    pub correctness_result: String,
    pub input_artifact_id: Option<Uuid>,
    pub output_artifact_id: Option<Uuid>,
    /// Truncated server-side to the configured preview length; the output
    /// artifact holds the complete text.
    pub output_preview: Option<String>,
    pub stdout_artifact_id: Option<Uuid>,
    pub stderr_artifact_id: Option<Uuid>,
    pub crash_artifact_id: Option<Uuid>,
    pub error_summary: Option<String>,
}

/// A validation failure naming the request field it concerns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldError {
    pub field: &'static str,
    pub message: String,
}

fn field_err(field: &'static str, message: impl Into<String>) -> FieldError {
    FieldError {
        field,
        message: message.into(),
    }
}

impl From<FieldError> for ApiError {
    fn from(err: FieldError) -> Self {
        ApiError::invalid_field(err.field, format!("{}: {}", err.field, err.message))
    }
}

fn sha(field: &'static str, raw: String) -> Result<Sha256Hex, FieldError> {
    Sha256Hex::try_from(raw).map_err(|err| field_err(field, err.to_string()))
}

fn non_negative(field: &'static str, value: i64) -> Result<i64, FieldError> {
    if value < 0 {
        Err(field_err(field, "must not be negative"))
    } else {
        Ok(value)
    }
}

fn non_negative_f64(field: &'static str, value: f64) -> Result<f64, FieldError> {
    if !value.is_finite() || value < 0.0 {
        Err(field_err(field, "must be a finite, non-negative number"))
    } else {
        Ok(value)
    }
}

fn positive_clock(field: &'static str, value: Option<i64>) -> Result<Option<i64>, FieldError> {
    match value {
        Some(v) if v <= 0 => Err(field_err(field, "must be greater than zero MHz")),
        other => Ok(other),
    }
}

fn temperature(field: &'static str, value: Option<f64>) -> Result<Option<f64>, FieldError> {
    match value {
        Some(v) if !(-40.0..=150.0).contains(&v) => {
            Err(field_err(field, "must be between -40 and 150 degrees Celsius"))
        }
        other => Ok(other),
    }
}

fn required<T>(field: &'static str, value: Option<T>, why: &str) -> Result<T, FieldError> {
    value.ok_or_else(|| field_err(field, format!("required {why}")))
}

fn forbidden<T>(field: &'static str, value: &Option<T>, why: &str) -> Result<(), FieldError> {
    if value.is_some() {
        Err(field_err(field, format!("must be null {why}")))
    } else {
        Ok(())
    }
}

/// Builds the platform-specific snapshot, enforcing the same rules as the
/// database CHECK so a violation is reported by field instead of as a
/// constraint failure.
fn host_state(
    req: &CreateRunRequest,
    platform: Platform,
    device_class: DeviceClass,
) -> Result<HostState, FieldError> {
    let gpu = positive_clock("gpu_clock_mhz", req.gpu_clock_mhz)?;
    let mif = positive_clock("mif_clock_mhz", req.mif_clock_mhz)?;
    let int = positive_clock("int_clock_mhz", req.int_clock_mhz)?;
    let initial = temperature("initial_temperature_celsius", req.initial_temperature_celsius)?;
    let max = temperature("max_temperature_celsius", req.max_temperature_celsius)?;
    if let Some(uptime) = req.device_uptime_seconds {
        non_negative("device_uptime_seconds", uptime)?;
    }
    if let Some(n) = req.host_cpu_count
        && n <= 0
    {
        return Err(field_err("host_cpu_count", "must be greater than zero"));
    }
    if let Some(n) = req.host_memory_bytes {
        non_negative("host_memory_bytes", n)?;
    }

    match platform {
        Platform::Linux => {
            let why = "on a linux run";
            forbidden("bsp_version", &req.bsp_version, why)?;
            forbidden("sumd_driver_version", &req.sumd_driver_version, why)?;
            forbidden("gpu_clock_mhz", &gpu, why)?;
            forbidden("mif_clock_mhz", &mif, why)?;
            forbidden("int_clock_mhz", &int, why)?;
            forbidden("battery_charging", &req.battery_charging, why)?;
            forbidden("initial_temperature_celsius", &initial, why)?;
            forbidden("max_temperature_celsius", &max, why)?;
            Ok(HostState::Linux(LinuxHostState {
                os: required("host_os", req.host_os.clone(), why)?,
                kernel: required("host_kernel", req.host_kernel.clone(), why)?,
                cpu_model: required("host_cpu_model", req.host_cpu_model.clone(), why)?,
                cpu_count: req.host_cpu_count,
                memory_bytes: req.host_memory_bytes,
                accelerator: required("host_accelerator", req.host_accelerator.clone(), why)?,
                accelerator_driver: req.host_accelerator_driver.clone(),
                uptime_seconds: req.device_uptime_seconds,
                thermal_throttling: req.thermal_throttling,
            }))
        }
        Platform::Android => {
            let lab_fields = [
                ("bsp_version", req.bsp_version.is_some()),
                ("sumd_driver_version", req.sumd_driver_version.is_some()),
                ("gpu_clock_mhz", gpu.is_some()),
                ("mif_clock_mhz", mif.is_some()),
                ("int_clock_mhz", int.is_some()),
            ];
            let present = lab_fields.iter().filter(|(_, p)| *p).count();
            let lab = if present == lab_fields.len() {
                Some(AndroidLabConfig {
                    bsp_version: req.bsp_version.clone().unwrap(),
                    sumd_driver_version: req.sumd_driver_version.clone().unwrap(),
                    gpu_clock_mhz: gpu.unwrap(),
                    mif_clock_mhz: mif.unwrap(),
                    int_clock_mhz: int.unwrap(),
                })
            } else if present == 0 {
                None
            } else {
                let missing = lab_fields.iter().find(|(_, p)| !*p).map(|(f, _)| *f).unwrap();
                return Err(field_err(
                    missing,
                    "BSP version, SUMD driver version, and the GPU/MIF/INT clocks are recorded all together or not at all",
                ));
            };
            let state = AndroidDeviceState {
                os: req.host_os.clone(),
                kernel: req.host_kernel.clone(),
                soc: req.host_cpu_model.clone(),
                cpu_count: req.host_cpu_count,
                memory_bytes: req.host_memory_bytes,
                gpu: req.host_accelerator.clone(),
                gpu_driver: req.host_accelerator_driver.clone(),
                uptime_seconds: req.device_uptime_seconds,
                battery_charging: req.battery_charging,
                initial_temperature_celsius: initial,
                max_temperature_celsius: max,
                thermal_throttling: req.thermal_throttling,
                lab,
            };
            if device_class == DeviceClass::Internal {
                let why = "on an internal android device";
                if state.lab.is_none() {
                    let missing = lab_fields.iter().find(|(_, p)| !*p).map(|(f, _)| *f).unwrap();
                    return Err(field_err(missing, format!("required {why}")));
                }
                required("device_uptime_seconds", state.uptime_seconds, why)?;
                required("battery_charging", state.battery_charging, why)?;
                required("initial_temperature_celsius", state.initial_temperature_celsius, why)?;
                required("max_temperature_celsius", state.max_temperature_celsius, why)?;
                required("thermal_throttling", state.thermal_throttling, why)?;
            }
            Ok(HostState::Android(state))
        }
    }
}

/// Converts a validated request into the storage type, reporting the first
/// rule violated by field. Reference existence is checked separately by the
/// handler (it needs the database).
pub fn to_new_run(req: &CreateRunRequest, preview_length: usize) -> Result<NewRun, FieldError> {
    let platform = Platform::try_from(req.platform.as_str())
        .map_err(|err| field_err("platform", err.to_string()))?;
    let device_class = DeviceClass::try_from(req.device_class.as_str())
        .map_err(|err| field_err("device_class", err.to_string()))?;
    let exit_status = ExitStatus::try_from(req.exit_status.as_str())
        .map_err(|err| field_err("exit_status", err.to_string()))?;
    let correctness_result = CorrectnessResult::try_from(req.correctness_result.as_str())
        .map_err(|err| field_err("correctness_result", err.to_string()))?;
    let repetition = non_negative("repetition", req.repetition)?;
    let command_args_json = validate_command_args(&req.command_args.to_string())
        .map_err(|err| field_err("command_args", err.to_string()))?;
    if !req.input_parameters.is_object() {
        return Err(field_err("input_parameters", "must be a JSON object"));
    }
    let input_parameters_json = validate_json(&req.input_parameters.to_string())
        .map_err(|err| field_err("input_parameters", err.to_string()))?;
    let env_vars_json = validate_env_vars(&req.env_vars.to_string())
        .map_err(|err| field_err("env_vars", err.to_string()))?;
    if req.device_serial.trim().is_empty() {
        return Err(field_err("device_serial", "must not be empty"));
    }
    if req.git_commit_sha.trim().is_empty() {
        return Err(field_err("git_commit_sha", "must not be empty"));
    }
    let host = host_state(req, platform, device_class)?;
    let executable_sha256 = req
        .executable_sha256
        .clone()
        .map(|raw| sha("executable_sha256", raw))
        .transpose()?;
    let prompt_sha256 = sha("prompt_sha256", req.prompt_sha256.clone())?;
    let input_token_count = non_negative("input_token_count", req.input_token_count)?;
    let output_token_count = non_negative("output_token_count", req.output_token_count)?;
    let prefill_tokens_per_sec = non_negative_f64("prefill_tokens_per_sec", req.prefill_tokens_per_sec)?;
    let decode_tokens_per_sec = req
        .decode_tokens_per_sec
        .map(|v| non_negative_f64("decode_tokens_per_sec", v))
        .transpose()?;

    Ok(NewRun {
        id: req.id,
        started_at: req.started_at,
        finished_at: req.finished_at,
        repetition,
        command_args_json,
        command_line: req.command_line.clone(),
        input_parameters_json,
        env_vars_json,
        env_allowlist_version: req.env_allowlist_version.clone(),
        collector_version: req.collector_version.clone(),
        device_class,
        device_serial: req.device_serial.clone(),
        device_model: req.device_model.clone(),
        host,
        git_commit_sha: req.git_commit_sha.clone(),
        git_dirty: req.git_dirty,
        git_branch: req.git_branch.clone(),
        git_commit_timestamp: req.git_commit_timestamp,
        git_commit_subject: req.git_commit_subject.clone(),
        executable_sha256,
        model_asset_id: req.model_asset_id,
        prompt_sha256,
        input_token_count,
        output_token_count,
        prefill_tokens_per_sec,
        decode_tokens_per_sec,
        exit_status,
        correctness_result,
        input_artifact_id: req.input_artifact_id,
        output_artifact_id: req.output_artifact_id,
        output_preview: req
            .output_preview
            .as_deref()
            .map(|p| output_preview(p, preview_length)),
        stdout_artifact_id: req.stdout_artifact_id,
        stderr_artifact_id: req.stderr_artifact_id,
        crash_artifact_id: req.crash_artifact_id,
        error_summary: req.error_summary.clone(),
    })
}

/// Confirms every referenced model asset and artifact exists, naming the
/// first missing reference by field.
async fn check_references(state: &AppState, run: &NewRun) -> Result<String, ApiError> {
    let asset = get_model_asset(&state.pool, run.model_asset_id)
        .await
        .map_err(|_| ApiError::internal("internal error reading model asset"))?
        .ok_or_else(|| {
            ApiError::invalid_field(
                "model_asset_id",
                format!("model_asset_id: no registered model asset {}", run.model_asset_id),
            )
        })?;
    for (field, id) in [
        ("input_artifact_id", run.input_artifact_id),
        ("output_artifact_id", run.output_artifact_id),
        ("stdout_artifact_id", run.stdout_artifact_id),
        ("stderr_artifact_id", run.stderr_artifact_id),
        ("crash_artifact_id", run.crash_artifact_id),
    ] {
        if let Some(id) = id {
            let found = get_artifact_record(&state.pool, id)
                .await
                .map_err(|_| ApiError::internal("internal error reading artifact"))?;
            if found.is_none() {
                return Err(ApiError::invalid_field(
                    field,
                    format!("{field}: no artifact {id}"),
                ));
            }
        }
    }
    Ok(asset.original_name)
}

/// Record one complete run attempt. The body mirrors the run response;
/// enumerated fields take their documented string values. Validation
/// failures name the field in `details.field`; a run ID that already exists
/// is rejected with `409 conflict` and left unchanged, so a client that
/// lost a response may retry and then confirm with `GET /api/v1/runs/{id}`.
/// See `specs/ingestion-service` - "Service accepts complete run records
/// over HTTP".
#[utoipa::path(
    post,
    path = "/api/v1/runs",
    operation_id = "createRun",
    tag = "runs",
    request_body = CreateRunRequest,
    responses(
        (status = 201, description = "The run was stored; the body is what `GET /api/v1/runs/{id}` returns.", body = RunResponse),
        (status = 400, description = "The body is not valid JSON, a field is invalid, the snapshot does not match the platform and device class, or a referenced model asset or artifact does not exist. `details.field` names the field.", body = ApiErrorResponse),
        (status = 409, description = "A run with this ID already exists; it is unchanged.", body = ApiErrorResponse),
        (status = 500, description = "Internal storage error.", body = ApiErrorResponse),
    )
)]
async fn create_run(State(state): State<AppState>, body: Bytes) -> Response {
    // Parse by hand so a malformed body or unknown enum value gets the
    // consistent envelope rather than the JSON extractor's plain-text
    // rejection.
    let req: CreateRunRequest = match serde_json::from_slice(&body) {
        Ok(req) => req,
        Err(err) => {
            return ApiError::invalid_request(format!("invalid run body: {err}")).into_response();
        }
    };
    let new_run = match to_new_run(&req, state.config.limits.output_preview_length) {
        Ok(run) => run,
        Err(err) => return ApiError::from(err).into_response(),
    };
    let model_original_name = match check_references(&state, &new_run).await {
        Ok(name) => name,
        Err(err) => return err.into_response(),
    };

    if let Err(err) = insert_run(&state.pool, &new_run).await {
        let text = err.to_string();
        if text.contains("UNIQUE constraint failed: runs.id") {
            return ApiError::conflict(format!("run {} already exists", new_run.id)).into_response();
        }
        // Validation above mirrors every CHECK; reaching here is a bug.
        eprintln!("run insert failed after validation: {text}");
        return ApiError::internal("internal error storing run").into_response();
    }

    state.events.publish(Event::RunCreated(RunCreatedEvent {
        id: new_run.id,
        started_at: new_run.started_at,
        finished_at: new_run.finished_at,
        repetition: new_run.repetition,
        platform: new_run.host.platform(),
        device_class: new_run.device_class,
        device_serial: new_run.device_serial.clone(),
        device_model: new_run.device_model.clone(),
        model_asset_id: new_run.model_asset_id,
        model_original_name,
        git_commit_sha: new_run.git_commit_sha.clone(),
        git_dirty: new_run.git_dirty,
        git_branch: new_run.git_branch.clone(),
        exit_status: new_run.exit_status,
        correctness_result: new_run.correctness_result,
        prefill_tokens_per_sec: new_run.prefill_tokens_per_sec,
        decode_tokens_per_sec: new_run.decode_tokens_per_sec,
    }));

    match get_run(&state.pool, new_run.id).await {
        Ok(Some(run)) => {
            (StatusCode::CREATED, Json(build_run_response(&state, run).await)).into_response()
        }
        _ => ApiError::internal("internal error reading stored run").into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn linux_request() -> CreateRunRequest {
        serde_json::from_value(serde_json::json!({
            "id": Uuid::now_v7(),
            "started_at": "2026-09-01T12:00:00Z",
            "finished_at": "2026-09-01T12:00:05Z",
            "repetition": 0,
            "command_args": ["--model_path=m.pte", "--max_new_tokens=1"],
            "command_line": "llama_main --model_path=m.pte --max_new_tokens=1",
            "input_parameters": {"backend": "vulkan"},
            "env_vars": {"ET_LOG": null},
            "env_allowlist_version": "v1",
            "collector_version": "collector/0.1",
            "platform": "linux",
            "device_class": "external",
            "device_serial": "ubuntu-lts-gpu",
            "host_os": "Ubuntu 24.04.4 LTS",
            "host_kernel": "7.0.0-30-generic",
            "host_cpu_model": "AMD EPYC 4464P",
            "host_accelerator": "Intel Arc B580",
            "git_commit_sha": "e4d02f41f",
            "git_dirty": false,
            "executable_sha256": "a".repeat(64),
            "model_asset_id": Uuid::now_v7(),
            "prompt_sha256": "c".repeat(64),
            "input_token_count": 2048,
            "output_token_count": 0,
            "prefill_tokens_per_sec": 385.6,
            "decode_tokens_per_sec": null,
            "exit_status": "succeeded",
            "correctness_result": "not_checked",
        }))
        .unwrap()
    }

    fn android_internal_request() -> CreateRunRequest {
        let mut req = linux_request();
        req.platform = "android".into();
        req.device_class = "internal".into();
        req.device_serial = "R5CX12ABCDE".into();
        req.host_os = None;
        req.host_kernel = None;
        req.host_cpu_model = None;
        req.host_accelerator = None;
        req.bsp_version = Some("bsp-2.3.1".into());
        req.sumd_driver_version = Some("sumd-1.8.0".into());
        req.gpu_clock_mhz = Some(980);
        req.mif_clock_mhz = Some(5333);
        req.int_clock_mhz = Some(934);
        req.device_uptime_seconds = Some(3600);
        req.battery_charging = Some(true);
        req.initial_temperature_celsius = Some(31.0);
        req.max_temperature_celsius = Some(40.5);
        req.thermal_throttling = Some(false);
        req
    }

    fn android_external_request() -> CreateRunRequest {
        let mut req = linux_request();
        req.platform = "android".into();
        req.device_class = "external".into();
        req.device_serial = "3A021JEHN02756".into();
        req.device_model = Some("Pixel 7a".into());
        req.host_os = Some("Android 17 (CP2A.260705.006)".into());
        req.host_cpu_model = Some("GS201".into());
        req.host_accelerator = Some("Mali-G710".into());
        req
    }

    fn field_of(result: Result<NewRun, FieldError>) -> &'static str {
        result.expect_err("expected a validation error").field
    }

    #[test]
    fn linux_happy_path_builds_a_linux_host() {
        let run = to_new_run(&linux_request(), 2048).unwrap();
        assert_eq!(run.device_class, DeviceClass::External);
        let linux = run.host.linux().expect("linux host");
        assert_eq!(linux.accelerator, "Intel Arc B580");
        assert_eq!(run.command_args_json, r#"["--model_path=m.pte","--max_new_tokens=1"]"#);
        assert_eq!(run.env_vars_json, r#"{"ET_LOG":null}"#);
        assert_eq!(run.exit_status, ExitStatus::Succeeded);
    }

    #[test]
    fn android_internal_happy_path_builds_the_lab_snapshot() {
        let run = to_new_run(&android_internal_request(), 2048).unwrap();
        let android = run.host.android().expect("android host");
        assert!(android.is_complete_lab_snapshot());
        assert_eq!(android.lab.as_ref().unwrap().gpu_clock_mhz, 980);
    }

    #[test]
    fn android_external_happy_path_has_no_lab_config() {
        let run = to_new_run(&android_external_request(), 2048).unwrap();
        let android = run.host.android().expect("android host");
        assert!(android.lab.is_none());
        assert_eq!(android.gpu.as_deref(), Some("Mali-G710"));
        assert_eq!(run.device_model.as_deref(), Some("Pixel 7a"));
    }

    #[test]
    fn each_rejection_rule_names_its_field() {
        let mut r = linux_request();
        r.platform = "ios".into();
        assert_eq!(field_of(to_new_run(&r, 10)), "platform");

        let mut r = linux_request();
        r.device_class = "lab".into();
        assert_eq!(field_of(to_new_run(&r, 10)), "device_class");

        let mut r = linux_request();
        r.exit_status = "exploded".into();
        assert_eq!(field_of(to_new_run(&r, 10)), "exit_status");

        let mut r = linux_request();
        r.correctness_result = "maybe".into();
        assert_eq!(field_of(to_new_run(&r, 10)), "correctness_result");

        let mut r = linux_request();
        r.repetition = -1;
        assert_eq!(field_of(to_new_run(&r, 10)), "repetition");

        let mut r = linux_request();
        r.command_args = serde_json::json!({"not": "an array"});
        assert_eq!(field_of(to_new_run(&r, 10)), "command_args");

        let mut r = linux_request();
        r.input_parameters = serde_json::json!([1, 2]);
        assert_eq!(field_of(to_new_run(&r, 10)), "input_parameters");

        let mut r = linux_request();
        r.env_vars = serde_json::json!("x");
        assert_eq!(field_of(to_new_run(&r, 10)), "env_vars");

        let mut r = linux_request();
        r.prompt_sha256 = "abc".into();
        assert_eq!(field_of(to_new_run(&r, 10)), "prompt_sha256");

        let mut r = linux_request();
        r.executable_sha256 = Some("A".repeat(64));
        assert_eq!(field_of(to_new_run(&r, 10)), "executable_sha256");

        let mut r = linux_request();
        r.input_token_count = -5;
        assert_eq!(field_of(to_new_run(&r, 10)), "input_token_count");

        let mut r = linux_request();
        r.output_token_count = -5;
        assert_eq!(field_of(to_new_run(&r, 10)), "output_token_count");

        let mut r = linux_request();
        r.prefill_tokens_per_sec = -1.0;
        assert_eq!(field_of(to_new_run(&r, 10)), "prefill_tokens_per_sec");

        let mut r = linux_request();
        r.decode_tokens_per_sec = Some(f64::NAN);
        assert_eq!(field_of(to_new_run(&r, 10)), "decode_tokens_per_sec");

        // Linux must carry its description and no lab fields.
        let mut r = linux_request();
        r.host_os = None;
        assert_eq!(field_of(to_new_run(&r, 10)), "host_os");
        let mut r = linux_request();
        r.host_accelerator = None;
        assert_eq!(field_of(to_new_run(&r, 10)), "host_accelerator");
        let mut r = linux_request();
        r.gpu_clock_mhz = Some(980);
        assert_eq!(field_of(to_new_run(&r, 10)), "gpu_clock_mhz");
        let mut r = linux_request();
        r.bsp_version = Some("bsp".into());
        assert_eq!(field_of(to_new_run(&r, 10)), "bsp_version");
        let mut r = linux_request();
        r.battery_charging = Some(true);
        assert_eq!(field_of(to_new_run(&r, 10)), "battery_charging");

        // Clocks must be positive, temperatures in range.
        let mut r = android_internal_request();
        r.mif_clock_mhz = Some(0);
        assert_eq!(field_of(to_new_run(&r, 10)), "mif_clock_mhz");
        let mut r = android_internal_request();
        r.max_temperature_celsius = Some(400.0);
        assert_eq!(field_of(to_new_run(&r, 10)), "max_temperature_celsius");

        // Internal Android needs the full snapshot.
        let mut r = android_internal_request();
        r.bsp_version = None;
        assert_eq!(field_of(to_new_run(&r, 10)), "bsp_version");
        let mut r = android_internal_request();
        r.battery_charging = None;
        assert_eq!(field_of(to_new_run(&r, 10)), "battery_charging");
        let mut r = android_internal_request();
        r.thermal_throttling = None;
        assert_eq!(field_of(to_new_run(&r, 10)), "thermal_throttling");

        // External Android: lab fields all or none.
        let mut r = android_external_request();
        r.gpu_clock_mhz = Some(980);
        assert_eq!(field_of(to_new_run(&r, 10)), "bsp_version");
    }

    #[test]
    fn output_preview_is_truncated_to_the_configured_length() {
        let mut r = linux_request();
        r.output_preview = Some("héllo world".into());
        let run = to_new_run(&r, 5).unwrap();
        assert_eq!(run.output_preview.as_deref(), Some("héllo"));
    }
}
