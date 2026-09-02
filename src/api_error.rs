//! The consistent JSON error envelope every HTTP operation responds with on
//! failure. See `specs/ingestion-service` - "HTTP error responses use a
//! consistent JSON envelope".

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use utoipa::ToSchema;

/// The `error` object inside an [`ApiErrorResponse`].
#[derive(Debug, Serialize, ToSchema)]
pub struct ApiErrorBody {
    /// Stable, machine-readable failure category. Current values: `invalid_request`,
    /// `not_found`, `artifact_file_missing`, `payload_too_large`, `conflict`,
    /// `not_implemented`, `internal_error`. This field is an open string, not a
    /// closed enum - new categories may be added as the API grows.
    #[schema(example = "invalid_request")]
    pub code: String,
    /// Human-readable description of the failure. Not guaranteed stable across
    /// versions or releases - match on `code`, not this text.
    #[schema(example = "gpu_clock_mhz must be greater than zero")]
    pub message: String,
    /// Optional structured context about the failure, e.g. which field was invalid.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
    /// Opaque identifier correlating this error with server-side logs, when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

/// The consistent JSON envelope every rejected or failed request responds with.
#[derive(Debug, Serialize, ToSchema)]
pub struct ApiErrorResponse {
    pub error: ApiErrorBody,
}

/// An in-flight API error: an HTTP status plus the body that gets wrapped in
/// an [`ApiErrorResponse`] when converted into a response.
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    body: ApiErrorBody,
}

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        ApiError {
            status,
            body: ApiErrorBody {
                code: code.to_string(),
                message: message.into(),
                details: None,
                request_id: None,
            },
        }
    }

    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.body.details = Some(details);
        self
    }

    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "invalid_request", message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", message)
    }

    pub fn artifact_file_missing(message: impl Into<String>) -> Self {
        Self::new(StatusCode::GONE, "artifact_file_missing", message)
    }

    pub fn payload_too_large(message: impl Into<String>) -> Self {
        Self::new(StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large", message)
    }

    /// The request names a resource that already exists (e.g. a run ID
    /// posted twice). The existing resource is left unchanged.
    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, "conflict", message)
    }

    /// A validation failure naming the offending request field in
    /// `details.field`.
    pub fn invalid_field(field: &str, message: impl Into<String>) -> Self {
        Self::invalid_request(message).with_details(serde_json::json!({ "field": field }))
    }

    pub fn not_implemented(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_IMPLEMENTED, "not_implemented", message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", message)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(ApiErrorResponse { error: self.body })).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    #[tokio::test]
    async fn conflict_serialises_the_envelope_at_409() {
        let response = ApiError::conflict("run already exists").into_response();
        assert_eq!(response.status(), StatusCode::CONFLICT);
        let bytes = to_bytes(response.into_body(), 1024).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["error"]["code"], "conflict");
        assert_eq!(body["error"]["message"], "run already exists");
        assert!(body["error"].get("details").is_none());
    }

    #[tokio::test]
    async fn invalid_field_names_the_field_in_details() {
        let response = ApiError::invalid_field("bsp_version", "required").into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let bytes = to_bytes(response.into_body(), 1024).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["error"]["code"], "invalid_request");
        assert_eq!(body["error"]["details"]["field"], "bsp_version");
    }
}
