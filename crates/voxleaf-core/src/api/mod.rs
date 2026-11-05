//! HTTP surface: an exact reproduction of the Next.js `/api/*` contract.
//!
//! Same paths, same `{ data }` / `{ error: { code, message } }` envelope, same
//! status codes. The React UI is served from this same origin, so the ported
//! frontend keeps using plain `fetch('/api/...')`.

pub mod multipart;
pub mod stream;

use std::sync::Arc;

use axum::{
    body::Body,
    extract::{DefaultBodyLimit, Path, Query, Request, State},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, patch, post, put},
    Json, Router,
};
use serde::Serialize;
use serde_json::{json, Value};

use crate::config::Config;
use crate::db::Database;
use crate::error::{ApiError, ApiResult};
use crate::services::Services;

/// Shared state for every handler.
#[derive(Clone)]
pub struct AppState {
    pub db: Database,
    pub cfg: Arc<Config>,
    pub services: Arc<Services>,
}

impl AppState {
    pub fn new(db: Database, cfg: Config, services: Services) -> Self {
        Self {
            db,
            cfg: Arc::new(cfg),
            services: Arc::new(services),
        }
    }
}

/// Envelope a successful payload: `{ "data": ... }`.
pub fn ok<T: Serialize>(value: T) -> Response {
    Json(json!({ "data": value })).into_response()
}

/// Envelope a value that is already a `serde_json::Value`.
pub fn ok_value(value: Value) -> Response {
    Json(json!({ "data": value })).into_response()
}

/// Envelope a payload under a success status other than 200. The web routes
/// drew a real distinction on the status line — 201 for a creation, 202 when a
/// narration job was merely accepted — and a client reading `status` should
/// see the same answer here.
fn ok_status(status: StatusCode, value: Value) -> Response {
    (status, Json(json!({ "data": value }))).into_response()
}

pub fn err(e: ApiError) -> Response {
    e.into_response()
}

/* -------------------------------------------------------------------------- */
/* Route table                                                                */
/* -------------------------------------------------------------------------- */

/// Build the router. `spa` is the fallback served for non-API paths.
///
/// The API is mounted at `/api` because that is the prefix the web UI
/// addresses: `apiUrl()` in `hooks/use-api.ts` prepends `/api` to every path,
/// and stream URLs are minted as `/api/audio/{id}`. The prefix lives here
/// rather than on each route so the table keeps the Next.js route names.
///
/// Paths under `/api` that no route claims get a JSON 404 instead of the SPA
/// page: a mistyped endpoint should read as an API error, never as a blank
/// document.
pub fn router<S>(state: AppState, spa: S) -> Router
where
    S: tower::Service<axum::http::Request<axum::body::Body>, Response = axum::response::Response, Error = std::convert::Infallible>
        + Clone
        + Send
        + Sync
        + 'static,
    S::Future: Send + 'static,
{
    let api = Router::new()
        /* -- catalogue / discovery ---------------------------------------- */
        .route("/books/search", get(catalogue_search))
        .route("/covers", get(covers))
        /* -- library -------------------------------------------------------- */
        .route("/library", get(library_list).post(library_add).delete(library_clear))
        .route(
            "/library/{library_id}",
            get(library_get)
                .put(library_patch)
                .patch(library_patch)
                .delete(library_delete),
        )
        .route(
            "/library/{library_id}/chapters",
            get(library_chapters).post(library_chapters_add),
        )
        /* -- books & chapters ---------------------------------------------- */
        .route("/books/{id}", get(book_get))
        .route(
            "/chapters/{chapter_id}",
            get(chapter_get).patch(chapter_patch).delete(chapter_delete),
        )
        .route("/chapters/reorder", post(chapters_reorder))
        .route(
            "/chapters/{chapter_id}/audio",
            post(chapter_audio_upload).delete(chapter_audio_delete),
        )
        /* -- audio streaming ------------------------------------------------- */
        .route("/audio/{audio_id}", get(audio_stream))
        .route("/playback/resolve", get(playback_resolve))
        /* -- progress & bookmarks ------------------------------------------ */
        .route("/progress", put(progress_save))
        // The web app's frontend deletes `/api/progress/reset` (book-detail's
        // "Reset progress"), while its server only ever registered the handler
        // under `/api/progress/actions` — the button 404s upstream. Both URLs
        // are served here so the ported UI works; `/actions` is kept because it
        // is the route the web app's filesystem actually exposes.
        .route("/progress/reset", delete(progress_reset))
        .route(
            "/progress/actions",
            post(progress_complete).delete(progress_reset),
        )
        .route("/bookmarks", get(bookmarks_list).post(bookmarks_create))
        .route(
            "/bookmarks/{bookmark_id}",
            patch(bookmarks_patch).delete(bookmarks_delete),
        )
        /* -- home ------------------------------------------------------------ */
        .route("/home", get(home))
        /* -- settings & auth -------------------------------------------------- */
        .route("/settings", get(settings_get).put(settings_put))
        .route("/auth/status", get(auth_status).post(auth_login))
        /* -- TTS ---------------------------------------------------------------- */
        .route("/tts/voices", get(tts_voices))
        .route(
            "/chapters/{chapter_id}/tts",
            get(chapters_tts_status)
                .post(chapters_tts)
                .delete(chapters_tts_delete),
        )
        /* -- EPUB ---------------------------------------------------------------- */
        .route("/books/{id}/epub", get(epub_status).post(epub_reparse).delete(epub_detach))
        .route("/books/{id}/epub/upload", post(epub_upload))
        .route("/epub/{book_id}/chapters", post(epub_chapters))
        .route("/epub/{book_id}/chapters/audio", get(epub_chapters_audio))
        .route(
            "/epub/{book_id}/chapter/{index}",
            get(epub_chapter),
        )
        .route(
            "/epub/{book_id}/chapter/{index}/ensure",
            post(epub_chapter_ensure),
        )
        .route("/epub/{book_id}/image/{ref}", get(epub_image))
        .route(
            "/epub/{book_id}/progress",
            get(epub_progress_get).patch(epub_progress_patch),
        )
        .route("/epub/{book_id}/translations", get(epub_translations))
        .route(
            "/epub/{book_id}/translations/{index}",
            get(epub_translation_get)
                .post(epub_translation_post)
                .delete(epub_translation_delete),
        );

    let api = api.fallback(api_not_found).with_state(state);

    Router::new()
        .nest("/api", api)
        .fallback_service(spa)
        .layer(DefaultBodyLimit::max(512 * 1024 * 1024))
}

/// Anything under `/api` no route claimed. The web app answered these with
/// Next.js' own 404 page; the envelope here keeps the failure parseable by the
/// same `request()` helper that reads every other error.
async fn api_not_found() -> Response {
    ApiError::not_found("That endpoint does not exist.").into_response()
}

/* -------------------------------------------------------------------------- */
/* Query helpers                                                              */
/* -------------------------------------------------------------------------- */

#[derive(serde::Deserialize)]
pub struct QueryMap {
    #[serde(flatten)]
    pub map: std::collections::HashMap<String, String>,
}

impl QueryMap {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.map.get(key).map(|s| s.as_str())
    }
    pub fn str_or(&self, key: &str, default: &str) -> String {
        self.get(key).unwrap_or(default).to_string()
    }
    pub fn int_or(&self, key: &str, default: i64) -> i64 {
        self.get(key)
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(default)
    }
    pub fn clamped_i64(&self, key: &str, default: i64, min: i64, max: i64) -> i64 {
        self.int_or(key, default).clamp(min, max)
    }
    pub fn float_or(&self, key: &str, default: f64) -> f64 {
        self.get(key)
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(default)
    }
}

/// Parse a JSON body into `T`, mirroring the web app's `parseJson`.
pub async fn parse_json<T: serde::de::DeserializeOwned>(req: Request) -> ApiResult<T> {
    let bytes = axum::body::to_bytes(req.into_body(), 64 * 1024 * 1024)
        .await
        .map_err(|_| ApiError::bad_request("Request body must be valid JSON."))?;
    serde_json::from_slice(&bytes)
        .map_err(|_| ApiError::bad_request("Request body must be valid JSON."))
}

/// Parse and validate a JSON body, converting failures into the web app's 400.
pub async fn parse_json_validated<T, F>(req: Request, validate: F) -> ApiResult<T>
where
    T: serde::de::DeserializeOwned,
    F: FnOnce(&T) -> Result<(), String>,
{
    let value: T = parse_json(req).await?;
    validate(&value).map_err(|m| ApiError::invalid("body", m))?;
    Ok(value)
}

/// A route index read the way `Number.parseInt` reads one: leading space and
/// sign, then the digits it finds — `"7"` and `"7abc"` are both 7 — with
/// nothing parseable naming no section at all.
fn web_index(raw: &str) -> Option<i64> {
    let rest = raw.trim_start();
    let (rest, negative) = match rest.strip_prefix('-') {
        Some(rest) => (rest, true),
        None => (rest.strip_prefix('+').unwrap_or(rest), false),
    };
    let digits: String = rest
        .chars()
        .take_while(|character| character.is_ascii_digit())
        .collect();
    let value: i64 = digits.parse().ok()?;
    Some(if negative { -value } else { value })
}

/// `translationRequestSchema` — the checks the web route made before opening
/// the stream. Its failures were plain JSON, readable by the reader's
/// `request()` helper; unknown keys were dropped, not rejected.
fn translation_request(body: &Value) -> ApiResult<()> {
    let language = body
        .get("language")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ApiError::invalid(
                "language",
                "Invalid input: expected string, received something else",
            )
        })?;
    // The schema trimmed first, then measured in UTF-16 units.
    let trimmed = language.trim();
    let length = trimmed.encode_utf16().count();
    if length < 2 {
        return Err(ApiError::invalid(
            "language",
            "String must contain at least 2 character(s).",
        ));
    }
    if length > 20 {
        return Err(ApiError::invalid(
            "language",
            "String must contain at most 20 character(s).",
        ));
    }
    if !crate::services::translation::is_valid_language_code(trimmed) {
        return Err(ApiError::invalid("language", "Not a language code."));
    }
    if let Some(force) = body.get("force") {
        if !force.is_boolean() {
            return Err(ApiError::invalid(
                "force",
                "Invalid input: expected boolean, received something else",
            ));
        }
    }
    Ok(())
}

/* -------------------------------------------------------------------------- */
/* Handlers                                                                   */
/* -------------------------------------------------------------------------- */

/// Run a handler body, converting any `ApiError` into the web app's envelope.
async fn err_handler(future: impl std::future::Future<Output = ApiResult<Response>>) -> Response {
    match future.await {
        Ok(r) => r,
        Err(e) => err(e),
    }
}

/* -- catalogue -------------------------------------------------------------- */

async fn catalogue_search(State(st): State<AppState>, Query(q): Query<QueryMap>) -> Response {
    err_handler(async {
        let books = st.services.catalogue.search(
            &st.db,
            &q.str_or("q", ""),
            &q.str_or("genre", "all"),
            &q.str_or("sort", "relevance"),
            q.clamped_i64("page", 1, 1, 500),
            q.clamped_i64("pageSize", 20, 1, 50),
        )
        .await?;
        Ok(ok(books))
    })
    .await
}

async fn covers(State(st): State<AppState>, Query(q): Query<QueryMap>) -> Response {
    err_handler(async {
        // This route answers with image bytes (or a bare plain-text error),
        // not the `{ data }` envelope, so the response is produced directly.
        Ok(st.services.covers.proxy(q.get("u")).await)
    })
    .await
}

/* -- library ---------------------------------------------------------------- */

async fn library_list(State(st): State<AppState>, Query(q): Query<QueryMap>) -> Response {
    err_handler(async {
        let filter = q.str_or("filter", "all");
        let sort = q.str_or("sort", "recently-added");
        if !crate::services::library::LIBRARY_FILTERS.contains(&filter.as_str()) {
            return Err(ApiError::invalid("filter", "That filter is not available."));
        }
        if !crate::services::library::LIBRARY_SORTS.contains(&sort.as_str()) {
            return Err(ApiError::invalid("sort", "That sort is not available."));
        }
        let page = q.clamped_i64("page", 1, 1, 500);
        let page_size = q.clamped_i64("pageSize", 24, 1, 100);
        let (items, total, stats) = crate::services::library::list_library(
            &st.db,
            &filter,
            &sort,
            page as u32,
            page_size as u32,
            q.str_or("q", "").as_str(),
        )?;
        // The exact keys `listLibrary` returned in the web app — the shelf
        // reads `data.books` and pages with `data.hasMore`.
        Ok(ok(json!({
            "books": items,
            "total": total,
            "page": page,
            "pageSize": page_size,
            "hasMore": page * page_size < total,
            "stats": stats,
        })))
    })
    .await
}

async fn library_add(State(st): State<AppState>, mut body: Request) -> Response {
    err_handler(async {
        let raw: Value = parse_json(std::mem::take(&mut body)).await?;
        let book_id = raw
            .get("bookId")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let external_id = raw
            .get("externalId")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let favorite = raw.get("favorite").and_then(|v| v.as_bool()).unwrap_or(false);
        if book_id.is_none() && external_id.is_none() {
            return Err(ApiError::invalid("bookId", "A bookId or externalId is required."));
        }
        let entry = crate::services::library::add_to_library(
            &st.db,
            &st.services,
            book_id.as_deref(),
            external_id.as_deref(),
            favorite,
        )
        .await?;
        // 201 with `no-store`: the shelf listing fetched right after an add
        // must never replay a cached pre-add copy.
        let mut response = ok_status(StatusCode::CREATED, entry);
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        Ok(response)
    })
    .await
}

async fn library_get(
    State(st): State<AppState>,
    Path(library_id): Path<String>,
) -> Response {
    err_handler(async {
        let entry = crate::services::library::get_library_entry(&st.db, &library_id)?;
        Ok(ok(crate::api::stream::sign_stream_urls(&st, entry)))
    })
    .await
}

async fn library_patch(
    State(st): State<AppState>,
    Path(library_id): Path<String>,
    mut body: Request,
) -> Response {
    err_handler(async {
        let raw: Value = parse_json(std::mem::take(&mut body)).await?;
        let favorite = raw.get("favorite").and_then(|v| v.as_bool());
        let entry = crate::services::library::update_library_by_id(&st.db, &library_id, favorite)?;
        Ok(ok(crate::api::stream::sign_stream_urls(&st, entry)))
    })
    .await
}

async fn library_clear(State(st): State<AppState>) -> Response {
    err_handler(async {
        crate::services::library::clear_library(&st.db)?;
        Ok(ok(json!({ "ok": true })))
    })
    .await
}

async fn library_delete(
    State(st): State<AppState>,
    Path(library_id): Path<String>,
) -> Response {
    err_handler(async {
        crate::services::library::remove_from_library(&st.db, &library_id)?;
        Ok(ok(json!({ "removed": true })))
    })
    .await
}

async fn library_chapters(
    State(st): State<AppState>,
    Path(library_id): Path<String>,
) -> Response {
    err_handler(async {
        let chapters = crate::services::library::list_chapters(&st.db, &library_id)?;
        Ok(ok(crate::api::stream::sign_stream_urls(&st, chapters)))
    })
    .await
}

async fn library_chapters_add(
    State(st): State<AppState>,
    Path(library_id): Path<String>,
    mut body: Request,
) -> Response {
    err_handler(async {
        let raw: Value = parse_json(std::mem::take(&mut body)).await?;
        let created = crate::services::library::add_chapters(&st.db, &library_id, &raw)?;
        // Every branch upstream answered 201 — batch, titles, and the single
        // chapter alike.
        Ok(ok_status(StatusCode::CREATED, created))
    })
    .await
}

/* -- books & chapters -------------------------------------------------------- */

async fn book_get(State(st): State<AppState>, Path(id): Path<String>) -> Response {
    err_handler(async {
        let book = crate::services::library::get_book(&st.db, &id).await?;
        Ok(ok(crate::api::stream::sign_stream_urls(&st, book)))
    })
    .await
}

async fn chapter_get(
    State(st): State<AppState>,
    Path(chapter_id): Path<String>,
) -> Response {
    err_handler(async {
        let chapter = crate::services::library::get_chapter(&st.db, &chapter_id)?;
        Ok(ok(chapter))
    })
    .await
}

async fn chapter_patch(
    State(st): State<AppState>,
    Path(chapter_id): Path<String>,
    mut body: Request,
) -> Response {
    err_handler(async {
        let raw: Value = parse_json(std::mem::take(&mut body)).await?;
        let chapter = crate::services::library::update_chapter(&st.db, &chapter_id, &raw)?;
        Ok(ok(chapter))
    })
    .await
}

async fn chapter_delete(
    State(st): State<AppState>,
    Path(chapter_id): Path<String>,
) -> Response {
    err_handler(async {
        crate::services::library::delete_chapter(&st.db, &chapter_id)?;
        Ok(ok(json!({ "removed": true })))
    })
    .await
}

async fn chapters_reorder(State(st): State<AppState>, mut body: Request) -> Response {
    err_handler(async {
        let raw: Value = parse_json(std::mem::take(&mut body)).await?;
        let book_id = raw
            .get("bookId")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ApiError::invalid("bookId", "A bookId is required."))?;
        let ids: Vec<String> = raw
            .get("orderedIds")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        if ids.is_empty() {
            return Err(ApiError::invalid("orderedIds", "An ordered list of chapter ids is required."));
        }
        let chapters = crate::services::library::reorder_chapters(&st.db, book_id, &ids)?;
        Ok(ok(chapters))
    })
    .await
}

async fn chapter_audio_upload(
    State(st): State<AppState>,
    Path(chapter_id): Path<String>,
    req: Request,
) -> Response {
    err_handler(async {
        let ct = req
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let audio = crate::api::multipart::receive_audio(&st, &chapter_id, &ct, req).await?;
        // 201 on a first upload, 200 on a replacement — the status line was the
        // web route's distinction; this body repeats it as `replaced` so the
        // dialog's toast can name what actually happened.
        let replaced = audio
            .get("replaced")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        Ok(ok_status(
            if replaced {
                StatusCode::OK
            } else {
                StatusCode::CREATED
            },
            audio,
        ))
    })
    .await
}

async fn chapter_audio_delete(
    State(st): State<AppState>,
    Path(chapter_id): Path<String>,
) -> Response {
    err_handler(async {
        crate::services::audio::remove_audio(&st.db, &chapter_id)?;
        Ok(ok(json!({ "removed": true })))
    })
    .await
}

/* -- audio streaming ---------------------------------------------------------- */

async fn audio_stream(
    State(st): State<AppState>,
    Path(audio_id): Path<String>,
    req: Request,
) -> Response {
    match stream::serve_audio(&st, &audio_id, req).await {
        Ok(r) => r,
        Err(e) => err(e),
    }
}

async fn playback_resolve(State(st): State<AppState>, Query(q): Query<QueryMap>) -> Response {
    err_handler(async {
        let resolved = crate::services::playback::resolve(
            &st.db,
            q.get("bookId"),
            q.get("libraryId"),
            q.get("chapterId"),
        )?;
        Ok(ok(crate::api::stream::sign_stream_urls(&st, resolved)))
    })
    .await
}

/* -- progress ------------------------------------------------------------------ */

async fn progress_save(State(st): State<AppState>, mut body: Request) -> Response {
    err_handler(async {
        let raw: Value = parse_json(std::mem::take(&mut body)).await?;
        let progress = crate::services::progress::save(&st.db, &raw)?;
        Ok(ok(progress))
    })
    .await
}

async fn progress_complete(State(st): State<AppState>, mut body: Request) -> Response {
    err_handler(async {
        let raw: Value = parse_json(std::mem::take(&mut body)).await?;
        let progress = crate::services::progress::set_completed(&st.db, &raw)?;
        Ok(ok(progress))
    })
    .await
}

async fn progress_reset(State(st): State<AppState>, mut body: Request) -> Response {
    err_handler(async {
        let raw: Value = parse_json(std::mem::take(&mut body)).await?;
        crate::services::progress::reset(&st.db, &raw)?;
        Ok(ok(json!({ "reset": true })))
    })
    .await
}

/* -- bookmarks ------------------------------------------------------------------ */

async fn bookmarks_list(State(st): State<AppState>, Query(q): Query<QueryMap>) -> Response {
    err_handler(async {
        let items = crate::services::bookmarks::list(&st.db, q.get("chapterId"))?;
        Ok(ok(items))
    })
    .await
}

async fn bookmarks_create(State(st): State<AppState>, mut body: Request) -> Response {
    err_handler(async {
        let raw: Value = parse_json(std::mem::take(&mut body)).await?;
        let bookmark = crate::services::bookmarks::create(&st.db, &raw)?;
        Ok(ok_status(StatusCode::CREATED, bookmark))
    })
    .await
}

async fn bookmarks_patch(
    State(st): State<AppState>,
    Path(bookmark_id): Path<String>,
    mut body: Request,
) -> Response {
    err_handler(async {
        let raw: Value = parse_json(std::mem::take(&mut body)).await?;
        let bookmark = crate::services::bookmarks::update(&st.db, &bookmark_id, &raw)?;
        Ok(ok(bookmark))
    })
    .await
}

async fn bookmarks_delete(
    State(st): State<AppState>,
    Path(bookmark_id): Path<String>,
) -> Response {
    err_handler(async {
        crate::services::bookmarks::remove(&st.db, &bookmark_id)?;
        Ok(ok(json!({ "removed": true })))
    })
    .await
}

/* -- home ------------------------------------------------------------------------ */

async fn home(State(st): State<AppState>) -> Response {
    err_handler(async {
        let payload = crate::services::home::home_payload(&st.db, &st.services).await?;
        // The web route pinned a short private lifetime, so two dashboards
        // opened seconds apart read as the same shelf.
        let mut response = ok(payload);
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("private, max-age=15"),
        );
        Ok(response)
    })
    .await
}

/* -- settings & auth ----------------------------------------------------------------- */

async fn settings_get(State(st): State<AppState>) -> Response {
    err_handler(async {
        let settings = crate::services::settings::get(&st.db).await?;
        Ok(ok(crate::services::settings::to_json(&settings)))
    })
    .await
}

async fn settings_put(State(st): State<AppState>, mut body: Request) -> Response {
    err_handler(async {
        let raw: Value = parse_json(std::mem::take(&mut body)).await?;
        let settings = crate::services::settings::update(&st.db, &raw).await?;
        Ok(ok(crate::services::settings::to_json(&settings)))
    })
    .await
}

async fn auth_status(State(st): State<AppState>) -> Response {
    err_handler(async { Ok(ok(crate::services::auth::status(&st.db))) })
    .await
}

async fn auth_login(State(st): State<AppState>, mut body: Request) -> Response {
    err_handler(async {
        let raw: Value = parse_json(std::mem::take(&mut body)).await?;
        let status = crate::services::auth::login(&st.db, &raw)?;
        Ok(ok(status))
    })
    .await
}

/* -- TTS ------------------------------------------------------------------------------ */

async fn tts_voices(State(st): State<AppState>) -> Response {
    err_handler(async {
        let voices = st.services.tts.voices().await?;
        Ok(ok(voices))
    })
    .await
}

async fn chapters_tts(
    State(st): State<AppState>,
    Path(chapter_id): Path<String>,
    mut body: Request,
) -> Response {
    err_handler(async {
        let raw: Value = parse_json(std::mem::take(&mut body)).await?;
        let result = st
            .services
            .tts
            .synthesize_chapter(&st, &chapter_id, &raw)
            .await?;
        // 202 — the job is accepted, not finished; synthesis runs for minutes
        // and the client polls `GET` for its progress.
        Ok(ok_status(StatusCode::ACCEPTED, result))
    })
    .await
}

async fn chapters_tts_delete(
    State(st): State<AppState>,
    Path(chapter_id): Path<String>,
) -> Response {
    err_handler(async {
        st.services.tts.remove_chapter_audio(&st.db, &chapter_id)?;
        Ok(ok(json!({ "ok": true })))
    })
    .await
}

/// `GET /api/chapters/:id/tts` — the job's current status, or `null`.
///
/// The reader polls this while a chapter is being narrated, so a page reload
/// mid-generation picks the bar back up rather than starting over.
async fn chapters_tts_status(
    State(st): State<AppState>,
    Path(chapter_id): Path<String>,
) -> Response {
    err_handler(async {
        let status = st.services.tts.job_status(&st, &chapter_id).await?;
        Ok(ok(crate::api::stream::sign_stream_urls(&st, status)))
    })
    .await
}

/* -- EPUB ------------------------------------------------------------------------------ */

async fn epub_status(State(st): State<AppState>, Path(id): Path<String>) -> Response {
    err_handler(async {
        let mut status = st.services.epub.status(&st.db, &id)?;
        // Only the read route carries the instance's switch — the web app
        // added it to this response and nowhere else.
        status["enabled"] = json!(st.cfg.epub.enabled);
        Ok(ok(status))
    })
    .await
}

async fn epub_reparse(State(st): State<AppState>, Path(id): Path<String>) -> Response {
    err_handler(async {
        // Re-parsing reads what is already stored, but the instance keeps its
        // switch: books are off, re-parsing is part of books.
        if !st.cfg.epub.enabled {
            return Err(ApiError::new(
                axum::http::StatusCode::FORBIDDEN,
                "disabled",
                "Book files are disabled on this instance.",
            ));
        }
        let action = st.services.epub.reparse(&st.db, &id).await?;
        let mut payload = st.services.epub.status(&st.db, &id)?;
        payload["action"] = action.get("status").cloned().unwrap_or(Value::Null);
        payload["error"] = action.get("error").cloned().unwrap_or(Value::Null);
        Ok(ok(payload))
    })
    .await
}

async fn epub_detach(State(st): State<AppState>, Path(id): Path<String>) -> Response {
    err_handler(async {
        st.services.epub.detach(&st.db, &id).await?;
        Ok(ok(json!({ "removed": true })))
    })
    .await
}

async fn epub_upload(
    State(st): State<AppState>,
    Path(id): Path<String>,
    req: Request,
) -> Response {
    err_handler(async {
        let result = st.services.epub.receive_upload(&st, &id, req).await?;
        Ok(ok(result))
    })
    .await
}

async fn epub_chapters(State(st): State<AppState>, Path(book_id): Path<String>) -> Response {
    err_handler(async {
        let sync = st.services.epub.sync_chapters(&st.db, &book_id)?;
        Ok(ok(sync))
    })
    .await
}

async fn epub_chapters_audio(State(st): State<AppState>, Path(book_id): Path<String>) -> Response {
    err_handler(async {
        let list = st.services.epub.chapters_audio(&st.db, &book_id)?;
        Ok(ok(crate::api::stream::sign_stream_urls(&st, list)))
    })
    .await
}

async fn epub_chapter(
    State(st): State<AppState>,
    Path((book_id, index)): Path<(String, String)>,
) -> Response {
    err_handler(async {
        let index = web_index(&index)
            .filter(|index| *index >= 0)
            .ok_or_else(|| ApiError::not_found("That chapter does not exist."))?;
        let payload = st.services.epub.chapter(&st.db, &book_id, index)?;
        let mut response = ok(payload);
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("private, max-age=0, must-revalidate"),
        );
        Ok(response)
    })
    .await
}

async fn epub_chapter_ensure(
    State(st): State<AppState>,
    Path((book_id, index)): Path<(String, String)>,
    mut body: Request,
) -> Response {
    err_handler(async {
        let index = web_index(&index)
            .filter(|index| *index >= 0)
            .ok_or_else(|| ApiError::not_found("That section does not exist."))?;
        // The reader sends the EPUB's own title when it has one. A body that
        // will not parse — or a title that is not a usable string — simply
        // falls back to the document's title, exactly as the web route did;
        // its `max(300)` counted UTF-16 units.
        let raw: Value = parse_json(std::mem::take(&mut body))
            .await
            .ok()
            .unwrap_or_default();
        let title = raw
            .get("title")
            .and_then(Value::as_str)
            .filter(|title| !title.is_empty() && title.encode_utf16().count() <= 300)
            .map(str::to_string);
        let chapter =
            st.services
                .epub
                .ensure_chapter(&st.db, &book_id, index, title.as_deref())?;
        Ok(multipart::created(chapter))
    })
    .await
}

async fn epub_image(
    State(st): State<AppState>,
    Path((book_id, ref_)): Path<(String, String)>,
) -> Response {
    err_handler(async {
        let resp = st.services.epub.image(&st.db, &book_id, &ref_)?;
        Ok(resp)
    })
    .await
}

async fn epub_progress_get(State(st): State<AppState>, Path(book_id): Path<String>) -> Response {
    err_handler(async {
        let progress = st.services.epub.progress(&st.db, &book_id)?;
        Ok(ok(progress))
    })
    .await
}

async fn epub_progress_patch(
    State(st): State<AppState>,
    Path(book_id): Path<String>,
    mut body: Request,
) -> Response {
    err_handler(async {
        let raw: Value = parse_json(std::mem::take(&mut body)).await?;
        let progress = st.services.epub.save_progress(&st.db, &book_id, &raw)?;
        Ok(ok(progress))
    })
    .await
}

async fn epub_translations(
    State(st): State<AppState>,
    Path(book_id): Path<String>,
    Query(q): Query<QueryMap>,
) -> Response {
    err_handler(async {
        // The web route answered for the book's file before anything else.
        let status = st.services.epub.status(&st.db, &book_id)?;
        if status.get("hasEpub").and_then(Value::as_bool) != Some(true) {
            return Err(ApiError::not_found("This book has no EPUB yet."));
        }
        // `?index=` narrows the list to one section; the reader asks that way
        // when it only needs one section's labels.
        let index = match q.get("index") {
            Some(raw) => Some(
                web_index(raw)
                    .filter(|index| *index >= 0)
                    .ok_or_else(|| ApiError::not_found("That section does not exist."))?,
            ),
            None => None,
        };
        let list = st.services.translation.list(&st.db, &book_id, index)?;
        let mut response = ok(list);
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("private, max-age=0, must-revalidate"),
        );
        Ok(response)
    })
    .await
}

async fn epub_translation_get(
    State(st): State<AppState>,
    Path((book_id, index)): Path<(String, String)>,
    Query(q): Query<QueryMap>,
) -> Response {
    err_handler(async {
        let index = web_index(&index)
            .filter(|index| *index >= 0)
            .ok_or_else(|| ApiError::not_found("That section does not exist."))?;
        let language = q.str_or("language", "");
        let item = st
            .services
            .translation
            .get(&st.db, &book_id, index, &language)?;
        let mut response = ok(item);
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("private, max-age=0, must-revalidate"),
        );
        Ok(response)
    })
    .await
}

/// `POST /api/epub/:bookId/translations/:index` — translate a section, and
/// save it.
///
/// The answer is a stream of newline-delimited frames rather than one JSON
/// document, because a section is translated in batches and can take half a
/// minute: a reader watching a spinner for thirty seconds cannot tell a
/// working request from a stalled one. Validation failures are decided before
/// the stream opens, so they keep the JSON envelope; anything after that
/// arrives as an `error` frame carrying the same message the JSON routes
/// would have sent. Nothing is written unless every batch succeeded.
async fn epub_translation_post(
    State(st): State<AppState>,
    Path((book_id, index)): Path<(String, String)>,
    mut body: Request,
) -> Response {
    let precheck: ApiResult<(i64, Value)> = async {
        let index = web_index(&index)
            .filter(|index| *index >= 0)
            .ok_or_else(|| ApiError::not_found("That section does not exist."))?;
        let raw: Value = parse_json(std::mem::take(&mut body)).await?;
        Ok((index, raw))
    }
    .await;
    let (index, raw) = match precheck {
        Ok(prepared) => prepared,
        Err(error) => return err(error),
    };
    if let Err(error) = translation_request(&raw) {
        return err(error);
    }

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Value>();
    tokio::spawn(async move {
        let progress = {
            let tx = tx.clone();
            move |done: i64, total: i64| {
                let _ = tx.send(json!({ "type": "progress", "done": done, "total": total }));
            }
        };
        // A separate sender watches for the reader hanging up; dropping the
        // future cancels the provider request, exactly as the web route's
        // `request.signal` did.
        let waiter = tx.clone();
        tokio::select! {
            outcome = st.services.translation.translate_inner(&st, &book_id, index, &raw, &progress) => {
                let frame = match outcome {
                    Ok(entry) => json!({ "type": "done", "data": entry }),
                    Err(error) => json!({ "type": "error", "code": error.code, "message": error.message }),
                };
                let _ = tx.send(frame);
            }
            () = waiter.closed() => {}
        }
    });

    let stream = futures_util::stream::unfold(rx, |mut rx| async move {
        let frame = rx.recv().await?;
        let mut line = frame.to_string();
        line.push('\n');
        Some((Ok::<_, std::convert::Infallible>(bytes::Bytes::from(line)), rx))
    });
    match Response::builder()
        .status(StatusCode::OK)
        .header(
            header::CONTENT_TYPE,
            "application/x-ndjson; charset=utf-8",
        )
        .header(header::CACHE_CONTROL, "no-store")
        // The frames must reach the reader as they are produced; a buffering
        // proxy between them and us would defeat the whole point.
        .header("x-accel-buffering", "no")
        .body(Body::from_stream(stream))
    {
        Ok(response) => response,
        Err(error) => err(ApiError::internal(format!(
            "The translation stream could not be opened: {error}"
        ))),
    }
}

async fn epub_translation_delete(
    State(st): State<AppState>,
    Path((book_id, index)): Path<(String, String)>,
    Query(q): Query<QueryMap>,
) -> Response {
    err_handler(async {
        // No language means no translation to forget — the web route said so
        // before it read the index.
        let language = q.str_or("language", "");
        if language.trim().is_empty() {
            return Err(ApiError::not_found("That translation does not exist."));
        }
        let index = web_index(&index)
            .filter(|index| *index >= 0)
            .ok_or_else(|| ApiError::not_found("That section does not exist."))?;
        let removed = st
            .services
            .translation
            .remove(&st.db, &book_id, index, &language)?;
        Ok(ok(json!({ "removed": removed })))
    })
    .await
}

/* -------------------------------------------------------------------------- */
/* Tests                                                                      */
/* -------------------------------------------------------------------------- */

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use tower::ServiceExt;

    fn fixture() -> (tempfile::TempDir, AppState) {
        let dir = tempfile::tempdir().expect("temp dir");
        let config = crate::config::Config::for_data_dir(dir.path().to_path_buf());
        let db = crate::db::Database::open(&config.database_path).expect("database");
        let state = AppState::new(
            db,
            config.clone(),
            crate::services::Services::new(&config),
        );
        (dir, state)
    }

    /// The router under test, with a fallback that says so plainly.
    fn app(state: AppState) -> Router {
        router(
            state,
            tower::service_fn(|_request: Request| async move {
                Ok::<_, std::convert::Infallible>(
                    (StatusCode::NOT_FOUND, "test spa").into_response(),
                )
            }),
        )
    }

    async fn get(app: &Router, path: &str) -> (u16, String) {
        let request = Request::builder()
            .uri(path)
            .body(Body::empty())
            .expect("request");
        let response = app.clone().oneshot(request).await.expect("response");
        let status = response.status().as_u16();
        let bytes = to_bytes(response.into_body(), 4 * 1024 * 1024)
            .await
            .unwrap_or_default();
        (status, String::from_utf8_lossy(&bytes).into_owned())
    }

    /// Building the route table is itself an assertion: axum 0.8 panics on
    /// registration for the legacy `:param` syntax and for conflicting
    /// patterns, and every other test in the crate stops at the service layer,
    /// so nothing else would notice a table that cannot be built.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn every_route_registers() {
        let (_dir, state) = fixture();
        let _app = app(state);
    }

    /// Path parameters extract through the envelope: a miss is a 404 an API
    /// client can read, never a 500 from a failed extraction.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn param_routes_extract_and_answer_with_the_envelope() {
        let (_dir, state) = fixture();
        let app = app(state);

        let (status, body) = get(&app, "/api/books/bk_missing").await;
        assert_eq!(status, 404);
        assert!(body.contains("\"not_found\""), "envelope: {body}");

        // Two path parameters in one route.
        let (status, body) = get(&app, "/api/epub/bk_missing/chapter/0").await;
        assert_eq!(status, 404);
        assert!(body.contains("This book has no EPUB yet."), "{body}");

        // A param route whose answer is a success with no data at all.
        let (status, body) = get(&app, "/api/epub/bk_missing/chapters/audio").await;
        assert_eq!(status, 200);
        assert!(body.contains("\"entries\""), "{body}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn unknown_api_paths_get_json_and_the_shell_gets_the_fallback() {
        let (_dir, state) = fixture();
        let app = app(state);

        let (status, body) = get(&app, "/api/no-such-endpoint").await;
        assert_eq!(status, 404);
        assert!(body.contains("\"error\""), "{body}");

        // Anything that is not an API path reaches the SPA fallback.
        let (status, body) = get(&app, "/library/lb_x").await;
        assert_eq!(status, 404);
        assert_eq!(body, "test spa");
    }
}
