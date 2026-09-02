mod common;

use executorch_bencher::domain::Sha256Hex;
use executorch_bencher::runs::{get_run, insert_run};
use uuid::Uuid;

#[tokio::test]
async fn creating_and_retrieving_a_complete_run_round_trips_all_fields() {
    let (pool, ctx) = common::migrated_pool().await;
    let id = Uuid::now_v7();
    let new_run = common::seed_new_run(&ctx, id).await;

    insert_run(&pool, &new_run)
        .await
        .expect("insert should succeed");
    let fetched = get_run(&pool, id)
        .await
        .expect("get should succeed")
        .expect("run should exist");

    assert_eq!(fetched.id, id);
    assert_eq!(fetched.repetition, new_run.repetition);
    assert_eq!(fetched.command_args_json, new_run.command_args_json);
    assert_eq!(fetched.command_line, new_run.command_line);
    assert_eq!(fetched.device_serial, new_run.device_serial);
    assert_eq!(fetched.host, new_run.host);
    assert_eq!(fetched.git_commit_sha, new_run.git_commit_sha);
    assert_eq!(fetched.git_dirty, new_run.git_dirty);
    assert_eq!(fetched.executable_sha256, new_run.executable_sha256);
    assert_eq!(fetched.model_asset_id, new_run.model_asset_id);
    assert_eq!(fetched.prompt_sha256, new_run.prompt_sha256);
    assert_eq!(fetched.input_token_count, new_run.input_token_count);
    assert_eq!(fetched.output_token_count, new_run.output_token_count);
    assert_eq!(
        fetched.prefill_tokens_per_sec,
        new_run.prefill_tokens_per_sec
    );
    assert_eq!(fetched.decode_tokens_per_sec, new_run.decode_tokens_per_sec);
    assert_eq!(fetched.exit_status, new_run.exit_status);
    assert_eq!(fetched.correctness_result, new_run.correctness_result);
}

#[tokio::test]
async fn a_run_with_null_decode_speed_round_trips_as_none() {
    let (pool, ctx) = common::migrated_pool().await;
    let id = Uuid::now_v7();
    let mut new_run = common::seed_new_run(&ctx, id).await;
    new_run.decode_tokens_per_sec = None;

    insert_run(&pool, &new_run)
        .await
        .expect("insert should succeed");
    let fetched = get_run(&pool, id)
        .await
        .expect("get should succeed")
        .expect("run should exist");

    assert_eq!(fetched.decode_tokens_per_sec, None);
    // prefill speed remains required and present.
    assert_eq!(
        fetched.prefill_tokens_per_sec,
        new_run.prefill_tokens_per_sec
    );
}

#[tokio::test]
async fn malformed_sha256_is_rejected_at_the_application_boundary_before_any_query_runs() {
    // The type system itself is the boundary: a `NewRun` cannot even be
    // constructed with an invalid digest, so no query is ever issued.
    assert!(Sha256Hex::try_from("too-short".to_string()).is_err());
    assert!(Sha256Hex::try_from("A".repeat(64)).is_err());
    assert!(Sha256Hex::try_from("g".repeat(64)).is_err());
}

#[tokio::test]
async fn an_invalid_exit_status_is_rejected_by_the_database_check_constraint() {
    let (pool, ctx) = common::migrated_pool().await;
    let model_asset_id = ctx.shared_test_model().await;
    let result =
        insert_bare_run_with_overrides(&pool, model_asset_id, &[("exit_status", "'bogus'")]).await;
    let err = result.expect_err("invalid exit status should be rejected");
    assert!(err.to_string().to_lowercase().contains("check"));
}

#[tokio::test]
async fn an_invalid_correctness_result_is_rejected_by_the_database_check_constraint() {
    let (pool, ctx) = common::migrated_pool().await;
    let model_asset_id = ctx.shared_test_model().await;
    let result =
        insert_bare_run_with_overrides(&pool, model_asset_id, &[("correctness_result", "'bogus'")])
            .await;
    let err = result.expect_err("invalid correctness result should be rejected");
    assert!(err.to_string().to_lowercase().contains("check"));
}

#[tokio::test]
async fn a_negative_token_count_is_rejected_by_the_database_check_constraint() {
    let (pool, ctx) = common::migrated_pool().await;
    let model_asset_id = ctx.shared_test_model().await;
    let result =
        insert_bare_run_with_overrides(&pool, model_asset_id, &[("input_token_count", "-1")]).await;
    let err = result.expect_err("negative token count should be rejected");
    assert!(err.to_string().to_lowercase().contains("check"));
}

#[tokio::test]
async fn a_non_positive_clock_frequency_is_rejected_by_the_database_check_constraint() {
    let (pool, ctx) = common::migrated_pool().await;
    let model_asset_id = ctx.shared_test_model().await;
    let result =
        insert_bare_run_with_overrides(&pool, model_asset_id, &[("gpu_clock_mhz", "0")]).await;
    let err = result.expect_err("zero clock frequency should be rejected");
    assert!(err.to_string().to_lowercase().contains("check"));
}

/// Issues a raw SQL insert (bypassing the type-safe `NewRun`/`insert_run`
/// API) with the given column overrides, to exercise the database-level
/// `CHECK` constraints directly as a defense-in-depth guard against any
/// write path that bypasses the Rust domain types.
async fn insert_bare_run_with_overrides(
    pool: &sqlx::SqlitePool,
    model_asset_id: Uuid,
    overrides: &[(&str, &str)],
) -> Result<(), sqlx::Error> {
    let mut columns: Vec<(&str, String)> = vec![
        ("id", format!("'{}'", Uuid::now_v7())),
        ("started_at", "'2026-09-01T00:00:00Z'".to_string()),
        ("repetition", "0".to_string()),
        ("command_args", "'[]'".to_string()),
        ("input_parameters", "'{}'".to_string()),
        ("env_vars", "'{}'".to_string()),
        ("env_allowlist_version", "'v1'".to_string()),
        ("collector_version", "'c1'".to_string()),
        ("platform", "'android'".to_string()),
        ("device_class", "'internal'".to_string()),
        ("device_serial", "'dev1'".to_string()),
        ("bsp_version", "'bsp1'".to_string()),
        ("sumd_driver_version", "'sumd1'".to_string()),
        ("device_uptime_seconds", "100".to_string()),
        ("battery_charging", "0".to_string()),
        ("initial_temperature_celsius", "20.0".to_string()),
        ("max_temperature_celsius", "25.0".to_string()),
        ("thermal_throttling", "0".to_string()),
        ("gpu_clock_mhz", "980".to_string()),
        ("mif_clock_mhz", "5333".to_string()),
        ("int_clock_mhz", "934".to_string()),
        ("git_commit_sha", "'abc'".to_string()),
        ("git_dirty", "0".to_string()),
        ("executable_sha256", format!("'{}'", "a".repeat(64))),
        ("model_asset_id", format!("'{model_asset_id}'")),
        ("prompt_sha256", format!("'{}'", "c".repeat(64))),
        ("input_token_count", "1".to_string()),
        ("output_token_count", "1".to_string()),
        ("prefill_tokens_per_sec", "10.0".to_string()),
        ("exit_status", "'succeeded'".to_string()),
        ("correctness_result", "'not_checked'".to_string()),
    ];

    for (column, value) in overrides {
        if let Some(existing) = columns.iter_mut().find(|(c, _)| c == column) {
            existing.1 = (*value).to_string();
        } else {
            columns.push((column, (*value).to_string()));
        }
    }

    let column_names: Vec<&str> = columns.iter().map(|(c, _)| *c).collect();
    let values: Vec<String> = columns.iter().map(|(_, v)| v.clone()).collect();
    let sql = format!(
        "INSERT INTO runs ({}) VALUES ({})",
        column_names.join(", "),
        values.join(", ")
    );

    // Test-only helper: `sql` is built entirely from fixed literals and a
    // closed set of caller-supplied test overrides, never external input.
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .execute(pool)
        .await
        .map(|_| ())
}

#[tokio::test]
async fn git_commit_metadata_round_trips_when_present_and_when_absent() {
    let ctx = common::test_context().await;

    let with_id = Uuid::now_v7();
    let mut with_metadata = common::seed_new_run(&ctx, with_id).await;
    let commit_time = chrono::DateTime::parse_from_rfc3339("2026-08-30T12:34:56Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    with_metadata.git_branch = Some("feature/faster-prefill".to_string());
    with_metadata.git_commit_timestamp = Some(commit_time);
    with_metadata.git_commit_subject = Some("Speed up prefill".to_string());
    insert_run(&ctx.pool, &with_metadata).await.unwrap();

    let without_id = Uuid::now_v7();
    let without_metadata = common::seed_new_run(&ctx, without_id).await;
    insert_run(&ctx.pool, &without_metadata).await.unwrap();

    let fetched = get_run(&ctx.pool, with_id).await.unwrap().unwrap();
    assert_eq!(fetched.git_branch.as_deref(), Some("feature/faster-prefill"));
    assert_eq!(fetched.git_commit_timestamp, Some(commit_time));
    assert_eq!(fetched.git_commit_subject.as_deref(), Some("Speed up prefill"));

    let fetched = get_run(&ctx.pool, without_id).await.unwrap().unwrap();
    assert_eq!(fetched.git_branch, None);
    assert_eq!(fetched.git_commit_timestamp, None);
    assert_eq!(fetched.git_commit_subject, None);
}

/// The platform CHECK constraint: an Android row must carry every Android
/// snapshot column and no Linux one, and vice versa.
#[tokio::test]
async fn platform_check_constraint_rejects_mismatched_snapshots() {
    let ctx = common::test_context().await;
    let model_asset_id = ctx.shared_test_model().await;

    // Internal Android row missing a lab column.
    let err = insert_bare_run_with_overrides(&ctx.pool, model_asset_id, &[("bsp_version", "NULL")])
        .await
        .expect_err("internal android row without bsp_version should be rejected");
    assert!(err.to_string().to_lowercase().contains("check"));

    // Internal Android rows may carry the host description (build, SoC, GPU).
    insert_bare_run_with_overrides(
        &ctx.pool,
        model_asset_id,
        &[("host_accelerator", "'Samsung Xclipse 940'")],
    )
    .await
    .expect("android row with a GPU name is accepted");

    // External Android: everything optional, but BSP/SUMD/clocks all-or-none.
    let external: &[(&str, &str)] = &[
        ("device_class", "'external'"),
        ("bsp_version", "NULL"),
        ("sumd_driver_version", "NULL"),
        ("device_uptime_seconds", "NULL"),
        ("battery_charging", "NULL"),
        ("initial_temperature_celsius", "NULL"),
        ("max_temperature_celsius", "NULL"),
        ("thermal_throttling", "NULL"),
        ("gpu_clock_mhz", "NULL"),
        ("mif_clock_mhz", "NULL"),
        ("int_clock_mhz", "NULL"),
        ("device_model", "'Pixel 7a'"),
        ("host_os", "'Android 17 (CP2A.260705.006)'"),
        ("host_cpu_model", "'GS201'"),
    ];
    insert_bare_run_with_overrides(&ctx.pool, model_asset_id, external)
        .await
        .expect("an external android row without the lab snapshot is accepted");
    let mut half_lab = external.to_vec();
    half_lab.push(("gpu_clock_mhz", "980"));
    let err = insert_bare_run_with_overrides(&ctx.pool, model_asset_id, &half_lab)
        .await
        .expect_err("external android row with only one lab column should be rejected");
    assert!(err.to_string().to_lowercase().contains("check"));

    // An external Linux row cannot claim to be internal-with-lab-snapshot
    // (no android columns), and unknown device classes are rejected.
    let err = insert_bare_run_with_overrides(&ctx.pool, model_asset_id, &[("device_class", "'lab'")])
        .await
        .expect_err("unknown device class should be rejected");
    assert!(err.to_string().to_lowercase().contains("check"));

    // Unknown platform.
    let err = insert_bare_run_with_overrides(&ctx.pool, model_asset_id, &[("platform", "'ios'")])
        .await
        .expect_err("unknown platform should be rejected");
    assert!(err.to_string().to_lowercase().contains("check"));

    // A well-formed Linux row is accepted; one with a leftover clock is not.
    let linux_columns: &[(&str, &str)] = &[
        ("platform", "'linux'"),
        ("device_class", "'external'"),
        ("bsp_version", "NULL"),
        ("sumd_driver_version", "NULL"),
        ("device_uptime_seconds", "NULL"),
        ("battery_charging", "NULL"),
        ("initial_temperature_celsius", "NULL"),
        ("max_temperature_celsius", "NULL"),
        ("thermal_throttling", "NULL"),
        ("gpu_clock_mhz", "NULL"),
        ("mif_clock_mhz", "NULL"),
        ("int_clock_mhz", "NULL"),
        ("host_os", "'Ubuntu 24.04.4 LTS'"),
        ("host_kernel", "'7.0.0-30-generic'"),
        ("host_cpu_model", "'AMD EPYC 4464P'"),
        ("host_accelerator", "'Intel Arc B580'"),
        ("executable_sha256", "NULL"),
    ];
    insert_bare_run_with_overrides(&ctx.pool, model_asset_id, linux_columns)
        .await
        .expect("a well-formed linux row is accepted");
    let mut with_clock = linux_columns.to_vec();
    with_clock.push(("gpu_clock_mhz", "980"));
    let err = insert_bare_run_with_overrides(&ctx.pool, model_asset_id, &with_clock)
        .await
        .expect_err("linux row with a pinned clock should be rejected");
    assert!(err.to_string().to_lowercase().contains("check"));
}
