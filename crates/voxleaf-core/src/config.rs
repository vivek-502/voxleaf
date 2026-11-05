//! Runtime configuration.
//!
//! The web app read everything from `.env`; a desktop app has no `.env`, so the
//! data directory is handed in by the shell (Tauri resolves the per-platform app
//! data directory) and everything else keeps the same environment overrides for
//! the sake of anyone scripting the app. Nothing here is exposed to the UI
//! beyond what the API already returns.

use std::path::PathBuf;
use std::sync::OnceLock;

use crate::error::{ApiError, ApiResult};

#[derive(Debug, Clone)]
pub struct Config {
    pub data_dir: PathBuf,
    pub storage_dir: PathBuf,
    pub cover_cache_dir: PathBuf,
    pub epub_dir: PathBuf,
    pub database_path: PathBuf,

    /// Local object storage for uploaded audio, rooted at `storage_dir`.
    pub storage: crate::storage::LocalStorage,

    pub book: BookConfig,
    pub epub: EpubConfig,
    pub upload: UploadConfig,
    pub translation: TranslationConfig,

    /// Optional shared token. Empty means the local-only install the app is
    /// designed around: no accounts, no sign-in.
    pub access_token: String,
}

#[derive(Debug, Clone)]
pub struct BookConfig {
    pub base_url: String,
    pub cover_base_url: String,
    pub timeout_ms: u64,
    pub offline_fallback: bool,
}

#[derive(Debug, Clone)]
pub struct EpubConfig {
    pub enabled: bool,
    pub max_bytes: u64,
    pub max_image_bytes: u64,
    pub max_documents: usize,
}

#[derive(Debug, Clone)]
pub struct UploadConfig {
    pub max_bytes: u64,
    pub allowed_extensions: Vec<&'static str>,
    pub allowed_mime_types: Vec<&'static str>,
}

#[derive(Debug, Clone)]
pub struct TranslationConfig {
    pub base_url: String,
    pub timeout_ms: u64,
    pub user_agent: String,
}

fn env_str(key: &str, fallback: &str) -> String {
    std::env::var(key).ok().filter(|value| !value.is_empty()).unwrap_or_else(|| fallback.to_string())
}

fn env_num(key: &str, fallback: u64) -> u64 {
    std::env::var(key)
        .ok()
        .filter(|value| !value.is_empty())
        .and_then(|value| value.parse().ok())
        .unwrap_or(fallback)
}

fn env_bool(key: &str, fallback: bool) -> bool {
    match std::env::var(key) {
        Ok(value) => matches!(
            value.to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => fallback,
    }
}

impl Config {
    /// Build the configuration for a given data directory, creating nothing.
    pub fn for_data_dir(data_dir: PathBuf) -> Self {
        let epub_dir = data_dir.join("epub");
        let storage_dir = data_dir.join("audio");

        let storage = crate::storage::LocalStorage::new(storage_dir.clone())
            .unwrap_or_else(|error| {
                tracing::error!(%error, "could not open the audio storage directory");
                crate::storage::LocalStorage::new(data_dir.join("audio-fallback"))
                    .expect("fallback storage directory")
            });

        Self {
            storage_dir,
            cover_cache_dir: data_dir.join("covers"),
            database_path: data_dir.join("voxleaf.db"),
            data_dir,
            epub_dir,
            storage,

            book: BookConfig {
                base_url: env_str("BOOK_API_BASE_URL", "https://openlibrary.org"),
                cover_base_url: env_str("BOOK_COVER_BASE_URL", "https://covers.openlibrary.org"),
                timeout_ms: env_num("BOOK_API_TIMEOUT_MS", 8_000),
                offline_fallback: env_bool("BOOK_OFFLINE_FALLBACK", true),
            },

            epub: EpubConfig {
                enabled: env_bool("EPUB_ENABLED", true),
                max_bytes: env_num("EPUB_MAX_SIZE_MB", 200) * 1024 * 1024,
                max_image_bytes: env_num("EPUB_MAX_IMAGE_MB", 8) * 1024 * 1024,
                max_documents: env_num("EPUB_MAX_DOCUMENTS", 4_000) as usize,
            },

            upload: UploadConfig {
                max_bytes: env_num("MAX_UPLOAD_SIZE_MB", 512) * 1024 * 1024,
                allowed_extensions: vec![
                    ".mp3", ".m4a", ".mp4", ".aac", ".wav", ".wave", ".ogg", ".oga", ".opus",
                    ".flac",
                ],
                allowed_mime_types: vec![
                    "audio/mpeg", "audio/mp3", "audio/mp4", "audio/x-m4a", "audio/aac",
                    "audio/m4a", "audio/x-aac", "audio/wav", "audio/x-wav", "audio/wave",
                    "audio/ogg", "application/ogg", "audio/flac", "audio/x-flac",
                    "application/octet-stream", "audio/mpeg3", "audio/vnd.wave",
                ],
            },

            translation: TranslationConfig {
                base_url: env_str(
                    "TRANSLATION_BASE_URL",
                    "https://translate.googleapis.com/translate_a/single",
                ),
                timeout_ms: env_num("TRANSLATION_TIMEOUT_MS", 20_000),
                user_agent: env_str(
                    "TRANSLATION_USER_AGENT",
                    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                     (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36",
                ),
            },

            access_token: env_str("APP_ACCESS_TOKEN", ""),
        }
    }

    /// Create the writable directories the app assumes exist. Idempotent.
    pub fn ensure_directories(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.data_dir)?;
        std::fs::create_dir_all(&self.cover_cache_dir)?;
        std::fs::create_dir_all(&self.epub_dir)?;
        std::fs::create_dir_all(&self.storage_dir)?;
        Ok(())
    }

    /// The secret that signs stream URLs, read (or created) once per boot.
    ///
    /// A desktop app is single-user, so the threat this guards against is a
    /// link outliving the session it was minted for — not an attacker with the
    /// data directory, who could read the audio directly.
    pub fn stream_secret(&self) -> Vec<u8> {
        static SECRET: OnceLock<Vec<u8>> = OnceLock::new();
        SECRET
            .get_or_init(|| {
                let path = self.data_dir.join(".stream-secret");
                if let Ok(existing) = std::fs::read(&path) {
                    if existing.len() >= 32 {
                        return existing;
                    }
                }
                use rand::RngCore;
                let mut bytes = vec![0u8; 32];
                rand::rng().fill_bytes(&mut bytes);
                if let Err(error) = std::fs::write(&path, &bytes) {
                    tracing::error!(%error, "could not persist the stream secret");
                }
                bytes
            })
            .clone()
    }
}

static CONFIG: OnceLock<Config> = OnceLock::new();

/// The process-wide configuration.
///
/// The shell calls [`init`] once at startup with the platform data directory;
/// until then the environment (or `./data`) decides, which is what keeps the
/// crate usable in tests without a desktop session.
pub fn init(data_dir: PathBuf) -> &'static Config {
    CONFIG.get_or_init(|| {
        let config = Config::for_data_dir(data_dir);
        if let Err(error) = config.ensure_directories() {
            tracing::error!(%error, "could not create the data directories");
        }
        config
    })
}

pub fn get() -> ApiResult<&'static Config> {
    CONFIG
        .get()
        .ok_or_else(|| ApiError::internal("The application has not finished starting yet."))
}
