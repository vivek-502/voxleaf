//! Search inside the book, not just its table of contents.
//!
//! Port of `src/server/services/epub-search.ts`. The reader may be looking at a
//! machine translation, and the offsets a match reports have to mean something
//! on the page that will open — so the same language the reader is reading is
//! the one searched. Where a section has no translation yet, its original text
//! is searched instead and the result says so.
//!
//! Matching is done on folded text (case, and optionally diacritics) while the
//! offsets reported are into the unfolded text, because those offsets are what
//! the reader uses to scroll and to paint the hit.

use rusqlite::OptionalExtension;
use serde_json::{json, Value};
use unicode_normalization_alignments::char::is_combining_mark;
use unicode_normalization_alignments::UnicodeNormalization;

use crate::db::Database;
use crate::error::{ApiError, ApiResult};
use crate::services::library::block_db;
use crate::services::translation::{
    canonical_language_code, extract_text_blocks, translation_language_name,
};

const MAX_QUERY_LENGTH: usize = 200;
const DEFAULT_LIMIT: i64 = 80;
const MAX_LIMIT: i64 = 200;
const EXCERPT_RADIUS: usize = 60;

/// Search options, as the route reads them from the query string.
pub struct SearchOptions<'a> {
    pub query: &'a str,
    pub language: Option<&'a str>,
    pub limit: Option<i64>,
    pub case_sensitive: bool,
    pub match_diacritics: bool,
    pub whole_word: bool,
}

struct Folded {
    value: Vec<char>,
    /// For every folded character, the UTF-16 offset it came from.
    map: Vec<usize>,
}

fn fold_char(character: char, lowercase: bool, strip_marks: bool) -> String {
    let owned = character.to_string();
    let value: String = if strip_marks {
        owned
            .as_str()
            .nfd()
            .filter(|(folded, _)| !is_combining_mark(*folded))
            .map(|(folded, _)| folded)
            .collect()
    } else {
        owned
    };
    if lowercase {
        value.to_lowercase()
    } else {
        value
    }
}

/// Fold text for matching, keeping a map back to the original offsets.
fn fold(text: &str, lowercase: bool, strip_marks: bool) -> Folded {
    let mut value = Vec::new();
    let mut map = Vec::new();
    let mut offset = 0usize;
    for character in text.chars() {
        for folded in fold_char(character, lowercase, strip_marks).chars() {
            value.push(folded);
            map.push(offset);
        }
        offset += character.len_utf16();
    }
    map.push(offset);
    Folded { value, map }
}

fn is_word_character(character: char) -> bool {
    character.is_alphanumeric()
}

fn is_word_boundary(text: &[char], index: usize) -> bool {
    if index == 0 || index >= text.len() {
        return true;
    }
    !is_word_character(text[index])
}

fn find_all(haystack: &Folded, needle: &[char], whole_word: bool) -> Vec<(usize, usize)> {
    let mut found = Vec::new();
    if needle.is_empty() {
        return found;
    }

    let mut at = 0usize;
    while at + needle.len() <= haystack.value.len() {
        let Some(offset) = haystack.value[at..]
            .windows(needle.len())
            .position(|window| window == needle)
        else {
            break;
        };
        let matched_at = at + offset;
        let start = haystack.map[matched_at];
        let end = haystack.map[matched_at + needle.len()];
        let whole_word_ok = !whole_word
            || (is_word_boundary(&haystack.value, matched_at)
                && is_word_boundary(&haystack.value, matched_at + needle.len()));

        if whole_word_ok && end > start {
            found.push((start, end));
        }
        at = matched_at + needle.len().max(1);
    }
    found
}

fn flatten(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Slice a string by UTF-16 offsets, the unit the reader's offsets use.
fn utf16_slice(text: &str, start: usize, end: usize) -> String {
    let mut out = String::new();
    let mut units = 0usize;
    for character in text.chars() {
        let next = units + character.len_utf16();
        if next > start && units < end {
            out.push(character);
        }
        if units >= end {
            break;
        }
        units = next;
    }
    out
}

fn excerpt(text: &str, start: usize, end: usize) -> Value {
    let before_start = start.saturating_sub(EXCERPT_RADIUS);
    let after_end = (end + EXCERPT_RADIUS).min(text.encode_utf16().count());
    json!({
        // Only the outer edges are trimmed: the space that separates the match
        // from its neighbours belongs to the excerpt.
        "before": flatten(&utf16_slice(text, before_start, start)).trim_start().to_string(),
        "match": flatten(&utf16_slice(text, start, end)).trim().to_string(),
        "after": flatten(&utf16_slice(text, end, after_end)).trim_end().to_string(),
    })
}

/// Read a stored translation, tolerating a row written by an older shape.
fn parse_translation_blocks(value: &str) -> Vec<String> {
    let Ok(parsed) = serde_json::from_str::<Value>(value) else {
        return Vec::new();
    };
    let Some(array) = parsed.as_array() else {
        return Vec::new();
    };
    array
        .iter()
        .filter_map(|block| {
            block
                .get("text")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect()
}

struct SearchChapter {
    index: i64,
    title: String,
    html: String,
}

struct TranslationRow {
    epub_index: i64,
    blocks: String,
    language_name: String,
}

/// Every match of `query` in the book, capped so one common word cannot return
/// a result list the reader has to scroll for a minute.
pub fn search(db: &Database, book_id: &str, options: &SearchOptions<'_>) -> ApiResult<Value> {
    let query: String = options
        .query
        .trim()
        .chars()
        .take(MAX_QUERY_LENGTH)
        .collect();
    if query.is_empty() {
        return Ok(json!({
            "query": "",
            "results": [],
            "total": 0,
            "truncated": false,
            "chaptersScanned": 0,
        }));
    }

    let language = options
        .language
        .map(str::trim)
        .filter(|language| !language.is_empty())
        .map(canonical_language_code);

    let query_language = language.clone();
    let book_id = book_id.to_string();
    let (chapters, translations): (Vec<SearchChapter>, Vec<TranslationRow>) =
        block_db(db, move |conn| {
            let epub: Option<(String, String)> = conn
                .query_row(
                    "SELECT id, status FROM epub_files WHERE book_id = ?1",
                    [&book_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;
            let Some((epub_id, status)) = epub else {
                return Err(ApiError::not_found("This book has no EPUB yet."));
            };
            if status != "ready" {
                return Err(ApiError::not_found("This book has no EPUB yet."));
            }

            let mut statement = conn.prepare(
                "SELECT idx, title, html FROM epub_chapters WHERE epub_id = ?1 ORDER BY idx ASC",
            )?;
            let chapters = statement
                .query_map([&epub_id], |row| {
                    Ok(SearchChapter {
                        index: row.get(0)?,
                        title: row.get(1)?,
                        html: row.get(2)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;

            let translations = match &query_language {
                Some(language) => {
                    let mut statement = conn.prepare(
                        "SELECT epub_index, blocks, language_name FROM epub_translations \
                         WHERE book_id = ?1 AND language = ?2",
                    )?;
                    let rows = statement
                        .query_map(rusqlite::params![book_id, language], |row| {
                            Ok(TranslationRow {
                                epub_index: row.get(0)?,
                                blocks: row.get(1)?,
                                language_name: row.get(2)?,
                            })
                        })?
                        .collect::<Result<Vec<_>, _>>()?;
                    rows
                }
                None => Vec::new(),
            };

            Ok((chapters, translations))
        })?;

    let limit = options.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let lowercase = !options.case_sensitive;
    let strip_marks = !options.match_diacritics;
    let needle = fold(&query, lowercase, strip_marks);
    let needle_chars: Vec<char> = needle.value;

    let mut results: Vec<Value> = Vec::new();
    let mut total: i64 = 0;
    let mut chapters_scanned: i64 = 0;

    for chapter in &chapters {
        chapters_scanned += 1;
        let translation = translations
            .iter()
            .find(|row| row.epub_index == chapter.index);
        let translated_blocks = translation
            .map(|row| parse_translation_blocks(&row.blocks))
            .unwrap_or_default();
        let use_translation = !translated_blocks.is_empty();
        let blocks: Vec<String> = if use_translation {
            translated_blocks
        } else {
            extract_text_blocks(&chapter.html)
                .into_iter()
                .map(|block| block.text)
                .collect()
        };
        let source = if use_translation {
            "translation"
        } else {
            "original"
        };
        let language_code = if use_translation {
            language.clone()
        } else {
            None
        };
        let language_name = match (&language_code, translation) {
            (Some(code), Some(row)) => {
                let known = translation_language_name(code);
                if known == *code && !row.language_name.is_empty() {
                    row.language_name.clone()
                } else {
                    known
                }
            }
            _ => String::new(),
        };

        for (block_index, text) in blocks.iter().enumerate() {
            if text.is_empty() {
                continue;
            }
            let haystack = fold(text, lowercase, strip_marks);
            let matches = find_all(&haystack, &needle_chars, options.whole_word);
            if matches.is_empty() {
                continue;
            }

            total += matches.len() as i64;
            for (start, end) in matches {
                if results.len() as i64 >= limit {
                    continue;
                }
                results.push(json!({
                    "id": format!("{}:{}:{}:{}", chapter.index, block_index, start, source),
                    "chapterIndex": chapter.index,
                    "chapterTitle": chapter.title,
                    "blockIndex": block_index,
                    "startOffset": start,
                    "endOffset": end,
                    "excerpt": excerpt(text, start, end),
                    "source": source,
                    "language": language_code,
                    "languageName": if language_code.is_some() { Some(language_name.clone()) } else { None },
                }));
            }
        }
    }

    Ok(json!({
        "query": query,
        "results": results,
        "total": total,
        "truncated": total > results.len() as i64,
        "chaptersScanned": chapters_scanned,
    }))
}
