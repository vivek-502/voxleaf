//! Reading a chapter's markup into the plain blocks a narrator and a
//! translator can be given.
//!
//! Two things need the same view of a chapter. Narration reads the text it is
//! about to speak; the staleness check asks whether the text a *saved
//! translation* was made from is still the text the chapter has now. Both go
//! through this scanner, so a translation that no longer matches its section
//! is caught the same way the web app caught it — with a hash over the same
//! block texts, in the same order, joined the same way.
//!
//! Port of the web app's `epub/blocks.ts` (scanner + tidy + named entities),
//! `tts/text.ts` (`htmlToPlainText`), and `translation.ts`'s `sourceHash`.
//! The input is the app's own sanitised output, so this is a scanner rather
//! than a parser: inline markup is flattened, block boundaries are kept.

use sha2::{Digest, Sha256};

/// A block of a section's text: a heading, a paragraph, a list item. Inline
/// markup is gone by this point.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TextBlock {
    pub tag: String,
    pub text: String,
}

/// Block-level tags worth keeping. Not a closed set in the markup — the
/// scanner keeps whatever it knows — but the ones the re-renderer can put
/// back.
const BLOCK_TAGS: [&str; 19] = [
    "h1", "h2", "h3", "h4", "h5", "h6", "p", "li", "blockquote", "figcaption", "pre", "img",
    "table", "thead", "tbody", "tfoot", "tr", "th", "td",
];

/// Block-level tags that close the block being read, without opening a new
/// one.
const CLOSING_ONLY: [&str; 5] = ["div", "section", "article", "ul", "ol"];

/// The named entities the scanner resolves. Anything else — a math symbol a
/// publisher invented, say — is left as its literal `&name;` text rather than
/// spoken as "& name semicolon", because that is what the web app did.
const NAMED_ENTITIES: [(&str, &str); 29] = [
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
];

fn named_entity(name: &str) -> Option<&'static str> {
    NAMED_ENTITIES
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| *value)
}

/// Zero-width and directional marks that publishers sprinkle through prose.
fn remove_invisible(text: &str) -> String {
    text.chars()
        .filter(|c| {
            !matches!(
                c,
                '\u{00AD}'
                    | '\u{200B}'..='\u{200F}'
                    | '\u{202A}'..='\u{202E}'
                    | '\u{2060}'..='\u{2064}'
                    | '\u{2066}'..='\u{2069}'
                    | '\u{FEFF}'
            )
        })
        .collect()
}

/// Resolve an XML entity the way the web app's `unescapeXml` did, matching
/// the original's regular expression scan: only a `&` whose body is entirely
/// `#x`-hex, `#`-decimal or letters, terminated by `;`, is touched at all.
fn unescape_xml(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    let mut out = String::with_capacity(value.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '&' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let mut j = i + 1;
        if j < chars.len() && chars[j] == '#' {
            j += 1;
            let hex = j < chars.len() && matches!(chars[j], 'x' | 'X');
            if hex {
                j += 1;
            }
            let digits_start = j;
            while j < chars.len()
                && if hex {
                    chars[j].is_ascii_hexdigit()
                } else {
                    // The original's pattern allows a-f even without the x,
                    // so "#ff;" is a decimal entity whose value is NaN —
                    // which JavaScript renders as NUL. Pinned here.
                    chars[j].is_ascii_hexdigit()
                }
            {
                j += 1;
            }
            if j > digits_start && j < chars.len() && chars[j] == ';' {
                let digits: String = chars[digits_start..j].iter().collect();
                let code = if hex {
                    u32::from_str_radix(&digits, 16).ok()
                } else {
                    // `String.fromCharCode` truncates to 16 bits, so
                    // `&#999999;` is U+423F and not an error — pinned here
                    // because a big decimal entity is exactly what a
                    // conversion tool emits.
                    digits.parse::<u64>().ok().map(|n| (n & 0xFFFF) as u32)
                };
                match code {
                    // A lone surrogate cannot live in a Rust string; the
                    // original handed one out silently.
                    Some(code) => match char::from_u32(code) {
                        Some(c) => out.push(c),
                        None => out.push('\u{FFFD}'),
                    },
                    // Unparseable digits gave NUL in the original; an
                    // out-of-range hex code point threw. Keeping the chapter
                    // readable is the one divergence, chosen so a malformed
                    // file cannot kill a narration run.
                    None if hex => out.push('\u{FFFD}'),
                    None => out.push('\0'),
                }
                i = j + 1;
                continue;
            }
            // Not a numeric entity after all: fall through, the `#` is text.
            out.push('&');
            i += 1;
            continue;
        }
        let digits_start = j;
        while j < chars.len() && chars[j].is_ascii_alphabetic() {
            j += 1;
        }
        if j > digits_start && j < chars.len() && chars[j] == ';' {
            let name: String = chars[digits_start..j].iter().collect();
            match named_entity(&name) {
                Some(resolved) => out.push_str(resolved),
                // Unknown name: the original kept the whole match, braces
                // and all.
                None => {
                    out.push('&');
                    out.push_str(&name);
                    out.push(';');
                }
            }
            i = j + 1;
            continue;
        }
        out.push('&');
        i += 1;
    }
    out
}

/// Tidy one piece of block text without touching its words: resolve entities,
/// drop the invisible marks, normalise line endings, collapse space runs but
/// keep line breaks (a stanza is not one line), and trim.
fn tidy(raw: &str) -> String {
    let text = remove_invisible(&unescape_xml(raw));
    let text = text.replace("\r\n", "\n").replace('\r', "\n");

    // Space runs other than newlines collapse to a single space.
    let mut collapsed: Vec<char> = Vec::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_whitespace() && chars[i] != '\n' {
            collapsed.push(' ');
            while i < chars.len() && chars[i].is_whitespace() && chars[i] != '\n' {
                i += 1;
            }
        } else {
            collapsed.push(chars[i]);
            i += 1;
        }
    }

    // Spaces around line breaks are dropped.
    let mut trimmed_lines: Vec<char> = Vec::with_capacity(collapsed.len());
    for &c in &collapsed {
        if c == '\n' {
            while matches!(trimmed_lines.last(), Some(' ')) {
                trimmed_lines.pop();
            }
            trimmed_lines.push('\n');
        } else if c == ' ' && trimmed_lines.last() == Some(&'\n') {
            // Skip: the space directly follows a line break.
        } else {
            trimmed_lines.push(c);
        }
    }

    // Three or more newlines collapse to a blank line.
    let mut out = String::with_capacity(trimmed_lines.len());
    let mut newlines = 0;
    for c in trimmed_lines {
        if c == '\n' {
            newlines += 1;
            if newlines <= 2 {
                out.push(c);
            }
        } else {
            newlines = 0;
            out.push(c);
        }
    }

    out.trim().to_string()
}

/// Remove HTML comments; an unterminated one is kept as text, because the
/// original's non-greedy pattern simply never matched it.
fn strip_comments(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(start) = rest.find("<!--") {
        out.push_str(&rest[..start]);
        match rest[start + 4..].find("-->") {
            Some(close) => rest = &rest[start + 4 + close + 3..],
            None => {
                out.push_str(&rest[start..]);
                return out;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The tag's name: an optional `/`, optional whitespace, then letters and
/// digits — lower-cased. Empty when there is no such name.
fn tag_name(tag: &str) -> String {
    let rest = tag.strip_prefix('/').unwrap_or(tag);
    let rest = rest.trim_start();
    let mut name = String::new();
    for c in rest.chars() {
        if c.is_ascii_alphabetic() || (!name.is_empty() && c.is_ascii_alphanumeric()) {
            name.push(c);
        } else {
            break;
        }
    }
    name.make_ascii_lowercase();
    name
}

/// Split a section's markup into its blocks of text.
///
/// Inline markup is dropped, so `<em>word</em>` and `word` are the same text;
/// a `<br>` inside a block becomes a line break; text that sits outside any
/// block tag is kept as a paragraph rather than dropped, because plenty of
/// books write their chapters as bare text in a `<div>`.
pub(crate) fn extract_text_blocks(html: &str) -> Vec<TextBlock> {
    let markup = strip_comments(html);

    let mut blocks: Vec<TextBlock> = Vec::new();
    let mut current: Option<String> = None;
    let mut buffer = String::new();

    // End the block in progress, keeping any stray text as a paragraph.
    macro_rules! flush {
        () => {
            let text = tidy(&buffer);
            if !text.is_empty() {
                blocks.push(TextBlock {
                    tag: current.clone().unwrap_or_else(|| "p".to_string()),
                    text,
                });
            }
            buffer.clear();
        };
    }
    macro_rules! close {
        () => {
            flush!();
        };
    }

    let markup_chars: Vec<char> = markup.chars().collect();
    let mut at = 0;

    while at < markup_chars.len() {
        // Find the next tag, keeping any text before it.
        let Some(open) = markup_chars[at..]
            .iter()
            .position(|c| *c == '<')
            .map(|offset| at + offset)
        else {
            buffer.extend(&markup_chars[at..]);
            break;
        };
        if open > at {
            buffer.extend(&markup_chars[at..open]);
        }

        let Some(end) = markup_chars[open + 1..]
            .iter()
            .position(|c| *c == '>')
            .map(|offset| open + 1 + offset)
        else {
            // An unterminated tag is text, not markup.
            buffer.extend(&markup_chars[open..]);
            break;
        };

        let tag: String = markup_chars[open + 1..end].iter().collect();
        let name = tag_name(&tag);

        // Doctype, CDATA, and other bang-tags are not text.
        if tag.starts_with('!') {
            at = end + 1;
            continue;
        }

        if tag.starts_with('/') {
            // A closing block tag ends the block being read, whoever it
            // belongs to; a closing-only tag (`div`, `ul`, …) does too.
            if current.as_deref() == Some(name.as_str())
                || CLOSING_ONLY.contains(&name.as_str())
                || BLOCK_TAGS.contains(&name.as_str())
            {
                close!();
                current = None;
            }
            at = end + 1;
            continue;
        }

        // The original also tested for a self-closing inline tag here; both
        // of its paths stepped past the tag identically, so the test is gone.

        if name == "br" {
            if current.is_some() {
                buffer.push('\n');
            }
            at = end + 1;
            continue;
        }

        if name == "img" {
            close!();
            current = None;
            blocks.push(TextBlock {
                tag: "img".to_string(),
                text: String::new(),
            });
            at = end + 1;
            continue;
        }

        if BLOCK_TAGS.contains(&name.as_str()) {
            close!();
            current = Some(name);
            at = end + 1;
            continue;
        }

        if CLOSING_ONLY.contains(&name.as_str()) {
            close!();
            current = None;
            at = end + 1;
            continue;
        }

        // An opening inline tag (`<em>`) is markup the scanner does not
        // track, so nothing to do but step past it.
        at = end + 1;
    }

    close!();
    blocks
}

/// A chapter's text as the narrator wants it: every block's text, joined by a
/// blank line. Images contribute nothing.
pub(crate) fn html_to_plain_text(html: &str) -> String {
    let blocks = extract_text_blocks(html);
    let parts: Vec<&str> = blocks
        .iter()
        .filter(|block| block.tag != "img")
        .filter(|block| !block.text.is_empty())
        .map(|block| block.text.as_str())
        .collect();
    parts.join("\n\n")
}

/// The hash that decides whether a saved translation still describes the
/// chapter: SHA-256 over the block texts joined by NUL, hex, first 32
/// characters. Byte-identical to the web app's `sourceHash`, so a library
/// translated under the web app is not seen as stale after the move to Rust.
pub(crate) fn source_hash(html: &str) -> String {
    let texts: Vec<String> = extract_text_blocks(html)
        .into_iter()
        .map(|block| block.text)
        .collect();
    let digest = Sha256::digest(texts.join("\u{0000}").as_bytes());
    hex::encode(digest)[..32].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_markup_is_flattened_away() {
        let blocks = extract_text_blocks("<p>Plain <em>emphasised</em> words.</p>");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].tag, "p");
        assert_eq!(blocks[0].text, "Plain emphasised words.");
    }

    #[test]
    fn block_tags_name_their_blocks() {
        let blocks = extract_text_blocks("<h1>Title</h1><p>Body.</p><li>Item</li>");
        let tags: Vec<&str> = blocks.iter().map(|b| b.tag.as_str()).collect();
        assert_eq!(tags, vec!["h1", "p", "li"]);
        assert_eq!(blocks[0].text, "Title");
    }

    #[test]
    fn a_br_becomes_a_line_break_and_stray_text_becomes_a_paragraph() {
        let blocks = extract_text_blocks("<p>one<br>two</p>");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].tag, "p");
        assert_eq!(blocks[0].text, "one\ntwo");

        // Bare text in a div: plenty of books write chapters this way. The
        // `div` closes nothing in particular, so the stray text is kept as a
        // paragraph of its own.
        let blocks = extract_text_blocks("<div>stray text</div>");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].tag, "p");
        assert_eq!(blocks[0].text, "stray text");
    }

    #[test]
    fn images_are_kept_as_blocks_but_speak_nothing() {
        let blocks = extract_text_blocks("<p>Look.</p><img src='cover.jpg'/>");
        assert_eq!(blocks.last().unwrap().tag, "img");
        assert_eq!(blocks.last().unwrap().text, "");
        assert_eq!(html_to_plain_text("<p>Look.</p><img src='c.jpg'/>"), "Look.");
    }

    #[test]
    fn comments_and_doctypes_are_not_text() {
        let blocks = extract_text_blocks("<!doctype html><!-- note --><p>Visible.</p>");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].text, "Visible.");
    }

    #[test]
    fn tidy_resolves_entities_and_drops_invisible_marks() {
        let blocks = extract_text_blocks("<p>a &amp; b&#8212;c&#x2014;d&nbsp;e&#999999;x</p>");
        // `&amp;` resolves; the decimal and hex em-dashes resolve; nbsp
        // becomes a space. `String.fromCharCode` truncates to 16 bits, so the
        // out-of-range decimal is U+423F and not an error.
        assert_eq!(blocks[0].text, "a & b—c—d e\u{423F}x");
        let blocks = extract_text_blocks("<p>kept&notanentity; here</p>");
        assert_eq!(blocks[0].text, "kept&notanentity; here");
    }

    #[test]
    fn tidy_collapses_runs_but_keeps_stanza_line_breaks() {
        let blocks = extract_text_blocks("<p>  a   b \r\n\r\n\r\n c\t\td  </p>");
        // `/ *\n */g` eats the spaces hugging each break, and three or more
        // newlines collapse to a blank line.
        assert_eq!(blocks[0].text, "a b\n\nc d");
    }

    #[test]
    fn plain_text_joins_blocks_with_a_blank_line() {
        let html = "<h1>T</h1><p>one</p><p>two</p>";
        assert_eq!(html_to_plain_text(html), "T\n\none\n\ntwo");
    }

    #[test]
    fn the_source_hash_pins_against_the_web_app() {
        // Computed with the web app's own algorithm: sha256 of the block
        // texts joined by NUL, hex, first 32 characters.
        assert_eq!(source_hash("<p>Hello &amp; bye</p>"), "32be4a8f831534018a670bb6a6a7e72e");
        assert_eq!(
            source_hash("<p>Hello &amp; bye</p><p>Second para</p>"),
            "863d6d4ce3d83ac441ecf5c4c3057922"
        );
    }

    #[test]
    fn editing_a_block_changes_the_hash() {
        let before = source_hash("<p>Word.</p>");
        let after = source_hash("<p>Word!</p>");
        assert_ne!(before, after);
    }
}
