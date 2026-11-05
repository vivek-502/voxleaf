//! Turning a chapter into an audio file, without anyone uploading anything.
//!
//! The text comes from the same sanitised chapter HTML the reader renders,
//! read back through the block scanner so an image contributes no words and a
//! table cell contributes only its text. The chunks are synthesised one at a
//! time and the MP3s are joined by simple concatenation, which is valid for
//! the MPEG audio stream Edge produces: each file is a run of self-contained
//! frames, so the result is one longer run of frames.
//!
//! The finished bytes are stored under a fresh key and the `chapter_audio`
//! row is written directly, matching what `uploadChapterAudio` produced in
//! the web app: same row, same replacement of any previous file, same
//! metadata. A generated chapter is therefore indistinguishable from an
//! imported one to everything downstream — the player, the progress service,
//! the stream route.
//!
//! Progress recompute is deliberately absent: the web app called its progress
//! service here, and that service is another module's to own. A chapter's
//! narration length is derived from its byte count below, so the row is
//! correct whenever the recompute runs.

use rusqlite::OptionalExtension;
use serde_json::json;

use crate::api::AppState;
use crate::error::{ApiError, ApiResult};
use crate::repo;
use crate::services::tts::chunk::chunk;
use crate::services::tts::client;
use crate::services::tts::jobs::{self, JobRegistry};
use crate::services::tts::narration;

/// 48 kbps constant bitrate, the only format the service returns. The
/// duration below is derived from it, so it must match `client::OUTPUT_FORMAT`.
const BIT_RATE_BPS: i64 = 48_000;
const SAMPLE_RATE: i64 = 24_000;
const CHANNELS: i64 = 1;

/// The service's own ceiling on how much text it will read in one go, and the
/// message it earns. Kept identical to the web app's so a too-long chapter
/// fails the same way either way.
const MAX_TOTAL_CHARS: usize = 400_000;

fn no_text() -> ApiError {
    ApiError::new(
        axum::http::StatusCode::UNPROCESSABLE_ENTITY,
        "request_failed",
        "There is no text in this chapter to narrate.",
    )
}

/// Seconds of audio a buffer of this many bytes contains, at the constant
/// bitrate the service speaks in.
fn duration_from_bytes(bytes: usize) -> f64 {
    (bytes as f64) * 8.0 / BIT_RATE_BPS as f64
}

/// A title is user text, so only a safe subset of it becomes a filename.
///
/// The web app normalised to NFKD first; that needs a Unicode table this app
/// does not carry, so the Latin result is matched exactly and other scripts
/// reduce to the same `chapter` fallback the web app gave them — which is
/// what both did for a title with no ASCII in it at all.
fn sanitize_filename(title: &str) -> String {
    let filtered: String = title
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c.is_whitespace() || c == '-' {
                c
            } else {
                ' '
            }
        })
        .collect();
    let collapsed = filtered.split_whitespace().collect::<Vec<&str>>().join("-");
    collapsed.chars().take(60).collect()
}

/// Name the file after the language it is in.
///
/// Without this, Hindi and English versions of a chapter are both called
/// `Chapter-1.mp3` and the only way to tell them apart is to listen to both.
fn build_filename(title: &str, language: Option<&str>) -> String {
    let base = {
        let base = sanitize_filename(title);
        if base.is_empty() {
            "chapter".to_string()
        } else {
            base
        }
    };
    match language {
        Some(language) => format!("{base} [{language}].mp3"),
        None => format!("{base}.mp3"),
    }
}

/// Synthesize a chapter and store it as its audio.
///
/// Runs detached from the request that started it; progress is reported
/// through the job registry. Never returns an error to its caller — a failure
/// is recorded on the job so the polling client can show why.
pub(crate) async fn run(
    st: AppState,
    registry: JobRegistry,
    chapter_id: String,
    language: Option<String>,
) {
    match synthesize(&st, &registry, &chapter_id, language.as_deref()).await {
        Ok(audio_id) => jobs::finish(&registry, &chapter_id, audio_id),
        Err(error) => jobs::fail(&registry, &chapter_id, error.to_string()),
    }
}

async fn synthesize(
    st: &AppState,
    registry: &JobRegistry,
    chapter_id: &str,
    language: Option<&str>,
) -> ApiResult<String> {
    let chapter = narration::load_chapter(&st.db, chapter_id).await?;

    let voice = narration::configured_voice(&st.db).await;
    let source =
        narration::resolve_narration_source(&st.db, &chapter, language, &voice).await?;

    let text = source.text.clone();
    if text.trim().is_empty() {
        return Err(no_text());
    }
    jobs::set_narration(registry, chapter_id, source.clone());

    if text.chars().count() > MAX_TOTAL_CHARS {
        return Err(ApiError::internal(format!(
            "This chapter is {} characters long, which is too long to narrate in one go. \
             Split it into smaller chapters first.",
            text.chars().count()
        )));
    }

    let chunks = chunk(&text);
    if chunks.is_empty() {
        return Err(no_text());
    }

    // Synthesise each chunk. The parts are held in memory: a chapter is a few
    // megabytes of MP3, and the temp-file dance the web app needed to hand
    // bytes between processes has no one to hand them to here.
    let mut parts: Vec<Vec<u8>> = Vec::with_capacity(chunks.len());
    for (index, part) in chunks.iter().enumerate() {
        let audio = client::synthesize_with_retry(part, &source.voice, 3).await?;
        parts.push(audio);
        jobs::advance(registry, chapter_id, index as i64 + 1);
    }

    let audio: Vec<u8> = parts.concat();
    if audio.is_empty() {
        return Err(ApiError::internal("The voice service returned no audio."));
    }

    let key = crate::storage::LocalStorage::key_for(".mp3", &st.cfg.upload.allowed_extensions)?;
    st.cfg.storage.put(&key, &audio)?;

    let audio_id = repo::new_id("au_");
    let duration = duration_from_bytes(audio.len());
    let file_size = audio.len() as i64;
    let uploaded_at = repo::now_iso();
    let original_name = build_filename(&chapter.title, source.language.as_deref());

    let inserted: ApiResult<()> = st
        .db
        .call({
            let (audio_id, chapter_id, key, original_name, uploaded_at) = (
                audio_id.clone(),
                chapter_id.to_string(),
                key.clone(),
                original_name.clone(),
                uploaded_at.clone(),
            );
            move |conn| {
                let tx = conn.unchecked_transaction()?;
                // Regenerating replaces the existing audio, exactly as an
                // upload of a new file for the same chapter did.
                tx.execute(
                    "DELETE FROM chapter_audio WHERE chapter_id = ?1",
                    [&chapter_id],
                )?;
                tx.execute(
                    "INSERT INTO chapter_audio \
                     (id, chapter_id, file_url, storage_key, duration, file_size, mime_type, \
                      original_name, bit_rate, sample_rate, channels, uploaded_at) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                    rusqlite::params![
                        audio_id,
                        chapter_id,
                        "/api/audio/stream",
                        key,
                        duration,
                        file_size,
                        "audio/mpeg",
                        original_name,
                        BIT_RATE_BPS,
                        SAMPLE_RATE,
                        CHANNELS,
                        uploaded_at,
                    ],
                )?;
                tx.commit()?;
                Ok(())
            }
        })
        .await;

    if let Err(error) = inserted {
        // The row never landed, so the bytes must not either.
        let _ = st.cfg.storage.delete(&key);
        return Err(error);
    }

    Ok(audio_id)
}

/// Delete generated audio for a chapter: the row and the bytes behind it.
pub(crate) async fn remove(
    db: &crate::db::Database,
    storage: &crate::storage::LocalStorage,
    chapter_id: &str,
) -> ApiResult<()> {
    let had_audio: Option<String> = db
        .call({
            let chapter_id = chapter_id.to_string();
            move |conn| {
                conn.query_row(
                    "SELECT storage_key FROM chapter_audio WHERE chapter_id = ?1",
                    [chapter_id],
                    |row| row.get(0),
                )
                .optional()
            }
        })
        .await?;

    // Nothing to do is a success, not an error: a chapter that never had
    // audio can be asked to lose it as often as the reader likes.
    let Some(key) = had_audio else {
        return Ok(());
    };

    // The bytes go first, then the row. A failure between the two leaves a
    // row pointing at a file that is gone, which the stream route reports as
    // unavailable — the reader sees "no audio", the same honest answer as a
    // chapter that never had any. The reverse order would leave an orphan
    // file nothing could ever find again.
    storage.delete(&key)?;

    db.call({
        let chapter_id = chapter_id.to_string();
        move |conn| {
            conn.execute("DELETE FROM chapter_audio WHERE chapter_id = ?1", [chapter_id])?;
            Ok::<(), ApiError>(())
        }
    })
    .await?;

    Ok(())
}

/// The JSON body a client needs to poll a job into a progress bar.
pub(crate) fn status_json(st: &AppState, job: &jobs::TtsJob) -> serde_json::Value {
    let stream_url = match (job.state, job.audio_id.as_deref()) {
        (jobs::JobState::Done, Some(audio_id)) => Some(crate::api::stream::signed_url(st, audio_id)),
        _ => None,
    };

    json!({
        "chapterId": job.chapter_id,
        "state": job.state.as_str(),
        "completed": job.completed,
        "total": job.total,
        "error": job.error,
        "audioId": job.audio_id,
        "streamUrl": stream_url,
        "narration": job.narration.as_ref().map(|source| source.to_json()),
    })
}

/// Spawn the generation and return immediately. This is the whole point of
/// the endpoint: synthesis takes minutes and the caller must not wait.
pub(crate) fn spawn(st: AppState, registry: JobRegistry, chapter_id: String, language: Option<String>) {
    tokio::spawn(async move {
        crate::services::tts::generate::run(st, registry, chapter_id, language).await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filenames_are_named_for_their_language() {
        assert_eq!(build_filename("Chapter 1", None), "Chapter-1.mp3");
        assert_eq!(build_filename("Chapter 1", Some("hi-IN")), "Chapter-1 [hi-IN].mp3");
        assert_eq!(build_filename("A Tale", Some("pt-BR")), "A-Tale [pt-BR].mp3");
    }

    #[test]
    fn a_hostile_title_cannot_escape_the_filename() {
        assert_eq!(build_filename("../../etc/passwd", None), "etc-passwd.mp3");
        assert_eq!(build_filename("   ", None), "chapter.mp3");
        assert_eq!(build_filename("", None), "chapter.mp3");
        assert!(!build_filename("a/b\\c", None).contains('/'));
    }

    #[test]
    fn long_titles_are_truncated() {
        let title = "a".repeat(200);
        let name = build_filename(&title, None);
        assert!(name.len() <= 64, "{name}");
    }

    #[test]
    fn duration_comes_from_the_constant_bitrate() {
        // One second of 48 kbps is 6000 bytes.
        assert!((duration_from_bytes(6000) - 1.0).abs() < 1e-9);
        assert!((duration_from_bytes(12_000) - 2.0).abs() < 1e-9);
        assert_eq!(duration_from_bytes(0), 0.0);
    }
}
