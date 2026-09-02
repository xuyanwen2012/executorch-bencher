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
async fn uploading_and_retrieving_an_artifact_round_trips_its_content() {
    let (pool, ctx) = common::migrated_pool().await;
    let config = ctx.config();
    let app = http::router(pool, config);

    let upload_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/artifacts?kind=stdout&original_name=run.log")
                .header("content-type", "text/plain")
                .body(Body::from("hello from an http upload"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(upload_response.status(), StatusCode::CREATED);
    let uploaded = body_json(upload_response).await;
    let id = uploaded["id"].as_str().unwrap().to_string();

    let metadata_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/artifacts/{id}/metadata"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(metadata_response.status(), StatusCode::OK);
    let metadata = body_json(metadata_response).await;
    assert_eq!(metadata["kind"], "stdout");
    assert_eq!(metadata["original_filename"], "run.log");
    assert_eq!(metadata["compression"], "zstd");
    assert_eq!(metadata["available"], true);

    let content_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/artifacts/{id}/content"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(content_response.status(), StatusCode::OK);
    let content = to_bytes(content_response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&content[..], b"hello from an http upload");

    let download_response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/artifacts/{id}/download"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(download_response.status(), StatusCode::OK);
    let disposition = download_response
        .headers()
        .get("content-disposition")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(disposition.contains("run.log"));
    let content = to_bytes(download_response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&content[..], b"hello from an http upload");
}

#[tokio::test]
async fn an_invalid_kind_is_rejected_with_bad_request() {
    let (pool, ctx) = common::migrated_pool().await;
    let app = http::router(pool, ctx.config());

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/artifacts?kind=not_a_real_kind")
                .body(Body::from("data"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn an_oversized_upload_is_rejected() {
    let (pool, ctx) = common::migrated_pool().await;
    let mut config = ctx.config();
    config.limits.max_artifact_upload_bytes = 10;
    let app = http::router(pool, config);

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/artifacts?kind=prompt")
                .body(Body::from("this body is much longer than ten bytes"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);

    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM artifacts")
        .fetch_one(&ctx.pool)
        .await
        .unwrap();
    assert_eq!(count, 0, "an oversized upload must not be stored");
}

#[tokio::test]
async fn a_missing_file_is_reported_clearly_through_content_and_download_routes() {
    let (pool, ctx) = common::migrated_pool().await;
    let config = ctx.config();
    let app = http::router(pool, config);

    let upload_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/artifacts?kind=prompt")
                .body(Body::from("will be deleted out of band"))
                .unwrap(),
        )
        .await
        .unwrap();
    let uploaded = body_json(upload_response).await;
    let id = uploaded["id"].as_str().unwrap().to_string();

    let record =
        executorch_bencher::artifact_store::get_artifact_record(&ctx.pool, id.parse().unwrap())
            .await
            .unwrap()
            .unwrap();
    std::fs::remove_file(ctx.artifact_root.join(&record.storage_path)).unwrap();

    let content_response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/artifacts/{id}/content"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(content_response.status(), StatusCode::GONE);
}

#[tokio::test]
async fn an_unknown_artifact_id_is_a_404() {
    let (pool, ctx) = common::migrated_pool().await;
    let app = http::router(pool, ctx.config());

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/v1/artifacts/{}/metadata",
                    uuid::Uuid::now_v7()
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
