mod common;

use executorch_bencher::artifact_store::{
    ArtifactKind, get_artifact_record, open_artifact_content, store_artifact_bytes,
};
use executorch_bencher::domain::{CorrectnessResult, ExitStatus};
use executorch_bencher::runs::{get_run, insert_run};
use tokio::io::AsyncReadExt;
use uuid::Uuid;

#[tokio::test]
async fn recording_a_crash_stores_a_resolvable_crash_log_artifact() {
    let ctx = common::test_context().await;

    let crash_log = store_artifact_bytes(
        &ctx.pool,
        &ctx.artifact_root,
        &ctx.temporary_dir,
        ArtifactKind::CrashLog,
        Some("crash.log"),
        Some("text/plain"),
        b"segfault at 0xdeadbeef".to_vec(),
    )
    .await
    .expect("storing crash log should succeed");

    let id = Uuid::now_v7();
    let mut new_run = common::seed_new_run(&ctx, id).await;
    new_run.exit_status = ExitStatus::Crashed;
    new_run.crash_artifact_id = Some(crash_log.id);
    new_run.error_summary = Some("segfault".to_string());

    insert_run(&ctx.pool, &new_run)
        .await
        .expect("insert should succeed");
    let fetched = get_run(&ctx.pool, id)
        .await
        .expect("get should succeed")
        .expect("run should exist");

    assert_eq!(fetched.exit_status, ExitStatus::Crashed);
    assert_eq!(fetched.crash_artifact_id, Some(crash_log.id));

    let record = get_artifact_record(&ctx.pool, crash_log.id)
        .await
        .expect("lookup should succeed")
        .expect("artifact record should exist");
    let mut content = open_artifact_content(&ctx.artifact_root, &record)
        .await
        .expect("crash log content should open");
    let mut contents = Vec::new();
    content
        .read_to_end(&mut contents)
        .await
        .expect("crash log should be readable");
    assert_eq!(contents, b"segfault at 0xdeadbeef");
}

#[tokio::test]
async fn a_run_attaches_input_output_stdout_stderr_and_crash_artifacts() {
    let ctx = common::test_context().await;

    async fn store(ctx: &common::TestContext, kind: ArtifactKind, content: &str) -> Uuid {
        store_artifact_bytes(
            &ctx.pool,
            &ctx.artifact_root,
            &ctx.temporary_dir,
            kind,
            None,
            None,
            content.as_bytes().to_vec(),
        )
        .await
        .expect("store should succeed")
        .id
    }

    let input_id = store(&ctx, ArtifactKind::Prompt, "prompt text").await;
    let output_id = store(&ctx, ArtifactKind::Output, "generated output text").await;
    let stdout_id = store(&ctx, ArtifactKind::Stdout, "stdout log").await;
    let stderr_id = store(&ctx, ArtifactKind::Stderr, "stderr log").await;
    let crash_id = store(&ctx, ArtifactKind::CrashLog, "crash log").await;

    let id = Uuid::now_v7();
    let mut new_run = common::seed_new_run(&ctx, id).await;
    new_run.input_artifact_id = Some(input_id);
    new_run.output_artifact_id = Some(output_id);
    new_run.stdout_artifact_id = Some(stdout_id);
    new_run.stderr_artifact_id = Some(stderr_id);
    new_run.crash_artifact_id = Some(crash_id);

    insert_run(&ctx.pool, &new_run)
        .await
        .expect("insert should succeed");
    let fetched = get_run(&ctx.pool, id)
        .await
        .expect("get should succeed")
        .expect("run should exist");

    assert_eq!(fetched.input_artifact_id, Some(input_id));
    assert_eq!(fetched.output_artifact_id, Some(output_id));
    assert_eq!(fetched.stdout_artifact_id, Some(stdout_id));
    assert_eq!(fetched.stderr_artifact_id, Some(stderr_id));
    assert_eq!(fetched.crash_artifact_id, Some(crash_id));
}

#[tokio::test]
async fn an_artifact_referenced_by_two_runs_is_preserved_and_readable_through_both() {
    let ctx = common::test_context().await;

    let shared = store_artifact_bytes(
        &ctx.pool,
        &ctx.artifact_root,
        &ctx.temporary_dir,
        ArtifactKind::Stdout,
        None,
        None,
        b"shared stdout content".to_vec(),
    )
    .await
    .expect("store should succeed");

    let first_id = Uuid::now_v7();
    let mut first_run = common::seed_new_run(&ctx, first_id).await;
    first_run.stdout_artifact_id = Some(shared.id);
    insert_run(&ctx.pool, &first_run)
        .await
        .expect("first insert should succeed");

    let second_id = Uuid::now_v7();
    let mut second_run = common::seed_new_run(&ctx, second_id).await;
    second_run.stdout_artifact_id = Some(shared.id);
    insert_run(&ctx.pool, &second_run)
        .await
        .expect("second insert should succeed");

    for run_id in [first_id, second_id] {
        let fetched = get_run(&ctx.pool, run_id)
            .await
            .expect("get should succeed")
            .expect("run should exist");
        assert_eq!(fetched.stdout_artifact_id, Some(shared.id));
    }

    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM artifacts")
        .fetch_one(&ctx.pool)
        .await
        .expect("failed to count artifacts");
    assert_eq!(
        count, 1,
        "the artifact is stored once and shared, not duplicated"
    );

    let record = get_artifact_record(&ctx.pool, shared.id)
        .await
        .expect("lookup should succeed")
        .expect("artifact should still be registered");
    let mut content = open_artifact_content(&ctx.artifact_root, &record)
        .await
        .expect("content should still be readable through the second run's reference");
    let mut buf = Vec::new();
    content.read_to_end(&mut buf).await.unwrap();
    assert_eq!(buf, b"shared stdout content");
}

#[tokio::test]
async fn a_successful_exit_and_a_failed_correctness_result_coexist_independently() {
    let (pool, ctx) = common::migrated_pool().await;
    let id = Uuid::now_v7();
    let mut new_run = common::seed_new_run(&ctx, id).await;
    new_run.exit_status = ExitStatus::Succeeded;
    new_run.correctness_result = CorrectnessResult::Failed;

    insert_run(&pool, &new_run)
        .await
        .expect("insert should succeed");
    let fetched = get_run(&pool, id)
        .await
        .expect("get should succeed")
        .expect("run should exist");

    assert_eq!(fetched.exit_status, ExitStatus::Succeeded);
    assert_eq!(fetched.correctness_result, CorrectnessResult::Failed);
}
