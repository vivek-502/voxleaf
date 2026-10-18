//! `GET /api/covers?u=<remote url>` — artwork proxy with a local disk cache.
//!
//! Two jobs, the same the web app's route did:
//!
//!  1. Keep third-party image hosts out of the page (no hotlinking, no
//!     cross-origin surprises, and the app still renders artwork offline once
//!     cached).
//!  2. Reject anything that is not a plain HTTPS image from a known cover
//!     host, so this route cannot be turned into an open proxy.
//!
//! The shipped UI passes provider cover URLs through untouched — the same as
//! the web app, whose `coverProxyUrl` hook exists but is never called — so the
//! route is reachable but unused. It is reproduced rather than dropped because
//! it is part of the API contract a client could address, and because the disk
//! cache is what makes offline artwork possible at all.
//!
//! Unlike the rest of the API this route answers with image bytes, or a
//! plain-text error on a bare status code. It does not use the `{ data }` /
//! `{ error }` envelope, and inventing one here would be a *new* contract
//! rather than the web app's.

use std::path::PathBuf;
use std::time::Duration;

use axum::body::Body;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use futures_util::StreamExt as _;
use reqwest::Url;

use crate::config::Config;

/// The web app's `FETCH_TIMEOUT_MS`.
const FETCH_TIMEOUT: Duration = Duration::from_secs(8);
/// The web app's `MAX_COVER_BYTES`.
const MAX_COVER_BYTES: u64 = 4 * 1024 * 1024;

/// The only hosts the proxy will talk to, matching the web app's
/// `ALLOWED_HOSTS`. Open Library for the store, the three Amazon image hosts
/// for books imported with an Amazon cover.
const ALLOWED_HOSTS: &[&str] = &[
    "covers.openlibrary.org",
    "images-na.ssl-images-amazon.com",
    "images.amazon.com",
    "m.media-amazon.com",
];

/// A failure the web app answered with a bare status and sentence.
type ProxyError = (StatusCode, &'static str);

pub struct CoverService {
    cover_cache_dir: PathBuf,
    http: reqwest::Client,
}

impl CoverService {
    pub fn new(cfg: &Config) -> Self {
        Self {
            cover_cache_dir: cfg.cover_cache_dir.clone(),
            http: reqwest::Client::new(),
        }
    }

    /// Answer one proxy request. `raw` is the `u` query parameter exactly as
    /// it arrived: `None` when absent (400 "Missing image URL.") and
    /// `Some("")` when present but empty (400 "Invalid image URL."), which is
    /// what `new URL()` failing on an empty string produced upstream.
    pub async fn proxy(&self, raw: Option<&str>) -> Response {
        let Some(raw) = raw else {
            return plain_text(StatusCode::BAD_REQUEST, "Missing image URL.");
        };

        let target = match Url::parse(raw) {
            Ok(url) => url,
            Err(_) => return plain_text(StatusCode::BAD_REQUEST, "Invalid image URL."),
        };

        let host = target.host_str().unwrap_or_default();
        if target.scheme() != "https" || !ALLOWED_HOSTS.contains(&host) {
            return plain_text(StatusCode::FORBIDDEN, "Image host is not allowed.");
        }

        let cache_path = self.cover_cache_dir.join(format!("{}.img", cache_key(&target)));

        // A cache hit answers without touching the network, which is what
        // keeps artwork on the page offline.
        if let Ok(cached) = std::fs::read(&cache_path) {
            return image_response(&cached, "image/jpeg");
        }

        match self.fetch_cover(&target).await {
            Ok((bytes, content_type)) => {
                // Best-effort: a full disk must not fail a request that
                // already holds its bytes. (The web route swallowed this
                // error the same way.)
                if let Err(error) = std::fs::write(&cache_path, &bytes) {
                    tracing::warn!(%error, "could not cache a cover");
                }
                image_response(&bytes, &content_type)
            }
            Err((status, message)) => plain_text(status, message),
        }
    }

    /// Fetch the image, enforcing the size cap as the body arrives so a
    /// hostile upstream cannot exhaust memory before the check.
    async fn fetch_cover(&self, target: &Url) -> Result<(Vec<u8>, String), ProxyError> {
        let response = self
            .http
            .get(target.clone())
            .timeout(FETCH_TIMEOUT)
            .header(header::ACCEPT, "image/*")
            .header(header::USER_AGENT, "VoxLeaf/1.0")
            .send()
            .await
            // Both a refused connection and an 8 s abort land here, exactly as
            // the web route's `catch` mapped them.
            .map_err(|_| (StatusCode::GATEWAY_TIMEOUT, "Cover unavailable."))?;

        if !response.status().is_success() {
            return Err((StatusCode::BAD_GATEWAY, "Cover unavailable."));
        }

        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        if !content_type.starts_with("image/") {
            return Err((
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "Upstream did not return an image.",
            ));
        }

        let mut bytes = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk
                .map_err(|_| (StatusCode::GATEWAY_TIMEOUT, "Cover unavailable."))?;
            bytes.extend_from_slice(&chunk);
            if bytes.len() as u64 > MAX_COVER_BYTES {
                return Err((StatusCode::PAYLOAD_TOO_LARGE, "Cover is too large."));
            }
        }

        Ok((bytes, content_type))
    }
}

/// The cache file name, mirroring the web proxy:
/// `sha256(url).hex()[0..40] + ".img"`.
fn cache_key(url: &Url) -> String {
    use sha2::{Digest, Sha256};
    let hex = hex::encode(Sha256::digest(url.as_str().as_bytes()));
    hex[..40].to_string()
}

fn plain_text(status: StatusCode, message: &'static str) -> Response {
    (status, message).into_response()
}

fn image_response(bytes: &[u8], content_type: &str) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_LENGTH, bytes.len().to_string())
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .header(header::CACHE_CONTROL, "public, max-age=2592000, immutable")
        .body(Body::from(bytes.to_vec()))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service() -> CoverService {
        let dir = tempfile::tempdir().expect("temp dir");
        let cfg = Config::for_data_dir(dir.path().to_path_buf());
        // Keep the directory alive for the cache read by leaking it: these
        // tests are short-lived and the paths must not vanish mid-request.
        let path = cfg.cover_cache_dir.clone();
        std::mem::forget(dir);
        std::fs::create_dir_all(&path).expect("cache dir");
        CoverService::new(&cfg)
    }

    #[tokio::test]
    async fn a_missing_url_is_a_400_in_plain_text() {
        let response = service().proxy(None).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(response.into_body(), 1024).await.unwrap();
        assert_eq!(&body[..], b"Missing image URL.");
    }

    #[tokio::test]
    async fn an_empty_url_is_invalid_not_missing() {
        let response = service().proxy(Some("")).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(response.into_body(), 1024).await.unwrap();
        assert_eq!(&body[..], b"Invalid image URL.");
    }

    #[tokio::test]
    async fn a_relative_reference_is_invalid() {
        let response = service().proxy(Some("/covers/1.jpg")).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn plain_http_is_rejected() {
        let response = service()
            .proxy(Some("http://covers.openlibrary.org/b/id/1-L.jpg"))
            .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn an_unlisted_host_is_rejected() {
        let response = service()
            .proxy(Some("https://example.com/b/id/1-L.jpg"))
            .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body = axum::body::to_bytes(response.into_body(), 1024).await.unwrap();
        assert_eq!(&body[..], b"Image host is not allowed.");
    }

    #[tokio::test]
    async fn a_file_url_is_not_an_image_host() {
        let response = service()
            .proxy(Some("file:///etc/passwd"))
            .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn a_cached_cover_is_served_without_the_network() {
        let service = service();
        let url = Url::parse("https://covers.openlibrary.org/b/id/42-L.jpg").expect("url");
        let path = service.cover_cache_dir.join(format!("{}.img", cache_key(&url)));
        std::fs::write(&path, b"cached-bytes").expect("seed cache");

        let response = service.proxy(Some(url.as_str())).await;
        assert_eq!(response.status(), StatusCode::OK);
        let headers = response.headers().clone();
        assert_eq!(headers[header::CONTENT_TYPE], "image/jpeg");
        assert_eq!(
            headers[header::CACHE_CONTROL],
            "public, max-age=2592000, immutable"
        );
        let body = axum::body::to_bytes(response.into_body(), 1024).await.unwrap();
        assert_eq!(&body[..], b"cached-bytes");
    }

    #[tokio::test]
    async fn cache_names_are_the_first_forty_hex_digits_of_the_sha256() {
        let url = Url::parse("https://covers.openlibrary.org/b/id/1-L.jpg").expect("url");
        // Independently computed: sha256 of the exact URL string.
        assert_eq!(
            cache_key(&url),
            "b2b105e0f4019d695b7037d771683d465a96ca01"
        );
    }
}
