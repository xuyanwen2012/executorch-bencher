//! Every rejection - including the ones produced by parameter and body
//! extraction before a handler runs - uses the JSON error envelope. See
//! `specs/ingestion-service` - "HTTP error responses use a consistent JSON
//! envelope".

mod common;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use executorch_bencher::http;
use serde_json::Value;
use tower::ServiceExt;

async fn envelope(response: axum::response::Response) -> (StatusCode, Value) {
    let status = response.status();
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    assert!(
        content_type.starts_with("application/json"),
        "expected a JSON envelope, got content-type {content_type:?}"
    );
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&bytes).expect("body should be JSON");
    assert!(body["error"]["code"].is_string(), "missing error.code in {body}");
    assert!(body["error"]["message"].is_string(), "missing error.message in {body}");
    (status, body)
}

async fn get(app: axum::Router, uri: &str) -> (StatusCode, Value) {
    let response = app
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    envelope(response).await
}

#[tokio::test]
async fn malformed_query_parameters_are_enveloped() {
    let ctx = common::test_context().await;
    let app = http::router(ctx.pool.clone(), ctx.config());

    for uri in [
        "/api/v1/runs?limit=abc",
        "/api/v1/runs?git_dirty=yes",
        "/api/v1/runs?model_asset_id=not-a-uuid",
        "/api/v1/results?git_dirty=maybe",
        "/api/v1/results?platform=ios",
    ] {
        let (status, body) = get(app.clone(), uri).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}: {body}");
        assert_eq!(body["error"]["code"], "invalid_request", "{uri}: {body}");
    }

    // An upload with a missing `kind` is rejected by the query extractor.
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/artifacts?original_name=x.log")
                .body(Body::from("x"))
                .unwrap(),
        )
        .await
        .unwrap();
    let (status, body) = envelope(response).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"]["code"], "invalid_request");
}

#[tokio::test]
async fn malformed_path_parameters_are_enveloped() {
    let ctx = common::test_context().await;
    let app = http::router(ctx.pool.clone(), ctx.config());

    for uri in [
        "/api/v1/runs/not-a-uuid",
        "/api/v1/models/not-a-uuid",
        "/api/v1/artifacts/not-a-uuid/metadata",
        "/api/v1/artifacts/not-a-uuid/content",
        "/api/v1/artifacts/not-a-uuid/download",
    ] {
        let (status, body) = get(app.clone(), uri).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}: {body}");
        assert_eq!(body["error"]["code"], "invalid_request", "{uri}: {body}");
    }

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/models/not-a-uuid/verify")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let (status, body) = envelope(response).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_request");
}

#[tokio::test]
async fn a_malformed_model_registration_body_is_enveloped() {
    let ctx = common::test_context().await;
    let app = http::router(ctx.pool.clone(), ctx.config());

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/models/register")
                .header("content-type", "application/json")
                .body(Body::from("{\"path\": "))
                .unwrap(),
        )
        .await
        .unwrap();
    let (status, body) = envelope(response).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_request");

    // A body without the JSON content type is a client error too, not a
    // plain-text 415.
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/models/register")
                .body(Body::from("{\"path\": \"/x.pte\"}"))
                .unwrap(),
        )
        .await
        .unwrap();
    let (status, body) = envelope(response).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_request");
}

#[tokio::test]
async fn an_oversized_run_body_is_a_413_envelope() {
    let ctx = common::test_context().await;
    let app = http::router(ctx.pool.clone(), ctx.config());

    let oversized = vec![b'a'; 2 * 1024 * 1024 + 1];
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/runs")
                .header("content-type", "application/json")
                .body(Body::from(oversized))
                .unwrap(),
        )
        .await
        .unwrap();
    let (status, body) = envelope(response).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(body["error"]["code"], "payload_too_large");
}
