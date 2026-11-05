//! Highlights and notes, anchored to the source text.
//!
//! Port of `src/server/services/reader-annotations.ts`. The reader can be
//! looking at a machine translation when a highlight is made, and that
//! translation can be replaced or switched to another language later. Storing
//! character offsets into the visible words would therefore lose every
//! annotation the moment the page changed language. Instead the anchor is a
//! pair of block indices plus offsets into the *source* block text — the same
//! blocks the translator uses — so the highlight has a home that outlives the
//! words.

use rusqlite::OptionalExtension;
use serde_json::{json, Value};

use crate::db::Database;
use crate::error::{ApiError, ApiResult};
use crate::repo;
use crate::services::library::block_db;
use crate::services::translation::extract_text_blocks;

const QUOTE_LIMIT: usize = 4000;
const NOTE_LIMIT: usize = 8000;
const COLORS: &[&str] = &["yellow", "green", "blue", "pink", "violet"];

/// One anchored highlight, as the client sent it.
#[derive(Debug, Clone)]
struct Anchor {
    chapter_index: i64,
    start_block: i64,
    start_offset: i64,
    end_block: i64,
    end_offset: i64,
    quote: String,
    selected_text: String,
    language: Option<String>,
    color: String,
    note: Option<String>,
}

/// A chapter of the book's EPUB, enough to validate and title an anchor.
struct Chapter {
    index: i64,
    title: String,
    html: String,
}

fn chapters_for(db: &Database, book_id: &str) -> ApiResult<Vec<Chapter>> {
    let book_id = book_id.to_string();
    block_db(db, move |conn| {
        let ready: Option<String> = conn
            .query_row(
                "SELECT id FROM epub_files WHERE book_id = ?1",
                [&book_id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(epub_id) = ready else {
            return Err(ApiError::not_found("This book has no EPUB yet."));
        };
        let status: Option<String> = conn
            .query_row(
                "SELECT status FROM epub_files WHERE id = ?1",
                [&epub_id],
                |row| row.get(0),
            )
            .optional()?;
        if status.as_deref() != Some("ready") {
            return Err(ApiError::not_found("This book has no EPUB yet."));
        }

        let mut statement = conn.prepare(
            "SELECT idx, title, html FROM epub_chapters WHERE epub_id = ?1 ORDER BY idx ASC",
        )?;
        let rows = statement
            .query_map([&epub_id], |row| {
                Ok(Chapter {
                    index: row.get(0)?,
                    title: row.get(1)?,
                    html: row.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
}

fn chapter_title(chapters: &[Chapter], index: i64) -> String {
    chapters
        .iter()
        .find(|chapter| chapter.index == index)
        .map(|chapter| chapter.title.clone())
        .unwrap_or_else(|| format!("Section {}", index + 1))
}

/// UTF-16 length, the unit JavaScript's `String.length` counts and therefore
/// the unit the reader's selection offsets use.
fn utf16_len(text: &str) -> i64 {
    text.encode_utf16().count() as i64
}

fn clamp_index(value: i64, length: i64) -> i64 {
    value.clamp(0, length - 1)
}

/// Clamp an anchor onto the section that actually exists.
///
/// A re-parsed book can gain or lose blocks, and a client is not the only thing
/// that can send an index past the end. Storing an unreachable anchor would
/// mean a highlight that can never be drawn, so an out-of-range anchor heals to
/// the nearest real position instead.
fn clamp_anchor(
    anchor: Anchor,
    blocks: &[crate::services::translation::TextBlock],
) -> ApiResult<Anchor> {
    if blocks.is_empty() {
        return Err(ApiError::bad_request(
            "That section has no text to highlight.",
        ));
    }

    let length = blocks.len() as i64;
    let mut start_block = clamp_index(anchor.start_block, length);
    let mut end_block = clamp_index(anchor.end_block, length);
    let mut start_offset = anchor.start_offset;
    let mut end_offset = anchor.end_offset;

    if start_block > end_block {
        std::mem::swap(&mut start_block, &mut end_block);
        std::mem::swap(&mut start_offset, &mut end_offset);
    }

    start_offset = start_offset.clamp(0, utf16_len(&blocks[start_block as usize].text));
    end_offset = end_offset.clamp(0, utf16_len(&blocks[end_block as usize].text));

    if start_block == end_block && start_offset > end_offset {
        std::mem::swap(&mut start_offset, &mut end_offset);
    }

    let quote = anchor
        .quote
        .trim()
        .chars()
        .take(QUOTE_LIMIT)
        .collect::<String>();
    let selected = if anchor.selected_text.trim().is_empty() {
        quote.clone()
    } else {
        anchor
            .selected_text
            .trim()
            .chars()
            .take(QUOTE_LIMIT)
            .collect::<String>()
    };
    let note = match anchor.note {
        Some(note) if !note.trim().is_empty() => {
            Some(note.trim().chars().take(NOTE_LIMIT).collect::<String>())
        }
        _ => None,
    };

    Ok(Anchor {
        start_block,
        start_offset,
        end_block,
        end_offset,
        quote,
        selected_text: selected,
        note,
        ..anchor
    })
}

fn to_dto(row: &rusqlite::Row<'_>, chapter_title: &str) -> rusqlite::Result<Value> {
    Ok(json!({
        "id": row.get::<_, String>("id")?,
        "bookId": row.get::<_, String>("book_id")?,
        "chapterIndex": row.get::<_, i64>("chapter_index")?,
        "chapterTitle": chapter_title,
        "startBlock": row.get::<_, i64>("start_block")?,
        "startOffset": row.get::<_, i64>("start_offset")?,
        "endBlock": row.get::<_, i64>("end_block")?,
        "endOffset": row.get::<_, i64>("end_offset")?,
        "quote": row.get::<_, String>("quote")?,
        "selectedText": row.get::<_, String>("selected_text")?,
        "language": row.get::<_, Option<String>>("language")?,
        "color": row.get::<_, String>("color")?,
        "note": row.get::<_, Option<String>>("note")?,
        "createdAt": row.get::<_, String>("created_at")?,
        "updatedAt": row.get::<_, String>("updated_at")?,
    }))
}

const SELECT: &str = "SELECT id, book_id, chapter_index, start_block, start_offset, end_block, \
     end_offset, quote, selected_text, language, color, note, created_at, updated_at \
     FROM epub_annotations";

/// GET /api/epub/:bookId/annotations — every highlight and note in the book.
pub fn list(db: &Database, book_id: &str) -> ApiResult<Value> {
    let chapters = chapters_for(db, book_id)?;
    let book_id = book_id.to_string();
    let annotations = block_db(db, move |conn| {
        let mut statement = conn.prepare(&format!(
            "{SELECT} WHERE book_id = ?1 \
             ORDER BY chapter_index ASC, start_block ASC, start_offset ASC"
        ))?;
        let rows = statement
            .query_map([&book_id], |row| {
                let index: i64 = row.get("chapter_index")?;
                to_dto(row, &chapter_title(&chapters, index))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok::<_, ApiError>(rows)
    })?;
    Ok(json!({ "annotations": annotations }))
}

fn required_string(body: &Value, key: &str, max: usize) -> ApiResult<String> {
    let raw = body
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::invalid(key, "Invalid input: expected string."))?;
    let value = raw.trim().to_string();
    if value.is_empty() {
        return Err(ApiError::invalid(
            key,
            "String must contain at least 1 character(s).",
        ));
    }
    if value.chars().count() > max {
        return Err(ApiError::invalid(
            key,
            format!("String must contain at most {max} character(s)."),
        ));
    }
    Ok(value)
}

fn bounded_int(body: &Value, key: &str, max: i64) -> ApiResult<i64> {
    let value = body
        .get(key)
        .and_then(Value::as_f64)
        .filter(|value| value.fract() == 0.0)
        .ok_or_else(|| ApiError::invalid(key, "Invalid input: expected integer."))?;
    let value = value as i64;
    if !(0..=max).contains(&value) {
        return Err(ApiError::invalid(
            key,
            format!("Number must be greater than or equal to 0 and less than or equal to {max}."),
        ));
    }
    Ok(value)
}

fn optional_language(body: &Value) -> ApiResult<Option<String>> {
    match body.get("language") {
        None | Some(Value::Null) => Ok(None),
        Some(value) => {
            let raw = value
                .as_str()
                .ok_or_else(|| ApiError::invalid("language", "Invalid input: expected string."))?;
            let language = raw.trim().to_string();
            if language.encode_utf16().count() > 20 {
                return Err(ApiError::invalid(
                    "language",
                    "String must contain at most 20 character(s).",
                ));
            }
            if !crate::services::translation::is_valid_language_code(&language) {
                return Err(ApiError::invalid("language", "Not a language code."));
            }
            Ok(Some(language))
        }
    }
}

fn optional_note(body: &Value, max: usize) -> ApiResult<Option<String>> {
    match body.get("note") {
        None | Some(Value::Null) => Ok(None),
        Some(value) => {
            let raw = value
                .as_str()
                .ok_or_else(|| ApiError::invalid("note", "Invalid input: expected string."))?;
            let note = raw.trim();
            if note.chars().count() > max {
                return Err(ApiError::invalid(
                    "note",
                    format!("String must contain at most {max} character(s)."),
                ));
            }
            Ok((!note.is_empty()).then(|| note.to_string()))
        }
    }
}

fn parse_anchor(body: &Value) -> ApiResult<Anchor> {
    let color = match body.get("color") {
        None | Some(Value::Null) => "yellow".to_string(),
        Some(value) => {
            let color = value
                .as_str()
                .ok_or_else(|| ApiError::invalid("color", "Invalid input: expected string."))?;
            if !COLORS.contains(&color) {
                return Err(ApiError::invalid("color", "Invalid option."));
            }
            color.to_string()
        }
    };

    Ok(Anchor {
        chapter_index: bounded_int(body, "chapterIndex", 100_000)?,
        start_block: bounded_int(body, "startBlock", 1_000_000)?,
        start_offset: bounded_int(body, "startOffset", 1_000_000)?,
        end_block: bounded_int(body, "endBlock", 1_000_000)?,
        end_offset: bounded_int(body, "endOffset", 1_000_000)?,
        quote: required_string(body, "quote", QUOTE_LIMIT)?,
        selected_text: match body.get("selectedText") {
            None | Some(Value::Null) => String::new(),
            Some(_) => required_string(body, "selectedText", QUOTE_LIMIT)?,
        },
        language: optional_language(body)?,
        color,
        note: optional_note(body, NOTE_LIMIT)?,
    })
}

/// POST /api/epub/:bookId/annotations — keep a highlight, with or without a note.
pub fn create(db: &Database, book_id: &str, body: &Value) -> ApiResult<Value> {
    let anchor = parse_anchor(body)?;
    let chapters = chapters_for(db, book_id)?;
    let chapter = chapters
        .iter()
        .find(|chapter| chapter.index == anchor.chapter_index)
        .ok_or_else(|| ApiError::not_found("That section does not exist."))?;

    let blocks = extract_text_blocks(&chapter.html);
    let anchor = clamp_anchor(anchor, &blocks)?;
    if anchor.quote.is_empty() {
        return Err(ApiError::bad_request("Select some text to highlight."));
    }

    let title = chapter.title.clone();
    let book_id = book_id.to_string();
    let annotation = block_db(db, move |conn| {
        let id = repo::new_id("an_");
        let now = repo::now_iso();
        conn.execute(
            "INSERT INTO epub_annotations \
             (id, book_id, chapter_index, start_block, start_offset, end_block, end_offset, \
              quote, selected_text, language, color, note, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?13)",
            rusqlite::params![
                id,
                book_id,
                anchor.chapter_index,
                anchor.start_block,
                anchor.start_offset,
                anchor.end_block,
                anchor.end_offset,
                anchor.quote,
                anchor.selected_text,
                anchor.language,
                anchor.color,
                anchor.note,
                now,
            ],
        )?;
        load(conn, &id, &title)
    })?;
    Ok(annotation)
}

/// PATCH /api/epub/:bookId/annotations/:id — recolour or edit a note.
pub fn update(db: &Database, book_id: &str, id: &str, body: &Value) -> ApiResult<Value> {
    let color = match body.get("color") {
        Some(value) => {
            let color = value
                .as_str()
                .ok_or_else(|| ApiError::invalid("color", "Invalid input: expected string."))?;
            if !COLORS.contains(&color) {
                return Err(ApiError::invalid("color", "Invalid option."));
            }
            Some(color.to_string())
        }
        None => None,
    };
    let note = match body.get("note") {
        Some(value) => {
            let raw = value
                .as_str()
                .ok_or_else(|| ApiError::invalid("note", "Invalid input: expected string."))?;
            if raw.trim().chars().count() > NOTE_LIMIT {
                return Err(ApiError::invalid(
                    "note",
                    format!("String must contain at most {NOTE_LIMIT} character(s)."),
                ));
            }
            Some(if raw.trim().is_empty() {
                None
            } else {
                Some(raw.trim().to_string())
            })
        }
        None => None,
    };
    if color.is_none() && note.is_none() {
        return Err(ApiError::invalid("color", "Nothing to update."));
    }

    let book_id = book_id.to_string();
    let id = id.to_string();
    block_db(db, move |conn| {
        let chapter_index: Option<i64> = conn
            .query_row(
                "SELECT chapter_index FROM epub_annotations WHERE id = ?1 AND book_id = ?2",
                rusqlite::params![id, book_id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(chapter_index) = chapter_index else {
            return Err(ApiError::not_found("That highlight does not exist."));
        };

        if let Some(color) = color {
            conn.execute(
                "UPDATE epub_annotations SET color = ?1 WHERE id = ?2",
                rusqlite::params![color, id],
            )?;
        }
        if let Some(note) = note {
            conn.execute(
                "UPDATE epub_annotations SET note = ?1 WHERE id = ?2",
                rusqlite::params![note, id],
            )?;
        }
        conn.execute(
            "UPDATE epub_annotations SET updated_at = ?1 WHERE id = ?2",
            rusqlite::params![repo::now_iso(), id],
        )?;

        let title: Option<String> = conn
            .query_row(
                "SELECT c.title FROM epub_chapters c JOIN epub_files f ON c.epub_id = f.id \
                 WHERE f.book_id = ?1 AND c.idx = ?2",
                rusqlite::params![book_id, chapter_index],
                |row| row.get(0),
            )
            .optional()?;
        let title = title.unwrap_or_else(|| format!("Section {}", chapter_index + 1));
        load(conn, &id, &title)
    })
}

/// DELETE /api/epub/:bookId/annotations/:id.
pub fn remove(db: &Database, book_id: &str, id: &str) -> ApiResult<()> {
    let (book_id, id) = (book_id.to_string(), id.to_string());
    block_db(db, move |conn| {
        let deleted = conn.execute(
            "DELETE FROM epub_annotations WHERE id = ?1 AND book_id = ?2",
            rusqlite::params![id, book_id],
        )?;
        if deleted == 0 {
            return Err(ApiError::not_found("That highlight does not exist."));
        }
        Ok(())
    })
}

fn load(conn: &rusqlite::Connection, id: &str, title: &str) -> ApiResult<Value> {
    let mut statement = conn.prepare(&format!("{SELECT} WHERE id = ?1"))?;
    let mut rows = statement.query([id])?;
    match rows.next()? {
        Some(row) => Ok(to_dto(row, title)?),
        None => Err(ApiError::not_found("That highlight does not exist.")),
    }
}
