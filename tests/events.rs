//! `GET /api/v1/events`. See `specs/ingestion-service` - "Service streams
//! change notifications as Server-Sent Events".

mod common;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use executorch_bencher::events::Event;
use executorch_bencher::http;
use futures_util::StreamExt;
use serde_json::{Value, json};
use std::time::Duration;
use tower::ServiceExt;
use uuid::Uuid;

fn run_body(model_asset_id: Uuid) -> Value {
    json!({
        "id": Uuid::now_v7(),
        "started_at": "2026-09-01T12:00:00Z",
        "finished_at": "2026-09-01T12:00:05Z",
        "repetition": 0,
        "command_args": ["--max_new_tokens=1"],
        "input_parameters": {},
        "env_vars": {},
        "env_allowlist_version": "none",
        "collector_version": "collector/0.1",
        "platform": "linux",
        "device_class": "external",
        "device_serial": "box-a",
        "host_os": "Ubuntu 24.04.4 LTS",
        "host_kernel": "7.0.0-30-generic",
        "host_cpu_model": "AMD EPYC 4464P",
        "host_accelerator": "Intel Arc B580",
        "git_commit_sha": "abc123",
        "git_dirty": false,
        "model_asset_id": model_asset_id,
        "prompt_sha256": "c".repeat(64),
        "input_token_count": 2048,
        "output_token_count": 0,
        "prefill_tokens_per_sec": 385.6,
        "exit_status": "succeeded",
        "correctness_result": "not_checked",
    })
}

async fn post_json(app: axum::Router, uri: &str, body: &Value) -> axum::response::Response {
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

/// Reads the stream until `needle` appears or the timeout elapses.
async fn read_until(
    body: axum::body::Body,
    needle: &str,
    timeout: Duration,
) -> String {
    let mut stream = body.into_data_stream();
    let mut text = String::new();
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        match tokio::time::timeout(remaining, stream.next()).await {
            Ok(Some(Ok(chunk))) => {
                text.push_str(&String::from_utf8_lossy(&chunk));
                if text.contains(needle) {
                    break;
                }
            }
            _ => break,
        }
    }
    text
}

#[tokio::test]
async fn write_paths_publish_events_with_the_new_records_id() {
    let ctx = common::test_context().await;
    let model_asset_id = ctx.shared_test_model().await;
    let app = http::router(ctx.pool.clone(), ctx.config());
    // The state is cloned into every handler; grab a receiver through a
    // stream subscription first so the sender has a subscriber.
    let stream_response = app
        .clone()
        .oneshot(Request::builder().uri("/api/v1/events").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(stream_response.status(), StatusCode::OK);
    assert!(
        stream_response
            .headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("text/event-stream")
    );

    // Artifact upload.
    let upload = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/artifacts?kind=stdout")
                .header("content-type", "text/plain")
                .body(Body::from("PyTorchObserver {}\n"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(upload.status(), StatusCode::CREATED);
    let upload: Value = serde_json::from_slice(&to_bytes(upload.into_body(), usize::MAX).await.unwrap()).unwrap();
    let artifact_id = upload["id"].as_str().unwrap().to_string();

    // Model registration.
    let model_path = ctx.model_root.parent().unwrap().join("events-model.pte");
    std::fs::write(&model_path, b"events model bytes").unwrap();
    let registered = post_json(
        app.clone(),
        "/api/v1/models/register",
        &json!({ "path": model_path.to_str().unwrap() }),
    )
    .await;
    assert_eq!(registered.status(), StatusCode::CREATED);
    let registered: Value = serde_json::from_slice(&to_bytes(registered.into_body(), usize::MAX).await.unwrap()).unwrap();
    let registered_id = registered["id"].as_str().unwrap().to_string();

    // Run creation.
    let body = run_body(model_asset_id);
    let run_id = body["id"].as_str().unwrap().to_string();
    assert_eq!(post_json(app.clone(), "/api/v1/runs", &body).await.status(), StatusCode::CREATED);

    let text = read_until(stream_response.into_body(), &run_id, Duration::from_secs(5)).await;
    let artifact_pos = text.find("event: artifact.created").expect("artifact event");
    let model_pos = text.find("event: model.registered").expect("model event");
    let run_pos = text.find("event: run.created").expect("run event");
    assert!(artifact_pos < model_pos && model_pos < run_pos, "events arrive in write order");
    assert!(text.contains(&artifact_id));
    assert!(text.contains(&registered_id));
    // Each event carries an increasing id.
    let ids: Vec<u64> = text
        .lines()
        .filter_map(|l| l.strip_prefix("id: "))
        .map(|v| v.parse().unwrap())
        .collect();
    assert_eq!(ids.len(), 3);
    assert!(ids.windows(2).all(|w| w[0] < w[1]));
    // The run event's data is the listing summary.
    let data_line = text
        .lines()
        .skip_while(|l| *l != "event: run.created")
        .find(|l| l.starts_with("data: "))
        .unwrap();
    let data: Value = serde_json::from_str(data_line.strip_prefix("data: ").unwrap()).unwrap();
    assert_eq!(data["id"], run_id);
    assert_eq!(data["device_serial"], "box-a");
    assert_eq!(data["model_original_name"], "shared-test-model.pte");
    assert_eq!(data["prefill_tokens_per_sec"], 385.6);
}

#[tokio::test]
async fn the_stream_sends_keep_alives_while_idle() {
    let ctx = common::test_context().await;
    let mut config = ctx.config();
    config.events_keep_alive_seconds = 1;
    let app = http::router(ctx.pool.clone(), config);
    // `app` must outlive the read: dropping the router drops the event
    // bus, which ends every subscriber's stream.
    let response = app
        .clone()
        .oneshot(Request::builder().uri("/api/v1/events").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let text = read_until(response.into_body(), "keep-alive", Duration::from_secs(5)).await;
    assert!(text.contains(": keep-alive"), "got {text:?}");
    drop(app);
}

#[tokio::test]
async fn events_are_not_replayed_to_late_subscribers() {
    let ctx = common::test_context().await;
    let model_asset_id = ctx.shared_test_model().await;
    let app = http::router(ctx.pool.clone(), ctx.config());
    assert_eq!(
        post_json(app.clone(), "/api/v1/runs", &run_body(model_asset_id)).await.status(),
        StatusCode::CREATED
    );
    let response = app
        .clone()
        .oneshot(Request::builder().uri("/api/v1/events").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let text = read_until(response.into_body(), "run.created", Duration::from_millis(500)).await;
    assert!(!text.contains("run.created"), "late subscriber must not see earlier events");
    drop(app);
}

#[test]
fn event_names_match_the_documented_set() {
    use executorch_bencher::events::{ArtifactCreatedEvent, ModelRegisteredEvent};
    use executorch_bencher::artifact_store::ArtifactKind;
    let a = Event::ArtifactCreated(ArtifactCreatedEvent {
        id: Uuid::now_v7(),
        kind: ArtifactKind::Stdout,
        sha256: "a".repeat(64),
        size_bytes: 1,
    });
    let m = Event::ModelRegistered(ModelRegisteredEvent {
        id: Uuid::now_v7(),
        original_name: "m.pte".into(),
        sha256: "b".repeat(64),
    });
    assert_eq!(a.name(), "artifact.created");
    assert_eq!(m.name(), "model.registered");
    assert_eq!(a.data()["kind"], "stdout");
}
