//! Access-token status.
//!
//! This is *application* access control, not user authentication: VoxLeaf has
//! no accounts. A single shared optional token simply keeps an internet-facing
//! deployment from being open to anyone who guesses the URL, and a desktop
//! install — the case this port ships in — never sets one.

use serde_json::{json, Value};

use crate::db::Database;
use crate::error::{ApiError, ApiResult};

use super::library::sanitize_text;

/// GET /api/auth/status — is this instance open, or does it need a token?
pub fn status(_db: &Database) -> Value {
    match configured_token() {
        None => json!({ "required": false, "authorized": true }),
        // Without a request to inspect there is no token to check here, so a
        // configured instance reports "not yet authorized"; the login call
        // is what flips this to true.
        Some(_) => json!({ "required": true, "authorized": false }),
    }
}

/// POST /api/auth/status — exchange the shared token for a session.
pub fn login(_db: &Database, body: &Value) -> ApiResult<Value> {
    let Some(expected) = configured_token() else {
        return Ok(json!({ "required": false, "authorized": true }));
    };

    let provided = body
        .as_object()
        .and_then(|object| object.get("token"))
        .and_then(|value| value.as_str())
        .map(|value| sanitize_text(value, 400))
        .unwrap_or_default();

    if provided.is_empty() || provided != expected {
        return Err(ApiError::new(
            axum::http::StatusCode::UNAUTHORIZED,
            "unauthorized",
            "That access token is not valid.",
        ));
    }
    Ok(json!({ "required": true, "authorized": true }))
}

/// The shared token, or `None` when the instance is open (including before the
/// process-wide configuration has been initialised, e.g. in tests).
fn configured_token() -> Option<String> {
    crate::config::get()
        .ok()
        .map(|config| config.access_token.trim().to_string())
        .filter(|token| !token.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unconfigured_instance_is_open() {
        let fixture = crate::services::library::tests_support::fixture();
        assert_eq!(status(&fixture.db), json!({ "required": false, "authorized": true }));
        assert_eq!(
            login(&fixture.db, &json!({})).expect("login"),
            json!({ "required": false, "authorized": true })
        );
    }
}
