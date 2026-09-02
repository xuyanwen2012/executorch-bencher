//! Request extractors whose rejections use the JSON error envelope.
//!
//! Axum's built-in `Query`, `Path`, and `Json` extractors reject malformed
//! input with a plain-text body, which would let `?limit=abc` or
//! `/runs/not-a-uuid` bypass the contract. These thin wrappers delegate to
//! the built-ins and convert every rejection into an `ApiError` so the
//! envelope is uniform. See `specs/ingestion-service` - "HTTP error
//! responses use a consistent JSON envelope".

use crate::api_error::ApiError;
use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::extract::{FromRequest, FromRequestParts, Request};
use axum::http::request::Parts;
use serde::de::DeserializeOwned;

/// `axum::extract::Query` with an enveloped `400 invalid_request` rejection.
#[derive(Debug, Clone, Copy, Default)]
pub struct Query<T>(pub T);

impl<T, S> FromRequestParts<S> for Query<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        match axum::extract::Query::<T>::from_request_parts(parts, state).await {
            Ok(axum::extract::Query(value)) => Ok(Query(value)),
            Err(rejection) => Err(query_rejection(rejection)),
        }
    }
}

fn query_rejection(rejection: QueryRejection) -> ApiError {
    ApiError::invalid_request(format!("invalid query string: {}", rejection.body_text()))
}

/// `axum::extract::Path` with an enveloped `400 invalid_request` rejection
/// for a malformed path parameter (for example a run ID that is not a
/// UUID).
#[derive(Debug, Clone, Copy, Default)]
pub struct Path<T>(pub T);

impl<T, S> FromRequestParts<S> for Path<T>
where
    T: DeserializeOwned + Send,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        match axum::extract::Path::<T>::from_request_parts(parts, state).await {
            Ok(axum::extract::Path(value)) => Ok(Path(value)),
            Err(rejection) => Err(path_rejection(rejection)),
        }
    }
}

fn path_rejection(rejection: PathRejection) -> ApiError {
    ApiError::invalid_request(format!("invalid path parameter: {}", rejection.body_text()))
}

/// `axum::extract::Json` with enveloped rejections: `400 invalid_request`
/// for a missing/incorrect content type or a body that does not
/// deserialize, `413 payload_too_large` when the body exceeds the default
/// body limit.
#[derive(Debug, Clone, Copy, Default)]
pub struct Json<T>(pub T);

impl<T, S> FromRequest<S> for Json<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match axum::extract::Json::<T>::from_request(req, state).await {
            Ok(axum::extract::Json(value)) => Ok(Json(value)),
            Err(rejection) => Err(json_rejection(rejection)),
        }
    }
}

fn json_rejection(rejection: JsonRejection) -> ApiError {
    match rejection {
        JsonRejection::BytesRejection(inner)
            if inner.status() == axum::http::StatusCode::PAYLOAD_TOO_LARGE =>
        {
            ApiError::payload_too_large("request body exceeds the maximum size")
        }
        other => ApiError::invalid_request(format!("invalid JSON body: {}", other.body_text())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::body::{Body, to_bytes};
    use axum::http::{Request, StatusCode};
    use axum::routing::{get, post};
    use serde::Deserialize;
    use tower::ServiceExt;

    #[derive(Deserialize)]
    struct Params {
        #[allow(dead_code)]
        limit: u32,
    }

    #[derive(Deserialize)]
    struct Payload {
        #[allow(dead_code)]
        name: String,
    }

    fn app() -> Router {
        Router::new()
            .route("/q", get(|Query(_): Query<Params>| async { "ok" }))
            .route("/p/{id}", get(|Path(_): Path<uuid::Uuid>| async { "ok" }))
            .route("/j", post(|Json(_): Json<Payload>| async { "ok" }))
    }

    async fn envelope(response: axum::response::Response) -> (StatusCode, serde_json::Value) {
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    #[tokio::test]
    async fn a_malformed_query_string_is_enveloped() {
        let response = app()
            .oneshot(Request::builder().uri("/q?limit=abc").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let (status, body) = envelope(response).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "invalid_request");
        assert!(body["error"]["message"].as_str().unwrap().contains("limit"));
    }

    #[tokio::test]
    async fn a_malformed_path_parameter_is_enveloped() {
        let response = app()
            .oneshot(Request::builder().uri("/p/not-a-uuid").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let (status, body) = envelope(response).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "invalid_request");
    }

    #[tokio::test]
    async fn a_malformed_json_body_is_enveloped() {
        let response = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/j")
                    .header("content-type", "application/json")
                    .body(Body::from("{not json"))
                    .unwrap(),
            )
            .await
            .unwrap();
        let (status, body) = envelope(response).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "invalid_request");
    }

    #[tokio::test]
    async fn a_missing_json_content_type_is_enveloped() {
        let response = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/j")
                    .body(Body::from(r#"{"name":"x"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        let (status, body) = envelope(response).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "invalid_request");
    }

    #[tokio::test]
    async fn an_oversized_json_body_is_enveloped_as_413() {
        let app = app().layer(axum::extract::DefaultBodyLimit::max(16));
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/j")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"name":"this body is longer than sixteen bytes"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        let (status, body) = envelope(response).await;
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(body["error"]["code"], "payload_too_large");
    }
}
