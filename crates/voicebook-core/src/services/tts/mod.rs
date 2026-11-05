//! On-demand narration using Microsoft Edge's read-aloud service.
//!
//! The web app shelled out to the Python `edge-tts` client. A desktop app that
//! has to install cleanly on Linux, Windows and macOS should not ask for a
//! Python interpreter, so the protocol is spoken directly here: a
//! `Sec-MS-GEC` handshake, a WebSocket, and MP3 chunks back.
//!
//! Output is 24 kHz / 48 kbps mono MP3, which is what the service returns and
//! what the player expects.
//!
//! Generation is a job, not a request: a chapter is many chunks, each a round
//! trip, and the whole pass takes minutes. `synthesize_chapter` claims the
//! chapter, returns the job's status immediately, and lets the work run in
//! the background — the client polls the same chapter until the job settles.

mod chunk;
mod client;
mod drm;
mod generate;
mod jobs;
mod narration;
mod text;
mod voices;

use axum::http::StatusCode;
use serde_json::Value;

use crate::api::AppState;
use crate::config::Config;
use crate::db::Database;
use crate::error::{ApiError, ApiResult};
use crate::models::TtsVoice;

/// Default voice, matching the web app's `ttsVoice` default.
pub const DEFAULT_VOICE: &str = "en-US-GuyNeural";

/// Sleep-timer presets, matching `SLEEP_TIMER_IDS` in `src/types`.
pub const SLEEP_TIMER_IDS: &[&str] = &["off", "5", "10", "15", "30", "end-of-chapter"];

pub struct TtsService {
    #[allow(dead_code)]
    cfg: Config,
    /// In-process generation jobs, so a request can start one and return.
    jobs: jobs::JobRegistry,
}

impl TtsService {
    pub fn new(cfg: &Config) -> Self {
        Self {
            cfg: cfg.clone(),
            jobs: jobs::new_registry(),
        }
    }

    /// Every voice the service offers, live when it answers and from the
    /// captured catalogue when it does not.
    pub async fn voices(&self) -> ApiResult<Vec<TtsVoice>> {
        voices::list_voices().await
    }

    /// `POST /api/tts/speak` — read a selection or a dictionary word aloud.
    ///
    /// The text is capped, nothing is stored, and a requested voice is checked
    /// against Edge's own list rather than trusted.
    pub async fn speak(&self, db: &Database, body: &Value) -> ApiResult<Vec<u8>> {
        const MAX_SPEECH_CHARS: usize = 1500;

        let text = body
            .get("text")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or("");
        if text.is_empty() || text.chars().count() > MAX_SPEECH_CHARS {
            return Err(ApiError::bad_request("Choose some text to read aloud."));
        }
        let text = text.to_string();

        let language = match body.get("language") {
            None | Some(Value::Null) => None,
            Some(value) => {
                let raw = value.as_str().map(str::trim).unwrap_or_default();
                if raw.is_empty() {
                    return Err(ApiError::bad_request("Choose some text to read aloud."));
                }
                let canonical = crate::services::translation::canonical_language_code(raw);
                if canonical.encode_utf16().count() > 35
                    || !crate::services::translation::is_valid_language_code(&canonical)
                {
                    return Err(ApiError::invalid(
                        "language",
                        "That is not a language code.",
                    ));
                }
                Some(canonical)
            }
        };

        let requested_voice = match body.get("voice") {
            None | Some(Value::Null) => None,
            Some(value) => {
                let raw = value.as_str().map(str::trim).unwrap_or_default();
                if raw.is_empty() || raw.chars().count() > 120 {
                    return Err(ApiError::bad_request("Choose some text to read aloud."));
                }
                Some(raw.to_string())
            }
        };

        let configured = narration::configured_voice(db).await;
        let mut voice = configured.clone();

        if let Some(requested) = requested_voice {
            let available = self.voices().await?;
            let chosen = available
                .into_iter()
                .find(|candidate| candidate.name == requested)
                .ok_or_else(|| ApiError::not_found("That voice is not available."))?;
            voice = chosen.name;
        } else if let Some(language) = language.as_deref() {
            if !narration::voice_can_read(&configured, Some(language)) {
                // An English voice cannot pronounce Hindi; pick the language's
                // usual reader rather than producing noise.
                let available = self.voices().await?;
                let target = narration::base_language(language);
                voice = narration::preferred_voice(language)
                    .and_then(|preferred| {
                        available
                            .iter()
                            .find(|candidate| candidate.name == preferred)
                            .map(|candidate| candidate.name.clone())
                    })
                    .or_else(|| {
                        available
                            .iter()
                            .find(|candidate| narration::base_language(&candidate.locale) == target)
                            .map(|candidate| candidate.name.clone())
                    })
                    .unwrap_or(configured);
            }
        }

        let audio = client::synthesize_with_retry(&text, &voice, 2).await?;
        if audio.is_empty() {
            return Err(ApiError::new(
                StatusCode::BAD_GATEWAY,
                "speech_failed",
                "The voice service returned no audio.",
            ));
        }
        Ok(audio)
    }

    /// `POST /api/tts/preview` — a short recording of one voice.
    ///
    /// A sentence in the voice's own language, so the preview demonstrates the
    /// voice rather than an accent reading English.
    pub async fn preview(&self, body: &Value) -> ApiResult<Vec<u8>> {
        let requested = body
            .get("voice")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or("");
        if requested.is_empty() {
            return Err(ApiError::bad_request("Choose a voice to preview."));
        }

        let available = self.voices().await?;
        let chosen = available
            .into_iter()
            .find(|candidate| candidate.name == requested)
            .ok_or_else(|| ApiError::not_found("That voice is not available."))?;

        let sample = preview_sample(&narration::base_language(&chosen.locale));
        let audio = client::synthesize_with_retry(sample, &chosen.name, 2).await?;
        if audio.is_empty() {
            return Err(ApiError::new(
                StatusCode::BAD_GATEWAY,
                "preview_failed",
                "The voice service returned no audio.",
            ));
        }
        Ok(audio)
    }

    /// Start (or report) a narration job for a chapter.
    ///
    /// Returns as soon as the job is claimed — the whole point of the
    /// endpoint. Pass `language` to narrate a saved translation instead of
    /// the original; omit it to narrate the original. Regenerating is allowed
    /// and replaces the existing audio, because a chapter read again in a
    /// different voice should not need the old file deleted first.
    pub async fn synthesize_chapter(
        &self,
        st: &AppState,
        chapter_id: &str,
        body: &Value,
    ) -> ApiResult<Value> {
        let language = validate_language(body)?;

        // Resolved here as well as in the job so the reply can state up
        // front which language will be read. Doing it twice is cheap: both
        // read the same rows, and a disagreement is impossible because both
        // call the same function.
        let chapter = narration::load_chapter(&st.db, chapter_id).await?;
        let configured = narration::configured_voice(&st.db).await;
        let source =
            narration::resolve_narration_source(&st.db, &chapter, language.as_deref(), &configured)
                .await?;

        // Measured up front so the client can show real progress rather than
        // an indeterminate spinner, and so a chapter with no text fails
        // immediately instead of after a minute of synthesis.
        let total = chunk::chunk(&source.text).len() as i64;
        if total == 0 {
            return Err(ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "request_failed",
                "There is no text in this chapter to narrate.",
            ));
        }

        // Already running: report the running job rather than starting a
        // second one.
        let claimed = jobs::start(&self.jobs, chapter_id, total);
        if claimed.is_some() {
            // Deliberately not awaited — this is the whole point of the
            // endpoint.
            generate::spawn(
                st.clone(),
                self.jobs.clone(),
                chapter_id.to_string(),
                language,
            );
        }

        let job = jobs::get(&self.jobs, chapter_id)
            .expect("a claimed job is immediately visible in the registry");
        Ok(generate::status_json(st, &job))
    }

    /// The progress of a generation, for the client to poll. `null` when
    /// nothing has been generated for this chapter recently.
    pub async fn job_status(&self, st: &AppState, chapter_id: &str) -> ApiResult<Value> {
        Ok(match jobs::get(&self.jobs, chapter_id) {
            Some(job) => generate::status_json(st, &job),
            None => Value::Null,
        })
    }

    /// Delete generated audio for a chapter.
    ///
    /// The route handler is synchronous, and a database call is not: the
    /// deletion is claimed here and carried out in the background, so the
    /// handler returns the moment the request is understood. A failure is
    /// logged rather than raised, the same visible outcome as before — the
    /// chapter has no audio either way.
    pub fn remove_chapter_audio(&self, db: &Database, chapter_id: &str) -> ApiResult<()> {
        let db = db.clone();
        let storage = self.cfg.storage.clone();
        let chapter_id = chapter_id.to_string();
        tokio::spawn(async move {
            if let Err(error) = generate::remove(&db, &storage, &chapter_id).await {
                tracing::warn!(%error, chapter_id, "could not remove chapter audio");
            }
        });
        Ok(())
    }
}

/// A sentence in the voice's own language, so the preview demonstrates the
/// voice rather than an accent reading English. Languages without a line here
/// get the English one, which is still the right voice, just not the right text.
fn preview_sample(language: &str) -> &'static str {
    match language {
        "es" => "Hola. Así suena esta voz.",
        "fr" => "Bonjour. Voici comment sonne cette voix.",
        "de" => "Hallo. So klingt diese Stimme.",
        "it" => "Ciao. Ecco come suona questa voce.",
        "pt" => "Olá. É assim que esta voz soa.",
        "nl" => "Hallo. Zo klinkt deze stem.",
        "pl" => "Dzień dobry. Tak brzmi ten głos.",
        "ru" => "Здравствуйте. Так звучит этот голос.",
        "uk" => "Вітаю. Так звучить цей голос.",
        "tr" => "Merhaba. Bu ses böyle kulağa geliyor.",
        "ar" => "مرحباً. هكذا يبدو هذا الصوت.",
        "hi" => "नमस्ते। यह आवाज़ ऐसी सुनाई देती है।",
        "bn" => "নমস্কার। এই কণ্ঠস্বর এমন শোনায়।",
        "ta" => "வணக்கம். இந்தக் குரல் இப்படி ஒலிக்கிறது.",
        "ja" => "こんにちは。この声はこんなふうに聞こえます。",
        "ko" => "안녕하세요. 이 목소리는 이렇게 들립니다.",
        "zh" => "你好。这个声音听起来是这样的。",
        "id" => "Halo. Beginilah suara ini terdengar.",
        "vi" => "Xin chào. Giọng này nghe như thế này.",
        "th" => "สวัสดี เสียงนี้ฟังดูเป็นแบบนี้",
        _ => "Hello. This is how this voice sounds.",
    }
}

/// The request body's `language`, if one was sent.
///
/// A locale, not free text: it ends up in a provider query and a filename,
/// so it is checked the way the web app's schema checked it.
fn validate_language(body: &Value) -> ApiResult<Option<String>> {
    match body.get("language") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(language)) => {
            let trimmed = language.trim();
            let length = trimmed.chars().count();
            if !(2..=35).contains(&length) || !is_language_code(trimmed) {
                return Err(ApiError::invalid(
                    "language",
                    "That is not a language code.",
                ));
            }
            Ok(Some(trimmed.to_string()))
        }
        Some(_) => Err(ApiError::invalid(
            "language",
            "That is not a language code.",
        )),
    }
}

/// `^[A-Za-z]{2,3}(-[A-Za-z0-9]{2,8})*$`, without a regular-expression
/// engine: two or three letters, then hyphen-separated groups of two to eight
/// letters or digits.
fn is_language_code(code: &str) -> bool {
    let bytes = code.as_bytes();
    let mut at = 0;
    let mut letters = 0;
    while at < bytes.len() && bytes[at].is_ascii_alphabetic() {
        letters += 1;
        at += 1;
    }
    if !(2..=3).contains(&letters) {
        return false;
    }
    while at < bytes.len() {
        if bytes[at] != b'-' {
            return false;
        }
        at += 1;
        let start = at;
        while at < bytes.len() && bytes[at].is_ascii_alphanumeric() {
            at += 1;
        }
        if !((at - start) >= 2 && (at - start) <= 8) {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn language_codes_are_locales_not_free_text() {
        for good in ["hi", "pt-BR", "zh-Hant", "sr-Latn-RS", "es-419"] {
            assert_eq!(
                validate_language(&json!({ "language": good }))
                    .unwrap()
                    .as_deref(),
                Some(good),
                "{good} should be accepted"
            );
        }
        // `.trim()` runs before the pattern, so padding is normalised away
        // rather than refused.
        assert_eq!(
            validate_language(&json!({ "language": "  hi  " }))
                .unwrap()
                .as_deref(),
            Some("hi")
        );
        // Every hyphen group is bounded but the tag is not capped in count,
        // so this long locale is still a locale.
        assert_eq!(
            validate_language(&json!({ "language": "en-US-extra-extra-extra-extra" }))
                .unwrap()
                .as_deref(),
            Some("en-US-extra-extra-extra-extra")
        );
        for bad in ["", "e", "toolong", "1-N", "pt_", "nope!", "hi!!"] {
            assert!(
                validate_language(&json!({ "language": bad })).is_err(),
                "{bad} should be refused"
            );
        }
        // Absent and null both mean "the original".
        assert_eq!(validate_language(&json!({})).unwrap(), None);
        assert_eq!(
            validate_language(&json!({ "language": null })).unwrap(),
            None
        );
        // A number is not a language.
        assert!(validate_language(&json!({ "language": 42 })).is_err());
        // The message is the web app's, so a client sees the same words.
        let error = validate_language(&json!({ "language": "nope!" })).unwrap_err();
        assert_eq!(
            error.details,
            Some(json!([{ "path": "language", "message": "That is not a language code." }]))
        );
    }
}
