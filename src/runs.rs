use crate::domain::{CorrectnessResult, DeviceClass, ExitStatus, Platform, Sha256Hex};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, SecondsFormat, Utc};
use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool};
use std::fmt;
use uuid::Uuid;

#[derive(Debug)]
pub struct RunsError(pub(crate) String);

impl fmt::Display for RunsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for RunsError {}

impl From<sqlx::Error> for RunsError {
    fn from(err: sqlx::Error) -> Self {
        RunsError(format!("runs database error: {err}"))
    }
}

/// The lab-only part of an Android snapshot: what exists only on a
/// development phone under full control (BSP and SUMD driver versions and
/// the three pinned clocks). A retail phone has none of it. See
/// `specs/benchmark-schema` - "Performance configuration is captured per
/// run".
#[derive(Debug, Clone, PartialEq)]
pub struct AndroidLabConfig {
    pub bsp_version: String,
    pub sumd_driver_version: String,
    pub gpu_clock_mhz: i64,
    pub mif_clock_mhz: i64,
    pub int_clock_mhz: i64,
}

/// The immutable snapshot of an Android phone at the time of a run. An
/// `internal` device must fill every field including `lab`; an `external`
/// (retail, unrooted) device records what it can report and leaves the rest
/// `None`. The database CHECK enforces the same rule per device class. See
/// `specs/benchmark-schema` - "Host state is captured as a
/// platform-specific immutable snapshot".
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AndroidDeviceState {
    /// Android release and build id as the phone reports them.
    pub os: Option<String>,
    pub kernel: Option<String>,
    /// SoC model (`ro.soc.model`).
    pub soc: Option<String>,
    pub cpu_count: Option<i64>,
    pub memory_bytes: Option<i64>,
    /// GPU as the phone reports it.
    pub gpu: Option<String>,
    pub gpu_driver: Option<String>,
    pub uptime_seconds: Option<i64>,
    pub battery_charging: Option<bool>,
    pub initial_temperature_celsius: Option<f64>,
    pub max_temperature_celsius: Option<f64>,
    pub thermal_throttling: Option<bool>,
    /// Present only on lab-controlled devices.
    pub lab: Option<AndroidLabConfig>,
}

impl AndroidDeviceState {
    /// The full rigorous snapshot an internal device must provide.
    pub fn internal(
        lab: AndroidLabConfig,
        uptime_seconds: i64,
        battery_charging: bool,
        initial_temperature_celsius: f64,
        max_temperature_celsius: f64,
        thermal_throttling: bool,
    ) -> Self {
        AndroidDeviceState {
            uptime_seconds: Some(uptime_seconds),
            battery_charging: Some(battery_charging),
            initial_temperature_celsius: Some(initial_temperature_celsius),
            max_temperature_celsius: Some(max_temperature_celsius),
            thermal_throttling: Some(thermal_throttling),
            lab: Some(lab),
            ..Default::default()
        }
    }

    /// Whether every field an `internal` device must carry is present.
    pub fn is_complete_lab_snapshot(&self) -> bool {
        self.lab.is_some()
            && self.uptime_seconds.is_some()
            && self.battery_charging.is_some()
            && self.initial_temperature_celsius.is_some()
            && self.max_temperature_celsius.is_some()
            && self.thermal_throttling.is_some()
    }
}

/// The immutable snapshot of a Linux host at the time of a run. OS,
/// kernel, CPU model, and the accelerator the backend executed on are
/// required; the rest is recorded when the collector captured it.
#[derive(Debug, Clone, PartialEq)]
pub struct LinuxHostState {
    /// Distribution name and version (e.g. `Ubuntu 24.04.4 LTS`).
    pub os: String,
    /// Kernel release (`uname -r`).
    pub kernel: String,
    pub cpu_model: String,
    pub cpu_count: Option<i64>,
    pub memory_bytes: Option<i64>,
    /// The accelerator as the runtime backend reports it (e.g. the Vulkan
    /// device name). Part of the benchmark configuration key.
    pub accelerator: String,
    pub accelerator_driver: Option<String>,
    pub uptime_seconds: Option<i64>,
    pub thermal_throttling: Option<bool>,
}

/// The platform-specific host snapshot a run carries. Exactly one variant
/// per run; the database enforces the same shape with a CHECK constraint.
#[derive(Debug, Clone, PartialEq)]
pub enum HostState {
    Android(AndroidDeviceState),
    Linux(LinuxHostState),
}

impl HostState {
    pub fn platform(&self) -> Platform {
        match self {
            HostState::Android(_) => Platform::Android,
            HostState::Linux(_) => Platform::Linux,
        }
    }

    pub fn android(&self) -> Option<&AndroidDeviceState> {
        match self {
            HostState::Android(a) => Some(a),
            HostState::Linux(_) => None,
        }
    }

    pub fn linux(&self) -> Option<&LinuxHostState> {
        match self {
            HostState::Linux(l) => Some(l),
            HostState::Android(_) => None,
        }
    }

    /// Whether thermal throttling was detected; `None` when the platform's
    /// collector did not capture it.
    pub fn thermal_throttling(&self) -> Option<bool> {
        match self {
            HostState::Android(a) => a.thermal_throttling,
            HostState::Linux(l) => l.thermal_throttling,
        }
    }

    pub fn uptime_seconds(&self) -> Option<i64> {
        match self {
            HostState::Android(a) => a.uptime_seconds,
            HostState::Linux(l) => l.uptime_seconds,
        }
    }

    /// The host description columns (`host_*`) for either platform.
    pub fn description(&self) -> HostDescription<'_> {
        match self {
            HostState::Android(a) => HostDescription {
                os: a.os.as_deref(),
                kernel: a.kernel.as_deref(),
                cpu_model: a.soc.as_deref(),
                cpu_count: a.cpu_count,
                memory_bytes: a.memory_bytes,
                accelerator: a.gpu.as_deref(),
                accelerator_driver: a.gpu_driver.as_deref(),
            },
            HostState::Linux(l) => HostDescription {
                os: Some(&l.os),
                kernel: Some(&l.kernel),
                cpu_model: Some(&l.cpu_model),
                cpu_count: l.cpu_count,
                memory_bytes: l.memory_bytes,
                accelerator: Some(&l.accelerator),
                accelerator_driver: l.accelerator_driver.as_deref(),
            },
        }
    }
}

/// The platform-neutral host description a run exposes: OS, kernel, CPU or
/// SoC, memory, and the accelerator the backend executed on.
#[derive(Debug, Clone, Copy)]
pub struct HostDescription<'a> {
    pub os: Option<&'a str>,
    pub kernel: Option<&'a str>,
    pub cpu_model: Option<&'a str>,
    pub cpu_count: Option<i64>,
    pub memory_bytes: Option<i64>,
    pub accelerator: Option<&'a str>,
    pub accelerator_driver: Option<&'a str>,
}

/// A fully specified run attempt, ready to insert. JSON text fields are
/// expected to already be validated/canonicalized (see `crate::domain`).
#[derive(Debug, Clone)]
pub struct NewRun {
    pub id: Uuid,

    // Run metadata
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub repetition: i64,
    pub command_args_json: String,
    pub command_line: Option<String>,
    pub input_parameters_json: String,
    pub env_vars_json: String,
    pub env_allowlist_version: String,
    pub collector_version: String,

    // Host identity and platform-specific state
    pub device_class: DeviceClass,
    /// Android: the device serial. Linux: the hostname.
    pub device_serial: String,
    /// Product/model name as the host reports it, when captured.
    pub device_model: Option<String>,
    pub host: HostState,

    // Build and workload identity
    pub git_commit_sha: String,
    pub git_dirty: bool,
    /// Optional git commit metadata (branch, commit timestamp, subject).
    /// Nullable: absent for runs recorded without it.
    pub git_branch: Option<String>,
    pub git_commit_timestamp: Option<DateTime<Utc>>,
    pub git_commit_subject: Option<String>,
    /// Null when the binary's identity was not preserved (never a guess).
    pub executable_sha256: Option<Sha256Hex>,
    pub model_asset_id: Uuid,
    pub prompt_sha256: Sha256Hex,
    pub input_token_count: i64,
    pub output_token_count: i64,

    // Results
    pub prefill_tokens_per_sec: f64,
    pub decode_tokens_per_sec: Option<f64>,
    pub exit_status: ExitStatus,
    pub correctness_result: CorrectnessResult,
    pub input_artifact_id: Option<Uuid>,
    pub output_artifact_id: Option<Uuid>,
    pub output_preview: Option<String>,
    pub stdout_artifact_id: Option<Uuid>,
    pub stderr_artifact_id: Option<Uuid>,
    pub crash_artifact_id: Option<Uuid>,
    pub error_summary: Option<String>,
}

/// A run attempt as read back from storage.
#[derive(Debug, Clone)]
pub struct Run {
    pub id: Uuid,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub repetition: i64,
    pub command_args_json: String,
    pub command_line: Option<String>,
    pub input_parameters_json: String,
    pub env_vars_json: String,
    pub env_allowlist_version: String,
    pub collector_version: String,

    pub device_class: DeviceClass,
    pub device_serial: String,
    pub device_model: Option<String>,
    pub host: HostState,

    pub git_commit_sha: String,
    pub git_dirty: bool,
    /// Optional git commit metadata (branch, commit timestamp, subject).
    /// Nullable: absent for runs recorded without it.
    pub git_branch: Option<String>,
    pub git_commit_timestamp: Option<DateTime<Utc>>,
    pub git_commit_subject: Option<String>,
    /// Null when the binary's identity was not preserved (never a guess).
    pub executable_sha256: Option<Sha256Hex>,
    pub model_asset_id: Uuid,
    pub prompt_sha256: Sha256Hex,
    pub input_token_count: i64,
    pub output_token_count: i64,

    pub prefill_tokens_per_sec: f64,
    pub decode_tokens_per_sec: Option<f64>,
    pub exit_status: ExitStatus,
    pub correctness_result: CorrectnessResult,
    pub input_artifact_id: Option<Uuid>,
    pub output_artifact_id: Option<Uuid>,
    pub output_preview: Option<String>,
    pub stdout_artifact_id: Option<Uuid>,
    pub stderr_artifact_id: Option<Uuid>,
    pub crash_artifact_id: Option<Uuid>,
    pub error_summary: Option<String>,
}

fn parse_optional_uuid(value: Option<String>, field: &str) -> Result<Option<Uuid>, RunsError> {
    value
        .map(|v| Uuid::parse_str(&v).map_err(|err| RunsError(format!("invalid {field}: {err}"))))
        .transpose()
}

fn optional_flag(row: &sqlx::sqlite::SqliteRow, column: &str) -> Result<Option<bool>, RunsError> {
    Ok(row.try_get::<Option<i64>, _>(column)?.map(|v| v != 0))
}

fn required<T>(value: Option<T>, column: &str, platform: Platform) -> Result<T, RunsError> {
    value.ok_or_else(|| {
        RunsError(format!(
            "stored {} run is missing {column}",
            platform.as_str()
        ))
    })
}

/// Reconstructs the platform-specific snapshot from the flat `runs`
/// columns. The CHECK constraint guarantees the required columns for the
/// row's platform are present; a violation is reported, not papered over.
pub(crate) fn host_state_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<HostState, RunsError> {
    let platform: String = row.try_get("platform")?;
    let platform = Platform::try_from(platform.as_str())
        .map_err(|err| RunsError(format!("invalid stored platform: {err}")))?;
    match platform {
        Platform::Android => {
            let bsp_version: Option<String> = row.try_get("bsp_version")?;
            let lab = match bsp_version {
                Some(bsp_version) => Some(AndroidLabConfig {
                    bsp_version,
                    sumd_driver_version: required(
                        row.try_get("sumd_driver_version")?,
                        "sumd_driver_version",
                        platform,
                    )?,
                    gpu_clock_mhz: required(row.try_get("gpu_clock_mhz")?, "gpu_clock_mhz", platform)?,
                    mif_clock_mhz: required(row.try_get("mif_clock_mhz")?, "mif_clock_mhz", platform)?,
                    int_clock_mhz: required(row.try_get("int_clock_mhz")?, "int_clock_mhz", platform)?,
                }),
                None => None,
            };
            Ok(HostState::Android(AndroidDeviceState {
                os: row.try_get("host_os")?,
                kernel: row.try_get("host_kernel")?,
                soc: row.try_get("host_cpu_model")?,
                cpu_count: row.try_get("host_cpu_count")?,
                memory_bytes: row.try_get("host_memory_bytes")?,
                gpu: row.try_get("host_accelerator")?,
                gpu_driver: row.try_get("host_accelerator_driver")?,
                uptime_seconds: row.try_get("device_uptime_seconds")?,
                battery_charging: optional_flag(row, "battery_charging")?,
                initial_temperature_celsius: row.try_get("initial_temperature_celsius")?,
                max_temperature_celsius: row.try_get("max_temperature_celsius")?,
                thermal_throttling: optional_flag(row, "thermal_throttling")?,
                lab,
            }))
        }
        Platform::Linux => Ok(HostState::Linux(LinuxHostState {
            os: required(row.try_get("host_os")?, "host_os", platform)?,
            kernel: required(row.try_get("host_kernel")?, "host_kernel", platform)?,
            cpu_model: required(row.try_get("host_cpu_model")?, "host_cpu_model", platform)?,
            cpu_count: row.try_get("host_cpu_count")?,
            memory_bytes: row.try_get("host_memory_bytes")?,
            accelerator: required(
                row.try_get("host_accelerator")?,
                "host_accelerator",
                platform,
            )?,
            accelerator_driver: row.try_get("host_accelerator_driver")?,
            uptime_seconds: row.try_get("device_uptime_seconds")?,
            thermal_throttling: optional_flag(row, "thermal_throttling")?,
        })),
    }
}

fn row_to_run(row: sqlx::sqlite::SqliteRow) -> Result<Run, RunsError> {
    let id: String = row.try_get("id")?;
    let exit_status: String = row.try_get("exit_status")?;
    let correctness_result: String = row.try_get("correctness_result")?;
    let executable_sha256: Option<String> = row.try_get("executable_sha256")?;
    let model_asset_id: String = row.try_get("model_asset_id")?;
    let prompt_sha256: String = row.try_get("prompt_sha256")?;
    let host = host_state_from_row(&row)?;
    let device_class: String = row.try_get("device_class")?;

    Ok(Run {
        id: Uuid::parse_str(&id).map_err(|err| RunsError(format!("invalid run id: {err}")))?,
        started_at: row.try_get("started_at")?,
        finished_at: row.try_get("finished_at")?,
        repetition: row.try_get("repetition")?,
        command_args_json: row.try_get("command_args")?,
        command_line: row.try_get("command_line")?,
        input_parameters_json: row.try_get("input_parameters")?,
        env_vars_json: row.try_get("env_vars")?,
        env_allowlist_version: row.try_get("env_allowlist_version")?,
        collector_version: row.try_get("collector_version")?,

        device_class: DeviceClass::try_from(device_class.as_str())
            .map_err(|err| RunsError(format!("invalid stored device_class: {err}")))?,
        device_serial: row.try_get("device_serial")?,
        device_model: row.try_get("device_model")?,
        host,

        git_commit_sha: row.try_get("git_commit_sha")?,
        git_dirty: row.try_get::<i64, _>("git_dirty")? != 0,
        git_branch: row.try_get("git_branch")?,
        git_commit_timestamp: row.try_get("git_commit_timestamp")?,
        git_commit_subject: row.try_get("git_commit_subject")?,
        executable_sha256: executable_sha256
            .map(Sha256Hex::try_from)
            .transpose()
            .map_err(|err| RunsError(format!("invalid stored executable_sha256: {err}")))?,
        model_asset_id: Uuid::parse_str(&model_asset_id)
            .map_err(|err| RunsError(format!("invalid stored model_asset_id: {err}")))?,
        prompt_sha256: Sha256Hex::try_from(prompt_sha256)
            .map_err(|err| RunsError(format!("invalid stored prompt_sha256: {err}")))?,
        input_token_count: row.try_get("input_token_count")?,
        output_token_count: row.try_get("output_token_count")?,

        prefill_tokens_per_sec: row.try_get("prefill_tokens_per_sec")?,
        decode_tokens_per_sec: row.try_get("decode_tokens_per_sec")?,
        exit_status: ExitStatus::try_from(exit_status.as_str())
            .map_err(|err| RunsError(format!("invalid stored exit_status: {err}")))?,
        correctness_result: CorrectnessResult::try_from(correctness_result.as_str())
            .map_err(|err| RunsError(format!("invalid stored correctness_result: {err}")))?,
        input_artifact_id: parse_optional_uuid(
            row.try_get("input_artifact_id")?,
            "input_artifact_id",
        )?,
        output_artifact_id: parse_optional_uuid(
            row.try_get("output_artifact_id")?,
            "output_artifact_id",
        )?,
        output_preview: row.try_get("output_preview")?,
        stdout_artifact_id: parse_optional_uuid(
            row.try_get("stdout_artifact_id")?,
            "stdout_artifact_id",
        )?,
        stderr_artifact_id: parse_optional_uuid(
            row.try_get("stderr_artifact_id")?,
            "stderr_artifact_id",
        )?,
        crash_artifact_id: parse_optional_uuid(
            row.try_get("crash_artifact_id")?,
            "crash_artifact_id",
        )?,
        error_summary: row.try_get("error_summary")?,
    })
}

pub async fn insert_run(pool: &SqlitePool, run: &NewRun) -> Result<(), RunsError> {
    let android = run.host.android();
    if run.device_class == DeviceClass::Internal
        && android.is_some_and(|a| !a.is_complete_lab_snapshot())
    {
        return Err(RunsError(
            "an internal Android device must record the complete lab snapshot \
             (BSP, SUMD driver, clocks, uptime, battery, temperatures, throttling)"
                .to_string(),
        ));
    }
    let lab = android.and_then(|a| a.lab.as_ref());
    let desc = run.host.description();
    sqlx::query(
        "INSERT INTO runs (
            id, started_at, finished_at, repetition, command_args, command_line,
            input_parameters, env_vars, env_allowlist_version, collector_version,
            platform, device_class, device_serial, device_model,
            device_uptime_seconds, thermal_throttling,
            bsp_version, sumd_driver_version, battery_charging,
            initial_temperature_celsius, max_temperature_celsius,
            gpu_clock_mhz, mif_clock_mhz, int_clock_mhz,
            host_os, host_kernel, host_cpu_model, host_cpu_count, host_memory_bytes,
            host_accelerator, host_accelerator_driver,
            git_commit_sha, git_dirty, git_branch, git_commit_timestamp, git_commit_subject,
            executable_sha256, model_asset_id, prompt_sha256,
            input_token_count, output_token_count, prefill_tokens_per_sec,
            decode_tokens_per_sec, exit_status, correctness_result,
            input_artifact_id, output_artifact_id, output_preview,
            stdout_artifact_id, stderr_artifact_id, crash_artifact_id, error_summary
        ) VALUES (
            ?, ?, ?, ?, ?, ?, ?, ?, ?, ?,
            ?, ?, ?, ?, ?, ?,
            ?, ?, ?, ?, ?, ?, ?, ?,
            ?, ?, ?, ?, ?, ?, ?,
            ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?
        )",
    )
    .bind(run.id.to_string())
    .bind(run.started_at)
    .bind(run.finished_at)
    .bind(run.repetition)
    .bind(&run.command_args_json)
    .bind(&run.command_line)
    .bind(&run.input_parameters_json)
    .bind(&run.env_vars_json)
    .bind(&run.env_allowlist_version)
    .bind(&run.collector_version)
    .bind(run.host.platform().as_str())
    .bind(run.device_class.as_str())
    .bind(&run.device_serial)
    .bind(&run.device_model)
    .bind(run.host.uptime_seconds())
    .bind(run.host.thermal_throttling().map(i64::from))
    .bind(lab.map(|l| l.bsp_version.clone()))
    .bind(lab.map(|l| l.sumd_driver_version.clone()))
    .bind(android.and_then(|a| a.battery_charging).map(i64::from))
    .bind(android.and_then(|a| a.initial_temperature_celsius))
    .bind(android.and_then(|a| a.max_temperature_celsius))
    .bind(lab.map(|l| l.gpu_clock_mhz))
    .bind(lab.map(|l| l.mif_clock_mhz))
    .bind(lab.map(|l| l.int_clock_mhz))
    .bind(desc.os.map(str::to_string))
    .bind(desc.kernel.map(str::to_string))
    .bind(desc.cpu_model.map(str::to_string))
    .bind(desc.cpu_count)
    .bind(desc.memory_bytes)
    .bind(desc.accelerator.map(str::to_string))
    .bind(desc.accelerator_driver.map(str::to_string))
    .bind(&run.git_commit_sha)
    .bind(run.git_dirty as i64)
    .bind(&run.git_branch)
    .bind(run.git_commit_timestamp)
    .bind(&run.git_commit_subject)
    .bind(run.executable_sha256.as_ref().map(|s| s.as_str().to_string()))
    .bind(run.model_asset_id.to_string())
    .bind(run.prompt_sha256.as_str())
    .bind(run.input_token_count)
    .bind(run.output_token_count)
    .bind(run.prefill_tokens_per_sec)
    .bind(run.decode_tokens_per_sec)
    .bind(run.exit_status.as_str())
    .bind(run.correctness_result.as_str())
    .bind(run.input_artifact_id.map(|id| id.to_string()))
    .bind(run.output_artifact_id.map(|id| id.to_string()))
    .bind(&run.output_preview)
    .bind(run.stdout_artifact_id.map(|id| id.to_string()))
    .bind(run.stderr_artifact_id.map(|id| id.to_string()))
    .bind(run.crash_artifact_id.map(|id| id.to_string()))
    .bind(&run.error_summary)
    .execute(pool)
    .await?;

    Ok(())
}

/// Truncates `full_output` to at most `max_chars` characters (not bytes, so
/// multi-byte UTF-8 sequences are never split) for the `output_preview`
/// column. The run's `output` artifact - not this preview - remains the
/// complete, authoritative record of the generated content. See
/// `specs/benchmark-schema` - "A run retains a short output preview
/// alongside the complete artifact".
pub fn output_preview(full_output: &str, max_chars: usize) -> String {
    full_output.chars().take(max_chars).collect()
}

pub async fn get_run(pool: &SqlitePool, id: Uuid) -> Result<Option<Run>, RunsError> {
    let row = sqlx::query("SELECT * FROM runs WHERE id = ?")
        .bind(id.to_string())
        .fetch_optional(pool)
        .await?;

    row.map(row_to_run).transpose()
}


/// Exact-match filters for [`list_runs`]. Every field is optional; set
/// fields are combined conjunctively. Together the non-outcome fields form
/// the full benchmark configuration key, so a results row can link to
/// exactly the runs that produced it.
#[derive(Debug, Clone, Default)]
pub struct RunListFilter {
    pub platform: Option<Platform>,
    pub device_class: Option<DeviceClass>,
    pub device_serial: Option<String>,
    pub model_asset_id: Option<Uuid>,
    pub git_commit_sha: Option<String>,
    pub git_branch: Option<String>,
    pub git_dirty: Option<bool>,
    pub sumd_driver_version: Option<String>,
    pub bsp_version: Option<String>,
    pub gpu_clock_mhz: Option<i64>,
    pub mif_clock_mhz: Option<i64>,
    pub int_clock_mhz: Option<i64>,
    pub host_accelerator: Option<String>,
    pub prompt_sha256: Option<Sha256Hex>,
    pub exit_status: Option<ExitStatus>,
    pub correctness_result: Option<CorrectnessResult>,
}

impl RunListFilter {
    /// Appends `AND <column> = ?` clauses for every set field. The caller
    /// has already pushed a `WHERE 1=1` (or equivalent) so this can
    /// unconditionally prefix each clause with `AND`.
    pub(crate) fn push_where_clauses(&self, qb: &mut QueryBuilder<Sqlite>) {
        if let Some(v) = self.platform {
            qb.push(" AND r.platform = ").push_bind(v.as_str());
        }
        if let Some(v) = self.device_class {
            qb.push(" AND r.device_class = ").push_bind(v.as_str());
        }
        if let Some(v) = &self.device_serial {
            qb.push(" AND r.device_serial = ").push_bind(v.clone());
        }
        if let Some(v) = self.model_asset_id {
            qb.push(" AND r.model_asset_id = ").push_bind(v.to_string());
        }
        if let Some(v) = &self.git_commit_sha {
            qb.push(" AND r.git_commit_sha = ").push_bind(v.clone());
        }
        if let Some(v) = &self.git_branch {
            qb.push(" AND r.git_branch = ").push_bind(v.clone());
        }
        if let Some(v) = self.git_dirty {
            qb.push(" AND r.git_dirty = ").push_bind(v as i64);
        }
        if let Some(v) = &self.sumd_driver_version {
            qb.push(" AND r.sumd_driver_version = ").push_bind(v.clone());
        }
        if let Some(v) = &self.bsp_version {
            qb.push(" AND r.bsp_version = ").push_bind(v.clone());
        }
        if let Some(v) = self.gpu_clock_mhz {
            qb.push(" AND r.gpu_clock_mhz = ").push_bind(v);
        }
        if let Some(v) = self.mif_clock_mhz {
            qb.push(" AND r.mif_clock_mhz = ").push_bind(v);
        }
        if let Some(v) = self.int_clock_mhz {
            qb.push(" AND r.int_clock_mhz = ").push_bind(v);
        }
        if let Some(v) = &self.host_accelerator {
            qb.push(" AND r.host_accelerator = ").push_bind(v.clone());
        }
        if let Some(v) = &self.prompt_sha256 {
            qb.push(" AND r.prompt_sha256 = ").push_bind(v.as_str().to_string());
        }
        if let Some(v) = self.exit_status {
            qb.push(" AND r.exit_status = ").push_bind(v.as_str());
        }
        if let Some(v) = self.correctness_result {
            qb.push(" AND r.correctness_result = ").push_bind(v.as_str());
        }
    }
}

/// Keyset-pagination position: the `(started_at, id)` of the last run on
/// the previous page. Encoded as an opaque base64url token for clients.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunCursor {
    pub started_at: DateTime<Utc>,
    pub id: Uuid,
}

#[derive(Debug)]
pub struct InvalidCursor(String);

impl fmt::Display for InvalidCursor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid cursor: {}", self.0)
    }
}

impl std::error::Error for InvalidCursor {}

impl RunCursor {
    pub fn encode(&self) -> String {
        let raw = format!(
            "{}|{}",
            self.started_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
            self.id
        );
        URL_SAFE_NO_PAD.encode(raw.as_bytes())
    }

    pub fn decode(token: &str) -> Result<Self, InvalidCursor> {
        let bytes = URL_SAFE_NO_PAD
            .decode(token.as_bytes())
            .map_err(|_| InvalidCursor("not base64url".into()))?;
        let raw = String::from_utf8(bytes).map_err(|_| InvalidCursor("not UTF-8".into()))?;
        let (ts, id) = raw
            .split_once('|')
            .ok_or_else(|| InvalidCursor("missing separator".into()))?;
        let started_at = DateTime::parse_from_rfc3339(ts)
            .map_err(|_| InvalidCursor("bad timestamp".into()))?
            .with_timezone(&Utc);
        let id = Uuid::parse_str(id).map_err(|_| InvalidCursor("bad run id".into()))?;
        Ok(RunCursor { started_at, id })
    }
}

/// The per-run summary the list operation returns: list columns only, no
/// JSON blobs or environment capture.
#[derive(Debug, Clone)]
pub struct RunSummary {
    pub id: Uuid,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub repetition: i64,
    pub platform: Platform,
    pub device_class: DeviceClass,
    pub device_serial: String,
    pub device_model: Option<String>,
    pub git_commit_sha: String,
    pub git_dirty: bool,
    pub git_branch: Option<String>,
    /// Android only.
    pub sumd_driver_version: Option<String>,
    /// Android only.
    pub bsp_version: Option<String>,
    /// The accelerator the backend executed on, when captured.
    pub host_accelerator: Option<String>,
    pub model_asset_id: Uuid,
    pub model_original_name: String,
    pub exit_status: ExitStatus,
    pub correctness_result: CorrectnessResult,
    pub prefill_tokens_per_sec: f64,
    pub decode_tokens_per_sec: Option<f64>,
    /// `None` when the platform's collector did not capture it.
    pub thermal_throttling: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct RunPage {
    pub items: Vec<RunSummary>,
    /// Position of the last item, present only when more runs match.
    pub next_cursor: Option<RunCursor>,
}

fn row_to_summary(row: sqlx::sqlite::SqliteRow) -> Result<RunSummary, RunsError> {
    let id: String = row.try_get("id")?;
    let model_asset_id: String = row.try_get("model_asset_id")?;
    let exit_status: String = row.try_get("exit_status")?;
    let correctness_result: String = row.try_get("correctness_result")?;
    let platform: String = row.try_get("platform")?;
    let device_class: String = row.try_get("device_class")?;
    Ok(RunSummary {
        id: Uuid::parse_str(&id).map_err(|err| RunsError(format!("invalid run id: {err}")))?,
        started_at: row.try_get("started_at")?,
        finished_at: row.try_get("finished_at")?,
        repetition: row.try_get("repetition")?,
        platform: Platform::try_from(platform.as_str())
            .map_err(|err| RunsError(format!("invalid stored platform: {err}")))?,
        device_class: DeviceClass::try_from(device_class.as_str())
            .map_err(|err| RunsError(format!("invalid stored device_class: {err}")))?,
        device_serial: row.try_get("device_serial")?,
        device_model: row.try_get("device_model")?,
        git_commit_sha: row.try_get("git_commit_sha")?,
        git_dirty: row.try_get::<i64, _>("git_dirty")? != 0,
        git_branch: row.try_get("git_branch")?,
        sumd_driver_version: row.try_get("sumd_driver_version")?,
        bsp_version: row.try_get("bsp_version")?,
        host_accelerator: row.try_get("host_accelerator")?,
        model_asset_id: Uuid::parse_str(&model_asset_id)
            .map_err(|err| RunsError(format!("invalid stored model_asset_id: {err}")))?,
        model_original_name: row
            .try_get::<Option<String>, _>("model_original_name")?
            .unwrap_or_default(),
        exit_status: ExitStatus::try_from(exit_status.as_str())
            .map_err(|err| RunsError(format!("invalid stored exit_status: {err}")))?,
        correctness_result: CorrectnessResult::try_from(correctness_result.as_str())
            .map_err(|err| RunsError(format!("invalid stored correctness_result: {err}")))?,
        prefill_tokens_per_sec: row.try_get("prefill_tokens_per_sec")?,
        decode_tokens_per_sec: row.try_get("decode_tokens_per_sec")?,
        thermal_throttling: optional_flag(&row, "thermal_throttling")?,
    })
}

/// Lists runs newest first (`started_at DESC, id DESC`), returning up to
/// `limit` summaries and a cursor when more match. Keyset pagination on
/// `(started_at, id)` is stable under concurrent inserts: new runs sort
/// above any cursor already handed out, so later pages neither skip nor
/// repeat runs.
pub async fn list_runs(
    pool: &SqlitePool,
    filter: &RunListFilter,
    limit: usize,
    cursor: Option<&RunCursor>,
) -> Result<RunPage, RunsError> {
    let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
        "SELECT r.id, r.started_at, r.finished_at, r.repetition, r.platform, r.device_class,
                r.device_serial, r.device_model,
                r.git_commit_sha, r.git_dirty, r.git_branch, r.sumd_driver_version,
                r.bsp_version, r.host_accelerator, r.model_asset_id,
                m.original_name AS model_original_name,
                r.exit_status, r.correctness_result, r.prefill_tokens_per_sec,
                r.decode_tokens_per_sec, r.thermal_throttling
         FROM runs r
         LEFT JOIN model_assets m ON m.id = r.model_asset_id
         WHERE 1 = 1",
    );
    filter.push_where_clauses(&mut qb);
    if let Some(cursor) = cursor {
        qb.push(" AND (r.started_at < ")
            .push_bind(cursor.started_at)
            .push(" OR (r.started_at = ")
            .push_bind(cursor.started_at)
            .push(" AND r.id < ")
            .push_bind(cursor.id.to_string())
            .push("))");
    }
    qb.push(" ORDER BY r.started_at DESC, r.id DESC LIMIT ")
        .push_bind((limit + 1) as i64);

    let rows = qb.build().fetch_all(pool).await?;
    let mut items = rows
        .into_iter()
        .map(row_to_summary)
        .collect::<Result<Vec<_>, _>>()?;

    let next_cursor = if items.len() > limit {
        items.truncate(limit);
        items.last().map(|last| RunCursor {
            started_at: last.started_at,
            id: last.id,
        })
    } else {
        None
    };

    Ok(RunPage { items, next_cursor })
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    pub fn minimal_new_run(id: Uuid, model_asset_id: Uuid) -> NewRun {
        NewRun {
            id,
            started_at: DateTime::parse_from_rfc3339("2026-09-01T00:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            finished_at: None,
            repetition: 0,
            command_args_json: "[]".to_string(),
            command_line: None,
            input_parameters_json: "{}".to_string(),
            env_vars_json: "{}".to_string(),
            env_allowlist_version: "v1".to_string(),
            collector_version: "collector-0.1".to_string(),

            device_class: DeviceClass::Internal,
            device_serial: "device-001".to_string(),
            device_model: None,
            host: HostState::Android(AndroidDeviceState::internal(
                AndroidLabConfig {
                    bsp_version: "bsp-1.0".to_string(),
                    sumd_driver_version: "sumd-1.0".to_string(),
                    gpu_clock_mhz: 980,
                    mif_clock_mhz: 5333,
                    int_clock_mhz: 934,
                },
                100,
                false,
                20.0,
                25.0,
                false,
            )),

            git_commit_sha: "abc123".to_string(),
            git_dirty: false,
            git_branch: None,
            git_commit_timestamp: None,
            git_commit_subject: None,
            executable_sha256: Some(Sha256Hex::try_from("a".repeat(64)).unwrap()),
            model_asset_id,
            prompt_sha256: Sha256Hex::try_from("c".repeat(64)).unwrap(),
            input_token_count: 10,
            output_token_count: 20,

            prefill_tokens_per_sec: 100.0,
            decode_tokens_per_sec: Some(50.0),
            exit_status: ExitStatus::Succeeded,
            correctness_result: CorrectnessResult::NotChecked,
            input_artifact_id: None,
            output_artifact_id: None,
            output_preview: None,
            stdout_artifact_id: None,
            stderr_artifact_id: None,
            crash_artifact_id: None,
            error_summary: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::minimal_new_run;
    use super::*;
    use crate::db;
    use crate::model_registry::{ExternalModelStorage, ModelStorage};

    struct TestCtx {
        pool: SqlitePool,
        _dir: tempfile::TempDir,
    }

    async fn test_ctx() -> TestCtx {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let db_path = dir.path().join("benchmarks.sqlite3");
        let database_url = format!("sqlite://{}", db_path.display());
        let pool = db::connect_and_migrate(&database_url)
            .await
            .expect("failed to connect and migrate");
        TestCtx { pool, _dir: dir }
    }

    async fn register_test_model(ctx: &TestCtx) -> Uuid {
        let model_path = ctx._dir.path().join("model.pte");
        std::fs::write(&model_path, b"fake model bytes").unwrap();
        let asset = ExternalModelStorage
            .register(&ctx.pool, &model_path)
            .await
            .expect("model registration should succeed");
        asset.id
    }

    #[tokio::test]
    async fn a_complete_run_round_trips() {
        let ctx = test_ctx().await;
        let model_asset_id = register_test_model(&ctx).await;
        let id = Uuid::now_v7();
        let new_run = minimal_new_run(id, model_asset_id);

        insert_run(&ctx.pool, &new_run)
            .await
            .expect("insert should succeed");
        let fetched = get_run(&ctx.pool, id)
            .await
            .expect("get should succeed")
            .expect("run should exist");

        assert_eq!(fetched.id, id);
        assert_eq!(fetched.device_serial, new_run.device_serial);
        let lab = fetched.host.android().and_then(|a| a.lab.as_ref()).expect("a lab android run");
        assert_eq!(lab.gpu_clock_mhz, 980);
        assert_eq!(lab.mif_clock_mhz, 5333);
        assert_eq!(lab.int_clock_mhz, 934);
        assert_eq!(fetched.exit_status, ExitStatus::Succeeded);
        assert_eq!(fetched.correctness_result, CorrectnessResult::NotChecked);
        assert_eq!(fetched.decode_tokens_per_sec, Some(50.0));
        assert_eq!(fetched.model_asset_id, model_asset_id);
    }

    #[tokio::test]
    async fn a_run_referencing_a_nonexistent_model_asset_is_rejected() {
        let ctx = test_ctx().await;
        let id = Uuid::now_v7();
        let new_run = minimal_new_run(id, Uuid::now_v7());

        let err = insert_run(&ctx.pool, &new_run)
            .await
            .expect_err("nonexistent model asset reference should be rejected");
        assert!(err.to_string().to_lowercase().contains("foreign key"));
    }


    #[tokio::test]
    async fn runs_list_newest_first_and_page_with_a_cursor() {
        let ctx = test_ctx().await;
        let model_asset_id = register_test_model(&ctx).await;
        let base = DateTime::parse_from_rfc3339("2026-09-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut ids = Vec::new();
        for i in 0..5 {
            let id = Uuid::now_v7();
            let mut run = minimal_new_run(id, model_asset_id);
            run.started_at = base + chrono::Duration::minutes(i);
            insert_run(&ctx.pool, &run).await.unwrap();
            ids.push(id);
        }

        let page = list_runs(&ctx.pool, &RunListFilter::default(), 2, None)
            .await
            .unwrap();
        assert_eq!(page.items.len(), 2);
        assert_eq!(page.items[0].id, ids[4]);
        assert_eq!(page.items[1].id, ids[3]);
        assert_eq!(page.items[0].model_original_name, "model.pte");
        let cursor = page.next_cursor.expect("more runs remain");

        let page = list_runs(&ctx.pool, &RunListFilter::default(), 2, Some(&cursor))
            .await
            .unwrap();
        assert_eq!(page.items[0].id, ids[2]);
        assert_eq!(page.items[1].id, ids[1]);
        let cursor = page.next_cursor.expect("one run remains");

        let page = list_runs(&ctx.pool, &RunListFilter::default(), 2, Some(&cursor))
            .await
            .unwrap();
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].id, ids[0]);
        assert!(page.next_cursor.is_none());
    }

    #[test]
    fn cursor_round_trips_through_its_token() {
        let cursor = RunCursor {
            started_at: DateTime::parse_from_rfc3339("2026-09-01T12:34:56.123456789Z")
                .unwrap()
                .with_timezone(&Utc),
            id: Uuid::now_v7(),
        };
        let token = cursor.encode();
        assert!(!token.contains('|'), "token must be opaque");
        assert_eq!(RunCursor::decode(&token).unwrap(), cursor);
    }

    #[test]
    fn cursor_rejects_malformed_tokens() {
        assert!(RunCursor::decode("not base64!!").is_err());
        let valid = RunCursor {
            started_at: Utc::now(),
            id: Uuid::now_v7(),
        }
        .encode();
        assert!(RunCursor::decode(&valid[..valid.len() / 2]).is_err());
        let garbage = URL_SAFE_NO_PAD.encode(b"no separator here");
        assert!(RunCursor::decode(&garbage).is_err());
        let bad_ts = URL_SAFE_NO_PAD.encode(format!("yesterday|{}", Uuid::now_v7()).as_bytes());
        assert!(RunCursor::decode(&bad_ts).is_err());
        let bad_id = URL_SAFE_NO_PAD.encode(b"2026-09-01T00:00:00Z|not-a-uuid");
        assert!(RunCursor::decode(&bad_id).is_err());
    }

    #[test]
    fn output_preview_truncates_at_a_char_boundary() {
        let full = "a".repeat(100);
        let preview = output_preview(&full, 10);
        assert_eq!(preview.chars().count(), 10);
        assert!(full.len() > preview.len());
    }

    #[test]
    fn output_preview_never_splits_a_multibyte_character() {
        let full = "€".repeat(50); // each € is 3 bytes in UTF-8
        let preview = output_preview(&full, 5);
        assert_eq!(preview.chars().count(), 5);
        assert!(std::str::from_utf8(preview.as_bytes()).is_ok());
    }
}
