//! `POST /api/v1/runs`. See `specs/ingestion-service` - "Service accepts
//! complete run records over HTTP" and "Run creation validates references
//! and snapshot rules before writing".

mod common;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use executorch_bencher::artifact_store::{ArtifactKind, store_artifact_bytes};
use executorch_bencher::http;
use serde_json::{Value, json};
use sqlx::Row;
use tower::ServiceExt;
use uuid::Uuid;

async fn body_json(response: axum::response::Response) -> Value {
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

async fn post(app: axum::Router, uri: &str, body: &Value) -> axum::response::Response {
    app.oneshot(
        Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap(),
    )
    .await
    .unwrap()
}

async fn get(app: axum::Router, uri: &str) -> axum::response::Response {
    app.oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap()
}

async fn run_count(pool: &sqlx::SqlitePool) -> i64 {
    sqlx::query("SELECT count(*) AS c FROM runs")
        .fetch_one(pool)
        .await
        .unwrap()
        .get("c")
}

fn linux_body(model_asset_id: Uuid) -> Value {
    json!({
        "id": Uuid::now_v7(),
        "started_at": "2026-09-01T12:00:00Z",
        "finished_at": "2026-09-01T12:00:05Z",
        "repetition": 0,
        "command_args": ["--model_path=m.pte", "--max_new_tokens=1"],
        "command_line": "llama_main --model_path=m.pte --max_new_tokens=1",
        "input_parameters": {"backend": "vulkan", "benchmark": "prefill-2048"},
        "env_vars": {},
        "env_allowlist_version": "none",
        "collector_version": "collector/0.1",
        "platform": "linux",
        "device_class": "external",
        "device_serial": "ubuntu-lts-gpu",
        "host_os": "Ubuntu 24.04.4 LTS",
        "host_kernel": "7.0.0-30-generic",
        "host_cpu_model": "AMD EPYC 4464P 12-Core Processor",
        "host_cpu_count": 16,
        "host_accelerator": "Intel(R) Arc(tm) B580 Graphics (BMG G21)",
        "host_accelerator_driver": "Mesa 25.2.8",
        "git_commit_sha": "e4d02f41f7909e8ed5bf4a14ffc520d733453d9f",
        "git_dirty": false,
        "git_branch": "release/1.4",
        "executable_sha256": "0".repeat(64),
        "model_asset_id": model_asset_id,
        "prompt_sha256": "c".repeat(64),
        "input_token_count": 2048,
        "output_token_count": 0,
        "prefill_tokens_per_sec": 385.6,
        "decode_tokens_per_sec": null,
        "exit_status": "succeeded",
        "correctness_result": "not_checked",
    })
}

#[tokio::test]
async fn a_posted_run_is_stored_once_and_visible_everywhere() {
    let ctx = common::test_context().await;
    let model_asset_id = ctx.shared_test_model().await;
    let stdout = store_artifact_bytes(
        &ctx.pool,
        &ctx.artifact_root,
        &ctx.temporary_dir,
        ArtifactKind::Stdout,
        Some("stdout.txt"),
        Some("text/plain"),
        b"PyTorchObserver {}\n".to_vec(),
    )
    .await
    .unwrap();
    let app = http::router(ctx.pool.clone(), ctx.config());

    let mut body = linux_body(model_asset_id);
    body["stdout_artifact_id"] = json!(stdout.id);
    let id = body["id"].as_str().unwrap().to_string();

    let response = post(app.clone(), "/api/v1/runs", &body).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let created = body_json(response).await;
    assert_eq!(created["id"], id);
    assert_eq!(created["platform"], "linux");
    assert_eq!(created["device_class"], "external");
    assert_eq!(created["host_accelerator"], "Intel(R) Arc(tm) B580 Graphics (BMG G21)");
    assert_eq!(created["stdout_artifact"]["id"], stdout.id.to_string());
    assert_eq!(created["model_asset"]["original_name"], "shared-test-model.pte");
    assert_eq!(created["command_args"], json!(["--model_path=m.pte", "--max_new_tokens=1"]));
    assert!(created["bsp_version"].is_null());

    let fetched = body_json(get(app.clone(), &format!("/api/v1/runs/{id}")).await).await;
    assert_eq!(fetched, created);
    let listed = body_json(get(app.clone(), "/api/v1/runs").await).await;
    assert_eq!(listed["items"][0]["id"], id);
    let results = body_json(get(app, "/api/v1/results").await).await;
    assert_eq!(results["rows"][0]["prefill"]["median"], 385.6);
    assert_eq!(results["rows"][0]["total_runs"], 1);
    assert_eq!(run_count(&ctx.pool).await, 1);
}

#[tokio::test]
async fn a_crashed_run_counts_as_not_succeeded() {
    let ctx = common::test_context().await;
    let model_asset_id = ctx.shared_test_model().await;
    let crash = store_artifact_bytes(
        &ctx.pool,
        &ctx.artifact_root,
        &ctx.temporary_dir,
        ArtifactKind::CrashLog,
        Some("crash.txt"),
        Some("text/plain"),
        b"device rebooted".to_vec(),
    )
    .await
    .unwrap();
    let app = http::router(ctx.pool.clone(), ctx.config());

    let ok = linux_body(model_asset_id);
    assert_eq!(post(app.clone(), "/api/v1/runs", &ok).await.status(), StatusCode::CREATED);
    let mut crashed = linux_body(model_asset_id);
    crashed["finished_at"] = Value::Null;
    crashed["prefill_tokens_per_sec"] = json!(0.0);
    crashed["exit_status"] = json!("crashed");
    crashed["error_summary"] = json!("device rebooted during the first 8B attempt");
    crashed["crash_artifact_id"] = json!(crash.id);
    let response = post(app.clone(), "/api/v1/runs", &crashed).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let created = body_json(response).await;
    assert!(created["finished_at"].is_null());
    assert_eq!(created["crash_artifact"]["kind"], "crash_log");

    let results = body_json(get(app, "/api/v1/results").await).await;
    let row = &results["rows"][0];
    assert_eq!(row["total_runs"], 2);
    assert_eq!(row["not_succeeded"], 1);
    assert_eq!(row["prefill"]["n"], 1);
    assert_eq!(row["prefill"]["median"], 385.6);
}

#[tokio::test]
async fn an_external_phone_posts_without_lab_fields() {
    let ctx = common::test_context().await;
    let model_asset_id = ctx.shared_test_model().await;
    let app = http::router(ctx.pool.clone(), ctx.config());

    let mut body = linux_body(model_asset_id);
    body["platform"] = json!("android");
    body["device_serial"] = json!("3A021JEHN02756");
    body["device_model"] = json!("Pixel 7a");
    body["host_os"] = json!("Android 17 (CP2A.260705.006)");
    body["host_kernel"] = json!("6.1.157-android14-11");
    body["host_cpu_model"] = json!("GS201");
    body["host_cpu_count"] = json!(8);
    body["host_accelerator"] = json!("Mali-G710");
    body["host_accelerator_driver"] = json!("v1.r54p3");
    let response = post(app.clone(), "/api/v1/runs", &body).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let created = body_json(response).await;
    assert_eq!(created["platform"], "android");
    assert_eq!(created["device_class"], "external");
    assert_eq!(created["device_model"], "Pixel 7a");
    assert_eq!(created["host_cpu_model"], "GS201");
    for lab in [
        "bsp_version",
        "sumd_driver_version",
        "gpu_clock_mhz",
        "mif_clock_mhz",
        "int_clock_mhz",
        "battery_charging",
        "initial_temperature_celsius",
        "max_temperature_celsius",
        "device_uptime_seconds",
        "thermal_throttling",
    ] {
        assert!(created[lab].is_null(), "{lab} should be null");
    }
}

#[tokio::test]
async fn a_missing_artifact_reference_is_rejected_and_nothing_is_stored() {
    let ctx = common::test_context().await;
    let model_asset_id = ctx.shared_test_model().await;
    let app = http::router(ctx.pool.clone(), ctx.config());

    let mut body = linux_body(model_asset_id);
    body["stdout_artifact_id"] = json!(Uuid::now_v7());
    let response = post(app.clone(), "/api/v1/runs", &body).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let err = body_json(response).await;
    assert_eq!(err["error"]["code"], "invalid_request");
    assert_eq!(err["error"]["details"]["field"], "stdout_artifact_id");
    assert_eq!(run_count(&ctx.pool).await, 0);

    let mut body = linux_body(model_asset_id);
    body["model_asset_id"] = json!(Uuid::now_v7());
    let err = body_json(post(app, "/api/v1/runs", &body).await).await;
    assert_eq!(err["error"]["details"]["field"], "model_asset_id");
    assert_eq!(run_count(&ctx.pool).await, 0);
}

#[tokio::test]
async fn an_internal_device_without_its_lab_snapshot_is_rejected() {
    let ctx = common::test_context().await;
    let model_asset_id = ctx.shared_test_model().await;
    let app = http::router(ctx.pool.clone(), ctx.config());

    let mut body = linux_body(model_asset_id);
    body["platform"] = json!("android");
    body["device_class"] = json!("internal");
    for key in ["host_os", "host_kernel", "host_cpu_model", "host_cpu_count", "host_accelerator", "host_accelerator_driver"] {
        body[key] = Value::Null;
    }
    let response = post(app, "/api/v1/runs", &body).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let err = body_json(response).await;
    assert_eq!(err["error"]["details"]["field"], "bsp_version");
    assert_eq!(run_count(&ctx.pool).await, 0);
}

#[tokio::test]
async fn a_retried_submission_is_a_conflict_not_a_duplicate() {
    let ctx = common::test_context().await;
    let model_asset_id = ctx.shared_test_model().await;
    let app = http::router(ctx.pool.clone(), ctx.config());

    let body = linux_body(model_asset_id);
    assert_eq!(post(app.clone(), "/api/v1/runs", &body).await.status(), StatusCode::CREATED);
    let response = post(app, "/api/v1/runs", &body).await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let err = body_json(response).await;
    assert_eq!(err["error"]["code"], "conflict");
    assert_eq!(run_count(&ctx.pool).await, 1);
}

#[tokio::test]
async fn an_oversized_output_preview_is_bounded() {
    let ctx = common::test_context().await;
    let model_asset_id = ctx.shared_test_model().await;
    let mut config = ctx.config();
    config.limits.output_preview_length = 8;
    let app = http::router(ctx.pool.clone(), config);

    let mut body = linux_body(model_asset_id);
    body["output_preview"] = json!("0123456789abcdef");
    let created = body_json(post(app, "/api/v1/runs", &body).await).await;
    assert_eq!(created["output_preview"], "01234567");
}

#[tokio::test]
async fn malformed_bodies_and_unknown_enum_values_get_the_envelope() {
    let ctx = common::test_context().await;
    let model_asset_id = ctx.shared_test_model().await;
    let app = http::router(ctx.pool.clone(), ctx.config());

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/runs")
                .header("content-type", "application/json")
                .body(Body::from("{not json"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(body_json(response).await["error"]["code"], "invalid_request");

    let mut body = linux_body(model_asset_id);
    body["exit_status"] = json!("exploded");
    let response = post(app, "/api/v1/runs", &body).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let err = body_json(response).await;
    assert_eq!(err["error"]["details"]["field"], "exit_status");
    assert_eq!(run_count(&ctx.pool).await, 0);
}

/// Every shape the database CHECK constraint rejects must be caught by
/// validation first, as `invalid_request` naming the field, never `500`.
#[tokio::test]
async fn every_check_violating_shape_is_refused_by_validation() {
    let ctx = common::test_context().await;
    let model_asset_id = ctx.shared_test_model().await;
    let app = http::router(ctx.pool.clone(), ctx.config());

    let android_external = |serial: &str| {
        let mut b = linux_body(model_asset_id);
        b["platform"] = json!("android");
        b["device_serial"] = json!(serial);
        for key in ["host_os", "host_kernel", "host_cpu_model", "host_cpu_count", "host_accelerator", "host_accelerator_driver"] {
            b[key] = Value::Null;
        }
        b
    };
    let mut cases: Vec<(&str, Value)> = Vec::new();
    let mut b = linux_body(model_asset_id);
    b["gpu_clock_mhz"] = json!(980);
    cases.push(("gpu_clock_mhz", b));
    let mut b = linux_body(model_asset_id);
    b["sumd_driver_version"] = json!("sumd");
    cases.push(("sumd_driver_version", b));
    let mut b = linux_body(model_asset_id);
    b["initial_temperature_celsius"] = json!(30.0);
    cases.push(("initial_temperature_celsius", b));
    let mut b = linux_body(model_asset_id);
    b["host_kernel"] = Value::Null;
    cases.push(("host_kernel", b));
    let mut b = android_external("ext-1");
    b["device_class"] = json!("internal");
    cases.push(("bsp_version", b));
    let mut b = android_external("ext-2");
    b["mif_clock_mhz"] = json!(5333);
    cases.push(("bsp_version", b));
    let mut b = android_external("ext-3");
    b["gpu_clock_mhz"] = json!(-1);
    cases.push(("gpu_clock_mhz", b));
    let mut b = linux_body(model_asset_id);
    b["platform"] = json!("windows");
    cases.push(("platform", b));
    let mut b = linux_body(model_asset_id);
    b["device_class"] = json!("lab");
    cases.push(("device_class", b));
    let mut b = linux_body(model_asset_id);
    b["executable_sha256"] = json!("short");
    cases.push(("executable_sha256", b));
    let mut b = linux_body(model_asset_id);
    b["repetition"] = json!(-1);
    cases.push(("repetition", b));
    let mut b = linux_body(model_asset_id);
    b["input_token_count"] = json!(-1);
    cases.push(("input_token_count", b));
    let mut b = linux_body(model_asset_id);
    b["prefill_tokens_per_sec"] = json!(-0.5);
    cases.push(("prefill_tokens_per_sec", b));
    let mut b = linux_body(model_asset_id);
    b["command_args"] = json!("not an array");
    cases.push(("command_args", b));

    for (field, body) in cases {
        let response = post(app.clone(), "/api/v1/runs", &body).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "case {field}");
        let err = body_json(response).await;
        assert_eq!(err["error"]["code"], "invalid_request", "case {field}");
        assert_eq!(err["error"]["details"]["field"], field, "case {field}: {}", err["error"]["message"]);
    }
    assert_eq!(run_count(&ctx.pool).await, 0);
}
