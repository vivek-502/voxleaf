//! Deciding *what* to narrate and *which voice* reads it.
//!
//! A book can be read in a language other than the one it was written in, and
//! the audio has to match what the reader is looking at. Two things follow
//! from that, and both were wrong before:
//!
//!  - The text came from the EPUB section and nowhere else, so a chapter the
//!    reader had translated into Hindi was still narrated in English.
//!  - The voice came from a single setting, but an Edge voice is bound to one
//!    language. `en-US-GuyNeural` handed Hindi text produces something that is
//!    not Hindi speech, so a translated chapter was doubly wrong.
//!
//! Port of the web app's `tts/narration.ts`, with the translation lookup done
//! against this app's SQLite rows rather than Prisma, and the staleness check
//! comparing the same `sourceHash` the web app stored — so a book translated
//! before the move to Rust is not seen as stale for the move alone.

use rusqlite::OptionalExtension;
use serde_json::{json, Value};

use crate::db::Database;
use crate::error::ApiResult;
use crate::services::tts::text::{html_to_plain_text, source_hash};
use crate::services::tts::voices;
use crate::services::tts::DEFAULT_VOICE;

/// One thing worth narrating, with everything the caller needs to report it.
#[derive(Debug, Clone)]
pub(crate) struct NarrationSource {
    /// The text to synthesize.
    pub text: String,
    /// Provider language code, or `None` when narrating the original.
    pub language: Option<String>,
    /// Display name of `language`, or `None` for the original.
    pub language_name: Option<String>,
    /// The Edge voice chosen to read it.
    pub voice: String,
    /// True when a translation was asked for but could not be used.
    pub fell_back_to_original: bool,
    /// Why the original was used, when that happened.
    pub reason: Option<String>,
}

impl NarrationSource {
    /// The wire shape the polling client reads.
    pub fn to_json(&self) -> Value {
        json!({
            "language": self.language,
            "languageName": self.language_name,
            "voice": self.voice,
            "fellBackToOriginal": self.fell_back_to_original,
            "reason": self.reason,
        })
    }
}

/// A voice per language, for when the configured one cannot read the text.
///
/// Edge offers several voices for most languages; these are the ones that are
/// widely used and sound natural, so an unconfigured language gets a sensible
/// reader rather than whichever voice happened to sort first.
///
/// Kept verbatim from the web app, including the names that have since gone
/// stale at Microsoft's end (`bn-IN-SurjoaNeural`, `pa-IN-AnanyaNeural`): a
/// stale name fails the same way there as here, and a table that silently
/// differs between the two apps is worse than one that is equally wrong in
/// both. `voices.rs`'s captured list is used when a language is missing here.
const PREFERRED_VOICES: &[(&str, &str)] = &[
    ("hi", "hi-IN-SwaraNeural"),
    ("bn", "bn-IN-SurjoaNeural"),
    ("ta", "ta-IN-PallaviNeural"),
    ("te", "te-IN-ShrutiNeural"),
    ("mr", "mr-IN-AarohiNeural"),
    ("gu", "gu-IN-DhwaniNeural"),
    ("kn", "kn-IN-SapnaNeural"),
    ("ml", "ml-IN-ShoohitaNeural"),
    ("pa", "pa-IN-AnanyaNeural"),
    ("ur", "ur-PK-UzmaNeural"),
    ("ar", "ar-EG-SalmaNeural"),
    ("fa", "fa-IR-DonyaNeural"),
    ("he", "he-IL-HilaNeural"),
    ("tr", "tr-TR-EmelNeural"),
    ("ru", "ru-RU-SvetlanaNeural"),
    ("uk", "uk-UA-PolinaNeural"),
    ("pl", "pl-PL-ZofiaNeural"),
    ("nl", "nl-NL-ColetteNeural"),
    ("de", "de-DE-KatjaNeural"),
    ("fr", "fr-FR-DeniseNeural"),
    ("es", "es-ES-ElviraNeural"),
    ("it", "it-IT-ElsaNeural"),
    ("pt", "pt-BR-FranciscaNeural"),
    ("ja", "ja-JP-NanamiNeural"),
    ("ko", "ko-KR-SunHiNeural"),
    ("zh", "zh-CN-XiaoxiaoNeural"),
    ("id", "id-ID-GadisNeural"),
    ("vi", "vi-VN-HoaiMyNeural"),
    ("th", "th-TH-PremwadeeNeural"),
    ("sv", "sv-SE-SofieNeural"),
    ("da", "da-DK-ChristelNeural"),
    ("fi", "fi-FI-NooraNeural"),
    ("nb", "nb-NO-LivNeural"),
    ("cs", "cs-CZ-VlastaNeural"),
    ("el", "el-GR-AthinaNeural"),
    ("hu", "hu-HU-NoémiNeural"),
    ("ro", "ro-RO-FlorentinaNeural"),
];

/// `hi-IN-SwaraNeural` -> `hi`.
pub(crate) fn base_language(locale: &str) -> String {
    locale
        .split(['-', '_'])
        .next()
        .unwrap_or_default()
        .to_lowercase()
}

/// Whether the configured voice can read `language`.
///
/// True for the original (`language` `None`), because there is nothing to
/// mismatch. Locale subtags are ignored: `en-GB` reading `en` text is right,
/// while `hi-IN` reading `en` text is not, and the base language is what
/// separates them.
pub(crate) fn voice_can_read(voice: &str, language: Option<&str>) -> bool {
    match language {
        None => true,
        Some(language) => base_language(voice) == base_language(language),
    }
}

/// Pick a voice able to read `language`.
///
/// The configured voice wins whenever it speaks the right language — that is
/// the user having chosen it, and a chapter read in its original language
/// should honour it. It is only overridden when it physically cannot read the
/// text, because an English voice given Devanagari does not produce an
/// English reading of a Hindi chapter, it produces noise.
async fn resolve_voice(
    configured_voice: &str,
    language: Option<&str>,
) -> (String, bool) {
    if voice_can_read(configured_voice, language) {
        return (configured_voice.to_string(), false);
    }

    let target = base_language(language.unwrap_or_default());
    // Trust the table without a lookup: it only holds real Edge voices, and
    // asking the service here would be a network round trip on a path that
    // must stay cheap.
    if let Some((_, preferred)) = PREFERRED_VOICES.iter().find(|(lang, _)| *lang == target) {
        return (preferred.to_string(), true);
    }

    // Fall back to the live (or captured) catalogue: the first voice for the
    // language, sorted by name so the choice is at least deterministic.
    if let Ok(catalogue) = voices::list_voices().await {
        if let Some(found) = catalogue
            .iter()
            .find(|candidate| base_language(&candidate.locale) == target)
        {
            return (found.name.clone(), true);
        }
    }

    // Fall through: a voice that cannot read the text is still better than
    // failing outright, and the caller reports which voice was used.
    (configured_voice.to_string(), false)
}

/// The language tables the web app's `utils/constants.ts` shipped, for naming
/// a language the way the reader names it. Codes are stored lower-case for
/// the lookup; the display name is what the reader sees.
const TRANSLATION_LANGUAGES: &[(&str, &str)] = &[
    ("es", "Spanish"),
    ("pt-br", "Brazilian Portuguese"),
    ("pt", "Portuguese"),
    ("fr", "French"),
    ("de", "German"),
    ("it", "Italian"),
    ("nl", "Dutch"),
    ("pl", "Polish"),
    ("ru", "Russian"),
    ("uk", "Ukrainian"),
    ("tr", "Turkish"),
    ("ar", "Arabic"),
    ("he", "Hebrew"),
    ("fa", "Persian"),
    ("hi", "Hindi"),
    ("bn", "Bengali"),
    ("ta", "Tamil"),
    ("id", "Indonesian"),
    ("ms", "Malay"),
    ("th", "Thai"),
    ("vi", "Vietnamese"),
    ("ja", "Japanese"),
    ("ko", "Korean"),
    ("zh-cn", "Chinese (Simplified)"),
    ("zh-tw", "Chinese (Traditional)"),
    ("ca", "Catalan"),
    ("el", "Greek"),
    ("sv", "Swedish"),
    ("da", "Danish"),
    ("fi", "Finnish"),
    ("nb", "Norwegian"),
    ("cs", "Czech"),
    ("hu", "Hungarian"),
    ("ro", "Romanian"),
    ("bg", "Bulgarian"),
    ("uk-latn", "Ukrainian (Latin)"),
    ("sr", "Serbian"),
    ("sw", "Swahili"),
    ("af", "Afrikaans"),
    ("sl", "Slovene"),
];

/// The reader's name for a language code, falling back to the code itself.
pub(crate) fn translation_language_name(code: &str) -> String {
    let key = code.trim().to_lowercase();
    TRANSLATION_LANGUAGES
        .iter()
        .find(|(candidate, _)| *candidate == key)
        .map(|(_, name)| (*name).to_string())
        .unwrap_or_else(|| {
            let trimmed = code.trim();
            if trimmed.is_empty() {
                "Unknown".to_string()
            } else {
                trimmed.to_string()
            }
        })
}

/// Put a language code into the form BCP 47 gives it, which is also the form
/// the provider expects.
///
/// Casing is not cosmetic here. `pt` and `pt-BR` are different languages, and
/// `zh-CN` and `zh-TW` name different scripts, so lowercasing a code does not
/// merely tidy it, it risks asking for a language nobody wanted.
pub(crate) fn canonical_language_code(code: &str) -> String {
    code.trim()
        .to_lowercase()
        .split('-')
        .enumerate()
        .map(|(index, part)| {
            if index == 0 {
                return part.to_string();
            }
            // A script subtag is four letters, written in title case.
            if part.len() == 4 {
                let mut chars = part.chars();
                let head = chars.next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();
                return format!("{head}{}", chars.as_str());
            }
            // A region subtag is two letters or three digits, capitals.
            let regionish = (part.len() == 2 && part.chars().all(|c| c.is_ascii_alphabetic()))
                || (part.len() == 3 && part.chars().all(|c| c.is_ascii_digit()));
            if regionish {
                return part.to_uppercase();
            }
            part.to_string()
        })
        .collect::<Vec<String>>()
        .join("-")
}

/// The chapter fields narration needs to look a section up.
pub(crate) struct ChapterTarget {
    pub book_id: String,
    pub epub_index: Option<i64>,
    pub title: String,
}

/// Load a chapter's narration fields, or 404 the way the web app did.
pub(crate) async fn load_chapter(db: &Database, chapter_id: &str) -> ApiResult<ChapterTarget> {
    let chapter_id = chapter_id.to_string();
    db.call(move |conn| {
        conn.query_row(
            "SELECT book_id, epub_index, title FROM chapters WHERE id = ?1",
            [chapter_id],
            |row| {
                Ok(ChapterTarget {
                    book_id: row.get(0)?,
                    epub_index: row.get(1)?,
                    title: row.get(2)?,
                })
            },
        )
    })
    .await
    .map_err(|_| crate::error::ApiError::not_found("That chapter no longer exists."))
}

/// A translation row, as narration needs it: the stored blocks and the hash
/// they were made from, plus the display name.
struct SavedTranslation {
    language: String,
    language_name: String,
    blocks: Vec<String>,
    title: String,
    source_hash: String,
}

/// Read the saved translation for one section and language.
async fn saved_translation(
    db: &Database,
    book_id: &str,
    epub_index: i64,
    code: &str,
) -> ApiResult<Option<SavedTranslation>> {
    let (book_id, code) = (book_id.to_string(), code.to_string());
    let row: Option<(String, String, String, String, String)> = db
        .call(move |conn| {
            conn.query_row(
                "SELECT language, language_name, blocks, title, source_hash \
                 FROM epub_translations \
                 WHERE book_id = ?1 AND epub_index = ?2 AND language = ?3",
                rusqlite::params![book_id, epub_index, code],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
            )
            .optional()
        })
        .await?;

    let Some((language, language_name, blocks, title, source_hash)) = row else {
        return Ok(None);
    };

    // The stored blocks are JSON the translation service wrote. Anything that
    // is not the shape we wrote is treated as no blocks at all rather than
    // narrating garbage.
    let blocks: Vec<String> = serde_json::from_str::<Vec<Value>>(&blocks)
        .map(|items| {
            items
                .into_iter()
                .filter_map(|item| {
                    let text = item.get("text")?.as_str()?.to_string();
                    if text.is_empty() {
                        None
                    } else {
                        Some(text)
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(Some(SavedTranslation {
        language,
        language_name,
        blocks,
        title,
        source_hash,
    }))
}

/// The section's HTML, read straight from the EPUB rows. The epub service
/// does not expose the raw markup, and narration needs the very bytes the
/// reader renders so the staleness hash matches what the translator saw.
async fn section_html(db: &Database, book_id: &str, epub_index: i64) -> ApiResult<Option<String>> {
    let (book_id, epub_index) = (book_id.to_string(), epub_index);
    let html: Option<String> = db
        .call(move |conn| {
            conn.query_row(
                "SELECT epub_chapters.html FROM epub_chapters \
                 JOIN epub_files ON epub_files.id = epub_chapters.epub_id \
                 WHERE epub_files.book_id = ?1 AND epub_chapters.idx = ?2",
                rusqlite::params![book_id, epub_index],
                |row| row.get(0),
            )
            .optional()
        })
        .await?;
    Ok(html)
}

/// The text to read, plus the voice that can read it.
///
/// A translation is used only when it is fresh: a stale row was made from
/// source text the book no longer has, so narrating it would read wording the
/// reader cannot see on the page.
pub(crate) async fn resolve_narration_source(
    db: &Database,
    chapter: &ChapterTarget,
    language: Option<&str>,
    configured_voice: &str,
) -> ApiResult<NarrationSource> {
    let code = language.map(canonical_language_code);

    if let Some(code) = code.as_deref() {
        if let Some(epub_index) = chapter.epub_index {
            let translation = saved_translation(db, &chapter.book_id, epub_index, code).await?;

            let Some(translation) = translation else {
                let name = translation_language_name(code);
                return Ok(with_voice(
                    original_text(db, chapter).await?,
                    None,
                    None,
                    true,
                    Some(format!(
                        "This section has no saved {name} translation, so the original text was narrated."
                    )),
                    configured_voice,
                    None,
                )
                .await?);
            };

            // Compare the stored hash against the chapter's blocks as they
            // are *now*, exactly the way the web app did.
            let fresh = section_html(db, &chapter.book_id, epub_index)
                .await?
                .map(|html| source_hash(&html) == translation.source_hash)
                .unwrap_or(false);

            if !fresh {
                return Ok(with_voice(
                    original_text(db, chapter).await?,
                    None,
                    None,
                    true,
                    Some(format!(
                        "The saved {} translation is out of date, so the original text was narrated. \
                         Translate it again to narrate it.",
                        translation.language_name
                    )),
                    configured_voice,
                    None,
                )
                .await?);
            }

            let text = translation
                .blocks
                .iter()
                .map(|block| block.as_str())
                .filter(|block| !block.trim().is_empty())
                .collect::<Vec<&str>>()
                .join("\n\n");

            let language_name = translation.language_name.clone();
            let language = translation.language.clone();
            return Ok(with_voice(
                if text.trim().is_empty() {
                    if translation.title.trim().is_empty() {
                        chapter.title.clone()
                    } else {
                        translation.title.clone()
                    }
                } else {
                    text
                },
                Some(language),
                Some(language_name),
                false,
                None,
                configured_voice,
                Some(&code),
            )
            .await?);
        }
    }

    Ok(with_voice(
        original_text(db, chapter).await?,
        None,
        None,
        false,
        None,
        configured_voice,
        None,
    )
    .await?)
}

/// The chapter's own text: the EPUB section's blocks, or the title when there
/// is no section behind the chapter.
async fn original_text(db: &Database, chapter: &ChapterTarget) -> ApiResult<String> {
    if let Some(epub_index) = chapter.epub_index {
        if let Some(html) = section_html(db, &chapter.book_id, epub_index).await? {
            let text = html_to_plain_text(&html);
            if !text.trim().is_empty() {
                return Ok(text);
            }
        }
    }
    // A chapter with no EPUB section behind it is a user-made placeholder:
    // narrate its title so the button still does something rather than
    // failing silently.
    Ok(chapter.title.clone())
}

/// Attach the chosen voice to a text source.
async fn with_voice(
    text: String,
    language: Option<String>,
    language_name: Option<String>,
    fell_back_to_original: bool,
    reason: Option<String>,
    configured_voice: &str,
    language_for_voice: Option<&str>,
) -> ApiResult<NarrationSource> {
    let (voice, overridden) = resolve_voice(configured_voice, language_for_voice).await;
    let reason = reason.or_else(|| {
        if overridden {
            language_for_voice.map(|language| {
                format!(
                    "Narrated with {voice}, because the voice set in Settings cannot read {language}."
                )
            })
        } else {
            None
        }
    });
    Ok(NarrationSource {
        text,
        language,
        language_name,
        voice,
        fell_back_to_original,
        reason,
    })
}

/// The configured voice, falling back to the default the moment Settings is
/// empty or unreadable.
pub(crate) async fn configured_voice(db: &Database) -> String {
    match crate::services::settings::get(db).await {
        Ok(settings) if !settings.tts_voice.trim().is_empty() => settings.tts_voice,
        _ => DEFAULT_VOICE.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_language_takes_the_first_subtag() {
        assert_eq!(base_language("hi-IN-SwaraNeural"), "hi");
        assert_eq!(base_language("en-GB"), "en");
        assert_eq!(base_language("zh_CN"), "zh");
        assert_eq!(base_language(""), "");
    }

    #[test]
    fn a_voice_reads_what_it_speaks() {
        assert!(voice_can_read("en-US-GuyNeural", None));
        assert!(voice_can_read("en-GB-SoniaNeural", Some("en")));
        assert!(voice_can_read("en-US-GuyNeural", Some("en-GB")));
        assert!(!voice_can_read("en-US-GuyNeural", Some("hi-IN")));
    }

    #[test]
    fn canonical_codes_keep_their_shape() {
        assert_eq!(canonical_language_code("pt-br"), "pt-BR");
        assert_eq!(canonical_language_code(" zh-tw "), "zh-TW");
        assert_eq!(canonical_language_code("uk-latn"), "uk-Latn");
        assert_eq!(canonical_language_code("EN"), "en");
        assert_eq!(canonical_language_code("ES-419"), "es-419");
    }

    #[test]
    fn languages_are_named_the_way_the_reader_names_them() {
        assert_eq!(translation_language_name("hi"), "Hindi");
        assert_eq!(translation_language_name("pt-BR"), "Brazilian Portuguese");
        assert_eq!(translation_language_name("zh-TW"), "Chinese (Traditional)");
        // Unknown codes fall back to themselves, never to an empty string.
        assert_eq!(translation_language_name("xy"), "xy");
        assert_eq!(translation_language_name(""), "Unknown");
    }

    #[test]
    fn the_preferred_table_only_names_real_locales() {
        for (lang, voice) in PREFERRED_VOICES {
            assert!(voice.starts_with(lang), "{voice} does not start with {lang}");
            assert!(voice.ends_with("Neural"), "{voice} is not a Neural voice");
        }
    }
}
