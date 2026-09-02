mod common;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use executorch_bencher::http;
use serde_json::Value;
use tower::ServiceExt;

async fn body_json(response: axum::response::Response) -> Value {
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn registering_and_fetching_a_model_round_trips() {
    let (pool, ctx) = common::migrated_pool().await;
    let config = ctx.config();
    let model_path = config.model_root.parent().unwrap().join("http-model.pte");
    std::fs::create_dir_all(model_path.parent().unwrap()).unwrap();
    std::fs::write(&model_path, b"fake .pte bytes for http test").unwrap();

    let app = http::router(pool, config);

    let register_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/models/register")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "path": model_path.to_str().unwrap() }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(register_response.status(), StatusCode::CREATED);
    let registered = body_json(register_response).await;
    let id = registered["id"].as_str().unwrap().to_string();
    assert_eq!(registered["storage_mode"], "external");
    assert_eq!(registered["available"], true);

    let get_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/models/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(get_response.status(), StatusCode::OK);
    let fetched = body_json(get_response).await;
    assert_eq!(fetched["id"], id);

    let list_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/models")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(list_response.status(), StatusCode::OK);
    let listed = body_json(list_response).await;
    assert!(listed.as_array().unwrap().iter().any(|m| m["id"] == id));

    let verify_response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/models/{id}/verify"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(verify_response.status(), StatusCode::OK);
    let verified = body_json(verify_response).await;
    assert_eq!(verified["sha256"], registered["sha256"]);
    assert!(verified["last_verified_at"].is_string());

    // Registration never copies the model file into managed storage.
    assert!(!ctx.model_root.exists());
}

#[tokio::test]
async fn registering_a_missing_path_is_a_bad_request() {
    let (pool, ctx) = common::migrated_pool().await;
    let app = http::router(pool, ctx.config());

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/models/register")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "path": "/nonexistent/model.pte" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// See `specs/ingestion-service` - "Service exposes model lookup by content
/// hash".
#[tokio::test]
async fn models_can_be_looked_up_by_sha256() {
    let ctx = common::test_context().await;
    let id = ctx.shared_test_model().await;
    let app = http::router(ctx.pool.clone(), ctx.config());

    let all = body_json(
        app.clone()
            .oneshot(Request::builder().uri("/api/v1/models").body(Body::empty()).unwrap())
            .await
            .unwrap(),
    )
    .await;
    let sha = all[0]["sha256"].as_str().unwrap().to_string();

    let found = body_json(
        app.clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/models?sha256={sha}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(found.as_array().unwrap().len(), 1);
    assert_eq!(found[0]["id"], id.to_string());

    let none = body_json(
        app.clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/models?sha256={}", "f".repeat(64)))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(none, serde_json::json!([]));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/models?sha256=NOPE")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(body_json(response).await["error"]["code"], "invalid_request");
}
