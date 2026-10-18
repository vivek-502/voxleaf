//! Listening progress: saves, completion toggles and resets.
//!
//! Port of `src/app/api/progress/*` and `src/server/services/progress.ts`.
//! Per-chapter positions live in `listening_progress`; `library_books` keeps a
//! denormalised rollup that `recompute_book_progress` refreshes after every
//! write.

use rusqlite::Connection;
use serde_json::{json, Value};

use crate::db::Database;
use crate::error::{ApiError, ApiResult};
use crate::repo;

use super::library::{block_db, load_chapter_infos, request_error, MAX_TIMESTAMP};

/// The threshold a chapter's percentage must reach before it counts as
/// completed (`DEFAULT_COMPLETION_THRESHOLD` in the web app).
pub const DEFAULT_COMPLETION_THRESHOLD: f64 = 0.95;

/// Below this fraction we treat a chapter as "not really started", so a stray
/// two-second play does not hijack "Continue listening".
const EFFECTIVE_START: f64 = 0.02;

/* -------------------------------------------------------------------------- */
/* Save                                                                       */
/* -------------------------------------------------------------------------- */

/// PUT /api/progress — persist a listening position.
pub fn save(db: &Database, body: &Value) -> ApiResult<Value> {
    let chapter_id = required_trimmed(body, "chapterId", 64)?;
    let position = timestamp_field(body, "position")?;
    let duration = timestamp_field(body, "duration")?;
    let increment_play_count = optional_bool(body, "incrementPlayCount")?;

    block_db(db, move |conn| {
        // `Math.max(0, Math.min(duration || MAX_SAFE_INTEGER, position))`.
        let duration = duration.max(0.0);
        let position = position.max(0.0).min(if duration > 0.0 { duration } else { f64::MAX });
        let threshold = completion_threshold(conn);
        let percentage = if duration > 0.0 {
            (position / duration).min(1.0)
        } else {
            0.0
        };
        let completed = duration > 0.0 && percentage >= threshold;

        let (book_id, title, chapter_number) = conn
            .query_row(
                "SELECT c.book_id, c.title, c.chapter_number \
                 FROM chapters c WHERE c.id = ?1",
                [&chapter_id],
                |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?))
                },
            )
            .map_err(|_| request_error(404, "Chapter not found"))?;

        let now = repo::now_iso();
        let play_count = if increment_play_count { 1 } else { 0 };
        conn.execute(
            "INSERT INTO listening_progress \
                (id, chapter_id, position, duration, percentage, completed, play_count, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8) \
             ON CONFLICT(chapter_id) DO UPDATE SET \
                position = excluded.position, \
                duration = excluded.duration, \
                percentage = excluded.percentage, \
                completed = excluded.completed, \
                play_count = listening_progress.play_count + excluded.play_count, \
                updated_at = excluded.updated_at",
            rusqlite::params![
                repo::new_id("lp"),
                chapter_id,
                position,
                duration,
                percentage,
                completed as i64,
                play_count,
                now
            ],
        )?;

        // Only meaningfully-started or completed chapters become the resume
        // point, so a stray 2-second play does not hijack "Continue listening".
        if percentage >= EFFECTIVE_START {
            conn.execute(
                "UPDATE library_books SET \
                    last_played_at = ?1, last_played_chapter_id = ?2, last_position = ?3, \
                    last_chapter_title = ?4, last_chapter_number = ?5 \
                 WHERE book_id = ?6",
                rusqlite::params![now, chapter_id, position, title, chapter_number, book_id],
            )?;
        }

        recompute_book_progress(conn, &book_id)?;
        Ok(json!({ "percentage": percentage, "completed": completed }))
    })
}

/* -------------------------------------------------------------------------- */
/* Completion toggle                                                          */
/* -------------------------------------------------------------------------- */

/// POST /api/progress/actions — manually mark a chapter finished or unfinished.
pub fn set_completed(db: &Database, body: &Value) -> ApiResult<Value> {
    let chapter_id = required_trimmed(body, "chapterId", 64)?;
    let completed = body
        .get("completed")
        .and_then(|value| value.as_bool())
        .ok_or_else(|| ApiError::invalid("completed", "Invalid input: expected boolean."))?;

    block_db(db, move |conn| {
        let book_id = conn
            .query_row("SELECT book_id FROM chapters WHERE id = ?1", [&chapter_id], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|_| ApiError::not_found("That chapter no longer exists."))?;

        // `markChapterCompleted` keeps the stored duration and only moves the
        // position to (or away from) its end.
        let duration: f64 = conn
            .query_row(
                "SELECT duration FROM listening_progress WHERE chapter_id = ?1",
                [&chapter_id],
                |row| row.get(0),
            )
            .unwrap_or(0.0);
        let position = if completed { duration } else { 0.0 };
        let percentage = if completed { 1.0 } else { 0.0 };
        let now = repo::now_iso();
        conn.execute(
            "INSERT INTO listening_progress \
                (id, chapter_id, position, duration, percentage, completed, play_count, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7, ?7) \
             ON CONFLICT(chapter_id) DO UPDATE SET \
                position = excluded.position, \
                percentage = excluded.percentage, \
                completed = excluded.completed, \
                updated_at = excluded.updated_at",
            rusqlite::params![
                repo::new_id("lp"),
                chapter_id,
                position,
                duration,
                percentage,
                completed as i64,
                now
            ],
        )?;

        recompute_book_progress(conn, &book_id)?;
        let chapters = load_chapter_infos(conn, &book_id)?;
        Ok(json!({ "chapters": chapters }))
    })
}

/* -------------------------------------------------------------------------- */
/* Reset                                                                      */
/* -------------------------------------------------------------------------- */

/// DELETE /api/progress/actions — clear all listening state for a book.
pub fn reset(db: &Database, body: &Value) -> ApiResult<()> {
    let book_id = required_trimmed(body, "bookId", 64)?;

    block_db(db, move |conn| {
        let found = conn
            .query_row(
                "SELECT 1 FROM library_books WHERE book_id = ?1",
                [&book_id],
                |row| row.get::<_, i64>(0),
            )
            .is_ok();
        if !found {
            return Err(ApiError::not_found("That book is not in your library."));
        }

        conn.execute(
            "DELETE FROM listening_progress \
             WHERE chapter_id IN (SELECT id FROM chapters WHERE book_id = ?1)",
            [&book_id],
        )?;
        conn.execute(
            "UPDATE library_books SET \
                overall_progress = 0, last_position = 0, last_played_at = NULL, \
                last_played_chapter_id = NULL, last_chapter_title = NULL, \
                last_chapter_number = NULL, completed_at = NULL \
             WHERE book_id = ?1",
            [&book_id],
        )?;
        Ok(())
    })
}

/* -------------------------------------------------------------------------- */
/* Rollup                                                                     */
/* -------------------------------------------------------------------------- */

/// Recalculate the cached rollup for one book: overall progress, total audio
/// duration, and the completed timestamp. `recomputeBookProgress`.
pub(crate) fn recompute_book_progress(
    conn: &Connection,
    book_id: &str,
) -> rusqlite::Result<(f64, bool)> {
    let has_library = conn
        .query_row(
            "SELECT 1 FROM library_books WHERE book_id = ?1",
            [book_id],
            |row| row.get::<_, i64>(0),
        )
        .is_ok();
    if !has_library {
        return Ok((0.0, false));
    }

    let mut total_duration = 0.0f64;
    let mut consumed = 0.0f64;
    let mut with_audio = 0i64;
    let mut completed_chapters = 0i64;
    {
        let mut statement = conn.prepare(
            "SELECT a.duration, p.percentage, p.completed \
             FROM chapters c \
             JOIN chapter_audio a ON a.chapter_id = c.id \
             LEFT JOIN listening_progress p ON p.chapter_id = c.id \
             WHERE c.book_id = ?1",
        )?;
        let mut rows = statement.query([book_id])?;
        while let Some(row) = rows.next()? {
            with_audio += 1;
            let duration: f64 = row.get::<_, f64>(0).unwrap_or(0.0);
            total_duration += duration;
            let percentage: f64 = row.get::<_, Option<f64>>(1)?.unwrap_or(0.0).clamp(0.0, 1.0);
            let completed = row.get::<_, Option<i64>>(2)?.unwrap_or(0) != 0;
            consumed += if completed { duration } else { duration * percentage };
            if completed {
                completed_chapters += 1;
            }
        }
    }

    let progress = if total_duration > 0.0 {
        (consumed / total_duration).min(1.0)
    } else {
        0.0
    };
    let is_complete = with_audio > 0 && completed_chapters == with_audio;

    conn.execute(
        "UPDATE library_books SET overall_progress = ?1, total_audio_duration = ?2, completed_at = ?3 \
         WHERE book_id = ?4",
        rusqlite::params![progress, total_duration, if is_complete { Some(repo::now_iso()) } else { None::<String> }, book_id],
    )?;

    Ok((progress, is_complete))
}

/// The completion threshold, in the 0..1 fraction the schema stores.
pub(crate) fn completion_threshold(conn: &Connection) -> f64 {
    conn.query_row(
        "SELECT completion_threshold FROM app_settings WHERE id = 'singleton'",
        [],
        |row| row.get::<_, f64>(0),
    )
    .unwrap_or(DEFAULT_COMPLETION_THRESHOLD)
}

/* -------------------------------------------------------------------------- */
/* Body validation (the Zod `saveProgressSchema` / `completeChapterSchema`)   */
/* -------------------------------------------------------------------------- */

fn object<'a>(body: &'a Value) -> ApiResult<&'a serde_json::Map<String, Value>> {
    body.as_object()
        .ok_or_else(|| ApiError::invalid("body", "Expected a JSON object."))
}

fn required_trimmed(body: &Value, key: &str, max: usize) -> ApiResult<String> {
    let value = object(body)?
        .get(key)
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

fn timestamp_field(body: &Value, key: &str) -> ApiResult<f64> {
    let value = object(body)?
        .get(key)
        .and_then(|value| value.as_f64())
        .ok_or_else(|| ApiError::invalid(key, "Invalid input: expected number."))?;
    if !(0.0..=MAX_TIMESTAMP).contains(&value) {
        return Err(ApiError::invalid(
            key,
            format!("Number must be greater than or equal to 0 and less than or equal to {MAX_TIMESTAMP}."),
        ));
    }
    Ok(value)
}

fn optional_bool(body: &Value, key: &str) -> ApiResult<bool> {
    match object(body)?.get(key) {
        None | Some(Value::Null) => Ok(false),
        Some(value) => value
            .as_bool()
            .ok_or_else(|| ApiError::invalid(key, "Invalid input: expected boolean.")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> crate::services::library::tests_support::Fixture {
        crate::services::library::tests_support::fixture()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn save_writes_progress_and_rolls_the_book_up() {
        let fixture = fixture();
        fixture.seed_book("bk1", 2, true);
        fixture.library_link("bk1", "lb1");

        let result = save(
            &fixture.db,
            &json!({ "chapterId": "chbk1-0", "position": 95.0, "duration": 100.0 }),
        )
        .expect("save");
        assert_eq!(result["completed"], true);
        assert_eq!(result["percentage"].as_f64().unwrap(), 0.95);

        // Resume point is stored on the shelf row.
        let entry = crate::services::library::get_library_entry(&fixture.db, "lb1")
            .expect("entry")
            ["libraryBook"]
            .clone();
        assert_eq!(entry["lastPlayedChapterId"], "chbk1-0");
        assert_eq!(entry["lastPosition"].as_f64().unwrap(), 95.0);
        assert_eq!(
            entry["overallProgress"].as_f64().unwrap(),
            0.5,
            "only half the book's audio is complete"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn save_ignores_tiny_positions_for_the_resume_point() {
        let fixture = fixture();
        fixture.seed_book("bk1", 1, true);
        fixture.library_link("bk1", "lb1");

        save(
            &fixture.db,
            &json!({ "chapterId": "chbk1-0", "position": 1.0, "duration": 100.0 }),
        )
        .expect("save");
        let entry = crate::services::library::get_library_entry(&fixture.db, "lb1")
            .expect("entry")["libraryBook"]
            .clone();
        assert!(entry["lastPlayedAt"].is_null());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn save_rejects_a_foreign_or_unknown_chapter() {
        let fixture = fixture();
        fixture.seed_book("bk1", 1, true);

        let error = save(
            &fixture.db,
            &json!({ "chapterId": "nope", "position": 1.0, "duration": 100.0 }),
        )
        .expect_err("unknown chapter");
        assert_eq!(error.status, axum::http::StatusCode::NOT_FOUND);
        assert_eq!(error.code, "request_failed");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn save_validates_bounds_like_zod() {
        let fixture = fixture();
        fixture.seed_book("bk1", 1, true);

        let error = save(
            &fixture.db,
            &json!({ "chapterId": "chbk1-0", "position": -1.0, "duration": 100.0 }),
        )
        .expect_err("negative position");
        assert_eq!(error.status, axum::http::StatusCode::BAD_REQUEST);

        let error = save(
            &fixture.db,
            &json!({ "chapterId": "", "position": 1.0, "duration": 100.0 }),
        )
        .expect_err("empty chapter id");
        assert_eq!(error.status, axum::http::StatusCode::BAD_REQUEST);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn set_completed_toggles_and_lists_chapters() {
        let fixture = fixture();
        fixture.seed_book("bk1", 2, true);
        fixture.library_link("bk1", "lb1");
        save(
            &fixture.db,
            &json!({ "chapterId": "chbk1-0", "position": 50.0, "duration": 100.0 }),
        )
        .expect("save");

        let result = set_completed(
            &fixture.db,
            &json!({ "chapterId": "chbk1-0", "completed": true }),
        )
        .expect("complete");
        assert_eq!(result["chapters"][0]["progress"]["completed"], true);
        assert_eq!(result["chapters"][0]["progress"]["position"].as_f64().unwrap(), 100.0);

        let entry = crate::services::library::get_library_entry(&fixture.db, "lb1")
            .expect("entry")["libraryBook"]
            .clone();
        assert_eq!(entry["overallProgress"].as_f64().unwrap(), 0.5);

        let error =
            set_completed(&fixture.db, &json!({ "chapterId": "gone", "completed": true }))
                .expect_err("unknown chapter");
        assert_eq!(error.status, axum::http::StatusCode::NOT_FOUND);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn reset_clears_everything_for_the_book() {
        let fixture = fixture();
        fixture.seed_book("bk1", 2, true);
        fixture.library_link("bk1", "lb1");
        save(
            &fixture.db,
            &json!({ "chapterId": "chbk1-0", "position": 100.0, "duration": 100.0 }),
        )
        .expect("save");

        reset(&fixture.db, &json!({ "bookId": "bk1" })).expect("reset");

        let entry = crate::services::library::get_library_entry(&fixture.db, "lb1")
            .expect("entry")["libraryBook"]
            .clone();
        assert_eq!(entry["overallProgress"].as_f64().unwrap(), 0.0);
        assert!(entry["lastPlayedAt"].is_null());
        assert!(entry["completedAt"].is_null());
        // totalAudioDuration is a rollup of uploaded audio, not progress: it stays.
        assert_eq!(entry["totalAudioDuration"].as_f64().unwrap(), 200.0);

        let error = reset(&fixture.db, &json!({ "bookId": "not-there" }))
            .expect_err("unknown book");
        assert_eq!(error.status, axum::http::StatusCode::NOT_FOUND);
    }

    #[test]
    fn completion_threshold_reads_the_singleton_row() {
        let fixture = fixture();
        let connection = fixture.raw();
        assert_eq!(completion_threshold(&connection), 0.95);
        connection
            .execute(
                "INSERT INTO app_settings (id, completion_threshold, created_at, updated_at) \
                 VALUES ('singleton', 0.8, '2026-01-01T00:00:00.000Z', '2026-01-01T00:00:00.000Z')",
                [],
            )
            .expect("insert");
        assert_eq!(completion_threshold(&connection), 0.8);
    }

    #[test]
    fn timestamp_bounds_match_the_zod_schema() {
        let body = json!({ "chapterId": "x", "position": MAX_TIMESTAMP + 1.0, "duration": 0.0 });
        assert!(timestamp_field(&body, "position").is_err());
        let body = json!({ "chapterId": "x", "position": MAX_TIMESTAMP, "duration": 0.0 });
        assert_eq!(timestamp_field(&body, "position").unwrap(), MAX_TIMESTAMP);
    }
}
