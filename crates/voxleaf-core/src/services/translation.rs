//! Saved translations of EPUB sections.
//!
//! A translation is expensive to make and cheap to keep, so the rule is that
//! once a section has been translated into a language it is stored and reused:
//! the reader never pays for the same paragraph twice, and the app keeps
//! working with the network unplugged.
//!
//! Rows are keyed by section and language, and hold text rather than markup.
//! The reader renders blocks as React elements, so no string that came back
//! from a translation service is ever parsed as HTML.

pub mod google;

use rusqlite::OptionalExtension;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::api::AppState;
use crate::config::{Config, TranslationConfig};
use crate::db::Database;
use crate::error::{ApiError, ApiResult};
use crate::models::{EpubTranslationEntry, EpubTranslationSummary, TranslationBlock};
use crate::repo;

/// Ceiling on one section's source text.
///
/// The provider is metered by request and takes paragraphs at a time, so this
/// is really a ceiling on how long a reader will wait: 250 000 characters is
/// around eighty requests, a minute of work, and longer than any chapter in
/// any book in this library. Refusing a section past it is worth doing up
/// front, because a run that is throttled half-way leaves the reader with
/// nothing at all, whereas this says exactly why.
pub const MAX_TRANSLATION_CHARS: usize = 250_000;

/// Fingerprint of the text a translation was made from.
///
/// Stored with each row so a section that has been edited since can be flagged
/// as stale rather than silently shown against changed prose.
pub fn source_hash(texts: &[String]) -> String {
    let joined = texts.join("\u{0}");
    let mut hasher = Sha256::new();
    hasher.update(joined.as_bytes());
    hex::encode(hasher.finalize())[..32].to_string()
}

/// `toLocaleString`'s thousands separators, for the one message that quotes a
/// character count back at the reader.
fn group_thousands(value: usize) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    let first = digits.len() % 3;
    for (position, c) in digits.chars().enumerate() {
        if position > 0 && (position % 3) == first {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/* -------------------------------------------------------------------------- */
/* Language codes                                                             */
/* -------------------------------------------------------------------------- */

/// Languages offered for translating a section, and the names the reader sees.
///
/// The provider understands well over a hundred codes; these are the ones
/// worth one click, and a code the reader types that is not on the list is
/// still accepted, named after itself.
const TRANSLATION_LANGUAGES: &[(&str, &str)] = &[
    ("es", "Spanish"),
    ("pt-BR", "Brazilian Portuguese"),
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
    ("zh-CN", "Chinese (Simplified)"),
    ("zh-TW", "Chinese (Traditional)"),
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
    ("uk-Latn", "Ukrainian (Latin)"),
    ("sr", "Serbian"),
    ("sw", "Swahili"),
    ("af", "Afrikaans"),
    ("sl", "Slovene"),
];

/// The reader's name for a language code, falling back to the code itself.
pub fn translation_language_name(code: &str) -> String {
    let key = code.trim().to_lowercase();
    if let Some((_, name)) = TRANSLATION_LANGUAGES
        .iter()
        .find(|(candidate, _)| candidate.to_lowercase() == key)
    {
        return (*name).to_string();
    }
    let trimmed = code.trim();
    if trimmed.is_empty() {
        "Unknown".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Put a language code into the form BCP 47 gives it, which is also the form
/// the provider expects.
///
/// Casing is not cosmetic here. `pt` and `pt-BR` are different languages —
/// general Portuguese and Brazilian Portuguese — and `zh-CN` and `zh-TW` name
/// different scripts, so lowercasing a code does not merely tidy it, it risks
/// asking for a language nobody wanted. Canonicalising means a language typed
/// two ways is stored once and sent the same way both times.
pub fn canonical_language_code(code: &str) -> String {
    code.trim()
        .to_lowercase()
        .split('-')
        .enumerate()
        .map(|(position, part)| {
            if position == 0 {
                return part.to_string();
            }
            let letters = part.chars().collect::<Vec<_>>();
            // A script subtag is four letters, written in title case.
            if letters.len() == 4 && letters.iter().all(|c| c.is_ascii_alphabetic()) {
                let mut titled = letters[0].to_uppercase().collect::<String>();
                titled.extend(&letters[1..]);
                return titled;
            }
            // A region subtag is two letters or three digits, written in capitals.
            if (letters.len() == 2 && letters.iter().all(|c| c.is_ascii_alphabetic()))
                || (letters.len() == 3 && letters.iter().all(|c| c.is_ascii_digit()))
            {
                return part.to_uppercase();
            }
            part.to_string()
        })
        .collect::<Vec<_>>()
        .join("-")
}

/// A language code bounded and restricted to the characters a language tag is
/// made of, because it goes straight into an outbound URL.
///
/// `^[A-Za-z]{2,3}(-[A-Za-z0-9]{2,8})*$`, two to thirty-five characters.
pub fn is_valid_language_code(code: &str) -> bool {
    if !(2..=35).contains(&code.chars().count()) {
        return false;
    }
    let mut parts = code.split('-');
    let Some(primary) = parts.next() else {
        return false;
    };
    if !(2..=3).contains(&primary.len()) || !primary.chars().all(|c| c.is_ascii_alphabetic()) {
        return false;
    }
    parts.all(|part| {
        (2..=8).contains(&part.len()) && part.chars().all(|c| c.is_ascii_alphanumeric())
    })
}

/* -------------------------------------------------------------------------- */
/* Reading a section's markup into blocks                                     */
/* -------------------------------------------------------------------------- */

/// Block tags worth keeping, in the order they can appear.
pub const TEXT_BLOCK_TAGS: &[&str] = &[
    "h1", "h2", "h3", "h4", "h5", "h6", "p", "li", "blockquote", "figcaption", "pre", "img",
    "table", "thead", "tbody", "tfoot", "tr", "th", "td",
];

/// Block-level tags that close the block being read, without opening a new one.
const CLOSING_ONLY: &[&str] = &["div", "section", "article", "ul", "ol"];

/// A block of a section: its tag, its text, and (for an image) its markup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextBlock {
    pub tag: String,
    pub text: String,
    pub html: Option<String>,
}

const NAMED_ENTITIES: &[(&str, &str)] = &[
    ("amp", "&"),
    ("lt", "<"),
    ("gt", ">"),
    ("quot", "\""),
    ("apos", "'"),
    ("nbsp", " "),
    ("ndash", "–"),
    ("mdash", "—"),
    ("lsquo", "‘"),
    ("rsquo", "’"),
    ("ldquo", "“"),
    ("rdquo", "”"),
    ("hellip", "…"),
    ("laquo", "«"),
    ("raquo", "»"),
    ("deg", "°"),
    ("middot", "·"),
    ("bull", "•"),
    ("copy", "©"),
    ("reg", "®"),
    ("trade", "™"),
    ("eacute", "é"),
    ("egrave", "è"),
    ("agrave", "à"),
    ("ccedil", "ç"),
    ("uuml", "ü"),
    ("ouml", "ö"),
    ("auml", "ä"),
    ("szlig", "ß"),
    ("ntilde", "ñ"),
];

/// Zero-width and directional marks that publishers sprinkle through prose.
fn is_invisible(c: char) -> bool {
    matches!(c,
        '\u{00AD}'
        | '\u{200B}'..='\u{200F}'
        | '\u{202A}'..='\u{202E}'
        | '\u{2060}'..='\u{2064}'
        | '\u{2066}'..='\u{2069}'
        | '\u{FEFF}')
}

fn unescape_xml(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let tail = &rest[amp..];
        match tail.find(';').filter(|semi| *semi <= 12) {
            Some(semi) => {
                let entity = &tail[1..semi];
                let replacement = if let Some(hex) = entity.strip_prefix("#x").or_else(|| entity.strip_prefix("#X")) {
                    u32::from_str_radix(hex, 16).ok().and_then(char::from_u32).map(|c| c.to_string())
                } else if let Some(dec) = entity.strip_prefix('#') {
                    dec.parse::<u32>().ok().and_then(char::from_u32).map(|c| c.to_string())
                } else {
                    entity
                        .chars()
                        .all(|c| c.is_ascii_alphabetic())
                        .then(|| {
                            NAMED_ENTITIES
                                .iter()
                                .find(|(name, _)| name.eq_ignore_ascii_case(entity))
                                .map(|(_, text)| *text)
                        })
                        .flatten()
                        .map(|text| text.to_string())
                };
                match replacement {
                    Some(text) => out.push_str(&text),
                    None => {
                        out.push('&');
                        out.push_str(entity);
                        out.push(';');
                    }
                }
                rest = &tail[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Tidy one piece of block text without touching its words.
fn tidy(raw: &str) -> String {
    let unescaped = unescape_xml(raw);
    let mut out = String::with_capacity(unescaped.len());
    for c in unescaped.chars() {
        if c == '\r' {
            continue;
        }
        if is_invisible(c) {
            continue;
        }
        if c == '\n' {
            while out.ends_with(' ') {
                out.pop();
            }
            out.push('\n');
        } else if c.is_whitespace() {
            if !out.ends_with(' ') && !out.ends_with('\n') {
                out.push(' ');
            }
        } else {
            out.push(c);
        }
    }

    // Three or more line breaks in a row become one blank line.
    let mut paragraphs = String::with_capacity(out.len());
    let mut newlines = 0;
    for c in out.chars() {
        if c == '\n' {
            newlines += 1;
            if newlines <= 2 {
                paragraphs.push('\n');
            }
        } else {
            newlines = 0;
            paragraphs.push(c);
        }
    }

    paragraphs.trim().to_string()
}

/// The name of a tag's opening `</? name`, or empty when it has none.
fn tag_name(tag: &str) -> String {
    let rest = tag.strip_prefix('/').unwrap_or(tag);
    let rest = rest.trim_start();
    let mut chars = rest.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() => {
            let mut name = first.to_ascii_lowercase().to_string();
            for c in chars {
                if !c.is_ascii_alphanumeric() {
                    break;
                }
                name.push(c.to_ascii_lowercase());
            }
            name
        }
        _ => String::new(),
    }
}

fn strip_comments(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(start) = rest.find("<!--") {
        out.push_str(&rest[..start]);
        match rest[start + 4..].find("-->") {
            Some(offset) => rest = &rest[start + 4 + offset + 3..],
            None => {
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Split a section's markup into its blocks of text.
///
/// Inline markup is dropped, so `<em>word</em>` and `word` are the same text;
/// a `<br>` inside a block becomes a line break; text that sits outside any
/// block tag is kept as a paragraph rather than dropped, because plenty of
/// books write their chapters as bare text in a `<div>`.
///
/// The input is the app's own sanitised output, so this is a scanner rather
/// than a parser.
pub fn extract_text_blocks(html: &str) -> Vec<TextBlock> {
    let markup = strip_comments(html);
    let mut blocks: Vec<TextBlock> = Vec::new();
    let mut current: Option<String> = None;
    let mut buffer = String::new();

    fn flush(blocks: &mut Vec<TextBlock>, buffer: &mut String, current: &Option<String>) {
        let text = tidy(buffer);
        buffer.clear();
        if !text.is_empty() {
            blocks.push(TextBlock {
                tag: current.clone().unwrap_or_else(|| "p".to_string()),
                text,
                html: None,
            });
        }
    }

    /// End the block in progress, keeping any stray text as a paragraph.
    fn close(blocks: &mut Vec<TextBlock>, buffer: &mut String, current: &mut Option<String>) {
        flush(blocks, buffer, current);
        *current = None;
    }

    let mut at = 0usize;
    while at < markup.len() {
        let Some(open) = markup[at..].find('<') else {
            buffer.push_str(&markup[at..]);
            break;
        };
        let open = at + open;
        if open > at {
            buffer.push_str(&markup[at..open]);
        }

        let Some(end) = markup[open..].find('>') else {
            // An unterminated tag is text, not markup.
            buffer.push_str(&markup[open..]);
            break;
        };
        let end = open + end;

        let tag = &markup[open + 1..end];
        let name = tag_name(tag);

        if tag.starts_with('!') {
            at = end + 1;
            continue;
        }

        if tag.starts_with('/') {
            if (current.as_deref() == Some(name.as_str()) && !name.is_empty())
                || CLOSING_ONLY.contains(&name.as_str())
                || TEXT_BLOCK_TAGS.contains(&name.as_str())
            {
                close(&mut blocks, &mut buffer, &mut current);
            }
            at = end + 1;
            continue;
        }

        let self_closing = tag.ends_with('/') || name == "br" || name == "img" || name == "hr";

        if name == "br" {
            if current.is_some() {
                buffer.push('\n');
            }
            at = end + 1;
            continue;
        }

        if name == "img" {
            close(&mut blocks, &mut buffer, &mut current);
            blocks.push(TextBlock {
                tag: "img".to_string(),
                text: String::new(),
                html: Some(format!("<{tag}>")),
            });
            at = end + 1;
            continue;
        }

        if TEXT_BLOCK_TAGS.contains(&name.as_str()) {
            close(&mut blocks, &mut buffer, &mut current);
            current = Some(name);
            at = end + 1;
            continue;
        }

        if CLOSING_ONLY.contains(&name.as_str()) {
            close(&mut blocks, &mut buffer, &mut current);
            at = end + 1;
            continue;
        }

        if self_closing {
            at = end + 1;
            continue;
        }

        at = end + 1;
    }

    close(&mut blocks, &mut buffer, &mut current);
    blocks
}

/// Characters of prose across a set of blocks — what a provider charges for.
pub fn blocks_length(blocks: &[TextBlock]) -> usize {
    blocks
        .iter()
        // Purely structural elements like images carry no prose to translate.
        .filter(|block| block.tag != "img")
        .map(|block| block.text.chars().count())
        .sum()
}

/// Take any markup the provider echoed back out of a translated string: the
/// text is stored as text and rendered as elements, so a stray tag would show
/// the reader angle brackets rather than a sentence.
fn strip_tags(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('<') {
        let head = &rest[..open];
        let tail = &rest[open..];
        // Only something that opens or closes like a tag is dropped, so a
        // comparison such as "1 < 2 > 0" keeps its brackets.
        let looks_like_tag = tail[1..].starts_with('/')
            || tail[1..]
                .chars()
                .next()
                .map(|c| c.is_ascii_alphabetic())
                .unwrap_or(false);
        match (looks_like_tag, tail[1..].find('>')) {
            (true, Some(offset)) => {
                out.push_str(head);
                rest = &tail[1 + offset + 1..];
            }
            _ => {
                out.push_str(&rest[..open + 1]);
                rest = &rest[open + 1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/* -------------------------------------------------------------------------- */
/* Stored rows                                                                */
/* -------------------------------------------------------------------------- */

#[derive(Debug, Clone)]
struct TranslationRow {
    epub_index: i64,
    language: String,
    language_name: String,
    title: String,
    blocks: String,
    char_count: i64,
    source_hash: String,
    updated_at: String,
}

/// Read a stored JSON block column, refusing anything that is not the shape we
/// wrote. A corrupt row is treated as no row rather than as a broken section.
fn parse_blocks(raw: &str) -> Vec<TranslationBlock> {
    let Ok(parsed) = serde_json::from_str::<Value>(raw) else {
        return Vec::new();
    };
    let Some(array) = parsed.as_array() else {
        return Vec::new();
    };
    array
        .iter()
        .filter_map(|block| {
            Some(TranslationBlock {
                tag: block.get("tag")?.as_str()?.to_string(),
                text: block.get("text")?.as_str()?.to_string(),
                html: None,
                colspan: None,
                rowspan: None,
                is_header: None,
            })
        })
        .collect()
}

fn to_summary(row: &TranslationRow, current_hash: &str) -> EpubTranslationSummary {
    EpubTranslationSummary {
        epub_index: row.epub_index,
        language: row.language.clone(),
        language_name: row.language_name.clone(),
        title: row.title.clone(),
        char_count: row.char_count,
        stale: row.source_hash != current_hash,
        updated_at: row.updated_at.clone(),
    }
}

fn to_entry(row: &TranslationRow, current_hash: &str) -> EpubTranslationEntry {
    EpubTranslationEntry {
        epub_index: row.epub_index,
        language: row.language.clone(),
        language_name: translation_language_name(&row.language),
        title: row.title.clone(),
        blocks: parse_blocks(&row.blocks),
        char_count: row.char_count,
        stale: row.source_hash != current_hash,
        updated_at: row.updated_at.clone(),
    }
}

const ROW_COLUMNS: &str =
    "epub_index, language, language_name, title, blocks, char_count, source_hash, updated_at";

fn map_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TranslationRow> {
    Ok(TranslationRow {
        epub_index: row.get(0)?,
        language: row.get(1)?,
        language_name: row.get(2)?,
        title: row.get(3)?,
        blocks: row.get(4)?,
        char_count: row.get(5)?,
        source_hash: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

/* -------------------------------------------------------------------------- */
/* Sync database bridge                                                       */
/* -------------------------------------------------------------------------- */

/// Run database work from a synchronous service method.
///
/// `Database::call` runs on the blocking pool; the runtime this app serves on
/// is multi-threaded, so parking the current worker while the pool answers is
/// safe and keeps every service method's signature synchronous, exactly as the
/// web app's were.
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

/// The current source hash of a section, or `""` when the book has no such
/// section — the same sentinel the web app used to mark every row stale.
fn current_hash(sections: &[(i64, String)], epub_index: i64) -> String {
    sections
        .iter()
        .find(|(index, _)| *index == epub_index)
        .map(|(_, html)| {
            let texts: Vec<String> = extract_text_blocks(html)
                .into_iter()
                .map(|block| block.text)
                .collect();
            source_hash(&texts)
        })
        .unwrap_or_default()
}

fn translation_api_error(error: google::TranslationError) -> ApiError {
    let status = match error.status {
        Some(code) if (400..600).contains(&code) => {
            axum::http::StatusCode::from_u16(code).unwrap_or(axum::http::StatusCode::BAD_GATEWAY)
        }
        _ => axum::http::StatusCode::BAD_GATEWAY,
    };
    ApiError::translation(error.message, status)
}

/* -------------------------------------------------------------------------- */
/* Service                                                                    */
/* -------------------------------------------------------------------------- */

/// The translation service: one HTTP client, one provider configuration.
pub struct TranslationService {
    cfg: TranslationConfig,
    client: reqwest::Client,
}

impl TranslationService {
    pub fn new(cfg: &Config) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(cfg.translation.timeout_ms))
            .build()
            .unwrap_or_else(|error| {
                tracing::error!(%error, "could not build the translation HTTP client");
                reqwest::Client::new()
            });
        Self {
            cfg: cfg.translation.clone(),
            client,
        }
    }

    /// Every saved translation for a book, as summaries — enough to label the
    /// language control and to mark sections that already have one, without
    /// sending tens of kilobytes of text the reader rarely shows.
    ///
    /// `index` narrows the list to one section; the reader asks that way when
    /// it only needs one section's labels.
    ///
    /// Rows come back with their staleness already resolved, which means
    /// reading the source text of every section that has a translation.
    pub fn list(&self, db: &Database, book_id: &str, index: Option<i64>) -> ApiResult<Value> {
        let rows = sync_db(db, {
            let book_id = book_id.to_string();
            move |conn| {
                let mut statement = conn.prepare(&format!(
                    "SELECT {ROW_COLUMNS} FROM epub_translations WHERE book_id = ?1 {} \
                     ORDER BY epub_index ASC, language ASC",
                    if index.is_some() {
                        "AND epub_index = ?2"
                    } else {
                        ""
                    }
                ))?;
                let rows = match index {
                    Some(index) => statement
                        .query_map(rusqlite::params![book_id, index], map_row)?
                        .collect::<Result<Vec<_>, _>>()?,
                    None => statement
                        .query_map([&book_id], map_row)?
                        .collect::<Result<Vec<_>, _>>()?,
                };
                Ok(rows)
            }
        })?;

        if rows.is_empty() {
            return Ok(json!({ "entries": [] }));
        }

        let sections = sync_db(db, {
            let book_id = book_id.to_string();
            move |conn| {
                let mut statement = conn.prepare(
                    "SELECT c.idx, c.html FROM epub_chapters c \
                     JOIN epub_files f ON c.epub_id = f.id WHERE f.book_id = ?1",
                )?;
                let rows = statement
                    .query_map([&book_id], |row| {
                        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            }
        })?;

        let entries: Vec<EpubTranslationSummary> = rows
            .iter()
            .map(|row| to_summary(row, &current_hash(&sections, row.epub_index)))
            .collect();

        Ok(json!({ "entries": entries }))
    }

    /// One saved translation, with its text. A 404 when that language has none
    /// for the section, which is the normal case for most sections and is not
    /// an error the reader needs reporting.
    pub fn get(
        &self,
        db: &Database,
        book_id: &str,
        index: i64,
        language: &str,
    ) -> ApiResult<Value> {
        // The compound key held the canonical form, so that is what a lookup
        // must ask for.
        let language = canonical_language_code(language);
        let row = sync_db(db, {
            let (book_id, index, language) = (book_id.to_string(), index, language);
            move |conn| {
                conn.query_row(
                    &format!(
                        "SELECT {ROW_COLUMNS} FROM epub_translations \
                         WHERE book_id = ?1 AND epub_index = ?2 AND language = ?3"
                    ),
                    rusqlite::params![book_id, index, language],
                    map_row,
                )
                .optional()
            }
        })?;

        let row = row.ok_or_else(|| ApiError::not_found("That translation does not exist."))?;

        let section: Option<String> = sync_db(db, {
            let (book_id, index) = (book_id.to_string(), index);
            move |conn| {
                conn.query_row(
                    "SELECT c.html FROM epub_chapters c \
                     JOIN epub_files f ON c.epub_id = f.id \
                     WHERE f.book_id = ?1 AND c.idx = ?2",
                    rusqlite::params![book_id, index],
                    |row| row.get::<_, String>(0),
                )
                .optional()
            }
        })?;

        let hash = current_hash(
            &[(index, section.unwrap_or_default())],
            index,
        );
        Ok(serde_json::to_value(to_entry(&row, &hash)).unwrap_or(Value::Null))
    }

    /// Translate one section into one language and store the result.
    ///
    /// Returns the saved entry. A translation already on file is returned
    /// untouched — translating is a deliberate act, and spending a provider's
    /// good will on the same words twice helps nobody. Callers that mean to
    /// refresh pass `force`.
    ///
    /// Nothing is written until every batch has succeeded, so an abandoned or
    /// refused run leaves the section exactly as it was.
    pub async fn translate(
        &self,
        st: &AppState,
        book_id: &str,
        index: i64,
        body: &Value,
    ) -> ApiResult<Value> {
        self.translate_inner(st, book_id, index, body, |_, _| {})
            .await
    }

    /// The streaming route's body: identical to `translate`, but every batch
    /// reports through `on_progress` so the reader can show a real number
    /// rather than a spinner.
    pub async fn translate_inner<P>(
        &self,
        st: &AppState,
        book_id: &str,
        index: i64,
        body: &Value,
        on_progress: P,
    ) -> ApiResult<Value>
    where
        P: Fn(i64, i64) + Send + Sync,
    {
        let raw_language = body
            .get("language")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .trim();
        if raw_language.is_empty() {
            return Err(ApiError::bad_request("Choose a language to translate into."));
        }
        if !is_valid_language_code(raw_language) {
            return Err(ApiError::invalid("language", "Not a language code."));
        }
        let code = canonical_language_code(raw_language);
        let force = body.get("force").and_then(|value| value.as_bool()).unwrap_or(false);

        let db = st.db.clone();
        let section: Option<(String, String)> = db
            .call({
                let (book_id, index) = (book_id.to_string(), index);
                move |conn| {
                    conn.query_row(
                        "SELECT c.title, c.html FROM epub_chapters c \
                         JOIN epub_files f ON c.epub_id = f.id \
                         WHERE f.book_id = ?1 AND c.idx = ?2",
                        rusqlite::params![book_id, index],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .optional()
                }
            })
            .await
            .map_err(|_| ApiError::not_found("That section does not exist."))?;
        let (title, html) = section.ok_or_else(|| ApiError::not_found("That section does not exist."))?;

        let existing = db
            .call({
                let (book_id, index, code) = (book_id.to_string(), index, code.clone());
                move |conn| {
                    conn.query_row(
                        &format!(
                            "SELECT {ROW_COLUMNS} FROM epub_translations \
                             WHERE book_id = ?1 AND epub_index = ?2 AND language = ?3"
                        ),
                        rusqlite::params![book_id, index, code],
                        map_row,
                    )
                    .optional()
                }
            })
            .await?;

        let texts: Vec<String> = extract_text_blocks(&html)
            .into_iter()
            .map(|block| block.text)
            .collect();

        if let Some(row) = existing {
            if !force {
                // A stored translation that no longer matches its section is
                // returned too, flagged stale: showing it beats pretending the
                // section has never been translated, and the reader offers to
                // translate it again.
                let hash = source_hash(&texts);
                return Ok(serde_json::to_value(to_entry(&row, &hash)).unwrap_or(Value::Null));
            }
        }

        let blocks = extract_text_blocks(&html);
        let chars = blocks_length(&blocks);
        if blocks.is_empty() {
            return Err(ApiError::bad_request(
                "This section is only a picture, so there is nothing to translate.",
            ));
        }
        if chars > MAX_TRANSLATION_CHARS {
            return Err(ApiError::bad_request(format!(
                "This section is {} characters long, which is more than the {} one translation \
                 allows. A section that size takes over a minute and is likely to be refused by \
                 the free service part-way through, so nothing would be saved. Translate a \
                 shorter section instead.",
                group_thousands(chars),
                group_thousands(MAX_TRANSLATION_CHARS),
            )));
        }

        let translated =
            google::translate_blocks(&texts, &code, &self.client, &self.cfg, |done, total| {
                on_progress(done, total)
            })
            .await
            .map_err(translation_api_error)?;

        let result: Vec<TranslationBlock> = blocks
            .iter()
            .enumerate()
            .map(|(position, block)| {
                let stripped = strip_tags(translated[position].trim());
                TranslationBlock {
                    tag: block.tag.clone(),
                    // A block can come back empty when the provider decided it
                    // had nothing to say. Keeping the source text is the
                    // honest outcome: a blank paragraph looks like a bug.
                    text: if stripped.is_empty() {
                        block.text.clone()
                    } else {
                        stripped
                    },
                    html: block.html.clone(),
                    colspan: None,
                    rowspan: None,
                    is_header: None,
                }
            })
            .collect();

        let blocks_json = serde_json::to_string(&result)
            .map_err(|_| ApiError::internal("That translation could not be stored."))?;
        let hash = source_hash(&texts);
        let language_name = translation_language_name(&code);
        let updated_at = repo::now_iso();
        let created_at = updated_at.clone();

        db.call({
            let (book_id, index, code) = (book_id.to_string(), index, code.clone());
            let (language_name, title, blocks_json, hash, created_at, updated_at) = (
                language_name.clone(),
                title.clone(),
                blocks_json,
                hash.clone(),
                created_at,
                updated_at.clone(),
            );
            move |conn| {
                conn.execute(
                    "INSERT INTO epub_translations \
                     (id, book_id, epub_index, language, language_name, blocks, title, \
                      char_count, source_hash, provider, created_at, updated_at) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12) \
                     ON CONFLICT (book_id, epub_index, language) DO UPDATE SET \
                      language_name = excluded.language_name, blocks = excluded.blocks, \
                      title = excluded.title, char_count = excluded.char_count, \
                      source_hash = excluded.source_hash, provider = excluded.provider, \
                      updated_at = excluded.updated_at",
                    rusqlite::params![
                        repo::new_id("tr_"),
                        book_id,
                        index,
                        code,
                        language_name,
                        blocks_json,
                        title,
                        chars as i64,
                        hash,
                        google::TRANSLATION_PROVIDER,
                        created_at,
                        updated_at,
                    ],
                )?;
                Ok::<(), ApiError>(())
            }
        })
        .await?;

        let entry = EpubTranslationEntry {
            epub_index: index,
            language_name: translation_language_name(&code),
            language: code,
            title,
            blocks: result,
            char_count: chars as i64,
            stale: false,
            updated_at,
        };
        Ok(serde_json::to_value(entry).unwrap_or(Value::Null))
    }

    /// Forget one language's translation of one section. The reader drops
    /// back to the original text as soon as this returns. Answers whether a
    /// translation was actually on file.
    pub fn remove(
        &self,
        db: &Database,
        book_id: &str,
        index: i64,
        language: &str,
    ) -> ApiResult<bool> {
        let language = canonical_language_code(language);
        sync_db(db, {
            let (book_id, index, language) = (book_id.to_string(), index, language);
            move |conn| {
                let changed = conn.execute(
                    "DELETE FROM epub_translations \
                     WHERE book_id = ?1 AND epub_index = ?2 AND language = ?3",
                    rusqlite::params![book_id, index, language],
                )?;
                Ok(changed > 0)
            }
        })
    }
}

/* -------------------------------------------------------------------------- */
/* Tests                                                                      */
/* -------------------------------------------------------------------------- */

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_codes_follow_bcp47() {
        assert_eq!(canonical_language_code("es"), "es");
        assert_eq!(canonical_language_code("pt-BR"), "pt-BR");
        assert_eq!(canonical_language_code("pt-br"), "pt-BR");
        assert_eq!(canonical_language_code("PT-br"), "pt-BR");
        assert_eq!(canonical_language_code("en-GB"), "en-GB");
        assert_eq!(canonical_language_code("zh-cn"), "zh-CN");
        assert_eq!(canonical_language_code("ZH-cn"), "zh-CN");
        assert_eq!(canonical_language_code("zh-tw"), "zh-TW");
        assert_eq!(canonical_language_code("sr-latn"), "sr-Latn");
        assert_eq!(canonical_language_code("es-419"), "es-419");
        assert_eq!(canonical_language_code("  fr  "), "fr");
    }

    #[test]
    fn language_codes_are_bounded_and_tag_shaped() {
        assert!(is_valid_language_code("es"));
        assert!(is_valid_language_code("pt-BR"));
        assert!(is_valid_language_code("zh-Hant-TW"));
        assert!(is_valid_language_code("sr-Latn"));

        assert!(!is_valid_language_code("e"));
        assert!(!is_valid_language_code("english"));
        assert!(!is_valid_language_code("en-"));
        assert!(!is_valid_language_code("en-a"));
        assert!(!is_valid_language_code("en-US!"));
        assert!(!is_valid_language_code("en_US"));
        assert!(!is_valid_language_code(&"x".repeat(36)));
    }

    #[test]
    fn language_names_fall_back_to_the_code() {
        assert_eq!(translation_language_name("pt-BR"), "Brazilian Portuguese");
        assert_eq!(translation_language_name("PT-br"), "Brazilian Portuguese");
        assert_eq!(translation_language_name("sw-KE"), "sw-KE");
        assert_eq!(translation_language_name("  "), "Unknown");
    }

    #[test]
    fn hashing_matches_the_web_app_fingerprint() {
        let texts = vec!["a".to_string(), "b".to_string()];
        assert_eq!(source_hash(&texts), "59b271ae1bbcb1d31d41929817f4b16f");

        let changed = vec!["a".to_string(), "c".to_string()];
        assert_ne!(source_hash(&texts), source_hash(&changed));
    }

    #[test]
    fn thousands_are_grouped_the_way_locale_string_does() {
        assert_eq!(group_thousands(0), "0");
        assert_eq!(group_thousands(999), "999");
        assert_eq!(group_thousands(1_000), "1,000");
        assert_eq!(group_thousands(12_345_678), "12,345,678");
        assert_eq!(group_thousands(MAX_TRANSLATION_CHARS), "250,000");
    }

    #[test]
    fn blocks_keep_the_shape_of_the_chapter() {
        let html = "<h2>Chapter <em>One</em></h2><p>Hello   there<br>friend.</p><div>Bare text</div>";
        let blocks = extract_text_blocks(html);
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[0].tag, "h2");
        assert_eq!(blocks[0].text, "Chapter One");
        assert_eq!(blocks[1].tag, "p");
        assert_eq!(blocks[1].text, "Hello there\nfriend.");
        assert_eq!(blocks[2].tag, "p");
        assert_eq!(blocks[2].text, "Bare text");
    }

    #[test]
    fn images_become_their_own_block_with_markup() {
        let html = "<p>before</p><img src=\"cover.jpg\" alt=\"A cover\" /><p>after</p>";
        let blocks = extract_text_blocks(html);
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[1].tag, "img");
        assert_eq!(blocks[1].text, "");
        assert_eq!(blocks[1].html.as_deref(), Some("<img src=\"cover.jpg\" alt=\"A cover\" />"));
    }

    #[test]
    fn entities_and_invisible_marks_are_tidied() {
        let blocks = extract_text_blocks("<p>Tom &amp; Jerry&#8217;s&#x2014;show\u{200B}time</p>");
        assert_eq!(blocks[0].text, "Tom & Jerry’s—showtime");

        let commented = extract_text_blocks("<!-- <p>gone</p> --><p>kept</p>");
        assert_eq!(commented.len(), 1);
        assert_eq!(commented[0].text, "kept");
    }

    #[test]
    fn character_counts_skip_pure_structure() {
        let blocks = extract_text_blocks("<p>abc</p><img src=\"x.jpg\" /><p>de</p>");
        assert_eq!(blocks_length(&blocks), 5);
    }

    #[test]
    fn stored_blocks_are_read_back_as_text() {
        let stored = r#"[{"tag":"p","text":"Hola"},{"tag":"img","text":"","html":"<img src=\"x\" />"},{"nope":1}]"#;
        let blocks = parse_blocks(stored);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].tag, "p");
        assert_eq!(blocks[0].text, "Hola");
        assert!(blocks[0].html.is_none());

        assert!(parse_blocks("not json").is_empty());
        assert!(parse_blocks("{\"tag\":\"p\"}").is_empty());
    }

    #[test]
    fn provider_echoed_markup_is_stripped_from_translations() {
        assert_eq!(strip_tags("plain text"), "plain text");
        assert_eq!(strip_tags("<b>bold</b> and <i>italic</i>"), "bold and italic");
        assert_eq!(strip_tags("1 < 2 > 0"), "1 < 2 > 0");
    }

    fn test_db() -> (tempfile::TempDir, Database) {
        let dir = tempfile::tempdir().expect("temp dir");
        let database = Database::open(&dir.path().join("vb.db")).expect("open");
        (dir, database)
    }

    async fn seed_book_and_section(database: &Database) {
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
                        "INSERT INTO epub_files (id, book_id, status, created_at, updated_at) \
                         VALUES ('ep_1', 'bk_test', 'ready', ?1, ?1)",
                        [&now],
                    )?;
                    conn.execute(
                        "INSERT INTO epub_chapters (id, epub_id, idx, href, title, html) \
                         VALUES ('ec_1', 'ep_1', 0, 'ch1.xhtml', 'One', '<p>Hello world.</p>')",
                        [],
                    )?;
                    Ok::<(), rusqlite::Error>(())
                }
            })
            .await
            .expect("seed");
    }

    async fn save_row(database: &Database, language: &str, blocks: &str, hash: &str) {
        let now = repo::now_iso();
        database
            .call({
                let (language, blocks, hash, now) = (
                    language.to_string(),
                    blocks.to_string(),
                    hash.to_string(),
                    now.clone(),
                );
                move |conn| {
                    conn.execute(
                        "INSERT INTO epub_translations \
                         (id, book_id, epub_index, language, language_name, blocks, title, \
                          char_count, source_hash, provider, created_at, updated_at) \
                         VALUES (?1, 'bk_test', 0, ?2, ?2, ?3, 'One', 11, ?4, 'google', ?5, ?5)",
                        rusqlite::params![repo::new_id("tr_"), language, blocks, hash, now],
                    )?;
                    Ok::<(), rusqlite::Error>(())
                }
            })
            .await
            .expect("insert row");
    }

    fn service() -> TranslationService {
        TranslationService::new(&Config::for_data_dir(
            std::env::temp_dir().join("vb-translation-tests"),
        ))
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn list_reports_summaries_and_staleness() {
        let (_dir, database) = test_db();
        seed_book_and_section(&database).await;

        let current = {
            let blocks = extract_text_blocks("<p>Hello world.</p>");
            let texts: Vec<String> = blocks.into_iter().map(|block| block.text).collect();
            source_hash(&texts)
        };
        save_row(&database, "es", r#"[{"tag":"p","text":"Hola mundo."}]"#, &current).await;
        save_row(&database, "fr", r#"[{"tag":"p","text":"Bonjour."}]"#, "stale-hash").await;

        let listed = service().list(&database, "bk_test", None).expect("list");
        let entries = listed["entries"].as_array().expect("entries array");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0]["language"], "es");
        assert_eq!(entries[0]["stale"], false);
        assert_eq!(entries[1]["language"], "fr");
        assert_eq!(entries[1]["stale"], true);
        assert_eq!(entries[0]["languageName"], "es");

        // `?index=` narrows the list to one section — the reader asks that
        // way when it only needs one section's labels.
        let one = service()
            .list(&database, "bk_test", Some(0))
            .expect("list one section");
        assert_eq!(one["entries"].as_array().expect("entries").len(), 2);
        let elsewhere = service()
            .list(&database, "bk_test", Some(5))
            .expect("list another section");
        assert_eq!(elsewhere["entries"].as_array().expect("entries").len(), 0);

        let missing = service().list(&database, "bk_nope", None).expect("list");
        assert_eq!(missing["entries"].as_array().expect("entries").len(), 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn get_returns_text_or_a_404() {
        let (_dir, database) = test_db();
        seed_book_and_section(&database).await;
        save_row(
            &database,
            "es",
            r#"[{"tag":"p","text":"Hola mundo."},{"tag":"img","text":"","html":"<img src=\"x\" />"}]"#,
            "whatever",
        )
        .await;

        let found = service().get(&database, "bk_test", 0, "es").expect("get");
        assert_eq!(found["language"], "es");
        assert_eq!(found["blocks"][0]["text"], "Hola mundo.");
        assert_eq!(found["stale"], true);
        assert!(found["blocks"][0].get("html").is_none());

        let error = service()
            .get(&database, "bk_test", 7, "es")
            .expect_err("no section translation");
        assert_eq!(error.message, "That translation does not exist.");

        // The lookup asks for one language: another one is simply not there,
        // even on a section that has been translated.
        let wrong_language = service()
            .get(&database, "bk_test", 0, "fr")
            .expect_err("no translation in that language");
        assert_eq!(wrong_language.message, "That translation does not exist.");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn remove_forgets_one_language_of_a_section() {
        let (_dir, database) = test_db();
        seed_book_and_section(&database).await;
        save_row(&database, "es", r#"[{"tag":"p","text":"Hola."}]"#, "h").await;
        save_row(&database, "fr", r#"[{"tag":"p","text":"Bonjour."}]"#, "h").await;

        let removed = service()
            .remove(&database, "bk_test", 0, "es")
            .expect("remove");
        assert!(removed, "the translation was on file");

        // The other language stays: forgetting is per language, so choosing a
        // new one in the menu does not silently drop the rest.
        let listed = service().list(&database, "bk_test", None).expect("list");
        let entries = listed["entries"].as_array().expect("entries");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["language"], "fr");

        // Removing what is already gone reports so, without an error.
        let again = service()
            .remove(&database, "bk_test", 0, "es")
            .expect("remove again");
        assert!(!again, "nothing was left to remove");

        let last = service()
            .remove(&database, "bk_test", 0, "fr")
            .expect("remove last");
        assert!(last);
        let empty = service().list(&database, "bk_test", None).expect("list");
        assert_eq!(empty["entries"].as_array().expect("entries").len(), 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn translate_refuses_a_bad_language_before_any_request() {
        let (_dir, database) = test_db();
        seed_book_and_section(&database).await;

        let config = Config::for_data_dir(std::env::temp_dir().join("vb-translation-tests"));
        let st = crate::api::AppState::new(
            database.clone(),
            config.clone(),
            crate::services::Services::new(&config),
        );
        let service = service();

        let empty = service
            .translate(&st, "bk_test", 0, &json!({ "language": " " }))
            .await
            .expect_err("no language");
        assert_eq!(empty.message, "Choose a language to translate into.");

        let invalid = service
            .translate(&st, "bk_test", 0, &json!({ "language": "english!" }))
            .await
            .expect_err("not a tag");
        assert_eq!(
            invalid.details,
            Some(json!([{ "path": "language", "message": "Not a language code." }]))
        );

        let missing = service
            .translate(&st, "bk_test", 9, &json!({ "language": "es" }))
            .await
            .expect_err("no section");
        assert_eq!(missing.message, "That section does not exist.");
    }
}
