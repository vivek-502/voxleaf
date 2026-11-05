//! Row shapes and mappers.
//!
//! The web app kept every row→DTO conversion in `src/server/db/mappers.ts` so
//! no route could accidentally leak a storage key or an internal timestamp
//! format. This is that file: one place that turns SQLite rows into exactly the
//! JSON the React types expect.

use rusqlite::Row;
use serde_json::{json, Value};

use crate::error::{ApiError, ApiResult};
use crate::ids;
use crate::models::*;

pub const MAX_TIMESTAMP: f64 = 60.0 * 60.0 * 24.0 * 30.0;

/// Clamp a float into `[min, max]`, falling back to `default` when it is not a
/// finite number. Mirrors the web app's `safeNumber`.
pub fn safe_number(value: Option<f64>, min: f64, max: f64, default: f64) -> f64 {
    match value {
        Some(v) if v.is_finite() => v.clamp(min, max),
        _ => default,
    }
}

/// Parse a JSON array-of-strings column, tolerating anything malformed.
pub fn parse_string_array(raw: Option<&str>) -> Vec<String> {
    raw.and_then(|r| serde_json::from_str::<Vec<String>>(r).ok())
        .unwrap_or_default()
}

/// Read the `raw` metadata blob the way `mapBookSummary` did: ratings are
/// volatile provider data, so they are promoted out of the blob and nowhere
/// else.
pub fn parse_raw_metadata(raw: Option<&str>) -> (Option<f64>, Option<i64>) {
    let Some(raw) = raw else { return (None, None) };
    let Ok(parsed) = serde_json::from_str::<Value>(raw) else {
        return (None, None);
    };
    let rating = parsed
        .get("rating")
        .and_then(|v| v.as_f64())
        .filter(|v| v.is_finite() && (0.0..=5.0).contains(v));
    let rating_count = parsed
        .get("ratingCount")
        .and_then(|v| v.as_f64())
        .filter(|v| v.is_finite() && (0.0..=10_000_000.0).contains(v))
        .map(|v| v as i64);
    (rating, rating_count)
}

/* -------------------------------------------------------------------------- */
/* Book                                                                       */
/* -------------------------------------------------------------------------- */

pub fn map_book_summary(row: &Row<'_>) -> rusqlite::Result<BookSummary> {
    let raw: Option<String> = row.get("raw")?;
    let (rating, rating_count) = parse_raw_metadata(raw.as_deref());
    Ok(BookSummary {
        id: row.get("id")?,
        external_id: row.get("external_id")?,
        title: row.get("title")?,
        subtitle: row.get("subtitle")?,
        author: row.get("author").unwrap_or_default(),
        description: row.get("description")?,
        cover_url: row.get("cover_url")?,
        publisher: row.get("publisher")?,
        publication_date: row.get("publication_date")?,
        isbn: row.get("isbn")?,
        categories: parse_string_array(row.get::<_, Option<String>>("categories")?.as_deref()),
        page_count: row.get("page_count")?,
        language: row.get("language")?,
        rating,
        rating_count,
        in_library: row.get::<_, Option<String>>("library_id")?.is_some(),
        favorite: row.get::<_, i64>("favorite").unwrap_or(0) != 0,
        library_id: row.get("library_id")?,
    })
}

/// The column list every "book summary with library state" query selects.
pub const BOOK_SUMMARY_COLUMNS: &str = "b.id, b.external_id, b.title, b.subtitle, b.author, \
     b.description, b.cover_url, b.publisher, b.publication_date, b.isbn, b.categories, \
     b.page_count, b.language, b.raw, \
     lb.id AS library_id, lb.favorite";

pub const BOOK_FROM: &str = "FROM books b \
     LEFT JOIN library_books lb ON lb.book_id = b.id";

/* -------------------------------------------------------------------------- */
/* Chapter audio & progress                                                   */
/* -------------------------------------------------------------------------- */

pub fn map_chapter_audio(row: &Row<'_>, prefix: &str) -> rusqlite::Result<Option<ChapterAudioInfo>> {
    let audio_id: Option<String> = row.get(format!("{prefix}audio_id").as_str())?;
    let Some(audio_id) = audio_id else { return Ok(None) };
    Ok(Some(ChapterAudioInfo {
        chapter_id: row.get(format!("{prefix}chapter_id").as_str())?,
        stream_url: format!("/api/audio/{audio_id}"),
        duration: safe_number(
            row.get::<_, Option<f64>>(format!("{prefix}audio_duration").as_str())?,
            0.0,
            MAX_TIMESTAMP,
            0.0,
        ),
        file_size: row.get(format!("{prefix}file_size").as_str()).unwrap_or(0),
        mime_type: row
            .get::<_, Option<String>>(format!("{prefix}mime_type").as_str())?
            .unwrap_or_else(|| "audio/mpeg".to_string()),
        original_name: row.get(format!("{prefix}original_name").as_str())?,
        uploaded_at: row
            .get::<_, Option<String>>(format!("{prefix}uploaded_at").as_str())?
            .unwrap_or_else(|| now_iso()),
        id: audio_id,
    }))
}

pub fn map_chapter_progress(
    row: &Row<'_>,
    prefix: &str,
) -> rusqlite::Result<Option<ChapterProgressInfo>> {
    let position: Option<f64> = row.get(format!("{prefix}progress_position").as_str())?;
    if position.is_none() {
        return Ok(None);
    }
    Ok(Some(ChapterProgressInfo {
        position: safe_number(position, 0.0, f64::MAX, 0.0),
        duration: safe_number(
            row.get(format!("{prefix}progress_duration").as_str())?,
            0.0,
            f64::MAX,
            0.0,
        ),
        percentage: safe_number(row.get(format!("{prefix}percentage").as_str())?, 0.0, 1.0, 0.0),
        completed: row.get::<_, i64>(format!("{prefix}completed").as_str()).unwrap_or(0) != 0,
        play_count: row.get(format!("{prefix}play_count").as_str()).unwrap_or(0),
        updated_at: row
            .get::<_, Option<String>>(format!("{prefix}progress_updated_at").as_str())?
            .unwrap_or_else(|| now_iso()),
    }))
}

/// Columns needed to map one chapter with its audio and progress.
pub const CHAPTER_RELATION_COLUMNS: &str = "\
     a.id AS audio_id, a.chapter_id, a.duration AS audio_duration, a.file_size, \
     a.mime_type, a.original_name, a.uploaded_at, \
     p.position AS progress_position, p.duration AS progress_duration, p.percentage, \
     p.completed, p.play_count, p.updated_at AS progress_updated_at";

pub const CHAPTER_RELATION_FROM: &str = "\
     LEFT JOIN chapter_audio a ON a.chapter_id = c.id \
     LEFT JOIN listening_progress p ON p.chapter_id = c.id";

pub fn map_chapter(row: &Row<'_>, prefix: &str) -> ApiResult<ChapterInfo> {
    let audio = map_chapter_audio(row, prefix)
        .map_err(|e| ApiError::internal(format!("audio mapping failed: {e}")))?;
    let progress = map_chapter_progress(row, prefix)
        .map_err(|e| ApiError::internal(format!("progress mapping failed: {e}")))?;
    Ok(ChapterInfo {
        id: row.get(format!("{prefix}id").as_str()).map_err(|e| {
            ApiError::internal(format!("chapter mapping failed: {e}"))
        })?,
        book_id: row.get(format!("{prefix}book_id").as_str()).unwrap_or_default(),
        chapter_number: row.get(format!("{prefix}chapter_number").as_str()).unwrap_or(0),
        title: row.get(format!("{prefix}title").as_str()).unwrap_or_default(),
        sort_index: row.get(format!("{prefix}sort_index").as_str()).unwrap_or(0),
        audio,
        progress,
        bookmark_count: row
            .get::<_, Option<i64>>(format!("{prefix}bookmark_count").as_str())
            .unwrap_or(None)
            .unwrap_or(0),
        created_at: row
            .get::<_, Option<String>>(format!("{prefix}created_at").as_str())
            .unwrap_or(None)
            .unwrap_or_else(|| now_iso()),
        updated_at: row
            .get::<_, Option<String>>(format!("{prefix}updated_at").as_str())
            .unwrap_or(None)
            .unwrap_or_else(|| now_iso()),
    })
}

/* -------------------------------------------------------------------------- */
/* Bookmark                                                                   */
/* -------------------------------------------------------------------------- */

pub fn map_bookmark(row: &Row<'_>) -> rusqlite::Result<BookmarkInfo> {
    Ok(BookmarkInfo {
        id: row.get("id")?,
        chapter_id: row.get("chapter_id")?,
        book_id: row.get("book_id")?,
        book_title: row.get("book_title")?,
        chapter_title: row
            .get::<_, Option<String>>("chapter_title")?
            .unwrap_or_else(|| "Chapter".to_string()),
        chapter_number: row.get::<_, Option<i64>>("chapter_number")?.unwrap_or(0),
        timestamp: safe_number(row.get("timestamp")?, 0.0, f64::MAX, 0.0),
        note: row.get("note")?,
        created_at: row.get("created_at")?,
    })
}

pub const BOOKMARK_COLUMNS: &str = "bm.id, bm.chapter_id, bm.book_id, bm.book_title, \
     bm.timestamp, bm.note, bm.created_at, c.title AS chapter_title, c.chapter_number";

pub const BOOKMARK_FROM: &str = "FROM bookmarks bm \
     LEFT JOIN chapters c ON c.id = bm.chapter_id";

/// UTC now, ISO-8601 with a `Z` suffix — the format Prisma produced.
pub fn now_iso() -> String {
    chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string()
}

/// A fresh prefixed id, e.g. `bk_xxxxxxxxxxxx`.
pub fn new_id(prefix: &str) -> String {
    ids::new_id(prefix)
}

/// A chapter's identity, used by every chapter-scoped route so a bad id is a
/// 404 rather than a foreign-key error.
#[derive(Debug, Clone)]
pub struct ChapterRef {
    pub id: String,
    pub book_id: String,
    pub title: String,
    pub chapter_number: i64,
    pub epub_index: Option<i64>,
}

pub fn require_chapter(
    conn: &rusqlite::Connection,
    chapter_id: &str,
) -> ApiResult<ChapterRef> {
    conn.query_row(
        "SELECT id, book_id, title, chapter_number, epub_index FROM chapters WHERE id = ?1",
        [chapter_id],
        |row| {
            Ok(ChapterRef {
                id: row.get(0)?,
                book_id: row.get(1)?,
                title: row.get(2)?,
                chapter_number: row.get(3)?,
                epub_index: row.get(4)?,
            })
        },
    )
    .map_err(|_| ApiError::not_found("That chapter no longer exists."))
}

/// Look up a chapter's book id only.
pub fn chapter_book_id(conn: &rusqlite::Connection, chapter_id: &str) -> ApiResult<String> {
    conn.query_row(
        "SELECT book_id FROM chapters WHERE id = ?1",
        [chapter_id],
        |row| row.get(0),
    )
    .map_err(|_| ApiError::not_found("That chapter no longer exists."))
}

/// Look up a book, or 404.
pub fn require_book(conn: &rusqlite::Connection, book_id: &str) -> ApiResult<String> {
    conn.query_row("SELECT id FROM books WHERE id = ?1", [book_id], |row| row.get(0))
        .map_err(|_| ApiError::not_found("That book no longer exists."))
}

/// Look up a library entry id, returning its book id.
pub fn require_library_entry(
    conn: &rusqlite::Connection,
    library_id: &str,
) -> ApiResult<String> {
    conn.query_row(
        "SELECT book_id FROM library_books WHERE id = ?1",
        [library_id],
        |row| row.get(0),
    )
    .map_err(|_| ApiError::not_found("That book is not in your library."))
}

/// JSON helper: a `Vec<String>` as a JSON array value.
pub fn json_strings(values: &[String]) -> Value {
    json!(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_number_clamps_and_defaults() {
        assert_eq!(safe_number(Some(0.5), 0.0, 1.0, 0.0), 0.5);
        assert_eq!(safe_number(Some(7.0), 0.0, 5.0, 0.0), 5.0);
        assert_eq!(safe_number(Some(-1.0), 0.0, 5.0, 0.0), 0.0);
        assert_eq!(safe_number(Some(f64::NAN), 0.0, 1.0, 0.42), 0.42);
        assert_eq!(safe_number(None, 0.0, 1.0, 0.42), 0.42);
    }

    #[test]
    fn raw_metadata_promotes_ratings_only() {
        let (rating, count) = parse_raw_metadata(Some(r#"{"rating":4.5,"ratingCount":120}"#));
        assert_eq!(rating, Some(4.5));
        assert_eq!(count, Some(120));

        let (rating, count) = parse_raw_metadata(Some(r#"{"rating":99}"#));
        assert_eq!(rating, None);
        assert_eq!(count, None);

        let (rating, count) = parse_raw_metadata(None);
        assert_eq!((rating, count), (None, None));
    }

    #[test]
    fn malformed_categories_parse_as_empty() {
        assert!(parse_string_array(Some("not json")).is_empty());
        assert_eq!(
            parse_string_array(Some(r#"["History","Biography"]"#)),
            vec!["History".to_string(), "Biography".to_string()]
        );
    }
}
