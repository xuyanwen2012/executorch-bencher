//! Imports one `llama_main` benchmark log (a sequence of `=== <tag> rep<N> ===`
//! markers each followed by a `PyTorchObserver {json}` line) into the
//! database, using a JSON manifest for everything the log does not carry:
//! host identity and hardware, git provenance, the runner binary's hash
//! when it was preserved, prompt and model identities, and the command
//! template. See `openspec/changes/support-linux-hosts/design.md` -
//! "Import via manifest + log, idempotent" and `imports/README.md`.
//!
//! Usage: `cargo run --bin import-observer-log -- <manifest.json>` with the
//! target profile's `.env` loaded (`just import-log <manifest>`).
//!
//! Idempotent: a run is identified by `(log sha256, tag, repetition)` and
//! skipped when already present. Values the manifest marks unknown are
//! stored as null, never guessed.

use chrono::{DateTime, Utc};
use executorch_bencher::artifact_store::{ArtifactKind, store_artifact_bytes};
use executorch_bencher::config::Config;
use executorch_bencher::db;
use executorch_bencher::domain::{CorrectnessResult, DeviceClass, ExitStatus, Platform, Sha256Hex};
use executorch_bencher::model_registry::{
    ExternalModelStorage, ModelAsset, ModelStorage, find_by_sha256,
};
use executorch_bencher::runs::{
    AndroidDeviceState, HostState, LinuxHostState, NewRun, insert_run,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{Row, SqlitePool};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use uuid::Uuid;

#[derive(Deserialize)]
struct Manifest {
    collector_version: String,
    /// Log path relative to the manifest's directory.
    log: String,
    benchmark: String,
    host: HostSpec,
    git: GitSpec,
    executable: ExecutableSpec,
    prompt: PromptSpec,
    /// Runner arguments with `{model}`, `{model_on_device}`, `{tokenizer}`,
    /// `{prompt_path}`, and `{prompt_text}` placeholders.
    args_template: Vec<String>,
    runner: RunnerSpec,
    models: BTreeMap<String, ModelSpec>,
    #[serde(default)]
    skip_tags: BTreeMap<String, String>,
    /// Repetitions whose marker has no observer line: what happened to
    /// them. A marker with no observer line and no entry here is an error.
    #[serde(default)]
    failures: Vec<FailureSpec>,
    #[serde(default)]
    input_parameters: serde_json::Map<String, Value>,
    #[serde(default)]
    notes: Option<String>,
}

/// The host a log came from. `platform` defaults to `linux` and
/// `device_class` to `external`, so the original Linux manifests stay
/// valid. On Android, `hostname` holds the device serial.
#[derive(Deserialize)]
struct HostSpec {
    #[serde(default = "default_platform")]
    platform: String,
    #[serde(default = "default_device_class")]
    device_class: String,
    hostname: String,
    #[serde(default)]
    device_model: Option<String>,
    os: Option<String>,
    kernel: Option<String>,
    /// CPU model on Linux; SoC model on Android.
    cpu_model: Option<String>,
    cpu_count: Option<i64>,
    memory_bytes: Option<i64>,
    accelerator: Option<String>,
    accelerator_driver: Option<String>,
    #[serde(default)]
    vulkan_api_version: Option<String>,
    /// Free-form facts about the host worth keeping with every run
    /// (e.g. rooted: false, build fingerprint).
    #[serde(default)]
    extra: serde_json::Map<String, Value>,
}

fn default_platform() -> String {
    "linux".to_string()
}

fn default_device_class() -> String {
    "external".to_string()
}

/// A repetition that produced no observer line.
#[derive(Deserialize)]
struct FailureSpec {
    tag: String,
    rep: i64,
    /// One of the stable `ExitStatus` values.
    exit_status: String,
    error_summary: String,
    /// When known. Otherwise the last timestamp seen in the log before the
    /// marker is used and flagged as estimated.
    #[serde(default)]
    started_at: Option<DateTime<Utc>>,
}

#[derive(Deserialize)]
struct GitSpec {
    commit_sha: String,
    branch: Option<String>,
    commit_timestamp: Option<DateTime<Utc>>,
    commit_subject: Option<String>,
    dirty: bool,
    #[serde(default)]
    modified_files: Vec<String>,
    #[serde(default)]
    notes: Option<String>,
}

#[derive(Deserialize)]
struct ExecutableSpec {
    path: String,
    sha256: Option<String>,
    #[serde(default)]
    note: Option<String>,
}

#[derive(Deserialize)]
struct RunnerSpec {
    executable_name: String,
    #[serde(default)]
    tokenizer_sha256: Option<String>,
    #[serde(default)]
    stdout_capture: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum PromptSpec {
    File {
        /// Relative to the manifest's directory.
        file: String,
        path_on_host: String,
        #[serde(default)]
        sha256: Option<String>,
        /// The prompt's token count under the runner's tokenizer, for reps
        /// that failed before reporting one. A property of the input, not
        /// a measurement; omit it and failed reps record 0.
        #[serde(default)]
        token_count: Option<i64>,
    },
    Text {
        text: String,
        #[serde(default)]
        token_count: Option<i64>,
    },
}

impl PromptSpec {
    fn token_count(&self) -> Option<i64> {
        match self {
            PromptSpec::File { token_count, .. } | PromptSpec::Text { token_count, .. } => *token_count,
        }
    }
}

#[derive(Deserialize)]
struct ModelSpec {
    /// Where the file can be read for registration (host side).
    path: String,
    /// The path the runner was actually given, when it differs (a phone's
    /// on-device copy). Fills `{model_on_device}` in the args template.
    #[serde(default)]
    on_device_path: Option<String>,
    sha256: String,
    tokenizer_path: String,
    #[serde(default)]
    export: Value,
}

/// The `PyTorchObserver` payload `llama_main` prints. Times are epoch ms.
#[derive(Deserialize, Clone)]
struct Observer {
    prefill_token_per_sec: f64,
    #[serde(default)]
    decode_token_per_sec: Option<f64>,
    prompt_tokens: i64,
    generated_tokens: i64,
    model_load_start_ms: i64,
    inference_end_ms: i64,
}

struct Record {
    tag: String,
    rep: i64,
    line_number: usize,
    /// The captured observer line, or `None` when the marker was followed
    /// by nothing (the rep produced no output: crash, reboot, unreachable).
    raw_line: Option<String>,
    observer: Option<Observer>,
}

fn parse_log(text: &str) -> Result<Vec<Record>, String> {
    let mut records = Vec::new();
    let mut pending: Option<(String, i64, usize)> = None;
    for (idx, line) in text.lines().enumerate() {
        let n = idx + 1;
        if let Some(inner) = line.strip_prefix("=== ").and_then(|l| l.strip_suffix(" ===")) {
            if let Some((tag, rep, at)) = pending.take() {
                records.push(Record {
                    tag,
                    rep,
                    line_number: at,
                    raw_line: None,
                    observer: None,
                });
            }
            match inner.rsplit_once(" rep") {
                Some((tag, rep)) => {
                    let rep: i64 = rep
                        .parse()
                        .map_err(|_| format!("line {n}: bad repetition in marker {line:?}"))?;
                    pending = Some((tag.to_string(), rep, n));
                }
                // `=== BENCH ALL DONE ===` / `=== DONE ===` trailers.
                None => continue,
            }
        } else if let Some(json) = line.strip_prefix("PyTorchObserver ") {
            let (tag, rep, at) = pending
                .take()
                .ok_or_else(|| format!("line {n}: observer line without a preceding marker"))?;
            let observer: Observer = serde_json::from_str(json)
                .map_err(|err| format!("line {n}: bad observer JSON: {err}"))?;
            records.push(Record {
                tag,
                rep,
                line_number: at,
                raw_line: Some(line.to_string()),
                observer: Some(observer),
            });
        } else if !line.trim().is_empty() {
            return Err(format!("line {n}: unrecognized line {line:?}"));
        }
    }
    if let Some((tag, rep, at)) = pending {
        records.push(Record {
            tag,
            rep,
            line_number: at,
            raw_line: None,
            observer: None,
        });
    }
    Ok(records)
}

fn host_state(h: &HostSpec, platform: Platform) -> Result<HostState, String> {
    match platform {
        Platform::Linux => {
            let need = |v: &Option<String>, name: &str| {
                v.clone()
                    .ok_or_else(|| format!("linux host is missing host.{name}"))
            };
            Ok(HostState::Linux(LinuxHostState {
                os: need(&h.os, "os")?,
                kernel: need(&h.kernel, "kernel")?,
                cpu_model: need(&h.cpu_model, "cpu_model")?,
                cpu_count: h.cpu_count,
                memory_bytes: h.memory_bytes,
                accelerator: need(&h.accelerator, "accelerator")?,
                accelerator_driver: h.accelerator_driver.clone(),
                uptime_seconds: None,
                thermal_throttling: None,
            }))
        }
        Platform::Android => Ok(HostState::Android(AndroidDeviceState {
            os: h.os.clone(),
            kernel: h.kernel.clone(),
            soc: h.cpu_model.clone(),
            cpu_count: h.cpu_count,
            memory_bytes: h.memory_bytes,
            gpu: h.accelerator.clone(),
            gpu_driver: h.accelerator_driver.clone(),
            ..Default::default()
        })),
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

fn epoch_ms(ms: i64) -> Result<DateTime<Utc>, String> {
    DateTime::<Utc>::from_timestamp_millis(ms).ok_or_else(|| format!("bad epoch ms {ms}"))
}

async fn resolve_model(pool: &SqlitePool, tag: &str, spec: &ModelSpec) -> Result<ModelAsset, String> {
    if let Some(asset) = find_by_sha256(pool, &spec.sha256)
        .await
        .map_err(|err| format!("model lookup failed: {err}"))?
    {
        return Ok(asset);
    }
    let path = Path::new(&spec.path);
    if !path.is_file() {
        return Err(format!(
            "model {tag} (sha256 {}) is not registered and {} is not readable here; register it first",
            spec.sha256,
            path.display()
        ));
    }
    eprintln!("registering model {tag} from {} (hashing)...", path.display());
    let asset = ExternalModelStorage
        .register(pool, path)
        .await
        .map_err(|err| format!("model registration failed for {tag}: {err}"))?;
    if asset.sha256 != spec.sha256 {
        return Err(format!(
            "model {tag}: file at {} hashes to {} but the manifest says {}",
            path.display(),
            asset.sha256,
            spec.sha256
        ));
    }
    Ok(asset)
}

async fn already_imported(
    pool: &SqlitePool,
    log_sha256: &str,
    tag: &str,
    repetition: i64,
) -> Result<bool, String> {
    let row = sqlx::query(
        "SELECT count(*) AS c FROM runs
         WHERE json_extract(input_parameters, '$.import.log_sha256') = ?
           AND json_extract(input_parameters, '$.import.tag') = ?
           AND repetition = ?",
    )
    .bind(log_sha256)
    .bind(tag)
    .bind(repetition)
    .fetch_one(pool)
    .await
    .map_err(|err| format!("duplicate check failed: {err}"))?;
    Ok(row.get::<i64, _>("c") > 0)
}

async fn run(manifest_path: &Path) -> Result<(), String> {
    let config = Config::from_env().map_err(|err| format!("configuration error: {err}"))?;
    config
        .prepare_storage_roots()
        .map_err(|err| format!("storage configuration error: {err}"))?;
    let pool = db::connect_and_migrate(&config.database_url)
        .await
        .map_err(|err| format!("database error: {err}"))?;

    let manifest_dir = manifest_path.parent().unwrap_or(Path::new("."));
    let manifest_text = std::fs::read_to_string(manifest_path)
        .map_err(|err| format!("cannot read {}: {err}", manifest_path.display()))?;
    let manifest: Manifest = serde_json::from_str(&manifest_text)
        .map_err(|err| format!("{} is not a valid manifest: {err}", manifest_path.display()))?;
    let manifest_name = manifest_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let platform = Platform::try_from(manifest.host.platform.as_str())
        .map_err(|err| format!("host.platform: {err}"))?;
    let device_class = DeviceClass::try_from(manifest.host.device_class.as_str())
        .map_err(|err| format!("host.device_class: {err}"))?;
    if device_class == DeviceClass::Internal {
        return Err("this importer only records external hosts; internal lab devices need the full snapshot a collector captures".into());
    }
    let host = host_state(&manifest.host, platform)?;

    let log_path: PathBuf = manifest_dir.join(&manifest.log);
    let log_bytes =
        std::fs::read(&log_path).map_err(|err| format!("cannot read {}: {err}", log_path.display()))?;
    let log_sha256 = sha256_hex(&log_bytes);
    let log_text = String::from_utf8(log_bytes).map_err(|_| "log is not UTF-8".to_string())?;
    let records = parse_log(&log_text)?;
    let log_name = log_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();

    // Prompt: file or literal; stored once as a `prompt` artifact.
    let (prompt_bytes, prompt_path_on_host, prompt_text) = match &manifest.prompt {
        PromptSpec::File {
            file,
            path_on_host,
            sha256,
            ..
        } => {
            let p = manifest_dir.join(file);
            let bytes = std::fs::read(&p).map_err(|err| format!("cannot read {}: {err}", p.display()))?;
            if let Some(expected) = sha256 {
                let actual = sha256_hex(&bytes);
                if &actual != expected {
                    return Err(format!(
                        "prompt file {} hashes to {actual}, manifest says {expected}",
                        p.display()
                    ));
                }
            }
            (bytes, Some(path_on_host.clone()), None)
        }
        PromptSpec::Text { text, .. } => (text.as_bytes().to_vec(), None, Some(text.clone())),
    };
    let prompt_sha256 = Sha256Hex::try_from(sha256_hex(&prompt_bytes))
        .map_err(|err| format!("prompt hash: {err}"))?;
    let prompt_artifact = store_artifact_bytes(
        &pool,
        &config.artifact_root,
        &config.temporary_dir,
        ArtifactKind::Prompt,
        Some(if prompt_text.is_some() { "prompt.txt" } else { "prompt_2048.txt" }),
        Some("text/plain"),
        prompt_bytes,
    )
    .await
    .map_err(|err| format!("storing prompt artifact: {err}"))?;

    let executable_sha256 = manifest
        .executable
        .sha256
        .clone()
        .map(Sha256Hex::try_from)
        .transpose()
        .map_err(|err| format!("executable sha256: {err}"))?;

    let mut models: BTreeMap<String, ModelAsset> = BTreeMap::new();
    let mut inserted = 0usize;
    let mut skipped_existing = 0usize;
    let mut skipped_tags = 0usize;

    for record in &records {
        if let Some(reason) = manifest.skip_tags.get(&record.tag) {
            eprintln!("skip {} rep{}: {reason}", record.tag, record.rep);
            skipped_tags += 1;
            continue;
        }
        let repetition = record.rep - 1;
        if repetition < 0 {
            return Err(format!("{} rep{}: repetitions start at 1", record.tag, record.rep));
        }
        if already_imported(&pool, &log_sha256, &record.tag, repetition).await? {
            skipped_existing += 1;
            continue;
        }
        let spec = manifest
            .models
            .get(&record.tag)
            .ok_or_else(|| format!("tag {} has no entry in manifest.models", record.tag))?;
        if !models.contains_key(&record.tag) {
            let asset = resolve_model(&pool, &record.tag, spec).await?;
            models.insert(record.tag.clone(), asset);
        }
        let asset = &models[&record.tag];

        let failure = match &record.observer {
            Some(_) => None,
            None => Some(
                manifest
                    .failures
                    .iter()
                    .find(|f| f.tag == record.tag && f.rep == record.rep)
                    .ok_or_else(|| {
                        format!(
                            "{} rep{} (line {}) has no observer line and no `failures` entry in the manifest",
                            record.tag, record.rep, record.line_number
                        )
                    })?,
            ),
        };

        let args: Vec<String> = manifest
            .args_template
            .iter()
            .map(|a| {
                a.replace("{model}", &spec.path)
                    .replace(
                        "{model_on_device}",
                        spec.on_device_path.as_deref().unwrap_or(&spec.path),
                    )
                    .replace("{tokenizer}", &spec.tokenizer_path)
                    .replace("{prompt_path}", prompt_path_on_host.as_deref().unwrap_or(""))
                    .replace("{prompt_text}", prompt_text.as_deref().unwrap_or(""))
            })
            .collect();
        let command_line = format!("{} {}", manifest.runner.executable_name, args.join(" "));

        let stdout_artifact = match &record.raw_line {
            Some(raw_line) => Some(
                store_artifact_bytes(
                    &pool,
                    &config.artifact_root,
                    &config.temporary_dir,
                    ArtifactKind::Stdout,
                    Some(&format!("{}-rep{}.stdout.txt", record.tag, record.rep)),
                    Some("text/plain"),
                    format!("{raw_line}\n").into_bytes(),
                )
                .await
                .map_err(|err| format!("storing stdout artifact: {err}"))?,
            ),
            None => None,
        };

        let observer_json: Value = match &record.raw_line {
            Some(raw_line) => serde_json::from_str(
                raw_line.strip_prefix("PyTorchObserver ").unwrap_or(raw_line),
            )
            .map_err(|err| format!("observer JSON: {err}"))?,
            None => Value::Null,
        };
        let mut input_parameters = manifest.input_parameters.clone();
        if !manifest.host.extra.is_empty() {
            input_parameters.insert("host".into(), Value::Object(manifest.host.extra.clone()));
        }
        input_parameters.insert("benchmark".into(), json!(manifest.benchmark));
        input_parameters.insert("tag".into(), json!(record.tag));
        input_parameters.insert("model_export".into(), spec.export.clone());
        input_parameters.insert("observer".into(), observer_json);
        input_parameters.insert(
            "import".into(),
            json!({
                "manifest": manifest_name,
                "log": log_name,
                "log_sha256": log_sha256,
                "line": record.line_number,
                "tag": record.tag,
                "rep": record.rep,
            }),
        );
        input_parameters.insert(
            "executable".into(),
            json!({"path": manifest.executable.path, "note": manifest.executable.note}),
        );
        input_parameters.insert(
            "git_modified_files".into(),
            json!(manifest.git.modified_files),
        );
        if let Some(n) = &manifest.git.notes {
            input_parameters.insert("git_notes".into(), json!(n));
        }
        if let Some(v) = &manifest.host.vulkan_api_version {
            input_parameters.insert("vulkan_api_version".into(), json!(v));
        }
        if let Some(t) = &manifest.runner.tokenizer_sha256 {
            input_parameters.insert("tokenizer_sha256".into(), json!(t));
        }
        if let Some(c) = &manifest.runner.stdout_capture {
            input_parameters.insert("stdout_capture".into(), json!(c));
        }
        if let Some(n) = &manifest.notes {
            input_parameters.insert("notes".into(), json!(n));
        }

        // Timing and outcome: from the observer line, or from the
        // manifest's failure entry for a rep that produced nothing.
        let (started_at, finished_at, exit_status, error_summary) = match (&record.observer, failure) {
            (Some(o), _) => (
                epoch_ms(o.model_load_start_ms)?,
                Some(epoch_ms(o.inference_end_ms)?),
                ExitStatus::Succeeded,
                None,
            ),
            (None, Some(f)) => {
                let exit_status = ExitStatus::try_from(f.exit_status.as_str())
                    .map_err(|err| format!("failures[{} rep{}]: {err}", f.tag, f.rep))?;
                let started_at = match f.started_at {
                    Some(t) => t,
                    None => {
                        let last_seen = records[..records.iter().position(|r| std::ptr::eq(r, record)).unwrap()]
                            .iter()
                            .rev()
                            .find_map(|r| r.observer.as_ref().map(|o| o.inference_end_ms))
                            .ok_or_else(|| {
                                format!(
                                    "{} rep{}: no timestamp precedes this failed rep; give failures[].started_at",
                                    record.tag, record.rep
                                )
                            })?;
                        input_parameters.insert(
                            "started_at_estimated".into(),
                            json!("no timestamp was captured for this failed repetition; started_at is the end of the last observed repetition before it"),
                        );
                        epoch_ms(last_seen)?
                    }
                };
                (started_at, None, exit_status, Some(f.error_summary.clone()))
            }
            (None, None) => unreachable!("checked above"),
        };
        let (prompt_tokens, generated_tokens, prefill, decode) = match &record.observer {
            Some(o) => (
                o.prompt_tokens,
                o.generated_tokens,
                o.prefill_token_per_sec,
                if o.generated_tokens > 0 { o.decode_token_per_sec } else { None },
            ),
            // Nothing was measured; the prompt's known token count is kept
            // (it is a property of the input), 0 is the floor for the
            // rest, and the exit status keeps the row out of every
            // statistic.
            None => (manifest.prompt.token_count().unwrap_or(0), 0, 0.0, None),
        };
        let new_run = NewRun {
            id: Uuid::now_v7(),
            started_at,
            finished_at,
            repetition,
            command_args_json: serde_json::to_string(&args).expect("args serialize"),
            command_line: Some(command_line),
            input_parameters_json: Value::Object(input_parameters).to_string(),
            env_vars_json: "{}".to_string(),
            env_allowlist_version: "none".to_string(),
            collector_version: manifest.collector_version.clone(),

            device_class,
            device_serial: manifest.host.hostname.clone(),
            device_model: manifest.host.device_model.clone(),
            host: host.clone(),

            git_commit_sha: manifest.git.commit_sha.clone(),
            git_dirty: manifest.git.dirty,
            git_branch: manifest.git.branch.clone(),
            git_commit_timestamp: manifest.git.commit_timestamp,
            git_commit_subject: manifest.git.commit_subject.clone(),
            executable_sha256: executable_sha256.clone(),
            model_asset_id: asset.id,
            prompt_sha256: prompt_sha256.clone(),
            input_token_count: prompt_tokens,
            output_token_count: generated_tokens,

            prefill_tokens_per_sec: prefill,
            // The runner prints `decode_token_per_sec: 0` (or omits it)
            // when it generated nothing; that is the absence of a decode
            // measurement, not a measured 0 tok/s, and must not enter the
            // decode median.
            decode_tokens_per_sec: decode,
            // A recorded observer line is the only success signal the
            // script kept; a rep with no line is described by `failures`.
            exit_status,
            correctness_result: CorrectnessResult::NotChecked,
            input_artifact_id: Some(prompt_artifact.id),
            output_artifact_id: None,
            output_preview: None,
            stdout_artifact_id: stdout_artifact.map(|a| a.id),
            stderr_artifact_id: None,
            crash_artifact_id: None,
            error_summary,
        };
        insert_run(&pool, &new_run)
            .await
            .map_err(|err| format!("{} rep{}: {err}", record.tag, record.rep))?;
        inserted += 1;
    }

    println!(
        "{manifest_name}: {} records, {inserted} inserted, {skipped_existing} already present, {skipped_tags} skipped by manifest",
        records.len()
    );
    Ok(())
}

#[tokio::main]
async fn main() -> ExitCode {
    let manifest = match std::env::args().nth(1) {
        Some(p) => PathBuf::from(p),
        None => {
            eprintln!("usage: import-observer-log <manifest.json>");
            return ExitCode::from(2);
        }
    };
    match run(&manifest).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("import failed: {err}");
            ExitCode::FAILURE
        }
    }
}
