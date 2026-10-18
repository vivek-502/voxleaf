//! Resolving what to play.
//!
//! Port of `src/app/api/playback/resolve/route.ts` plus the two repository
//! helpers it leans on (`resolveResumeChapter`, `findAdjacentChapter`). One
//! request gives the player the chapter to start, the resume position, the
//! stream URL and both neighbours, so pressing Play is instant.

use serde_json::{json, Value};

use crate::db::Database;
use crate::error::{ApiError, ApiResult};

use super::library::{block_db, request_error};

/// Resumed positions get a two-second rewind so a paused listener hears the
/// sentence they were in, not the tail of the previous one.
const RESUME_REWIND: f64 = 2.0;

struct ResumeChapter {
    chapter_id: String,
    position: f64,
    chapter_number: i64,
    title: String,
}

/// GET /api/playback/resolve — everything the audio engine needs to start.
///
/// The web route also accepts a `position` override; the ported caller never
/// sends one, so the resume position always comes from stored progress.
pub fn resolve(
    db: &Database,
    book_id: Option<&str>,
    library_id: Option<&str>,
    chapter_id: Option<&str>,
) -> ApiResult<Value> {
    if book_id.is_none() && library_id.is_none() {
        return Err(request_error(400, "A bookId or libraryId is required."));
    }
    let book_id = book_id.map(|value| value.to_string());
    let library_id = library_id.map(|value| value.to_string());
    let chapter_id = chapter_id.map(|value| value.to_string());

    let (library_key, book_row, favorite, last_played_chapter_id) = block_db(db, move |conn| {
        let row = match library_id {
            Some(library_id) => conn.query_row(
                "SELECT lb.id, lb.book_id, lb.favorite, lb.last_played_chapter_id \
                 FROM library_books lb WHERE lb.id = ?1",
                [&library_id],
                map_library_row,
            ),
            None => {
                let book_id = book_id.as_deref().unwrap_or_default();
                conn.query_row(
                    "SELECT lb.id, lb.book_id, lb.favorite, lb.last_played_chapter_id \
                     FROM library_books lb WHERE lb.book_id = ?1 \
                     ORDER BY lb.added_at ASC LIMIT 1",
                    [book_id],
                    map_library_row,
                )
            }
        };
        row.map_err(|_| ApiError::not_found("That book is not in your library."))
    })?;

    let (book_title, author, cover_url) = {
        let lookup = book_row.clone();
        block_db(db, move |conn| {
            conn.query_row(
                "SELECT title, author, cover_url FROM books WHERE id = ?1",
                [&lookup],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ))
                },
            )
        })
        .map_err(|_| ApiError::not_found("That book no longer exists."))?
    };

    let preferred = chapter_id.clone().or(last_played_chapter_id);
    let resume: Option<ResumeChapter> = {
        let lookup = book_row.clone();
        block_db(db, move |conn| resolve_resume_chapter(conn, &lookup, preferred.as_deref()))?
    };
    let Some(resume) = resume else {
        return Ok(json!({
            "playable": false,
            "reason": "no_audio",
            "bookId": book_row,
            "libraryId": library_key,
        }));
    };

    let base = json!({
        "bookId": book_row,
        "libraryId": library_key,
        "bookTitle": book_title,
        "author": author,
        "coverUrl": cover_url,
        "favorite": favorite,
    });

    // An explicit chapter request is honoured even when it has no audio: the
    // player shows the "needs audio" state rather than silently jumping.
    if let Some(requested) = chapter_id {
        let detail = {
            let lookup = requested.clone();
            let book = book_row.clone();
            block_db(db, move |conn| {
                let (title, chapter_number, audio_id, duration, progress_position, progress_completed) =
                    conn.query_row(
                        "SELECT c.title, c.chapter_number, a.id, a.duration, \
                                p.position, p.completed \
                         FROM chapters c \
                         LEFT JOIN chapter_audio a ON a.chapter_id = c.id \
                         LEFT JOIN listening_progress p ON p.chapter_id = c.id \
                         WHERE c.id = ?1 AND c.book_id = ?2",
                        rusqlite::params![lookup, book],
                        |row| {
                            Ok((
                                row.get::<_, String>(0)?,
                                row.get::<_, i64>(1)?,
                                row.get::<_, Option<String>>(2)?,
                                row.get::<_, Option<f64>>(3)?.unwrap_or(0.0),
                                row.get::<_, Option<f64>>(4)?,
                                row.get::<_, Option<i64>>(5)?.unwrap_or(0) != 0,
                            ))
                        },
                    )
                    .map_err(|_| ApiError::not_found("That chapter no longer exists."))?;
                Ok::<(String, i64, Option<String>, f64, Option<f64>, bool), ApiError>((
                    title,
                    chapter_number,
                    audio_id,
                    duration,
                    progress_position,
                    progress_completed,
                ))
            })?
        };
        let (title, chapter_number, audio_id, duration, progress_position, progress_completed) = detail;

        let Some(audio_id) = audio_id else {
            return Ok(json!({
                "playable": false,
                "reason": "no_audio",
                "bookId": book_row,
                "libraryId": library_key,
                "chapter": { "id": requested, "chapterNumber": chapter_number, "title": title },
            }));
        };

        let (next, previous) = adjacent_chapters(db, &book_row, chapter_number)?;
        let start_position = match progress_position {
            Some(position) if !progress_completed => position.max(0.0) - RESUME_REWIND,
            _ => 0.0,
        };
        return Ok(merge(base, json!({
            "playable": true,
            "chapterId": requested,
            "chapterNumber": chapter_number,
            "chapterTitle": title,
            "audioId": audio_id,
            "duration": duration,
            "streamUrl": format!("/api/audio/{audio_id}"),
            "startPosition": start_position.max(0.0),
            "next": next,
            "previous": previous,
        })));
    }

    let audio = {
        let lookup = resume.chapter_id.clone();
        block_db(db, move |conn| {
            conn.query_row(
                "SELECT id, duration FROM chapter_audio WHERE chapter_id = ?1",
                [&lookup],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?)),
            )
            .map(Some)
            .or_else(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(other),
            })
        })?
    };
    let Some((audio_id, duration)) = audio else {
        return Ok(json!({
            "playable": false,
            "reason": "no_audio",
            "bookId": book_row,
            "libraryId": library_key,
        }));
    };

    let (next, previous) = adjacent_chapters(db, &book_row, resume.chapter_number)?;
    Ok(merge(base, json!({
        "playable": true,
        "chapterId": resume.chapter_id,
        "chapterNumber": resume.chapter_number,
        "chapterTitle": resume.title,
        "audioId": audio_id,
        "duration": duration,
        "streamUrl": format!("/api/audio/{audio_id}"),
        "startPosition": resume.position.max(0.0),
        "next": next,
        "previous": previous,
    })))
}

/* -------------------------------------------------------------------------- */
/* Resume & neighbours                                                        */
/* -------------------------------------------------------------------------- */

fn map_library_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<(String, String, bool, Option<String>)> {
    Ok((
        row.get::<_, String>(0)?,
        row.get::<_, String>(1)?,
        row.get::<_, i64>(2)? != 0,
        row.get::<_, Option<String>>(3)?,
    ))
}

/// `resolveResumeChapter`: the preferred chapter when it has audio, otherwise
/// the first unfinished chapter with audio, otherwise the last one with audio.
fn resolve_resume_chapter(
    conn: &rusqlite::Connection,
    book_id: &str,
    preferred_chapter_id: Option<&str>,
) -> ApiResult<Option<ResumeChapter>> {
    let mut statement = conn.prepare(
        "SELECT c.id, c.chapter_number, c.title, a.id, p.position, p.completed \
         FROM chapters c \
         LEFT JOIN chapter_audio a ON a.chapter_id = c.id \
         LEFT JOIN listening_progress p ON p.chapter_id = c.id \
         WHERE c.book_id = ?1 ORDER BY c.chapter_number ASC",
    )?;
    let rows = statement.query_map([book_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, Option<f64>>(4)?,
            row.get::<_, Option<i64>>(5)?.unwrap_or(0) != 0,
        ))
    })?;

    let mut chapters: Vec<(String, i64, String, Option<String>, Option<f64>, bool)> =
        Vec::new();
    for row in rows {
        chapters.push(row?);
    }
    if chapters.is_empty() {
        return Ok(None);
    }

    let with_audio: Vec<&(String, i64, String, Option<String>, Option<f64>, bool)> =
        chapters.iter().filter(|chapter| chapter.3.is_some()).collect();
    if with_audio.is_empty() {
        return Ok(None);
    }

    let target = preferred_chapter_id.and_then(|wanted| {
        chapters.iter().find(|chapter| chapter.0 == wanted)
    });
    let fallback = with_audio
        .iter()
        .find(|chapter| !chapter.5)
        .copied()
        .unwrap_or(with_audio[with_audio.len() - 1]);
    let chosen = match target {
        Some(target) if target.3.is_some() => target,
        _ => fallback,
    };

    let position = match chosen.4 {
        Some(position) if !chosen.5 => (position - RESUME_REWIND).max(0.0),
        _ => 0.0,
    };
    Ok(Some(ResumeChapter {
        chapter_id: chosen.0.clone(),
        position,
        chapter_number: chosen.1,
        title: chosen.2.clone(),
    }))
}

/// `findAdjacentChapter` for both directions in one round trip.
fn adjacent_chapters(
    db: &Database,
    book_id: &str,
    chapter_number: i64,
) -> ApiResult<(Option<Value>, Option<Value>)> {
    let book_id = book_id.to_string();
    block_db(db, move |conn| {
        let mut statement = conn.prepare(
            "SELECT c.id, c.chapter_number, c.title, a.id \
             FROM chapters c LEFT JOIN chapter_audio a ON a.chapter_id = c.id \
             WHERE c.book_id = ?1 ORDER BY c.chapter_number ASC",
        )?;
        let rows = statement.query_map([book_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })?;
        let chapters: Vec<(String, i64, String, Option<String>)> =
            rows.collect::<Result<_, _>>()?;

        let index = chapters.iter().position(|chapter| chapter.1 == chapter_number);
        let neighbour = |index: Option<usize>| -> Option<Value> {
            let chapter = index.and_then(|index| chapters.get(index))?;
            Some(json!({
                "id": chapter.0,
                "chapterNumber": chapter.1,
                "hasAudio": chapter.3.is_some(),
                "title": chapter.2,
            }))
        };
        let next = index.and_then(|index| index.checked_add(1));
        let previous = index.and_then(|index| index.checked_sub(1));
        Ok((neighbour(next), neighbour(previous)))
    })
}

fn merge(base: Value, extra: Value) -> Value {
    let mut merged = base;
    if let (Some(object), Some(extra)) = (merged.as_object_mut(), extra.as_object()) {
        for (key, value) in extra {
            object.insert(key.clone(), value.clone());
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::library::tests_support::fixture;

    #[tokio::test(flavor = "multi_thread")]
    async fn requires_a_book_or_library_id() {
        let fixture = fixture();
        let error = resolve(&fixture.db, None, None, None).expect_err("no id");
        assert_eq!(error.status, axum::http::StatusCode::BAD_REQUEST);
        assert_eq!(error.message, "A bookId or libraryId is required.");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn answers_not_in_the_library_for_a_stranger() {
        let fixture = fixture();
        fixture.seed_book("bk1", 1, true);

        let error = resolve(&fixture.db, Some("bk1"), None, None).expect_err("not in library");
        assert_eq!(error.message, "That book is not in your library.");

        fixture.library_link("bk1", "lb1");
        let resolved = resolve(&fixture.db, None, Some("lb1"), None).expect("resolve");
        assert_eq!(resolved["playable"], true);
        assert_eq!(resolved["chapterId"], "chbk1-0");
        assert_eq!(resolved["startPosition"].as_f64().unwrap(), 0.0);
        assert_eq!(resolved["streamUrl"], "/api/audio/aubk1-0");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn resumes_the_first_unfinished_chapter_with_a_rewind() {
        let fixture = fixture();
        fixture.seed_book("bk1", 2, true);
        fixture.library_link("bk1", "lb1");

        // Chapter 1 finished, chapter 2 half played.
        crate::services::progress::set_completed(
            &fixture.db,
            &json!({ "chapterId": "chbk1-0", "completed": true }),
        )
        .expect("complete");
        crate::services::progress::save(
            &fixture.db,
            &json!({ "chapterId": "chbk1-1", "position": 50.0, "duration": 100.0 }),
        )
        .expect("save");

        let resolved = resolve(&fixture.db, Some("bk1"), None, None).expect("resolve");
        assert_eq!(resolved["chapterId"], "chbk1-1");
        assert_eq!(
            resolved["startPosition"].as_f64().unwrap(),
            48.0,
            "half a minute in, two seconds back"
        );
        assert_eq!(resolved["previous"]["id"], "chbk1-0");
        assert_eq!(resolved["next"], Value::Null);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn explicit_chapters_are_honoured_even_without_audio() {
        let fixture = fixture();
        fixture.seed_book("bk1", 2, false);
        fixture.library_link("bk1", "lb1");
        // Only chapter 1 carries audio — resume resolution therefore has a
        // candidate, and the audioless chapter 2 is the explicit request that
        // must still be honoured.
        fixture
            .raw()
            .execute(
                "INSERT INTO chapter_audio \
                    (id, chapter_id, file_url, storage_key, duration, mime_type, uploaded_at) \
                 VALUES ('au-first', 'chbk1-0', 'file.mp3', 'sk-first', 100.0, 'audio/mpeg', '2026-01-01T00:00:00.000Z')",
                [],
            )
            .expect("audio");

        let resolved = resolve(&fixture.db, Some("bk1"), None, Some("chbk1-1")).expect("explicit");
        assert_eq!(resolved["playable"], false);
        assert_eq!(resolved["reason"], "no_audio");
        assert_eq!(resolved["chapter"]["chapterNumber"], 2);
        assert_eq!(resolved["chapter"]["title"], "Chapter 2");

        let error =
            resolve(&fixture.db, Some("bk1"), None, Some("missing")).expect_err("missing");
        assert_eq!(error.message, "That chapter no longer exists.");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn adjacent_chapters_skip_nothing_but_report_audio() {
        let fixture = fixture();
        fixture.seed_book("bk1", 3, false);
        fixture.library_link("bk1", "lb1");
        // Only the middle chapter has audio.
        fixture
            .raw()
            .execute(
                "INSERT INTO chapter_audio \
                    (id, chapter_id, file_url, storage_key, duration, mime_type, uploaded_at) \
                 VALUES ('au-mid', 'chbk1-1', 'file.mp3', 'sk-mid', 100.0, 'audio/mpeg', '2026-01-01T00:00:00.000Z')",
                [],
            )
            .expect("audio");

        let resolved =
            resolve(&fixture.db, Some("bk1"), None, Some("chbk1-1")).expect("explicit");
        assert_eq!(resolved["playable"], true);
        assert_eq!(resolved["next"]["hasAudio"], false);
        assert_eq!(resolved["previous"]["hasAudio"], false);
        assert_eq!(resolved["previous"]["chapterNumber"], 1);
    }
}
