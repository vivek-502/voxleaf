//! Multipart uploads.
//!
//! Both upload routes — a chapter's audio and a book's EPUB — arrive as
//! `multipart/form-data` from an XHR so the browser can report real progress.
//! This parses the form, validates before writing, and commits the database row
//! only after the bytes are safely on disk.

use axum::extract::Multipart;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::{json, Value};

use crate::api::stream;
use crate::api::AppState;
use crate::error::{ApiError, ApiResult};
use crate::repo::{self, now_iso};
use crate::services::audio_meta::{AudioMeta, AudioProperties};
use crate::services::epub::size_limit_message;
use crate::services::library::request_error;

/// One buffered file from a multipart form.
pub struct UploadedFile {
    pub field: String,
    pub filename: String,
    pub content_type: String,
    pub bytes: bytes::Bytes,
}

/// Read a single-file multipart body, enforcing the declared length limit.
pub async fn read_single_file(
    req: axum::extract::Request,
    max_bytes: u64,
) -> ApiResult<UploadedFile> {
    let content_type = req
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();

    if !content_type.contains("multipart/form-data") {
        // The web app threw a plain `Error` with a status, and its envelope
        // named every such failure `request_failed`.
        return Err(request_error(
            415,
            "Audio must be uploaded as multipart/form-data.",
        ));
    }

    // Reject an over-sized upload before buffering it into memory. The +1MB is
    // form overhead, exactly as the web app allowed.
    let hard_limit = max_bytes + 1024 * 1024;
    if let Some(length) = req
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
    {
        if length > hard_limit {
            return Err(ApiError::too_large(format!(
                "That file is larger than the {} MB limit.",
                (max_bytes / (1024 * 1024)).max(1)
            )));
        }
    }

    // `Multipart` is an `FromRequest`; with no app state of its own, the unit
    // state is what the extractor is generic over.
    let mut multipart = <Multipart as axum::extract::FromRequest<()>>::from_request(req, &())
        .await
        .map_err(|error| {
            tracing::error!(%error, "multipart parse failed");
            request_error(400, "The upload could not be read. Please try again.")
        })?;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| request_error(400, "The upload could not be read. Please try again."))?
    {
        let name = field.name().unwrap_or("").to_string();
        if name != "file" {
            // Drain is implicit: dropping the field moves to the next one.
            continue;
        }

        let filename = field
            .file_name()
            .map(|s| s.to_string())
            .unwrap_or_else(|| "upload.bin".to_string());
        let content_type = field
            .content_type()
            .map(|s| s.to_string())
            .unwrap_or_else(|| "application/octet-stream".to_string());

        let bytes = field.bytes().await.map_err(|_| {
            request_error(400, "The upload ended early. Please try again.")
        })?;

        if bytes.is_empty() {
            return Err(request_error(
                400,
                "No audio file was included in the upload.",
            ));
        }
        if bytes.len() as u64 > hard_limit {
            return Err(ApiError::too_large(format!(
                "That file is larger than the {} MB limit.",
                (max_bytes / (1024 * 1024)).max(1)
            )));
        }

        return Ok(UploadedFile {
            field: name,
            filename,
            content_type,
            bytes,
        });
    }

    Err(request_error(
        400,
        "No audio file was included in the upload.",
    ))
}

/// Read the EPUB out of a single-file multipart body.
///
/// The same shape as the audio upload, with the web app's own route messages:
/// this upload's failures are shown beside the file picker, so its wording
/// belongs to the picker rather than to the audio dialog. An empty file is not
/// refused here — the service answers with a `failed` status beside the
/// picker, exactly as the web app did.
pub async fn read_epub_file(
    req: axum::extract::Request,
    max_bytes: u64,
) -> ApiResult<UploadedFile> {
    let content_type = req
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();

    if !content_type.contains("multipart/form-data") {
        return Err(request_error(
            415,
            "An EPUB must be uploaded as multipart/form-data.",
        ));
    }

    // Refuse an over-sized body before buffering it into memory. The +4 MB is
    // the multipart envelope, which wraps the file in headers of its own.
    let hard_limit = max_bytes + 4 * 1024 * 1024;
    if let Some(length) = req
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
    {
        if length > hard_limit {
            return Err(ApiError::too_large(size_limit_message(max_bytes)));
        }
    }

    let mut multipart = <Multipart as axum::extract::FromRequest<()>>::from_request(req, &())
        .await
        .map_err(|error| {
            tracing::error!(%error, "epub multipart parse failed");
            request_error(400, "The upload could not be read. Please try again.")
        })?;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|error| {
            tracing::error!(%error, "epub multipart read failed");
            request_error(400, "The upload could not be read. Please try again.")
        })?
    {
        if field.name().unwrap_or("") != "file" {
            // Drain is implicit: dropping the field moves to the next one.
            continue;
        }
        // A `file` field carrying no filename is not a file — the web app's
        // FormData handed back its string value, and the route said so.
        let Some(filename) = field.file_name().map(str::to_string) else {
            return Err(request_error(
                400,
                "No EPUB file was included in the upload.",
            ));
        };
        let content_type = field
            .content_type()
            .map(str::to_string)
            .unwrap_or_else(|| "application/octet-stream".to_string());

        let bytes = field.bytes().await.map_err(|error| {
            tracing::error!(%error, "epub multipart read failed");
            request_error(400, "The upload could not be read. Please try again.")
        })?;

        return Ok(UploadedFile {
            field: "file".to_string(),
            filename,
            content_type,
            bytes,
        });
    }

    Err(request_error(
        400,
        "No EPUB file was included in the upload.",
    ))
}

/// Validate an audio upload's name and declared type before any parsing.
pub fn validate_audio_upload(
    file: &UploadedFile,
    cfg: &crate::config::Config,
) -> ApiResult<()> {
    let extension = std::path::Path::new(&file.filename)
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy().to_ascii_lowercase()))
        .unwrap_or_default();

    if !cfg.upload.allowed_extensions.contains(&extension.as_str()) {
        return Err(ApiError::upload(
            "file",
            "That file type is not supported. Use MP3, M4A, AAC, WAV, OGG or FLAC.",
        ));
    }

    let declared = file.content_type.to_ascii_lowercase();
    let declared = declared.split(';').next().unwrap_or("").trim().to_string();
    if !cfg
        .upload
        .allowed_mime_types
        .contains(&declared.as_str())
    {
        return Err(ApiError::upload(
            "file",
            "That file does not look like audio.",
        ));
    }

    Ok(())
}

/// Parse the container to confirm the bytes really are audio, and read the
/// duration the player needs for its timeline.
pub fn inspect_audio(file: &UploadedFile) -> ApiResult<AudioProperties> {
    AudioMeta::new()
        .inspect(&file.bytes, &file.filename)
        .map_err(|e| ApiError::upload(e.field, e.message))
}

/// `POST /api/chapters/:id/audio` — replace a chapter's audio.
pub async fn receive_audio(
    st: &AppState,
    chapter_id: &str,
    _content_type: &str,
    req: axum::extract::Request,
) -> ApiResult<Value> {
    // The chapter must exist before a byte is read, so a typo'd id fails fast.
    let chapter = st
        .db
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

    let file = read_single_file(req, st.cfg.upload.max_bytes).await?;
    validate_audio_upload(&file, &st.cfg)?;

    let properties = inspect_audio(&file)?;

    // Anything already there is replaced, so the previous bytes are removed
    // first — a half-written replacement must not leave two files claiming one
    // chapter.
    let previous = st
        .db
        .call({
            let chapter_id = chapter_id.to_string();
            move |conn| {
                conn.query_row(
                    "SELECT storage_key FROM chapter_audio WHERE chapter_id = ?1",
                    [chapter_id],
                    |row| row.get::<_, String>(0),
                )
            }
        })
        .await
        .ok();

    let extension = std::path::Path::new(&file.filename)
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy().to_ascii_lowercase()))
        .unwrap_or_else(|| ".mp3".to_string());

    let key = crate::storage::LocalStorage::key_for(
        &extension,
        &st.cfg.upload.allowed_extensions,
    )?;
    let mime = if file.content_type.starts_with("application/octet-stream") {
        properties.mime_type.clone()
    } else {
        file.content_type.clone()
    };

    st.cfg.storage.put(&key, &file.bytes)?;

    let audio_id = repo::new_id("au_");
    let uploaded_at = now_iso();

    let inserted: Result<(), ApiError> = st
        .db
        .call({
            let (audio_id, chapter_id, key, mime, uploaded_at) = (
                audio_id.clone(),
                chapter_id.to_string(),
                key.clone(),
                mime.clone(),
                uploaded_at.clone(),
            );
            let (duration, file_size) = (properties.duration, file.bytes.len() as i64);
            let (bit_rate, sample_rate, channels) =
                (properties.bit_rate, properties.sample_rate, properties.channels);
            let original_name = file.filename.clone();
            move |conn| {
                let tx = conn.unchecked_transaction()?;
                tx.execute(
                    "DELETE FROM chapter_audio WHERE chapter_id = ?1",
                    [&chapter_id],
                )?;
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
                        mime,
                        original_name,
                        bit_rate,
                        sample_rate,
                        channels,
                        uploaded_at,
                    ],
                )?;
                tx.commit()?;
                Ok(())
            }
        })
        .await;

    if let Err(error) = inserted {
        // The row never landed, so the bytes must not either.
        let _ = st.cfg.storage.delete(&key);
        return Err(error);
    }

    let replaced = previous.is_some();
    if let Some(ref old_key) = previous {
        if *old_key != key {
            let _ = st.cfg.storage.delete(old_key);
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
            "mimeType": mime,
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

/// Build a 201/200 distinction the way the web app did.
pub fn created(value: Value) -> Response {
    (
        StatusCode::CREATED,
        axum::Json(json!({ "data": value })),
    )
        .into_response()
}
