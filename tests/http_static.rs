mod common;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use executorch_bencher::http;
use tower::ServiceExt;

async fn get(app: axum::Router, uri: &str) -> (StatusCode, String) {
    let response = app
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

#[tokio::test]
async fn a_configured_dashboard_directory_is_served_with_an_spa_fallback() {
    let ctx = common::test_context().await;
    let dist = tempfile::tempdir().unwrap();
    std::fs::write(dist.path().join("index.html"), "<html>shell</html>").unwrap();
    std::fs::write(dist.path().join("app-abc123.js"), "console.log('asset')").unwrap();
    let mut config = ctx.config();
    config.dashboard_dist = Some(dist.path().to_path_buf());
    let app = http::router(ctx.pool.clone(), config);

    let (status, body) = get(app.clone(), "/").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "<html>shell</html>");

    let (status, body) = get(
        app.clone(),
        &format!("/runs/{}", uuid::Uuid::now_v7()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "client-side route reloads get the shell: {body}");
    assert_eq!(body, "<html>shell</html>");

    let (status, body) = get(app.clone(), "/app-abc123.js").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "console.log('asset')");

    // API, health, and docs routes take precedence over static serving.
    let (status, _) = get(app.clone(), "/health").await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = get(app.clone(), "/api/v1/version").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("api_version"));
    let (status, _) = get(app.clone(), "/openapi.json").await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = get(
        app,
        &format!("/api/v1/runs/{}", uuid::Uuid::now_v7()),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "API 404s are still API 404s");
}

#[tokio::test]
async fn without_a_dashboard_directory_the_site_root_stays_unserved() {
    let ctx = common::test_context().await;
    let app = http::router(ctx.pool.clone(), ctx.config());
    let (status, _) = get(app.clone(), "/").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = get(app, "/runs/anything").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
