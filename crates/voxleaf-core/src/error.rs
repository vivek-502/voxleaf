//! One error type for the whole API surface.
//!
//! The web app answered with `{ error: { code, message, details? } }` and mapped
//! internal failures onto friendly, specific messages — an internal detail was
//! logged, never returned. This mirrors that contract, including the status
//! codes, so a client error looks the same whether it came from Node or Rust.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};

pub type ApiResult<T> = Result<T, ApiError>;

#[derive(Debug, Clone)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: String,
    pub message: String,
    pub details: Option<Value>,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ApiError {}

impl ApiError {
    pub fn new(status: StatusCode, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status,
            code: code.into(),
            message: message.into(),
            details: None,
        }
    }

    pub fn with_details(mut self, details: Value) -> Self {
        self.details = Some(details);
        self
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "bad_request", message)
    }

    pub fn unauthorized() -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "This instance requires an access token.",
        )
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", message)
    }

    pub fn too_large(message: impl Into<String>) -> Self {
        Self::new(StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large", message)
    }

    pub fn rate_limited() -> Self {
        Self::new(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
            "Too many requests. Please slow down.",
        )
    }

    pub fn bad_origin() -> Self {
        Self::new(
            StatusCode::FORBIDDEN,
            "bad_origin",
            "Cross-origin requests are not allowed.",
        )
    }

    /// An upload the validator refused: the field is named so the UI can point
    /// at the offending file.
    pub fn upload(field: &str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNPROCESSABLE_ENTITY, format!("upload_{field}"), message)
    }

    /// The book metadata provider could not be reached or said no.
    pub fn provider_unavailable() -> Self {
        Self::new(
            StatusCode::BAD_GATEWAY,
            "provider_unavailable",
            "The book catalogue is not responding right now. Please try again in a moment.",
        )
    }

    /// Translation failed. The provider's own words are the useful thing to show
    /// a reader who cannot translate: a refusal by the free endpoint is a fact
    /// about their connection, not a bug to retry past.
    pub fn translation(message: impl Into<String>, status: StatusCode) -> Self {
        Self::new(status, "translation_unavailable", message)
    }

    /// A validation failure, shaped exactly like the web app's Zod reports:
    /// `details: [{ path, message }]`.
    pub fn invalid(path: &str, message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            "bad_request",
            "Some of the submitted values are not valid.",
        )
        .with_details(json!([{ "path": path, "message": message.into() }]))
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", message)
    }
}

impl From<std::io::Error> for ApiError {
    fn from(error: std::io::Error) -> Self {
        tracing::error!(%error, "io error");
        match error.kind() {
            std::io::ErrorKind::NotFound => Self::not_found("That file is no longer available."),
            std::io::ErrorKind::PermissionDenied => {
                Self::internal("VoxLeaf could not read that file.")
            }
            std::io::ErrorKind::StorageFull => {
                Self::internal("There is no disk space left. Free some and try again.")
            }
            _ => Self::internal("Something went wrong on our side. Please try again."),
        }
    }
}

impl From<rusqlite::Error> for ApiError {
    fn from(error: rusqlite::Error) -> Self {
        tracing::error!(%error, "database error");
        // A malformed query or a constraint violation is a bug, but the user is
        // still owed something legible rather than a stack trace.
        match &error {
            rusqlite::Error::SqliteFailure(inner, _)
                if inner.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                Self::internal("That change conflicts with an existing record.")
            }
            _ => Self::internal("Something went wrong on our side. Please try again."),
        }
    }
}

impl From<reqwest::Error> for ApiError {
    fn from(error: reqwest::Error) -> Self {
        tracing::error!(%error, "http client error");
        if error.is_timeout() {
            Self::new(
                StatusCode::GATEWAY_TIMEOUT,
                "request_failed",
                "The service did not respond in time. Please try again.",
            )
        } else {
            Self::new(
                StatusCode::BAD_GATEWAY,
                "request_failed",
                "Could not reach the service. Please try again.",
            )
        }
    }
}

impl From<serde_json::Error> for ApiError {
    fn from(error: serde_json::Error) -> Self {
        tracing::error!(%error, "json error");
        Self::bad_request("That request could not be understood.")
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        // Internal detail is logged, never returned.
        if self.status.is_server_error() {
            tracing::error!(code = %self.code, message = %self.message, "request failed");
        }

        let mut body = json!({ "code": self.code, "message": self.message });
        if let Some(details) = self.details {
            body["details"] = details;
        }

        (self.status, Json(json!({ "error": body }))).into_response()
    }
}

/// Wrap a handler's fallible result so an unexpected panic becomes a clean 500
/// rather than a dropped connection.
pub fn internal_error(message: impl Into<String>) -> ApiError {
    ApiError::internal(message)
}
