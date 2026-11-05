//! The serialisable shapes the API returns.
//!
//! These mirror `src/types/index.ts` from the web app field for field. Keeping
//! the names in one place means the ported React components can keep importing
//! from `@/types` and read exactly the JSON they always did.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Read a JSON array stored as a text column, tolerating null and corruption.
pub fn parse_string_array(raw: Option<&str>) -> Vec<String> {
    raw.and_then(|value| serde_json::from_str::<Vec<String>>(value).ok())
        .unwrap_or_default()
}

/// Clamp a number into `[min, max]`, returning `fallback` when unusable.
pub fn safe_number(value: Option<f64>, min: f64, max: f64, fallback: f64) -> f64 {
    match value {
        Some(number) if number.is_finite() => number.clamp(min, max),
        _ => fallback,
    }
}

/// A catalog book as returned by the book metadata provider.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BookSummary {
    pub id: String,
    pub external_id: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub author: String,
    pub description: Option<String>,
    pub cover_url: Option<String>,
    pub publisher: Option<String>,
    pub publication_date: Option<String>,
    pub isbn: Option<String>,
    pub categories: Vec<String>,
    pub page_count: Option<i64>,
    pub language: Option<String>,
    pub rating: Option<f64>,
    pub rating_count: Option<i64>,
    /// Whether the book is already in the user's library.
    pub in_library: bool,
    pub favorite: bool,
    /// Internal catalogue id when known, so pages can link straight through.
    pub library_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookSearchResult {
    pub books: Vec<BookSummary>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
    pub has_more: bool,
    /// True when the provider was unreachable and local results were served.
    pub degraded: bool,
    pub provider: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterAudioInfo {
    pub id: String,
    pub chapter_id: String,
    /// Signed, expiring stream URL. Never a raw storage path.
    pub stream_url: String,
    pub duration: f64,
    pub file_size: i64,
    pub mime_type: String,
    pub original_name: Option<String>,
    pub uploaded_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterProgressInfo {
    pub position: f64,
    pub duration: f64,
    pub percentage: f64,
    pub completed: bool,
    pub play_count: i64,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterInfo {
    pub id: String,
    pub book_id: String,
    pub chapter_number: i64,
    pub title: String,
    pub sort_index: i64,
    pub audio: Option<ChapterAudioInfo>,
    pub progress: Option<ChapterProgressInfo>,
    pub bookmark_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// A book in the user's library, with its chapters and progress rollup.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryBookInfo {
    pub library_id: String,
    pub book_id: String,
    pub added_at: String,
    pub favorite: bool,
    pub overall_progress: f64,
    pub completed: bool,
    pub last_played_at: Option<String>,
    pub last_played_chapter_id: Option<String>,
    pub last_chapter_title: Option<String>,
    pub last_chapter_number: Option<i64>,
    pub last_position: f64,
    pub total_audio_duration: f64,
    pub chapter_count: i64,
    pub chapters_with_audio: i64,
    pub book: BookSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookmarkInfo {
    pub id: String,
    pub chapter_id: String,
    pub book_id: String,
    pub book_title: String,
    pub chapter_title: String,
    pub chapter_number: i64,
    pub timestamp: f64,
    pub note: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettingsInfo {
    pub playback_speed: f64,
    pub theme: String,
    pub volume: f64,
    pub muted: bool,
    pub sleep_timer_preference: String,
    pub skip_forward_seconds: i64,
    pub skip_backward_seconds: i64,
    pub completion_threshold: f64,
    pub auto_advance: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bookmark_note: Option<String>,
    pub tts_voice: String,
}

/// The single track the audio engine is currently bound to.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackTrack {
    pub book_id: String,
    pub library_id: String,
    pub chapter_id: String,
    pub audio_id: String,
    pub book_title: String,
    pub chapter_title: String,
    pub chapter_number: i64,
    pub cover_url: Option<String>,
    pub author: String,
    pub stream_url: String,
    /// Resumed position, in seconds.
    pub start_position: f64,
    pub duration: f64,
    pub favorite: bool,
}

/* -------------------------------------------------------------------------- */
/* EPUB                                                                       */
/* -------------------------------------------------------------------------- */

/// State of a book's stored EPUB.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum EpubStatusKind {
    /// Nothing uploaded yet, or a rejected upload.
    None,
    /// Stored, parsed and ready to read.
    Ready,
    /// A file was supplied but could not be used.
    Failed,
    /// EPUB support is switched off on this instance.
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EpubStatusInfo {
    pub has_epub: bool,
    pub status: Option<EpubStatusKind>,
    pub error: Option<String>,
    pub file_name: Option<String>,
    pub file_size: Option<i64>,
    pub chapter_count: i64,
    pub image_count: i64,
    pub title: Option<String>,
    pub creator: Option<String>,
    pub language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
}

/// A voice offered by Edge TTS.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TtsVoice {
    /// Short name passed to the voice service, e.g. `en-US-GuyNeural`.
    pub name: String,
    pub locale: String,
    pub gender: String,
}

/// Progress of one chapter's narration, polled by the client until it settles.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TtsJobStatus {
    pub chapter_id: String,
    pub state: String,
    /// Parts finished so far. A long chapter is synthesised in several.
    pub completed: i64,
    pub total: i64,
    pub error: Option<String>,
    pub audio_id: Option<String>,
    pub stream_url: Option<String>,
    /// What is being narrated. Null until the source is resolved.
    pub narration: Option<TtsNarrationInfo>,
}

/// Which language a generation settled on, and the voice reading it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TtsNarrationInfo {
    /// Provider language code, or null when narrating the original.
    pub language: Option<String>,
    pub language_name: Option<String>,
    pub voice: String,
    /// A translation was asked for but the original was narrated instead.
    pub fell_back_to_original: bool,
    /// Why the original was used, or why a different voice was chosen.
    pub reason: Option<String>,
}

/// A reading-order document from the EPUB spine.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EpubChapterEntry {
    pub id: String,
    pub index: i64,
    pub title: String,
    /// Nesting depth from the table of contents, 0 for a flat spine.
    pub depth: i64,
    pub in_toc: bool,
    pub word_count: i64,
    pub char_offset: i64,
}

/// One row of the book's own table of contents.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EpubContentsEntry {
    /// Reading-order chapter the row opens.
    pub index: i64,
    /// Anchor id within the chapter, empty when the row has no fragment.
    pub anchor: String,
    /// Visible-text character offset inside the chapter.
    pub offset: i64,
    pub title: String,
    pub depth: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EpubChapterBody {
    pub index: i64,
    pub title: String,
    /// Sanitised at import time, so it is safe to inject.
    pub html: String,
    pub word_count: i64,
    pub depth: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EpubMeta {
    pub title: Option<String>,
    pub creator: Option<String>,
    pub language: Option<String>,
    pub chapter_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EpubChapterPayload {
    pub chapter: EpubChapterBody,
    pub chapters: Vec<EpubChapterEntry>,
    pub contents: Vec<EpubContentsEntry>,
    pub meta: EpubMeta,
}

/// Audio state for one reading-order document.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EpubAudioEntry {
    /// Spine index, matching `EpubChapterEntry.index`.
    pub index: i64,
    /// Library chapter carrying the audio, or null when nothing is attached.
    pub chapter_id: Option<String>,
    pub has_audio: bool,
    pub audio_id: Option<String>,
    pub duration: f64,
    pub progress: f64,
    pub completed: bool,
}

/// One block of a translated section: the source block's tag, and its new text.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationBlock {
    pub tag: String,
    pub text: String,
    /// Original HTML attributes for non-text blocks (e.g. img src/alt).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub html: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colspan: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rowspan: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_header: Option<bool>,
}

/// A saved translation of one section.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EpubTranslationEntry {
    pub epub_index: i64,
    pub language: String,
    pub language_name: String,
    pub title: String,
    pub blocks: Vec<TranslationBlock>,
    pub char_count: i64,
    /// True when the section's text changed since the translation was made.
    pub stale: bool,
    pub updated_at: String,
}

/// A saved translation without its text — enough to offer it in a picker.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EpubTranslationSummary {
    pub epub_index: i64,
    pub language: String,
    pub language_name: String,
    pub title: String,
    pub char_count: i64,
    pub stale: bool,
    pub updated_at: String,
}

/// How a translation job is going, for a long section.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationProgress {
    pub done: i64,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EpubProgressInfo {
    pub book_id: String,
    pub chapter_index: i64,
    pub scroll_percent: f64,
    pub font_size: i64,
    pub line_height: f64,
    pub theme: String,
    pub font_family: String,
    pub column_width: i64,
}

/// The aggregated payload behind the home dashboard.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HomePayload {
    pub continue_listening: Vec<LibraryBookInfo>,
    pub recently_added: Vec<LibraryBookInfo>,
    pub library: Vec<LibraryBookInfo>,
    pub favorites: Vec<LibraryBookInfo>,
    pub recommendations: Vec<BookSummary>,
    pub stats: Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_clamped_and_rejected_when_not_finite() {
        assert_eq!(safe_number(Some(0.5), 0.0, 1.0, 0.0), 0.5);
        assert_eq!(safe_number(Some(3.0), 0.0, 1.0, 0.0), 1.0);
        assert_eq!(safe_number(Some(-1.0), 0.0, 1.0, 0.0), 0.0);
        assert_eq!(safe_number(Some(f64::NAN), 0.0, 1.0, 0.7), 0.7);
        assert_eq!(safe_number(None, 0.0, 1.0, 0.7), 0.7);
    }

    #[test]
    fn string_arrays_tolerate_garbage() {
        assert_eq!(parse_string_array(Some(r#"["a","b"]"#)), vec!["a", "b"]);
        assert!(parse_string_array(Some("not json")).is_empty());
        assert!(parse_string_array(None).is_empty());
    }

    #[test]
    fn dto_field_names_match_the_web_app() {
        let summary = BookSummary {
            id: "bk_1".into(),
            external_id: "OL123W".into(),
            title: "Title".into(),
            subtitle: None,
            author: "Author".into(),
            description: None,
            cover_url: None,
            publisher: None,
            publication_date: None,
            isbn: None,
            categories: vec![],
            page_count: Some(100),
            language: Some("en".into()),
            rating: None,
            rating_count: None,
            in_library: true,
            favorite: false,
            library_id: Some("lb_1".into()),
        };
        let json: serde_json::Value = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["externalId"], "OL123W");
        assert_eq!(json["inLibrary"], true);
        assert_eq!(json["libraryId"], "lb_1");
    }
}
