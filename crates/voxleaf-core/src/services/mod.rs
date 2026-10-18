//! Domain services. Each module mirrors one folder of the web app's
//! `src/server/services` (or `src/server/db`) so behaviour stays identical.

pub mod audio;
pub mod audio_meta;
pub mod auth;
pub mod bookmarks;
pub mod catalogue;
pub mod covers;
pub mod epub;
pub mod home;
pub mod library;
pub mod playback;
pub mod progress;
pub mod settings;
pub mod translation;
pub mod tts;

/// Everything the API layer needs, constructed once at boot.
pub struct Services {
    pub tts: tts::TtsService,
    pub epub: epub::EpubService,
    pub translation: translation::TranslationService,
    pub catalogue: catalogue::CatalogueService,
    pub covers: covers::CoverService,
    pub audio_meta: audio_meta::AudioMeta,
}

impl Services {
    pub fn new(cfg: &crate::config::Config) -> Self {
        Self {
            tts: tts::TtsService::new(cfg),
            epub: epub::EpubService::new(cfg),
            translation: translation::TranslationService::new(cfg),
            catalogue: catalogue::CatalogueService::new(cfg),
            covers: covers::CoverService::new(cfg),
            audio_meta: audio_meta::AudioMeta::new(),
        }
    }
}
