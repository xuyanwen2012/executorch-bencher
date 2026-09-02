mod common;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use executorch_bencher::artifact_store::{ArtifactKind, store_artifact_bytes};
use executorch_bencher::http;
use executorch_bencher::runs::insert_run;
use serde_json::Value;
use tower::ServiceExt;
use uuid::Uuid;

async fn body_json(response: axum::response::Response) -> Value {
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn a_fetched_run_includes_its_attached_artifact_metadata() {
    let ctx = common::test_context().await;

    let stdout = store_artifact_bytes(
        &ctx.pool,
        &ctx.artifact_root,
        &ctx.temporary_dir,
        ArtifactKind::Stdout,
        Some("stdout.log"),
        Some("text/plain"),
        b"stdout content".to_vec(),
    )
    .await
    .unwrap();

    let id = Uuid::now_v7();
    let mut new_run = common::seed_new_run(&ctx, id).await;
    new_run.stdout_artifact_id = Some(stdout.id);
    insert_run(&ctx.pool, &new_run).await.unwrap();

    let pool = ctx.pool.clone();
    let app = http::router(pool, ctx.config());

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/runs/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;

    assert_eq!(body["id"], id.to_string());
    assert_eq!(
        body["model_asset"]["id"],
        new_run.model_asset_id.to_string()
    );
    let stdout_view = &body["stdout_artifact"];
    assert_eq!(stdout_view["id"], stdout.id.to_string());
    assert_eq!(stdout_view["kind"], "stdout");
    assert_eq!(stdout_view["original_filename"], "stdout.log");
    assert_eq!(stdout_view["available"], true);
    assert!(body["output_artifact"].is_null());
}

#[tokio::test]
async fn an_unknown_run_id_is_a_404() {
    let ctx = common::test_context().await;
    let pool = ctx.pool.clone();
    let app = http::router(pool, ctx.config());

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/runs/{}", Uuid::now_v7()))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

async fn get(app: axum::Router, uri: &str) -> axum::response::Response {
    app.oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap()
}

/// Inserts `count` runs at strictly increasing start times and returns
/// their IDs in insertion (oldest-first) order.
async fn seed_runs(ctx: &common::TestContext, count: i64) -> Vec<Uuid> {
    let base = chrono::DateTime::parse_from_rfc3339("2026-09-01T00:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let mut ids = Vec::new();
    for i in 0..count {
        let id = Uuid::now_v7();
        let mut run = common::seed_new_run(ctx, id).await;
        run.started_at = base + chrono::Duration::minutes(i);
        insert_run(&ctx.pool, &run).await.unwrap();
        ids.push(id);
    }
    ids
}

fn item_ids(body: &Value) -> Vec<String> {
    body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn listing_returns_newest_first_with_a_cursor_only_when_more_remain() {
    let ctx = common::test_context().await;
    let ids = seed_runs(&ctx, 3).await;
    let app = http::router(ctx.pool.clone(), ctx.config());

    let response = get(app.clone(), "/api/v1/runs?limit=2").await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(
        item_ids(&body),
        vec![ids[2].to_string(), ids[1].to_string()]
    );
    assert!(body["next_cursor"].is_string());
    let item = &body["items"][0];
    assert_eq!(item["model_asset"]["original_name"], "shared-test-model.pte");
    assert_eq!(item["decode_tokens_per_sec"], 50.0);
    assert_eq!(item["exit_status"], "succeeded");

    let response = get(app, "/api/v1/runs?limit=10").await;
    let body = body_json(response).await;
    assert_eq!(item_ids(&body).len(), 3);
    assert!(body["next_cursor"].is_null());
}

#[tokio::test]
async fn listing_filters_combine_conjunctively() {
    let ctx = common::test_context().await;
    let a = Uuid::now_v7();
    let mut run_a = common::seed_new_run(&ctx, a).await;
    run_a.device_serial = "dev-A".into();
    run_a.exit_status = executorch_bencher::domain::ExitStatus::Crashed;
    insert_run(&ctx.pool, &run_a).await.unwrap();
    let b = Uuid::now_v7();
    let mut run_b = common::seed_new_run(&ctx, b).await;
    run_b.device_serial = "dev-A".into();
    insert_run(&ctx.pool, &run_b).await.unwrap();
    let c = Uuid::now_v7();
    let mut run_c = common::seed_new_run(&ctx, c).await;
    run_c.device_serial = "dev-B".into();
    run_c.exit_status = executorch_bencher::domain::ExitStatus::Crashed;
    insert_run(&ctx.pool, &run_c).await.unwrap();
    let app = http::router(ctx.pool.clone(), ctx.config());

    let body = body_json(
        get(
            app,
            "/api/v1/runs?device_serial=dev-A&exit_status=crashed",
        )
        .await,
    )
    .await;
    assert_eq!(item_ids(&body), vec![a.to_string()]);
}

#[tokio::test]
async fn a_full_configuration_key_selects_exactly_one_configurations_runs() {
    let ctx = common::test_context().await;
    let model_asset_id = ctx.shared_test_model().await;
    let prompt = "d".repeat(64);
    let mut in_key = Vec::new();
    for _ in 0..2 {
        let id = Uuid::now_v7();
        let mut run = common::build_new_run(id, model_asset_id);
        run.git_commit_sha = "abc123".into();
        run.git_dirty = true;
        common::android_lab_mut(&mut run).sumd_driver_version = "sumd-2.0".into();
        common::android_lab_mut(&mut run).bsp_version = "bsp-2.0".into();
        common::android_lab_mut(&mut run).gpu_clock_mhz = 1000;
        run.prompt_sha256 = executorch_bencher::domain::Sha256Hex::try_from(prompt.clone()).unwrap();
        insert_run(&ctx.pool, &run).await.unwrap();
        in_key.push(id.to_string());
    }
    // Same in every field but one, for each field.
    let mut near_miss = common::build_new_run(Uuid::now_v7(), model_asset_id);
    near_miss.git_commit_sha = "abc123".into();
    near_miss.git_dirty = false;
    common::android_lab_mut(&mut near_miss).sumd_driver_version = "sumd-2.0".into();
    common::android_lab_mut(&mut near_miss).bsp_version = "bsp-2.0".into();
    common::android_lab_mut(&mut near_miss).gpu_clock_mhz = 1000;
    near_miss.prompt_sha256 = executorch_bencher::domain::Sha256Hex::try_from(prompt.clone()).unwrap();
    insert_run(&ctx.pool, &near_miss).await.unwrap();
    let mut other_clock = near_miss.clone();
    other_clock.id = Uuid::now_v7();
    other_clock.git_dirty = true;
    common::android_lab_mut(&mut other_clock).mif_clock_mhz = 1;
    insert_run(&ctx.pool, &other_clock).await.unwrap();

    let app = http::router(ctx.pool.clone(), ctx.config());
    let uri = format!(
        "/api/v1/runs?device_serial=device-001&model_asset_id={model_asset_id}&git_commit_sha=abc123&git_dirty=true&sumd_driver_version=sumd-2.0&bsp_version=bsp-2.0&gpu_clock_mhz=1000&mif_clock_mhz=5333&int_clock_mhz=934&prompt_sha256={prompt}"
    );
    let body = body_json(get(app, &uri).await).await;
    let mut got = item_ids(&body);
    got.sort();
    in_key.sort();
    assert_eq!(got, in_key);
}

#[tokio::test]
async fn invalid_listing_parameters_return_the_invalid_request_envelope() {
    let ctx = common::test_context().await;
    let app = http::router(ctx.pool.clone(), ctx.config());

    for uri in [
        "/api/v1/runs?limit=0",
        "/api/v1/runs?limit=201",
        "/api/v1/runs?exit_status=exploded",
        "/api/v1/runs?correctness_result=maybe",
        "/api/v1/runs?cursor=definitely-not-a-cursor",
        "/api/v1/runs?prompt_sha256=abc",
    ] {
        let response = get(app.clone(), uri).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{uri}");
        let body = body_json(response).await;
        assert_eq!(body["error"]["code"], "invalid_request", "{uri}");
        assert!(body["error"]["message"].is_string(), "{uri}");
    }
}

#[tokio::test]
async fn paging_to_exhaustion_neither_skips_nor_repeats_despite_a_concurrent_insert() {
    let ctx = common::test_context().await;
    let ids = seed_runs(&ctx, 5).await;
    let app = http::router(ctx.pool.clone(), ctx.config());

    let mut seen = Vec::new();
    let mut cursor: Option<String> = None;
    let mut page_no = 0;
    loop {
        let uri = match &cursor {
            Some(c) => format!("/api/v1/runs?limit=2&cursor={c}"),
            None => "/api/v1/runs?limit=2".to_string(),
        };
        let body = body_json(get(app.clone(), &uri).await).await;
        seen.extend(item_ids(&body));
        page_no += 1;
        if page_no == 1 {
            // A run that starts *after* every seeded run lands above the
            // cursor and must not disturb the pages still to come.
            let late = Uuid::now_v7();
            let mut run = common::seed_new_run(&ctx, late).await;
            run.started_at = chrono::Utc::now();
            insert_run(&ctx.pool, &run).await.unwrap();
        }
        match body["next_cursor"].as_str() {
            Some(c) => cursor = Some(c.to_string()),
            None => break,
        }
    }

    let expected: Vec<String> = ids.iter().rev().map(|id| id.to_string()).collect();
    assert_eq!(seen, expected);
}

#[tokio::test]
async fn a_fetched_run_exposes_its_complete_recorded_record() {
    let ctx = common::test_context().await;
    let id = Uuid::now_v7();
    let mut new_run = common::seed_new_run(&ctx, id).await;
    new_run.command_args_json = r#"["--model","llama","--reps","3"]"#.to_string();
    new_run.input_parameters_json = r#"{"batch_size":1,"seq_len":512}"#.to_string();
    new_run.env_vars_json = r#"{"FOO":null,"BAR":""}"#.to_string();
    new_run.git_branch = Some("main".into());
    new_run.git_commit_timestamp = Some(
        chrono::DateTime::parse_from_rfc3339("2026-08-30T10:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc),
    );
    new_run.git_commit_subject = Some("Tune prefill".into());
    new_run.error_summary = None;
    insert_run(&ctx.pool, &new_run).await.unwrap();
    let app = http::router(ctx.pool.clone(), ctx.config());

    let response = get(app, &format!("/api/v1/runs/{id}")).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;

    // Pre-existing fields keep their names and shapes.
    assert_eq!(body["id"], id.to_string());
    assert_eq!(body["exit_status"], "succeeded");
    assert_eq!(body["correctness_result"], "not_checked");
    assert!(body["output_preview"].is_null());
    assert!(body["model_asset"]["id"].is_string());
    assert!(body["stdout_artifact"].is_null());

    // Structured JSON columns come back as JSON, not encoded strings.
    assert_eq!(body["command_args"], serde_json::json!(["--model", "llama", "--reps", "3"]));
    assert_eq!(body["input_parameters"]["seq_len"], 512);
    assert!(body["env_vars"]["FOO"].is_null());
    assert_eq!(body["env_vars"]["BAR"], "");

    assert_eq!(body["repetition"], 0);
    assert_eq!(body["command_line"], "./run.sh resnet50");
    assert_eq!(body["env_allowlist_version"], "v1");
    assert_eq!(body["collector_version"], "collector-0.1");
    assert_eq!(body["platform"], "android");
    assert_eq!(body["device_class"], "internal");
    assert_eq!(body["device_serial"], "device-001");
    assert!(body["device_model"].is_null());
    assert!(body["host_os"].is_null());
    assert!(body["host_accelerator"].is_null());
    assert_eq!(body["bsp_version"], "bsp-1.0");
    assert_eq!(body["sumd_driver_version"], "sumd-1.0");
    assert_eq!(body["device_uptime_seconds"], 100);
    assert_eq!(body["battery_charging"], false);
    assert_eq!(body["initial_temperature_celsius"], 20.0);
    assert_eq!(body["max_temperature_celsius"], 25.0);
    assert_eq!(body["thermal_throttling"], false);
    assert_eq!(body["gpu_clock_mhz"], 980);
    assert_eq!(body["mif_clock_mhz"], 5333);
    assert_eq!(body["int_clock_mhz"], 934);
    assert_eq!(body["git_commit_sha"], "abc123");
    assert_eq!(body["git_dirty"], false);
    assert_eq!(body["git_branch"], "main");
    assert_eq!(body["git_commit_timestamp"], "2026-08-30T10:00:00Z");
    assert_eq!(body["git_commit_subject"], "Tune prefill");
    assert_eq!(body["executable_sha256"], "a".repeat(64));
    assert_eq!(body["prompt_sha256"], "c".repeat(64));
    assert_eq!(body["input_token_count"], 10);
    assert_eq!(body["output_token_count"], 20);
    assert_eq!(body["prefill_tokens_per_sec"], 100.0);
    assert_eq!(body["decode_tokens_per_sec"], 50.0);
    assert!(body["error_summary"].is_null());
}

#[tokio::test]
async fn linux_runs_round_trip_with_host_fields_and_null_android_fields() {
    let ctx = common::test_context().await;
    let model_asset_id = ctx.shared_test_model().await;
    let id = Uuid::now_v7();
    let mut run = common::build_new_linux_run(id, model_asset_id, "ubuntu-lts-gpu");
    run.executable_sha256 = None;
    insert_run(&ctx.pool, &run).await.unwrap();
    let app = http::router(ctx.pool.clone(), ctx.config());

    let body = body_json(get(app.clone(), &format!("/api/v1/runs/{id}")).await).await;
    assert_eq!(body["platform"], "linux");
    assert_eq!(body["device_class"], "external");
    assert_eq!(body["device_serial"], "ubuntu-lts-gpu");
    assert_eq!(body["host_os"], "Ubuntu 24.04.4 LTS");
    assert_eq!(body["host_kernel"], "7.0.0-30-generic");
    assert_eq!(body["host_cpu_model"], "AMD EPYC 4464P 12-Core Processor");
    assert_eq!(body["host_cpu_count"], 16);
    assert_eq!(body["host_memory_bytes"], 16_299_392_i64 * 1024);
    assert_eq!(body["host_accelerator"], "Intel(R) Arc(tm) B580 Graphics (BMG G21)");
    assert_eq!(body["host_accelerator_driver"], "Mesa 25.2.8");
    for android_only in [
        "bsp_version",
        "sumd_driver_version",
        "battery_charging",
        "initial_temperature_celsius",
        "max_temperature_celsius",
        "gpu_clock_mhz",
        "mif_clock_mhz",
        "int_clock_mhz",
    ] {
        assert!(body[android_only].is_null(), "{android_only} should be null on linux");
    }
    assert!(body["device_uptime_seconds"].is_null());
    assert!(body["thermal_throttling"].is_null());
    assert!(body["executable_sha256"].is_null());

    // The listing carries the platform and accelerator, and filters on them.
    let body = body_json(get(app.clone(), "/api/v1/runs?platform=linux").await).await;
    assert_eq!(item_ids(&body), vec![id.to_string()]);
    let item = &body["items"][0];
    assert_eq!(item["platform"], "linux");
    assert_eq!(item["host_accelerator"], "Intel(R) Arc(tm) B580 Graphics (BMG G21)");
    assert!(item["sumd_driver_version"].is_null());
    assert!(item["thermal_throttling"].is_null());
    let body = body_json(get(app.clone(), "/api/v1/runs?platform=android").await).await;
    assert!(item_ids(&body).is_empty());
    let body = body_json(
        get(app.clone(), "/api/v1/runs?host_accelerator=Intel(R)%20Arc(tm)%20B580%20Graphics%20(BMG%20G21)").await,
    )
    .await;
    assert_eq!(item_ids(&body), vec![id.to_string()]);

    let response = get(app, "/api/v1/runs?platform=windows").await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn external_android_runs_carry_what_the_phone_reports_and_nothing_else() {
    let ctx = common::test_context().await;
    let model_asset_id = ctx.shared_test_model().await;
    let id = Uuid::now_v7();
    let run = common::build_new_external_android_run(id, model_asset_id, "R5CY21Y3VEV");
    insert_run(&ctx.pool, &run).await.unwrap();
    let lab_id = Uuid::now_v7();
    insert_run(&ctx.pool, &common::build_new_run(lab_id, model_asset_id))
        .await
        .unwrap();
    let app = http::router(ctx.pool.clone(), ctx.config());

    let body = body_json(get(app.clone(), &format!("/api/v1/runs/{id}")).await).await;
    assert_eq!(body["platform"], "android");
    assert_eq!(body["device_class"], "external");
    assert_eq!(body["device_serial"], "R5CY21Y3VEV");
    assert_eq!(body["device_model"], "SM-S926B");
    assert_eq!(body["host_os"], "Android 16 (BP4A.251205.006)");
    assert_eq!(body["host_cpu_model"], "s5e9945");
    assert_eq!(body["host_accelerator"], "Samsung Xclipse 940");
    for lab_only in [
        "bsp_version",
        "sumd_driver_version",
        "gpu_clock_mhz",
        "mif_clock_mhz",
        "int_clock_mhz",
        "battery_charging",
        "initial_temperature_celsius",
        "device_uptime_seconds",
        "thermal_throttling",
    ] {
        assert!(body[lab_only].is_null(), "{lab_only} should be null on a retail phone");
    }

    let body = body_json(get(app.clone(), "/api/v1/runs?device_class=external").await).await;
    assert_eq!(item_ids(&body), vec![id.to_string()]);
    assert_eq!(body["items"][0]["device_model"], "SM-S926B");
    let body = body_json(get(app.clone(), "/api/v1/runs?device_class=internal").await).await;
    assert_eq!(item_ids(&body), vec![lab_id.to_string()]);
    let response = get(app.clone(), "/api/v1/runs?device_class=lab").await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    // An internal device cannot be recorded without the full lab snapshot.
    let mut incomplete = common::build_new_external_android_run(Uuid::now_v7(), model_asset_id, "X");
    incomplete.device_class = executorch_bencher::domain::DeviceClass::Internal;
    let err = insert_run(&ctx.pool, &incomplete)
        .await
        .expect_err("internal device without lab snapshot must be rejected");
    assert!(err.to_string().contains("lab snapshot"));
}
