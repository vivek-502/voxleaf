//! Serving uploaded audio with HTTP Range support.
//!
//! `<audio src>` seeking depends on `Range` requests, so this is a real
//! range server: 206 with `Content-Range` for a satisfiable range, 200 with the
//! whole file otherwise, and 416 when the range lies beyond the end. The URL is
//! short-lived and signed, exactly as the web app's were.

use axum::body::Body;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::Value;

use crate::api::AppState;
use crate::error::{ApiError, ApiResult};
use crate::storage::{self, ByteRange};

/// Answer a request for one audio file.
pub async fn serve_audio(
    st: &AppState,
    audio_id: &str,
    req: axum::extract::Request,
) -> ApiResult<Response> {
    verify_token(st, audio_id, req.uri().query())?;

    let audio_id_owned = audio_id.to_string();
    let row = st
        .db
        .call(move |conn| {
            conn.query_row(
                "SELECT storage_key, mime_type, duration, original_name \
                 FROM chapter_audio WHERE id = ?1",
                [audio_id_owned],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, f64>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                },
            )
        })
        .await
        .map_err(|_| ApiError::not_found("That audio is no longer available."))?;

    let (key, mime, duration, original_name) = row;
    let total = st
        .cfg
        .storage
        .len(&key)
        .map_err(|_| ApiError::not_found("That audio is no longer available."))?;

    if total == 0 {
        return Err(ApiError::not_found("That audio file is empty."));
    }

    let headers = req.headers();
    let range = storage::parse_range(headers.get(header::RANGE).and_then(|v| v.to_str().ok()), total);

    let mut response = match range {
        // A satisfiable range: 206 Partial Content, which is what lets the
        // player seek without downloading the file first.
        Some(range) => {
            let bytes = st.cfg.storage.read_range_file(&key, range)?;
            let mut response = Response::builder()
                .status(StatusCode::PARTIAL_CONTENT)
                .header(header::CONTENT_TYPE, mime)
                .header(header::ACCEPT_RANGES, "bytes")
                .header(header::CONTENT_LENGTH, range.len)
                .header(header::CONTENT_RANGE, range.content_range(total))
                .body(Body::from(bytes))
                .map_err(|e| ApiError::internal(format!("response build failed: {e}")))?;
            apply_common_headers(response.headers_mut(), audio_id, duration, &original_name);
            response
        }
        // No range, or an unsatisfiable one that we degrade to a full read when
        // the client sent nothing better.
        None => {
            if headers.get(header::RANGE).is_some() {
                // A range the server cannot satisfy: the spec wants 416.
                return Ok(Response::builder()
                    .status(StatusCode::RANGE_NOT_SATISFIABLE)
                    .header(header::CONTENT_RANGE, format!("bytes */{total}"))
                    .body(Body::empty())
                    .unwrap_or_else(|_| StatusCode::RANGE_NOT_SATISFIABLE.into_response()));
            }
            let bytes = st.cfg.storage.read_range_file(
                &key,
                ByteRange { start: 0, end: total - 1, len: total },
            )?;
            let mut response = Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, mime)
                .header(header::ACCEPT_RANGES, "bytes")
                .header(header::CONTENT_LENGTH, total)
                .body(Body::from(bytes))
                .map_err(|e| ApiError::internal(format!("response build failed: {e}")))?;
            apply_common_headers(response.headers_mut(), audio_id, duration, &original_name);
            response
        }
    };

    // Downloads must not be sniffed or framed by anything else.
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("private, max-age=3600"),
    );
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        header::HeaderValue::from_static("nosniff"),
    );

    Ok(response)
}

fn apply_common_headers(
    headers: &mut HeaderMap,
    audio_id: &str,
    duration: f64,
    original_name: &Option<String>,
) {
    let _ = audio_id;
    // The duration rides along so a player can paint a timeline before the
    // first byte arrives.
    if let Ok(value) = header::HeaderValue::from_str(&format!("{duration:.3}")) {
        headers.insert("x-audio-duration", value);
    }
    if let Some(name) = original_name {
        if let Ok(value) = header::HeaderValue::from_str(&sanitize_filename(name)) {
            headers.insert("x-original-name", value);
        }
    }
}

/// RFC 5987 filename, so a non-ASCII title survives the header round trip.
fn sanitize_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_control() || c == '"' { '_' } else { c })
        .collect();
    format!("UTF-8''{}", urlencoding::encode(&cleaned))
}

/// Check the signed token that guards every stream URL.
fn verify_token(st: &AppState, audio_id: &str, query: Option<&str>) -> ApiResult<()> {
    let secret = st.cfg.stream_secret();
    let mut token = None;
    let mut exp: Option<i64> = None;

    if let Some(query) = query {
        for pair in query.split('&') {
            let Some((key, value)) = pair.split_once('=') else {
                continue;
            };
            match key {
                "token" => token = Some(value.to_string()),
                "exp" => exp = value.parse::<i64>().ok(),
                _ => {}
            }
        }
    }

    let (Some(token), Some(exp)) = (token, exp) else {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "invalid_stream_url",
            "This audio link is missing its signature.",
        ));
    };

    if !storage::verify_stream(&secret, audio_id, exp, &token) {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "invalid_stream_url",
            "This audio link has expired.",
        ));
    }

    Ok(())
}

/// Public helper used by the TTS and playback services.
pub fn signed_url(st: &AppState, audio_id: &str) -> String {
    storage::stream_url(&st.cfg.stream_secret(), audio_id)
}

/// Re-point a payload's placeholder stream URLs at freshly signed ones.
///
/// The web app mints signatures at request time — `resolveStreamUrl` is called
/// by every route that ships a chapter to the player (`books/[id]`,
/// `library/[libraryId]`, `playback/resolve`, chapter audio, TTS) — so a URL
/// read out of a stored row is never stale. Only the `streamUrl` key is
/// rewritten: `fileUrl` is a stored column and is passed through untouched,
/// exactly as the web app's mappers do.
pub fn sign_stream_urls(st: &AppState, value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, value)| {
                    let value = if key == "streamUrl" {
                        match &value {
                            Value::String(url) => match placeholder_audio_id(url) {
                                Some(audio_id) => Value::String(signed_url(st, &audio_id)),
                                None => value,
                            },
                            _ => value,
                        }
                    } else {
                        value
                    };
                    (key, sign_stream_urls(st, value))
                })
                .collect(),
        ),
        Value::Array(items) => {
            Value::Array(items.into_iter().map(|v| sign_stream_urls(st, v)).collect())
        }
        other => other,
    }
}

/// The audio id inside an unsigned `/api/audio/{id}` placeholder.
///
/// Anything with a query string is already signed (or pointing somewhere
/// else entirely) and is left alone.
fn placeholder_audio_id(url: &str) -> Option<String> {
    let rest = url.strip_prefix("/api/audio/")?;
    if rest.is_empty() || rest.contains('?') || rest.contains('/') {
        return None;
    }
    Some(rest.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placeholder() -> Value {
        serde_json::json!({
            "id": "bk1",
            "audio": { "id": "au1", "streamUrl": "/api/audio/au1", "fileUrl": "/api/audio/au1" },
        })
    }
}

/// Build the `{ data: … }` shape the player reads for a resolved stream.
pub fn stream_payload(audio_id: &str, url: &str, duration: f64, mime: &str) -> Value {
    serde_json::json!({
        "audioId": audio_id,
        "streamUrl": url,
        "duration": duration,
        "mimeType": mime,
    })
}
