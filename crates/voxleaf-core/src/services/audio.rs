//! Chapter audio lifecycle: replace and delete.
//!
//! The `chapter_audio` row is the only source of truth for a chapter's audio.
//! Replacing an upload overwrites the row in place — the audio id, and every
//! link the reader has built to it, survives a re-upload — and the previous
//! object is removed from storage once the new row has landed, so storage
//! never accumulates orphans.
//!
//! The multipart parsing itself lives in `api::multipart`; this module is the
//! domain operation behind the upload route (and the desktop shell), and it
//! takes the already-buffered bytes.

use rusqlite::OptionalExtension;
use serde_json::{json, Value};

use crate::api::stream;
use crate::api::AppState;
use crate::db::Database;
use crate::error::{ApiError, ApiResult};
use crate::repo;

/// Run database work from a synchronous service method, exactly as the
/// translation service does.
fn sync_db<T>(
    db: &Database,
    work: impl FnOnce(&rusqlite::Connection) -> rusqlite::Result<T> + Send + 'static,
) -> ApiResult<T>
where
    T: Send + 'static,
{
    tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current()
            .block_on(db.call(work))
            .map_err(ApiError::from)
    })
}

fn extension_of(filename: &str) -> String {
    std::path::Path::new(filename)
        .extension()
        .map(|extension| format!(".{}", extension.to_string_lossy().to_ascii_lowercase()))
        .unwrap_or_default()
}

/// Replace a chapter's audio with `bytes`, returning the new audio DTO.
///
/// Nothing is written until the bytes are validated, parsed and safely on
/// disk; if the row then fails to land, the freshly written object is removed
/// again rather than left unreferenced.
pub async fn replace_audio(
    st: &AppState,
    db: &Database,
    chapter_id: &str,
    bytes: bytes::Bytes,
    filename: &str,
    mime: &str,
) -> ApiResult<Value> {
    // The chapter must exist before a byte is read, so a typo'd id fails fast.
    let chapter = db
        .call({
            let chapter_id = chapter_id.to_string();
            move |conn| {
                conn.query_row(
                    "SELECT id, book_id, title, chapter_number FROM chapters WHERE id = ?1",
                    [chapter_id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, i64>(3)?,
                        ))
                    },
                )
            }
        })
        .await
        .map_err(|_| ApiError::not_found("That chapter no longer exists."))?;

    // Validation and container inspection are the upload route's, byte for
    // byte, so an upload behaves the same whether it arrives as multipart or
    // is handed straight to the service.
    let file = crate::api::multipart::UploadedFile {
        field: "file".to_string(),
        filename: filename.to_string(),
        content_type: mime.to_string(),
        bytes,
    };
    crate::api::multipart::validate_audio_upload(&file, &st.cfg)?;
    let properties = crate::api::multipart::inspect_audio(&file)?;

    let previous: Option<(String, String)> = db
        .call({
            let chapter_id = chapter_id.to_string();
            move |conn| {
                conn.query_row(
                    "SELECT id, storage_key FROM chapter_audio WHERE chapter_id = ?1",
                    [chapter_id],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                )
                .optional()
            }
        })
        .await
        .ok()
        .flatten();

    let extension = {
        let extension = extension_of(&file.filename);
        if extension.is_empty() {
            ".mp3".to_string()
        } else {
            extension
        }
    };
    let key = crate::storage::LocalStorage::key_for(&extension, &st.cfg.upload.allowed_extensions)?;
    let stored_mime = if file.content_type.is_empty() || file.content_type.starts_with("application/octet-stream")
    {
        properties.mime_type.clone()
    } else {
        file.content_type.clone()
    };

    st.cfg.storage.put(&key, &file.bytes)?;

    let uploaded_at = repo::now_iso();
    let audio_id = match &previous {
        Some((existing_id, _)) => existing_id.clone(),
        None => repo::new_id("au_"),
    };

    let committed: Result<(), ApiError> = db
        .call({
            let had_previous = previous.is_some();
            let (audio_id, chapter_id, key, stored_mime, uploaded_at) = (
                audio_id.clone(),
                chapter_id.to_string(),
                key.clone(),
                stored_mime.clone(),
                uploaded_at.clone(),
            );
            let (duration, file_size) = (properties.duration, file.bytes.len() as i64);
            let (bit_rate, sample_rate, channels) =
                (properties.bit_rate, properties.sample_rate, properties.channels);
            let original_name = file.filename.clone();
            move |conn| {
                let tx = conn.unchecked_transaction()?;
                if had_previous {
                    tx.execute(
                        "UPDATE chapter_audio SET \
                          file_url = ?1, storage_key = ?2, duration = ?3, file_size = ?4, \
                          mime_type = ?5, original_name = ?6, bit_rate = ?7, sample_rate = ?8, \
                          channels = ?9, uploaded_at = ?10 \
                          WHERE chapter_id = ?11",
                        rusqlite::params![
                            format!("/api/audio/{audio_id}"),
                            key,
                            duration,
                            file_size,
                            stored_mime,
                            original_name,
                            bit_rate,
                            sample_rate,
                            channels,
                            uploaded_at,
                            chapter_id,
                        ],
                    )?;
                } else {
                    tx.execute(
                        "INSERT INTO chapter_audio \
                         (id, chapter_id, file_url, storage_key, duration, file_size, mime_type, \
                          original_name, bit_rate, sample_rate, channels, uploaded_at) \
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                        rusqlite::params![
                            audio_id,
                            chapter_id,
                            format!("/api/audio/{audio_id}"),
                            key,
                            duration,
                            file_size,
                            stored_mime,
                            original_name,
                            bit_rate,
                            sample_rate,
                            channels,
                            uploaded_at,
                        ],
                    )?;
                }
                tx.commit()?;
                Ok(())
            }
        })
        .await;

    if let Err(error) = committed {
        // The row never landed, so the new bytes must not either.
        let _ = st.cfg.storage.delete(&key);
        return Err(error);
    }

    // The previous object is now unreferenced.
    let replaced = previous.is_some();
    if let Some((_, old_key)) = &previous {
        if *old_key != key {
            if let Err(error) = st.cfg.storage.delete(old_key) {
                tracing::error!(%error, "could not delete the replaced audio object");
            }
        }
    }

    let stream_url = stream::signed_url(st, &audio_id);
    let (_id, _book_id, title, number) = chapter;

    Ok(json!({
        "replaced": replaced,
        "audio": {
            "id": audio_id,
            "chapterId": chapter_id,
            "streamUrl": stream_url,
            "duration": properties.duration,
            "fileSize": file.bytes.len() as i64,
            "mimeType": stored_mime,
            "originalName": file.filename,
            "uploadedAt": uploaded_at,
        },
        "chapter": {
            "id": chapter_id,
            "chapterNumber": number,
            "title": title,
        },
    }))
}

/// Delete a chapter's audio row and its stored bytes.
///
/// The row goes first — it is the source of truth — and the object's removal
/// is best-effort afterwards: a file that refuses to be deleted is logged and
/// costs a few bytes, whereas a row that points at nothing costs a reader a
/// broken player.
pub fn remove_audio(db: &Database, chapter_id: &str) -> ApiResult<()> {
    let existing: Option<(String, String)> = sync_db(db, {
        let chapter_id = chapter_id.to_string();
        move |conn| {
            conn.query_row(
                "SELECT id, storage_key FROM chapter_audio WHERE chapter_id = ?1",
                [chapter_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()
        }
    })?;

    let Some((_audio_id, storage_key)) = existing else {
        return Ok(());
    };

    sync_db(db, {
        let chapter_id = chapter_id.to_string();
        move |conn| {
            conn.execute("DELETE FROM chapter_audio WHERE chapter_id = ?1", [chapter_id])
        }
    })?;

    match crate::config::get() {
        Ok(config) => {
            if let Err(error) = config.storage.delete(&storage_key) {
                tracing::error!(%error, "could not delete the audio object");
            }
        }
        Err(_) => {
            tracing::warn!(%storage_key, "audio row deleted without storage cleanup: no configuration");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> (tempfile::TempDir, Database) {
        let dir = tempfile::tempdir().expect("temp dir");
        let database = Database::open(&dir.path().join("vb.db")).expect("open");
        (dir, database)
    }

    async fn seed_chapter(database: &Database) {
        let now = repo::now_iso();
        database
            .call({
                let now = now.clone();
                move |conn| {
                    conn.execute(
                        "INSERT INTO books (id, external_id, title, created_at, updated_at) \
                         VALUES ('bk_test', 'ext-1', 'A Book', ?1, ?1)",
                        [&now],
                    )?;
                    conn.execute(
                        "INSERT INTO chapters (id, book_id, chapter_number, title, created_at, updated_at) \
                         VALUES ('ch_1', 'bk_test', 1, 'One', ?1, ?1)",
                        [&now],
                    )?;
                    Ok::<(), rusqlite::Error>(())
                }
            })
            .await
            .expect("seed");
    }

    async fn audio_row(database: &Database, chapter_id: &str) -> Option<String> {
        database
            .call({
                let chapter_id = chapter_id.to_string();
                move |conn| {
                    conn.query_row(
                        "SELECT storage_key FROM chapter_audio WHERE chapter_id = ?1",
                        [chapter_id],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()
                }
            })
            .await
            .expect("query")
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn removing_silently_ignores_a_chapter_with_no_audio() {
        let (_dir, database) = test_db();
        seed_chapter(&database).await;

        remove_audio(&database, "ch_1").expect("nothing to remove");
        remove_audio(&database, "ch_nope").expect("nothing to remove either");
        assert!(audio_row(&database, "ch_1").await.is_none());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn removing_deletes_the_row() {
        let (_dir, database) = test_db();
        seed_chapter(&database).await;

        let now = repo::now_iso();
        database
            .call({
                let now = now.clone();
                move |conn| {
                    conn.execute(
                        "INSERT INTO chapter_audio \
                         (id, chapter_id, file_url, storage_key, duration, file_size, mime_type, uploaded_at) \
                         VALUES ('au_1', 'ch_1', '/api/audio/au_1', 'orphan.wav', 1.0, 10, 'audio/wav', ?1)",
                        [&now],
                    )?;
                    Ok::<(), rusqlite::Error>(())
                }
            })
            .await
            .expect("seed audio");

        remove_audio(&database, "ch_1").expect("remove");
        assert!(audio_row(&database, "ch_1").await.is_none());
    }
}
