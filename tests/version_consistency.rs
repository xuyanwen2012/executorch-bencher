//! The three hand-maintained version numbers cannot drift apart: the
//! `api_version` the server reports is the OpenAPI document's
//! `info.version`, and the `schema_version` it reports is what the
//! migrations wrote to `schema_metadata`. See `specs/api-documentation` -
//! "System exposes version and compatibility information".

mod common;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use executorch_bencher::{http, version_api};
use serde_json::Value;
use sqlx::Row;
use tower::ServiceExt;

#[tokio::test]
async fn reported_versions_match_the_openapi_document_and_the_database() {
    let ctx = common::test_context().await;
    let app = http::router(ctx.pool.clone(), ctx.config());

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/version")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();

    assert_eq!(body["api_version"], version_api::API_VERSION);
    assert_eq!(
        http::openapi_document().info.version,
        version_api::API_VERSION,
        "openapi info.version must be API_VERSION"
    );

    let stored: i64 = sqlx::query("SELECT schema_version FROM schema_metadata WHERE id = 1")
        .fetch_one(&ctx.pool)
        .await
        .expect("schema_metadata should exist after migration")
        .get("schema_version");
    assert_eq!(body["schema_version"], stored);
    assert_eq!(i64::from(version_api::SCHEMA_VERSION), stored);
}
