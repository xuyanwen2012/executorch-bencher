mod common;

use executorch_bencher::artifact_store::{
    ArtifactKind, get_artifact_record, open_artifact_content, store_artifact_bytes,
};
use executorch_bencher::domain::{CorrectnessResult, ExitStatus, Sha256Hex};
use executorch_bencher::runs::{get_run, insert_run};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::process::Command;
use tokio::io::AsyncReadExt;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
struct MockResult {
    mode: String,
    answer: f64,
    expected_answer: f64,
    correctness_result: String,
    exit_status: String,
    error_summary: Option<String>,
    input_value_count: i64,
    elapsed_ms: i64,
    artifact_bundle: String,
    crash_log: Option<String>,
}

#[tokio::test]
async fn external_experiments_round_trip_inputs_parameters_logs_outcomes_and_artifacts() {
    let ctx = common::test_context().await;
    let model_asset_id = ctx.shared_test_model().await;
    let work = tempfile::tempdir().expect("temporary experiment workspace");
    let input_a = work.path().join("a.txt");
    let input_b = work.path().join("b.txt");
    std::fs::write(&input_a, "1 2 3\n").unwrap();
    std::fs::write(&input_b, "4 5\n").unwrap();
    let input_bytes = [
        std::fs::read(&input_a).unwrap(),
        std::fs::read(&input_b).unwrap(),
    ]
    .concat();
    let input_sha = Sha256::digest(&input_bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();

    for (repetition, mode) in ["success", "incorrect", "crash"].into_iter().enumerate() {
        let artifact_dir = work.path().join(format!("artifacts-{mode}"));
        let result_path = work.path().join(format!("result-{mode}.json"));
        let args = vec![
            "examples/mock_experiment.py".to_string(),
            "--input".into(),
            input_a.display().to_string(),
            "--input".into(),
            input_b.display().to_string(),
            "--scale".into(),
            "2.5".into(),
            "--bias".into(),
            "1".into(),
            "--mode".into(),
            mode.into(),
            "--artifact-dir".into(),
            artifact_dir.display().to_string(),
            "--result-json".into(),
            result_path.display().to_string(),
        ];
        let process = Command::new("python3")
            .args(&args)
            .output()
            .expect("run mock experiment");
        assert_eq!(process.status.success(), mode != "crash");

        let result: MockResult =
            serde_json::from_slice(&std::fs::read(&result_path).unwrap()).unwrap();
        assert_eq!(result.mode, mode);
        assert_eq!(result.input_value_count, 5);
        assert!(result.elapsed_ms >= 1);

        let input = store_artifact_bytes(
            &ctx.pool,
            &ctx.artifact_root,
            &ctx.temporary_dir,
            ArtifactKind::Prompt,
            Some("experiment-inputs.txt"),
            Some("text/plain"),
            input_bytes.clone(),
        )
        .await
        .unwrap();
        let stdout = store_artifact_bytes(
            &ctx.pool,
            &ctx.artifact_root,
            &ctx.temporary_dir,
            ArtifactKind::Stdout,
            Some(&format!("{mode}.stdout.log")),
            Some("text/plain"),
            process.stdout,
        )
        .await
        .unwrap();
        let stderr = store_artifact_bytes(
            &ctx.pool,
            &ctx.artifact_root,
            &ctx.temporary_dir,
            ArtifactKind::Stderr,
            Some(&format!("{mode}.stderr.log")),
            Some("text/plain"),
            process.stderr,
        )
        .await
        .unwrap();
        let bundle_bytes = std::fs::read(&result.artifact_bundle).unwrap();
        let bundle = store_artifact_bytes(
            &ctx.pool,
            &ctx.artifact_root,
            &ctx.temporary_dir,
            ArtifactKind::Output,
            Some(&format!("{mode}.artifacts.zip")),
            Some("application/zip"),
            bundle_bytes.clone(),
        )
        .await
        .unwrap();
        let crash = if let Some(path) = &result.crash_log {
            Some(
                store_artifact_bytes(
                    &ctx.pool,
                    &ctx.artifact_root,
                    &ctx.temporary_dir,
                    ArtifactKind::CrashLog,
                    Some("crash.log"),
                    Some("text/plain"),
                    std::fs::read(path).unwrap(),
                )
                .await
                .unwrap(),
            )
        } else {
            None
        };

        let mut run = common::build_new_linux_run(Uuid::now_v7(), model_asset_id, "mock-host");
        run.repetition = repetition as i64;
        run.command_args_json = serde_json::to_string(&args).unwrap();
        run.command_line = Some(format!("python3 {}", args.join(" ")));
        run.input_parameters_json = serde_json::json!({
            "input_files": [input_a, input_b], "scale": 2.5, "bias": 1,
            "result_manifest": result_path, "answer": result.answer, "expected_answer": result.expected_answer,
        }).to_string();
        run.collector_version = "mock-experiment-test/1".into();
        run.prompt_sha256 = Sha256Hex::try_from(input_sha.clone()).unwrap();
        run.input_token_count = result.input_value_count;
        run.output_token_count = 1;
        run.prefill_tokens_per_sec = 1000.0 / result.elapsed_ms as f64;
        run.decode_tokens_per_sec = None;
        run.exit_status = ExitStatus::try_from(result.exit_status.as_str()).unwrap();
        run.correctness_result =
            CorrectnessResult::try_from(result.correctness_result.as_str()).unwrap();
        run.input_artifact_id = Some(input.id);
        run.output_artifact_id = Some(bundle.id);
        run.stdout_artifact_id = Some(stdout.id);
        run.stderr_artifact_id = Some(stderr.id);
        run.crash_artifact_id = crash.as_ref().map(|a| a.id);
        run.output_preview = Some(format!(
            "answer={} expected={}",
            result.answer, result.expected_answer
        ));
        run.error_summary = result.error_summary.clone();
        insert_run(&ctx.pool, &run).await.unwrap();

        let stored = get_run(&ctx.pool, run.id).await.unwrap().unwrap();
        assert_eq!(stored.command_args_json, run.command_args_json);
        assert_eq!(stored.input_parameters_json, run.input_parameters_json);
        assert_eq!(
            stored.correctness_result.as_str(),
            result.correctness_result
        );
        assert_eq!(stored.exit_status.as_str(), result.exit_status);
        assert_eq!(stored.crash_artifact_id.is_some(), mode == "crash");
        assert_eq!(stored.input_artifact_id, Some(input.id));
        assert_eq!(stored.output_artifact_id, Some(bundle.id));
        assert_eq!(stored.stdout_artifact_id, Some(stdout.id));
        assert_eq!(stored.stderr_artifact_id, Some(stderr.id));

        let record = get_artifact_record(&ctx.pool, bundle.id)
            .await
            .unwrap()
            .unwrap();
        let mut reopened = open_artifact_content(&ctx.artifact_root, &record)
            .await
            .unwrap();
        let mut round_trip = Vec::new();
        reopened.read_to_end(&mut round_trip).await.unwrap();
        assert_eq!(round_trip, bundle_bytes);
        assert!(
            round_trip
                .windows(b"counter-000.json".len())
                .any(|w| w == b"counter-000.json")
        );
    }
}

#[test]
fn mock_experiment_handles_nested_outputs_and_does_not_bundle_stale_files() {
    let work = tempfile::tempdir().unwrap();
    let input = work.path().join("input.txt");
    let artifact_dir = work.path().join("shared-artifacts");
    std::fs::write(&input, "2 3\n").unwrap();
    std::fs::create_dir(&artifact_dir).unwrap();
    std::fs::write(
        artifact_dir.join("stale-secret.txt"),
        "must not be uploaded",
    )
    .unwrap();
    std::fs::write(artifact_dir.join("crash.log"), "stale crash").unwrap();
    let result = work.path().join("nested/results/result.json");

    let process = Command::new("python3")
        .args([
            "examples/mock_experiment.py",
            "--input",
            input.to_str().unwrap(),
            "--mode",
            "success",
            "--artifact-dir",
            artifact_dir.to_str().unwrap(),
            "--result-json",
            result.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        process.status.success(),
        "{}",
        String::from_utf8_lossy(&process.stderr)
    );

    let manifest: MockResult = serde_json::from_slice(&std::fs::read(result).unwrap()).unwrap();
    let bundle = std::fs::read(manifest.artifact_bundle).unwrap();
    assert!(
        bundle
            .windows(b"counter-001.json".len())
            .any(|w| w == b"counter-001.json")
    );
    assert!(
        !bundle
            .windows(b"stale-secret.txt".len())
            .any(|w| w == b"stale-secret.txt")
    );
    assert!(
        !bundle
            .windows(b"crash.log".len())
            .any(|w| w == b"crash.log")
    );
}
