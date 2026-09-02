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
    let model_path = config.model_root.join("http-model.pte");
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

    // Registration never copies the model file into managed storage: the
    // only thing beneath the model root is the file we placed there.
    assert!(!ctx.model_root.join("sha256").exists());
    assert_eq!(std::fs::read_dir(&ctx.model_root).unwrap().count(), 1);
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

async fn register(app: axum::Router, path: &str) -> (StatusCode, Value) {
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/models/register")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::json!({ "path": path }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    (status, body_json(response).await)
}

/// `POST /api/v1/models/register` is unauthenticated, so it must not be
/// usable as a file-existence or hash oracle over the server filesystem:
/// only `.pte` files beneath a registrable root are accepted, with
/// symlinks and `..` resolved before the check.
#[tokio::test]
async fn registration_is_confined_to_pte_files_beneath_the_registrable_roots() {
    let (pool, ctx) = common::migrated_pool().await;
    let config = ctx.config();
    let model_root = config.model_root.clone();
    std::fs::create_dir_all(&model_root).unwrap();
    let outside_dir = model_root.parent().unwrap().join("outside");
    std::fs::create_dir_all(&outside_dir).unwrap();
    let outside = outside_dir.join("secret.pte");
    std::fs::write(&outside, b"not registrable").unwrap();
    let wrong_extension = model_root.join("notes.txt");
    std::fs::write(&wrong_extension, b"text").unwrap();
    let inside = model_root.join("ok.pte");
    std::fs::write(&inside, b"registrable").unwrap();
    let escaping_link = model_root.join("link.pte");
    std::os::unix::fs::symlink(&outside, &escaping_link).unwrap();
    let dot_dot = model_root.join("..").join("outside").join("secret.pte");

    let app = http::router(pool, config);

    let rejected = [
        (outside.clone(), "a .pte outside every root"),
        (wrong_extension, "a non-.pte file inside the root"),
        (escaping_link, "a symlink escaping the root"),
        (dot_dot, "a `..` path escaping the root"),
        (std::path::PathBuf::from("relative.pte"), "a relative path"),
        (model_root.join("missing.pte"), "a missing file"),
        (model_root.clone(), "a directory"),
    ];
    for (path, why) in rejected {
        let (status, body) = register(app.clone(), path.to_str().unwrap()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{why} should be rejected: {body}");
        assert_eq!(body["error"]["code"], "invalid_request", "{why}: {body}");
        assert_eq!(body["error"]["details"]["field"], "path", "{why}: {body}");
        // The rejection must not leak the file's size or hash.
        assert!(body.get("sha256").is_none() && body.get("size_bytes").is_none());
    }

    let (status, body) = register(app.clone(), inside.to_str().unwrap()).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert!(body["external_path"].as_str().unwrap().ends_with("ok.pte"));

    let list_response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/models")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let listed = body_json(list_response).await;
    assert_eq!(listed.as_array().unwrap().len(), 1, "only the in-root file is registered");
}

#[tokio::test]
async fn verifying_a_model_rehashes_it_and_unknown_or_managed_models_are_reported() {
    let (pool, ctx) = common::migrated_pool().await;
    let config = ctx.config();
    std::fs::create_dir_all(&config.model_root).unwrap();
    let path = config.model_root.join("verify-me.pte");
    std::fs::write(&path, b"first content").unwrap();
    let app = http::router(pool.clone(), config);

    let (status, registered) = register(app.clone(), path.to_str().unwrap()).await;
    assert_eq!(status, StatusCode::CREATED);
    let id = registered["id"].as_str().unwrap().to_string();
    let original_sha = registered["sha256"].as_str().unwrap().to_string();

    async fn verify(app: axum::Router, id: &str) -> (StatusCode, Value) {
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/models/{id}/verify"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        (status, body_json(response).await)
    }

    let (status, verified) = verify(app.clone(), &id).await;
    assert_eq!(status, StatusCode::OK, "{verified}");
    assert_eq!(verified["sha256"], original_sha);
    assert_eq!(verified["available"], true);
    assert!(verified["last_verified_at"].is_string());

    // The file changes underneath the registry: a full verification always
    // rehashes and reports the new digest.
    std::fs::write(&path, b"second content, longer than before").unwrap();
    let (status, reverified) = verify(app.clone(), &id).await;
    assert_eq!(status, StatusCode::OK, "{reverified}");
    assert_ne!(reverified["sha256"], original_sha);
    assert_eq!(reverified["available"], true);

    let unknown = uuid::Uuid::now_v7();
    let (status, body) = verify(app.clone(), &unknown.to_string()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "not_found");

    let get_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/models/{unknown}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(get_response.status(), StatusCode::NOT_FOUND);
    assert_eq!(body_json(get_response).await["error"]["code"], "not_found");

    // A managed-mode row (no registration path creates one yet) is
    // reported as not implemented rather than failing internally.
    let managed_id = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO model_assets (
            id, sha256, original_name, size_bytes, model_format, storage_mode,
            external_path, relative_path, file_modified_at, registered_at,
            last_verified_at, available
         )
         SELECT ?, ?, 'managed.pte', size_bytes, model_format, 'managed',
                NULL, 'sha256/ab/abcdef', file_modified_at, registered_at,
                last_verified_at, 1
         FROM model_assets WHERE id = ?",
    )
    .bind(managed_id.to_string())
    .bind("0123456789abcdef".repeat(4))
    .bind(&id)
    .execute(&pool)
    .await
    .expect("managed row insert should satisfy the schema");
    let (status, body) = verify(app, &managed_id.to_string()).await;
    assert_eq!(status, StatusCode::NOT_IMPLEMENTED, "{body}");
    assert_eq!(body["error"]["code"], "not_implemented");
}
