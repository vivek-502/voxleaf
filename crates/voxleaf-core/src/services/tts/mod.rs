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
                return Err(ApiError::invalid("language", "That is not a language code."));
            }
            Ok(Some(trimmed.to_string()))
        }
        Some(_) => Err(ApiError::invalid("language", "That is not a language code.")),
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
                validate_language(&json!({ "language": good })).unwrap().as_deref(),
                Some(good),
                "{good} should be accepted"
            );
        }
        // `.trim()` runs before the pattern, so padding is normalised away
        // rather than refused.
        assert_eq!(
            validate_language(&json!({ "language": "  hi  " })).unwrap().as_deref(),
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
        assert_eq!(validate_language(&json!({ "language": null })).unwrap(), None);
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
