mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use executorch_bencher::http;
use tower::ServiceExt;

#[tokio::test]
async fn health_reports_ok_when_database_is_reachable() {
    let (pool, ctx) = common::migrated_pool().await;
    let config = ctx.config();
    let app = http::router(pool, config);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn health_reports_failure_when_database_is_unreachable() {
    let (pool, ctx) = common::migrated_pool().await;
    let config = ctx.config();
    pool.close().await;
    let app = http::router(pool, config);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}
