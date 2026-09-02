mod common;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use executorch_bencher::domain::ExitStatus;
use executorch_bencher::http;
use executorch_bencher::runs::insert_run;
use serde_json::Value;
use tower::ServiceExt;
use uuid::Uuid;

async fn get_json(app: axum::Router, uri: &str) -> (StatusCode, Value) {
    let response = app
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test]
async fn results_group_runs_and_report_statistics_counts_and_facets() {
    let ctx = common::test_context().await;
    let model_asset_id = ctx.shared_test_model().await;
    let commit_time = chrono::DateTime::parse_from_rfc3339("2026-08-30T10:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    for (i, prefill) in [100.0, 120.0, 140.0].into_iter().enumerate() {
        let mut run = common::build_new_run(Uuid::now_v7(), model_asset_id);
        run.prefill_tokens_per_sec = prefill;
        run.decode_tokens_per_sec = Some(30.0 + i as f64);
        run.git_branch = Some("main".into());
        run.git_commit_timestamp = Some(commit_time);
        run.git_commit_subject = Some("Tune prefill".into());
        run.started_at = commit_time + chrono::Duration::hours(i as i64 + 1);
        insert_run(&ctx.pool, &run).await.unwrap();
    }
    let mut crashed = common::build_new_run(Uuid::now_v7(), model_asset_id);
    crashed.exit_status = ExitStatus::Crashed;
    common::android_mut(&mut crashed).thermal_throttling = Some(true);
    crashed.git_commit_timestamp = Some(commit_time);
    insert_run(&ctx.pool, &crashed).await.unwrap();
    // A second device, same commit: a distinct row.
    let mut other = common::build_new_run(Uuid::now_v7(), model_asset_id);
    other.device_serial = "device-002".into();
    other.git_commit_timestamp = Some(commit_time);
    insert_run(&ctx.pool, &other).await.unwrap();

    let app = http::router(ctx.pool.clone(), ctx.config());
    let (status, body) = get_json(app.clone(), "/api/v1/results").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["truncated"], false);
    let rows = body["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2);

    let row = rows
        .iter()
        .find(|r| r["device_serial"] == "device-001")
        .unwrap();
    assert_eq!(row["platform"], "android");
    assert_eq!(row["device_class"], "internal");
    assert!(row["host_accelerator"].is_null());
    assert_eq!(row["model_asset"]["id"], model_asset_id.to_string());
    assert_eq!(row["model_asset"]["original_name"], "shared-test-model.pte");
    assert_eq!(row["git_commit_sha"], "abc123");
    assert_eq!(row["git_dirty"], false);
    assert_eq!(row["git_branch"], "main");
    assert_eq!(row["git_commit_subject"], "Tune prefill");
    assert_eq!(row["git_commit_timestamp"], "2026-08-30T10:00:00Z");
    assert_eq!(row["sumd_driver_version"], "sumd-1.0");
    assert_eq!(row["bsp_version"], "bsp-1.0");
    assert_eq!(row["gpu_clock_mhz"], 980);
    assert_eq!(row["prompt_sha256"], "c".repeat(64));
    assert_eq!(row["input_token_count"], 10);
    assert_eq!(row["prefill"]["median"], 120.0);
    assert_eq!(row["prefill"]["min"], 100.0);
    assert_eq!(row["prefill"]["max"], 140.0);
    assert_eq!(row["prefill"]["n"], 3);
    assert_eq!(row["decode"]["median"], 31.0);
    assert_eq!(row["total_runs"], 4);
    assert_eq!(row["not_succeeded"], 1);
    assert_eq!(row["correctness_failed"], 0);
    assert_eq!(row["throttled"], 1);
    assert!(row["first_run_at"].is_string());
    assert!(row["last_run_at"].is_string());

    let facets = &body["facets"];
    assert_eq!(
        facets["device_serials"],
        serde_json::json!(["device-001", "device-002"])
    );
    assert_eq!(facets["models"][0]["original_name"], "shared-test-model.pte");
    assert_eq!(facets["git_branches"], serde_json::json!(["main"]));
    assert_eq!(facets["sumd_driver_versions"], serde_json::json!(["sumd-1.0"]));
    assert_eq!(facets["bsp_versions"], serde_json::json!(["bsp-1.0"]));
    assert_eq!(facets["platforms"], serde_json::json!(["android"]));
    assert_eq!(facets["device_classes"], serde_json::json!(["internal"]));
    assert_eq!(facets["host_accelerators"], serde_json::json!([]));

    // Filtering narrows rows but not facets.
    let (_, filtered) = get_json(app.clone(), "/api/v1/results?device_serial=device-002").await;
    assert_eq!(filtered["rows"].as_array().unwrap().len(), 1);
    assert_eq!(filtered["rows"][0]["device_serial"], "device-002");
    assert_eq!(filtered["facets"], *facets);

    let (status, err) = get_json(app, "/api/v1/results?prompt_sha256=nope").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(err["error"]["code"], "invalid_request");
}

#[tokio::test]
async fn results_with_no_runs_are_empty_with_empty_facets() {
    let ctx = common::test_context().await;
    let app = http::router(ctx.pool.clone(), ctx.config());
    let (status, body) = get_json(app, "/api/v1/results").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["rows"], serde_json::json!([]));
    assert_eq!(body["truncated"], false);
    assert_eq!(body["facets"]["device_serials"], serde_json::json!([]));
}

#[tokio::test]
async fn linux_configurations_are_keyed_by_host_and_accelerator() {
    let ctx = common::test_context().await;
    let model_asset_id = ctx.shared_test_model().await;
    for prefill in [900.0, 1000.0, 1100.0] {
        let mut run = common::build_new_linux_run(Uuid::now_v7(), model_asset_id, "ubuntu-lts-gpu");
        run.prefill_tokens_per_sec = prefill;
        run.decode_tokens_per_sec = None;
        insert_run(&ctx.pool, &run).await.unwrap();
    }
    let android = common::build_new_run(Uuid::now_v7(), model_asset_id);
    insert_run(&ctx.pool, &android).await.unwrap();

    let app = http::router(ctx.pool.clone(), ctx.config());
    let (status, body) = get_json(app.clone(), "/api/v1/results?platform=linux").await;
    assert_eq!(status, StatusCode::OK);
    let rows = body["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row["platform"], "linux");
    assert_eq!(row["device_serial"], "ubuntu-lts-gpu");
    assert_eq!(row["host_accelerator"], "Intel(R) Arc(tm) B580 Graphics (BMG G21)");
    assert!(row["sumd_driver_version"].is_null());
    assert!(row["bsp_version"].is_null());
    assert!(row["gpu_clock_mhz"].is_null());
    assert_eq!(row["prefill"]["median"], 1000.0);
    assert!(row["decode"].is_null());
    assert_eq!(row["throttled"], 0);
    assert_eq!(body["facets"]["platforms"], serde_json::json!(["android", "linux"]));
    assert_eq!(
        body["facets"]["host_accelerators"],
        serde_json::json!(["Intel(R) Arc(tm) B580 Graphics (BMG G21)"])
    );

    let (_, all) = get_json(app.clone(), "/api/v1/results").await;
    assert_eq!(all["rows"].as_array().unwrap().len(), 2);
    let (status, _) = get_json(app, "/api/v1/results?platform=ios").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
