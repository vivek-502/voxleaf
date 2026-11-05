//! EPUB import and the reader's data surface.
//!
//! Port of the web app's `src/server/services/epub.ts` and `epub-chapters.ts`
//! with their Prisma helpers in `src/server/db/epub.ts`: the container is kept
//! on disk, its chapters and images live in the database, and the library's
//! chapter rows are bridged to the spine through `chapters.epub_index`.
//!
//! Text and audio live in different tables on purpose, exactly as they did in
//! the web app: `epub_chapters` holds sanitised markup straight out of the
//! container, while `chapters` owns the uploaded audio, listening progress and
//! bookmarks. Each reading-order document gets at most one chapter row.

pub mod parse;

use std::collections::HashSet;
use std::path::PathBuf;

use axum::body::{Body, Bytes};
use axum::extract::Request;
use axum::http::{header, StatusCode};
use axum::response::Response;
use rusqlite::types::Value as SqlValue;
use rusqlite::{params, params_from_iter, OptionalExtension};
use serde_json::{json, Value};

use crate::api::multipart;
use crate::api::AppState;
use crate::config::Config;
use crate::db::Database;
use crate::error::{ApiError, ApiResult};
use crate::repo;
use crate::services::library::{block_db, load_chapter_info, load_chapter_infos, request_error};
use crate::services::progress::recompute_book_progress;

pub use parse::{
    is_zip, parse, ContentsRow, EpubParseError, ParsedChapter, ParsedEpub, ParsedImage,
};

/// Longest chapter title kept — `TITLE_LIMIT` in the web app, counted the way
/// JavaScript counts: UTF-16 code units.
const TITLE_LIMIT: usize = 300;

/// Longest stored file name — `slice(0, 200)` in the web app's upload route.
const FILE_NAME_LIMIT: usize = 200;

/* -------------------------------------------------------------------------- */
/* Service                                                                    */
/* -------------------------------------------------------------------------- */

pub struct EpubService {
    cfg: Config,
}

impl EpubService {
    pub fn new(cfg: &Config) -> Self {
        Self { cfg: cfg.clone() }
    }

    /* -- status ---------------------------------------------------------- */

    /// `getEpubStatus` — what the reader needs to draw its import panel, and
    /// the flag the instance uses to decide whether books may be stored.
    pub fn status(&self, db: &Database, book_id: &str) -> ApiResult<Value> {
        let book_id = book_id.to_string();
        block_db(db, move |conn| {
            let record: Option<(
                String,
                Option<String>,
                Option<String>,
                Option<i64>,
                Option<String>,
                Option<String>,
                Option<String>,
                i64,
                i64,
            )> = conn
                .query_row(
                    "SELECT e.status, e.error, e.file_name, e.file_size, e.title, e.creator, \
                            e.language, \
                            (SELECT COUNT(*) FROM epub_chapters c WHERE c.epub_id = e.id), \
                            (SELECT COUNT(*) FROM epub_images i WHERE i.epub_id = e.id) \
                     FROM epub_files e WHERE e.book_id = ?1",
                    [&book_id],
                    |row| {
                        Ok((
                            row.get(0)?,
                            row.get(1)?,
                            row.get(2)?,
                            row.get(3)?,
                            row.get(4)?,
                            row.get(5)?,
                            row.get(6)?,
                            row.get(7)?,
                            row.get(8)?,
                        ))
                    },
                )
                .optional()?;

            let Some((
                status,
                error,
                file_name,
                file_size,
                title,
                creator,
                language,
                chapter_count,
                image_count,
            )) = record
            else {
                return Ok(json!({
                    "hasEpub": false,
                    "status": null,
                    "error": null,
                    "fileName": null,
                    "fileSize": null,
                    "chapterCount": 0,
                    "imageCount": 0,
                    "title": null,
                    "creator": null,
                    "language": null,
                }));
            };

            let has_epub = status == "ready";
            // Anything between "ready" and "failed" is not worth a word on the
            // panel — the web app collapsed it to "none" before it shipped.
            let collapsed = if has_epub || status == "failed" {
                status
            } else {
                "none".to_string()
            };
            Ok(json!({
                "hasEpub": has_epub,
                "status": collapsed,
                "error": error,
                "fileName": file_name,
                "fileSize": file_size,
                "chapterCount": chapter_count,
                "imageCount": image_count,
                "title": title,
                "creator": creator,
                "language": language,
            }))
        })
    }

    /* -- upload ---------------------------------------------------------- */

    /// `POST /api/books/:id/epub/upload` — the reader hands over a file; the
    /// database holds whatever is currently stored until a parse succeeds.
    pub async fn receive_upload(
        &self,
        st: &AppState,
        book_id: &str,
        req: Request,
    ) -> ApiResult<Value> {
        // The catalogue decides what can carry a file — same 404 as the route.
        let book: Option<(String, Option<String>)> = st
            .db
            .call({
                let book_id = book_id.to_string();
                move |conn| {
                    conn.query_row(
                        "SELECT title, author FROM books WHERE id = ?1",
                        [&book_id],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .optional()
                }
            })
            .await?;
        let (book_title, book_author) =
            book.ok_or_else(|| ApiError::not_found("That book is not in the catalogue."))?;

        let file = multipart::read_epub_file(req, self.cfg.epub.max_bytes).await?;
        let buffer = file.bytes;
        let fallback_name = format!("{book_title}.epub");
        let raw_name = if file.filename.is_empty() {
            fallback_name.as_str()
        } else {
            file.filename.as_str()
        };
        let file_name = truncate_utf16(raw_name, FILE_NAME_LIMIT).to_string();

        // The checks that decide "not usable" without touching what is stored.
        let early = if buffer.is_empty() {
            Some("That file was empty.".to_string())
        } else if buffer.len() as u64 > self.cfg.epub.max_bytes {
            Some(size_limit_message(self.cfg.epub.max_bytes))
        } else if !is_zip(&buffer) {
            Some(
                "That is not an EPUB file. EPUBs are zip archives, and this one does not start \
                 like one."
                    .to_string(),
            )
        } else {
            None
        };
        if let Some(message) = early {
            return self.upload_payload(st, book_id, "failed", false, Some(message)).await;
        }

        let was_readable = self.existing_status(st, book_id).await?.as_deref() == Some("ready");

        let outcome: Result<i64, String> = async {
            let prefix = format!("/api/epub/{book_id}/image/");
            let parsed = parse_blocking(
                buffer.clone(),
                prefix,
                self.cfg.epub.max_image_bytes,
                self.cfg.epub.max_documents,
            )
            .await?;
            self.store_parsed(
                &st.db,
                book_id,
                &buffer,
                parsed,
                file_name.clone(),
                book_title,
                book_author.unwrap_or_default(),
            )
            .await
            .map_err(|error| error.message)
        }
        .await;

        let (action, error) = match outcome {
            Ok(_) => ("ready".to_string(), None),
            Err(message) => {
                // A rejected file must not take a working book down with it:
                // the web app left the old chapters serving and recorded the
                // failure beside them.
                block_db(&st.db, {
                    let (book_id, message) = (book_id.to_string(), message.clone());
                    move |conn| {
                        let patch = RecordPatch {
                            error: Some(if was_readable {
                                None
                            } else {
                                Some(message)
                            }),
                            ..RecordPatch::default()
                        };
                        upsert_record(
                            conn,
                            &book_id,
                            if was_readable { "ready" } else { "failed" },
                            patch,
                        )
                        .map(|_| ())
                    }
                })?;
                ("failed".to_string(), Some(message))
            }
        };

        // `syncChaptersWithEpub` only when this book is on the shelf — the web
        // route swallowed its failures, and so does this one.
        if action == "ready" && self.in_library(st, book_id).await? {
            if let Err(error) = self.sync_chapters(&st.db, book_id) {
                tracing::warn!(%error, "epub chapter auto-sync failed");
            }
        }

        self.upload_payload(st, book_id, &action, was_readable, error)
            .await
    }

    /// `{...status, action, replaced, error}` — the upload route's answer.
    async fn upload_payload(
        &self,
        st: &AppState,
        book_id: &str,
        action: &str,
        replaced: bool,
        error: Option<String>,
    ) -> ApiResult<Value> {
        let mut payload = self.status(&st.db, book_id)?;
        payload["action"] = json!(action);
        payload["replaced"] = json!(replaced);
        payload["error"] = json!(error);
        Ok(payload)
    }

    async fn existing_status(&self, st: &AppState, book_id: &str) -> ApiResult<Option<String>> {
        st.db
            .call({
                let book_id = book_id.to_string();
                move |conn| {
                    conn.query_row(
                        "SELECT status FROM epub_files WHERE book_id = ?1",
                        [&book_id],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()
                }
            })
            .await
            .map_err(Into::into)
    }

    async fn in_library(&self, st: &AppState, book_id: &str) -> ApiResult<bool> {
        st.db
            .call({
                let book_id = book_id.to_string();
                move |conn| {
                    let found = conn
                        .query_row(
                            "SELECT 1 FROM library_books WHERE book_id = ?1",
                            [&book_id],
                            |row| row.get::<_, i64>(0),
                        )
                        .optional()?
                        .is_some();
                    Ok::<_, rusqlite::Error>(found)
                }
            })
            .await
            .map_err(Into::into)
    }

    /* -- re-parse -------------------------------------------------------- */

    /// `POST /api/books/:id/epub` — re-read the stored container, so a
    /// metadata fix or an image fix reaches books imported before it shipped.
    pub async fn reparse(&self, db: &Database, book_id: &str) -> ApiResult<Value> {
        let known: Option<String> = db
            .call({
                let book_id = book_id.to_string();
                move |conn| {
                    conn.query_row(
                        "SELECT id FROM books WHERE id = ?1",
                        [&book_id],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()
                }
            })
            .await?;
        if known.is_none() {
            return Err(ApiError::not_found("That book is not in the catalogue."));
        }

        let record: Option<(String, String)> = db
            .call({
                let book_id = book_id.to_string();
                move |conn| {
                    conn.query_row(
                        "SELECT id, status FROM epub_files WHERE book_id = ?1",
                        [&book_id],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .optional()
                }
            })
            .await?;
        let Some((epub_id, record_status)) = record else {
            return Ok(json!({ "status": "failed", "error": "No EPUB record found." }));
        };

        let Some(archive) = read_archive(&self.cfg, book_id).await? else {
            return Ok(json!({ "status": "failed", "error": "No stored EPUB found." }));
        };

        let outcome: Result<i64, String> = async {
            let prefix = format!("/api/epub/{book_id}/image/");
            let parsed = parse_blocking(
                Bytes::from(archive),
                prefix,
                self.cfg.epub.max_image_bytes,
                self.cfg.epub.max_documents,
            )
            .await?;
            self.reparse_store(db, book_id, &epub_id, &parsed)
                .await
                .map_err(|error| error.message)
        }
        .await;

        match outcome {
            Ok(count) => Ok(json!({ "status": "ready", "chapterCount": count })),
            Err(message) => {
                // A book that is still readable must stay readable — only one
                // that was broken flips to "failed" with the new message.
                if record_status != "ready" {
                    block_db(db, {
                        let book_id = book_id.to_string();
                        let message = message.clone();
                        move |conn| {
                            let patch = RecordPatch {
                                error: Some(Some(message)),
                                ..RecordPatch::default()
                            };
                            upsert_record(conn, &book_id, "failed", patch).map(|_| ())
                        }
                    })?;
                }
                Ok(json!({ "status": "failed", "error": message }))
            }
        }
    }

    /* -- detach ---------------------------------------------------------- */

    /// `DELETE /api/books/:id/epub` — forget the file. The library's chapter
    /// rows survive; only the extracted text, images and position go.
    pub async fn detach(&self, db: &Database, book_id: &str) -> ApiResult<()> {
        db.call({
            let book_id = book_id.to_string();
            move |conn| -> ApiResult<()> {
                conn.execute("DELETE FROM epub_files WHERE book_id = ?1", [&book_id])?;
                conn.execute("DELETE FROM epub_progress WHERE book_id = ?1", [&book_id])?;
                Ok(())
            }
        })
        .await?;
        delete_archive(&self.cfg, book_id).await
    }

    /* -- sync ------------------------------------------------------------ */

    /// `POST /api/epub/:bookId/chapters` — "use the file's table of contents":
    /// the spine becomes the chapter list, and audio that was attached to a
    /// section stays with it.
    pub fn sync_chapters(&self, db: &Database, book_id: &str) -> ApiResult<Value> {
        let book_id = book_id.to_string();
        block_db(db, move |conn| {
            let record: Option<(String, String)> = conn
                .query_row(
                    "SELECT id, status FROM epub_files WHERE book_id = ?1",
                    [&book_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;
            let Some((epub_id, _status)) = record.filter(|(_, status)| status == "ready") else {
                return Err(request_error(409, "Add the book file before using its contents."));
            };

            // Documents from the table of contents, in reading order; plates
            // and covers are not navigation and never link a chapter.
            let documents: Vec<(i64, String)> = {
                let mut statement = conn.prepare(
                    "SELECT idx, title FROM epub_chapters WHERE epub_id = ?1 AND in_toc = 1 \
                     ORDER BY idx ASC",
                )?;
                let rows = statement
                    .query_map([&epub_id], |row| Ok((row.get(0)?, row.get(1)?)))?
                    .collect::<Result<Vec<_>, _>>()?;
                rows
            };
            if documents.is_empty() {
                return Err(request_error(
                    409,
                    "This book has no table of contents to sync from.",
                ));
            }

            let existing: Vec<ExistingRow> = {
                let mut statement = conn.prepare(
                    "SELECT c.id, c.epub_index, EXISTS(SELECT 1 FROM chapter_audio a \
                     WHERE a.chapter_id = c.id) FROM chapters c WHERE c.book_id = ?1 \
                     ORDER BY c.chapter_number ASC, c.sort_index ASC",
                )?;
                let rows = statement
                    .query_map([&book_id], |row| {
                        Ok(ExistingRow {
                            id: row.get(0)?,
                            epub_index: row.get(1)?,
                            has_audio: row.get::<_, i64>(2)? != 0,
                        })
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                rows
            };

            let plan = plan_chapter_sync(&documents, &existing);
            let now = repo::now_iso();

            let tx = conn.unchecked_transaction()?;
            if !plan.remove.is_empty() {
                let placeholders = vec!["?"; plan.remove.len()].join(", ");
                tx.execute(
                    &format!("DELETE FROM chapters WHERE id IN ({placeholders})"),
                    params_from_iter(plan.remove.iter()),
                )?;
            }
            // Park every surviving row at a negative number first — UNIQUE
            // (book_id, chapter_number) would otherwise trip while the
            // keepers wait their turn for the numbers at the end.
            for (order, id) in plan.park.iter().enumerate() {
                tx.execute(
                    "UPDATE chapters SET chapter_number = ?1, updated_at = ?2 WHERE id = ?3",
                    params![-(order as i64 + 1), now, id],
                )?;
            }
            for item in &plan.upserts {
                match &item.id {
                    Some(id) => {
                        tx.execute(
                            "UPDATE chapters SET chapter_number = ?1, title = ?2, \
                             sort_index = ?3, updated_at = ?4 WHERE id = ?5",
                            params![item.chapter_number, item.title, item.sort_index, now, id],
                        )?;
                    }
                    None => {
                        tx.execute(
                            "INSERT INTO chapters (id, book_id, chapter_number, title, \
                             sort_index, epub_index, created_at, updated_at) \
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
                            params![
                                repo::new_id("ch_"),
                                book_id,
                                item.chapter_number,
                                item.title,
                                item.sort_index,
                                item.epub_index,
                                now
                            ],
                        )?;
                    }
                }
            }
            // Untouched rows — audio with no matching document — land after
            // the reading order; audio is never deleted by a sync.
            let tail = documents.len() as i64;
            for (offset, id) in plan.keepers.iter().enumerate() {
                tx.execute(
                    "UPDATE chapters SET chapter_number = ?1, sort_index = ?2, updated_at = ?3 \
                     WHERE id = ?4",
                    params![tail + offset as i64 + 1, tail + offset as i64, now, id],
                )?;
            }
            tx.commit()?;

            recompute_book_progress(conn, &book_id)?;
            let chapters = load_chapter_infos(conn, &book_id)?;
            Ok(json!({
                "linked": plan.linked,
                "removed": plan.remove.len(),
                "chapters": chapters,
            }))
        })
    }

    /* -- audio map ------------------------------------------------------- */

    /// `GET /api/epub/:bookId/chapters/audio` — one entry per section with a
    /// chapter row, so the reader can label sections by what they can play.
    pub fn chapters_audio(&self, db: &Database, book_id: &str) -> ApiResult<Value> {
        let book_id = book_id.to_string();
        block_db(db, move |conn| {
            if !epub_ready(conn, &book_id)? {
                return Ok(json!({ "entries": [] }));
            }
            let mut statement = conn.prepare(
                "SELECT c.epub_index, c.id, a.id, a.duration, p.percentage, p.completed \
                 FROM chapters c \
                 LEFT JOIN chapter_audio a ON a.chapter_id = c.id \
                 LEFT JOIN listening_progress p ON p.chapter_id = c.id \
                 WHERE c.book_id = ?1 AND c.epub_index IS NOT NULL",
            )?;
            let entries: Vec<Value> = statement
                .query_map([&book_id], |row| {
                    let index: i64 = row.get(0)?;
                    let chapter_id: String = row.get(1)?;
                    let audio_id: Option<String> = row.get(2)?;
                    let duration: Option<f64> = row.get(3)?;
                    let percentage: Option<f64> = row.get(4)?;
                    let completed = row.get::<_, Option<i64>>(5)?.unwrap_or(0) != 0;
                    Ok(json!({
                        "index": index,
                        "chapterId": chapter_id,
                        "hasAudio": audio_id.is_some(),
                        "audioId": audio_id,
                        "duration": duration.unwrap_or(0.0),
                        "progress": if completed { 1.0 } else { percentage.unwrap_or(0.0) },
                        "completed": completed,
                    }))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(json!({ "entries": entries }))
        })
    }

    /* -- reading --------------------------------------------------------- */

    /// `GET /api/epub/:bookId/chapter/:index` — one section's sanitised HTML
    /// beside its siblings, the table of contents, and the book's metadata.
    pub fn chapter(&self, db: &Database, book_id: &str, index: i64) -> ApiResult<Value> {
        let book_id = book_id.to_string();
        block_db(db, move |conn| {
            let record: Option<(
                String,
                String,
                Option<String>,
                Option<String>,
                Option<String>,
            )> = conn
                .query_row(
                    "SELECT id, status, title, creator, language FROM epub_files \
                     WHERE book_id = ?1",
                    [&book_id],
                    |row| {
                        Ok((
                            row.get(0)?,
                            row.get(1)?,
                            row.get(2)?,
                            row.get(3)?,
                            row.get(4)?,
                        ))
                    },
                )
                .optional()?;
            let Some((epub_id, _status, title, creator, language)) =
                record.filter(|(_, status, ..)| status == "ready")
            else {
                return Err(ApiError::not_found("This book has no EPUB yet."));
            };

            let document: Option<(i64, String, String, i64, i64)> = conn
                .query_row(
                    "SELECT idx, title, html, word_count, depth FROM epub_chapters \
                     WHERE epub_id = ?1 AND idx = ?2",
                    params![epub_id, index],
                    |row| {
                        Ok((
                            row.get(0)?,
                            row.get(1)?,
                            row.get(2)?,
                            row.get(3)?,
                            row.get(4)?,
                        ))
                    },
                )
                .optional()?;
            let Some((idx, document_title, html, word_count, depth)) = document else {
                return Err(ApiError::not_found("That chapter does not exist."));
            };

            let chapters = chapter_entries(conn, &epub_id)?;
            let contents = contents_rows(conn, &epub_id)?;
            Ok(json!({
                "chapter": {
                    "index": idx,
                    "title": document_title,
                    "html": html,
                    "wordCount": word_count,
                    "depth": depth,
                },
                "chapters": chapters,
                "contents": contents,
                "meta": {
                    "title": title,
                    "creator": creator,
                    "language": language,
                    "chapterCount": chapters.as_array().map_or(0, Vec::len),
                },
            }))
        })
    }

    /// `POST /api/epub/:bookId/chapter/:index/ensure` — a chapter row for the
    /// section, so audio can be attached to it from the reader. Nothing is
    /// created until the user acts on a section.
    pub fn ensure_chapter(
        &self,
        db: &Database,
        book_id: &str,
        index: i64,
        title: Option<&str>,
    ) -> ApiResult<Value> {
        let book_id = book_id.to_string();
        let title = title.map(str::to_string);
        block_db(db, move |conn| {
            // The reader only attaches audio to something on the shelf...
            let linked: Option<String> = conn
                .query_row(
                    "SELECT id FROM library_books WHERE book_id = ?1",
                    [&book_id],
                    |row| row.get(0),
                )
                .optional()?;
            if linked.is_none() {
                return Err(ApiError::not_found(
                    "Add this book to your library to attach audio.",
                ));
            }

            // ...and only to a section the file actually has.
            let document: Option<String> = conn
                .query_row(
                    "SELECT c.title FROM epub_chapters c \
                     JOIN epub_files f ON c.epub_id = f.id \
                     WHERE f.book_id = ?1 AND f.status = 'ready' AND c.idx = ?2",
                    params![book_id, index],
                    |row| row.get(0),
                )
                .optional()?;
            let Some(document_title) = document else {
                return Err(ApiError::not_found("That section does not exist."));
            };
            let final_title = title.unwrap_or(document_title);

            let existing: Option<String> = conn
                .query_row(
                    "SELECT id FROM chapters WHERE book_id = ?1 AND epub_index = ?2",
                    params![book_id, index],
                    |row| row.get(0),
                )
                .optional()?;

            let chapter_id = match existing {
                Some(id) => id,
                None => {
                    let last: Option<i64> = conn.query_row(
                        "SELECT MAX(sort_index) FROM chapters WHERE book_id = ?1",
                        [&book_id],
                        |row| row.get(0),
                    )?;
                    let number = lowest_free_chapter_number(conn, &book_id)?;
                    let id = repo::new_id("ch_");
                    conn.execute(
                        "INSERT INTO chapters (id, book_id, chapter_number, title, sort_index, \
                         epub_index, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
                        params![
                            id,
                            book_id,
                            number,
                            chapter_title(&final_title, index),
                            last.map(|last| last + 1).unwrap_or(0),
                            index,
                            repo::now_iso()
                        ],
                    )?;
                    recompute_book_progress(conn, &book_id)?;
                    id
                }
            };

            let info = load_chapter_info(conn, &chapter_id)?.ok_or_else(|| {
                ApiError::internal("The chapter could not be read back after saving.")
            })?;
            serde_json::to_value(info).map_err(|error| ApiError::internal(error.to_string()))
        })
    }

    /* -- images ---------------------------------------------------------- */

    /// `GET /api/epub/:bookId/image/:ref` — one extracted image. Refs are
    /// content-addressed by import, so the bytes never change and the route
    /// caches aggressively.
    pub fn image(&self, db: &Database, book_id: &str, reference: &str) -> ApiResult<Response> {
        if !valid_image_ref(reference) {
            return Err(ApiError::not_found("That image does not exist."));
        }
        let (book_id, reference) = (book_id.to_string(), reference.to_string());
        let row: Option<(Vec<u8>, String)> = block_db(db, move |conn| {
            conn.query_row(
                "SELECT i.bytes, i.mime_type FROM epub_images i \
                 JOIN epub_files f ON i.epub_id = f.id \
                 WHERE f.book_id = ?1 AND i.ref = ?2",
                params![book_id, reference],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
        })?;
        let Some((bytes, mime)) = row else {
            return Err(ApiError::not_found("That image does not exist."));
        };
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, mime)
            .header(header::CONTENT_LENGTH, bytes.len())
            .header(header::CACHE_CONTROL, "private, max-age=31536000, immutable")
            .body(Body::from(bytes))
            .map_err(|error| ApiError::internal(format!("That image could not be sent: {error}")))
    }

    /* -- reading position ------------------------------------------------ */

    /// `GET /api/epub/:bookId/progress` — where the reader left off, and how
    /// the page is set. A book never opened answers with the smallest shape
    /// that still carries a position.
    pub fn progress(&self, db: &Database, book_id: &str) -> ApiResult<Value> {
        let book_id = book_id.to_string();
        block_db(db, move |conn| {
            match progress_row(conn, &book_id)? {
                Some(row) => Ok(row),
                None => Ok(json!({
                    "bookId": book_id,
                    "chapterIndex": 0,
                    "scrollPercent": 0,
                })),
            }
        })
    }

    /// `PATCH /api/epub/:bookId/progress` — save position and typography. The
    /// request is validated the way the web app's schema did, and the position
    /// is clamped on the way in as well as on the way out.
    pub fn save_progress(
        &self,
        db: &Database,
        book_id: &str,
        body: &Value,
    ) -> ApiResult<Value> {
        let fields = progress_fields(body)?;
        let book_id = book_id.to_string();
        block_db(db, move |conn| {
            if !epub_ready(conn, &book_id)? {
                return Err(ApiError::not_found("This book has no EPUB yet."));
            }

            let mut provided: Vec<(&'static str, SqlValue)> = Vec::new();
            let mut chapter_index: Option<i64> = None;
            for (key, value) in &fields {
                let column = progress_column(key).expect("only schema keys survive");
                let sql = match value {
                    Value::Number(number) => match number.as_i64() {
                        Some(integer) => SqlValue::Integer(integer),
                        None => SqlValue::Real(number.as_f64().unwrap_or(0.0)),
                    },
                    Value::String(text) => SqlValue::Text(text.clone()),
                    _ => SqlValue::Null,
                };
                if *key == "chapter_index" {
                    chapter_index = match sql {
                        SqlValue::Integer(integer) => Some(integer),
                        _ => None,
                    };
                }
                provided.push((column, sql));
            }
            if let Some(index) = chapter_index {
                let clamped = clamp_chapter_index(conn, &book_id, index)?;
                for (_, value) in provided.iter_mut() {
                    if *value == SqlValue::Integer(index) {
                        *value = SqlValue::Integer(clamped);
                    }
                }
            }

            let now = repo::now_iso();
            let has_row = conn
                .query_row(
                    "SELECT id FROM epub_progress WHERE book_id = ?1",
                    [&book_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
                .is_some();

            if has_row {
                let sets: Vec<String> = provided
                    .iter()
                    .enumerate()
                    .map(|(i, (column, _))| format!("{column} = ?{}", i + 1))
                    .collect();
                let mut all: Vec<SqlValue> = provided.into_iter().map(|(_, value)| value).collect();
                all.push(SqlValue::Text(now));
                all.push(SqlValue::Text(book_id.clone()));
                conn.execute(
                    &format!(
                        "UPDATE epub_progress SET {}, updated_at = ?{} WHERE book_id = ?{}",
                        sets.join(", "),
                        all.len() - 1,
                        all.len()
                    ),
                    params_from_iter(all.iter()),
                )?;
            } else {
                // `create` needs a position; every field the request omits
                // falls back to the column default rather than to nothing.
                // `id` has no default and SQLite lets a TEXT PRIMARY KEY stay
                // NULL, so binding it here is the only thing that makes the
                // row readable back — an unbound id saved silently and then
                // failed every read of the position.
                let mut columns: Vec<&str> = vec!["id", "book_id"];
                let mut all: Vec<SqlValue> = vec![
                    SqlValue::Text(repo::new_id("epg_")),
                    SqlValue::Text(book_id.clone()),
                ];
                if !provided.iter().any(|(column, _)| *column == "chapter_index") {
                    columns.push("chapter_index");
                    all.push(SqlValue::Integer(0));
                }
                if !provided.iter().any(|(column, _)| *column == "scroll_percent") {
                    columns.push("scroll_percent");
                    all.push(SqlValue::Real(0.0));
                }
                for (column, value) in provided {
                    columns.push(column);
                    all.push(value);
                }
                all.push(SqlValue::Text(now.clone()));
                all.push(SqlValue::Text(now));
                let placeholders: Vec<String> =
                    (1..=columns.len() + 2).map(|i| format!("?{i}")).collect();
                conn.execute(
                    &format!(
                        "INSERT INTO epub_progress ({}, created_at, updated_at) VALUES ({})",
                        columns.join(", "),
                        placeholders.join(", ")
                    ),
                    params_from_iter(all.iter()),
                )?;
            }

            progress_row(conn, &book_id)?.ok_or_else(|| {
                ApiError::internal("The reading position could not be read back after saving.")
            })
        })
    }
}

/* -------------------------------------------------------------------------- */
/* Archive on disk                                                            */
/* -------------------------------------------------------------------------- */

fn archive_path(cfg: &Config, book_id: &str) -> PathBuf {
    cfg.epub_dir.join(format!("{book_id}.epub"))
}

async fn read_archive(cfg: &Config, book_id: &str) -> ApiResult<Option<Vec<u8>>> {
    let path = archive_path(cfg, book_id);
    tokio::task::spawn_blocking(move || match std::fs::read(&path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(ApiError::internal(format!(
            "The stored book file could not be read: {error}"
        ))),
    })
    .await
    .map_err(|error| ApiError::internal(format!("The stored book file could not be read: {error}")))?
}

async fn save_archive(cfg: &Config, book_id: &str, buffer: Bytes) -> ApiResult<()> {
    let dir = cfg.epub_dir.clone();
    let path = archive_path(cfg, book_id);
    tokio::task::spawn_blocking(move || {
        std::fs::create_dir_all(&dir)
            .and_then(|()| std::fs::write(&path, &buffer))
            .map_err(|error| {
                ApiError::internal(format!("The book file could not be stored: {error}"))
            })
    })
    .await
    .map_err(|error| ApiError::internal(format!("The book file could not be stored: {error}")))?
}

async fn delete_archive(cfg: &Config, book_id: &str) -> ApiResult<()> {
    let path = archive_path(cfg, book_id);
    tokio::task::spawn_blocking(move || match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(ApiError::internal(format!(
            "The stored book file could not be removed: {error}"
        ))),
    })
    .await
    .map_err(|error| ApiError::internal(format!("The stored book file could not be removed: {error}")))?
}

/// Parse on the blocking pool — a container can be a couple of hundred
/// megabytes, and the web app's `parseEpub` was plain synchronous work.
async fn parse_blocking(
    buffer: Bytes,
    prefix: String,
    max_image_bytes: u64,
    max_documents: usize,
) -> Result<ParsedEpub, String> {
    tokio::task::spawn_blocking(move || parse(&buffer, &prefix, max_image_bytes, max_documents))
        .await
        .map_err(|error| {
            tracing::error!(%error, "epub parse task failed");
            "The book file could not be parsed.".to_string()
        })
        .and_then(|parsed| parsed.map_err(|error| error.to_string()))
}

/* -------------------------------------------------------------------------- */
/* Database helpers (the Prisma calls in `db/epub.ts`)                       */
/* -------------------------------------------------------------------------- */

/// What to do with each `epub_files` column in an upsert. `None` leaves the
/// column alone; `Some(None)` clears it — Prisma writes only the keys a call
/// passed, and a passed `null` meant null.
#[derive(Default)]
struct RecordPatch {
    error: Option<Option<String>>,
    source: Option<String>,
    md5: Option<Option<String>>,
    file_name: Option<String>,
    file_size: Option<i64>,
    stored_at: Option<String>,
    title: Option<String>,
    creator: Option<String>,
    language: Option<Option<String>>,
}

/// `upsertEpub` — create or update the book's file row, and hand back its id.
fn upsert_record(
    conn: &rusqlite::Connection,
    book_id: &str,
    status: &str,
    patch: RecordPatch,
) -> ApiResult<String> {
    let mut columns: Vec<&str> = vec!["status"];
    let mut values: Vec<SqlValue> = vec![SqlValue::Text(status.to_string())];
    if let Some(error) = patch.error {
        columns.push("error");
        values.push(optionally(error));
    }
    if let Some(source) = patch.source {
        columns.push("source");
        values.push(SqlValue::Text(source));
    }
    if let Some(md5) = patch.md5 {
        columns.push("md5");
        values.push(optionally(md5));
    }
    if let Some(file_name) = patch.file_name {
        columns.push("file_name");
        values.push(SqlValue::Text(file_name));
    }
    if let Some(file_size) = patch.file_size {
        columns.push("file_size");
        values.push(SqlValue::Integer(file_size));
    }
    if let Some(stored_at) = patch.stored_at {
        columns.push("stored_at");
        values.push(SqlValue::Text(stored_at));
    }
    if let Some(title) = patch.title {
        columns.push("title");
        values.push(SqlValue::Text(title));
    }
    if let Some(creator) = patch.creator {
        columns.push("creator");
        values.push(SqlValue::Text(creator));
    }
    if let Some(language) = patch.language {
        columns.push("language");
        values.push(optionally(language));
    }

    let now = repo::now_iso();
    let existing: Option<String> = conn
        .query_row(
            "SELECT id FROM epub_files WHERE book_id = ?1",
            [book_id],
            |row| row.get(0),
        )
        .optional()?;

    match existing {
        Some(id) => {
            let mut all: Vec<SqlValue> = values;
            all.push(SqlValue::Text(now));
            all.push(SqlValue::Text(book_id.to_string()));
            let sets: Vec<String> = columns
                .iter()
                .enumerate()
                .map(|(i, column)| format!("{column} = ?{}", i + 1))
                .collect();
            let (updated, book) = (all.len() - 1, all.len());
            conn.execute(
                &format!(
                    "UPDATE epub_files SET {}, updated_at = ?{updated} WHERE book_id = ?{book}",
                    sets.join(", ")
                ),
                params_from_iter(all.iter()),
            )?;
            Ok(id)
        }
        None => {
            let id = repo::new_id("ep_");
            // `id` has no default and `book_id` is NOT NULL, so both must be
            // bound here in exactly the order the column list names them —
            // omitting either shifts every parameter one place and the first
            // upload for a book that has never held an EPUB dies on the
            // constraint instead of creating its row.
            // Order matters: id, book_id, then the patch columns, then the
            // two timestamps — exactly as the column list below names them.
            let mut all: Vec<SqlValue> = vec![
                SqlValue::Text(id.clone()),
                SqlValue::Text(book_id.to_string()),
            ];
            all.extend(values);
            all.push(SqlValue::Text(now.clone()));
            all.push(SqlValue::Text(now));
            let placeholders: Vec<String> =
                (1..=columns.len() + 4).map(|i| format!("?{i}")).collect();
            conn.execute(
                &format!(
                    "INSERT INTO epub_files (id, book_id, {}, created_at, updated_at) VALUES ({})",
                    columns.join(", "),
                    placeholders.join(", ")
                ),
                params_from_iter(all.iter()),
            )?;
            Ok(id)
        }
    }
}

fn optionally(value: Option<String>) -> SqlValue {
    value.map_or(SqlValue::Null, SqlValue::Text)
}

/// `replaceEpubChapters` — the extracted reading order and the table of
/// contents, written together: one cannot be right without the other.
fn replace_chapters(
    conn: &rusqlite::Connection,
    epub_id: &str,
    chapters: &[ParsedChapter],
    contents_json: &str,
) -> ApiResult<usize> {
    conn.execute("DELETE FROM epub_chapters WHERE epub_id = ?1", [epub_id])?;
    {
        let mut statement = conn.prepare(
            "INSERT INTO epub_chapters (id, epub_id, idx, href, title, depth, in_toc, \
             word_count, char_offset, html) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        )?;
        for chapter in chapters {
            statement.execute(params![
                repo::new_id("ec_"),
                epub_id,
                chapter.index as i64,
                chapter.href,
                chapter.title,
                chapter.depth,
                chapter.in_toc as i64,
                chapter.word_count,
                chapter.char_offset,
                chapter.html,
            ])?;
        }
    }
    conn.execute(
        "UPDATE epub_files SET contents = ?1, updated_at = ?2 WHERE id = ?3",
        params![contents_json, repo::now_iso(), epub_id],
    )?;
    Ok(chapters.len())
}

/// `replaceEpubImages` — one row per extracted image.
fn replace_images(
    conn: &rusqlite::Connection,
    epub_id: &str,
    images: &[ParsedImage],
) -> ApiResult<usize> {
    conn.execute("DELETE FROM epub_images WHERE epub_id = ?1", [epub_id])?;
    if images.is_empty() {
        return Ok(0);
    }
    let mut statement = conn.prepare(
        "INSERT INTO epub_images (id, epub_id, ref, mime_type, alt, bytes) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?;
    for image in images {
        statement.execute(params![
            repo::new_id("ei_"),
            epub_id,
            image.ref_id,
            image.mime_type,
            image.alt,
            image.bytes,
        ])?;
    }
    Ok(images.len())
}

/* -------------------------------------------------------------------------- */
/* Store paths                                                                */
/* -------------------------------------------------------------------------- */

impl EpubService {
    /// `storeEpubArchive`: the container goes to disk first — a parse that
    /// succeeded but a disk that failed must not leave derived rows without
    /// their source — then the row, then everything the reader will load.
    async fn store_parsed(
        &self,
        db: &Database,
        book_id: &str,
        buffer: &Bytes,
        parsed: ParsedEpub,
        file_name: String,
        title_fallback: String,
        creator_fallback: String,
    ) -> ApiResult<i64> {
        save_archive(&self.cfg, book_id, buffer.clone()).await?;

        let contents_json = contents_json(&parsed.contents);
        let file_size = buffer.len() as i64;
        let parsed = parsed.clone();
        let book_id = book_id.to_string();
        db.call(move |conn| {
            let tx = conn.unchecked_transaction()?;
            let patch = RecordPatch {
                error: Some(None),
                source: Some("upload".to_string()),
                md5: Some(None),
                file_name: Some(file_name),
                file_size: Some(file_size),
                stored_at: Some(repo::now_iso()),
                title: Some(parsed.title.clone().unwrap_or(title_fallback)),
                creator: Some(parsed.creator.clone().unwrap_or(creator_fallback)),
                // The parser always answers; `null` clears what an older file
                // said.
                language: Some(parsed.language.clone()),
            };
            let epub_id = upsert_record(&tx, &book_id, "ready", patch)?;
            let count = replace_chapters(&tx, &epub_id, &parsed.chapters, &contents_json)?;
            replace_images(&tx, &epub_id, &parsed.images)?;
            // A stored file replaces whatever was here, including the reading
            // position — the same index can now point at different text.
            // Reading preferences are kept.
            tx.execute(
                "UPDATE epub_progress SET chapter_index = 0, scroll_percent = 0, updated_at = ?1 \
                 WHERE book_id = ?2",
                params![repo::now_iso(), book_id],
            )?;
            tx.commit()?;
            Ok(count as i64)
        })
        .await
    }

    /// `reparseEpub`: the container stays as it is; the extracted text and
    /// metadata are rebuilt from it. Metadata is refreshed only when the parse
    /// has something to say.
    async fn reparse_store(
        &self,
        db: &Database,
        book_id: &str,
        epub_id: &str,
        parsed: &ParsedEpub,
    ) -> ApiResult<i64> {
        let contents_json = contents_json(&parsed.contents);
        let (book_id, epub_id) = (book_id.to_string(), epub_id.to_string());
        let parsed = parsed.clone();
        db.call(move |conn| {
            let tx = conn.unchecked_transaction()?;
            let count = replace_chapters(&tx, &epub_id, &parsed.chapters, &contents_json)?;
            replace_images(&tx, &epub_id, &parsed.images)?;
            let patch = RecordPatch {
                error: Some(None),
                title: non_empty(parsed.title.clone()),
                creator: non_empty(parsed.creator.clone()),
                language: non_empty(parsed.language.clone()).map(Some),
                ..RecordPatch::default()
            };
            upsert_record(&tx, &book_id, "ready", patch)?;
            tx.commit()?;
            Ok(count as i64)
        })
        .await
    }
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

pub(crate) fn size_limit_message(max_bytes: u64) -> String {
    format!(
        "That file is larger than the {} MB limit.",
        (max_bytes as f64 / (1024.0 * 1024.0)).round()
    )
}

/* -------------------------------------------------------------------------- */
/* Chapter bridging (`epub-chapters.ts`)                                      */
/* -------------------------------------------------------------------------- */

/// An existing chapter as the planner sees it.
struct ExistingRow {
    id: String,
    epub_index: Option<i64>,
    has_audio: bool,
}

/// One upsert the plan asks for.
struct ChapterUpsert {
    /// `Some(id)` when the section already has a chapter; `None` to create.
    id: Option<String>,
    epub_index: i64,
    chapter_number: i64,
    title: String,
    sort_index: i64,
}

/// What a sync will do, decided before anything is written.
struct ChapterPlan {
    remove: Vec<String>,
    upserts: Vec<ChapterUpsert>,
    keepers: Vec<String>,
    park: Vec<String>,
    linked: usize,
}

/// `planChapterSync` — the rules, in full:
///
///  - audio is never deleted; a section that still owns audio is kept, parked
///    after the reading order;
///  - a chapter already joined to a section is updated in place, so its audio
///    and progress come along;
///  - sections are numbered 1..n in reading order; keepers take the numbers
///    after them.
fn plan_chapter_sync(documents: &[(i64, String)], existing: &[ExistingRow]) -> ChapterPlan {
    // First chapter for a section wins; a duplicate join is a leftover, not a
    // second claim.
    let mut by_index: Vec<(i64, usize)> = Vec::new();
    let mut remove: Vec<usize> = Vec::new();
    let mut keepers: Vec<usize> = Vec::new();

    for (position, row) in existing.iter().enumerate() {
        if let Some(index) = row.epub_index {
            if !by_index.iter().any(|(known, _)| *known == index) {
                by_index.push((index, position));
                continue;
            }
        }
        if row.has_audio {
            keepers.push(position);
        } else {
            remove.push(position);
        }
    }

    let upserts: Vec<ChapterUpsert> = documents
        .iter()
        .enumerate()
        .map(|(position, (index, title))| ChapterUpsert {
            id: by_index
                .iter()
                .find(|(known, _)| known == index)
                .map(|(_, position)| existing[*position].id.clone()),
            epub_index: *index,
            chapter_number: position as i64 + 1,
            title: chapter_title(title, *index),
            sort_index: position as i64,
        })
        .collect();

    let park: Vec<String> = by_index
        .iter()
        .map(|(_, position)| existing[*position].id.clone())
        .chain(keepers.iter().map(|position| existing[*position].id.clone()))
        .collect();
    let remove: Vec<String> = remove
        .into_iter()
        .map(|position| existing[position].id.clone())
        .collect();
    let keepers: Vec<String> = keepers
        .into_iter()
        .map(|position| existing[position].id.clone())
        .collect();
    let linked = by_index.len();

    ChapterPlan {
        remove,
        upserts,
        keepers,
        park,
        linked,
    }
}

/// `chapterTitle` — trim, then cut to the title limit on a UTF-16 boundary
/// (JavaScript slices by code unit); a blank result falls back to the spine
/// position.
fn chapter_title(raw: &str, epub_index: i64) -> String {
    let trimmed = truncate_utf16(raw.trim(), TITLE_LIMIT);
    if trimmed.is_empty() {
        format!("Section {}", epub_index + 1)
    } else {
        trimmed.to_string()
    }
}

/// `truncate_utf16` — cut to at most `limit` UTF-16 code units, never through
/// the middle of a surrogate pair.
fn truncate_utf16(value: &str, limit: usize) -> &str {
    let mut units = 0;
    for (offset, character) in value.char_indices() {
        let width = character.len_utf16();
        if units + width > limit {
            return &value[..offset];
        }
        units += width;
    }
    value
}

/// `lowestFreeChapterNumber` — the lowest positive number no chapter holds.
fn lowest_free_chapter_number(conn: &rusqlite::Connection, book_id: &str) -> rusqlite::Result<i64> {
    let mut taken = HashSet::new();
    {
        let mut statement = conn.prepare("SELECT chapter_number FROM chapters WHERE book_id = ?1")?;
        let rows = statement.query_map([book_id], |row| row.get::<_, i64>(0))?;
        for number in rows {
            taken.insert(number?);
        }
    }
    let mut candidate = 1;
    while taken.contains(&candidate) {
        candidate += 1;
    }
    Ok(candidate)
}

/* -------------------------------------------------------------------------- */
/* Payload helpers                                                            */
/* -------------------------------------------------------------------------- */

fn epub_ready(conn: &rusqlite::Connection, book_id: &str) -> ApiResult<bool> {
    let status: Option<String> = conn
        .query_row(
            "SELECT status FROM epub_files WHERE book_id = ?1",
            [book_id],
            |row| row.get(0),
        )
        .optional()?;
    Ok(status.as_deref() == Some("ready"))
}

/// `getEpubChapters` — every document beside its chapter row, so the reader
/// can label a section as joinable.
fn chapter_entries(conn: &rusqlite::Connection, epub_id: &str) -> ApiResult<Value> {
    let mut statement = conn.prepare(
        "SELECT id, idx, title, depth, in_toc, word_count, char_offset FROM epub_chapters \
         WHERE epub_id = ?1 ORDER BY idx ASC",
    )?;
    let entries: Vec<Value> = statement
        .query_map([epub_id], |row| {
            Ok(json!({
                "id": row.get::<_, String>(0)?,
                "index": row.get::<_, i64>(1)?,
                "title": row.get::<_, String>(2)?,
                "depth": row.get::<_, i64>(3)?,
                "inToc": row.get::<_, i64>(4)? != 0,
                "wordCount": row.get::<_, i64>(5)?,
                "charOffset": row.get::<_, i64>(6)?,
            }))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Value::Array(entries))
}

/// `parseContents` — the table of contents stored with the import, kept only
/// where it still points at a real section and carries real text; a chapter's
/// own heading stands in when nothing survives.
fn contents_rows(conn: &rusqlite::Connection, epub_id: &str) -> ApiResult<Value> {
    let stored: Option<String> = conn
        .query_row(
            "SELECT contents FROM epub_files WHERE id = ?1",
            [epub_id],
            |row| row.get(0),
        )
        .optional()?
        .flatten();
    let known: HashSet<i64> = {
        let mut statement = conn.prepare("SELECT idx FROM epub_chapters WHERE epub_id = ?1")?;
        let rows = statement.query_map([epub_id], |row| row.get::<_, i64>(0))?;
        let mut set = HashSet::new();
        for index in rows {
            set.insert(index?);
        }
        set
    };

    if let Some(stored) = stored {
        if let Ok(Value::Array(rows)) = serde_json::from_str::<Value>(&stored) {
            let filtered: Vec<Value> = rows
                .into_iter()
                .filter(|row| {
                    let Some(object) = row.as_object() else {
                        return false;
                    };
                    let Some(index) = object.get("index").and_then(Value::as_f64) else {
                        return false;
                    };
                    if !object
                        .get("title")
                        .map(Value::is_string)
                        .unwrap_or(false)
                        || !object
                            .get("offset")
                            .map(Value::is_number)
                            .unwrap_or(false)
                    {
                        return false;
                    }
                    index.fract() == 0.0 && known.contains(&(index as i64))
                })
                .collect();
            if !filtered.is_empty() {
                return Ok(Value::Array(filtered));
            }
        }
    }

    // The fallback: each chapter's own heading, which is always true.
    let mut statement = conn.prepare(
        "SELECT idx, title, depth FROM epub_chapters WHERE epub_id = ?1 ORDER BY idx ASC",
    )?;
    let rows = statement.query_map([epub_id], |row| {
        Ok(json!({
            "index": row.get::<_, i64>(0)?,
            "anchor": "",
            "offset": 0,
            "title": row.get::<_, String>(1)?,
            "depth": row.get::<_, i64>(2)?,
        }))
    })?;
    Ok(Value::Array(rows.collect::<Result<Vec<_>, _>>()?))
}

/// The stored reading position, clamped on the way out.
fn progress_row(conn: &rusqlite::Connection, book_id: &str) -> ApiResult<Option<Value>> {
    let row: Option<(
        String,
        String,
        i64,
        f64,
        i64,
        f64,
        String,
        String,
        i64,
        String,
        String,
    )> = conn
        .query_row(
            "SELECT id, book_id, chapter_index, scroll_percent, font_size, line_height, theme, \
                    font_family, column_width, created_at, updated_at \
             FROM epub_progress WHERE book_id = ?1",
            [book_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                    row.get(9)?,
                    row.get(10)?,
                ))
            },
        )
        .optional()?;
    let Some((
        id,
        book,
        chapter_index,
        scroll_percent,
        font_size,
        line_height,
        theme,
        font_family,
        column_width,
        created_at,
        updated_at,
    )) = row
    else {
        return Ok(None);
    };
    Ok(Some(json!({
        "id": id,
        "bookId": book,
        "chapterIndex": clamp_chapter_index(conn, book_id, chapter_index)?,
        "scrollPercent": scroll_percent,
        "fontSize": font_size,
        "lineHeight": line_height,
        "theme": theme,
        "fontFamily": font_family,
        "columnWidth": column_width,
        "createdAt": created_at,
        "updatedAt": updated_at,
    })))
}

/// `clampChapterIndex` — a stored position that outlived its book comes home
/// to the nearest section that exists; there is no book or no section, it is 0.
fn clamp_chapter_index(
    conn: &rusqlite::Connection,
    book_id: &str,
    index: i64,
) -> rusqlite::Result<i64> {
    let last: Option<i64> = conn.query_row(
        "SELECT MAX(c.idx) FROM epub_chapters c \
         JOIN epub_files f ON c.epub_id = f.id WHERE f.book_id = ?1",
        [book_id],
        |row| row.get(0),
    )?;
    let Some(last) = last else {
        return Ok(0);
    };
    Ok(index.clamp(0, last))
}

/// `epubProgressSchema` — only these keys, only these shapes; unknown keys are
/// dropped rather than rejected, exactly as the schema's default did. Returns
/// the surviving fields as `(snake_case_column, value)` pairs.
fn progress_fields(body: &Value) -> ApiResult<Vec<(&'static str, Value)>> {
    let Some(object) = body.as_object() else {
        return Err(ApiError::invalid(
            "body",
            "Invalid input: expected an object, received nothing",
        ));
    };

    let mut fields: Vec<(&'static str, Value)> = Vec::new();
    for (key, value) in object {
        match key.as_str() {
            "chapterIndex" => {
                fields.push(("chapter_index", integer_in(value, 0, 100_000, "chapterIndex")?));
            }
            "scrollPercent" => {
                fields.push((
                    "scroll_percent",
                    number_in(value, 0.0, 1.0, "scrollPercent")?,
                ));
            }
            "fontSize" => {
                fields.push(("font_size", integer_in(value, 12, 34, "fontSize")?));
            }
            "lineHeight" => {
                fields.push((
                    "line_height",
                    number_in(value, 1.1, 2.6, "lineHeight")?,
                ));
            }
            "theme" => {
                fields.push((
                    "theme",
                    one_of(value, &["paper", "sepia", "night", "ink"], "theme")?,
                ));
            }
            "fontFamily" => {
                fields.push((
                    "font_family",
                    one_of(value, &["serif", "sans", "mono"], "fontFamily")?,
                ));
            }
            "columnWidth" => {
                fields.push(("column_width", integer_in(value, 360, 1200, "columnWidth")?));
            }
            _ => {}
        }
    }
    Ok(fields)
}

fn progress_column(key: &str) -> Option<&'static str> {
    match key {
        "chapter_index" => Some("chapter_index"),
        "scroll_percent" => Some("scroll_percent"),
        "font_size" => Some("font_size"),
        "line_height" => Some("line_height"),
        "theme" => Some("theme"),
        "font_family" => Some("font_family"),
        "column_width" => Some("column_width"),
        _ => None,
    }
}

fn number_in(value: &Value, min: f64, max: f64, key: &str) -> ApiResult<Value> {
    let number = value.as_f64().ok_or_else(|| {
        ApiError::invalid(
            key,
            format!("Invalid input: expected number, received {}", kind(value)),
        )
    })?;
    if !number.is_finite() || !(min..=max).contains(&number) {
        return Err(ApiError::invalid(
            key,
            format!("Invalid input: number must be between {min} and {max}"),
        ));
    }
    Ok(json!(number))
}

fn integer_in(value: &Value, min: i64, max: i64, key: &str) -> ApiResult<Value> {
    let number = value.as_f64().ok_or_else(|| {
        ApiError::invalid(
            key,
            format!("Invalid input: expected number, received {}", kind(value)),
        )
    })?;
    if number.fract() != 0.0 {
        return Err(ApiError::invalid(
            key,
            "Invalid input: expected integer, received number with fractional part",
        ));
    }
    let integer = number as i64;
    if !(min..=max).contains(&integer) {
        return Err(ApiError::invalid(
            key,
            format!("Invalid input: number must be between {min} and {max}"),
        ));
    }
    Ok(json!(integer))
}

fn one_of<'a>(value: &Value, options: &[&'a str], key: &str) -> ApiResult<Value> {
    let text = value.as_str().ok_or_else(|| {
        ApiError::invalid(
            key,
            format!("Invalid input: expected string, received {}", kind(value)),
        )
    })?;
    if !options.contains(&text) {
        return Err(ApiError::invalid(
            key,
            format!("Invalid input: expected one of {}", options.join(", ")),
        ));
    }
    Ok(json!(text))
}

fn kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/* -------------------------------------------------------------------------- */
/* Images                                                                     */
/* -------------------------------------------------------------------------- */

/// `^img[0-9a-z]{1,8}$` with the flag the importer set — anything else is a
/// probe, not a page.
fn valid_image_ref(reference: &str) -> bool {
    let rest = match reference.get(..3) {
        Some(prefix) if prefix.eq_ignore_ascii_case("img") => &reference[3..],
        _ => return false,
    };
    let length = rest.chars().count();
    (1..=8).contains(&length) && rest.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

/* -------------------------------------------------------------------------- */
/* Contents JSON                                                              */
/* -------------------------------------------------------------------------- */

fn contents_json(contents: &[ContentsRow]) -> String {
    let rows: Vec<Value> = contents
        .iter()
        .map(|row| {
            json!({
                "index": row.index,
                "anchor": row.anchor,
                "offset": row.offset,
                "title": row.title,
                "depth": row.depth,
            })
        })
        .collect();
    serde_json::to_string(&rows).unwrap_or_else(|_| "[]".to_string())
}

/* -------------------------------------------------------------------------- */
/* Tests                                                                      */
/* -------------------------------------------------------------------------- */

#[cfg(test)]
mod tests {
    use super::*;

    /* -- `save_progress` / `progress` -- */

    /// A saved reading position must come back. The INSERT binds `id` — SQLite
    /// lets a TEXT PRIMARY KEY stay NULL, which saved silently and then made
    /// every read of the position fail.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_saved_reading_position_reads_back() {
        let dir = tempfile::tempdir().expect("temp dir");
        let db = crate::db::Database::open(&dir.path().join("vb.db")).expect("database");
        let cfg = Config::for_data_dir(dir.path().to_path_buf());
        let service = EpubService::new(&cfg);

        db.call(|conn| {
            let now = repo::now_iso();
            conn.execute(
                "INSERT INTO books (id, external_id, title, created_at, updated_at) \
                 VALUES ('bk_pos', 'ext-pos', 'A Book', ?1, ?1)",
                [&now],
            )?;
            conn.execute(
                "INSERT INTO epub_files (id, book_id, status, created_at, updated_at) \
                 VALUES ('ep_pos', 'bk_pos', 'ready', ?1, ?1)",
                [&now],
            )?;
            for (idx, title) in [(0, "One"), (1, "Two")] {
                conn.execute(
                    "INSERT INTO epub_chapters (id, epub_id, idx, href, title, html) \
                     VALUES (?1, 'ep_pos', ?2, ?3, ?4, '<p>x</p>')",
                    rusqlite::params![format!("ec_pos{idx}"), idx, format!("{idx}.xhtml"), title],
                )?;
            }
            Ok::<_, rusqlite::Error>(())
        })
        .await
        .expect("seed");

        // An index the book does not have is clamped on the way in, and the
        // saved row comes back with a real id.
        let saved = service
            .save_progress(
                &db,
                "bk_pos",
                &json!({ "chapterIndex": 99_000, "scrollPercent": 0.5 }),
            )
            .expect("save");
        assert_eq!(saved["chapterIndex"], 1);
        assert_eq!(saved["scrollPercent"], 0.5);
        let id = saved["id"].as_str().expect("a bound id, not null");
        assert!(id.starts_with("epg_"));

        // What GET reads is what PATCH wrote.
        let restored = service.progress(&db, "bk_pos").expect("read back");
        assert_eq!(restored["id"], saved["id"]);
        assert_eq!(restored["chapterIndex"], 1);
        assert_eq!(restored["scrollPercent"], 0.5);

        // A second save takes the UPDATE branch and keeps the same row.
        let updated = service
            .save_progress(&db, "bk_pos", &json!({ "fontSize": 22 }))
            .expect("update");
        assert_eq!(updated["id"], saved["id"]);
        assert_eq!(updated["fontSize"], 22);
        assert_eq!(updated["scrollPercent"], 0.5);
    }

    /* -- `upsert_record` -- */

    /// A store-imported book has never held an EPUB, so its first upload takes
    /// the INSERT branch: `id` and the NOT NULL `book_id` must both be bound in
    /// the order the column list names them. When they were not, every fresh
    /// upload failed the constraint and surfaced as a 500.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_fresh_row_is_created_with_its_book_id() {
        let dir = tempfile::tempdir().expect("temp dir");
        let db = crate::db::Database::open(&dir.path().join("vb.db")).expect("database");

        let created = db
            .call({
                let now = repo::now_iso();
                move |conn| {
                    conn.execute(
                        "INSERT INTO books (id, external_id, title, created_at, updated_at) \
                         VALUES ('bk_fresh', 'ext-fresh', 'A Book', ?1, ?1)",
                        [&now],
                    )?;
                    upsert_record(conn, "bk_fresh", "parsing", RecordPatch::default())
                }
            })
            .await
            .expect("fresh insert");
        assert!(!created.is_empty());

        let row = db
            .call(|conn| {
                conn.query_row(
                    "SELECT id, book_id, status FROM epub_files WHERE book_id = 'bk_fresh'",
                    [],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    },
                )
            })
            .await
            .expect("row readable");
        assert_eq!(row.0, created);
        assert_eq!(row.1, "bk_fresh");
        assert_eq!(row.2, "parsing");

        // The second upsert takes the UPDATE branch and must not duplicate.
        db.call(move |conn| upsert_record(conn, "bk_fresh", "ready", RecordPatch::default()))
            .await
            .expect("second upsert");
        let count = db
            .call(|conn| {
                conn.query_row("SELECT COUNT(*) FROM epub_files", [], |row| row.get::<_, i64>(0))
            })
            .await
            .expect("count");
        assert_eq!(count, 1);
    }

    fn existing(id: &str, epub_index: Option<i64>, has_audio: bool) -> ExistingRow {
        ExistingRow {
            id: id.to_string(),
            epub_index,
            has_audio,
        }
    }

    fn documents(items: &[(i64, &str)]) -> Vec<(i64, String)> {
        items
            .iter()
            .map(|(index, title)| (*index, title.to_string()))
            .collect()
    }

    /* -- `plan_chapters_sync` -- */

    #[test]
    fn unclaimed_sections_claim_chapters_in_reading_order() {
        let docs = documents(&[(0, "One"), (2, "Three")]);
        let plan = plan_chapter_sync(
            &docs,
            &[
                existing("lb_1", None, false),
                existing("lb_2", Some(2), false),
                existing("lb_3", None, true),
            ],
        );

        // Only `lb_2` was joined to a section the new spine still has; the
        // unlinked rows claim nothing.
        assert_eq!(plan.linked, 1);
        assert_eq!(plan.remove, ["lb_1"]);
        assert_eq!(plan.keepers, ["lb_3"]);
        // `lb_2` was already section 2 and keeps its row; section 1 is new.
        assert_eq!(plan.upserts[0].id.as_deref(), None);
        assert_eq!(plan.upserts[1].id.as_deref(), Some("lb_2"));
        assert_eq!(plan.upserts[1].chapter_number, 2);
        // Park every surviving row before anything is written.
        let mut parked = plan.park.clone();
        parked.sort();
        assert_eq!(parked, ["lb_2", "lb_3"]);
    }

    #[test]
    fn sync_numbers_a_spine_and_parks_every_surviving_row() {
        let docs = documents(&[(0, "One"), (1, "Two"), (2, "Three")]);
        let existing = vec![
            existing("lb_a", Some(0), false),
            existing("lb_b", None, true),
        ];
        let plan = plan_chapter_sync(&docs, &existing);

        assert_eq!(plan.linked, 1);
        assert!(plan.remove.is_empty());
        assert_eq!(plan.keepers, ["lb_b"]);
        assert_eq!(plan.upserts.len(), 3);
        assert_eq!(plan.upserts[0].id.as_deref(), Some("lb_a"));
        assert_eq!(plan.upserts[0].chapter_number, 1);
        assert_eq!(plan.upserts[1].id, None);
        assert_eq!(plan.upserts[2].id, None);
        let mut parked = plan.park.clone();
        parked.sort();
        assert_eq!(parked, ["lb_a", "lb_b"]);
    }

    #[test]
    fn a_row_whose_section_vanished_is_parked_and_left_alone() {
        // The web app's planner parks a chapter whose section no longer exists
        // and never renumbers or removes it — audio is never destroyed by a
        // sync, even when its section is gone.
        let docs = documents(&[(0, "One")]);
        let plan = plan_chapter_sync(&docs, &[existing("lb_old", Some(9), true)]);

        assert_eq!(plan.linked, 1);
        assert_eq!(plan.upserts[0].id, None);
        assert_eq!(plan.park, ["lb_old"]);
        assert!(plan.keepers.is_empty());
        assert!(plan.remove.is_empty());
    }

    #[test]
    fn a_duplicate_join_is_a_leftover_unless_it_still_owns_audio() {
        let docs = documents(&[(0, "One")]);
        let existing = vec![
            existing("lb_first", Some(0), false),
            existing("lb_leftover", Some(0), false),
            existing("lb_second", Some(0), true),
        ];
        let plan = plan_chapter_sync(&docs, &existing);

        // The first join wins; both later rows wait for their own chapter.
        assert_eq!(plan.upserts[0].id.as_deref(), Some("lb_first"));
        assert_eq!(plan.remove, ["lb_leftover"]);
        assert_eq!(plan.keepers, ["lb_second"]);
    }

    #[test]
    fn an_unjoined_row_with_audio_is_kept_not_rejoined() {
        let docs = documents(&[(0, "One")]);
        let existing = vec![existing("lb_audio", None, true)];
        let plan = plan_chapter_sync(&docs, &existing);

        // Audio is never deleted and never silently attached to a section it
        // did not come from: it waits past the reading order.
        assert_eq!(plan.upserts[0].id, None);
        assert_eq!(plan.keepers, ["lb_audio"]);
        assert!(plan.remove.is_empty());
    }

    /* -- `chapter_title` -- */

    #[test]
    fn a_title_is_trimmed_then_cut_to_the_limit() {
        assert_eq!(chapter_title("  The Tale  ", 0), "The Tale");
        assert_eq!(chapter_title(&"x".repeat(1_000), 0).len(), 300);
        // A cut never lands inside a surrogate pair.
        let astral = format!("{}{}", "a".repeat(TITLE_LIMIT - 1), "\u{1F600}");
        assert_eq!(
            chapter_title(&astral, 0).encode_utf16().count(),
            TITLE_LIMIT - 1
        );
    }

    #[test]
    fn a_blank_title_falls_back_to_the_spine_position() {
        assert_eq!(chapter_title("   ", 4), "Section 5");
        assert_eq!(chapter_title("", 0), "Section 1");
    }

    /* -- `truncate_utf16` -- */

    #[test]
    fn truncation_counts_utf16_units() {
        assert_eq!(truncate_utf16("hello", 3), "hel");
        assert_eq!(truncate_utf16("a\u{1F600}b", 2), "a");
        assert_eq!(truncate_utf16("a\u{1F600}b", 3), "a\u{1F600}");
        assert_eq!(truncate_utf16("short", 100), "short");
    }

    /* -- `valid_image_ref` -- */

    #[test]
    fn image_refs_are_importer_made_or_they_are_probes() {
        assert!(valid_image_ref("img1"));
        assert!(valid_image_ref("IMGa1b2"));
        assert!(valid_image_ref("img01234567"));
        assert!(!valid_image_ref("img"));
        assert!(!valid_image_ref("img012345678"));
        assert!(!valid_image_ref("image1"));
        assert!(!valid_image_ref("img-1"));
        assert!(!valid_image_ref("img\u{1F600}"));
    }

    /* -- `progress_fields` -- */

    #[test]
    fn progress_accepts_only_the_schema_keys() {
        let fields = progress_fields(&json!({
            "chapterIndex": 3,
            "scrollPercent": 0.5,
            "fontSize": 20,
            "lineHeight": 1.8,
            "theme": "sepia",
            "fontFamily": "mono",
            "columnWidth": 700,
            "unknown": "dropped",
        }))
        .expect("valid fields");
        // `serde_json` hands back object keys in its own order; the fields
        // land on their own columns by name, so only the set matters.
        let mut keys: Vec<&str> = fields.iter().map(|(key, _)| *key).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "chapter_index",
                "column_width",
                "font_family",
                "font_size",
                "line_height",
                "scroll_percent",
                "theme"
            ]
        );
    }

    #[test]
    fn progress_rejects_values_the_schema_would() {
        assert!(progress_fields(&json!({ "chapterIndex": -1 })).is_err());
        assert!(progress_fields(&json!({ "chapterIndex": 1.5 })).is_err());
        assert!(progress_fields(&json!({ "scrollPercent": 1.2 })).is_err());
        assert!(progress_fields(&json!({ "fontSize": 11 })).is_err());
        assert!(progress_fields(&json!({ "lineHeight": 3.0 })).is_err());
        assert!(progress_fields(&json!({ "theme": "blue" })).is_err());
        assert!(progress_fields(&json!({ "fontFamily": 3 })).is_err());
        assert!(progress_fields(&json!({ "columnWidth": 120 })).is_err());
        assert!(progress_fields(&json!({ "theme": null })).is_err());
    }

    /* -- `chapter_title` boundaries at the UTF-16 limit -- */

    #[test]
    fn a_title_of_exactly_the_limit_is_kept_whole() {
        let exact = "y".repeat(TITLE_LIMIT);
        assert_eq!(chapter_title(&exact, 0), exact);
    }
}
