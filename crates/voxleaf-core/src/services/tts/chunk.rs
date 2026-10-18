//! Splitting chapter text into pieces Edge TTS can be asked for.
//!
//! Edge TTS streams a synthesis request and answers with one MP3, so a very
//! long chapter is synthesised in one pass — but a pass that takes minutes,
//! during which a single connection is held open. Splitting the text and
//! synthesising each chunk separately gives two things the one-shot call
//! cannot: progress the UI can show, and a failure that only costs one chunk
//! instead of the chapter.
//!
//! Port of the web app's `chunk.ts`, character for character. Chunks break on
//! paragraph boundaries, then sentences, then words: cutting mid-sentence is
//! audible as a clipped word, so it is the last resort.
//!
//! The original measures in UTF-16 code units; this port measures in
//! `char`s. The two agree for every BMP script (which is every script
//! VoxLeaf can translate into), and the ported tests pin that behaviour.

/// Characters per request. Comfortably under Edge's own limit, and short
/// enough that one chunk is a couple of seconds of network time rather than
/// minutes.
pub(crate) const MAX_CHUNK_CHARS: usize = 2000;

/// Above this, a chapter is too long to narrate in one sitting.
#[allow(dead_code)]
pub(crate) const MAX_TOTAL_CHARS: usize = 400_000;

/// Rightmost occurrence of `needle` in `hay`, as a code-unit analogue of
/// JavaScript's `lastIndexOf`.
fn rfind_sub(hay: &[char], needle: &[char]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    (0..=hay.len() - needle.len())
        .rev()
        .find(|&start| &hay[start..start + needle.len()] == needle)
}

fn rfind_char(hay: &[char], needle: char) -> Option<usize> {
    hay.iter().rposition(|c| *c == needle)
}

/// `str::trim` over a char buffer, returning a fresh `Vec<char>`.
fn trim_chars(chars: &[char]) -> Vec<char> {
    let start = chars
        .iter()
        .position(|c| !c.is_whitespace())
        .unwrap_or(chars.len());
    let end = chars
        .iter()
        .rposition(|c| !c.is_whitespace())
        .map_or(start, |i| i + 1);
    chars[start..end].to_vec()
}

fn to_string(chars: &[char]) -> String {
    chars.iter().collect()
}

/// Break text into synthesizable chunks, each at most `limit` characters.
///
/// Paragraph breaks are preferred, then sentence ends, then a plain space.
/// Runs of whitespace collapse so a chunk's length is its real spoken length.
pub(crate) fn chunk_text(text: &str, limit: usize) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();

    // Runs of whitespace other than newlines collapse to a single space.
    let mut collapsed: Vec<char> = Vec::with_capacity(chars.len());
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
    // Three or more newlines collapse to a paragraph break.
    let mut paragraphed: Vec<char> = Vec::with_capacity(collapsed.len());
    let mut newlines = 0;
    for &c in &collapsed {
        if c == '\n' {
            newlines += 1;
            if newlines <= 2 {
                paragraphed.push(c);
            }
        } else {
            newlines = 0;
            paragraphed.push(c);
        }
    }

    let mut rest = trim_chars(&paragraphed);
    if rest.is_empty() {
        return Vec::new();
    }

    let threshold = limit as f64 * 0.4;
    let mut chunks: Vec<String> = Vec::new();

    while rest.len() > limit {
        let window = &rest[..limit];

        // The last `limit` characters are the tail of the window we may cut
        // at. Paragraph break first.
        let mut cut = rfind_sub(window, &['\n', '\n']);
        let usable = |cut: Option<usize>| cut.map_or(true, |c| (c as f64) < threshold);

        if usable(cut) {
            cut = rfind_char(window, '\n');
        }
        if usable(cut) {
            // The last position of a sentence end: `. `-style terminators are
            // two characters (so the space is guaranteed to follow), the
            // CJK full stops are one. `lastIndexOf`'s -1-for-miss becomes a
            // negative maximum term.
            let sentence = [
                rfind_sub(window, &['.', ' ']),
                rfind_sub(window, &['!', ' ']),
                rfind_sub(window, &['?', ' ']),
                rfind_char(window, '。'),
                rfind_char(window, '！'),
                rfind_char(window, '？'),
            ]
            .into_iter()
            .map(|found| found.map_or(-1isize, |found| found as isize))
            .max()
            .unwrap_or(-1);
            if (sentence as f64) > threshold {
                cut = Some((sentence + 1) as usize);
            }
        }
        if usable(cut) {
            cut = rfind_char(window, ' ');
        }

        // Nothing usable to break on (one very long word): take what fits.
        let cut = match cut {
            Some(cut) if cut > 0 => cut,
            _ => limit,
        };

        let chunk = to_string(&trim_chars(&rest[..cut]));
        if !chunk.is_empty() {
            chunks.push(chunk);
        }
        rest = trim_chars(&rest[cut..]);
    }

    if !rest.is_empty() {
        chunks.push(to_string(&rest));
    }
    chunks.retain(|chunk| !chunk.is_empty());
    chunks
}

/// The chunker's default limit, for callers that do not choose one.
pub(crate) fn chunk(text: &str) -> Vec<String> {
    chunk_text(text, MAX_CHUNK_CHARS)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every chunk rejoined must be the original text — a dropped word is a
    /// silence.
    fn rejoined(chunks: &[String]) -> String {
        chunks.join(" ")
    }

    #[test]
    fn returns_nothing_for_empty_or_whitespace_only_text() {
        assert!(chunk_text("", MAX_CHUNK_CHARS).is_empty());
        assert!(chunk_text("   \n\n  ", MAX_CHUNK_CHARS).is_empty());
    }

    #[test]
    fn leaves_short_text_as_a_single_chunk() {
        assert_eq!(
            chunk_text("A short chapter.", MAX_CHUNK_CHARS),
            vec!["A short chapter."]
        );
    }

    #[test]
    fn splits_long_text_and_loses_nothing() {
        let paragraph = "The quick brown fox jumps over the lazy dog. ".repeat(100);
        let chunks = chunk_text(&paragraph, MAX_CHUNK_CHARS);
        assert!(chunks.len() > 1);
        assert_eq!(rejoined(&chunks), paragraph.trim());
    }

    #[test]
    fn keeps_every_chunk_within_the_limit() {
        let text = "word ".repeat(5000);
        for chunk in chunk_text(&text, MAX_CHUNK_CHARS) {
            assert!(chunk.chars().count() <= MAX_CHUNK_CHARS);
        }
    }

    #[test]
    fn prefers_breaking_at_a_paragraph_rather_than_mid_sentence() {
        let first = "a".repeat(900);
        let second = "b".repeat(900);
        let text = format!("{first}\n\n{second}");
        assert_eq!(chunk_text(&text, 1000), vec![first, second]);
    }

    #[test]
    fn falls_back_to_a_sentence_boundary_when_there_are_no_paragraphs() {
        let text = format!(
            "{}. {}. {}.",
            "x".repeat(500),
            "y".repeat(500),
            "z".repeat(500)
        );
        let chunks = chunk_text(&text, 600);
        assert!(chunks.len() > 1);
        for chunk in &chunks {
            assert!(chunk.chars().count() <= 600);
        }
        assert_eq!(rejoined(&chunks), text);
    }

    #[test]
    fn breaks_a_single_unbroken_word_rather_than_looping_forever() {
        let chunks = chunk_text(&"z".repeat(5000), 1000);
        assert_eq!(chunks.len(), 5);
        // Hard cuts, so the chunks abut with no separator — no word is
        // dropped.
        assert_eq!(chunks.concat(), "z".repeat(5000));
    }

    #[test]
    fn never_emits_an_empty_chunk() {
        let text = format!("{}\n\n\n\n{}", "a".repeat(1900), "b".repeat(1900));
        for chunk in chunk_text(&text, 2000) {
            assert!(!chunk.is_empty());
        }
    }

    #[test]
    fn collapses_whitespace_so_chunk_length_reflects_spoken_length() {
        let text = format!("{}     {}", "a".repeat(900), "b".repeat(900));
        let chunks = chunk_text(&text, 2000);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], format!("{} {}", "a".repeat(900), "b".repeat(900)));
    }

    #[test]
    fn cjk_full_stops_are_sentence_boundaries_too() {
        // 50-character "sentences" of ideographic full stops: the cut lands
        // after 。 even though there is no space anywhere.
        let text = "字".repeat(300);
        let text = format!("{text}。{}", "句".repeat(300));
        let limit = 350;
        let chunks = chunk_text(&text, limit);
        assert!(chunks.len() > 1);
        for chunk in &chunks {
            assert!(chunk.chars().count() <= limit);
        }
    }

    #[test]
    fn the_default_limit_is_the_documented_one() {
        assert_eq!(MAX_CHUNK_CHARS, 2000);
        assert_eq!(MAX_TOTAL_CHARS, 400_000);
        let chunks = chunk("word ".repeat(5000).trim_end());
        assert!(chunks.iter().all(|c| c.chars().count() <= 2000));
    }
}
