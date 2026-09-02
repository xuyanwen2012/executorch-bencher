mod common;

use executorch_bencher::runs::insert_run;
use uuid::Uuid;

#[tokio::test]
async fn a_run_referencing_a_nonexistent_artifact_is_rejected_by_the_foreign_key_constraint() {
    let (pool, ctx) = common::migrated_pool().await;
    let id = Uuid::now_v7();
    let mut new_run = common::seed_new_run(&ctx, id).await;
    new_run.stdout_artifact_id = Some(Uuid::now_v7()); // no matching artifacts row

    let err = insert_run(&pool, &new_run)
        .await
        .expect_err("nonexistent artifact reference should be rejected");
    assert!(err.to_string().to_lowercase().contains("foreign key"));
}

#[tokio::test]
async fn foreign_key_enforcement_is_active_end_to_end_on_the_pooled_connection() {
    let (pool, ctx) = common::migrated_pool().await;

    // Confirm the pragma is actually active on this connection (not just
    // that the migration declared the foreign key column).
    let enabled: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
        .fetch_one(&pool)
        .await
        .expect("failed to read foreign_keys pragma");
    assert_eq!(enabled, 1);

    let model_asset_id = ctx.shared_test_model().await;

    let result = sqlx::query(
        "INSERT INTO runs (
            id, started_at, repetition, command_args, input_parameters, env_vars,
            env_allowlist_version, collector_version, platform, device_class, device_serial, bsp_version,
            sumd_driver_version, device_uptime_seconds, battery_charging,
            initial_temperature_celsius, max_temperature_celsius, thermal_throttling,
            gpu_clock_mhz, mif_clock_mhz, int_clock_mhz, git_commit_sha, git_dirty,
            executable_sha256, model_asset_id, prompt_sha256, input_token_count,
            output_token_count, prefill_tokens_per_sec, exit_status,
            correctness_result, stdout_artifact_id
        ) VALUES (
            ?, '2026-09-01T00:00:00Z', 0, '[]', '{}', '{}', 'v1', 'c1', 'android', 'internal', 'dev1', 'bsp1',
            'sumd1', 100, 0, 20.0, 25.0, 0, 980, 5333, 934, 'abc', 0, ?, ?, ?, 1, 1, 10.0,
            'succeeded', 'not_checked', ?
        )",
    )
    .bind(Uuid::now_v7().to_string())
    .bind("a".repeat(64))
    .bind(model_asset_id.to_string())
    .bind("c".repeat(64))
    .bind(Uuid::now_v7().to_string())
    .execute(&pool)
    .await;

    let err = result.expect_err("raw insert violating the foreign key should fail");
    let db_err = err.as_database_error().expect("expected a database error");
    // SQLite reports a dedicated extended error code for foreign key
    // violations (787 = SQLITE_CONSTRAINT_FOREIGNKEY), not a generic
    // constraint failure.
    assert_eq!(db_err.code().as_deref(), Some("787"));
}

#[tokio::test]
async fn a_run_referencing_a_nonexistent_model_asset_is_rejected_by_the_foreign_key_constraint() {
    let (pool, _ctx) = common::migrated_pool().await;
    let new_run = common::build_new_run(Uuid::now_v7(), Uuid::now_v7()); // no matching model_assets row

    let err = insert_run(&pool, &new_run)
        .await
        .expect_err("nonexistent model asset reference should be rejected");
    assert!(err.to_string().to_lowercase().contains("foreign key"));
}
