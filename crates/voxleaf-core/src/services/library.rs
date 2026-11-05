//! The user's shelf.
//!
//! Filtering, sorting, pagination, chapter bookkeeping and the add/remove
//! flows, ported from the web app's `src/server/db/repository.ts` and
//! `src/app/api/library/*` so the wire JSON stays identical.

use rusqlite::Connection;
use serde_json::{json, to_value, Value};

use crate::db::Database;
use crate::error::{ApiError, ApiResult};
use crate::models::{BookSummary, ChapterInfo, LibraryBookInfo};
use crate::repo;
use crate::services::Services;

/// Store filters, exactly `LIBRARY_FILTERS` from `src/utils/constants.ts`.
pub const LIBRARY_FILTERS: &[&str] = &[
    "all",
    "in-progress",
    "completed",
    "not-started",
    "has-audio",
    "needs-audio",
];

/// Library sorts, exactly `LIBRARY_SORTS` from `src/utils/constants.ts`.
pub const LIBRARY_SORTS: &[&str] = &[
    "recently-added",
    "recently-listened",
    "title",
    "author",
    "progress",
];

/// Longest chapter title we accept (`MAX_CHAPTER_TITLE` in the web app).
const MAX_CHAPTER_TITLE: usize = 160;
/// Ceiling on chapters per book (`MAX_CHAPTERS_PER_BOOK` in the web app).
const MAX_CHAPTERS_PER_BOOK: i64 = 1000;
/// Bounds every client-supplied timestamp, as the Zod schemas did.
pub(crate) const MAX_TIMESTAMP: f64 = 60.0 * 60.0 * 24.0 * 30.0;

/* -------------------------------------------------------------------------- */
/* Sync-over-async bridge                                                     */
/* -------------------------------------------------------------------------- */

/// Run a synchronous service body against the database.
///
/// The HTTP handlers call these services synchronously while `Database`
/// drives its queries from `spawn_blocking`; `block_in_place` parks the
/// current worker so the runtime can drive that blocking task to completion
/// (the handlers run on tokio's multi-thread runtime — the one Tauri's async
/// runtime hands the server).
pub(crate) fn block_db<T, E>(
    db: &Database,
    work: impl FnOnce(&Connection) -> Result<T, E> + Send + 'static,
) -> Result<T, E>
where
    T: Send + 'static,
    E: Send + 'static,
{
    tokio::task::block_in_place(|| tokio::runtime::Handle::current().block_on(db.try_call(work)))
}

/// Error for the raw (non-Zod) `Object.assign(new Error(...), { status })`
/// throws in the web app, which `toErrorResponse` answered as
/// `code: 'request_failed'` with the thrown status.
pub(crate) fn request_error(status: u16, message: impl Into<String>) -> ApiError {
    ApiError::new(
        axum::http::StatusCode::from_u16(status).unwrap_or(axum::http::StatusCode::BAD_REQUEST),
        "request_failed",
        message,
    )
}

/* -------------------------------------------------------------------------- */
/* Text sanitisation (`src/utils/sanitize.ts`)                                */
/* -------------------------------------------------------------------------- */

/// Remove C0/C1 control chars (keep tab/newline), bidi overrides and
/// zero-widths, exactly as the web app's `CONTROL_CHARS` regex did.
fn strip_control_chars(input: &str) -> String {
    input
        .chars()
        .filter(|ch| {
            let cp = *ch as u32;
            !matches!(
                cp,
                0x00..=0x08
                    | 0x0B
                    | 0x0C
                    | 0x0E..=0x1F
                    | 0x7F..=0x9F
                    | 0x202A..=0x202E
                    | 0x2066..=0x2069
                    | 0x200B..=0x200D
                    | 0xFEFF
            )
        })
        .collect()
}

/// Normalise free text: strip control characters, collapse whitespace runs
/// and clamp to `max_chars`. `sanitizeText` in the web app.
pub(crate) fn sanitize_text(input: &str, max_chars: usize) -> String {
    let value = strip_control_chars(input);
    let mut out = String::with_capacity(value.len());
    let mut in_space = false;
    for ch in value.chars() {
        let ch = if matches!(ch, '\r' | '\n' | '\t') { ' ' } else { ch };
        if ch == ' ' {
            if in_space {
                continue;
            }
            in_space = true;
        } else {
            in_space = false;
        }
        out.push(ch);
    }
    out.trim().chars().take(max_chars).collect()
}

/// Like [`sanitize_text`] but keeps paragraph breaks. `sanitizeMultiline`.
pub(crate) fn sanitize_multiline(input: &str, max_chars: usize) -> String {
    let value = strip_control_chars(input).replace("\r\n", "\n");
    let mut out = String::with_capacity(value.len());
    let mut newlines = 0usize;
    for ch in value.chars() {
        if ch == '\n' {
            newlines += 1;
            if newlines > 2 {
                continue;
            }
        } else {
            newlines = 0;
        }
        out.push(ch);
    }
    out.trim().chars().take(max_chars).collect()
}

/// Collapse every whitespace run into one space (`\s+` → `' '`).
pub(crate) fn collapse_whitespace(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

/* -------------------------------------------------------------------------- */
/* Shared row loaders                                                         */
/* -------------------------------------------------------------------------- */

const CHAPTER_SELECT: &str =
    "SELECT c.id, c.book_id, c.chapter_number, c.title, c.sort_index, \
     c.created_at, c.updated_at, \
     (SELECT COUNT(*) FROM bookmarks bm WHERE bm.chapter_id = c.id) AS bookmark_count, \
     a.id AS audio_id, a.chapter_id, a.duration AS audio_duration, a.file_size, \
     a.mime_type, a.original_name, a.uploaded_at, \
     p.position AS progress_position, p.duration AS progress_duration, p.percentage, \
     p.completed, p.play_count, p.updated_at AS progress_updated_at \
     FROM chapters c \
     LEFT JOIN chapter_audio a ON a.chapter_id = c.id \
     LEFT JOIN listening_progress p ON p.chapter_id = c.id";

/// Every chapter of a book, with audio and progress, in reading order.
pub(crate) fn load_chapter_infos(conn: &Connection, book_id: &str) -> ApiResult<Vec<ChapterInfo>> {
    let sql = format!("{CHAPTER_SELECT} WHERE c.book_id = ?1 ORDER BY c.chapter_number ASC, c.sort_index ASC");
    let mut statement = conn.prepare(&sql)?;
    let mut rows = statement.query([book_id])?;
    let mut chapters: Vec<ChapterInfo> = Vec::new();
    while let Some(row) = rows.next()? {
        chapters.push(repo::map_chapter(row, "")?);
    }
    Ok(chapters)
}

/// A single chapter, with its audio, progress and bookmark count.
pub(crate) fn load_chapter_info(
    conn: &Connection,
    chapter_id: &str,
) -> ApiResult<Option<ChapterInfo>> {
    let sql = format!("{CHAPTER_SELECT} WHERE c.id = ?1");
    let mut statement = conn.prepare(&sql)?;
    let mut rows = statement.query([chapter_id])?;
    match rows.next()? {
        Some(row) => Ok(Some(repo::map_chapter(row, "")?)),
        None => Ok(None),
    }
}

/// A book summary carrying its library state (the `libraryStateInclude` join).
pub(crate) fn load_book_summary(conn: &Connection, book_id: &str) -> ApiResult<Option<BookSummary>> {
    let sql = format!(
        "SELECT {} {} WHERE b.id = ?1",
        repo::BOOK_SUMMARY_COLUMNS,
        repo::BOOK_FROM
    );
    let mut statement = conn.prepare(&sql)?;
    let mut rows = statement.query_map([book_id], repo::map_book_summary)?;
    match rows.next() {
        Some(row) => Ok(Some(row.map_err(ApiError::from)?)),
        None => Ok(None),
    }
}

/// The same join, keyed by the provider id (`Book.externalId`).
pub(crate) fn load_book_summary_by_external(
    conn: &Connection,
    external_id: &str,
) -> ApiResult<Option<BookSummary>> {
    let sql = format!(
        "SELECT {} {} WHERE b.external_id = ?1",
        repo::BOOK_SUMMARY_COLUMNS,
        repo::BOOK_FROM
    );
    let mut statement = conn.prepare(&sql)?;
    let mut rows = statement.query_map([external_id], repo::map_book_summary)?;
    match rows.next() {
        Some(row) => Ok(Some(row.map_err(ApiError::from)?)),
        None => Ok(None),
    }
}

/// Bookmarks stored for a book, newest first (`listBookmarksForBook`).
pub(crate) fn load_bookmarks(conn: &Connection, book_id: &str) -> ApiResult<Vec<Value>> {
    let sql = format!(
        "SELECT {} {} WHERE bm.book_id = ?1 ORDER BY bm.created_at DESC",
        repo::BOOKMARK_COLUMNS,
        repo::BOOKMARK_FROM
    );
    let mut statement = conn.prepare(&sql)?;
    let rows = statement.query_map([book_id], repo::map_bookmark)?;
    rows.map(|row| {
        row.map(|bookmark| to_value(bookmark).unwrap_or(Value::Null))
            .map_err(ApiError::from)
    })
    .collect()
}

struct LibraryLink {
    book_id: String,
    added_at: String,
    favorite: bool,
    last_played_chapter_id: Option<String>,
    last_played_at: Option<String>,
    last_position: f64,
    last_chapter_title: Option<String>,
    last_chapter_number: Option<i64>,
}

fn load_link(conn: &Connection, library_id: &str) -> ApiResult<LibraryLink> {
    conn.query_row(
        "SELECT book_id, added_at, favorite, last_played_chapter_id, last_played_at, \
         last_position, last_chapter_title, last_chapter_number \
         FROM library_books WHERE id = ?1",
        [library_id],
        |row| {
            Ok(LibraryLink {
                book_id: row.get(0)?,
                added_at: row.get(1)?,
                favorite: row.get::<_, i64>(2)? != 0,
                last_played_chapter_id: row.get(3)?,
                last_played_at: row.get(4)?,
                last_position: row.get(5)?,
                last_chapter_title: row.get(6)?,
                last_chapter_number: row.get(7)?,
            })
        },
    )
    .map_err(|_| ApiError::not_found("That book is not in your library."))
}

/// The `mapLibraryBook` port: roll the chapters up into the DTO the shelf,
/// home and continue-listening views read.
fn build_library_book(conn: &Connection, library_id: &str) -> ApiResult<LibraryBookInfo> {
    let link = load_link(conn, library_id)?;
    let book = load_book_summary(conn, &link.book_id)?
        .ok_or_else(|| ApiError::not_found("That book no longer exists."))?;
    let chapters = load_chapter_infos(conn, &link.book_id)?;

    let with_audio: Vec<&ChapterInfo> = chapters.iter().filter(|c| c.audio.is_some()).collect();
    let completed_chapters = chapters
        .iter()
        .filter(|c| c.progress.as_ref().map(|p| p.completed).unwrap_or(false))
        .count();

    let total_audio_duration: f64 = with_audio
        .iter()
        .map(|c| c.audio.as_ref().map(|a| a.duration).unwrap_or(0.0))
        .sum();

    // Rollup: the share of *playable* audio the user has consumed. A book with
    // no uploaded audio reads as 0% rather than NaN.
    let consumed: f64 = with_audio
        .iter()
        .map(|c| {
            let fraction = if c.progress.as_ref().map(|p| p.completed).unwrap_or(false) {
                1.0
            } else {
                c.progress
                    .as_ref()
                    .map(|p| p.percentage.clamp(0.0, 1.0))
                    .unwrap_or(0.0)
            };
            c.audio.as_ref().map(|a| a.duration).unwrap_or(0.0) * fraction
        })
        .sum();
    let rollup = if total_audio_duration > 0.0 {
        consumed / total_audio_duration
    } else {
        0.0
    };
    let overall_progress = rollup.clamp(0.0, 1.0);
    let is_complete = !with_audio.is_empty()
        && completed_chapters > 0
        && completed_chapters == with_audio.len();

    Ok(LibraryBookInfo {
        library_id: library_id.to_string(),
        book_id: link.book_id,
        added_at: link.added_at,
        favorite: link.favorite,
        overall_progress,
        completed: is_complete,
        last_played_at: link.last_played_at,
        last_played_chapter_id: link.last_played_chapter_id,
        last_chapter_title: link.last_chapter_title,
        last_chapter_number: link.last_chapter_number,
        last_position: repo::safe_number(Some(link.last_position), 0.0, f64::MAX, 0.0),
        total_audio_duration,
        chapter_count: chapters.len() as i64,
        chapters_with_audio: with_audio.len() as i64,
        book,
    })
}

/// A library entry by id, or the web app's 404.
pub(crate) fn load_library_entry(conn: &Connection, library_id: &str) -> ApiResult<LibraryBookInfo> {
    build_library_book(conn, library_id)
}

/// One page of library entries for a raw `WHERE`/`ORDER BY` pair — the shape
/// the home aggregation needs (`listContinueListening` & friends).
pub(crate) fn entries_where(
    conn: &Connection,
    filter_sql: &str,
    order_sql: &str,
    limit: i64,
) -> ApiResult<Vec<LibraryBookInfo>> {
    let where_sql = if filter_sql.is_empty() {
        String::new()
    } else {
        format!("WHERE {filter_sql}")
    };
    let sql = format!(
        "SELECT lb.id FROM library_books lb {where_sql} ORDER BY {order_sql} LIMIT {limit}"
    );
    let mut statement = conn.prepare(&sql)?;
    let ids = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(ApiError::from)?;
    ids.iter().map(|id| load_library_entry(conn, id)).collect()
}

/// The shelf-wide counters behind the library header (`libraryStats`).
pub(crate) fn library_stats(conn: &Connection) -> ApiResult<Value> {
    let count = |sql: &str| -> ApiResult<i64> {
        conn.query_row(sql, [], |row| row.get(0)).map_err(ApiError::from)
    };
    let book_count = count("SELECT COUNT(*) FROM library_books")?;
    let chapter_count = count(
        "SELECT COUNT(*) FROM chapters c JOIN library_books lb ON lb.book_id = c.book_id",
    )?;
    let chapters_with_audio = count(
        "SELECT COUNT(*) FROM chapter_audio a \
         JOIN chapters c ON c.id = a.chapter_id \
         JOIN library_books lb ON lb.book_id = c.book_id",
    )?;
    let total_audio_duration: f64 = conn
        .query_row("SELECT COALESCE(SUM(duration), 0) FROM chapter_audio", [], |row| {
            row.get(0)
        })
        .map_err(ApiError::from)?;
    let bookmark_count = count("SELECT COUNT(*) FROM bookmarks")?;
    let completed_count = count("SELECT COUNT(*) FROM library_books WHERE completed_at IS NOT NULL")?;

    Ok(json!({
        "bookCount": book_count,
        "chapterCount": chapter_count,
        "chaptersWithAudio": chapters_with_audio,
        // `totalAudioDuration` is the web contract. `totalAudioSeconds` is an
        // additive alias for it: the shared dashboard component reads the
        // alias while the web server only ever sends the contract key, so the
        // "… of audio" line renders 0:00 in the web app itself. Serving both
        // keeps every web-exact consumer working and makes the line work here.
        "totalAudioDuration": total_audio_duration,
        "totalAudioSeconds": total_audio_duration,
        "bookmarkCount": bookmark_count,
        "completedCount": completed_count,
    }))
}

/* -------------------------------------------------------------------------- */
/* Library listing                                                            */
/* -------------------------------------------------------------------------- */

pub fn list_library(
    db: &Database,
    filter: &str,
    sort: &str,
    page: u32,
    page_size: u32,
    query: &str,
) -> ApiResult<(Vec<Value>, i64, Value)> {
    let filter = filter.to_string();
    let sort = sort.to_string();
    let query = query.to_string();

    block_db(db, move |conn| {
        let page = (page.max(1)) as i64;
        let page_size = page_size.clamp(1, 100) as i64;
        let term = sanitize_text(&query, 120);

        let mut wheres: Vec<String> = Vec::new();
        match filter.as_str() {
            "in-progress" => {
                wheres.push("lb.overall_progress > 0.001 AND lb.overall_progress < 0.999".into())
            }
            "completed" => wheres.push("lb.completed_at IS NOT NULL".into()),
            "not-started" => wheres.push("lb.overall_progress <= 0.001".into()),
            "has-audio" => wheres.push("lb.total_audio_duration > 0".into()),
            "needs-audio" => wheres.push("lb.total_audio_duration <= 0".into()),
            _ => {}
        }
        if !term.is_empty() {
            wheres.push("(b.title LIKE ? OR b.author LIKE ?)".into());
        }
        let where_sql = if wheres.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", wheres.join(" AND "))
        };
        let order_sql = match sort.as_str() {
            "title" => "b.title ASC",
            "author" => "b.author ASC",
            "progress" => "lb.overall_progress DESC",
            "recently-listened" => "lb.last_played_at DESC, lb.added_at DESC",
            _ => "lb.added_at DESC",
        };

        let pattern = format!("%{term}%");
        let total: i64 = {
            let sql = format!(
                "SELECT COUNT(*) FROM library_books lb JOIN books b ON b.id = lb.book_id {where_sql}"
            );
            let mut statement = conn.prepare(&sql)?;
            if term.is_empty() {
                statement.query_row([], |row| row.get(0))?
            } else {
                statement.query_row(rusqlite::params![pattern, pattern], |row| row.get(0))?
            }
        };

        let ids: Vec<String> = {
            let sql = format!(
                "SELECT lb.id FROM library_books lb JOIN books b ON b.id = lb.book_id \
                 {where_sql} ORDER BY {order_sql} LIMIT ? OFFSET ?"
            );
            let mut statement = conn.prepare(&sql)?;
            let offset = (page - 1) * page_size;
            let mut rows = if term.is_empty() {
                statement.query(rusqlite::params![page_size, offset])?
            } else {
                statement.query(rusqlite::params![pattern, pattern, page_size, offset])?
            };
            let mut ids: Vec<String> = Vec::new();
            while let Some(row) = rows.next()? {
                ids.push(row.get(0)?);
            }
            ids
        };

        let items = ids
            .iter()
            .map(|id| {
                load_library_entry(conn, id).map(|entry| to_value(entry).unwrap_or(Value::Null))
            })
            .collect::<ApiResult<Vec<_>>>()?;
        let stats = library_stats(conn)?;
        Ok((items, total, stats))
    })
}

/* -------------------------------------------------------------------------- */
/* Adding & removing                                                          */
/* -------------------------------------------------------------------------- */

pub async fn add_to_library(
    db: &Database,
    services: &Services,
    book_id: Option<&str>,
    external_id: Option<&str>,
    favorite: bool,
) -> ApiResult<Value> {
    let mut resolved = book_id.map(|s| s.to_string());

    if resolved.is_none() {
        if let Some(external) = external_id.map(|s| s.to_string()) {
            let existing: Option<String> = {
                let lookup = external.clone();
                block_db(db, move |conn| {
                    conn.query_row("SELECT id FROM books WHERE external_id = ?1", [&lookup], |row| {
                        row.get(0)
                    })
                    .map(Some)
                    .or_else(|error| match error {
                        rusqlite::Error::QueryReturnedNoRows => Ok(None),
                        other => Err(other),
                    })
                })?
            };

            resolved = Some(match existing {
                Some(id) => id,
                None => {
                    // Not mirrored yet: hydrate from the provider, exactly as
                    // the web route did before inserting.
                    let provider_book = services
                        .catalogue
                        .get_book(&external)
                        .await?
                        .ok_or_else(|| ApiError::not_found("We could not find that book."))?;
                    services.catalogue.upsert_catalog_book(db, &provider_book)?
                }
            });
        }
    }

    let book_id =
        resolved.ok_or_else(|| ApiError::invalid("bookId", "A bookId or externalId is required."))?;

    let book_row = {
        let lookup = book_id.clone();
        block_db(db, move |conn| {
            conn.query_row("SELECT id FROM books WHERE id = ?1", [&lookup], |row| {
                row.get::<_, String>(0)
            })
        })
        .map_err(|_| ApiError::not_found("That book is no longer in the catalogue."))?
    };

    let epub_flag = if crate::config::get()
        .map(|cfg| cfg.epub.enabled)
        .unwrap_or(true)
    {
        "pending"
    } else {
        "disabled"
    };

    block_db(db, move |conn| {
        let existing: Option<String> = conn
            .query_row("SELECT id FROM library_books WHERE book_id = ?1", [&book_row], |row| {
                row.get(0)
            })
            .map(Some)
            .or_else(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(other),
            })?;

        if let Some(library_id) = existing {
            let entry = load_library_entry(conn, &library_id)?;
            return Ok(json!({
                "libraryBook": entry,
                "seededChapters": 0,
                "epub": epub_flag,
            }));
        }

        let library_id = repo::new_id("lb");
        let now = repo::now_iso();
        conn.execute(
            "INSERT INTO library_books (id, book_id, added_at, favorite) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![library_id, book_row, now, favorite as i64],
        )?;

        // Chapter data is provider-dependent. Open Library exposes none, so a
        // new library book starts from a single placeholder so the chapter
        // list is never empty on first open; an EPUB import fills it in.
        let chapter_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM chapters WHERE book_id = ?1",
            [&book_row],
            |row| row.get(0),
        )?;
        let seeded = if chapter_count == 0 {
            let chapter_id = repo::new_id("ch");
            conn.execute(
                "INSERT INTO chapters (id, book_id, chapter_number, title, sort_index, created_at, updated_at) \
                 VALUES (?1, ?2, 1, 'Chapter 1', 0, ?3, ?3)",
                rusqlite::params![chapter_id, book_row, now],
            )?;
            1
        } else {
            0
        };

        let entry = load_library_entry(conn, &library_id)?;
        Ok(json!({
            "libraryBook": entry,
            "seededChapters": seeded,
            "epub": epub_flag,
        }))
    })
}

pub fn get_library_entry(db: &Database, id: &str) -> ApiResult<Value> {
    let id = id.to_string();
    block_db(db, move |conn| {
        let entry = load_library_entry(conn, &id)?;
        let chapters = load_chapter_infos(conn, &entry.book_id)?;
        let bookmarks = load_bookmarks(conn, &entry.book_id)?;
        Ok(json!({
            "libraryBook": to_value(&entry).unwrap_or(Value::Null),
            "chapters": chapters,
            "bookmarks": bookmarks,
        }))
    })
}

pub fn update_library_by_id(db: &Database, id: &str, favorite: Option<bool>) -> ApiResult<Value> {
    let id = id.to_string();
    block_db(db, move |conn| {
        let lib_id = if load_library_entry(conn, &id).is_ok() {
            id.clone()
        } else if let Ok(lb) = conn.query_row(
            "SELECT id FROM library_books WHERE book_id = ?1",
            [&id],
            |row| row.get::<_, String>(0),
        ) {
            lb
        } else {
            id.clone()
        };
        update_library_core(conn, &lib_id, favorite)
    })
}

pub fn update_library(db: &Database, id: &str, favorite: Option<bool>) -> ApiResult<Value> {
    let id = id.to_string();
    block_db(db, move |conn| update_library_core(conn, &id, favorite))
}

fn update_library_core(conn: &Connection, id: &str, favorite: Option<bool>) -> ApiResult<Value> {
    let favorite = favorite.ok_or_else(|| request_error(400, "Nothing to update."))?;
    repo::require_library_entry(conn, id)?;
    conn.execute(
        "UPDATE library_books SET favorite = ?1 WHERE id = ?2",
        rusqlite::params![favorite as i64, id],
    )?;
    Ok(json!({ "favorite": favorite }))
}

/// `DELETE /api/library` — clear all library data.
///
/// The settings page promises "Every book, chapter, audio file, bookmark and
/// listening progress will be permanently deleted", so unlike
/// `remove_from_library` — which keeps chapters, progress and bookmarks with
/// the catalogue book — this wipes user content outright. Deleting the books
/// takes the dependent rows with it (every foreign key in the schema
/// cascades), but stored audio is not owned by a database row, so its keys
/// are collected first and the files removed explicitly, the same way a
/// single removal does it.
///
/// Settings are deliberately kept: the dialog names books, chapters, audio,
/// bookmarks and progress, and making someone re-pick a voice and a theme
/// after a library reset would be spite rather than safety.
pub fn clear_library(db: &Database) -> ApiResult<()> {
    let (audio_keys, book_ids) = block_db(db, move |conn| {
        let audio_keys = conn
            .prepare("SELECT storage_key FROM chapter_audio")?
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        let book_ids = conn
            .prepare("SELECT id FROM books")?
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok::<_, ApiError>((audio_keys, book_ids))
    })?;

    block_db(db, move |conn| {
        conn.execute("DELETE FROM books", [])?;
        Ok::<_, ApiError>(())
    })?;

    match crate::config::get() {
        Ok(config) => {
            for key in audio_keys {
                if let Err(error) = config.storage.delete(&key) {
                    tracing::error!(%error, "could not delete an audio object during a library wipe");
                }
            }
            for book_id in book_ids {
                let path = config.epub_dir.join(format!("{book_id}.epub"));
                if let Err(error) = std::fs::remove_file(&path) {
                    if error.kind() != std::io::ErrorKind::NotFound {
                        tracing::error!(%error, "could not delete a stored EPUB during a library wipe");
                    }
                }
            }
        }
        Err(_) => tracing::warn!("library wiped without storage cleanup: no configuration"),
    }

    Ok(())
}

pub fn remove_from_library(db: &Database, id: &str) -> ApiResult<()> {
    let chapter_ids: Vec<String> = {
        let lookup = id.to_string();
        block_db(db, move |conn| {
            let book_id = repo::require_library_entry(conn, &lookup)?;
            let mut statement = conn.prepare(
                "SELECT a.chapter_id FROM chapter_audio a \
                 JOIN chapters c ON c.id = a.chapter_id WHERE c.book_id = ?1",
            )?;
            let rows = statement.query_map([book_id], |row| row.get(0))?;
            rows.collect::<Result<Vec<_>, _>>().map_err(ApiError::from)
        })?
    };

    // Stored audio is not owned by a database row: clean it up explicitly.
    for chapter_id in chapter_ids {
        crate::services::audio::remove_audio(db, &chapter_id)?;
    }

    let id = id.to_string();
    block_db(db, move |conn| {
        // Chapters, progress and bookmarks stay with the catalogue book; only
        // the shelf link goes.
        conn.execute("DELETE FROM library_books WHERE id = ?1", [&id])?;
        Ok(())
    })
}

/* -------------------------------------------------------------------------- */
/* Chapters                                                                   */
/* -------------------------------------------------------------------------- */

pub fn list_chapters(db: &Database, library_id: &str) -> ApiResult<Value> {
    let library_id = library_id.to_string();
    block_db(db, move |conn| {
        let book_id = repo::require_library_entry(conn, &library_id)?;
        let chapters = load_chapter_infos(conn, &book_id)?;
        Ok(to_value(chapters).unwrap_or(Value::Null))
    })
}

/// Add one chapter or a bulk paste — the three shapes of `addChapterSchema`.
pub fn add_chapters(db: &Database, library_id: &str, body: &Value) -> ApiResult<Value> {
    let library_id = library_id.to_string();
    let body = body.clone();
    block_db(db, move |conn| {
        let book_id = repo::require_library_entry(conn, &library_id)?;
        let object = body
            .as_object()
            .ok_or_else(|| ApiError::invalid("body", "Expected a JSON object."))?;

        if let Some(chapters) = object.get("chapters").and_then(|v| v.as_array()) {
            if chapters.is_empty() || chapters.len() > 500 {
                return Err(ApiError::invalid(
                    "chapters",
                    "Array must contain at least 1 item(s).",
                ));
            }
            let entries = chapters
                .iter()
                .map(|entry| {
                    let title = entry
                        .get("title")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| ApiError::invalid("title", "Required."))?;
                    let title = sanitize_text(title, MAX_CHAPTER_TITLE);
                    if title.is_empty() {
                        return Err(ApiError::invalid(
                            "title",
                            "String must contain at least 1 character(s).",
                        ));
                    }
                    let number = parse_chapter_number(entry.get("chapterNumber"))?;
                    Ok((title, number))
                })
                .collect::<ApiResult<Vec<(String, Option<i64>)>>>()?;
            let (count, chapters) = add_chapters_bulk(conn, &book_id, &entries)?;
            return Ok(json!({ "created": count, "chapters": chapters }));
        }

        if let Some(titles) = object.get("titles").and_then(|v| v.as_array()) {
            if titles.is_empty() || titles.len() > 500 {
                return Err(ApiError::invalid(
                    "titles",
                    "Array must contain at least 1 item(s).",
                ));
            }
            let entries: Vec<(String, Option<i64>)> = titles
                .iter()
                .map(|title| {
                    (
                        sanitize_text(title.as_str().unwrap_or(""), MAX_CHAPTER_TITLE),
                        None,
                    )
                })
                .collect();
            let (count, chapters) = add_chapters_bulk(conn, &book_id, &entries)?;
            return Ok(json!({ "created": count, "chapters": chapters }));
        }

        if let Some(title) = object.get("title").and_then(|v| v.as_str()) {
            let chapter = add_single_chapter(
                conn,
                &book_id,
                title,
                parse_chapter_number(object.get("chapterNumber"))?,
            )?;
            return Ok(json!({
                "created": 1,
                "chapter": to_value(chapter).unwrap_or(Value::Null),
            }));
        }

        Err(ApiError::invalid(
            "body",
            "Some of the submitted values are not valid.",
        ))
    })
}

fn parse_chapter_number(value: Option<&Value>) -> ApiResult<Option<i64>> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(value) => {
            let number = value.as_i64().filter(|n| (1..=1000).contains(n));
            match number {
                Some(number) => Ok(Some(number)),
                None => Err(ApiError::invalid(
                    "chapterNumber",
                    "Number must be an integer, greater than or equal to 1 and less than or equal to 1000.",
                )),
            }
        }
    }
}

fn next_chapter_number(conn: &Connection, book_id: &str) -> rusqlite::Result<i64> {
    let last: Option<i64> = conn
        .query_row(
            "SELECT chapter_number FROM chapters WHERE book_id = ?1 \
             ORDER BY chapter_number DESC LIMIT 1",
            [book_id],
            |row| row.get(0),
        )
        .map(Some)
        .or_else(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        })?;
    Ok(last.unwrap_or(0) + 1)
}

fn next_sort_index(conn: &Connection, book_id: &str) -> rusqlite::Result<i64> {
    let last: Option<i64> = conn
        .query_row(
            "SELECT sort_index FROM chapters WHERE book_id = ?1 ORDER BY sort_index DESC LIMIT 1",
            [book_id],
            |row| row.get(0),
        )
        .map(Some)
        .or_else(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        })?;
    Ok(last.unwrap_or(-1) + 1)
}

/// The `addChapter` port: one chapter, appended at the end.
fn add_single_chapter(
    conn: &Connection,
    book_id: &str,
    title: &str,
    chapter_number: Option<i64>,
) -> ApiResult<ChapterInfo> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM chapters WHERE book_id = ?1",
        [book_id],
        |row| row.get(0),
    )?;
    if count >= MAX_CHAPTERS_PER_BOOK {
        return Err(request_error(
            400,
            format!("A book cannot have more than {MAX_CHAPTERS_PER_BOOK} chapters."),
        ));
    }

    let title = sanitize_text(title, MAX_CHAPTER_TITLE);
    let title = if title.is_empty() {
        "Untitled chapter".to_string()
    } else {
        title
    };
    let chapter_number = match chapter_number {
        Some(number) => number,
        None => next_chapter_number(conn, book_id)?,
    };
    let sort_index = next_sort_index(conn, book_id)?;
    let chapter_id = repo::new_id("ch");
    let now = repo::now_iso();
    conn.execute(
        "INSERT INTO chapters (id, book_id, chapter_number, title, sort_index, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
        rusqlite::params![chapter_id, book_id, chapter_number, title, sort_index, now],
    )?;
    crate::services::progress::recompute_book_progress(conn, book_id)?;

    load_chapter_info(conn, &chapter_id)?.ok_or_else(|| ApiError::internal("Chapter not found"))
}

/// The `addChaptersBulk` port, resolving number collisions so a bad paste
/// never fails the whole batch.
fn add_chapters_bulk(
    conn: &Connection,
    book_id: &str,
    entries: &[(String, Option<i64>)],
) -> ApiResult<(usize, Vec<Value>)> {
    let cleaned: Vec<(String, Option<i64>)> = entries
        .iter()
        .filter(|(title, _)| !title.is_empty())
        .take(MAX_CHAPTERS_PER_BOOK as usize)
        .cloned()
        .collect();
    if cleaned.is_empty() {
        return Ok((0, Vec::new()));
    }

    let mut taken: Vec<i64> = {
        let mut statement =
            conn.prepare("SELECT chapter_number FROM chapters WHERE book_id = ?1")?;
        let rows = statement.query_map([book_id], |row| row.get(0))?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    let mut next_free = next_chapter_number(conn, book_id)?;
    let mut sort_index = next_sort_index(conn, book_id)?;
    let now = repo::now_iso();

    for (title, requested) in &cleaned {
        let chapter_number = match requested {
            Some(number) if !taken.contains(number) => *number,
            _ => {
                while taken.contains(&next_free) {
                    next_free += 1;
                }
                next_free
            }
        };
        taken.push(chapter_number);

        let chapter_id = repo::new_id("ch");
        conn.execute(
            "INSERT INTO chapters (id, book_id, chapter_number, title, sort_index, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
            rusqlite::params![chapter_id, book_id, chapter_number, title, sort_index, now],
        )?;
        sort_index += 1;
    }

    crate::services::progress::recompute_book_progress(conn, book_id)?;
    let chapters = load_chapter_infos(conn, book_id)?;
    Ok((
        cleaned.len(),
        chapters
            .iter()
            .map(|c| to_value(c).unwrap_or(Value::Null))
            .collect(),
    ))
}

pub async fn get_book(db: &Database, id: &str) -> ApiResult<Value> {
    const EXTERNAL_PREFIX: &str = "external:";
    let external = id
        .strip_prefix(EXTERNAL_PREFIX)
        .map(|suffix| suffix.to_string());

    // Phase 1: look for a local mirror.
    let local: Option<BookSummary> = {
        let lookup = id.to_string();
        let wanted = external.clone();
        block_db(db, move |conn| match wanted {
            Some(key) => load_book_summary_by_external(conn, &key),
            None => load_book_summary(conn, &lookup),
        })?
    };

    let Some(local) = local else {
        // The book may only exist upstream (e.g. a deep link the catalogue has
        // not seen yet). Hydrate it, then continue.
        let Some(key) = external else {
            return Err(ApiError::not_found("That book is no longer in the catalogue."));
        };
        let catalogue = crate::services::catalogue::CatalogueService::new(
            crate::config::get()?,
        );
        let hydrated = catalogue
            .get_book_summary(db, &key)
            .await?
            .ok_or_else(|| ApiError::not_found("We could not find that book."))?;
        return Ok(json!({
            "book": hydrated,
            "libraryBook": null,
            "chapters": [],
            "bookmarks": [],
        }));
    };

    // Enrich from the provider when the description or cover is still thin.
    let book = if (local.description.is_none() || local.cover_url.is_none())
        && local.library_id.is_none()
    {
        let catalogue = crate::services::catalogue::CatalogueService::new(
            crate::config::get()?,
        );
        match catalogue.get_book_summary(db, &local.external_id).await {
            Ok(Some(hydrated)) => merge_hydrated(&local, hydrated),
            _ => to_value(&local).unwrap_or(Value::Null),
        }
    } else {
        to_value(&local).unwrap_or(Value::Null)
    };

    // Phase 3: chapters and bookmarks, only for books on the shelf.
    let book_id = local.id.clone();
    block_db(db, move |conn| {
        let library_book = match &local.library_id {
            Some(library_id) => {
                let entry = load_library_entry(conn, library_id)?;
                Some(to_value(entry).unwrap_or(Value::Null))
            }
            None => None,
        };
        let (chapters, bookmarks) = match &local.library_id {
            Some(_) => (
                load_chapter_infos(conn, &book_id)?,
                load_bookmarks(conn, &book_id)?,
            ),
            None => (Vec::new(), Vec::new()),
        };
        Ok(json!({
            "book": book,
            "libraryBook": library_book,
            "chapters": chapters,
            "bookmarks": bookmarks,
        }))
    })
}

/// `{ ...book, ...hydrated, inLibrary, favorite }` — the provider wins on
/// metadata, the shelf state always stays local.
fn merge_hydrated(local: &BookSummary, hydrated: Value) -> Value {
    let mut merged = hydrated;
    if let Some(object) = merged.as_object_mut() {
        object.insert("inLibrary".into(), json!(local.in_library));
        object.insert("favorite".into(), json!(local.favorite));
        object.insert(
            "libraryId".into(),
            match &local.library_id {
                Some(id) => json!(id),
                None => Value::Null,
            },
        );
    }
    merged
}

pub fn get_chapter(db: &Database, id: &str) -> ApiResult<Value> {
    let id = id.to_string();
    block_db(db, move |conn| match load_chapter_info(conn, &id)? {
        Some(chapter) => Ok(to_value(chapter).unwrap_or(Value::Null)),
        None => Err(ApiError::not_found("That chapter no longer exists.")),
    })
}

/// Rename or renumber (`updateChapter`).
pub fn update_chapter(db: &Database, id: &str, body: &Value) -> ApiResult<Value> {
    let id = id.to_string();
    let body = body.clone();
    block_db(db, move |conn| -> ApiResult<Value> {
        let title = body.get("title");
        let number = body.get("chapterNumber");
        if title.is_none() && number.is_none() {
            return Err(request_error(400, "Nothing to update."));
        }

        let reference = repo::require_chapter(conn, &id)?;

        let mut assignments: Vec<String> = Vec::new();
        let mut params: Vec<Box<dyn rusqlite::ToSql + Send>> = Vec::new();

        if let Some(title) = title {
            let title = sanitize_text(title.as_str().unwrap_or(""), MAX_CHAPTER_TITLE);
            if title.is_empty() {
                return Err(ApiError::invalid(
                    "title",
                    "String must contain at least 1 character(s).",
                ));
            }
            assignments.push(format!("title = ?{}", params.len() + 1));
            params.push(Box::new(title));
        }

        if let Some(number) = number {
            let number = number
                .as_i64()
                .filter(|n| (1..=1000).contains(n))
                .ok_or_else(|| {
                    ApiError::invalid(
                        "chapterNumber",
                        "Number must be an integer, greater than or equal to 1 and less than or equal to 1000.",
                    )
                })?;
            let clash: Option<String> = conn
                .query_row(
                    "SELECT id FROM chapters WHERE book_id = ?1 AND chapter_number = ?2 AND id != ?3",
                    rusqlite::params![reference.book_id, number, id],
                    |row| row.get(0),
                )
                .map(Some)
                .or_else(|error| match error {
                    rusqlite::Error::QueryReturnedNoRows => Ok(None),
                    other => Err(other),
                })?;
            if clash.is_some() {
                return Err(request_error(409, format!("Chapter {number} already exists.")));
            }
            assignments.push(format!("chapter_number = ?{}", params.len() + 1));
            params.push(Box::new(number));
            assignments.push(format!("sort_index = ?{}", params.len() + 1));
            params.push(Box::new(number));
        }

        let sql = format!("UPDATE chapters SET {} WHERE id = ?", assignments.join(", "));
        let mut refs: Vec<&dyn rusqlite::ToSql> = params
            .iter()
            .map(|boxed| boxed.as_ref() as &dyn rusqlite::ToSql)
            .collect();
        refs.push(&id);
        conn.execute(&sql, rusqlite::params_from_iter(refs))?;

        let chapter = load_chapter_info(conn, &id)?
            .ok_or_else(|| ApiError::not_found("That chapter no longer exists."))?;
        Ok::<Value, ApiError>(serde_json::to_value(chapter)?)
    })
}

pub fn delete_chapter(db: &Database, id: &str) -> ApiResult<()> {
    let (book_id, has_audio): (String, bool) = {
        let lookup = id.to_string();
        block_db(db, move |conn| {
            let reference = repo::require_chapter(conn, &lookup)?;
            let audio: Option<String> = conn
                .query_row(
                    "SELECT id FROM chapter_audio WHERE chapter_id = ?1",
                    [&lookup],
                    |row| row.get(0),
                )
                .map(Some)
                .or_else(|error| match error {
                    rusqlite::Error::QueryReturnedNoRows => Ok(None),
                    other => Err(other),
                })?;
            Ok::<(String, bool), ApiError>((reference.book_id, audio.is_some()))
        })?
    };

    if has_audio {
        crate::services::audio::remove_audio(db, id)?;
    }

    let id = id.to_string();
    block_db(db, move |conn| {
        // Bookmarks and progress cascade from the chapter row.
        conn.execute("DELETE FROM chapters WHERE id = ?1", [&id])?;
        renumber_book(conn, &book_id)?;
        crate::services::progress::recompute_book_progress(conn, &book_id)?;
        Ok(())
    })
}

/// Close the gap left by a deleted chapter so numbering stays contiguous 1..n.
fn renumber_book(conn: &Connection, book_id: &str) -> ApiResult<()> {
    let mut statement = conn.prepare(
        "SELECT id, chapter_number FROM chapters WHERE book_id = ?1 \
         ORDER BY chapter_number ASC, sort_index ASC",
    )?;
    let rows = statement.query_map([book_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    let chapters = rows.collect::<Result<Vec<_>, _>>()?;

    let already_contiguous = chapters
        .iter()
        .enumerate()
        .all(|(index, (_, number))| *number == index as i64 + 1);
    if already_contiguous {
        return Ok(());
    }

    for (index, (id, _)) in chapters.iter().enumerate() {
        conn.execute(
            "UPDATE chapters SET chapter_number = ?1, sort_index = ?2 WHERE id = ?3",
            rusqlite::params![index as i64 + 1, index as i64, id],
        )?;
    }
    Ok(())
}

/// Persist a drag-and-drop reorder (`reorderChapters`). Numbers are rewritten
/// to match the order; anything the client forgot keeps its relative position
/// at the end.
pub fn reorder_chapters(db: &Database, book_id: &str, ids: &[String]) -> ApiResult<Value> {
    let book_id = book_id.to_string();
    let ids: Vec<String> = ids.to_vec();
    block_db(db, move |conn| {
        let mut statement = conn.prepare(
            "SELECT id FROM chapters WHERE book_id = ?1 ORDER BY chapter_number ASC, sort_index ASC",
        )?;
        let existing = statement
            .query_map([book_id.as_str()], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;

        let mut next: Vec<String> = ids.into_iter().filter(|id| existing.contains(id)).collect();
        for id in &existing {
            if !next.contains(id) {
                next.push(id.clone());
            }
        }

        // Two passes so renumbering can never collide with a number that is
        // about to move: park every chapter on a temporary number first.
        for (index, id) in next.iter().enumerate() {
            conn.execute(
                "UPDATE chapters SET chapter_number = ?1 WHERE id = ?2",
                rusqlite::params![-(index as i64) - 1, id],
            )?;
        }
        for (index, id) in next.iter().enumerate() {
            conn.execute(
                "UPDATE chapters SET sort_index = ?1, chapter_number = ?2 WHERE id = ?3",
                rusqlite::params![index as i64, index as i64 + 1, id],
            )?;
        }

        let chapters = load_chapter_infos(conn, &book_id)?;
        Ok(to_value(chapters).unwrap_or(Value::Null))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        _dir: tempfile::TempDir,
        db: Database,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().expect("temp dir");
        let db = Database::open(&dir.path().join("vb.db")).expect("open");
        Fixture { _dir: dir, db }
    }

    impl Fixture {
        fn raw(&self) -> rusqlite::Connection {
            rusqlite::Connection::open(self._dir.path().join("vb.db")).expect("raw connection")
        }

        fn insert_book(&self, id: &str, external: &str, title: &str, author: &str, created: &str) {
            let connection = self.raw();
            connection
                .execute(
                    "INSERT INTO books (id, external_id, title, author, categories, created_at, updated_at) \
                     VALUES (?1, ?2, ?3, ?4, '[]', ?5, ?5)",
                    rusqlite::params![id, external, title, author, created],
                )
                .expect("insert book");
        }

        fn insert_library(&self, id: &str, book_id: &str, added: &str) {
            let connection = self.raw();
            connection
                .execute(
                    "INSERT INTO library_books (id, book_id, added_at) VALUES (?1, ?2, ?3)",
                    rusqlite::params![id, book_id, added],
                )
                .expect("insert library");
        }

        fn services(&self) -> Services {
            Services::new(&crate::config::Config::for_data_dir(self._dir.path().to_path_buf()))
        }
    }

    #[test]
    fn constants_match_the_web_app() {
        assert_eq!(
            LIBRARY_FILTERS,
            &["all", "in-progress", "completed", "not-started", "has-audio", "needs-audio"]
        );
        assert_eq!(
            LIBRARY_SORTS,
            &["recently-added", "recently-listened", "title", "author", "progress"]
        );
    }

    #[test]
    fn sanitize_text_strips_controls_and_clamps() {
        assert_eq!(sanitize_text("  hello\u{0000}   world \n", 120), "hello world");
        assert_eq!(sanitize_text("a\u{202E}b\u{200B}c", 120), "abc");
        assert_eq!(sanitize_text(&"x".repeat(200), 160), "x".repeat(160));
        assert_eq!(sanitize_multiline("a\n\n\n\n\nb", 100), "a\n\nb");
        assert_eq!(sanitize_multiline("a\r\nb", 100), "a\nb");
        assert_eq!(collapse_whitespace("a \t b"), "a b");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn list_library_filters_sorts_and_paginates() {
        let fixture = fixture();
        fixture.insert_book("bk_a", "OL1A", "Zebra", "Ann", "2026-01-01T00:00:00.000Z");
        fixture.insert_book("bk_b", "OL2B", "alpha", "Bob", "2026-01-02T00:00:00.000Z");
        fixture.insert_book("bk_c", "OL3C", "Middle", "Cid", "2026-01-03T00:00:00.000Z");
        fixture.insert_library("lb_a", "bk_a", "2026-01-01T00:00:00.000Z");
        fixture.insert_library("lb_b", "bk_b", "2026-01-02T00:00:00.000Z");
        fixture.insert_library("lb_c", "bk_c", "2026-01-03T00:00:00.000Z");
        {
            // lb_a: started but incomplete; lb_b: completed (the rollup writes
            // `overallProgress: 1` in the same update that stamps
            // `completedAt`); lb_c: untouched.
            let connection = fixture.raw();
            connection
                .execute("UPDATE library_books SET overall_progress = 0.5 WHERE id = 'lb_a'", [])
                .expect("progress a");
            connection
                .execute(
                    "UPDATE library_books SET overall_progress = 1.0, completed_at = '2026-01-05T00:00:00.000Z' WHERE id = 'lb_b'",
                    [],
                )
                .expect("completed b");
        }

        let (items, total, stats) = list_library(&fixture.db, "all", "recently-added", 1, 24, "")
            .expect("list");
        assert_eq!(total, 3);
        assert_eq!(items.len(), 3);
        // Recently added: newest first.
        assert_eq!(items[0]["bookId"], "bk_c");
        assert_eq!(items[2]["bookId"], "bk_a");
        assert_eq!(stats["bookCount"], 3);

        let (in_progress, total, _) =
            list_library(&fixture.db, "in-progress", "title", 1, 24, "").expect("in-progress");
        assert_eq!(total, 1);
        assert_eq!(in_progress[0]["bookId"], "bk_a");

        let (_, total, _) = list_library(&fixture.db, "completed", "title", 1, 24, "").expect("completed");
        assert_eq!(total, 1);

        let (not_started, _, _) =
            list_library(&fixture.db, "not-started", "title", 1, 24, "").expect("not-started");
        assert_eq!(not_started.len(), 1);
        assert_eq!(not_started[0]["bookId"], "bk_c");

        // SQLite's BINARY collation, the same one Prisma ordered with.
        let (sorted, _, _) = list_library(&fixture.db, "all", "title", 1, 24, "").expect("title sort");
        assert_eq!(sorted[0]["book"]["title"], "Middle");

        // Author search.
        let (found, total, _) = list_library(&fixture.db, "all", "title", 1, 24, "Bob").expect("search");
        assert_eq!(total, 1);
        assert_eq!(found[0]["bookId"], "bk_b");

        // Pagination.
        let (page_two, total, _) =
            list_library(&fixture.db, "all", "recently-added", 2, 2, "").expect("page 2");
        assert_eq!(total, 3);
        assert_eq!(page_two.len(), 1);
        assert_eq!(page_two[0]["bookId"], "bk_a");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn add_to_library_seeds_one_placeholder_chapter() {
        let fixture = fixture();
        let services = fixture.services();
        fixture.insert_book("bk_1", "OL1W", "Dune", "Herbert", "2026-01-01T00:00:00.000Z");

        let entry = add_to_library(&fixture.db, &services, Some("bk_1"), None, true).await.expect("add");
        assert_eq!(entry["seededChapters"], 1);
        assert_eq!(entry["libraryBook"]["favorite"], true);
        assert_eq!(entry["libraryBook"]["bookId"], "bk_1");

        let library_id = entry["libraryBook"]["libraryId"]
            .as_str()
            .expect("id")
            .to_string();
        let chapters = list_chapters(&fixture.db, &library_id).expect("chapters");
        assert_eq!(chapters.as_array().expect("array").len(), 1);
        assert_eq!(chapters[0]["title"], "Chapter 1");
        assert_eq!(chapters[0]["chapterNumber"], 1);

        // Adding again is idempotent: no new entry, no extra chapter.
        let again = add_to_library(&fixture.db, &services, Some("bk_1"), None, false).await.expect("again");
        assert_eq!(again["seededChapters"], 0);
        assert_eq!(
            again["libraryBook"]["libraryId"],
            entry["libraryBook"]["libraryId"]
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn chapters_add_reorder_and_delete_renumber() {
        let fixture = fixture();
        let services = fixture.services();
        fixture.insert_book("bk_1", "OL1W", "Dune", "Herbert", "2026-01-01T00:00:00.000Z");

        let entry = add_to_library(&fixture.db, &services, Some("bk_1"), None, false).await.expect("add");
        let library_id = entry["libraryBook"]["libraryId"]
            .as_str()
            .expect("id")
            .to_string();

        let created = add_chapters(
            &fixture.db,
            &library_id,
            &json!({ "titles": ["Chapter 2", "Chapter 3", ""] }),
        )
        .expect("bulk");
        assert_eq!(created["created"], 2);

        let chapters = list_chapters(&fixture.db, &library_id).expect("chapters");
        assert_eq!(chapters.as_array().expect("array").len(), 3);

        let ids: Vec<String> = chapters
            .as_array()
            .expect("array")
            .iter()
            .map(|c| c["id"].as_str().expect("id").to_string())
            .collect();

        // Reverse the order: numbers must follow.
        let reversed: Vec<String> = ids.iter().rev().cloned().collect();
        let reordered = reorder_chapters(&fixture.db, "bk_1", &reversed).expect("reorder");
        assert_eq!(reordered[0]["id"], reversed[0]);
        assert_eq!(reordered[0]["chapterNumber"], 1);
        assert_eq!(reordered[2]["chapterNumber"], 3);

        // Delete the new first chapter; numbering closes the gap.
        delete_chapter(&fixture.db, &reversed[0]).expect("delete");
        let after = list_chapters(&fixture.db, &library_id).expect("after");
        let list = after.as_array().expect("array");
        assert_eq!(list.len(), 2);
        assert_eq!(list[0]["chapterNumber"], 1);
        assert_eq!(list[1]["chapterNumber"], 2);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn removing_the_library_entry_keeps_the_catalogue_book() {
        let fixture = fixture();
        let services = fixture.services();
        fixture.insert_book("bk_1", "OL1W", "Dune", "Herbert", "2026-01-01T00:00:00.000Z");

        let entry = add_to_library(&fixture.db, &services, Some("bk_1"), None, false).await.expect("add");
        let library_id = entry["libraryBook"]["libraryId"]
            .as_str()
            .expect("id")
            .to_string();

        remove_from_library(&fixture.db, &library_id).expect("remove");
        let error = get_library_entry(&fixture.db, &library_id).expect_err("gone");
        assert_eq!(error.message, "That book is not in your library.");

        // The catalogue book survives, so re-adding works.
        let again = add_to_library(&fixture.db, &services, Some("bk_1"), None, false).await.expect("re-add");
        assert_eq!(again["seededChapters"], 0);
    }
}

/// A tiny seeding helper shared by the service test suites (progress,
/// bookmarks, playback, home, covers). Kept here because this module already
/// owns the schema knowledge for books, chapters, audio and library rows.
#[cfg(test)]
pub(crate) mod tests_support {
    use crate::db::Database;

    /// Chapters per seeded book are named `ch{book}-{index}` (0-based) and
    /// their audio rows `au{book}-{index}` with a 100-second duration.
    pub(crate) struct Fixture {
        pub(crate) db: Database,
        pub(crate) dir: tempfile::TempDir,
    }

    pub(crate) fn fixture() -> Fixture {
        let dir = tempfile::tempdir().expect("temp dir");
        let db = Database::open(&dir.path().join("vb.db")).expect("open");
        Fixture { db, dir }
    }

    impl Fixture {
        /// The directory backing the database, for building a `Config` whose
        /// paths sit beside it.
        pub(crate) fn data_dir(&self) -> std::path::PathBuf {
            self.dir.path().to_path_buf()
        }

        /// A raw connection for seeding rows the services would not create.
        pub(crate) fn raw(&self) -> rusqlite::Connection {
            rusqlite::Connection::open(self.dir.path().join("vb.db")).expect("raw connection")
        }

        /// A book with `chapters` chapters; optionally with audio attached.
        pub(crate) fn seed_book(&self, book_id: &str, chapters: i64, with_audio: bool) {
            let connection = self.raw();
            connection
                .execute(
                    "INSERT INTO books (id, external_id, title, author, categories, created_at, updated_at) \
                     VALUES (?1, ?2, ?3, 'Author', '[]', '2026-01-01T00:00:00.000Z', '2026-01-01T00:00:00.000Z')",
                    rusqlite::params![book_id, format!("OL-{book_id}"), format!("Book {book_id}")],
                )
                .expect("insert book");
            for index in 0..chapters {
                connection
                    .execute(
                        "INSERT INTO chapters (id, book_id, chapter_number, title, sort_index, created_at, updated_at) \
                         VALUES (?1, ?2, ?3, ?4, ?3, '2026-01-01T00:00:00.000Z', '2026-01-01T00:00:00.000Z')",
                        rusqlite::params![
                            format!("ch{book_id}-{index}"),
                            book_id,
                            index + 1,
                            format!("Chapter {}", index + 1)
                        ],
                    )
                    .expect("insert chapter");
                if with_audio {
                    connection
                        .execute(
                            "INSERT INTO chapter_audio \
                                (id, chapter_id, file_url, storage_key, duration, mime_type, uploaded_at) \
                             VALUES (?1, ?2, 'file.mp3', ?3, 100.0, 'audio/mpeg', '2026-01-01T00:00:00.000Z')",
                            rusqlite::params![
                                format!("au{book_id}-{index}"),
                                format!("ch{book_id}-{index}"),
                                format!("sk-{book_id}-{index}")
                            ],
                        )
                        .expect("insert audio");
                }
            }
        }

        /// Attach the book to the shelf under `library_id`.
        pub(crate) fn library_link(&self, book_id: &str, library_id: &str) {
            self.raw()
                .execute(
                    "INSERT INTO library_books (id, book_id, added_at) VALUES (?1, ?2, '2026-01-01T00:00:00.000Z')",
                    rusqlite::params![library_id, book_id],
                )
                .expect("insert library");
        }
    }
}
