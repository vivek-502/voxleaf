//! Object storage for uploaded audio, and the signed URLs that serve it.
//!
//! The web app could point at local disk or at S3; a desktop app has no reason
//! to leave the machine, so this is the local driver. What is kept from the
//! original is the part that mattered for safety: an opaque, date-sharded key
//! that is never derived from a user filename, and short-lived signed stream
//! URLs checked in constant time.

use std::path::{Path, PathBuf};

use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::error::{ApiError, ApiResult};

type HmacSha256 = Hmac<Sha256>;

/// How long a stream URL stays valid. Long enough to start a chapter, short
/// enough that a leaked link is worthless by the time it is shared.
const STREAM_URL_TTL_SECONDS: i64 = 900;

#[derive(Debug, Clone)]
pub struct LocalStorage {
    root: PathBuf,
}

impl LocalStorage {
    pub fn new(root: PathBuf) -> anyhow::Result<Self> {
        std::fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    /// A storage key for a new upload: random id plus a validated extension,
    /// sharded by date so no directory grows unbounded.
    ///
    /// The extension is validated against an allow-list rather than taken from
    /// the filename, because the key becomes a path on disk.
    pub fn key_for(extension: &str, allowed: &[&str]) -> ApiResult<String> {
        let normalised = normalise_extension(extension);
        if !allowed.contains(&normalised.as_str()) {
            return Err(ApiError::bad_request("That audio format is not supported."));
        }

        let now = chrono::Utc::now();
        let shard = now.format("%Y/%m/%d").to_string();
        let id: String = nanoid::nanoid!(16);
        Ok(format!("{shard}/{id}{normalised}"))
    }

    /// Resolve a key to a path, refusing anything that would escape the root.
    pub fn path_for(&self, key: &str) -> ApiResult<PathBuf> {
        if key.contains('\0') || key.split('/').any(|part| part == "..") || key.starts_with('/') {
            return Err(ApiError::bad_request("That storage key is not valid."));
        }

        let path = self.root.join(key);
        let root = self
            .root
            .canonicalize()
            .unwrap_or_else(|_| self.root.clone());

        // The parent may not exist yet (a brand new shard), so compare against
        // the nearest existing ancestor rather than the leaf.
        let mut probe = path.clone();
        while !probe.exists() {
            probe = match probe.parent() {
                Some(parent) => parent.to_path_buf(),
                None => return Err(ApiError::bad_request("That storage key is not valid.")),
            };
        }
        let probe = probe.canonicalize().map_err(|_| {
            ApiError::bad_request("That storage key is not valid.")
        })?;

        if !probe.starts_with(&root) {
            return Err(ApiError::bad_request("That storage key is not valid."));
        }

        Ok(path)
    }

    /// Write bytes to a key atomically: temp file, then rename.
    pub fn put(&self, key: &str, bytes: &[u8]) -> ApiResult<u64> {
        let path = self.path_for(key)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let temp = path.with_extension(format!(
            "part-{}",
            nanoid::nanoid!(8)
        ));

        std::fs::write(&temp, bytes)
            .map_err(|error| write_error(error, "That file could not be saved."))?;
        std::fs::rename(&temp, &path)
            .map_err(|error| write_error(error, "That file could not be saved."))?;

        Ok(bytes.len() as u64)
    }

    pub fn exists(&self, key: &str) -> bool {
        self.path_for(key).map(|path| path.exists()).unwrap_or(false)
    }

    pub fn len(&self, key: &str) -> ApiResult<u64> {
        let path = self.path_for(key)?;
        let metadata = std::fs::metadata(&path)
            .map_err(|_| ApiError::not_found("That file is no longer available."))?;
        Ok(metadata.len())
    }

    pub fn delete(&self, key: &str) -> ApiResult<()> {
        let path = self.path_for(key)?;
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(write_error(error, "That file could not be removed.")),
        }
    }

    /// Read a byte range from a stored object without loading the whole file.
    pub fn read_range_file(&self, key: &str, range: ByteRange) -> ApiResult<Vec<u8>> {
        let path = self.path_for(key)?;
        read_range(&path, range)
    }
}

fn write_error(error: std::io::Error, fallback: &str) -> ApiError {
    // Internal detail is logged; the user gets a legible sentence.
    tracing::error!(%error, "storage error");
    match error.kind() {
        std::io::ErrorKind::NotFound => ApiError::not_found("That file is no longer available."),
        std::io::ErrorKind::PermissionDenied => {
            ApiError::internal("VoxLeaf could not read that file.")
        }
        std::io::ErrorKind::StorageFull => {
            ApiError::internal("There is no disk space left. Free some and try again.")
        }
        _ => ApiError::internal(fallback),
    }
}

/// Lower-case an extension and add the leading dot.
pub fn normalise_extension(extension: &str) -> String {
    let trimmed = extension.trim().trim_start_matches('.');
    format!(".{}", trimmed.to_ascii_lowercase())
}

/// Best-effort content type for an allowed extension.
pub fn mime_for_extension(extension: &str) -> String {
    match normalise_extension(extension).as_str() {
        ".mp3" => "audio/mpeg",
        ".m4a" | ".mp4" => "audio/mp4",
        ".aac" => "audio/aac",
        ".wav" | ".wave" => "audio/wav",
        ".ogg" | ".oga" | ".opus" => "audio/ogg",
        ".flac" => "audio/flac",
        _ => "application/octet-stream",
    }
    .to_string()
}

/* -------------------------------------------------------------------------- */
/* Signed stream URLs                                                         */
/* -------------------------------------------------------------------------- */

/// Signs `audioId` with the app's secret, producing an expiring token.
pub fn sign_stream(secret: &[u8], audio_id: &str, expires_at: i64) -> String {
    let payload = format!("{audio_id}.{expires_at}");
    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(payload.as_bytes());
    let digest = mac.finalize().into_bytes();

    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest)
}

/// Constant-time comparison, so a wrong token is not distinguishable by timing.
pub fn verify_stream(secret: &[u8], audio_id: &str, expires_at: i64, token: &str) -> bool {
    if expires_at < now_seconds() {
        return false;
    }

    let expected = sign_stream(secret, audio_id, expires_at);
    if expected.len() != token.len() {
        return false;
    }

    expected
        .bytes()
        .zip(token.bytes())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}

pub fn now_seconds() -> i64 {
    chrono::Utc::now().timestamp()
}

pub fn stream_url(secret: &[u8], audio_id: &str) -> String {
    let expires_at = now_seconds() + STREAM_URL_TTL_SECONDS;
    let token = sign_stream(secret, audio_id, expires_at);
    format!("/api/audio/{audio_id}?token={token}&exp={expires_at}")
}

/* -------------------------------------------------------------------------- */
/* Range requests                                                             */
/* -------------------------------------------------------------------------- */

/// A parsed, clamped HTTP byte range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteRange {
    pub start: u64,
    pub end: u64,
    pub len: u64,
}

impl ByteRange {
    pub fn content_range(&self, total: u64) -> String {
        format!("bytes {}-{}/{}", self.start, self.end, total)
    }
}

/// Parse a `Range: bytes=…` header against a known size.
///
/// Returns `None` when the header is absent or unparseable, which the caller
/// answers with the whole file. Suffix (`bytes=-500`) and open-ended
/// (`bytes=500-`) forms are both supported, and ranges are clamped to the file.
pub fn parse_range(header: Option<&str>, total: u64) -> Option<ByteRange> {
    let header = header?.trim();
    let spec = header.strip_prefix("bytes=")?;

    // Multiple ranges are legal but nobody needs them for audio seeking.
    let spec = spec.split(',').next()?.trim();

    let (start_text, end_text) = spec.split_once('-')?;

    if start_text.is_empty() {
        // Suffix range: the last N bytes.
        let suffix: u64 = end_text.parse().ok()?;
        if suffix == 0 || total == 0 {
            return None;
        }
        let start = total.saturating_sub(suffix);
        return Some(ByteRange {
            start,
            end: total - 1,
            len: total - start,
        });
    }

    let start: u64 = start_text.parse().ok()?;

    if start >= total {
        return None;
    }

    let end = if end_text.is_empty() {
        total - 1
    } else {
        end_text.parse::<u64>().ok()?.min(total - 1)
    };

    if end < start {
        return None;
    }

    Some(ByteRange {
        start,
        end,
        len: end - start + 1,
    })
}

/// Read a byte range from a file without loading the whole thing.
pub fn read_range(path: &Path, range: ByteRange) -> ApiResult<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};

    let mut file = std::fs::File::open(path)
        .map_err(|_| ApiError::not_found("That file is no longer available."))?;

    file.seek(SeekFrom::Start(range.start))
        .map_err(|_| ApiError::not_found("That file is no longer available."))?;

    let mut buffer = vec![0u8; range.len as usize];
    file.read_exact(&mut buffer)
        .map_err(|_| ApiError::not_found("That file is no longer available."))?;

    Ok(buffer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_ended_range_is_clamped_to_the_file() {
        let range = parse_range(Some("bytes=100-"), 1000).expect("range");
        assert_eq!(range, ByteRange { start: 100, end: 999, len: 900 });
        assert_eq!(range.content_range(1000), "bytes 100-999/1000");
    }

    #[test]
    fn suffix_range_takes_the_tail() {
        let range = parse_range(Some("bytes=-250"), 1000).expect("range");
        assert_eq!(range, ByteRange { start: 750, end: 999, len: 250 });
    }

    #[test]
    fn closed_range_is_honoured() {
        assert_eq!(
            parse_range(Some("bytes=0-499"), 1000),
            Some(ByteRange { start: 0, end: 499, len: 500 })
        );
    }

    #[test]
    fn malformed_or_out_of_bounds_ranges_fall_back_to_the_whole_file() {
        assert_eq!(parse_range(None, 1000), None);
        assert_eq!(parse_range(Some("bytes=abc-"), 1000), None);
        assert_eq!(parse_range(Some("items=0-5"), 1000), None);
        assert_eq!(parse_range(Some("bytes=1000-"), 1000), None);
        assert_eq!(parse_range(Some("bytes=500-100"), 1000), None);
    }

    #[test]
    fn tokens_verify_only_for_the_right_id_and_within_the_window() {
        let secret = b"super-secret-value-for-tests-here";
        let expires = now_seconds() + 60;
        let token = sign_stream(secret, "au_123", expires);

        assert!(verify_stream(secret, "au_123", expires, &token));
        assert!(!verify_stream(secret, "au_other", expires, &token));
        assert!(!verify_stream(secret, "au_123", expires, "not-the-token"));
        assert!(!verify_stream(b"different-secret", "au_123", expires, &token));
        assert!(!verify_stream(secret, "au_123", now_seconds() - 1, &token));
    }

    #[test]
    fn keys_are_sharded_and_never_derived_from_a_filename() {
        let allowed = [".mp3", ".m4a"];
        let key = LocalStorage::key_for(".mp3", &allowed).expect("key");

        assert!(key.contains('/'), "expected a date shard, got {key}");
        assert!(key.ends_with(".mp3"));
        assert!(!key.contains(".."));

        assert!(LocalStorage::key_for(".exe", &allowed).is_err());
        assert!(LocalStorage::key_for("../../etc/passwd", &allowed).is_err());
    }

    #[test]
    fn path_resolution_refuses_to_escape_the_root() {
        let root = tempfile::tempdir().expect("temp dir");
        let storage = LocalStorage::new(root.path().to_path_buf()).expect("storage");

        assert!(storage.path_for("2026/01/01/abc.mp3").is_ok());
        assert!(storage.path_for("../../etc/passwd").is_err());
        assert!(storage.path_for("/etc/passwd").is_err());
        assert!(storage.path_for("a\0b").is_err());
    }
}
