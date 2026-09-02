use executorch_bencher::artifact_store::{ArtifactKind, store_artifact_bytes};
use executorch_bencher::config::Config;
use executorch_bencher::db;
use executorch_bencher::domain::{CorrectnessResult, DeviceClass, ExitStatus, Sha256Hex};
use executorch_bencher::model_registry::{ExternalModelStorage, ModelStorage};
use executorch_bencher::runs::{AndroidDeviceState, AndroidLabConfig, HostState, NewRun, get_run, insert_run};
use uuid::Uuid;

#[tokio::main]
async fn main() {
    let config = Config::from_env().expect("failed to load config from .env");
    config
        .prepare_storage_roots()
        .expect("failed to prepare storage roots");
    let pool = db::connect_and_migrate(&config.database_url)
        .await
        .expect("failed to connect and migrate");

    // Register the model this run exercised. The source file deliberately
    // lives *outside* the managed model root (external mode registers a
    // model wherever it already is, and never copies it in) - registration
    // is also idempotent (deduplicated by SHA-256), so re-running this
    // example never creates a second `model_assets` row for the same file.
    let external_models_dir = config
        .data_root
        .join("external-models-not-managed-by-this-service");
    std::fs::create_dir_all(&external_models_dir).expect("failed to create external models dir");
    let model_path = external_models_dir.join("e2e-example-model.pte");
    std::fs::write(&model_path, b"pretend .pte model bytes").expect("failed to write model file");
    let model_asset = ExternalModelStorage
        .register(&pool, &model_path)
        .await
        .expect("failed to register model asset");
    assert!(
        !config.model_root.exists()
            || std::fs::read_dir(&config.model_root)
                .unwrap()
                .next()
                .is_none(),
        "external-mode registration must never copy the model into the managed model root"
    );

    let stdout_artifact = store_artifact_bytes(
        &pool,
        &config.artifact_root,
        &config.temporary_dir,
        ArtifactKind::Stdout,
        Some("stdout.log"),
        Some("text/plain"),
        b"prefill: 120.4 tok/s\ndecode: 38.2 tok/s\n".to_vec(),
    )
    .await
    .expect("failed to store stdout artifact");

    let output_text = "generated output: the quick brown fox jumps over the lazy dog";
    let output_artifact = store_artifact_bytes(
        &pool,
        &config.artifact_root,
        &config.temporary_dir,
        ArtifactKind::Output,
        None,
        Some("text/plain"),
        output_text.as_bytes().to_vec(),
    )
    .await
    .expect("failed to store output artifact");

    let commit_time = chrono::Utc::now() - chrono::Duration::days(1);
    let id = Uuid::now_v7();
    let new_run = NewRun {
        id,
        started_at: chrono::Utc::now(),
        finished_at: Some(chrono::Utc::now()),
        repetition: 0,
        command_args_json: r#"["--model","resnet50","--reps","1"]"#.to_string(),
        command_line: Some("./run.sh --model resnet50 --reps 1".to_string()),
        input_parameters_json: r#"{"batch_size":1}"#.to_string(),
        env_vars_json: r#"{"EXPERIMENT_ID":"e2e-example"}"#.to_string(),
        env_allowlist_version: "v1".to_string(),
        collector_version: "collector-0.1".to_string(),

        device_class: DeviceClass::Internal,
        device_serial: "e2e-device-001".to_string(),
        device_model: None,
        host: HostState::Android(AndroidDeviceState::internal(
            AndroidLabConfig {
                bsp_version: "bsp-1.0".to_string(),
                sumd_driver_version: "sumd-1.0".to_string(),
                gpu_clock_mhz: 980,
                mif_clock_mhz: 5333,
                int_clock_mhz: 934,
            },
            3600,
            true,
            32.5,
            41.0,
            false,
        )),

        git_commit_sha: "e2e0000000000000000000000000000deadbeef".to_string(),
        git_dirty: false,
        git_branch: Some("main".to_string()),
        git_commit_timestamp: Some(commit_time),
        git_commit_subject: Some("e2e example commit".to_string()),
        executable_sha256: Some(Sha256Hex::try_from("a".repeat(64)).unwrap()),
        model_asset_id: model_asset.id,
        prompt_sha256: Sha256Hex::try_from("c".repeat(64)).unwrap(),
        input_token_count: 128,
        output_token_count: 64,

        prefill_tokens_per_sec: 120.4,
        decode_tokens_per_sec: Some(38.2),
        exit_status: ExitStatus::Succeeded,
        correctness_result: CorrectnessResult::Passed,
        input_artifact_id: None,
        output_artifact_id: Some(output_artifact.id),
        output_preview: Some(executorch_bencher::runs::output_preview(
            output_text,
            config.limits.output_preview_length,
        )),
        stdout_artifact_id: Some(stdout_artifact.id),
        stderr_artifact_id: None,
        crash_artifact_id: None,
        error_summary: None,
    };

    insert_run(&pool, &new_run)
        .await
        .expect("failed to insert run");
    println!("inserted run {id}");

    let fetched = get_run(&pool, id)
        .await
        .expect("failed to query run")
        .expect("run should exist after insert");

    println!("retrieved run {}", fetched.id);
    println!("  device_serial: {}", fetched.device_serial);
    println!(
        "  prefill/decode tok/s: {} / {:?}",
        fetched.prefill_tokens_per_sec, fetched.decode_tokens_per_sec
    );
    println!(
        "  exit_status={} correctness_result={}",
        fetched.exit_status.as_str(),
        fetched.correctness_result.as_str()
    );
    println!("  model_asset_id: {}", fetched.model_asset_id);
    println!("  stdout_artifact_id: {:?}", fetched.stdout_artifact_id);
    println!("  output_preview: {:?}", fetched.output_preview);

    assert_eq!(fetched.id, id);
    assert_eq!(fetched.model_asset_id, model_asset.id);
    assert_eq!(fetched.stdout_artifact_id, Some(stdout_artifact.id));
    assert_eq!(fetched.output_artifact_id, Some(output_artifact.id));
    println!("end-to-end insertion and retrieval succeeded");

    // Extra runs so the dashboard's results page has something to group:
    // two more repetitions of the same configuration, a newer commit with a
    // clean and a dirty run, a crashed run, and a run without a decode
    // measurement. Every run reuses the same registered model asset.
    let mut extra: Vec<NewRun> = Vec::new();
    for (i, prefill) in [118.9, 123.7].into_iter().enumerate() {
        let mut run = new_run.clone();
        run.id = Uuid::now_v7();
        run.repetition = i as i64 + 1;
        run.prefill_tokens_per_sec = prefill;
        run.output_artifact_id = None;
        run.output_preview = None;
        run.stdout_artifact_id = None;
        extra.push(run);
    }
    let newer_commit = "e2e1111111111111111111111111111cafef00d".to_string();
    for (dirty, prefill, decode) in [(false, 131.2, Some(39.9)), (true, 133.0, None)] {
        let mut run = new_run.clone();
        run.id = Uuid::now_v7();
        run.git_commit_sha = newer_commit.clone();
        run.git_dirty = dirty;
        run.git_commit_timestamp = Some(chrono::Utc::now() - chrono::Duration::hours(1));
        run.git_commit_subject = Some("e2e example: faster prefill".to_string());
        run.prefill_tokens_per_sec = prefill;
        run.decode_tokens_per_sec = decode;
        run.output_artifact_id = None;
        run.output_preview = None;
        run.stdout_artifact_id = None;
        extra.push(run);
    }
    let mut crashed = new_run.clone();
    crashed.id = Uuid::now_v7();
    crashed.git_commit_sha = newer_commit.clone();
    crashed.git_commit_timestamp = Some(chrono::Utc::now() - chrono::Duration::hours(1));
    crashed.exit_status = ExitStatus::Crashed;
    crashed.correctness_result = CorrectnessResult::NotChecked;
    if let HostState::Android(a) = &mut crashed.host {
        a.thermal_throttling = Some(true);
    }
    crashed.error_summary = Some("SIGSEGV in prefill kernel".to_string());
    crashed.output_artifact_id = None;
    crashed.output_preview = None;
    crashed.stdout_artifact_id = None;
    extra.push(crashed);
    for run in &extra {
        insert_run(&pool, run).await.expect("failed to insert extra run");
    }
    println!("inserted {} extra runs for the dashboard", extra.len());
}
