//! The read-aloud websocket protocol itself: handshake, SSML request, MP3 back.
//!
//! This is a port of `edge_tts/communicate.py` (v2 token era, Chromium 143
//! handshake), restricted to what VoxLeaf needs — audio only, no word or
//! sentence boundary metadata. The framing details are not ours: Microsoft's
//! service answers with text frames whose headers are CRLF-separated `Key:
//! Value` lines followed by a blank line and a body, and binary frames whose
//! first two bytes are the big-endian length of that same header block. The
//! port keeps the original's error conditions verbatim, because "we received a
//! binary message, but it is missing the header length" is exactly the kind of
//! message that tells someone debugging a Microsoft-side change where it broke.

use std::time::Duration;

use axum::http::StatusCode;
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_tungstenite::tungstenite::Error as WsError;

use crate::error::{ApiError, ApiResult};
use crate::services::tts::drm;

/// The trusted client token every endpoint is gated on.
pub(crate) const TRUSTED_CLIENT_TOKEN: &str = "6A5AA1D4EAFF4E9FB37E23D68491D6F4";

const WSS_URL: &str = concat!(
    "wss://speech.platform.bing.com/consumer/speech/synthesize/readaloud",
    "/edge/v1?TrustedClientToken=6A5AA1D4EAFF4E9FB37E23D68491D6F4"
);

/// The Chromium version Edge is on. The `Sec-MS-GEC-Version` header must
/// name the same build the `Sec-MS-GEC` token was derived for; a stale
/// version string is one of the things Microsoft hardens when it tightens
/// the handshake, which is why the web app pinned its Python dependency.
#[allow(dead_code)]
pub(crate) const CHROMIUM_FULL_VERSION: &str = "143.0.3650.75";
pub(crate) const SEC_MS_GEC_VERSION: &str = "2-143.0.3650.75";
pub(crate) const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
     (KHTML, like Gecko) Chrome/143.0.0.0 Safari/537.36 Edg/143.0.0.0";

/// The extension's origin the service expects to see.
pub(crate) const ORIGIN: &str = "chrome-extension://jdiccldimpdaibmpdkjnbmckianbfold";

/// The MP3 flavour the rest of VoxLeaf assumes: 24 kHz mono, 48 kbps
/// constant bitrate. `generate.rs` derives the chapter's duration from the
/// byte count with exactly this bitrate, so the two must not drift apart.
pub(crate) const OUTPUT_FORMAT: &str = "audio-24khz-48kbitrate-mono-mp3";

/// One websocket request's text is capped here, after XML escaping. The
/// original generator splits at 4096 *bytes*, not characters.
const TEXT_BYTE_LIMIT: usize = 4096;

/// How long the handshake may take, and how long we wait for any single
/// message once it has. Mirrors the original's connect/receive timeouts.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const RECEIVE_TIMEOUT: Duration = Duration::from_secs(60);

fn tts_err(message: impl Into<String>) -> ApiError {
    ApiError::new(StatusCode::BAD_GATEWAY, "tts_unavailable", message)
}

/// The long voice name the service is addressed by.
///
/// The original sends `Microsoft Server Speech Text to Speech Voice (en-US,
/// GuyNeural)` rather than the short `en-US-GuyNeural`, because that is what
/// real Edge does. A name that already is in long form passes through.
/// The `long form` of a voice name the service expects, from the short
/// `lang-Region-Name` shape.
///
/// Port of edge-tts's `TTSConfig.__post_init__`: `^([a-z]{2,})-([A-Z]{2,})-(.+Neural)$`,
/// and a name carrying its own hyphen folds its first segment into the
/// region — `en-US-Guy-Neural` is read as region `US-Guy`, name `Neural`.
pub(crate) fn long_voice_name(voice: &str) -> String {
    // Split into at most three pieces: the name owns whatever hyphens follow.
    let mut parts = voice.splitn(3, '-');
    let (Some(lang), Some(region), Some(name)) = (parts.next(), parts.next(), parts.next()) else {
        return voice.to_string();
    };
    if lang.len() < 2
        || region.len() < 2
        || !lang.chars().all(|c| c.is_ascii_lowercase())
        || !region.chars().all(|c| c.is_ascii_uppercase())
    {
        return voice.to_string();
    }
    // The short form only exists for the `...Neural` voices.
    if !name.ends_with("Neural") {
        return voice.to_string();
    }
    let (region, name) = match name.split_once('-') {
        Some((head, tail)) => (format!("{region}-{head}"), tail),
        None => (region.to_string(), name),
    };
    format!(
        "Microsoft Server Speech Text to Speech Voice ({lang}-{region}, {name})"
    )
}

/// Replace the control characters the service refuses — most importantly the
/// vertical tab, which OCR'd PDFs are full of.
pub(crate) fn remove_incompatible_characters(input: &str) -> String {
    input
        .chars()
        .map(|c| {
            let code = c as u32;
            if (0..=8).contains(&code) || (11..=12).contains(&code) || (14..=31).contains(&code) {
                ' '
            } else {
                c
            }
        })
        .collect()
}

/// The XML entity escape the original borrows from `xml.sax.saxutils`.
pub(crate) fn escape_xml(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

/// A per-connection request id, dashless, as the original's UUID.
pub(crate) fn connect_id() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// The JavaScript-style date string the service expects, in UTC regardless of
/// local time — the original's own comment admits `%Z` would lie here.
pub(crate) fn date_to_string() -> String {
    let now = chrono::Utc::now();
    now.format("%a %b %d %Y %H:%M:%S GMT+0000 (Coordinated Universal Time)")
        .to_string()
}

/// Find the rightmost newline (preferred) or space within the first `limit`
/// bytes. Returns `None` when neither is found in range.
fn find_last_newline_or_space_within_limit(text: &[u8], limit: usize) -> Option<usize> {
    let head = &text[..limit.min(text.len())];
    head.iter()
        .rposition(|b| *b == b'\n')
        .or_else(|| head.iter().rposition(|b| *b == b' '))
}

/// Find the largest prefix that is valid UTF-8, walking back from the end.
fn find_safe_utf8_split_point(text: &[u8]) -> usize {
    let mut split_at = text.len();
    while split_at > 0 {
        if std::str::from_utf8(&text[..split_at]).is_ok() {
            return split_at;
        }
        split_at -= 1;
    }
    split_at
}

/// Move a proposed split point back to the `&` of an XML entity that would
/// otherwise be cut in half — `&amp;` must never become `&am` + `p;`.
fn adjust_split_point_for_xml_entity(text: &[u8], split_at: isize) -> isize {
    let mut split_at = split_at;
    while split_at > 0 {
        let head = &text[..split_at as usize];
        let Some(ampersand_index) = head.iter().rposition(|b| *b == b'&') else {
            break;
        };
        // A terminated entity (`&amp;`) is a safe place to break after.
        if head[ampersand_index..].contains(&b';') {
            break;
        }
        split_at = ampersand_index as isize;
    }
    split_at
}

/// Python's `bytes.strip()`: ASCII whitespace only.
fn strip_ascii(bytes: &[u8]) -> &[u8] {
    let not_ws = |b: u8| !matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c);
    let start = bytes.iter().position(|b| not_ws(*b)).unwrap_or(bytes.len());
    let end = bytes.iter().rposition(|b| not_ws(*b)).map_or(start, |i| i + 1);
    &bytes[start..end]
}

/// Split text so no piece exceeds `byte_length` bytes, preferring newlines
/// then spaces, never breaking UTF-8 or an XML entity. Byte-for-byte the
/// original's generator, quirks included: a stretch with no whitespace in
/// range is yielded whole rather than split mid-word, because that is what
/// the service was observed to accept.
pub(crate) fn split_text_by_byte_length(text: &str, byte_length: usize) -> Vec<String> {
    let mut text = text.as_bytes().to_vec();
    let mut out = Vec::new();
    if byte_length == 0 {
        return out;
    }
    while text.len() > byte_length {
        let split_at = match find_last_newline_or_space_within_limit(&text, byte_length) {
            Some(index) => index as isize,
            None => find_safe_utf8_split_point(&text) as isize,
        };
        let split_at = adjust_split_point_for_xml_entity(&text, split_at);
        // A split point of zero would stall the loop; the original advances
        // by one byte instead.
        let advance = if split_at > 0 { split_at as usize } else { 1 };
        let chunk = strip_ascii(&text[..split_at.max(0) as usize]);
        if !chunk.is_empty() {
            out.push(String::from_utf8_lossy(chunk).into_owned());
        }
        text = text[advance..].to_vec();
    }
    let remaining = strip_ascii(&text);
    if !remaining.is_empty() {
        out.push(String::from_utf8_lossy(remaining).into_owned());
    }
    out
}

/// Build the SSML request frame.
fn mk_ssml(voice: &str, text: &str) -> String {
    format!(
        "X-RequestId:{request_id}\r\n\
         Content-Type:application/ssml+xml\r\n\
         X-Timestamp:{timestamp}Z\r\n\
         Path:ssml\r\n\r\n\
         <speak version='1.0' xmlns='http://www.w3.org/2001/10/synthesis' xml:lang='en-US'>\
         <voice name='{voice}'>\
         <prosody pitch='+0Hz' rate='+0%' volume='+0%'>\
         {text}\
         </prosody>\
         </voice>\
         </speak>",
        request_id = connect_id(),
        // The trailing Z is not a mistake; it is a Microsoft Edge bug the
        // service expects.
        timestamp = date_to_string(),
    )
}

/// Build the `speech.config` request frame.
fn mk_speech_config() -> String {
    format!(
        "X-Timestamp:{timestamp}\r\n\
         Content-Type:application/json; charset=utf-8\r\n\
         Path:speech.config\r\n\r\n\
         {{\"context\":{{\"synthesis\":{{\"audio\":{{\"metadataoptions\":{{\
         \"sentenceBoundaryEnabled\":\"true\",\"wordBoundaryEnabled\":\"false\"}},\
         \"outputFormat\":\"{OUTPUT_FORMAT}\"}}}}}}}}",
        timestamp = date_to_string()
    )
}

/// Parse a frame's CRLF-separated `Key: Value` header block.
fn frame_headers(bytes: &[u8]) -> Vec<(String, String)> {
    let mut headers = Vec::new();
    for line in bytes.split(|b| *b == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.is_empty() {
            continue;
        }
        let Some(colon) = line.iter().position(|b| *b == b':') else {
            continue;
        };
        let (key, value) = (&line[..colon], &line[colon + 1..]);
        headers.push((
            String::from_utf8_lossy(key).into_owned(),
            String::from_utf8_lossy(value).into_owned(),
        ));
    }
    headers
}

fn header_value<'a>(headers: &'a [(String, String)], key: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v.as_str())
}

/// Why one connection attempt produced no audio.
enum StreamError {
    /// The handshake was refused with a 403; carries the server `date`
    /// header the clock-skew correction is derived from.
    Refused(Option<String>),
    Failed(ApiError),
}

/// One connection, one text part: send the config and the SSML, read frames
/// until `turn.end`, and return every byte of MP3 that arrived.
async fn stream_part(part: &str, voice: &str) -> Result<Vec<u8>, StreamError> {
    let url = format!(
        "{WSS_URL}&ConnectionId={cid}&Sec-MS-GEC={gec}&Sec-MS-GEC-Version={SEC_MS_GEC_VERSION}",
        cid = connect_id(),
        gec = drm::generate_sec_ms_gec(),
    );

    let mut request = match url.into_client_request() {
        Ok(request) => request,
        Err(error) => return Err(StreamError::Failed(tts_err(error.to_string()))),
    };
    {
        let headers = request.headers_mut();
        for (name, value) in [
            ("Pragma", "no-cache"),
            ("Cache-Control", "no-cache"),
            ("Origin", ORIGIN),
            ("User-Agent", USER_AGENT),
            ("Accept-Encoding", "gzip, deflate, br, zstd"),
            ("Accept-Language", "en-US,en;q=0.9"),
            // A fresh machine id per connection, as the original does.
            ("Cookie", format!("muid={};", drm::generate_muid()).as_str()),
        ] {
            match HeaderValue::from_str(value) {
                Ok(parsed) => {
                    headers.insert(name, parsed);
                }
                Err(error) => return Err(StreamError::Failed(tts_err(error.to_string()))),
            }
        }
    }

    let connect = tokio_tungstenite::connect_async(request);
    let mut ws = match tokio::time::timeout(CONNECT_TIMEOUT, connect).await {
        Err(_) => {
            return Err(StreamError::Failed(tts_err(
                "Timed out connecting to the speech service.",
            )))
        }
        Ok(Err(WsError::Http(response))) => {
            let status = response.status();
            let date = response
                .headers()
                .get("date")
                .and_then(|value| value.to_str().ok())
                .map(str::to_string);
            if status == StatusCode::FORBIDDEN {
                return Err(StreamError::Refused(date));
            }
            return Err(StreamError::Failed(tts_err(format!(
                "The speech service answered with HTTP {status}."
            ))));
        }
        Ok(Err(error)) => {
            return Err(StreamError::Failed(tts_err(format!(
                "Could not reach the speech service: {error}"
            ))))
        }
        Ok(Ok((ws, _))) => ws,
    };

    if let Err(error) = ws.send(Message::text(mk_speech_config())).await {
        return Err(StreamError::Failed(tts_err(error.to_string())));
    }
    if let Err(error) = ws.send(Message::text(mk_ssml(voice, part))).await {
        return Err(StreamError::Failed(tts_err(error.to_string())));
    }

    let mut audio: Vec<u8> = Vec::new();
    let mut audio_was_received = false;
    loop {
        let received = match tokio::time::timeout(RECEIVE_TIMEOUT, ws.next()).await {
            Err(_) => {
                return Err(StreamError::Failed(tts_err(
                    "Timed out waiting for the speech service.",
                )))
            }
            Ok(received) => received,
        };
        match received {
            // The connection ended without `turn.end`.
            None => break,
            Some(Err(error)) => {
                return Err(StreamError::Failed(tts_err(format!(
                    "The websocket connection failed: {error}"
                ))))
            }
            Some(Ok(message)) => match message {
                Message::Text(frame) => {
                    let bytes = frame.as_bytes();
                    let (headers, _) = match bytes
                        .windows(4)
                        .position(|window| window == b"\r\n\r\n")
                    {
                        Some(separator) => (
                            frame_headers(&bytes[..separator]),
                            &bytes[separator + 4..],
                        ),
                        None => (frame_headers(bytes), &bytes[bytes.len()..]),
                    };
                    match header_value(&headers, "Path") {
                        Some("turn.end") => break,
                        // The metadata frames carry timing we do not need:
                        // audio only, no subtitles.
                        Some("audio.metadata") | Some("response") | Some("turn.start") => {}
                        Some(other) => {
                            return Err(StreamError::Failed(tts_err(format!(
                                "Unknown path received: {other}"
                            ))))
                        }
                        None => {
                            return Err(StreamError::Failed(tts_err(
                                "Received a message with no path.",
                            )))
                        }
                    }
                }
                Message::Binary(frame) => {
                    if frame.len() < 2 {
                        return Err(StreamError::Failed(tts_err(
                            "We received a binary message, but it is missing the header length.",
                        )));
                    }
                    let header_length =
                        u16::from_be_bytes([frame[0], frame[1]]) as usize;
                    if header_length > frame.len() {
                        return Err(StreamError::Failed(tts_err(
                            "The header length is greater than the length of the data.",
                        )));
                    }
                    let headers = frame_headers(&frame[..header_length]);
                    // The body starts two bytes past the header block.
                    let data = &frame[header_length + 2..];
                    if header_value(&headers, "Path") != Some("audio") {
                        return Err(StreamError::Failed(tts_err(
                            "Received binary message, but the path is not audio.",
                        )));
                    }
                    match header_value(&headers, "Content-Type") {
                        // At the end of the stream the service sends one
                        // headerless, empty binary frame. Empty is fine;
                        // headerless with data is not.
                        None => {
                            if data.is_empty() {
                                continue;
                            }
                            return Err(StreamError::Failed(tts_err(
                                "Received binary message with no Content-Type, but with data.",
                            )));
                        }
                        Some("audio/mpeg") => {}
                        Some(other) => {
                            return Err(StreamError::Failed(tts_err(format!(
                                "Received binary message, but with an unexpected Content-Type: {other}"
                            ))))
                        }
                    }
                    if data.is_empty() {
                        return Err(StreamError::Failed(tts_err(
                            "Received binary message, but it is missing the audio data.",
                        )));
                    }
                    audio_was_received = true;
                    audio.extend_from_slice(data);
                }
                Message::Close(_) => {
                    return Err(StreamError::Failed(tts_err(
                        "The speech service closed the connection before finishing the sentence.",
                    )))
                }
                _ => {}
            },
        }
    }

    if !audio_was_received {
        return Err(StreamError::Failed(tts_err(
            "No audio was received. Please verify that your parameters are correct.",
        )));
    }
    Ok(audio)
}

/// Synthesise one text piece into MP3 bytes.
///
/// The escaped text is split at 4096 bytes; each piece rides its own
/// connection, exactly as the original's `stream()` does. A 403 means the
/// clock skew correction was stale: the server's `date` header recalibrates
/// it and the piece is retried once.
pub(crate) async fn synthesize(text: &str, voice: &str) -> ApiResult<Vec<u8>> {
    let voice = long_voice_name(voice);
    let escaped = escape_xml(&remove_incompatible_characters(text));
    let parts = split_text_by_byte_length(&escaped, TEXT_BYTE_LIMIT);

    let mut audio: Vec<u8> = Vec::new();
    for part in &parts {
        let bytes = match stream_part(part, &voice).await {
            Ok(bytes) => bytes,
            Err(StreamError::Refused(date)) => {
                if let Some(date) = date.as_deref() {
                    drm::adjust_from_server_date(date);
                }
                match stream_part(part, &voice).await {
                    Ok(bytes) => bytes,
                    Err(StreamError::Refused(_)) => {
                        return Err(tts_err(
                            "The speech service refused the request. This version of the \
                             handshake may no longer be accepted; please update VoxLeaf.",
                        ))
                    }
                    Err(StreamError::Failed(error)) => return Err(error),
                }
            }
            Err(StreamError::Failed(error)) => return Err(error),
        };
        audio.extend_from_slice(&bytes);
    }
    Ok(audio)
}

/// Synthesise with the web app's retry posture.
///
/// A generation is many chunks over a service that closes connections
/// without ceremony. The failure being retried is a dropped connection
/// rather than a rejected request, so the wait doubles each attempt:
/// one second, then two.
pub(crate) async fn synthesize_with_retry(
    text: &str,
    voice: &str,
    attempts: u32,
) -> ApiResult<Vec<u8>> {
    let mut last: Option<ApiError> = None;
    for attempt in 1..=attempts.max(1) {
        match synthesize(text, voice).await {
            Ok(audio) => return Ok(audio),
            Err(error) => {
                last = Some(error);
                if attempt < attempts {
                    tokio::time::sleep(Duration::from_millis(1000 * 2u64.pow(attempt - 1))).await;
                }
            }
        }
    }
    Err(last.unwrap_or_else(|| tts_err("The speech service did not answer.")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_voice_names_follow_the_edge_shape() {
        assert_eq!(
            long_voice_name("en-US-GuyNeural"),
            "Microsoft Server Speech Text to Speech Voice (en-US, GuyNeural)"
        );
        assert_eq!(
            long_voice_name("fil-PH-AngeloNeural"),
            "Microsoft Server Speech Text to Speech Voice (fil-PH, AngeloNeural)"
        );
        assert_eq!(
            long_voice_name("en-US-EmmaMultilingualNeural"),
            "Microsoft Server Speech Text to Speech Voice (en-US, EmmaMultilingualNeural)"
        );
    }

    #[test]
    fn a_hyphenated_name_folds_into_the_region() {
        assert_eq!(
            long_voice_name("en-US-Guy-Neural"),
            "Microsoft Server Speech Text to Speech Voice (en-US-Guy, Neural)"
        );
    }

    #[test]
    fn non_matching_names_pass_through() {
        assert_eq!(long_voice_name("already long"), "already long");
        assert_eq!(long_voice_name("EN-us-GuyNeural"), "EN-us-GuyNeural");
        assert_eq!(long_voice_name("en-US-Guy"), "en-US-Guy");
        assert_eq!(long_voice_name(""), "");
    }

    #[test]
    fn incompatible_control_characters_become_spaces() {
        assert_eq!(
            remove_incompatible_characters("a\u{0b}b\u{00}c\u{1f}d"),
            "a b c d"
        );
        // Tab, newline and the rest of the printable set survive.
        assert_eq!(remove_incompatible_characters("a\tb\nc"), "a\tb\nc");
    }

    #[test]
    fn xml_specials_are_escaped() {
        assert_eq!(escape_xml("a & b < c > d"), "a &amp; b &lt; c &gt; d");
    }

    #[test]
    fn split_prefers_newlines_then_spaces() {
        let parts = split_text_by_byte_length("one two three four five", 12);
        assert_eq!(parts, vec!["one two", "three four", "five"]);
        let parts = split_text_by_byte_length("line one\nline two\nline three", 15);
        assert_eq!(parts, vec!["line one", "line two", "line three"]);
    }

    #[test]
    fn split_never_cuts_an_xml_entity() {
        // The limit lands inside `&amp;`; the break moves back to the `&`.
        let parts = split_text_by_byte_length("aa &amp; bbbbbbbbbbbbbbbb", 5);
        assert_eq!(parts[0], "aa");
        assert!(parts.iter().all(|part| !part.contains("&am\np;")));
        let joined = parts.join(" ");
        assert!(joined.contains("&amp;"), "entity lost: {joined}");
    }

    #[test]
    fn split_respects_utf8_boundaries() {
        // Six 3-byte characters and no whitespace anywhere: the original
        // falls back to `_find_safe_utf8_split_point`, which walks back from
        // the *end* of the text and so finds the whole string valid — one
        // piece, never a mid-character cut.
        let parts = split_text_by_byte_length("日本語日本語日本語", 10);
        assert_eq!(parts, vec!["日本語日本語日本語"]);

        // Whitespace present: the break lands on the space, at a boundary.
        let parts = split_text_by_byte_length("abc 日本語 def", 10);
        assert_eq!(parts, vec!["abc", "日本語", "def"]);
    }

    #[test]
    fn split_strips_and_drops_empty_pieces() {
        let parts = split_text_by_byte_length("  hello   world  ", 8);
        assert_eq!(parts, vec!["hello", "world"]);
        assert!(split_text_by_byte_length("   ", 4).is_empty());
    }

    #[test]
    fn frame_headers_parse_the_service_shape() {
        let headers = frame_headers(b"X-RequestId:abc\r\nPath:turn.end\r\n");
        assert_eq!(header_value(&headers, "Path"), Some("turn.end"));
        assert_eq!(header_value(&headers, "x-requestid"), Some("abc"));
        assert_eq!(header_value(&headers, "Missing"), None);
    }

    #[test]
    fn the_date_string_has_the_javascript_shape() {
        let date = date_to_string();
        // e.g. "Sat Oct 17 2026 12:34:56 GMT+0000 (Coordinated Universal Time)"
        assert!(date.contains("GMT+0000 (Coordinated Universal Time)"), "{date}");
        assert_eq!(date.len(), "Wed Sep 30 2026 00:00:00 GMT+0000 (Coordinated Universal Time)".len());
    }

    /// Live synthesis against the real service. Needs network; ignored in the
    /// default run the same way the web app's smoke test never shipped in CI.
    #[tokio::test]
    #[ignore = "live synthesis against Microsoft's service"]
    async fn live_synthesis_returns_mp3_bytes() {
        let audio = synthesize_with_retry("Hello from VoxLeaf.", "en-US-GuyNeural", 3)
            .await
            .expect("synthesis should succeed");
        assert!(audio.len() > 1000, "only {} bytes of audio", audio.len());
        let is_mp3 = audio.starts_with(b"ID3")
            || (audio.len() >= 2 && audio[0] == 0xFF && audio[1] & 0xE0 == 0xE0);
        assert!(is_mp3, "audio does not look like MP3: {:02x?}", &audio[..8]);
    }
}
