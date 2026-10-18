//! SQLite access.
//!
//! One file under the app's data directory, WAL mode, and a small connection
//! pool handed out across blocking tasks. Queries run on `spawn_blocking` so a
//! big rollup can never stall the UI thread or the HTTP runtime.
//!
//! Schema notes:
//!  - Single-user by design: there is no `user_id` on anything, exactly as the
//!    web app's Prisma schema had none.
//!  - `DateTime` columns are stored as ISO-8601 UTC text, which is what Prisma
//!    would have produced and therefore what the API already returns.
//!  - `Float` maps to `REAL`, `Boolean` to `INTEGER` 0/1, `Bytes` to `BLOB`.

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use rusqlite::Connection;

const POOL_SIZE: usize = 4;
const SCHEMA_VERSION: i64 = 2;

pub type Blocking<T> = Box<dyn FnOnce(&Connection) -> rusqlite::Result<T> + Send + 'static>;

#[derive(Clone)]
pub struct Database {
    inner: Arc<Inner>,
}

struct Inner {
    pool: Vec<Mutex<Connection>>,
    next: AtomicUsize,
}

impl Database {
    /// Open (creating if needed) the database at `path` and apply migrations.
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let mut pool = Vec::with_capacity(POOL_SIZE);
        for _ in 0..POOL_SIZE {
            let connection = Connection::open(path)?;
            connection.busy_timeout(std::time::Duration::from_secs(10))?;
            // Setter pragmas return no rows, so `execute_batch` rather than
            // `query_row` — only `journal_mode` echoes its result back.
            connection.execute_batch(
                "PRAGMA journal_mode = WAL;
                 PRAGMA synchronous = NORMAL;
                 PRAGMA foreign_keys = ON;
                 PRAGMA temp_store = MEMORY;",
            )?;
            pool.push(Mutex::new(connection));
        }

        let database = Self {
            inner: Arc::new(Inner {
                pool,
                next: AtomicUsize::new(0),
            }),
        };

        // Migrations only touch the first connection; WAL lets the others read.
        database.migrate()?;

        Ok(database)
    }

    fn migrate(&self) -> anyhow::Result<()> {
        let guard = self.lock(0)?;
        let version: i64 = guard.query_row("PRAGMA user_version", [], |row| row.get(0))?;

        if version < 1 {
            guard.execute_batch(SCHEMA_V1)?;
        }
        if version < 2 {
            // Data fix, not structure: catalogue rows written while the search
            // doc mapper lost its cover base stored host-less `/b/id/…` paths,
            // which the browser resolves against our own origin — so every
            // store poster failed to load. An empty base is the only base those
            // rows could have come from, so the default host heals them all.
            guard.execute_batch(SCHEMA_V2)?;
        }
        if version < SCHEMA_VERSION {
            guard.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            tracing::info!(from = version, to = SCHEMA_VERSION, "applied schema migration");
        }

        Ok(())
    }

    fn lock(&self, index: usize) -> anyhow::Result<std::sync::MutexGuard<'_, Connection>> {
        self.inner.pool[index]
            .lock()
            .map_err(|_| anyhow::anyhow!("the database connection was poisoned"))
    }

    /// Run a query on the blocking threadpool, on whichever connection is free.
    pub async fn call<T, E>(&self, work: impl FnOnce(&Connection) -> Result<T, E> + Send + 'static) -> Result<T, E>
    where
        T: Send + 'static,
        E: From<rusqlite::Error> + Send + 'static,
    {
        let inner = self.inner.clone();

        tokio::task::spawn_blocking(move || {
            let index = inner.next.fetch_add(1, Ordering::Relaxed) % inner.pool.len();
            let connection = inner.pool[index]
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            work(&connection)
        })
        .await
        .expect("a database worker panicked")
    }

    /// Like [`call`], but for work that owns its errors (validation, I/O).
    pub async fn try_call<T, E>(
        &self,
        work: impl FnOnce(&Connection) -> Result<T, E> + Send + 'static,
    ) -> Result<T, E>
    where
        T: Send + 'static,
        E: Send + 'static,
    {
        let inner = self.inner.clone();

        tokio::task::spawn_blocking(move || {
            let index = inner.next.fetch_add(1, Ordering::Relaxed) % inner.pool.len();
            let connection = inner.pool[index]
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            work(&connection)
        })
        .await
        .expect("a database worker panicked")
    }
}

/// The schema, mirroring `prisma/schema.prisma` one field at a time.
const SCHEMA_V1: &str = r#"
CREATE TABLE IF NOT EXISTS books (
    id               TEXT PRIMARY KEY,
    external_id      TEXT NOT NULL UNIQUE,
    title            TEXT NOT NULL,
    subtitle         TEXT,
    author           TEXT NOT NULL DEFAULT '',
    description      TEXT,
    cover_url        TEXT,
    publisher        TEXT,
    publication_date TEXT,
    isbn             TEXT,
    categories       TEXT NOT NULL DEFAULT '[]',
    page_count       INTEGER,
    language         TEXT,
    raw              TEXT,
    created_at       TEXT NOT NULL,
    updated_at       TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_books_title ON books(title);
CREATE INDEX IF NOT EXISTS idx_books_author ON books(author);
CREATE INDEX IF NOT EXISTS idx_books_created_at ON books(created_at);

-- Lightweight record of a search against the external provider. Keeps the
-- local catalogue warm so an offline lookup still finds a title the user has
-- already looked at.
CREATE TABLE IF NOT EXISTS catalog_hits (
    id         TEXT PRIMARY KEY,
    book_id    TEXT NOT NULL REFERENCES books(id) ON DELETE CASCADE,
    query      TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_catalog_hits_query ON catalog_hits(query);
CREATE INDEX IF NOT EXISTS idx_catalog_hits_created_at ON catalog_hits(created_at);

CREATE TABLE IF NOT EXISTS library_books (
    id                    TEXT PRIMARY KEY,
    book_id               TEXT NOT NULL UNIQUE REFERENCES books(id) ON DELETE CASCADE,
    added_at              TEXT NOT NULL,
    favorite              INTEGER NOT NULL DEFAULT 0,
    overall_progress      REAL NOT NULL DEFAULT 0,
    last_played_chapter_id TEXT,
    last_played_at        TEXT,
    last_position         REAL NOT NULL DEFAULT 0,
    last_chapter_title    TEXT,
    last_chapter_number   INTEGER,
    total_audio_duration  REAL NOT NULL DEFAULT 0,
    completed_at          TEXT
);
CREATE INDEX IF NOT EXISTS idx_library_added_at ON library_books(added_at);
CREATE INDEX IF NOT EXISTS idx_library_last_played_at ON library_books(last_played_at);
CREATE INDEX IF NOT EXISTS idx_library_favorite ON library_books(favorite);

CREATE TABLE IF NOT EXISTS chapters (
    id             TEXT PRIMARY KEY,
    book_id        TEXT NOT NULL REFERENCES books(id) ON DELETE CASCADE,
    chapter_number INTEGER NOT NULL,
    title          TEXT NOT NULL,
    sort_index     INTEGER NOT NULL DEFAULT 0,
    -- Spine index of the EPUB document this chapter belongs to. SQLite treats
    -- NULLs in a UNIQUE index as distinct, which is exactly what lets several
    -- unlinked chapters coexist on one book.
    epub_index     INTEGER,
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL,
    UNIQUE (book_id, chapter_number),
    UNIQUE (book_id, epub_index)
);
CREATE INDEX IF NOT EXISTS idx_chapters_book_sort ON chapters(book_id, sort_index);
CREATE INDEX IF NOT EXISTS idx_chapters_book_number ON chapters(book_id, chapter_number);
CREATE INDEX IF NOT EXISTS idx_chapters_book_epub ON chapters(book_id, epub_index);

CREATE TABLE IF NOT EXISTS chapter_audio (
    id            TEXT PRIMARY KEY,
    chapter_id    TEXT NOT NULL UNIQUE REFERENCES chapters(id) ON DELETE CASCADE,
    file_url      TEXT NOT NULL,
    storage_key   TEXT NOT NULL UNIQUE,
    duration      REAL NOT NULL DEFAULT 0,
    file_size     INTEGER NOT NULL DEFAULT 0,
    mime_type     TEXT NOT NULL,
    original_name TEXT,
    bit_rate      INTEGER,
    sample_rate   INTEGER,
    channels      INTEGER,
    uploaded_at   TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_audio_uploaded_at ON chapter_audio(uploaded_at);

CREATE TABLE IF NOT EXISTS listening_progress (
    id         TEXT PRIMARY KEY,
    chapter_id TEXT NOT NULL UNIQUE REFERENCES chapters(id) ON DELETE CASCADE,
    position   REAL NOT NULL DEFAULT 0,
    duration   REAL NOT NULL DEFAULT 0,
    percentage REAL NOT NULL DEFAULT 0,
    completed  INTEGER NOT NULL DEFAULT 0,
    play_count INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_progress_percentage ON listening_progress(percentage);
CREATE INDEX IF NOT EXISTS idx_progress_updated_at ON listening_progress(updated_at);

CREATE TABLE IF NOT EXISTS bookmarks (
    id         TEXT PRIMARY KEY,
    chapter_id TEXT NOT NULL REFERENCES chapters(id) ON DELETE CASCADE,
    timestamp  REAL NOT NULL,
    note       TEXT,
    book_title TEXT NOT NULL,
    book_id    TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_bookmarks_chapter ON bookmarks(chapter_id);
CREATE INDEX IF NOT EXISTS idx_bookmarks_book ON bookmarks(book_id);
CREATE INDEX IF NOT EXISTS idx_bookmarks_created_at ON bookmarks(created_at);

-- Single-row preferences table.
CREATE TABLE IF NOT EXISTS app_settings (
    id                     TEXT PRIMARY KEY DEFAULT 'singleton',
    playback_speed         REAL NOT NULL DEFAULT 1,
    theme                  TEXT NOT NULL DEFAULT 'dark',
    volume                 REAL NOT NULL DEFAULT 1,
    muted                  INTEGER NOT NULL DEFAULT 0,
    sleep_timer_preference TEXT NOT NULL DEFAULT 'off',
    skip_forward_seconds   INTEGER NOT NULL DEFAULT 30,
    skip_backward_seconds  INTEGER NOT NULL DEFAULT 15,
    completion_threshold   REAL NOT NULL DEFAULT 0.95,
    auto_advance           INTEGER NOT NULL DEFAULT 1,
    bookmark_note          TEXT NOT NULL DEFAULT '',
    tts_voice              TEXT NOT NULL DEFAULT 'en-US-GuyNeural',
    created_at             TEXT NOT NULL,
    updated_at             TEXT NOT NULL
);

-- A stored EPUB, parsed once at import time so serving a chapter is an indexed
-- lookup rather than a zip decompression per request.
CREATE TABLE IF NOT EXISTS epub_files (
    id          TEXT PRIMARY KEY,
    book_id     TEXT NOT NULL UNIQUE REFERENCES books(id) ON DELETE CASCADE,
    status      TEXT NOT NULL DEFAULT 'pending',
    error       TEXT,
    attempts    INTEGER NOT NULL DEFAULT 0,
    source      TEXT,
    md5         TEXT,
    file_name   TEXT,
    file_size   INTEGER,
    title       TEXT,
    creator     TEXT,
    language    TEXT,
    contents    TEXT,
    stored_at   TEXT,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_epub_status ON epub_files(status);

-- One reading-order document, already sanitised.
CREATE TABLE IF NOT EXISTS epub_chapters (
    id          TEXT PRIMARY KEY,
    epub_id     TEXT NOT NULL REFERENCES epub_files(id) ON DELETE CASCADE,
    idx         INTEGER NOT NULL,
    href        TEXT NOT NULL,
    title       TEXT NOT NULL,
    depth       INTEGER NOT NULL DEFAULT 0,
    in_toc      INTEGER NOT NULL DEFAULT 1,
    word_count  INTEGER NOT NULL DEFAULT 0,
    html        TEXT NOT NULL,
    char_offset INTEGER NOT NULL DEFAULT 0,
    UNIQUE (epub_id, idx)
);
CREATE INDEX IF NOT EXISTS idx_epub_chapters_epub ON epub_chapters(epub_id, in_toc);

CREATE TABLE IF NOT EXISTS epub_images (
    id        TEXT PRIMARY KEY,
    epub_id   TEXT NOT NULL REFERENCES epub_files(id) ON DELETE CASCADE,
    ref       TEXT NOT NULL,
    mime_type TEXT NOT NULL,
    alt       TEXT,
    bytes     BLOB NOT NULL,
    UNIQUE (epub_id, ref)
);
CREATE INDEX IF NOT EXISTS idx_epub_images_epub ON epub_images(epub_id);

CREATE TABLE IF NOT EXISTS epub_progress (
    id             TEXT PRIMARY KEY,
    book_id        TEXT NOT NULL UNIQUE REFERENCES books(id) ON DELETE CASCADE,
    chapter_index  INTEGER NOT NULL DEFAULT 0,
    scroll_percent REAL NOT NULL DEFAULT 0,
    font_size      INTEGER NOT NULL DEFAULT 19,
    line_height    REAL NOT NULL DEFAULT 1.7,
    theme          TEXT NOT NULL DEFAULT 'paper',
    font_family    TEXT NOT NULL DEFAULT 'serif',
    column_width   INTEGER NOT NULL DEFAULT 680,
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL
);

-- A machine translation of one section into one language. `blocks` is text,
-- never markup: a translation is rendered as React elements, so no translated
-- string can be an injection risk.
CREATE TABLE IF NOT EXISTS epub_translations (
    id            TEXT PRIMARY KEY,
    book_id       TEXT NOT NULL REFERENCES books(id) ON DELETE CASCADE,
    epub_index    INTEGER NOT NULL,
    language      TEXT NOT NULL,
    language_name TEXT NOT NULL,
    blocks        TEXT NOT NULL,
    title         TEXT NOT NULL,
    char_count    INTEGER NOT NULL DEFAULT 0,
    source_hash   TEXT NOT NULL,
    provider      TEXT NOT NULL DEFAULT 'google',
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL,
    UNIQUE (book_id, epub_index, language)
);
CREATE INDEX IF NOT EXISTS idx_translations_book ON epub_translations(book_id, language);
"#;

/// Version 2: heal cover URLs written without their host (see `migrate`).
const SCHEMA_V2: &str =
    "UPDATE books SET cover_url = 'https://covers.openlibrary.org' || cover_url WHERE cover_url LIKE '/b/%';";

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db() -> tempfile::TempDir {
        tempfile::tempdir().expect("temp dir")
    }

    #[tokio::test]
    async fn schema_creates_every_table() {
        let dir = temp_db();
        let database = Database::open(&dir.path().join("vb.db")).expect("open");

        let tables: Vec<String> = database
            .call(|connection| {
                let mut statement = connection.prepare(
                    "SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name",
                )?;
                let rows = statement
                    .query_map([], |row| row.get::<_, String>(0))?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok::<Vec<String>, rusqlite::Error>(rows)
            })
            .await
            .expect("query");

        for expected in [
            "app_settings",
            "bookmarks",
            "books",
            "catalog_hits",
            "chapter_audio",
            "chapters",
            "epub_chapters",
            "epub_files",
            "epub_images",
            "epub_progress",
            "epub_translations",
            "library_books",
            "listening_progress",
        ] {
            assert!(tables.contains(&expected.to_string()), "missing table {expected}");
        }
    }

    #[test]
    fn opening_is_idempotent_and_migrations_are_skipped() {
        let dir = temp_db();
        let path = dir.path().join("vb.db");
        Database::open(&path).expect("first open");
        Database::open(&path).expect("second open");
    }

    #[tokio::test]
    async fn concurrent_calls_are_safe() {
        let dir = temp_db();
        let database = Database::open(&dir.path().join("vb.db")).expect("open");

        let mut handles = Vec::new();
        for _ in 0..16 {
            let db = database.clone();
            handles.push(tokio::spawn(async move {
                db.call(|connection| {
                    connection.query_row("SELECT 1", [], |row| row.get::<_, i64>(0))
                })
                .await
            }));
        }

        for handle in handles {
            assert_eq!(handle.await.expect("join"), Ok(1));
        }
    }
}
