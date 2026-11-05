//! The word lookup behind "define selection", and the reader's notebook.
//!
//! Port of `src/server/services/dictionary.ts`. English goes to
//! dictionaryapi.dev first because its entries are already split into senses;
//! everything else — and English it does not know — goes to the matching
//! Wiktionary. A provider being down is reported as such, separately from a word
//! that simply has no entry.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use regex::Regex;
use rusqlite::OptionalExtension;
use serde_json::{json, Map, Value};

use crate::db::Database;
use crate::error::{ApiError, ApiResult};
use crate::repo;
use crate::services::library::block_db;
use crate::services::translation::{canonical_language_code, is_valid_language_code};

const LOOKUP_TIMEOUT: Duration = Duration::from_secs(6);
const POSITIVE_TTL: Duration = Duration::from_secs(12 * 60 * 60);
const NEGATIVE_TTL: Duration = Duration::from_secs(5 * 60);
const CACHE_LIMIT: usize = 500;
const WORD_LIMIT: usize = 80;
const PHONETIC_LIMIT: usize = 200;
const SOURCE_NAME_LIMIT: usize = 80;
const SOURCE_URL_LIMIT: usize = 500;
const SENSE_LIMIT: usize = 12;
const DEFINITIONS_PER_SENSE: usize = 20;
const PART_OF_SPEECH_LIMIT: usize = 80;
const DEFINITION_LIMIT: usize = 4000;
const EXAMPLE_LIMIT: usize = 2000;
const SAVED_WORDS_LIMIT: i64 = 500;

struct CacheEntry {
    expires: Instant,
    result: Option<Value>,
}

fn cache() -> &'static Mutex<HashMap<String, CacheEntry>> {
    static CACHE: OnceLock<Mutex<HashMap<String, CacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cache_get(key: &str) -> Option<Option<Value>> {
    let mut cache = cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    match cache.get(key) {
        Some(entry) if entry.expires > Instant::now() => Some(entry.result.clone()),
        Some(_) => {
            cache.remove(key);
            None
        }
        None => None,
    }
}

fn cache_set(key: String, result: Option<Value>) {
    let mut cache = cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if cache.len() >= CACHE_LIMIT {
        if let Some(oldest) = cache.keys().next().cloned() {
            cache.remove(&oldest);
        }
    }
    let ttl = if result.is_some() {
        POSITIVE_TTL
    } else {
        NEGATIVE_TTL
    };
    cache.insert(
        key,
        CacheEntry {
            expires: Instant::now() + ttl,
            result,
        },
    );
}

fn http() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(LOOKUP_TIMEOUT)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new())
    })
}

fn entity_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"(?i)&(#x?[0-9a-f]+|[a-z]+);").expect("entity regex"))
}

fn sup_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"(?is)<sup[^>]*>.*?</sup>").expect("sup regex"))
}

fn style_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"(?is)<style[^>]*>.*?</style>").expect("style regex"))
}

fn tag_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"<[^>]+>").expect("tag regex"))
}

fn named_entity(name: &str) -> Option<&'static str> {
    Some(match name {
        "amp" => "&",
        "lt" => "<",
        "gt" => ">",
        "quot" => "\"",
        "apos" => "'",
        "nbsp" => " ",
        "ndash" => "–",
        "mdash" => "—",
        "lsquo" => "‘",
        "rsquo" => "’",
        "ldquo" => "“",
        "rdquo" => "”",
        "hellip" => "…",
        _ => return None,
    })
}

fn decode_entities(value: &str) -> String {
    entity_pattern()
        .replace_all(value, |captures: &regex::Captures<'_>| {
            let entity = &captures[1];
            if let Some(hex) = entity
                .strip_prefix("#x")
                .or_else(|| entity.strip_prefix("#X"))
            {
                return u32::from_str_radix(hex, 16)
                    .ok()
                    .and_then(char::from_u32)
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| captures[0].to_string());
            }
            if let Some(decimal) = entity.strip_prefix('#') {
                return decimal
                    .parse::<u32>()
                    .ok()
                    .and_then(char::from_u32)
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| captures[0].to_string());
            }
            named_entity(&entity.to_lowercase())
                .map(str::to_string)
                .unwrap_or_else(|| captures[0].to_string())
        })
        .into_owned()
}

/// Wiktionary definitions carry links and superscript labels; readers get text.
fn plain_text(value: &Value) -> String {
    let Some(raw) = value.as_str() else {
        return String::new();
    };
    let without_sup = sup_pattern().replace_all(raw, "");
    let without_style = style_pattern().replace_all(without_sup.as_ref(), "");
    let without_tags = tag_pattern().replace_all(without_style.as_ref(), "");
    decode_entities(&without_tags)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// A word is one thing a reader can select; refuse anything that is not.
fn clean_word(word: &str) -> ApiResult<String> {
    let cleaned: String = collapse_whitespace(word.trim())
        .chars()
        .take(WORD_LIMIT)
        .collect();
    if cleaned.is_empty() || !cleaned.chars().any(char::is_alphabetic) {
        return Err(ApiError::bad_request(
            "Select a word or short phrase to look up.",
        ));
    }
    Ok(cleaned)
}

fn collapse_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn from_free_dictionary(entries: &Value, word: &str) -> Option<Value> {
    let entry = entries
        .as_array()?
        .iter()
        .find(|item| item.get("meanings").and_then(Value::as_array).is_some())?;

    let mut senses: Vec<Value> = Vec::new();
    for meaning in entry
        .get("meanings")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let definitions: Vec<Value> = meaning
            .get("definitions")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|item| {
                let definition = plain_text(item.get("definition")?);
                if definition.is_empty() {
                    return None;
                }
                let mut value = Map::new();
                value.insert("definition".to_string(), Value::String(definition));
                let example = plain_text(item.get("example").unwrap_or(&Value::Null));
                if !example.is_empty() {
                    value.insert("example".to_string(), Value::String(example));
                }
                Some(Value::Object(value))
            })
            .take(8)
            .collect();
        if definitions.is_empty() {
            continue;
        }
        let part_of_speech = plain_text(meaning.get("partOfSpeech").unwrap_or(&Value::Null));
        senses.push(json!({
            "partOfSpeech": if part_of_speech.is_empty() { "definition".to_string() } else { part_of_speech },
            "definitions": definitions,
        }));
        if senses.len() >= 10 {
            break;
        }
    }
    if senses.is_empty() {
        return None;
    }

    let phonetic = text(entry, "phonetic");
    let phonetic = if !phonetic.is_empty() {
        Some(phonetic)
    } else {
        entry
            .get("phonetics")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .find_map(|item| item.get("text").and_then(Value::as_str))
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };
    let source_url = entry
        .get("sourceUrls")
        .and_then(Value::as_array)
        .and_then(|urls| urls.first())
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| {
            format!(
                "https://en.wiktionary.org/wiki/{}",
                urlencoding::encode(word)
            )
        });

    let mut result = Map::new();
    let entry_word = text(entry, "word");
    let entry_word = if entry_word.is_empty() {
        word.to_string()
    } else {
        entry_word
    };
    result.insert("word".to_string(), json!(entry_word));
    result.insert("language".to_string(), json!("en"));
    if let Some(phonetic) = phonetic {
        result.insert("phonetic".to_string(), json!(phonetic));
    }
    result.insert("senses".to_string(), Value::Array(senses));
    result.insert(
        "source".to_string(),
        json!({ "name": "Free Dictionary", "url": source_url }),
    );
    Some(Value::Object(result))
}

fn from_wiktionary(payload: &Value, word: &str, language: &str, wiki: &str) -> Option<Value> {
    let object = payload.as_object()?;
    let groups: Vec<(&String, &Vec<Value>)> = object
        .iter()
        .filter_map(|(code, value)| {
            let groups = value.as_array()?;
            let has_definitions = groups
                .iter()
                .any(|group| group.get("definitions").and_then(Value::as_array).is_some());
            has_definitions.then_some((code, groups))
        })
        .collect();
    if groups.is_empty() {
        return None;
    }

    // An entry can hold several languages (the word "gift" is English and
    // German). Prefer the group that is the language being read.
    let base = language.split('-').next().unwrap_or("en").to_lowercase();
    let chosen = groups
        .iter()
        .find(|(code, _)| code.to_lowercase() == base)
        .copied()
        .unwrap_or(groups[0]);

    let mut senses: Vec<Value> = Vec::new();
    for group in chosen.1 {
        let definitions: Vec<Value> = group
            .get("definitions")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|item| {
                let definition = plain_text(item.get("definition")?);
                if definition.is_empty() {
                    return None;
                }
                let parsed_example = item
                    .get("parsedExamples")
                    .and_then(Value::as_array)
                    .and_then(|examples| examples.first())
                    .map(|example| plain_text(example.get("example").unwrap_or(&Value::Null)))
                    .unwrap_or_default();
                let raw_example = item
                    .get("examples")
                    .and_then(Value::as_array)
                    .and_then(|examples| examples.first())
                    .map(plain_text)
                    .unwrap_or_default();
                let example = if !parsed_example.is_empty() {
                    parsed_example
                } else {
                    raw_example
                };
                let mut value = Map::new();
                value.insert("definition".to_string(), Value::String(definition));
                if !example.is_empty() {
                    value.insert("example".to_string(), Value::String(example));
                }
                Some(Value::Object(value))
            })
            .take(8)
            .collect();
        if definitions.is_empty() {
            continue;
        }
        let part_of_speech = plain_text(group.get("partOfSpeech").unwrap_or(&Value::Null));
        senses.push(json!({
            "partOfSpeech": if part_of_speech.is_empty() { "definition".to_string() } else { part_of_speech },
            "definitions": definitions,
        }));
        if senses.len() >= 10 {
            break;
        }
    }
    if senses.is_empty() {
        return None;
    }

    let mut titled = wiki.to_string();
    if let Some(first) = titled.get_mut(0..1) {
        first.make_ascii_uppercase();
    }
    Some(json!({
        "word": word,
        "language": language,
        "senses": senses,
        "source": {
            "name": format!("{titled} Wiktionary"),
            "url": format!("https://{wiki}.wiktionary.org/wiki/{}", urlencoding::encode(word)),
        },
    }))
}

/// Ask a provider for JSON.
///
/// `Ok(None)` is the provider's own "no such entry" — a 404, not an outage —
/// and must stay distinct from `Err(())`, which is the service being
/// unreachable. The reader tells the two apart: no entry is a 404 it can
/// explain, an outage is a 502 it can only apologise for.
async fn request_json(url: &str) -> Result<Option<Value>, ()> {
    let response = match http()
        .get(url)
        .header(reqwest::header::ACCEPT, "application/json")
        .header(
            reqwest::header::USER_AGENT,
            "VoiceBookReader/1.0 (dictionary lookup)",
        )
        .send()
        .await
    {
        Ok(response) => response,
        Err(_) => return Err(()),
    };
    if !response.status().is_success() {
        return Ok(None);
    }
    response.json().await.map(Some).map_err(|_| ())
}

/// Look one word up, in the language it is being read in.
pub async fn lookup_word(raw_word: &str, raw_language: &str) -> ApiResult<Value> {
    let word = clean_word(raw_word)?;
    let language = canonical_language_code(if raw_language.trim().is_empty() {
        "en"
    } else {
        raw_language
    });
    let base = language.split('-').next().unwrap_or("en").to_lowercase();
    let cache_key = format!("{}:{}", language, word.to_lowercase());

    if let Some(cached) = cache_get(&cache_key) {
        if let Some(result) = cached {
            return Ok(result);
        }
        return Err(ApiError::not_found(format!(
            "No definition was found for “{word}”."
        )));
    }

    let mut result: Option<Value> = None;
    let mut failed = false;

    if base == "en" {
        let url = format!(
            "https://api.dictionaryapi.dev/api/v2/entries/en/{}",
            urlencoding::encode(&word)
        );
        match request_json(&url).await {
            Ok(Some(entries)) => result = from_free_dictionary(&entries, &word),
            Ok(None) => {}
            Err(()) => failed = true,
        }
    }

    if result.is_none() {
        // A word read in French still has an entry on fr.wiktionary, and a rare
        // English word may only exist on en.wiktionary. Try the reader's own
        // language's dictionary first, then English.
        let wikis: Vec<String> = if base == "en" {
            vec!["en".to_string()]
        } else {
            vec![base.clone(), "en".to_string()]
        };
        for wiki in wikis {
            if !(2..=3).contains(&wiki.len()) || !wiki.chars().all(|c| c.is_ascii_lowercase()) {
                continue;
            }
            let url = format!(
                "https://{wiki}.wiktionary.org/api/rest_v1/page/definition/{}",
                urlencoding::encode(&word)
            );
            match request_json(&url).await {
                Ok(Some(payload)) => {
                    result = from_wiktionary(&payload, &word, &language, &wiki);
                    if result.is_some() {
                        break;
                    }
                }
                Ok(None) => {}
                Err(()) => failed = true,
            }
        }
    }

    match result {
        Some(entry) => {
            cache_set(cache_key, Some(entry.clone()));
            Ok(entry)
        }
        None => {
            cache_set(cache_key, None);
            if failed {
                Err(ApiError::new(
                    axum::http::StatusCode::BAD_GATEWAY,
                    "dictionary_unavailable",
                    "The dictionary is not responding right now. Please try again in a moment.",
                ))
            } else {
                Err(ApiError::not_found(format!(
                    "No definition was found for “{word}”."
                )))
            }
        }
    }
}

/* -------------------------------------------------------------------------- */
/* The reader's own word list                                                 */
/* -------------------------------------------------------------------------- */

fn parse_senses(raw: &str) -> Vec<Value> {
    let Ok(parsed) = serde_json::from_str::<Value>(raw) else {
        return Vec::new();
    };
    let Some(array) = parsed.as_array() else {
        return Vec::new();
    };
    array
        .iter()
        .filter_map(|sense| {
            let part_of_speech = sense.get("partOfSpeech")?.as_str()?;
            let definitions = sense.get("definitions")?.as_array()?;
            let definitions: Vec<Value> = definitions
                .iter()
                .filter_map(|item| {
                    let definition = item.get("definition")?.as_str()?;
                    let mut value = Map::new();
                    value.insert("definition".to_string(), json!(definition));
                    if let Some(example) = item.get("example").and_then(Value::as_str) {
                        if !example.is_empty() {
                            value.insert("example".to_string(), json!(example));
                        }
                    }
                    Some(Value::Object(value))
                })
                .collect();
            Some(json!({ "partOfSpeech": part_of_speech, "definitions": definitions }))
        })
        .collect()
}

fn to_saved_word(row: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    let senses: String = row.get("senses")?;
    Ok(json!({
        "id": row.get::<_, String>("id")?,
        "word": row.get::<_, String>("word")?,
        "language": row.get::<_, String>("language")?,
        "phonetic": row.get::<_, Option<String>>("phonetic")?,
        "senses": parse_senses(&senses),
        "sourceName": row.get::<_, Option<String>>("source_name")?,
        "sourceUrl": row.get::<_, Option<String>>("source_url")?,
        "createdAt": row.get::<_, String>("created_at")?,
    }))
}

const WORD_COLUMNS: &str =
    "id, word, language, phonetic, senses, source_name, source_url, created_at";

/// GET /api/dictionary/saved — the reader's notebook of words.
pub fn list_saved(db: &Database) -> ApiResult<Value> {
    block_db(db, move |conn| {
        let mut statement = conn.prepare(&format!(
            "SELECT {WORD_COLUMNS} FROM reader_words ORDER BY created_at DESC LIMIT {SAVED_WORDS_LIMIT}"
        ))?;
        let words = statement
            .query_map([], to_saved_word)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(json!({ "words": words }))
    })
}

fn required_trimmed(body: &Value, key: &str, min: usize, max: usize) -> ApiResult<String> {
    let raw = body
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::invalid(key, "Invalid input: expected string."))?;
    let value = raw.trim();
    let length = value.chars().count();
    if length < min {
        return Err(ApiError::invalid(
            key,
            format!("String must contain at least {min} character(s)."),
        ));
    }
    if length > max {
        return Err(ApiError::invalid(
            key,
            format!("String must contain at most {max} character(s)."),
        ));
    }
    Ok(value.to_string())
}

fn optional_trimmed(body: &Value, key: &str, max: usize) -> ApiResult<Option<String>> {
    match body.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => {
            let raw = value
                .as_str()
                .ok_or_else(|| ApiError::invalid(key, "Invalid input: expected string."))?;
            let value = raw.trim();
            if value.chars().count() > max {
                return Err(ApiError::invalid(
                    key,
                    format!("String must contain at most {max} character(s)."),
                ));
            }
            Ok((!value.is_empty()).then(|| value.to_string()))
        }
    }
}

struct SaveWordInput {
    word: String,
    language: String,
    phonetic: Option<String>,
    senses: Vec<Value>,
    source_name: Option<String>,
    source_url: Option<String>,
}

fn parse_save_word(body: &Value) -> ApiResult<SaveWordInput> {
    let word = required_trimmed(body, "word", 1, WORD_LIMIT)?;
    let language = required_trimmed(body, "language", 2, 20)?;
    if !is_valid_language_code(&language) {
        return Err(ApiError::invalid("language", "Not a language code."));
    }
    let phonetic = optional_trimmed(body, "phonetic", PHONETIC_LIMIT)?;
    let source_name = optional_trimmed(body, "sourceName", SOURCE_NAME_LIMIT)?;
    let source_url = optional_trimmed(body, "sourceUrl", SOURCE_URL_LIMIT)?;

    let raw_senses = body
        .get("senses")
        .and_then(Value::as_array)
        .ok_or_else(|| ApiError::invalid("senses", "Invalid input: expected array."))?;
    if raw_senses.is_empty() {
        return Err(ApiError::invalid(
            "senses",
            "Array must contain at least 1 element(s).",
        ));
    }
    if raw_senses.len() > SENSE_LIMIT {
        return Err(ApiError::invalid(
            "senses",
            format!("Array must contain at most {SENSE_LIMIT} element(s)."),
        ));
    }

    let mut senses = Vec::new();
    for sense in raw_senses {
        let part_of_speech = required_trimmed(sense, "partOfSpeech", 0, PART_OF_SPEECH_LIMIT)?;
        let raw_definitions = sense
            .get("definitions")
            .and_then(Value::as_array)
            .ok_or_else(|| ApiError::invalid("definitions", "Invalid input: expected array."))?;
        if raw_definitions.is_empty() {
            return Err(ApiError::invalid(
                "definitions",
                "Array must contain at least 1 element(s).",
            ));
        }
        let mut definitions = Vec::new();
        for item in raw_definitions.iter().take(DEFINITIONS_PER_SENSE) {
            let definition = required_trimmed(item, "definition", 1, DEFINITION_LIMIT)?;
            let mut value = Map::new();
            value.insert("definition".to_string(), json!(definition));
            if let Some(example) = optional_trimmed(item, "example", EXAMPLE_LIMIT)? {
                value.insert("example".to_string(), json!(example));
            }
            definitions.push(Value::Object(value));
        }
        let part_of_speech = if part_of_speech.trim().is_empty() {
            "definition".to_string()
        } else {
            part_of_speech
        };
        senses.push(json!({
            "partOfSpeech": part_of_speech,
            "definitions": definitions,
        }));
    }

    Ok(SaveWordInput {
        word,
        language,
        phonetic,
        senses,
        source_name,
        source_url,
    })
}

/// POST /api/dictionary/saved — keep a looked-up word. Saving the same word
/// twice refreshes its entry.
pub fn save_word(db: &Database, body: &Value) -> ApiResult<Value> {
    let input = parse_save_word(body)?;
    let word = clean_word(&input.word)?.to_lowercase();
    let language = canonical_language_code(&input.language);
    let senses: Vec<Value> = input
        .senses
        .into_iter()
        .filter_map(|sense| {
            let part_of_speech = sense.get("partOfSpeech")?.as_str()?.to_string();
            let definitions = sense.get("definitions")?.as_array()?;
            let definitions: Vec<Value> = definitions
                .iter()
                .filter_map(|item| {
                    let definition = item.get("definition")?.as_str()?.trim().to_string();
                    if definition.is_empty() {
                        return None;
                    }
                    let definition: String = definition.chars().take(DEFINITION_LIMIT).collect();
                    let mut value = Map::new();
                    value.insert("definition".to_string(), json!(definition));
                    if let Some(example) = item.get("example").and_then(Value::as_str) {
                        let example = example.trim();
                        if !example.is_empty() {
                            let example: String = example.chars().take(EXAMPLE_LIMIT).collect();
                            value.insert("example".to_string(), json!(example));
                        }
                    }
                    Some(Value::Object(value))
                })
                .collect();
            (!definitions.is_empty()).then(|| {
                json!({
                    "partOfSpeech": part_of_speech.chars().take(PART_OF_SPEECH_LIMIT).collect::<String>(),
                    "definitions": definitions,
                })
            })
        })
        .collect();

    if senses.is_empty() {
        return Err(ApiError::bad_request(
            "A saved word needs at least one definition.",
        ));
    }

    let senses_json = serde_json::to_string(&senses)
        .map_err(|_| ApiError::internal("That word could not be stored."))?;
    let now = repo::now_iso();

    block_db(db, move |conn| {
        let existing: Option<String> = conn
            .query_row(
                "SELECT id FROM reader_words WHERE word = ?1 AND language = ?2",
                rusqlite::params![word, language],
                |row| row.get(0),
            )
            .optional()?;

        let id = match existing {
            Some(id) => {
                conn.execute(
                    "UPDATE reader_words SET phonetic = ?1, senses = ?2, source_name = ?3, \
                     source_url = ?4, updated_at = ?5 WHERE id = ?6",
                    rusqlite::params![
                        input.phonetic,
                        senses_json,
                        input.source_name,
                        input.source_url,
                        now,
                        id,
                    ],
                )?;
                id
            }
            None => {
                let id = repo::new_id("wd_");
                conn.execute(
                    "INSERT INTO reader_words \
                     (id, word, language, phonetic, senses, source_name, source_url, \
                      created_at, updated_at) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
                    rusqlite::params![
                        id,
                        word,
                        language,
                        input.phonetic,
                        senses_json,
                        input.source_name,
                        input.source_url,
                        now,
                    ],
                )?;
                id
            }
        };

        let mut statement = conn.prepare(&format!(
            "SELECT {WORD_COLUMNS} FROM reader_words WHERE id = ?1"
        ))?;
        let mut rows = statement.query([id])?;
        match rows.next()? {
            Some(row) => Ok(to_saved_word(row)?),
            None => Err(ApiError::internal(
                "That word could not be read back after saving.",
            )),
        }
    })
}

/// DELETE /api/dictionary/saved/:id — forget a word.
pub fn delete_saved(db: &Database, id: &str) -> ApiResult<()> {
    let id = id.to_string();
    block_db(db, move |conn| {
        let deleted = conn.execute("DELETE FROM reader_words WHERE id = ?1", [&id])?;
        if deleted == 0 {
            return Err(ApiError::not_found("That word is not in your list."));
        }
        Ok(())
    })
}
