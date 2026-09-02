mod common;

use sqlx::Row;

#[tokio::test]
async fn migrations_apply_cleanly_against_a_completely_empty_database() {
    // `test_context` points at a brand-new temp file path that does not
    // exist until `connect_and_migrate` creates it, so this exercises
    // "migrate from a completely empty database" as well as "migrations
    // apply cleanly".
    let ctx = common::test_context().await;

    let version: i64 = sqlx::query("SELECT schema_version FROM schema_metadata WHERE id = 1")
        .fetch_one(&ctx.pool)
        .await
        .expect("failed to read schema_metadata")
        .get("schema_version");
    assert_eq!(version, 1);
}

/// The git-metadata migration is additive: rolling it back and re-applying
/// it (through the normal startup path) against a database that already
/// holds a run must keep that run readable, with null metadata afterwards.
#[tokio::test]
async fn git_metadata_migration_round_trips_over_an_existing_run() {
    let ctx = common::test_context().await;
    let id = uuid::Uuid::now_v7();
    let mut new_run = common::seed_new_run(&ctx, id).await;
    new_run.git_branch = Some("main".to_string());
    new_run.git_commit_subject = Some("before rollback".to_string());
    executorch_bencher::runs::insert_run(&ctx.pool, &new_run)
        .await
        .expect("insert should succeed");

    // Roll back everything newer than the last pre-metadata migration
    // (drops the three columns while keeping the row).
    sqlx::migrate!()
        .undo(&ctx.pool, 20260901180002)
        .await
        .expect("down migration should apply against a populated table");
    let columns: Vec<String> = sqlx::query("PRAGMA table_info(runs)")
        .fetch_all(&ctx.pool)
        .await
        .unwrap()
        .iter()
        .map(|row| row.get::<String, _>("name"))
        .collect();
    assert!(!columns.iter().any(|c| c == "git_branch"));
    // Close the pool before re-migrating: a live SQLite connection caches
    // prepared-statement column metadata, so schema changes are only ever
    // applied on the startup path (`connect_and_migrate`), never under a
    // pool that is already serving queries.
    ctx.pool.close().await;

    let pool = executorch_bencher::db::connect_and_migrate(&ctx.database_url)
        .await
        .expect("startup migration should apply against a populated table");

    let fetched = executorch_bencher::runs::get_run(&pool, id)
        .await
        .expect("get should succeed")
        .expect("run should survive the migration round trip");
    assert_eq!(fetched.device_serial, new_run.device_serial);
    assert_eq!(fetched.git_branch, None);
    assert_eq!(fetched.git_commit_timestamp, None);
    assert_eq!(fetched.git_commit_subject, None);
}

/// The platform migration rebuilds `runs`: rolling it back keeps Android
/// rows (dropping Linux ones, which the old schema cannot hold) and
/// re-applying it reads them back as Android runs.
#[tokio::test]
async fn platform_migration_round_trips_android_runs_and_drops_linux_runs() {
    let ctx = common::test_context().await;
    let android_id = uuid::Uuid::now_v7();
    let android = common::seed_new_run(&ctx, android_id).await;
    executorch_bencher::runs::insert_run(&ctx.pool, &android)
        .await
        .unwrap();
    let linux_id = uuid::Uuid::now_v7();
    let linux = common::build_new_linux_run(linux_id, android.model_asset_id, "box-a");
    executorch_bencher::runs::insert_run(&ctx.pool, &linux)
        .await
        .unwrap();

    sqlx::migrate!()
        .undo(&ctx.pool, 20260901190000)
        .await
        .expect("down migrations should apply against a populated table");
    let columns: Vec<String> = sqlx::query("PRAGMA table_info(runs)")
        .fetch_all(&ctx.pool)
        .await
        .unwrap()
        .iter()
        .map(|row| row.get::<String, _>("name"))
        .collect();
    assert!(!columns.iter().any(|c| c == "platform"));
    let count: i64 = sqlx::query("SELECT count(*) AS c FROM runs")
        .fetch_one(&ctx.pool)
        .await
        .unwrap()
        .get("c");
    assert_eq!(count, 1, "only the android run survives the rollback");
    ctx.pool.close().await;

    let pool = executorch_bencher::db::connect_and_migrate(&ctx.database_url)
        .await
        .unwrap();
    let fetched = executorch_bencher::runs::get_run(&pool, android_id)
        .await
        .unwrap()
        .expect("android run survives the round trip");
    assert_eq!(fetched.host, android.host);
    assert!(executorch_bencher::runs::get_run(&pool, linux_id)
        .await
        .unwrap()
        .is_none());
}
