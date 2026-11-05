//! Saved places inside a book.
//!
//! Port of `src/server/services/reader-bookmarks.ts`. A bookmark is
//! deliberately language-independent: it records a section and how far through
//! it the reader was. Reading the same section in Hindi or Spanish does not move
//! the bookmark, because the place in the book did not move.

use rusqlite::OptionalExtension;
use serde_json::{json, Value};

use crate::db::Database;
use crate::error::{ApiError, ApiResult};
use crate::repo;
use crate::services::library::block_db;

const LABEL_LIMIT: usize = 200;

fn to_dto(row: &rusqlite::Row<'_>, chapter_title: &str) -> rusqlite::Result<Value> {
    Ok(json!({
        "id": row.get::<_, String>("id")?,
        "bookId": row.get::<_, String>("book_id")?,
        "chapterIndex": row.get::<_, i64>("chapter_index")?,
        "chapterTitle": chapter_title,
        "scrollPercent": row.get::<_, f64>("scroll_percent")?,
        "label": row.get::<_, Option<String>>("label")?,
        "createdAt": row.get::<_, String>("created_at")?,
        "updatedAt": row.get::<_, String>("updated_at")?,
    }))
}

const SELECT: &str = "SELECT id, book_id, chapter_index, scroll_percent, label, \
     created_at, updated_at FROM epub_bookmarks";

fn titles_for(conn: &rusqlite::Connection, book_id: &str) -> ApiResult<Vec<(i64, String)>> {
    let mut statement = conn.prepare(
        "SELECT c.idx, c.title FROM epub_chapters c \
         JOIN epub_files f ON c.epub_id = f.id WHERE f.book_id = ?1",
    )?;
    let rows = statement
        .query_map([book_id], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn title(titles: &[(i64, String)], index: i64) -> String {
    titles
        .iter()
        .find(|(at, _)| *at == index)
        .map(|(_, title)| title.clone())
        .unwrap_or_else(|| format!("Section {}", index + 1))
}

/// GET /api/epub/:bookId/bookmarks — saved places, in reading order.
pub fn list(db: &Database, book_id: &str) -> ApiResult<Value> {
    let book_id = book_id.to_string();
    block_db(db, move |conn| {
        let titles = titles_for(conn, &book_id)?;
        let mut statement = conn.prepare(&format!(
            "{SELECT} WHERE book_id = ?1 ORDER BY chapter_index ASC, scroll_percent ASC"
        ))?;
        let rows = statement
            .query_map([&book_id], |row| {
                let index: i64 = row.get("chapter_index")?;
                to_dto(row, &title(&titles, index))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(json!({ "bookmarks": rows }))
    })
}

fn required_index(body: &Value) -> ApiResult<i64> {
    let value = body
        .get("chapterIndex")
        .and_then(Value::as_f64)
        .filter(|value| value.fract() == 0.0)
        .ok_or_else(|| ApiError::invalid("chapterIndex", "Invalid input: expected integer."))?;
    let value = value as i64;
    if !(0..=100_000).contains(&value) {
        return Err(ApiError::invalid(
            "chapterIndex",
            "Number must be greater than or equal to 0 and less than or equal to 100000.",
        ));
    }
    Ok(value)
}

fn scroll_percent(body: &Value) -> ApiResult<f64> {
    let value = body
        .get("scrollPercent")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    if !(0.0..=1.0).contains(&value) || !value.is_finite() {
        return Err(ApiError::invalid(
            "scrollPercent",
            "Number must be greater than or equal to 0 and less than or equal to 1.",
        ));
    }
    Ok(value)
}

fn optional_label(body: &Value) -> ApiResult<Option<String>> {
    match body.get("label") {
        None | Some(Value::Null) => Ok(None),
        Some(value) => {
            let raw = value
                .as_str()
                .ok_or_else(|| ApiError::invalid("label", "Invalid input: expected string."))?;
            let label = raw.trim();
            if label.chars().count() > LABEL_LIMIT {
                return Err(ApiError::invalid(
                    "label",
                    format!("String must contain at most {LABEL_LIMIT} character(s)."),
                ));
            }
            Ok((!label.is_empty()).then(|| label.to_string()))
        }
    }
}

/// POST /api/epub/:bookId/bookmarks — save the reader's place.
pub fn create(db: &Database, book_id: &str, body: &Value) -> ApiResult<Value> {
    let chapter_index = required_index(body)?;
    let percent = scroll_percent(body)?;
    let label = optional_label(body)?;

    let book_id = book_id.to_string();
    block_db(db, move |conn| {
        let chapter: Option<String> = conn
            .query_row(
                "SELECT c.title FROM epub_chapters c \
                 JOIN epub_files f ON c.epub_id = f.id \
                 WHERE f.book_id = ?1 AND c.idx = ?2",
                rusqlite::params![book_id, chapter_index],
                |row| row.get(0),
            )
            .optional()?;
        let Some(chapter) = chapter else {
            return Err(ApiError::not_found("That section does not exist."));
        };

        let id = repo::new_id("eb_");
        let now = repo::now_iso();
        conn.execute(
            "INSERT INTO epub_bookmarks \
             (id, book_id, chapter_index, scroll_percent, label, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
            rusqlite::params![id, book_id, chapter_index, percent, label, now],
        )?;
        load(conn, &id, &chapter)
    })
}

/// PATCH /api/epub/:bookId/bookmarks/:id — rename a saved place.
pub fn update(db: &Database, book_id: &str, id: &str, body: &Value) -> ApiResult<Value> {
    let raw = body.get("label").ok_or_else(|| {
        ApiError::invalid(
            "label",
            "Invalid input: expected string, received undefined",
        )
    })?;
    let label = match raw {
        Value::Null => None,
        value => {
            let text = value
                .as_str()
                .ok_or_else(|| ApiError::invalid("label", "Invalid input: expected string."))?;
            let text = text.trim();
            if text.chars().count() > LABEL_LIMIT {
                return Err(ApiError::invalid(
                    "label",
                    format!("String must contain at most {LABEL_LIMIT} character(s)."),
                ));
            }
            (!text.is_empty()).then(|| text.to_string())
        }
    };

    let book_id = book_id.to_string();
    let id = id.to_string();
    block_db(db, move |conn| {
        let chapter_index: Option<i64> = conn
            .query_row(
                "SELECT chapter_index FROM epub_bookmarks WHERE id = ?1 AND book_id = ?2",
                rusqlite::params![id, book_id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(chapter_index) = chapter_index else {
            return Err(ApiError::not_found("That bookmark does not exist."));
        };

        conn.execute(
            "UPDATE epub_bookmarks SET label = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![label, repo::now_iso(), id],
        )?;

        let chapter: Option<String> = conn
            .query_row(
                "SELECT c.title FROM epub_chapters c JOIN epub_files f ON c.epub_id = f.id \
                 WHERE f.book_id = ?1 AND c.idx = ?2",
                rusqlite::params![book_id, chapter_index],
                |row| row.get(0),
            )
            .optional()?;
        let chapter = chapter.unwrap_or_else(|| format!("Section {}", chapter_index + 1));
        load(conn, &id, &chapter)
    })
}

/// DELETE /api/epub/:bookId/bookmarks/:id.
pub fn remove(db: &Database, book_id: &str, id: &str) -> ApiResult<()> {
    let (book_id, id) = (book_id.to_string(), id.to_string());
    block_db(db, move |conn| {
        let deleted = conn.execute(
            "DELETE FROM epub_bookmarks WHERE id = ?1 AND book_id = ?2",
            rusqlite::params![id, book_id],
        )?;
        if deleted == 0 {
            return Err(ApiError::not_found("That bookmark does not exist."));
        }
        Ok(())
    })
}

fn load(conn: &rusqlite::Connection, id: &str, chapter: &str) -> ApiResult<Value> {
    let mut statement = conn.prepare(&format!("{SELECT} WHERE id = ?1"))?;
    let mut rows = statement.query([id])?;
    match rows.next()? {
        Some(row) => Ok(to_dto(row, chapter)?),
        None => Err(ApiError::not_found("That bookmark does not exist.")),
    }
}
