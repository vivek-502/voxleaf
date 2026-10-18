//! Bookmarks — markers at a timestamp inside a chapter.
//!
//! Port of `src/app/api/bookmarks/*` and the bookmark half of
//! `src/server/db/repository.ts`.

use serde_json::{to_value, Value};

use crate::db::Database;
use crate::error::{ApiError, ApiResult};
use crate::models::BookmarkInfo;
use crate::repo;

use super::library::{block_db, request_error, sanitize_text, MAX_TIMESTAMP};

/// The longest note we keep (`MAX_BOOKMARK_NOTE`).
const MAX_NOTE: usize = 2000;
/// How many bookmarks the list endpoint returns (`listBookmarks` default).
const LIST_LIMIT: i64 = 200;
/// Prisma's `Number.MAX_SAFE_INTEGER`, the clamp when a chapter has no audio.
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

const SELECT: &str =
    "SELECT bm.id, bm.chapter_id, bm.book_id, bm.book_title, bm.timestamp, bm.note, \
     bm.created_at, c.title AS chapter_title, c.chapter_number \
     FROM bookmarks bm LEFT JOIN chapters c ON c.id = bm.chapter_id";

/// GET /api/bookmarks — every bookmark, newest first, optionally one chapter's.
pub fn list(db: &Database, chapter_id: Option<&str>) -> ApiResult<Value> {
    let chapter_id = chapter_id.map(|value| value.to_string());
    block_db(db, move |conn| {
        let mut items: Vec<Value> = Vec::new();
        match chapter_id {
            Some(chapter_id) => {
                let mut statement = conn
                    .prepare(&format!("{SELECT} WHERE bm.chapter_id = ?1 ORDER BY bm.created_at DESC LIMIT {LIST_LIMIT}"))?;
                let rows = statement.query_map([chapter_id], map_bookmark)?;
                for row in rows {
                    items.push(to_value(row?)?);
                }
            }
            None => {
                let mut statement = conn.prepare(&format!(
                    "{SELECT} ORDER BY bm.created_at DESC LIMIT {LIST_LIMIT}"
                ))?;
                let rows = statement.query_map([], map_bookmark)?;
                for row in rows {
                    items.push(to_value(row?)?);
                }
            }
        }
        Ok(Value::Array(items))
    })
}

/// POST /api/bookmarks — drop a marker at the current timestamp.
pub fn create(db: &Database, body: &Value) -> ApiResult<Value> {
    let chapter_id = required_trimmed(body, "chapterId", 64)?;
    let timestamp = timestamp_field(body)?;
    let note = optional_note(body)?;

    block_db(db, move |conn| {
        let (book_id, book_title) = conn
            .query_row(
                "SELECT c.book_id, b.title FROM chapters c \
                 JOIN books b ON b.id = c.book_id WHERE c.id = ?1",
                [&chapter_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .map_err(|_| request_error(404, "Chapter not found"))?;

        let duration: f64 = conn
            .query_row(
                "SELECT duration FROM chapter_audio WHERE chapter_id = ?1",
                [&chapter_id],
                |row| row.get(0),
            )
            .unwrap_or(0.0);
        let timestamp = timestamp.max(0.0).min(if duration > 0.0 { duration } else { MAX_SAFE_INTEGER });

        let bookmark_id = repo::new_id("bm");
        let now = repo::now_iso();
        conn.execute(
            "INSERT INTO bookmarks (id, chapter_id, timestamp, note, book_title, book_id, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![bookmark_id, chapter_id, timestamp, note, book_title, book_id, now],
        )?;

        let bookmark = load_bookmark(conn, &bookmark_id)?;
        Ok(to_value(bookmark)?)
    })
}

/// PATCH /api/bookmarks/:bookmarkId — edit the note.
pub fn update(db: &Database, id: &str, body: &Value) -> ApiResult<Value> {
    let note = match body.as_object().and_then(|object| object.get("note")) {
        None => {
            return Err(ApiError::invalid("note", "Invalid input: expected string, received undefined"))
        }
        Some(Value::Null) => None,
        Some(value) => Some(
            sanitize_text(value.as_str().unwrap_or(""), MAX_NOTE),
        ),
    };
    let note = note.filter(|note| !note.is_empty());
    let id = id.to_string();

    block_db(db, move |conn| {
        let found = conn
            .query_row("SELECT 1 FROM bookmarks WHERE id = ?1", [&id], |row| {
                row.get::<_, i64>(0)
            })
            .is_ok();
        if !found {
            return Err(ApiError::not_found("That bookmark no longer exists."));
        }
        conn.execute(
            "UPDATE bookmarks SET note = ?1 WHERE id = ?2",
            rusqlite::params![note, id],
        )?;
        let bookmark = load_bookmark(conn, &id)?;
        Ok(to_value(bookmark)?)
    })
}

/// DELETE /api/bookmarks/:bookmarkId.
pub fn remove(db: &Database, id: &str) -> ApiResult<()> {
    let id = id.to_string();
    block_db(db, move |conn| {
        let deleted = conn.execute("DELETE FROM bookmarks WHERE id = ?1", [&id])?;
        if deleted == 0 {
            return Err(ApiError::not_found("That bookmark no longer exists."));
        }
        Ok(())
    })
}

/* -------------------------------------------------------------------------- */
/* Helpers                                                                    */
/* -------------------------------------------------------------------------- */

fn map_bookmark(row: &rusqlite::Row<'_>) -> rusqlite::Result<BookmarkInfo> {
    repo::map_bookmark(row)
}

fn load_bookmark(conn: &rusqlite::Connection, id: &str) -> ApiResult<BookmarkInfo> {
    let mut statement = conn.prepare(&format!("{SELECT} WHERE bm.id = ?1"))?;
    let mut rows = statement.query_map([id], map_bookmark)?;
    match rows.next() {
        Some(row) => Ok(row?),
        None => Err(ApiError::not_found("That bookmark no longer exists.")),
    }
}

fn required_trimmed(body: &Value, key: &str, max: usize) -> ApiResult<String> {
    let value = body
        .as_object()
        .and_then(|object| object.get(key))
        .and_then(|value| value.as_str())
        .map(|value| value.trim().to_string())
        .unwrap_or_default();
    if value.is_empty() {
        return Err(ApiError::invalid(key, "String must contain at least 1 character(s)."));
    }
    if value.chars().count() > max {
        return Err(ApiError::invalid(key, format!("String must contain at most {max} character(s).")));
    }
    Ok(value)
}

/// `createBookmarkSchema.timestamp`: 0..=MAX_TIMESTAMP.
fn timestamp_field(body: &Value) -> ApiResult<f64> {
    let value = body
        .as_object()
        .and_then(|object| object.get("timestamp"))
        .and_then(|value| value.as_f64())
        .ok_or_else(|| ApiError::invalid("timestamp", "Invalid input: expected number."))?;
    if !(0.0..=MAX_TIMESTAMP).contains(&value) {
        return Err(ApiError::invalid(
            "timestamp",
            format!("Number must be greater than or equal to 0 and less than or equal to {MAX_TIMESTAMP}."),
        ));
    }
    Ok(value)
}

/// `createBookmarkSchema.note`: optional, trimmed, at most 2000 characters.
fn optional_note(body: &Value) -> ApiResult<Option<String>> {
    match body.as_object().and_then(|object| object.get("note")) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => {
            let raw = value
                .as_str()
                .ok_or_else(|| ApiError::invalid("note", "Invalid input: expected string."))?;
            if raw.chars().count() > MAX_NOTE {
                return Err(ApiError::invalid(
                    "note",
                    format!("String must contain at most {MAX_NOTE} character(s)."),
                ));
            }
            let note = sanitize_text(raw, MAX_NOTE);
            Ok(if note.is_empty() { None } else { Some(note) })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use crate::services::library::tests_support::fixture;

    #[tokio::test(flavor = "multi_thread")]
    async fn create_clamps_to_the_audio_duration_and_maps_chapter_fields() {
        let fixture = fixture();
        fixture.seed_book("bk1", 2, true);

        let bookmark = create(
            &fixture.db,
            &json!({ "chapterId": "chbk1-0", "timestamp": 500.0, "note": "  a note  " }),
        )
        .expect("create");
        assert_eq!(bookmark["timestamp"].as_f64().unwrap(), 100.0, "clamped to duration");
        assert_eq!(bookmark["note"], "a note");
        assert_eq!(bookmark["bookTitle"], "Book bk1");
        assert_eq!(bookmark["chapterTitle"], "Chapter 1");
        assert_eq!(bookmark["chapterNumber"], 1);

        // A chapter without audio clamps to the (huge) safe-integer ceiling.
        let bookmark = create(
            &fixture.db,
            &json!({ "chapterId": "chbk1-1", "timestamp": 42.0 }),
        )
        .expect("create");
        assert_eq!(bookmark["timestamp"].as_f64().unwrap(), 42.0);
        assert!(bookmark["note"].is_null());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn create_rejects_unknown_chapters_and_bad_timestamps() {
        let fixture = fixture();
        fixture.seed_book("bk1", 1, true);

        let error = create(
            &fixture.db,
            &json!({ "chapterId": "nope", "timestamp": 1.0 }),
        )
        .expect_err("unknown chapter");
        assert_eq!(error.status, axum::http::StatusCode::NOT_FOUND);

        let error = create(
            &fixture.db,
            &json!({ "chapterId": "chbk1-0", "timestamp": -1.0 }),
        )
        .expect_err("negative timestamp");
        assert_eq!(error.status, axum::http::StatusCode::BAD_REQUEST);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn list_orders_newest_first_and_filters_by_chapter() {
        let fixture = fixture();
        fixture.seed_book("bk1", 2, true);
        for (chapter, timestamp) in [("chbk1-0", 10.0), ("chbk1-1", 20.0), ("chbk1-0", 30.0)] {
            create(
                &fixture.db,
                &json!({ "chapterId": chapter, "timestamp": timestamp }),
            )
            .expect("create");
        }

        let all = list(&fixture.db, None).expect("list");
        let items = all.as_array().expect("array");
        assert_eq!(items.len(), 3);
        assert_eq!(items[0]["timestamp"].as_f64().unwrap(), 30.0, "newest first");

        let filtered = list(&fixture.db, Some("chbk1-1")).expect("filtered");
        let items = filtered.as_array().expect("array");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["chapterNumber"], 2);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn update_requires_the_note_key_and_missing_rows_404() {
        let fixture = fixture();
        fixture.seed_book("bk1", 1, true);
        let bookmark = create(
            &fixture.db,
            &json!({ "chapterId": "chbk1-0", "timestamp": 5.0, "note": "old" }),
        )
        .expect("create");
        let id = bookmark["id"].as_str().expect("id").to_string();

        let error = update(&fixture.db, &id, &json!({})).expect_err("no note key");
        assert_eq!(error.status, axum::http::StatusCode::BAD_REQUEST);

        let updated = update(&fixture.db, &id, &json!({ "note": "  new  " })).expect("update");
        assert_eq!(updated["note"], "new");

        let cleared = update(&fixture.db, &id, &json!({ "note": null })).expect("clear");
        assert!(cleared["note"].is_null());

        let error = update(&fixture.db, "missing", &json!({ "note": "x" }))
            .expect_err("missing bookmark");
        assert_eq!(error.message, "That bookmark no longer exists.");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn remove_deletes_once_then_404s() {
        let fixture = fixture();
        fixture.seed_book("bk1", 1, true);
        let bookmark = create(
            &fixture.db,
            &json!({ "chapterId": "chbk1-0", "timestamp": 5.0 }),
        )
        .expect("create");
        let id = bookmark["id"].as_str().expect("id").to_string();

        remove(&fixture.db, &id).expect("remove");
        let error = remove(&fixture.db, &id).expect_err("already gone");
        assert_eq!(error.message, "That bookmark no longer exists.");
    }
}
