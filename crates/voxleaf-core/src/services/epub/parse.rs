//! Parsing an uploaded `.epub` into what the reader needs.
//!
//! A port of the web app's `epub/parse.ts` and `epub/sanitize.ts`, which the
//! TypeScript tests in `epub/parse.test.ts` pin behaviour for. The archive is
//! a zip of arbitrary third-party markup; what comes out is the reading order,
//! a table of contents, and sanitised HTML with images rewritten to this
//! app's own route. Everything happens once, at import time, so a chapter
//! read is a database lookup instead of a zip decompression.
//!
//! Tolerances worth knowing about, because real EPUBs are messy:
//!  - Spine documents may be `.xhtml`, `.html`, or `.htm`; media types lie.
//!  - The TOC may be an NCX (`toc.ncx`, EPUB 2) or a `<nav>` element (EPUB 3),
//!    or both. We merge, preferring the nav.
//!  - Documents referenced by the spine but absent from the zip are skipped
//!    rather than aborting the whole import.
//!
//! The important subtlety is that **the spine and the table of contents are
//! not the same list**, and neither is right on its own — see `build_contents`
//! for how the two are merged.
//!
//! One porting decision runs through the whole file: **every character offset
//! is counted in UTF-16 code units**, not bytes, not `char`s. The web app is
//! JavaScript, where `str.length` and `regex.lastIndex` are UTF-16 units, and
//! the stored offsets are consumed by a browser measuring the injected HTML
//! with the same unit. Rust strings index by byte, so `utf16_len` and
//! `utf16_prefix` stand in wherever the TypeScript is doing arithmetic on
//! string lengths.

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::sync::OnceLock;

use html5ever::parse_document;
use html5ever::tendril::TendrilSink;
use markup5ever_rcdom::{Handle, NodeData, RcDom};
use regex::Regex;

/* -------------------------------------------------------------------------- */
/* Public types                                                                */
/* -------------------------------------------------------------------------- */

/// One image referenced by the sanitised chapter HTML.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedImage {
    /// Stable, short id used in the rewritten `src`. Unique per EPUB, not
    /// globally, because the sanitiser numbers images from 1 for every book.
    pub ref_id: String,
    pub mime_type: String,
    /// The source `alt`, trimmed to 300 units. `None` when it was empty —
    /// the web app's service stores `alt || null`.
    pub alt: Option<String>,
    pub bytes: Vec<u8>,
}

/// One reading-order document, already sanitised.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedChapter {
    /// Position in the reading order, 0-based, after plates have been folded in.
    pub index: usize,
    /// Source paths inside the zip, comma-joined.
    ///
    /// The TypeScript keeps `hrefs: string[]` and the web app's database
    /// layer joins with `','` before storing; since the stored shape is what
    /// survives anyway, the joined string is what this field carries.
    pub href: String,
    pub title: String,
    /// Nesting depth from the table of contents, 0 for a flat spine.
    pub depth: i64,
    /// Whether a TOC entry points here. Unlisted spine items are still readable.
    pub in_toc: bool,
    pub word_count: i64,
    pub html: String,
    /// Character offset of this document within the whole book.
    pub char_offset: i64,
}

/// One row of the book's own table of contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentsRow {
    /// Reading-order chapter the row opens.
    pub index: i64,
    /// Anchor id within the chapter, empty when the row has no fragment.
    pub anchor: String,
    /// Visible-text character offset inside the chapter.
    pub offset: i64,
    pub title: String,
    /// Nesting depth, 0-based.
    pub depth: i64,
}

/// Everything the reader needs from one uploaded book.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedEpub {
    pub title: Option<String>,
    pub creator: Option<String>,
    pub language: Option<String>,
    pub chapters: Vec<ParsedChapter>,
    pub images: Vec<ParsedImage>,
    pub contents: Vec<ContentsRow>,
    /// Total characters across all chapters, for "x% read". Carried for
    /// fidelity with the web app's `ParsedEpub`; nothing in the desktop app
    /// reads it yet.
    pub total_chars: i64,
}

/// Why a book file could not be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpubParseError {
    pub message: String,
}

impl std::fmt::Display for EpubParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for EpubParseError {}

impl EpubParseError {
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into() }
    }
}

/* -------------------------------------------------------------------------- */
/* Small JS-semantics helpers                                                  */
/* -------------------------------------------------------------------------- */

/// How many UTF-16 code units make up `text` — JavaScript's `text.length`.
fn utf16_len(text: &str) -> usize {
    text.chars().map(|c| c.len_utf16()).sum()
}

/// The first `units` UTF-16 code units of `text` — JavaScript's
/// `text.slice(0, units)`. JS would happily cut a surrogate pair in half; a
/// Rust string cannot hold half a pair, so the cut lands before it instead.
/// The divergence only shows for an astral character sitting exactly on a
/// 200- or 300-unit boundary, which no test or real book depends on.
fn utf16_prefix(text: &str, units: usize) -> &str {
    if utf16_len(text) <= units {
        return text;
    }
    let mut count = 0;
    for (at, c) in text.char_indices() {
        if count + c.len_utf16() > units {
            return &text[..at];
        }
        count += c.len_utf16();
    }
    text
}

/// `Math.round`: half rounds toward positive infinity. `f64::round` rounds
/// half away from zero instead, which differs for negative halves — the
/// spread arithmetic in `build_contents` can produce them.
fn js_round(value: f64) -> f64 {
    (value + 0.5).floor()
}

/// Compile-once accessor for every regular expression in this module.
macro_rules! cached_re {
    ($name:ident, $pattern:expr) => {
        fn $name() -> &'static Regex {
            static RE: OnceLock<Regex> = OnceLock::new();
            RE.get_or_init(|| Regex::new($pattern).expect("static regex pattern"))
        }
    };
}

/* -------------------------------------------------------------------------- */
/* Text decoding and small utilities                                           */
/* -------------------------------------------------------------------------- */

/// `TextDecoder('utf-8', { fatal: false })`: a malformed byte becomes U+FFFD
/// rather than losing an otherwise readable book.
fn decode(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

cached_re!(bad_percent, r"%");

/// `decodeURIComponent`, including its failure mode: a malformed escape is
/// kept verbatim, because a malformed escape is still a usable path or
/// fragment id.
fn decode_percent(value: &str) -> String {
    // Fast path: nothing percent-encoded, which is the common case.
    if !bad_percent().is_match(value) {
        return value.to_string();
    }
    match urlencoding::decode(value) {
        Ok(decoded) => decoded.into_owned(),
        // `decodeURIComponent` throws URIError for a bad escape and the
        // caller keeps the raw text; the `urlencoding` crate only errors on
        // invalid UTF-8, and for a stray `%` it leaves the character alone —
        // the same observable result as JS keeping the raw string.
        Err(_) => value.to_string(),
    }
}

/// ZIP magic. An EPUB must start with `PK\x03\x04`.
pub fn is_zip(bytes: &[u8]) -> bool {
    bytes.len() > 4 && bytes.starts_with(b"PK\x03\x04")
}

/// Resolve a relative href against the directory of the containing document,
/// normalising `.`/`..` segments. Path traversal out of the archive is not a
/// concern because we only ever look entries up in the unzipped map by exact
/// key, but a wrong key means a missing chapter.
pub fn resolve_path(base: &str, href: &str) -> String {
    let clean_href = href.split('#').next().unwrap_or_default();
    let clean_href = clean_href.split('?').next().unwrap_or_default();
    if clean_href.is_empty() {
        return base.to_string();
    }
    if let Some(rest) = clean_href.strip_prefix('/') {
        return rest.to_string();
    }

    let base_dir = match base.rfind('/') {
        Some(slash) => &base[..=slash],
        None => "",
    };
    let joined = format!("{base_dir}/{clean_href}");
    let mut resolved: Vec<&str> = Vec::new();
    for segment in joined.split('/') {
        if segment.is_empty() || segment == "." {
            continue;
        }
        if segment == ".." {
            resolved.pop();
            continue;
        }
        resolved.push(segment);
    }
    resolved.join("/")
}

fn dir_of(target: &str) -> String {
    match target.rfind('/') {
        Some(slash) => target[..=slash].to_string(),
        None => String::new(),
    }
}

cached_re!(decimal_entity, r"&#(\d+);");

/// Resolve the XML entities the web app's `unescapeXml` resolves, in the same
/// cascade order: `&amp;` last, so `&amp;lt;` survives as the literal text
/// `&lt;` and cannot be double-decoded.
fn unescape_xml(value: &str) -> String {
    let mut out = value
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'");
    out = decimal_entity()
        .replace_all(&out, |caps: &regex::Captures| {
            let code: u64 = caps[1].parse().unwrap_or(0);
            // `String.fromCharCode` truncates to 16 bits, so `&#999999;`
            // yields U+423F rather than an error. A lone surrogate cannot
            // live in a Rust string; it becomes U+FFFD, which is what a
            // browser would render it as anyway.
            let code = (code & 0xFFFF) as u32;
            char::from_u32(code)
                .map(|c| c.to_string())
                .unwrap_or_else(|| "\u{FFFD}".to_string())
        })
        .into_owned();
    out.replace("&amp;", "&")
}

cached_re!(tag_re, r"<[^>]+>");

/// Characters that look like they are there but are not.
///
/// Zero-width joiners and bidi marks are a typesetting tool, and a book that
/// uses them legitimately has them in its *text*. They are not legitimate in a
/// title: *12 Rules for Life* prefixes every rule with a zero-width
/// non-joiner, which shows in a contents row as a title that starts a
/// character too far right and cannot be selected with a double-click.
fn is_invisible(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{2069}'
            | '\u{FEFF}'
    )
}

cached_re!(whitespace_run, r"\s+");

/// Collapse markup debris and invisible characters into a displayable string.
fn clean_text(value: &str) -> String {
    let stripped = tag_re().replace_all(value, " ");
    let unescaped = unescape_xml(&stripped);
    let without_invisible: String = unescaped.chars().filter(|c| !is_invisible(*c)).collect();
    whitespace_run()
        .replace_all(&without_invisible, " ")
        .trim()
        .to_string()
}

cached_re!(trailing_separators, r"\s*[;,|]\s*$");
cached_re!(leading_separators, r"^\s*[;,|]\s*");

/// Tidy a Dublin Core value.
///
/// These fields routinely arrive with cataloguing debris: a trailing semicolon
/// (`Andy Weir;`), stray whitespace, or an empty tag. Displayed verbatim they
/// look like a bug in the reader rather than in the source file.
fn clean_metadata(value: Option<&str>) -> Option<String> {
    let value = value?;
    let text = clean_text(value);
    // Drop punctuation that only exists as a cataloguing separator.
    let text = trailing_separators().replace_all(&text, "");
    let text = leading_separators().replace_all(&text, "");
    if !text.is_empty() {
        Some(utf16_prefix(&text, 300).to_string())
    } else {
        None
    }
}

cached_re!(first_heading, r"(?i)<h[1-3][^>]*>([\s\S]*?)</h[1-3]>");

/// First heading in a document is a better chapter title than "Section 4".
fn title_from_document(markup: &str) -> Option<String> {
    let heading = first_heading().captures(markup)?;
    let text = clean_text(&heading[1]);
    let length = utf16_len(&text);
    if length > 0 && length <= 200 {
        Some(text)
    } else {
        None
    }
}

cached_re!(img_alt, r#"(?i)<img\b[^>]*?\salt\s*=\s*[\"']([^\"']{1,200})[\"']"#);
cached_re!(placeholder_image, r"(?i)^image(?:\s*\d+)?$");
cached_re!(placeholder_img_file, r"(?i)^img[_-]?\d+$");
cached_re!(has_whitespace, r"\s");

/// Alt text of the document's first image.
///
/// Front matter often consists of a single illustration with no heading at
/// all — *Project Hail Mary* has two such pages, and both carry a descriptive
/// `alt` that says what the plate is. Falling back to that beats "Section 5".
fn title_from_image_alt(markup: &str) -> Option<String> {
    let alt = img_alt().captures(markup)?[1].to_string();
    let text = clean_text(&alt);
    // Reject placeholder alts like "image", "img_01" or a bare filename,
    // which are less informative than a positional title.
    if text.is_empty()
        || placeholder_image().is_match(&text)
        || placeholder_img_file().is_match(&text)
    {
        return None;
    }
    if !has_whitespace().is_match(&text) {
        return None;
    }
    Some(text)
}

/* -------------------------------------------------------------------------- */
/* The HTML sanitiser (port of sanitize.ts)                                    */
/* -------------------------------------------------------------------------- */

/// Elements that carry flow/phrasing structure the reader styles.
const ALLOWED_TAGS: [&str; 68] = [
    "p", "div", "span", "br", "hr", "wbr", "h1", "h2", "h3", "h4", "h5", "h6", "em", "strong", "i",
    "b", "u", "s", "small", "sub", "sup", "blockquote", "q", "cite", "ul", "ol", "li", "dl", "dt",
    "dd", "a", "img", "figure", "figcaption", "table", "thead", "tbody", "tfoot", "tr", "th", "td",
    "caption", "colgroup", "col", "pre", "code", "kbd", "samp", "var", "section", "article", "aside",
    "header", "footer", "main", "nav", "abbr", "ruby", "rt", "rp", "mark", "time", "bdi", "bdo",
    "ins", "del", "details", "summary",
];

/// Tags we drop along with everything inside them.
const DROP_SUBTREE: [&str; 31] = [
    "script", "style", "link", "meta", "base", "head", "title", "iframe", "object", "embed",
    "applet", "form", "input", "button", "select", "textarea", "option", "label", "fieldset",
    "legend", "svg", "math", "audio", "video", "source", "track", "canvas", "noscript", "template",
    "xml", "import",
];

const VOID_TAGS: [&str; 4] = ["br", "hr", "wbr", "img"];

/// Attributes allowed per tag. Anything not listed is discarded.
fn allowed_attr(tag: &str, name: &str) -> bool {
    // The web app keeps a global set and a per-tag set, and permits a name
    // that appears in either.
    if matches!(name, "id" | "title" | "lang" | "dir") {
        return true;
    }
    match tag {
        "a" => name == "href" || name == "title",
        "img" => matches!(name, "src" | "alt" | "width" | "height"),
        "td" => name == "colspan" || name == "rowspan",
        "th" => matches!(name, "colspan" | "rowspan" | "scope"),
        "ol" => name == "start",
        "time" => name == "datetime",
        "q" | "blockquote" => name == "cite",
        "details" => name == "open",
        _ => false,
    }
}

/// Escape text the way the sanitiser emits it.
///
/// The web app's pattern is `/&(?![a-zA-Z#][a-zA-Z0-9]{0,31};)/g`: an
/// ampersand that already introduces an entity is passed through untouched,
/// so re-escaping a decoded `&amp;` reproduces the source spelling. The
/// look-ahead has no `regex`-crate equivalent, so the scan is written out.
fn escape_text_into(value: &str, out: &mut String) {
    let chars: Vec<char> = value.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '&' {
            // `&` followed by `[a-zA-Z#]`, up to 31 alphanumerics, then `;`.
            let mut entity = false;
            if i + 1 < chars.len() && (chars[i + 1].is_ascii_alphabetic() || chars[i + 1] == '#') {
                let mut k = i + 2;
                let mut alnum = 0;
                while k < chars.len() && chars[k].is_ascii_alphanumeric() && alnum < 31 {
                    k += 1;
                    alnum += 1;
                }
                entity = k < chars.len() && chars[k] == ';';
            }
            if !entity {
                out.push_str("&amp;");
            } else {
                out.push('&');
            }
            i += 1;
            continue;
        }
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
        i += 1;
    }
}

fn escape_text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    escape_text_into(value, &mut out);
    out
}

fn escape_attr(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    escape_text_into(value, &mut out);
    // The web app escapes quotes after the text escape, so `&quot;` spelled
    // in the source survives exactly once.
    out.replace('"', "&quot;")
}

cached_re!(control_chars, r"[\x00-\x1f\x7f]");
cached_re!(safe_schemes, r"(?i)^(?:https?:|mailto:|#|/|\./|\.\./)");

/// Only same-document, http(s) and mailto links survive. This rejects
/// `javascript:`, `data:` and `vbscript:` in one pass.
fn safe_href(href: &str) -> Option<String> {
    let trimmed = href.trim();
    // Strip control characters used to smuggle "java\0script:".
    let cleaned = control_chars().replace_all(trimmed, "");
    if cleaned.is_empty() {
        return None;
    }
    if cleaned.starts_with('#') {
        return Some(cleaned.into_owned());
    }
    if !safe_schemes().is_match(&cleaned) {
        return None;
    }
    // Protocol-relative URLs are fine over http(s) but not worth the ambiguity.
    if cleaned.starts_with("//") {
        return None;
    }
    Some(trimmed.to_string())
}

cached_re!(numeric_attrs, r"[^\d]");
cached_re!(start_attr, r"[^\d-]");

/// Called for every `<img src>`; the string returned is the `src` to emit, and
/// `None` drops the image and leaves its alt text in its place.
type ImageResolver<'a> = &'a mut dyn FnMut(&str, &str) -> Option<String>;

/// State threaded through the element rebuild.
struct EmitState<'a> {
    resolve_image: Option<ImageResolver<'a>>,
    budget: usize,
    dropped: usize,
    /// Visible text characters emitted so far, which is the anchor unit.
    text_length: usize,
    /// Wanted `id` -> offset, filled in as elements are emitted.
    anchors: HashMap<String, usize>,
    /// The fragment under construction.
    out: Vec<String>,
}

impl EmitState<'_> {
    fn out_push(&mut self, chunk: &str) {
        self.out.push(chunk.to_string());
    }
}

fn emit(node: &Handle, state: &mut EmitState<'_>) {
    if state.budget == 0 {
        return;
    }

    match &node.data {
        // Text. `contents` is entity-decoded by the parser; we re-escape on
        // the way out.
        NodeData::Text { contents } => {
            let text = contents.borrow();
            if text.is_empty() {
                return;
            }
            let chunk = escape_text(&text);
            // The web app's budget runs on JS string lengths; saturating is
            // equivalent because nothing reads the budget after it hits zero.
            state.budget = state.budget.saturating_sub(utf16_len(&chunk));
            state.text_length += utf16_len(&text);
            state.out_push(&chunk);
        }
        // Comments and anything else non-element: discard.
        NodeData::Element { name, attrs, .. } => {
            let tag = name.local.as_ref();
            if DROP_SUBTREE.contains(&tag) {
                state.dropped += 1;
                return;
            }
            // Unknown tags are unwrapped: their text is kept, the element is
            // not.
            if !ALLOWED_TAGS.contains(&tag) {
                state.dropped += 1;
                for child in node.children.borrow().iter() {
                    emit(child, state);
                }
                return;
            }
            emit_element(node, tag, attrs.borrow().as_slice(), state);
        }
        _ => {}
    }
}

/// Emit one allowed element: rebuild its attribute list from the allowlist,
/// then the element itself with its children.
fn emit_element(
    node: &Handle,
    tag: &str,
    source: &[html5ever::Attribute],
    state: &mut EmitState<'_>,
) {
    let attr_of = |name: &str| -> Option<String> {
        source
            .iter()
            .find(|a| a.name.local.as_ref() == name)
            .map(|a| a.value.to_string())
    };

    let mut attrs: Vec<String> = Vec::new();
    let mut discard_element = false;

    for attr in source {
        let lower = attr.name.local.as_ref();
        let value = attr.value.as_ref();

        // Hard stop on every event handler, before any allowlist
        // consultation.
        if lower.starts_with("on") {
            state.dropped += 1;
            continue;
        }
        if !allowed_attr(tag, lower) {
            state.dropped += 1;
            continue;
        }

        if lower == "href" && tag == "a" {
            let Some(safe) = safe_href(value) else {
                state.dropped += 1;
                continue;
            };
            attrs.push(format!("href=\"{}\"", escape_attr(&safe)));
            continue;
        }

        if lower == "src" && tag == "img" {
            let alt = attr_of("alt").unwrap_or_default();
            let resolved = match state.resolve_image.as_deref_mut() {
                Some(resolve) => resolve(value, &alt),
                None => None,
            };
            let Some(resolved) = resolved else {
                // No resolver, a rejected type, or a missing file. Emit the
                // alt text in its place so the reference is not silently
                // lost, then drop the <img> itself — an element with no src
                // renders as a broken icon.
                state.dropped += 1;
                discard_element = true;
                if !alt.is_empty() {
                    state.out_push(&format!("<p>{}</p>", escape_text(&alt)));
                }
                continue;
            };
            attrs.push(format!("src=\"{}\"", escape_attr(&resolved)));
            // The web app re-emits the alt here when the source had no alt
            // attribute — but `alt` above is read from that very attribute,
            // so the branch can never fire. Kept as written.
            if attr_of("alt").is_none() && !alt.is_empty() {
                attrs.push(format!("alt=\"{}\"", escape_attr(&alt)));
            }
            continue;
        }

        if matches!(lower, "width" | "height" | "colspan" | "rowspan") {
            let numeric: String = numeric_attrs().replace_all(value, "").into_owned();
            if !numeric.is_empty() {
                attrs.push(format!("{lower}=\"{numeric}\""));
            }
            continue;
        }

        if lower == "open" {
            attrs.push("open".to_string());
            continue;
        }

        if lower == "start" {
            let numeric: String = start_attr().replace_all(value, "").into_owned();
            if !numeric.is_empty() {
                attrs.push(format!("start=\"{numeric}\""));
            }
            continue;
        }

        if value.is_empty() {
            continue;
        }
        attrs.push(format!("{lower}=\"{}\"", escape_attr(value)));
    }

    if discard_element {
        return;
    }

    // An anchor's offset is where the element *starts*, so it is recorded
    // before any of its text is emitted. The first occurrence wins, which
    // matches how a browser resolves a duplicate id. Note the web app records
    // *every* `id`, not just the ones the TOC asked for — the "wanted anchors"
    // set it passes in is only a truthy flag.
    if let Some(id) = attr_of("id") {
        if !id.is_empty() && !state.anchors.contains_key(&id) {
            let at = state.text_length;
            state.anchors.insert(id, at);
        }
    }

    let open = if attrs.is_empty() {
        format!("<{tag}>")
    } else {
        format!("<{tag} {}>", attrs.join(" "))
    };
    state.out_push(&open);

    if VOID_TAGS.contains(&tag) {
        return;
    }
    for child in node.children.borrow().iter() {
        emit(child, state);
    }
    state.out_push(&format!("</{tag}>"));
}

/// What the sanitiser reports about one document.
pub(crate) struct Sanitized {
    pub html: String,
    /// How many nodes the rebuild refused. The importer reads it to decide
    /// whether a document survived at all — kept even though nothing in the
    /// parser itself consults it.
    #[allow(dead_code)]
    pub dropped: usize,
    pub text_length: usize,
    pub anchors: HashMap<String, usize>,
}

/// Default cap on emitted HTML, to bound a single chapter's payload.
const MAX_LENGTH: usize = 1_500_000;

/// Sanitise one EPUB document down to a safe HTML fragment.
///
/// The strategy is an allowlist *rebuild* rather than a blocklist filter: the
/// source tree is walked and only elements and attributes we explicitly
/// recognise are emitted. Anything unrecognised is dropped, so an attacker
/// cannot smuggle a tag through by naming it something new.
pub(crate) fn sanitize_epub_document(
    markup: &str,
    resolve_image: Option<ImageResolver<'_>>,
) -> Sanitized {
    // html5ever is a spec-compliant WHATWG parser where node-html-parser was
    // a lenient scanner; for the well-formed XHTML real EPUBs contain the two
    // agree on the text nodes, which is what both the emitted HTML and the
    // character offsets are built from. Two known divergences, neither
    // observed to matter: `<pre>` content is parsed as markup here where the
    // web app treated it as raw text (same visible text either way), and
    // attribute/tag names are lower-cased by the parser (the web app
    // lower-cased them in the emitter instead).
    let dom: RcDom = parse_document(RcDom::default(), Default::default()).one(markup);

    let mut state = EmitState {
        resolve_image,
        budget: MAX_LENGTH,
        dropped: 0,
        text_length: 0,
        anchors: HashMap::new(),
        out: Vec::new(),
    };

    // Start from `<body>` when present so we do not emit the document head.
    let body = find_element(&dom.document, "body").unwrap_or_else(|| dom.document.clone());
    for child in body.children.borrow().iter() {
        emit(child, &mut state);
    }

    Sanitized {
        html: state.out.concat(),
        dropped: state.dropped,
        text_length: state.text_length,
        anchors: state.anchors,
    }
}

/// First depth-first element with this tag name, in document order — what
/// `querySelector('body')` did in the web app.
fn find_element(node: &Handle, tag: &str) -> Option<Handle> {
    if let NodeData::Element { name, .. } = &node.data {
        if name.local.as_ref() == tag {
            return Some(node.clone());
        }
    }
    for child in node.children.borrow().iter() {
        if let Some(found) = find_element(child, tag) {
            return Some(found);
        }
    }
    None
}

/* -------------------------------------------------------------------------- */
/* Visible text of a sanitised fragment (port of sanitize.ts's helpers)        */
/* -------------------------------------------------------------------------- */

cached_re!(nbsp_entity, r"(?i)&nbsp;");
cached_re!(amp_entity, r"(?i)&amp;");
cached_re!(lt_entity, r"(?i)&lt;");
cached_re!(gt_entity, r"(?i)&gt;");
cached_re!(quot_entity, r"(?i)&quot;");
cached_re!(apos_entity, r"(?i)&#39;|&apos;");
cached_re!(entity_word, r"(?i)&[a-z]+;|&#\d+;");

/// The visible text of a sanitised fragment.
///
/// The tags are gone and the entities are decoded, so this is what a reader
/// sees and what a character offset into it means.
pub(crate) fn plain_text(html: &str) -> String {
    let text = tag_re().replace_all(html, " ");
    let text = nbsp_entity().replace_all(&text, " ");
    let text = amp_entity().replace_all(&text, "&");
    let text = lt_entity().replace_all(&text, "<");
    let text = gt_entity().replace_all(&text, ">");
    let text = quot_entity().replace_all(&text, "\"");
    let text = apos_entity().replace_all(&text, "'");
    let text = whitespace_run().replace_all(&text, " ");
    text.trim().to_string()
}

/// The first words of a fragment's visible text, for comparing two documents.
///
/// Used to recognise the running-head artefact some converters emit: a short
/// document whose whole text is the running head that the next document — the
/// chapter body — opens with.
pub(crate) fn head_text(html: &str) -> String {
    utf16_prefix(&plain_text(html), 200).to_string()
}

/// Rough word count, used for per-chapter progress weighting.
pub(crate) fn count_words(html: &str) -> usize {
    let text = tag_re().replace_all(html, " ");
    let text = entity_word().replace_all(&text, " ");
    let text = whitespace_run().replace_all(&text, " ");
    let text = text.trim();
    if text.is_empty() {
        return 0;
    }
    text.split(' ').count()
}

/* -------------------------------------------------------------------------- */
/* Table of contents: NCX and EPUB 3 nav (regex scans, as in parse.ts)         */
/* -------------------------------------------------------------------------- */

cached_re!(first_match_re, r#"(?i)<rootfile[^>]*\sfull-path=["']([^"']+)["']"#);
cached_re!(dc_title, r"(?i)<dc:title[^>]*>([\s\S]*?)</dc:title>");
cached_re!(dc_creator, r"(?i)<dc:creator[^>]*>([\s\S]*?)</dc:creator>");
cached_re!(dc_language, r"(?i)<dc:language[^>]*>([\s\S]*?)</dc:language>");

fn first_match(markup: &str, pattern: &Regex) -> Option<String> {
    let captures = pattern.captures(markup)?;
    Some(unescape_xml(captures[1].trim()))
}

/// Split an href into its path and its percent-decoded fragment.
fn split_fragment(href: &str) -> (String, String) {
    match href.find('#') {
        None => (href.to_string(), String::new()),
        Some(hash) => {
            let raw = &href[hash + 1..];
            (href[..hash].to_string(), decode_percent(raw))
        }
    }
}

/// One row of the book's own table of contents, before it is matched against
/// the spine.
#[derive(Debug, Clone)]
pub(crate) struct TocEntry {
    /// Archive path of the target document, already resolved against its base.
    pub href: String,
    /// Fragment within that document, empty when the entry names the whole file.
    pub fragment: String,
    pub label: String,
    pub depth: i64,
}

cached_re!(nav_map, r"(?i)<navMap[^>]*>([\s\S]*?)</navMap>");
cached_re!(nav_point_tag, r"(?i)<(/?)navPoint\b[^>]*>");
cached_re!(nav_label, r"(?i)<navLabel[\s\S]*?<text[^>]*>([\s\S]*?)</text>");
cached_re!(ncx_content, r#"(?i)<content[^>]*\ssrc=[\"']([^\"']+)[\"']"#);

/// Parse an EPUB 2 NCX. Nested `<navPoint>` elements carry their own depth,
/// which is what produces an indented contents panel.
fn parse_ncx(markup: &str, opf_path: &str) -> Vec<TocEntry> {
    let mut entries = Vec::new();
    let Some(nav_map_caps) = nav_map().captures(markup) else {
        return entries;
    };
    let nav_map = nav_map_caps.get(1).expect("capture 1").as_str();

    // NavPoints nest, so a single `<navPoint ...>...</navPoint>` regex cannot
    // describe the tree: the non-greedy body ends at the *child's* close tag,
    // which consumes a close the depth counter never sees, so the depth
    // climbs forever. Scan the tags as a token stream instead — every open
    // is one level deeper, every close is one level back.
    let tags: Vec<(bool, usize, usize)> = nav_point_tag()
        .captures_iter(nav_map)
        .map(|caps| {
            let m = caps.get(0).expect("capture 0 exists");
            (&caps[1] == "/", m.start(), m.end())
        })
        .collect();

    let mut depth: i64 = 0;
    for (i, &(closing, _start, end)) in tags.iter().enumerate() {
        if closing {
            depth = (depth - 1).max(0);
            continue;
        }

        // A navPoint's own `<navLabel>` and `<content>` always precede its
        // first child, so everything up to the next navPoint tag is this
        // point's markup.
        let body_end = tags.get(i + 1).map(|t| t.1).unwrap_or(nav_map.len());
        let body = &nav_map[end..body_end];

        let label = first_match(body, nav_label());
        let src = first_match(body, ncx_content());
        if let (Some(label), Some(src)) = (label, src) {
            let text = clean_text(&label);
            // A navPoint with no text of its own names nothing, and a row
            // that says nothing is worse than no row. The TS `continue` skips
            // the depth increment as well, so the children of an unlabelled
            // point sit at its own depth — reproduced here on purpose.
            if text.is_empty() {
                continue;
            }
            let (path, fragment) = split_fragment(&src);
            entries.push(TocEntry {
                href: resolve_path(opf_path, &path),
                fragment,
                label: text,
                depth,
            });
        }

        depth += 1;
    }

    entries
}

cached_re!(nav_element, r"(?i)<nav\b([^>]*)>([\s\S]*?)</nav>");
cached_re!(epub_toc_type, r#"(?i)epub:type\s*=\s*[\"'][^\"']*\btoc\b"#);
cached_re!(role_doc_toc, r#"(?i)role\s*=\s*[\"']doc-toc[\"']"#);
cached_re!(list_token, r"(?i)<(/?)(?:ol|ul|li|a|span)\b([^>]*)>");
cached_re!(
    leading_link,
    r#"(?i)^<a\b[^>]*\shref\s*=\s*[\"']([^\"']+)[\"'][^>]*>(?:[\s\S]*?)</a>"#
);

/// Parse an EPUB 3 `<nav epub:type="toc">` list.
fn parse_nav_document(markup: &str, nav_href: &str) -> Vec<TocEntry> {
    let mut entries = Vec::new();

    for nav_caps in nav_element().captures_iter(markup) {
        let attrs = &nav_caps[1];
        let is_toc = epub_toc_type().is_match(attrs) || role_doc_toc().is_match(attrs);
        if !is_toc {
            continue;
        }

        // Depth is the nesting of ancestor lists. Pulling the list out with
        // an `<ol>…</ol>` regex does not work: it is non-greedy, so it stops
        // at the first *nested* `</ol>` and silently truncates everything
        // after the first sub-list. Scan the tags and keep a stack instead.
        let body = nav_caps.get(2).expect("capture 2").as_str();

        let mut depth: i64 = 0;
        let mut pos = 0;
        while let Some(token) = list_token().captures(&body[pos..]) {
            let m = token.get(0).expect("capture 0");
            let closing = &token[1] == "/";
            let tag = token.get(2).expect("capture 2").as_str().to_ascii_lowercase();
            let match_end = pos + m.end();

            if tag == "ol" || tag == "ul" {
                depth = if closing { (depth - 1).max(0) } else { depth + 1 };
                pos = match_end;
                continue;
            }
            if closing || tag != "li" {
                pos = match_end;
                continue;
            }

            // The label is the first link in the item. A heading with no
            // target uses a <span>, which carries a label but nothing to
            // jump to.
            let rest = &body[match_end..];
            let Some(link) = leading_link().captures(rest) else {
                pos = match_end;
                continue;
            };
            let link_match = link.get(0).expect("capture 0");
            // When a link was consumed the token scan resumes past it, so
            // tokens inside its label are not re-read.
            pos = match_end + link_match.end();

            let label = clean_text(link.get(2).expect("capture 2").as_str());
            if label.is_empty() {
                continue;
            }
            let (path, fragment) = split_fragment(link.get(1).expect("capture 1").as_str());
            entries.push(TocEntry {
                href: resolve_path(nav_href, &path),
                fragment,
                label,
                depth,
            });
        }
        break;
    }

    entries
}

/* -------------------------------------------------------------------------- */
/* Contents building (buildContents)                                          */
/* -------------------------------------------------------------------------- */

/// Under this many visible characters a document cannot be a page of a book.
const EMPTY_DOC_CHARS: usize = 400;
/// A duplicated running head is never a quarter of the chapter body it precedes.
const RUNNING_HEAD_SHARE: f64 = 0.25;
/// Indentation stops growing here; the contents panel clamps it as well.
const MAX_CONTENTS_DEPTH: i64 = 6;

cached_re!(
    contentless_label,
    r"(?i)^(?:unnamed|untitled|no title|chapter|section|\.{3}|…|—|-)+$"
);
cached_re!(non_letter, r"[^\p{L}\p{N}]+");

/// Case- and punctuation-insensitive, for comparing two strings loosely.
fn loose(value: &str) -> String {
    let lower = value.to_lowercase();
    let removed = non_letter().replace_all(&lower, "");
    utf16_prefix(&removed, 80).to_string()
}

/// A contents row plus what the parser knows about where it came from.
#[derive(Debug, Clone)]
struct Placement {
    entry: TocEntry,
    /// Position in the original contents, which breaks ties deterministically.
    order: usize,
    doc: usize,
    anchor: String,
    offset: usize,
}

/// A contents row as `build_contents` emits it — `ContentsRow` plus the
/// `listed` flag the reader's rows drop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlacedRow {
    pub index: usize,
    pub anchor: String,
    pub offset: usize,
    pub title: String,
    pub depth: i64,
    /// False for the row invented for a document the book's contents skips.
    pub listed: bool,
}

pub(crate) struct ContentsOutput {
    /// Documents folded into each surviving document, in reading order.
    pub groups: Vec<Vec<usize>>,
    pub entries: Vec<PlacedRow>,
}

/// Looks a contents label up in one document and returns where its text
/// starts, in UTF-16 units, or `None` when the document does not hold it.
type HeadingLocator<'a> = Box<dyn Fn(usize, &str) -> Option<usize> + 'a>;

pub(crate) struct ContentsInput<'a> {
    /// Visible text length of each reading-order document, in UTF-16 units.
    pub text_lengths: &'a [usize],
    /// First words of each document's visible text, for the running-head test.
    pub heads: &'a [String],
    /// Text offset of each `id` a contents row may point at, per document.
    pub anchors: &'a [HashMap<String, usize>],
    /// Archive path of each reading-order document.
    pub hrefs: &'a [String],
    /// The book's own table of contents, in reading order.
    pub toc: &'a [TocEntry],
    /// Title for a document the table of contents never mentions.
    pub fallback_title: Box<dyn Fn(usize) -> String + 'a>,
    /// Where a heading appears in a document's visible text, or `None` if
    /// the caller has no such index. See `locate_heading`.
    pub locate_heading: Option<HeadingLocator<'a>>,
}

/// Merge the spine and the book's table of contents into one navigable list.
///
/// Pure, because this is where a book can quietly lose its chapters and there
/// is no way to unit-test that through a zip.
///
/// In order:
///
///  1. **Resolve.** Each contents row is attached to the spine document its
///     href names. A document that is not in the reading order at all — a
///     cover or a licence page — falls back to the nearest document in the
///     same folder.
///  2. **Fold.** Documents that cannot be a page on their own become the
///     opening of whatever follows them, carrying their contents rows with
///     them. This is what turns a duplicated running head plus the chapter
///     it precedes into one chapter named by the rule it contains, instead of
///     two rows where one said "Section 6".
///  3. **Repair.** A row whose fragment does not exist in the document it
///     names is the signature of a contents written against the file a
///     chapter was split out of. Such rows are placed on the text that
///     follows, spread across it in proportion to length so they land in
///     reading order.
///  4. **List.** Every surviving document gets its own rows, in the order the
///     rows land in the text. A document the contents never mentions still
///     gets one row, so no text in the book becomes unreachable.
pub(crate) fn build_contents(input: &ContentsInput<'_>) -> ContentsOutput {
    let count = input.text_lengths.len();

    let mut by_href: HashMap<&str, usize> = HashMap::new();
    for (index, href) in input.hrefs.iter().enumerate() {
        // First occurrence wins, as in the web app's Map.
        by_href.entry(href.as_str()).or_insert(index);
    }

    // A document too small to be a page, and so part of the one after it.
    //
    // Two shapes qualify. An empty plate is obvious. The other is the
    // artefact a converter leaves when it splits a page in two: a short
    // document holding nothing but the running head, immediately before the
    // chapter that repeats it. Comparing that text is what keeps a genuinely
    // brief chapter — a poem, an author's note — from being swallowed by the
    // long one after it.
    let is_plate = |index: usize| -> bool {
        if index + 1 >= count {
            return false;
        }
        let length = input.text_lengths.get(index).copied().unwrap_or(0);
        if length < EMPTY_DOC_CHARS {
            return true;
        }
        let head = input.heads.get(index).map(String::as_str).unwrap_or("");
        let next = input.text_lengths.get(index + 1).copied().unwrap_or(0);
        if utf16_len(head.trim()) < 8 || next == 0 {
            return false;
        }
        if length as f64 >= next as f64 * RUNNING_HEAD_SHARE {
            return false;
        }
        loose(head).starts_with(&loose(
            input.heads.get(index + 1).map(String::as_str).unwrap_or(""),
        ))
    };

    // 1. Resolve.
    let mut placements: Vec<Placement> = Vec::with_capacity(input.toc.len());
    for (order, entry) in input.toc.iter().enumerate() {
        let dir = dir_of(&entry.href);
        let target = match by_href.get(entry.href.as_str()) {
            Some(&exact) => exact,
            None => input
                .hrefs
                .iter()
                .position(|href| dir_of(href) == dir)
                .unwrap_or(0),
        };
        // Only trust a fragment that survived sanitising and actually exists
        // here.
        let offset = if entry.fragment.is_empty() {
            None
        } else {
            input
                .anchors
                .get(target)
                .and_then(|map| map.get(&entry.fragment))
                .copied()
        };
        // A converter that had no name to give an entry writes "Unnamed",
        // which says less than the heading on the page does. Several of *What
        // Every BODY Is Saying*'s rows are like this. (The web app's `??`
        // here can never fall through — its fallback title is never null.)
        let label = if contentless_label().is_match(&entry.label) {
            (input.fallback_title)(target)
        } else {
            entry.label.clone()
        };
        placements.push(Placement {
            entry: TocEntry { label, ..entry.clone() },
            order,
            doc: target,
            anchor: offset.map(|_| entry.fragment.clone()).unwrap_or_default(),
            offset: offset.unwrap_or(0),
        });
    }

    // 2. Fold plates forward. A plate joins the group being built only while
    //    that group is still all plates: once it holds a real document, the
    //    next plate belongs to the document *after* it. And a real document
    //    joins the run of plates in front of it, which is what turns a
    //    duplicated running head plus the chapter it precedes into one
    //    chapter.
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut position_of: HashMap<usize, usize> = HashMap::new();
    let mut pending: Vec<usize> = Vec::new();
    let mut anchored = false;

    for index in 0..count {
        if anchored && !pending.is_empty() {
            let position = groups.len();
            for &doc in &pending {
                position_of.insert(doc, position);
            }
            groups.push(std::mem::take(&mut pending));
            anchored = false;
        }
        pending.push(index);
        if !is_plate(index) {
            anchored = true;
        }
    }
    if !pending.is_empty() {
        let position = groups.len();
        for &doc in &pending {
            position_of.insert(doc, position);
        }
        groups.push(pending);
    }

    // 3. Repair the rows that land on the same place as another row —
    //    because their fragment no longer exists, or because several rows
    //    name one document with nothing to tell them apart. That is the
    //    shape of a book whose single HTML file was split up: the contents
    //    still points at the original. Such a run is spread across the text
    //    that follows, in proportion to its length, which keeps the rows in
    //    reading order.
    let mut collision_keys: Vec<(usize, usize)> = Vec::new();
    let mut collisions: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
    for (i, item) in placements.iter().enumerate() {
        // A resolved fragment already places a row exactly; leave those alone.
        if !item.anchor.is_empty() {
            continue;
        }
        let key = (item.doc, item.offset);
        collisions
            .entry(key)
            .or_insert_with(|| {
                collision_keys.push(key);
                Vec::new()
            })
            .push(i);
    }

    let mut batch_order: Vec<usize> = Vec::new();
    let mut batches: HashMap<usize, Vec<usize>> = HashMap::new();
    for key in &collision_keys {
        let list = &collisions[key];
        // Two rows on one document is ordinary — a part heading and the
        // chapter that opens it. It takes more than that before the pattern
        // can only mean the contents was written against a file that no
        // longer exists.
        if list.len() < 3 {
            continue;
        }
        let start = placements[list[0]].doc;
        batches
            .entry(start)
            .or_insert_with(|| {
                batch_order.push(start);
                Vec::new()
            })
            .extend(list.iter().copied());
    }

    for &start in &batch_order {
        let batch = batches[&start].clone();

        // Nothing to move the rows onto — a book of nothing but plates?
        // Leave them where the contents put them.
        let candidates: Vec<usize> = (start..count).filter(|&index| !is_plate(index)).collect();
        if candidates.is_empty() {
            continue;
        }

        // Rows should land in text a reader would actually be in, so
        // documents that are barely a paragraph are not treated as somewhere
        // a chapter begins.
        let largest = candidates
            .iter()
            .map(|&index| input.text_lengths.get(index).copied().unwrap_or(0))
            .max()
            .unwrap_or(0);
        let hosts: Vec<usize> = candidates
            .iter()
            .copied()
            .filter(|&index| {
                input.text_lengths.get(index).copied().unwrap_or(0) as f64 >= largest as f64 * 0.1
            })
            .collect();
        let targets = if hosts.is_empty() { candidates } else { hosts };

        let mut cumulative: Vec<usize> = Vec::with_capacity(targets.len());
        let mut running = 0usize;
        for &host in &targets {
            running += input.text_lengths.get(host).copied().unwrap_or(0).max(1);
            cumulative.push(running);
        }

        let mut host = 0usize;
        for (position, &item_index) in batch.iter().enumerate() {
            let point = ((position as f64 + 0.5) / batch.len() as f64) * running as f64;
            while host + 1 < targets.len() && point > cumulative[host] as f64 {
                host += 1;
            }
            let subtract = if host == 0 { 0 } else { cumulative[host - 1] };
            let item = &mut placements[item_index];
            item.doc = targets[host];
            item.offset = js_round(point - subtract as f64).max(0.0) as usize;
        }

        // The spread above only knows how long each document is. The book
        // knows better: it still has the heading this row is named after.
        // Prefer that, taking rows in contents order so each lands after the
        // last.
        let Some(locate) = input.locate_heading.as_ref() else {
            continue;
        };
        let locate = locate.as_ref();

        // Except when the book prints its own contents page, which holds
        // most of the headings at once and would swallow the whole run. Count
        // how many of the batch's headings each document holds and skip the
        // ones holding more than half of them.
        let found: Vec<usize> = targets
            .iter()
            .map(|&doc| {
                batch
                    .iter()
                    .filter(|&&idx| locate(doc, &placements[idx].entry.label).is_some())
                    .count()
            })
            .collect();
        let searchable: Vec<usize> = targets
            .iter()
            .enumerate()
            .filter(|&(position, _)| found[position] * 2 <= batch.len())
            .map(|(_, &doc)| doc)
            .collect();
        if searchable.is_empty() {
            continue;
        }

        let mut after: Option<usize> = None;
        let mut after_offset = 0usize;
        for &item_index in &batch {
            for &doc in &searchable {
                if doc < after.unwrap_or(0) {
                    continue;
                }
                let Some(at) = locate(doc, &placements[item_index].entry.label) else {
                    continue;
                };
                if after == Some(doc) && at <= after_offset {
                    continue;
                }
                let item = &mut placements[item_index];
                item.doc = doc;
                item.offset = at;
                after = Some(doc);
                after_offset = at;
                break;
            }
        }
    }

    // 4. List. A row's offset is relative to its own document, so folding
    //    plates in front of it shifts it by the text they contribute.
    let mut shift_of: HashMap<usize, usize> = HashMap::new();
    for group in &groups {
        let mut shift = 0;
        for &doc in group {
            shift_of.insert(doc, shift);
            shift += input.text_lengths.get(doc).copied().unwrap_or(0);
        }
    }

    let mut by_group: HashMap<usize, Vec<usize>> = HashMap::new();
    for (index, item) in placements.iter_mut().enumerate() {
        let Some(&position) = position_of.get(&item.doc) else {
            continue;
        };
        let shift = shift_of.get(&item.doc).copied().unwrap_or(0);
        item.offset += shift;
        by_group.entry(position).or_default().push(index);
    }

    // NCX starts its roots at depth 0 and EPUB 3 nav at depth 1; rebase so
    // the top level of any book is 0 and its children are 1. The fold starts
    // at 0, so a contents of only negative depths would rebase from 0 — as
    // the JS `reduce(…, 0)` does.
    let base = input
        .toc
        .iter()
        .fold(0i64, |min, item| min.min(item.depth));

    let mut entries: Vec<PlacedRow> = Vec::new();
    let mut previous_depth: i64 = 0;

    for (position, group) in groups.iter().enumerate() {
        let mut mine = by_group.get(&position).cloned().unwrap_or_default();
        // Stable sort, as JS `Array#sort` is specified to be.
        mine.sort_by(|&a, &b| {
            placements[a]
                .offset
                .cmp(&placements[b].offset)
                .then(placements[a].order.cmp(&placements[b].order))
        });

        if mine.is_empty() {
            // Unlisted text still gets a row, at the depth of whatever it
            // follows.
            let last = *group.last().expect("groups are never empty");
            entries.push(PlacedRow {
                index: position,
                anchor: String::new(),
                offset: 0,
                title: (input.fallback_title)(last),
                depth: previous_depth,
                listed: false,
            });
            continue;
        }

        for &item_index in &mine {
            let item = &placements[item_index];
            let depth = (item.entry.depth - base).clamp(0, MAX_CONTENTS_DEPTH);
            previous_depth = depth;
            entries.push(PlacedRow {
                index: position,
                anchor: item.anchor.clone(),
                offset: item.offset,
                title: item.entry.label.clone(),
                depth,
                listed: true,
            });
        }
    }

    ContentsOutput { groups, entries }
}

/* -------------------------------------------------------------------------- */
/* Heading location (locateHeading)                                            */
/* -------------------------------------------------------------------------- */

cached_re!(word_run, r"[\p{L}\p{N}]+");

/// A document's words, lowercased and joined so a heading can be found in it.
struct WordIndex {
    /// Offset in the visible text of each word, in UTF-16 units, in order.
    starts: Vec<usize>,
    /// Every word lowercased, joined by single spaces.
    joined: String,
}

/// Index a document's words so a heading can be looked up in it.
///
/// Matching happens on the joined form so that punctuation and capitalisation
/// stop mattering — a contents label of "One Mastering the Secrets" has to
/// find a heading set in small caps with an en dash after "One". The word
/// offsets are what turns a match back into a character offset the reader
/// can scroll to.
fn index_words(text: &str) -> WordIndex {
    let mut starts = Vec::new();
    let mut words = Vec::new();
    for m in word_run().find_iter(text) {
        starts.push(utf16_len(&text[..m.start()]));
        words.push(m.as_str().to_lowercase());
    }
    WordIndex {
        starts,
        joined: words.join(" "),
    }
}

/// JavaScript's `\w` — and therefore its `\b` — is ASCII-only, even for
/// non-ASCII text. Reproducing that exactly matters here: a heading word
/// ending in a non-ASCII letter fails the trailing `\b` in the web app, and
/// this port fails the same way.
fn js_is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Find `needle` in `joined` as the JavaScript pattern `\bneedle\b` would:
/// leftmost-first, non-overlapping, whole words only, exactly one occurrence.
fn unique_word_match(joined: &str, needle: &str) -> Option<usize> {
    let mut found: Option<usize> = None;
    let mut from = 0;
    while let Some(relative) = joined[from..].find(needle) {
        let at = from + relative;
        let before = joined[..at].chars().next_back();
        let first = joined[at..].chars().next().expect("needle is non-empty");
        let after_at = at + needle.len();
        let last = joined[..after_at].chars().next_back().expect("needle is non-empty");
        let next = joined[after_at..].chars().next();
        let boundary = before.is_some_and(js_is_word) != js_is_word(first)
            && js_is_word(last) != next.is_some_and(js_is_word);
        if boundary {
            if found.is_some() {
                return None;
            }
            found = Some(at);
        }
        // The JS pattern advances past each whole match, so overlaps are not
        // reconsidered.
        from = after_at;
    }
    found
}

fn locate_in(index: &WordIndex, heading: &str) -> Option<usize> {
    let words: String = non_letter().replace_all(heading, " ").into_owned();
    let lowered = words.trim().to_lowercase();
    let words: Vec<&str> = lowered.split(' ').filter(|word| !word.is_empty()).collect();
    let joined_words = words.join(" ");
    // Anything this short matches half the book, and "Part" is a real heading.
    if utf16_len(&joined_words) < 5 {
        return None;
    }

    for drop in 0..words.len() {
        // The label as written is always tried. Beyond that, a three-word
        // remainder is as short as a locator can trust.
        if drop > 0 && words.len() - drop < 3 {
            break;
        }
        let needle = words[drop..].join(" ");
        if utf16_len(&needle) < 5 {
            break;
        }
        if utf16_len(&index.joined) < utf16_len(&needle) {
            continue;
        }
        let Some(at) = unique_word_match(&index.joined, &needle) else {
            continue;
        };
        // The match begins at a word boundary, so the words before it are
        // exactly the spaces standing in front of it.
        let word = index.joined[..at].bytes().filter(|&b| b == b' ').count();
        return index.starts.get(word).copied();
    }

    None
}

/// Build the heading locator `build_contents` uses to place a contents row.
///
/// Only an unambiguous occurrence counts. A heading that appears more than
/// once in a document is a running head repeated down its pages, not the
/// place the chapter begins, and matching the first of forty would put every
/// row of a book at the top of its first page.
///
/// Indexing is lazy and per document, because only a handful of documents
/// ever need it: the ones a run of rows had to be spread across.
pub(crate) fn locate_heading(texts: &[String]) -> impl Fn(usize, &str) -> Option<usize> + '_ {
    let indexes: RefCell<HashMap<usize, WordIndex>> = RefCell::new(HashMap::new());
    move |document: usize, heading: &str| -> Option<usize> {
        if let Some(cached) = indexes.borrow().get(&document) {
            return locate_in(cached, heading);
        }
        let built = index_words(texts.get(document).map(String::as_str).unwrap_or(""));
        let result = locate_in(&built, heading);
        indexes.borrow_mut().insert(document, built);
        result
    }
}

/* -------------------------------------------------------------------------- */
/* Chapter naming (dominantTitle)                                              */
/* -------------------------------------------------------------------------- */

/// The row that most of a chapter's text belongs to.
///
/// A chapter can hold several rows — front matter folded into the first
/// chapter, or a contents that points twice into one document. Naming the
/// chapter after the row that owns the bulk of its text is what stops the
/// whole opening of a book being called "Cover".
pub(crate) fn dominant_title(rows: &[PlacedRow], text_length: usize) -> Option<String> {
    if rows.is_empty() {
        return None;
    }
    // Rows sharing an offset are the same place named twice — a part heading
    // and the chapter that opens it. Shallowest first, so the run is
    // represented by the level a reader is actually in. (The final tiebreak
    // is JS `localeCompare`; plain code-point order is used here, which can
    // disagree about case — only reachable for two same-offset same-depth
    // rows whose titles differ only by locale-sensitive ordering, and only
    // decides which of the two names the chapter.)
    let mut sorted: Vec<&PlacedRow> = rows.iter().collect();
    sorted.sort_by(|a, b| {
        a.offset
            .cmp(&b.offset)
            .then(a.depth.cmp(&b.depth))
            .then_with(|| a.title.cmp(&b.title))
    });

    let mut best = sorted[0];
    let mut best_span: isize = -1;
    let mut start = 0;
    while start < sorted.len() {
        let mut end = start;
        while end + 1 < sorted.len() && sorted[end + 1].offset == sorted[start].offset {
            end += 1;
        }
        let next = sorted.get(end + 1).map(|row| row.offset).unwrap_or(text_length);
        let span = next as isize - sorted[start].offset as isize;
        if span > best_span {
            best_span = span;
            best = sorted[start];
        }
        start = end + 1;
    }
    Some(best.title.clone())
}

/* -------------------------------------------------------------------------- */
/* The parser (parseEpub)                                                      */
/* -------------------------------------------------------------------------- */

const IMAGE_MIME: [(&str, &str); 8] = [
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("jpe", "image/jpeg"),
    ("png", "image/png"),
    ("gif", "image/gif"),
    ("webp", "image/webp"),
    ("bmp", "image/bmp"),
    ("svg", "image/svg+xml"),
];

cached_re!(document_ext, r"(?i)\.(?:x?html?|xml)$");
cached_re!(data_src, r"(?i)^\s*data:");
cached_re!(attr_id, r#"(?i)\bid\s*=\s*[\"']([^\"']+)[\"']"#);
cached_re!(attr_href, r#"(?i)\bhref\s*=\s*[\"']([^\"']+)[\"']"#);
cached_re!(attr_media_type, r#"(?i)\bmedia-type\s*=\s*[\"']([^\"']+)[\"']"#);
cached_re!(attr_idref, r#"(?i)\bidref\s*=\s*[\"']([^\"']+)[\"']"#);
cached_re!(item_tag, r"(?i)<item\b([^>]*)>");
cached_re!(spine_body, r"(?i)<spine\b[^>]*>([\s\S]*?)</spine>");
cached_re!(itemref_tag, r"(?i)<itemref\b([^>]*)>");

/// Read the whole archive into memory, keyed by entry name.
///
/// The web app's `unzipSync` is all-or-nothing: one corrupt entry fails the
/// whole archive. Any per-entry failure here reports the same "corrupt"
/// error rather than silently dropping a chapter.
fn read_archive(archive: &[u8]) -> Option<HashMap<String, Vec<u8>>> {
    let mut zip = zip::ZipArchive::new(Cursor::new(archive)).ok()?;
    let mut files = HashMap::new();
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).ok()?;
        let name = entry.name().to_string();
        let mut body = Vec::new();
        entry.read_to_end(&mut body).ok()?;
        files.insert(name, body);
    }
    Some(files)
}

/// `img1`, `img2`, … in base 36, as the web app names its image refs.
fn image_ref(counter: usize) -> String {
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut n = counter;
    let mut out = Vec::new();
    loop {
        out.push(DIGITS[n % 36]);
        n /= 36;
        if n == 0 {
            break;
        }
    }
    out.reverse();
    let mut name = String::from("img");
    name.extend(out.iter().map(|&b| b as char));
    name
}

struct Document {
    href: String,
    markup: String,
    html: String,
    text_length: usize,
    head: String,
    text: String,
    anchors: HashMap<String, usize>,
    word_count: usize,
}

/// Parse a full `.epub` buffer.
///
/// `image_url_prefix` is the route shape every extracted image's `src` is
/// rewritten to — the web app passes `format!("/api/epub/{book_id}/image/")`
/// because image refs restart at 1 for every book and the URL must be scoped
/// to the book the parser cannot see from here. `max_image_bytes` (web env
/// `EPUB_MAX_IMAGE_MB`, 8 MB) bounds one stored image; `max_documents`
/// (`EPUB_MAX_DOCUMENTS`, 4000) bounds the spine.
pub fn parse(
    archive: &[u8],
    image_url_prefix: &str,
    max_image_bytes: u64,
    max_documents: usize,
) -> Result<ParsedEpub, EpubParseError> {
    if !is_zip(archive) {
        return Err(EpubParseError::new(
            "That file is not a valid EPUB (it is not a zip archive).",
        ));
    }

    let Some(files) = read_archive(archive) else {
        return Err(EpubParseError::new(
            "That EPUB could not be opened — the archive is corrupt.",
        ));
    };

    let Some(container_file) = files.get("META-INF/container.xml") else {
        return Err(EpubParseError::new(
            "That file is missing META-INF/container.xml, so it is not a valid EPUB.",
        ));
    };

    let opf_path =
        first_match(&decode(container_file), first_match_re()).unwrap_or_default();
    if opf_path.is_empty() || !files.contains_key(&opf_path) {
        return Err(EpubParseError::new(
            "That EPUB does not point at a readable package document.",
        ));
    }

    let opf = decode(&files[&opf_path]);

    // --- metadata
    let meta_title = first_match(&opf, dc_title());
    let meta_creator = first_match(&opf, dc_creator());
    let meta_language = first_match(&opf, dc_language());

    // --- manifest: id -> href + type
    #[derive(Clone)]
    struct ManifestItem {
        href: String,
        item_type: String,
    }
    let mut manifest: Vec<ManifestItem> = Vec::new();
    let mut manifest_by_id: HashMap<String, usize> = HashMap::new();
    for caps in item_tag().captures_iter(&opf) {
        let attrs = &caps[1];
        let (Some(id), Some(href)) = (
            attr_id().captures(attrs).map(|c| c[1].to_string()),
            attr_href().captures(attrs).map(|c| c[1].to_string()),
        ) else {
            continue;
        };
        let item_type = attr_media_type()
            .captures(attrs)
            .map(|c| c[1].to_string())
            .unwrap_or_default();
        let item = ManifestItem {
            href: resolve_path(&opf_path, &decode_percent(&href)),
            item_type,
        };
        match manifest_by_id.get(&id) {
            // A duplicate id overwrites the value but keeps its position, as
            // the web app's Map does.
            Some(&index) => manifest[index] = item,
            None => {
                manifest.push(item);
                manifest_by_id.insert(id, manifest.len() - 1);
            }
        }
    }

    // --- spine
    let spine_body = spine_body()
        .captures(&opf)
        .map(|caps| caps[1].to_string())
        .unwrap_or_default();
    let mut spine: Vec<String> = Vec::new();
    for caps in itemref_tag().captures_iter(&spine_body) {
        let Some(idref) = attr_idref().captures(&caps[1]).map(|c| c[1].to_string()) else {
            continue;
        };
        let Some(&index) = manifest_by_id.get(&idref) else {
            continue;
        };
        // Trust the extension more than the declared media type: plenty of
        // EPUBs label HTML documents as application/xhtml+xml with a .html
        // file, and vice versa.
        let href = &manifest[index].href;
        if document_ext().is_match(href) {
            spine.push(href.clone());
        }
        if spine.len() >= max_documents {
            break;
        }
    }

    if spine.is_empty() {
        return Err(EpubParseError::new(
            "That EPUB has no readable chapters in its spine.",
        ));
    }

    // --- table of contents: prefer EPUB 3 nav, fall back to NCX
    let mut toc: Vec<TocEntry> = Vec::new();
    let nav_item = manifest
        .iter()
        .find(|item| item.item_type.contains("nav") || item.href.ends_with("nav.xhtml"));
    if let Some(nav_item) = nav_item {
        if let Some(file) = files.get(&nav_item.href) {
            toc = parse_nav_document(&decode(file), &nav_item.href);
        }
    }
    if toc.is_empty() {
        let ncx_item = manifest.iter().find(|item| item.href.ends_with(".ncx"));
        if let Some(ncx_item) = ncx_item {
            if let Some(file) = files.get(&ncx_item.href) {
                toc = parse_ncx(&decode(file), &opf_path);
            }
        }
    }

    // --- documents
    let mut images: Vec<ParsedImage> = Vec::new();
    let mut image_refs: HashMap<String, String> = HashMap::new();
    let mut image_counter: usize = 0;
    let mut documents: Vec<Document> = Vec::new();

    for href in &spine {
        let Some(file) = files.get(href) else {
            continue; // spine references a missing document; skip it
        };
        let markup = decode(file);

        let mut resolve_image = |raw_src: &str, alt: &str| -> Option<String> {
            if data_src().is_match(raw_src) {
                return None; // inline blobs bloat the DB
            }
            let target = resolve_path(href, &decode_percent(raw_src.trim()));
            let bytes = files.get(&target)?;
            if bytes.is_empty() || bytes.len() as u64 > max_image_bytes {
                return None;
            }

            let extension = target.rsplit('.').next().unwrap_or("").to_lowercase();
            let &mime = IMAGE_MIME
                .iter()
                .find(|(ext, _)| *ext == extension)
                .map(|(_, mime)| mime)?;
            // SVG can carry script; it is never rendered here.
            if mime == "image/svg+xml" {
                return None;
            }

            let ref_id = match image_refs.get(&target) {
                Some(existing) => existing.clone(),
                None => {
                    image_counter += 1;
                    let ref_id = image_ref(image_counter);
                    image_refs.insert(target.clone(), ref_id.clone());
                    let trimmed = alt.trim();
                    let alt = utf16_prefix(trimmed, 300);
                    images.push(ParsedImage {
                        ref_id: ref_id.clone(),
                        mime_type: mime.to_string(),
                        alt: if alt.is_empty() {
                            None
                        } else {
                            Some(alt.to_string())
                        },
                        bytes: bytes.clone(),
                    });
                    ref_id
                }
            };
            Some(format!("{image_url_prefix}{ref_id}"))
        };

        let sanitized = sanitize_epub_document(&markup, Some(&mut resolve_image));
        if sanitized.html.is_empty() {
            continue;
        }

        documents.push(Document {
            head: head_text(&sanitized.html),
            text: plain_text(&sanitized.html),
            text_length: sanitized.text_length,
            anchors: sanitized.anchors,
            word_count: count_words(&sanitized.html),
            html: sanitized.html,
            markup,
            href: href.clone(),
        });
    }

    if documents.is_empty() {
        return Err(EpubParseError::new(
            "That EPUB contained no readable text.",
        ));
    }

    let texts: Vec<String> = documents.iter().map(|document| document.text.clone()).collect();
    let locator = locate_heading(&texts);
    let output = {
        let text_lengths: Vec<usize> = documents.iter().map(|d| d.text_length).collect();
        let heads: Vec<String> = documents.iter().map(|d| d.head.clone()).collect();
        let anchors: Vec<HashMap<String, usize>> =
            documents.iter().map(|d| d.anchors.clone()).collect();
        let hrefs: Vec<String> = documents.iter().map(|d| d.href.clone()).collect();
        let fallback_title = Box::new(|index: usize| {
            title_from_document(&documents[index].markup)
                .or_else(|| title_from_image_alt(&documents[index].markup))
                .unwrap_or_else(|| format!("Section {}", index + 1))
        });
        let input = ContentsInput {
            text_lengths: &text_lengths,
            heads: &heads,
            anchors: &anchors,
            hrefs: &hrefs,
            toc: &toc,
            fallback_title,
            locate_heading: Some(Box::new(locator)),
        };
        build_contents(&input)
    };

    let mut chapters: Vec<ParsedChapter> = Vec::new();
    // Group position -> chapter index, so contents rows stay aligned.
    let mut chapter_of_group: HashMap<usize, usize> = HashMap::new();
    let mut char_offset: usize = 0;

    for (position, group) in output.groups.iter().enumerate() {
        let mut html = String::new();
        for &doc in group {
            html.push_str(&documents[doc].html);
        }
        let html = html.trim().to_string();
        if html.is_empty() {
            continue;
        }

        let rows: Vec<PlacedRow> = output
            .entries
            .iter()
            .filter(|entry| entry.index == position)
            .cloned()
            .collect();
        let index = chapters.len();
        chapter_of_group.insert(position, index);
        chapters.push(ParsedChapter {
            index,
            // A chapter is named by the row most of its text belongs to.
            href: group
                .iter()
                .map(|&doc| documents[doc].href.clone())
                .collect::<Vec<_>>()
                .join(","),
            title: {
                let title = dominant_title(&rows, utf16_len(&html))
                    .unwrap_or_else(|| format!("Section {}", index + 1));
                utf16_prefix(&title, 300).to_string()
            },
            depth: rows
                .iter()
                .map(|row| row.depth)
                .min()
                .unwrap_or(MAX_CONTENTS_DEPTH),
            in_toc: rows.iter().any(|row| row.listed),
            word_count: group.iter().map(|&doc| documents[doc].word_count).sum::<usize>() as i64,
            char_offset: char_offset as i64,
            html,
        });
        char_offset += utf16_len(&chapters[index].html);
    }

    if chapters.is_empty() {
        return Err(EpubParseError::new(
            "That EPUB contained no readable text.",
        ));
    }

    // A group with no markup never becomes a chapter, so its rows go with it.
    let mut contents: Vec<ContentsRow> = Vec::new();
    for row in &output.entries {
        if let Some(&index) = chapter_of_group.get(&row.index) {
            contents.push(ContentsRow {
                index: index as i64,
                anchor: row.anchor.clone(),
                offset: row.offset as i64,
                title: row.title.clone(),
                depth: row.depth,
            });
        }
    }

    Ok(ParsedEpub {
        title: clean_metadata(meta_title.as_deref()),
        creator: clean_metadata(meta_creator.as_deref()),
        language: clean_metadata(meta_language.as_deref()),
        chapters,
        contents,
        images,
        total_chars: char_offset as i64,
    })
}


/* -------------------------------------------------------------------------- */
/* Tests                                                                       */
/*                                                                             */
/* The TypeScript suite in `epub/parse.test.ts` is the arbiter; these are its   */
/* cases, with the fixtures built in-memory here for the same reason the web    */
/* app builds them there — every test states exactly what the archive contains  */
/* and needs no checked-in binaries.                                           */
/* -------------------------------------------------------------------------- */

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    /// The route shape the web app's default `imageUrl` produces.
    const IMAGE_PREFIX: &str = "/api/epub/image/";
    const MAX_IMAGE_BYTES: u64 = 8 * 1024 * 1024;
    const MAX_DOCUMENTS: usize = 4000;

    fn parse_book(archive: &[u8]) -> Result<ParsedEpub, EpubParseError> {
        parse(archive, IMAGE_PREFIX, MAX_IMAGE_BYTES, MAX_DOCUMENTS)
    }

    fn write_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut cursor);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            for (path, body) in entries {
                writer.start_file(*path, options).expect("entry name is valid");
                writer.write_all(body).expect("entry write into memory");
            }
            writer.finish().expect("archive finish");
        }
        cursor.into_inner()
    }

    /// Build a minimal but valid EPUB 2 container.
    fn build_epub(files: &[(&str, &str)]) -> Vec<u8> {
        let entries: Vec<(&str, &[u8])> = files
            .iter()
            .map(|(path, body)| (*path, body.as_bytes()))
            .collect();
        write_zip(&entries)
    }

    const CONTAINER: &str = r#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>"#;

    /// An OPF whose manifest and spine name `spine`, plus `extra` manifest items.
    fn opf(spine: &[&str], extra: &str) -> String {
        let items: String = spine
            .iter()
            .map(|id| {
                format!("<item id=\"{id}\" href=\"text/{id}.xhtml\" media-type=\"application/xhtml+xml\"/>")
            })
            .collect();
        let refs: String = spine
            .iter()
            .map(|id| format!("<itemref idref=\"{id}\"/>"))
            .collect();
        format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" unique-identifier="uid" version="2.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Test Book</dc:title>
    <dc:creator>Jane Author</dc:creator>
    <dc:language>en</dc:language>
  </metadata>
  <manifest>{items}{extra}</manifest>
  <spine>{refs}</spine>
</package>"#
        )
    }

    fn chapter(body: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml"><head><title>c</title></head>
<body>{body}</body></html>"#
        )
    }

    /// A body of roughly `paragraphs` paragraphs of filler.
    ///
    /// Fixtures need real page lengths. A document of three words is not a
    /// chapter, it is a plate, and the parser folds plates into the document
    /// that follows them — correct behaviour, but not what a test about
    /// contents nesting is measuring.
    fn prose(paragraphs: usize) -> String {
        (1..=paragraphs)
            .map(|i| format!("<p>{} {i}</p>", "word ".repeat(60).trim()))
            .collect::<Vec<_>>()
            .join("")
    }

    fn png_bytes(len: usize) -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        bytes.extend(std::iter::repeat_n(b'x', len));
        bytes
    }

    // The length of what a browser's `textContent` would return for a fragment
    // is deliberately not `plain_text`: that collapses whitespace for display,
    // which is precisely the rewrite the sanitiser must not make if the counts
    // it reports are to mean the same thing in the reader.
    cached_re!(comment_markup, r"(?s)<!--.*?-->");
    fn browsed_text_length(html: &str) -> usize {
        let without_comments = comment_markup().replace_all(html, "");
        let without_tags = tag_re().replace_all(&without_comments, "");
        let decoded = without_tags
            .replace("&amp;", "&")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&#39;", "'");
        utf16_len(&decoded)
    }

    /* --- isZip ---------------------------------------------------------- */

    #[test]
    fn accepts_a_real_archive() {
        let archive = build_epub(&[("a.txt", "x")]);
        assert!(is_zip(&archive));
    }

    #[test]
    fn rejects_a_non_archive() {
        assert!(!is_zip(b"%PDF-1.4 not a zip"));
        assert!(!is_zip(b""));
        // Exactly four bytes of magic is one short of a local file header.
        assert!(!is_zip(b"PK\x03\x04"));
    }

    /* --- resolvePath ---------------------------------------------------- */

    #[test]
    fn resolves_relative_to_the_document_directory() {
        assert_eq!(
            resolve_path("OEBPS/text/ch1.xhtml", "img/a.png"),
            "OEBPS/text/img/a.png"
        );
    }

    #[test]
    fn collapses_parent_segments() {
        assert_eq!(
            resolve_path("OEBPS/text/ch1.xhtml", "../images/a.png"),
            "OEBPS/images/a.png"
        );
    }

    #[test]
    fn strips_fragments_and_query_strings() {
        assert_eq!(
            resolve_path("OEBPS/ch1.xhtml", "ch2.xhtml#part3"),
            "OEBPS/ch2.xhtml"
        );
    }

    #[test]
    fn handles_absolute_archive_paths() {
        assert_eq!(resolve_path("OEBPS/ch1.xhtml", "/top.png"), "top.png");
    }

    /* --- parseEpub ------------------------------------------------------ */

    #[test]
    fn extracts_metadata_spine_and_titles() {
        let ncx = r#"<?xml version="1.0"?>
        <ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">
          <navMap>
            <navPoint id="n1" playOrder="1"><navLabel><text>Opening</text></navLabel>
              <content src="text/c1.xhtml"/></navPoint>
            <navPoint id="n2" playOrder="2"><navLabel><text>Middle</text></navLabel>
              <content src="text/c2.xhtml"/>
              <navPoint id="n3" playOrder="3"><navLabel><text>Nested</text></navLabel>
                <content src="text/c2.xhtml"/></navPoint>
            </navPoint>
          </navMap>
        </ncx>"#;
        let c1 = chapter(&format!("<h1>Opening</h1>{}", prose(6)));
        let c2 = chapter(&format!("<p>Second document body.</p>{}", prose(6)));
        let archive = build_epub(&[
            ("mimetype", "application/epub+zip"),
            ("META-INF/container.xml", CONTAINER),
            (
                "OEBPS/content.opf",
                &opf(
                    &["c1", "c2"],
                    r#"<item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>"#,
                ),
            ),
            ("OEBPS/toc.ncx", ncx),
            ("OEBPS/text/c1.xhtml", &c1),
            ("OEBPS/text/c2.xhtml", &c2),
        ]);

        let parsed = parse_book(&archive).expect("fixture parses");

        assert_eq!(parsed.title.as_deref(), Some("Test Book"));
        assert_eq!(parsed.creator.as_deref(), Some("Jane Author"));
        assert_eq!(parsed.language.as_deref(), Some("en"));
        assert_eq!(parsed.chapters.len(), 2);
        assert_eq!(parsed.chapters[0].title, "Opening");
        assert_eq!(parsed.chapters[1].title, "Middle");
        // `c2` is named by both the outer "Middle" and the nested "Nested"; the
        // document takes the shallowest entry, so this is 0 rather than 1.
        assert_eq!(parsed.chapters[1].depth, 0);
        assert!(parsed.total_chars > 0);
        assert!(parsed.chapters[0].word_count > 0);
        // Both rows for `c2` survive: a contents can name two places in one file.
        let titles: Vec<&str> = parsed.contents.iter().map(|row| row.title.as_str()).collect();
        assert_eq!(titles, ["Opening", "Middle", "Nested"]);
        assert_eq!(parsed.contents[2].index, 1);
        assert_eq!(parsed.contents[2].depth, 1);
    }

    #[test]
    fn falls_back_to_a_document_heading_when_there_is_no_toc() {
        let c1 = chapter(&format!("<h2>Chapter Title</h2>{}", prose(6)));
        let archive = build_epub(&[
            ("mimetype", "application/epub+zip"),
            ("META-INF/container.xml", CONTAINER),
            ("OEBPS/content.opf", &opf(&["c1"], "")),
            ("OEBPS/text/c1.xhtml", &c1),
        ]);

        let parsed = parse_book(&archive).expect("fixture parses");

        assert_eq!(parsed.chapters.len(), 1);
        assert_eq!(parsed.chapters[0].title, "Chapter Title");
        assert!(!parsed.chapters[0].in_toc);
        // Unlisted text still gets a row, so nothing in the book is unreachable.
        assert_eq!(parsed.contents.len(), 1);
        assert_eq!(parsed.contents[0].index, 0);
        assert_eq!(parsed.contents[0].anchor, "");
        assert_eq!(parsed.contents[0].offset, 0);
        assert_eq!(parsed.contents[0].title, "Chapter Title");
        assert_eq!(parsed.contents[0].depth, 0);
    }

    #[test]
    fn names_untitled_documents_sequentially() {
        let c1 = chapter(&prose(6));
        let c2 = chapter(&prose(6));
        let archive = build_epub(&[
            ("mimetype", "application/epub+zip"),
            ("META-INF/container.xml", CONTAINER),
            ("OEBPS/content.opf", &opf(&["c1", "c2"], "")),
            ("OEBPS/text/c1.xhtml", &c1),
            ("OEBPS/text/c2.xhtml", &c2),
        ]);

        let parsed = parse_book(&archive).expect("fixture parses");
        let titles: Vec<&str> = parsed.chapters.iter().map(|c| c.title.as_str()).collect();
        assert_eq!(titles, ["Section 1", "Section 2"]);
    }

    #[test]
    fn skips_spine_entries_missing_from_the_archive_instead_of_failing() {
        let c1 = chapter(&prose(6));
        let c2 = chapter(&prose(6));
        let archive = build_epub(&[
            ("mimetype", "application/epub+zip"),
            ("META-INF/container.xml", CONTAINER),
            ("OEBPS/content.opf", &opf(&["c1", "ghost", "c2"], "")),
            ("OEBPS/text/c1.xhtml", &c1),
            ("OEBPS/text/c2.xhtml", &c2),
        ]);

        let parsed = parse_book(&archive).expect("fixture parses");
        let titles: Vec<&str> = parsed.chapters.iter().map(|c| c.title.as_str()).collect();
        assert_eq!(titles, ["Section 1", "Section 2"]);
    }

    #[test]
    fn rejects_a_non_zip_buffer_with_the_same_message() {
        let error = parse_book(b"just text").expect_err("not a zip");
        assert_eq!(
            error.message,
            "That file is not a valid EPUB (it is not a zip archive)."
        );
    }

    #[test]
    fn rejects_a_zip_with_no_container_xml() {
        let archive = build_epub(&[("hello.txt", "hi")]);
        let error = parse_book(&archive).expect_err("no container");
        assert_eq!(
            error.message,
            "That file is missing META-INF/container.xml, so it is not a valid EPUB."
        );
    }

    #[test]
    fn rejects_a_corrupt_archive() {
        // Starts with the magic and nothing else: the central directory the
        // reader needs is simply not there.
        let error = parse_book(b"PK\x03\x04\0\0\0\0").expect_err("truncated zip");
        assert_eq!(
            error.message,
            "That EPUB could not be opened — the archive is corrupt."
        );
    }

    #[test]
    fn rejects_an_epub_whose_spine_is_empty() {
        let archive = build_epub(&[
            ("META-INF/container.xml", CONTAINER),
            ("OEBPS/content.opf", &opf(&[], "")),
        ]);
        let error = parse_book(&archive).expect_err("empty spine");
        assert_eq!(
            error.message,
            "That EPUB has no readable chapters in its spine."
        );
    }

    #[test]
    fn strips_the_invisible_characters_books_leave_in_their_titles() {
        // 12 Rules for Life prefixes every contents label with a zero-width
        // non-joiner, and its NCX nests them four deep. The character is real
        // text and would survive into the contents row as a title that cannot
        // be selected and looks a space out of place.
        let zwnj = '\u{200C}';
        let bom = '\u{FEFF}';
        let ncx = format!(
            r#"<?xml version="1.0"?><ncx><navMap>
          <navPoint id="p1"><navLabel><text>{zwnj}RULE 1: Stand up</text></navLabel><content src="text/c1.xhtml"/>
            <navPoint id="p2"><navLabel><text>{bom}FOREWORD{bom}</text></navLabel><content src="text/c2.xhtml"/></navPoint>
          </navPoint>
        </navMap></ncx>"#
        );
        let c1 = chapter(&format!("<h1>{zwnj}Rule 1{zwnj}</h1>{}", prose(6)));
        let c2 = chapter(&prose(6));
        let archive = build_epub(&[
            ("mimetype", "application/epub+zip"),
            ("META-INF/container.xml", CONTAINER),
            (
                "OEBPS/content.opf",
                &opf(
                    &["c1", "c2"],
                    r#"<item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>"#,
                ),
            ),
            ("OEBPS/toc.ncx", &ncx),
            ("OEBPS/text/c1.xhtml", &c1),
            ("OEBPS/text/c2.xhtml", &c2),
        ]);

        let parsed = parse_book(&archive).expect("fixture parses");

        let contents_titles: Vec<&str> =
            parsed.contents.iter().map(|row| row.title.as_str()).collect();
        assert_eq!(contents_titles, ["RULE 1: Stand up", "FOREWORD"]);
        // And the chapter takes its name from the contents row rather than from
        // the document heading, so the debris cannot reach the chrome either.
        let chapter_titles: Vec<&str> =
            parsed.chapters.iter().map(|c| c.title.as_str()).collect();
        assert_eq!(chapter_titles, ["RULE 1: Stand up", "FOREWORD"]);
    }

    /* --- sanitisation --------------------------------------------------- */

    const HOSTILE_BODY: &str = "
      <p onclick=\"alert(1)\">Keep this text</p>
      <script>alert('xss')</script>
      <a href=\"javascript:alert(1)\">bad link</a>
      <a href=\"https://example.com\">good link</a>
      <iframe src=\"https://evil.test\"></iframe>
      <p style=\"position:fixed;background:url(https://evil.test/x)\">Styled</p>
      <svg><script>alert(2)</script></svg>
      <object data=\"evil.swf\"></object>
      <p>Tail</p>
    ";

    fn hostile_archive() -> Vec<u8> {
        let body = chapter(HOSTILE_BODY);
        build_epub(&[
            ("META-INF/container.xml", CONTAINER),
            ("OEBPS/content.opf", &opf(&["c1"], "")),
            ("OEBPS/text/c1.xhtml", &body),
        ])
    }

    #[test]
    fn strips_scripts_handlers_and_dangerous_urls_but_keeps_the_prose() {
        let parsed = parse_book(&hostile_archive()).expect("fixture parses");
        let html = &parsed.chapters[0].html;

        assert!(html.contains("Keep this text"));
        assert!(html.contains("Tail"));
        assert!(html.contains("good link"));
        assert!(html.contains("https://example.com"));

        assert!(!html.to_lowercase().contains("<script"));
        assert!(!html.to_lowercase().contains("onclick"));
        assert!(!html.to_lowercase().contains("javascript:"));
        assert!(!html.to_lowercase().contains("<iframe"));
        assert!(!html.to_lowercase().contains("<object"));
        assert!(!html.to_lowercase().contains("<svg"));
        assert!(!html.to_lowercase().contains("style="));
        assert!(!html.contains("evil.test"));
    }

    #[test]
    fn unwraps_unknown_elements_instead_of_discarding_their_text() {
        let body = chapter("<custom-widget>inner text</custom-widget>");
        let archive = build_epub(&[
            ("META-INF/container.xml", CONTAINER),
            ("OEBPS/content.opf", &opf(&["c1"], "")),
            ("OEBPS/text/c1.xhtml", &body),
        ]);

        let parsed = parse_book(&archive).expect("fixture parses");
        let html = &parsed.chapters[0].html;
        assert!(html.contains("inner text"));
        assert!(!html.to_lowercase().contains("custom-widget"));
    }

    #[test]
    fn escapes_stray_ampersands_so_text_cannot_re_open_an_entity() {
        let body = chapter("<p>Tom &amp; Jerry &lt;b&gt; 5 &lt; 6</p>");
        let archive = build_epub(&[
            ("META-INF/container.xml", CONTAINER),
            ("OEBPS/content.opf", &opf(&["c1"], "")),
            ("OEBPS/text/c1.xhtml", &body),
        ]);

        let parsed = parse_book(&archive).expect("fixture parses");
        let html = &parsed.chapters[0].html;
        assert!(html.contains("Tom &amp; Jerry"));
        assert!(html.contains("&lt;b&gt;"));
        assert!(!html.contains("<b>"));
    }

    #[test]
    fn emits_exactly_the_text_it_counted() {
        // The counts this module reports — a section's length, a heading's
        // offset, a contents row's position — are only meaningful if the reader
        // counts the same characters in the browser. Any tidying that drops or
        // adds text silently breaks that agreement, and the symptom is not a
        // wrong number, it is every contents row past the first few landing on
        // the wrong page. So the invariant is asserted rather than the intent.
        let source = [
            "<p>One.</p>",
            "      <p>Two &amp; a half.</p>",
            "<p>",
            "        <br />",
            "        <br />",
            "        <br />",
            "        <br />",
            "        Three.",
            "        <br />",
            "</p>",
            "<p>Four   with    spaces.</p>",
            "<h2>Five</h2>",
            "",
        ]
        .join("\n");
        let markup = chapter(&source);

        let sanitized = sanitize_epub_document(&markup, None);

        assert_eq!(browsed_text_length(&sanitized.html), sanitized.text_length);
    }

    /* --- images --------------------------------------------------------- */

    #[test]
    fn extracts_images_and_rewrites_src_to_the_app_route() {
        let png = png_bytes(64);
        let doc = chapter("<p><img src=\"../img/pic.png\" alt=\"A plate\"/></p>");
        let archive = write_zip(&[
            ("META-INF/container.xml", CONTAINER.as_bytes()),
            ("OEBPS/content.opf", opf(&["c1"], "").as_bytes()),
            ("OEBPS/text/c1.xhtml", doc.as_bytes()),
            ("OEBPS/img/pic.png", &png),
        ]);

        let parsed = parse_book(&archive).expect("fixture parses");

        assert_eq!(parsed.images.len(), 1);
        assert_eq!(parsed.images[0].mime_type, "image/png");
        assert_eq!(parsed.images[0].alt.as_deref(), Some("A plate"));
        let html = &parsed.chapters[0].html;
        assert!(html.contains(&format!("src=\"{IMAGE_PREFIX}img1\"")));
        assert!(html.contains("alt=\"A plate\""));
    }

    #[test]
    fn reuses_one_stored_image_for_repeated_references() {
        let png = png_bytes(32);
        let doc = chapter(
            "<img src=\"../img/a.png\" alt=\"one\"/><img src=\"../img/a.png\" alt=\"two\"/>",
        );
        let archive = write_zip(&[
            ("META-INF/container.xml", CONTAINER.as_bytes()),
            ("OEBPS/content.opf", opf(&["c1"], "").as_bytes()),
            ("OEBPS/text/c1.xhtml", doc.as_bytes()),
            ("OEBPS/img/a.png", &png),
        ]);

        let parsed = parse_book(&archive).expect("fixture parses");
        assert_eq!(parsed.images.len(), 1);
    }

    #[test]
    fn drops_svg_which_can_carry_script() {
        let doc = chapter("<img src=\"../img/v.svg\" alt=\"vec\"/>");
        let archive = write_zip(&[
            ("META-INF/container.xml", CONTAINER.as_bytes()),
            ("OEBPS/content.opf", opf(&["c1"], "").as_bytes()),
            ("OEBPS/text/c1.xhtml", doc.as_bytes()),
            ("OEBPS/img/v.svg", b"<svg onload=\"alert(1)\"></svg>"),
        ]);

        let parsed = parse_book(&archive).expect("fixture parses");
        assert_eq!(parsed.images.len(), 0);
        let html = &parsed.chapters[0].html;
        assert!(!html.to_lowercase().contains("<img"));
        // The alt text survives so the reference is not silently lost.
        assert!(html.contains("vec"));
    }

    #[test]
    fn drops_inline_data_blobs() {
        let body = chapter(
            "<p><img src=\"data:image/png;base64,iVBORw0KGgo=\" alt=\"inline\"/></p>",
        );
        let archive = build_epub(&[
            ("META-INF/container.xml", CONTAINER),
            ("OEBPS/content.opf", &opf(&["c1"], "")),
            ("OEBPS/text/c1.xhtml", &body),
        ]);

        let parsed = parse_book(&archive).expect("fixture parses");
        assert_eq!(parsed.images.len(), 0);
        assert!(!parsed.chapters[0].html.contains("data:image"));
    }

    /* --- metadata cleanup ------------------------------------------------ */

    /// Build a one-document EPUB and return what the parser reports.
    fn parse_metadata(opf_body: &str) -> ParsedEpub {
        let container = r#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#;
        let package = format!(
            r#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/">{opf_body}</metadata><manifest><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine></package>"#
        );
        let document =
            r#"<?xml version="1.0"?><html xmlns="http://www.w3.org/1999/xhtml"><body><p>Hello.</p></body></html>"#;
        let archive = build_epub(&[
            ("mimetype", "application/epub+zip"),
            ("META-INF/container.xml", container),
            ("OEBPS/content.opf", &package),
            ("OEBPS/c1.xhtml", document),
        ]);
        parse_book(&archive).expect("fixture parses")
    }

    #[test]
    fn strips_the_trailing_semicolon_cataloguers_leave_on_creators() {
        let parsed = parse_metadata(
            "<dc:title>Project Hail Mary</dc:title><dc:creator>Andy Weir;</dc:creator>",
        );
        assert_eq!(parsed.creator.as_deref(), Some("Andy Weir"));
    }

    #[test]
    fn strips_leading_and_repeated_separators() {
        let parsed = parse_metadata("<dc:creator>; Mary Shelley ;</dc:creator>");
        assert_eq!(parsed.creator.as_deref(), Some("Mary Shelley"));
    }

    #[test]
    fn collapses_whitespace_inside_a_value() {
        let parsed = parse_metadata("<dc:creator>Andy\n  Weir</dc:creator>");
        assert_eq!(parsed.creator.as_deref(), Some("Andy Weir"));
    }

    #[test]
    fn returns_none_for_an_empty_tag_rather_than_an_empty_string() {
        let parsed = parse_metadata("<dc:creator>   </dc:creator>");
        assert_eq!(parsed.creator, None);
    }

    #[test]
    fn decodes_entities_in_the_title() {
        let parsed = parse_metadata("<dc:title>Frankenstein &amp; the Monster</dc:title>");
        assert_eq!(parsed.title.as_deref(), Some("Frankenstein & the Monster"));
    }

    #[test]
    fn leaves_an_ordinary_multi_author_string_intact() {
        let parsed = parse_metadata("<dc:creator>David Thomas &amp; Andrew Hunt</dc:creator>");
        assert_eq!(parsed.creator.as_deref(), Some("David Thomas & Andrew Hunt"));
    }

    /* --- image-only section titles --------------------------------------- */

    /// EPUB whose single document is an illustration with no heading.
    fn parse_illustration(alt: &str) -> ParsedEpub {
        let container = r#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#;
        let package = r#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>T</dc:title></metadata><manifest><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/><item id="i1" href="i1.png" media-type="image/png"/></manifest><spine><itemref idref="c1"/></spine></package>"#;
        let document = format!(
            r#"<?xml version="1.0"?><html xmlns="http://www.w3.org/1999/xhtml"><body><div><img src="i1.png" alt="{alt}"/></div></body></html>"#
        );
        let png = [0x89u8, 0x50, 0x4e, 0x47];
        let archive = write_zip(&[
            ("mimetype", b"application/epub+zip"),
            ("META-INF/container.xml", container.as_bytes()),
            ("OEBPS/content.opf", package.as_bytes()),
            ("OEBPS/c1.xhtml", document.as_bytes()),
            ("OEBPS/i1.png", &png),
        ]);
        parse(&archive, "/api/image/", MAX_IMAGE_BYTES, MAX_DOCUMENTS)
            .expect("fixture parses")
    }

    #[test]
    fn uses_a_descriptive_alt_as_the_section_title() {
        let parsed = parse_illustration("The ship Grapevine in orbit around Tau Ceti");
        assert_eq!(
            parsed.chapters[0].title,
            "The ship Grapevine in orbit around Tau Ceti"
        );
    }

    #[test]
    fn rejects_placeholder_alts_which_are_less_useful_than_a_positional_title() {
        assert_eq!(parse_illustration("image").chapters[0].title, "Section 1");
        assert_eq!(parse_illustration("Image 3").chapters[0].title, "Section 1");
        assert_eq!(parse_illustration("img_01").chapters[0].title, "Section 1");
        // A single word carries too little to identify a plate.
        assert_eq!(parse_illustration("Grapevine").chapters[0].title, "Section 1");
    }

    #[test]
    fn still_prefers_a_real_heading_over_the_alt_text() {
        let container = r#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#;
        let package = r#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>T</dc:title></metadata><manifest><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/><item id="i1" href="i1.png" media-type="image/png"/></manifest><spine><itemref idref="c1"/></spine></package>"#;
        let document =
            r#"<?xml version="1.0"?><html xmlns="http://www.w3.org/1999/xhtml"><body><h2>Chapter One</h2><img src="i1.png" alt="Some plate"/></body></html>"#;
        let png = [0x89u8, 0x50, 0x4e, 0x47];
        let archive = write_zip(&[
            ("mimetype", b"application/epub+zip"),
            ("META-INF/container.xml", container.as_bytes()),
            ("OEBPS/content.opf", package.as_bytes()),
            ("OEBPS/c1.xhtml", document.as_bytes()),
            ("OEBPS/i1.png", &png),
        ]);
        let parsed = parse_book(&archive).expect("fixture parses");
        assert_eq!(parsed.chapters[0].title, "Chapter One");
    }

    /* --- NCX table of contents nesting ------------------------------------ */

    fn nested_archive() -> Vec<u8> {
        // A four-level chain, each level pointing at its own document,
        // matching the spine order c1..c4.
        let names = ["Part One", "Chapter One", "Scene A", "Beat 1"];
        let chain = names
            .iter()
            .enumerate()
            .map(|(i, name)| {
                format!(
                    "<navPoint id=\"p{}\" playOrder=\"{}\"><navLabel><text>{name}</text></navLabel><content src=\"text/c{}.xhtml\"/>",
                    i + 1,
                    i + 1,
                    i + 1
                )
            })
            .collect::<Vec<_>>()
            .join("")
            + &"</navPoint>".repeat(names.len());

        let mut files: Vec<(String, String)> = vec![
            ("mimetype".to_string(), "application/epub+zip".to_string()),
            ("META-INF/container.xml".to_string(), CONTAINER.to_string()),
            (
                "OEBPS/content.opf".to_string(),
                opf(
                    &["c1", "c2", "c3", "c4"],
                    r#"<item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>"#,
                ),
            ),
            (
                "OEBPS/toc.ncx".to_string(),
                format!("<?xml version=\"1.0\"?><ncx><navMap>{chain}</navMap></ncx>"),
            ),
        ];
        for (i, name) in names.iter().enumerate() {
            files.push((
                format!("OEBPS/text/c{}.xhtml", i + 1),
                chapter(&format!("<h1>{name}</h1>{}", prose(6))),
            ));
        }
        let entries: Vec<(&str, &[u8])> = files
            .iter()
            .map(|(path, body)| (path.as_str(), body.as_bytes()))
            .collect();
        write_zip(&entries)
    }

    #[test]
    fn numbers_a_deeply_nested_toc_0_1_2_3_rather_than_0_1_2_3_4_5_6() {
        // The regression this guards against is invisible at one level of
        // nesting. A `<navPoint ...>...</navPoint>` regex stops at the
        // *child's* closing tag, so that close never reaches the depth counter
        // and depth climbs by one per entry forever — a real book came out with
        // depths 0..46, which renders a contents list as a single
        // over-indented column.
        let parsed = parse_book(&nested_archive()).expect("fixture parses");
        let depths: Vec<i64> = parsed.chapters.iter().map(|c| c.depth).collect();
        assert_eq!(depths, [0, 1, 2, 3]);
    }

    #[test]
    fn does_not_let_depth_grow_with_the_number_of_entries() {
        // The same four levels reached through eight sibling entries instead
        // of one chain: depth must still top out at 3.
        let names = ["Part One", "Chapter One", "Scene A", "Beat 1"];
        let point = |name: &str, file: &str, anchor: &str| {
            format!(
                "<navPoint id=\"x{name}\" playOrder=\"1\"><navLabel><text>{name}</text></navLabel><content src=\"text/{file}.xhtml#{anchor}\"/>"
            )
        };
        let mut body = String::new();
        for i in 0..8 {
            for (level, file) in ["c1", "c2", "c3", "c4"].iter().enumerate() {
                let letter = ["a", "b", "c", "d"][level];
                body.push_str(&point(
                    &format!("{}{i}", names[level]),
                    file,
                    &format!("{letter}{i}"),
                ));
            }
            body.push_str(&"</navPoint>".repeat(4));
        }
        // Each document needs the anchors its eight rows point at, or they
        // would all collapse onto one offset and be treated as wreckage to be
        // spread.
        let anchored = |letter: &str| -> String {
            let headings: String = (0..8)
                .map(|i| format!("<h2 id=\"{letter}{i}\">Section {i}</h2>"))
                .collect();
            format!("{}{headings}", prose(6))
        };
        let c1 = chapter(&anchored("a"));
        let c2 = chapter(&anchored("b"));
        let c3 = chapter(&anchored("c"));
        let c4 = chapter(&anchored("d"));
        let ncx = format!("<?xml version=\"1.0\"?><ncx><navMap>{body}</navMap></ncx>");
        let archive = build_epub(&[
            ("mimetype", "application/epub+zip"),
            ("META-INF/container.xml", CONTAINER),
            (
                "OEBPS/content.opf",
                &opf(
                    &["c1", "c2", "c3", "c4"],
                    r#"<item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>"#,
                ),
            ),
            ("OEBPS/toc.ncx", &ncx),
            ("OEBPS/text/c1.xhtml", &c1),
            ("OEBPS/text/c2.xhtml", &c2),
            ("OEBPS/text/c3.xhtml", &c3),
            ("OEBPS/text/c4.xhtml", &c4),
        ]);

        let parsed = parse_book(&archive).expect("fixture parses");

        assert_eq!(parsed.contents.len(), 32);
        assert_eq!(
            parsed.chapters.iter().map(|c| c.depth).max().expect("chapters"),
            3
        );
    }

    #[test]
    fn names_a_document_by_its_own_toc_entry_not_a_nested_one_that_repeats_the_href() {
        let c1 = chapter(&prose(6));
        let ncx = r#"<?xml version="1.0"?><ncx><navMap>
            <navPoint id="p1"><navLabel><text>Part One</text></navLabel><content src="text/c1.xhtml"/>
              <navPoint id="p2"><navLabel><text>Chapter One</text></navLabel><content src="text/c1.xhtml"/>
              </navPoint>
            </navPoint>
          </navMap></ncx>"#;
        let archive = build_epub(&[
            ("mimetype", "application/epub+zip"),
            ("META-INF/container.xml", CONTAINER),
            (
                "OEBPS/content.opf",
                &opf(
                    &["c1"],
                    r#"<item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>"#,
                ),
            ),
            ("OEBPS/toc.ncx", ncx),
            ("OEBPS/text/c1.xhtml", &c1),
        ]);

        let parsed = parse_book(&archive).expect("fixture parses");

        assert_eq!(parsed.chapters.len(), 1);
        assert_eq!(parsed.chapters[0].title, "Part One");
        assert_eq!(parsed.chapters[0].depth, 0);
        // Both rows survive: they are two names for the same place, not wreckage.
        let titles: Vec<&str> = parsed.contents.iter().map(|row| row.title.as_str()).collect();
        assert_eq!(titles, ["Part One", "Chapter One"]);
        assert!(parsed.contents.iter().all(|row| row.offset == 0));
    }

    #[test]
    fn still_terminates_on_an_unclosed_navpoint_rather_than_reporting_a_huge_depth() {
        // Malformed NCX in the wild: the file ends with tags still open.
        let c1 = chapter(&prose(2));
        let c2 = chapter(&prose(3));
        let ncx = r#"<?xml version="1.0"?><ncx><navMap>
            <navPoint id="p1"><navLabel><text>One</text></navLabel><content src="text/c1.xhtml"/>
            <navPoint id="p2"><navLabel><text>Two</text></navLabel><content src="text/c2.xhtml"/>
          </navMap></ncx>"#;
        let archive = build_epub(&[
            ("mimetype", "application/epub+zip"),
            ("META-INF/container.xml", CONTAINER),
            (
                "OEBPS/content.opf",
                &opf(
                    &["c1", "c2"],
                    r#"<item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>"#,
                ),
            ),
            ("OEBPS/toc.ncx", ncx),
            ("OEBPS/text/c1.xhtml", &c1),
            ("OEBPS/text/c2.xhtml", &c2),
        ]);

        let parsed = parse_book(&archive).expect("fixture parses");

        let titles: Vec<&str> = parsed.chapters.iter().map(|c| c.title.as_str()).collect();
        assert_eq!(titles, ["One", "Two"]);
        assert_eq!(parsed.chapters.iter().map(|c| c.depth).max().expect("chapters"), 1);
    }

    /* --- buildContents --------------------------------------------------- */

    /// One document of the `build_contents` input.
    struct DocSpec {
        text: String,
        head: Option<String>,
        anchors: Vec<(String, usize)>,
    }

    fn doc(text: &str) -> DocSpec {
        DocSpec {
            text: text.to_string(),
            head: None,
            anchors: Vec::new(),
        }
    }

    impl DocSpec {
        fn head(mut self, head: &str) -> Self {
            self.head = Some(head.to_string());
            self
        }
        fn anchors(mut self, pairs: &[(&str, usize)]) -> Self {
            self.anchors = pairs
                .iter()
                .map(|(key, value)| (key.to_string(), *value))
                .collect();
            self
        }
    }

    /// One row of the book's own contents.
    struct TocSpec {
        label: String,
        href: String,
        fragment: String,
        depth: i64,
    }

    fn toc_row(label: &str, href: &str) -> TocSpec {
        TocSpec {
            label: label.to_string(),
            href: href.to_string(),
            fragment: String::new(),
            depth: 0,
        }
    }

    impl TocSpec {
        fn fragment(mut self, fragment: &str) -> Self {
            self.fragment = fragment.to_string();
            self
        }
        fn depth(mut self, depth: i64) -> Self {
            self.depth = depth;
            self
        }
    }

    /// Build the input `build_contents` takes, with the same defaults the
    /// TypeScript test's helper uses.
    fn contents_of(documents: &[DocSpec], toc: &[TocSpec]) -> ContentsOutput {
        contents_of_with_locator(documents, toc, None)
    }

    fn contents_of_with_locator(
        documents: &[DocSpec],
        toc: &[TocSpec],
        locate: Option<HeadingLocator<'_>>,
    ) -> ContentsOutput {
        let text_lengths: Vec<usize> = documents.iter().map(|d| utf16_len(&d.text)).collect();
        let heads: Vec<String> = documents
            .iter()
            .map(|d| {
                d.head
                    .clone()
                    .unwrap_or_else(|| utf16_prefix(&d.text, 200).to_string())
            })
            .collect();
        let anchors: Vec<HashMap<String, usize>> = documents
            .iter()
            .map(|d| d.anchors.iter().cloned().collect())
            .collect();
        let hrefs: Vec<String> = (1..=documents.len())
            .map(|index| format!("text/c{index}.xhtml"))
            .collect();
        let toc_entries: Vec<TocEntry> = toc
            .iter()
            .map(|entry| TocEntry {
                label: entry.label.clone(),
                href: entry.href.clone(),
                fragment: entry.fragment.clone(),
                depth: entry.depth,
            })
            .collect();

        let input = ContentsInput {
            text_lengths: &text_lengths,
            heads: &heads,
            anchors: &anchors,
            hrefs: &hrefs,
            toc: &toc_entries,
            fallback_title: Box::new(|index: usize| format!("Section {}", index + 1)),
            locate_heading: locate,
        };
        build_contents(&input)
    }

    #[test]
    fn folds_a_document_too_small_to_be_a_page_into_the_one_after_it() {
        // A half-title page is not a chapter, and listing it as one gives the
        // reader a row that opens a nearly blank page.
        let out = contents_of(
            &[doc("Half title"), doc(&"x".repeat(2000))],
            &[
                toc_row("Chapter One", "text/c2.xhtml"),
                toc_row("Half title", "text/c1.xhtml"),
            ],
        );

        assert_eq!(out.groups, vec![vec![0usize, 1]]);
        assert_eq!(out.entries.len(), 2);
        assert_eq!(out.entries[0].title, "Half title");
        assert_eq!(out.entries[0].offset, 0);
        assert_eq!(out.entries[0].depth, 0);
        assert!(out.entries[0].listed);
        assert_eq!(out.entries[1].title, "Chapter One");
        assert_eq!(out.entries[1].offset, 10);
        assert_eq!(out.entries[1].depth, 0);
    }

    #[test]
    fn folds_a_duplicated_running_head_into_the_chapter_it_introduces() {
        // Some converters split a page in two, leaving a stub that holds
        // nothing but the running head. The stub is a quarter the length of
        // the chapter that follows and opens with the same words.
        let running_head = "RULE 1: The Obvious Is Overrated";
        let out = contents_of(
            &[
                doc(running_head),
                doc(&format!("{running_head} {}", "x".repeat(3000))),
                doc(&"y".repeat(3000)),
            ],
            &[
                toc_row("RULE 1", "text/c1.xhtml"),
                toc_row("RULE 2", "text/c3.xhtml"),
            ],
        );

        assert_eq!(out.groups, vec![vec![0usize, 1], vec![2]]);
        // The running head is not a chapter of its own; RULE 1 names what it
        // opens.
        assert_eq!(
            out.entries
                .iter()
                .filter(|row| row.title == "RULE 1")
                .count(),
            1
        );
        assert_eq!(
            out.entries
                .iter()
                .find(|row| row.title == "RULE 2")
                .expect("RULE 2 row")
                .index,
            1
        );
    }

    #[test]
    fn keeps_a_brief_chapter_that_is_not_a_running_head() {
        // An author's note is short but is not the head of the chapter after it.
        let out = contents_of(
            &[
                doc(&format!("A note from the author. {}", "word ".repeat(80))),
                doc(&"z".repeat(3000)),
            ],
            &[
                toc_row("Author's Note", "text/c1.xhtml"),
                toc_row("Chapter One", "text/c2.xhtml"),
            ],
        );

        assert_eq!(out.groups, vec![vec![0usize], vec![1]]);
    }

    #[test]
    fn shifts_a_row_by_the_text_of_the_plates_folded_in_front_of_it() {
        let out = contents_of(
            &[
                doc("Half title"),
                doc(&"x".repeat(2000)).anchors(&[("top", 0)]),
            ],
            &[
                toc_row("Half title", "text/c1.xhtml"),
                toc_row("Chapter One", "text/c2.xhtml").fragment("top"),
            ],
        );

        // "Half title" is ten characters, so an anchor in the second document
        // opens ten characters into the chapter that absorbed it.
        assert_eq!(out.entries[1].title, "Chapter One");
        assert_eq!(out.entries[1].anchor, "top");
        assert_eq!(out.entries[1].offset, 10);
    }

    #[test]
    fn spreads_a_run_of_rows_that_all_name_one_document_across_the_text_after_it() {
        // The signature of a book whose single HTML file was split up: the
        // contents still points at the original, so every row lands at offset
        // zero.
        let out = contents_of(
            &[
                doc(&"a".repeat(4000)),
                doc(&"b".repeat(4000)),
                doc(&"c".repeat(4000)),
            ],
            &["One", "Two", "Three", "Four", "Five", "Six"]
                .iter()
                .map(|label| toc_row(label, "text/c1.xhtml"))
                .collect::<Vec<_>>(),
        );

        // Two rows per document, in contents order, spread by how long each
        // document is — and no longer stacked at the top of the first one.
        let placed: Vec<(usize, usize)> = out
            .entries
            .iter()
            .map(|row| (row.index, row.offset))
            .collect();
        assert_eq!(
            placed,
            [(0usize, 1000usize), (0, 3000), (1, 1000), (1, 3000), (2, 1000), (2, 3000)]
        );
    }

    #[test]
    fn leaves_two_rows_on_one_document_alone() {
        // A part heading and the chapter that opens it are two names for one
        // place, not a contents pointing at a file that no longer exists.
        let out = contents_of(
            &[doc(&"x".repeat(4000))],
            &[
                toc_row("Part One", "text/c1.xhtml"),
                toc_row("Chapter One", "text/c1.xhtml").depth(1),
            ],
        );

        let rows: Vec<(&str, usize, i64)> = out
            .entries
            .iter()
            .map(|row| (row.title.as_str(), row.offset, row.depth))
            .collect();
        assert_eq!(rows, [("Part One", 0, 0), ("Chapter One", 0, 1)]);
    }

    #[test]
    fn falls_back_to_the_pages_own_name_when_the_contents_row_has_no_name() {
        // Converters write "Unnamed" when they cannot find a heading, which
        // says less than the heading sitting on the page does. Three of *What
        // Every BODY Is Saying*'s rows are like this, and the page here names
        // both sections.
        let text_lengths = [2000usize, 2000];
        let heads = [
            "The Other Side of the Coin".to_string(),
            "Chapter One".to_string(),
        ];
        let anchors = [HashMap::new(), HashMap::new()];
        let hrefs = ["text/c1.xhtml".to_string(), "text/c2.xhtml".to_string()];
        let toc_entries = [
            TocEntry {
                label: "Unnamed".to_string(),
                href: "text/c1.xhtml".to_string(),
                fragment: String::new(),
                depth: 0,
            },
            TocEntry {
                label: "Untitled".to_string(),
                href: "text/c2.xhtml".to_string(),
                fragment: String::new(),
                depth: 0,
            },
        ];
        let input = ContentsInput {
            text_lengths: &text_lengths,
            heads: &heads,
            anchors: &anchors,
            hrefs: &hrefs,
            toc: &toc_entries,
            fallback_title: Box::new(|index: usize| format!("Section {}", index + 1)),
            locate_heading: None,
        };
        let out = build_contents(&input);

        let titles: Vec<&str> = out.entries.iter().map(|row| row.title.as_str()).collect();
        assert_eq!(titles, ["Section 1", "Section 2"]);
    }

    #[test]
    fn prefers_where_the_heading_really_is_over_a_guess() {
        // The spread can only guess by proportion. The book still holds each
        // heading, so a row that finds its heading goes there instead.
        let labels = [
            "Alpha One",
            "Beta Two",
            "Gamma Three",
            "Delta Four",
            "Epsilon Five",
            "Zeta Six",
        ];
        let documents: Vec<DocSpec> = (0..4)
            .map(|index| {
                doc(&"x".repeat(4000)).head(&format!("{} and then a great deal of text", labels[index]))
            })
            .collect();
        let toc: Vec<TocSpec> = labels
            .iter()
            .map(|label| toc_row(label, "text/c1.xhtml"))
            .collect();
        let locate: HeadingLocator<'_> = Box::new(move |document: usize, heading: &str| {
                // Each heading lives in exactly one document, and no document
                // holds more than a third of them, so none of them looks like
                // a printed contents.
                let at = labels.iter().position(|label| *label == heading)?;
                if document != 1 + at / 2 {
                    return None;
                }
                Some((at % 2) * 1000 + 100)
            });

        let out = contents_of_with_locator(&documents, &toc, Some(locate));

        // The wrapper the contents pointed at is now text nothing names, so it
        // keeps a row of its own rather than becoming unreachable.
        let rows: Vec<(&str, usize, usize)> = out
            .entries
            .iter()
            .map(|row| (row.title.as_str(), row.index, row.offset))
            .collect();
        assert_eq!(
            rows,
            [
                ("Section 1", 0, 0),
                ("Alpha One", 1, 100),
                ("Beta Two", 1, 1100),
                ("Gamma Three", 2, 100),
                ("Delta Four", 2, 1100),
                ("Epsilon Five", 3, 100),
                ("Zeta Six", 3, 1100),
            ]
        );
    }

    /* --- locateHeading ---------------------------------------------------- */

    fn sample_texts() -> [String; 2] {
        [
            "The Chapter One begins here and runs on for a while.".to_string(),
            "Chapter One again in the running head, then more of the text.".to_string(),
        ]
    }

    #[test]
    fn finds_a_heading_and_reports_where_it_starts() {
        let texts = sample_texts();
        let find = locate_heading(&texts);
        assert_eq!(find(0, "Chapter One"), Some(4));
    }

    #[test]
    fn ignores_a_heading_repeated_down_the_page_as_a_running_head() {
        // Matching the first of forty would put a row at the top of the
        // document.
        let texts = [format!("Chapter One. {} Chapter One.", "text ".repeat(40))];
        let repeated = locate_heading(&texts);
        assert_eq!(repeated(0, "Chapter One"), None);
    }

    #[test]
    fn is_not_fooled_by_punctuation_or_capitals() {
        let texts = sample_texts();
        let find = locate_heading(&texts);
        assert_eq!(find(0, "chapter one"), Some(4));
        assert_eq!(find(0, "CHAPTER\u{2014}ONE"), Some(4));
    }

    #[test]
    fn refuses_a_heading_too_short_to_be_evidence_of_anything() {
        let texts = sample_texts();
        let find = locate_heading(&texts);
        assert_eq!(find(0, "One"), None);
        assert_eq!(find(0, "Nope"), None);
    }

    #[test]
    fn matches_whole_words_only() {
        let texts = sample_texts();
        let find = locate_heading(&texts);
        assert_eq!(find(0, "hapter One"), None);
        assert_eq!(find(0, "Chapter One begins"), Some(4));
    }

    #[test]
    fn finds_a_heading_a_truncated_contents_row_only_half_names() {
        // *What Every BODY Is Saying* lists its rows cut to a fixed width —
        // "FOUR Torso Tips: Nonverbals of the" — and stops, while the page
        // reads on to "...the Upper Body". Without the truncation rule this row
        // can only be placed by guessing at its position in the text, which is
        // what puts a contents jump a page or two out.
        let texts = [
            "Chapter FOUR then Torso Tips: Nonverbals of the Upper Body and more of the text."
                .to_string(),
        ];
        let truncated = locate_heading(&texts);
        assert_eq!(truncated(0, "FOUR Torso Tips: Nonverbals of the"), Some(18));
    }

    #[test]
    fn prefers_the_longest_tail_of_a_truncated_heading_that_still_matches() {
        // Several tails match here, and they are not equally good evidence:
        // the longest one starts highest up the page, which is where the
        // heading is.
        let texts = [
            "Prologue then Mastering the Secrets of Nonverbal Communication and prose.".to_string(),
        ];
        let truncated = locate_heading(&texts);
        // Both "mastering the secrets of nonverbal" and "the secrets of
        // nonverbal" occur once. The longer run wins, so the row lands at the
        // head of the heading rather than two words into it.
        assert_eq!(
            truncated(0, "Chapter Seven Mastering the Secrets of Nonverbal"),
            Some(14)
        );
    }

    #[test]
    fn will_not_stretch_a_truncated_heading_down_to_a_phrase_the_book_contains() {
        // Two words left is a phrase, and a phrase is somewhere in every book.
        // Three is the floor, and the unique-match rule still has to be
        // satisfied.
        let texts = ["The art of war is long and the art of war is old.".to_string()];
        let find = locate_heading(&texts);
        assert_eq!(find(0, "The art of war is"), None);
        assert_eq!(find(0, "The art of war is not studied here"), None);
    }

    /* --- dominantTitle ---------------------------------------------------- */

    fn placed_row(title: &str, offset: usize, depth: i64) -> PlacedRow {
        PlacedRow {
            index: 0,
            anchor: String::new(),
            offset,
            title: title.to_string(),
            depth,
            listed: true,
        }
    }

    #[test]
    fn names_a_chapter_after_the_row_most_of_its_text_belongs_to() {
        // The opening of a book is mostly front matter folded into chapter one.
        let rows = [placed_row("Cover", 0, 0), placed_row("Chapter One", 40, 0)];
        assert_eq!(dominant_title(&rows, 1000).as_deref(), Some("Chapter One"));
    }

    #[test]
    fn names_a_chapter_after_the_shallowest_of_two_rows_for_the_same_place() {
        let rows = [placed_row("Part One", 0, 0), placed_row("Chapter One", 0, 1)];
        assert_eq!(dominant_title(&rows, 1000).as_deref(), Some("Part One"));
    }

    #[test]
    fn has_nothing_to_say_about_a_chapter_no_row_points_at() {
        assert_eq!(dominant_title(&[], 1000), None);
    }
}
