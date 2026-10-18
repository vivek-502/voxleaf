//! Machine translation of a section's text, spoken to Google's public
//! translate endpoint — the one every browser's translate widget talks to.
//!
//! It needs no account and no key, which is the whole reason it is here: a
//! self-hosted reader should not have to be handed an API key to look a word up.
//!
//! What that costs is reliability, and the module is written around it:
//!
//!  - it answers with an HTTP 429 for many shared and residential IP addresses,
//!    which is not an error the app can fix, so it is reported as exactly what it
//!    is rather than retried forever;
//!  - it honours only the *first* `q` of a request, so a batch of paragraphs
//!    travels as a single string with a marker between them and is split back out
//!    of the answer afterwards;
//!  - the text travels in the query string, so a section is sent in batches
//!    budgeted by the encoded URL length, and an over-long paragraph is split at a
//!    sentence boundary rather than sent whole;
//!  - the response is a nested array of sentence fragments with no schema
//!    version, so it is read defensively and every batch is checked to have come
//!    back with the same number of paragraphs it was sent — a batch whose markers
//!    did not survive is retried in halves rather than accepted;
//!  - one bad batch must not leave a half-translated chapter behind, so the rows
//!    are written only once every batch has succeeded.
//!
//! `TRANSLATION_BASE_URL` points this at something else — a mirror, a caching
//! proxy, or a stand-in of your own — without touching the reader.

use std::fmt::Write as _;

use rand::RngCore;

use crate::config::TranslationConfig;

/// Named in saved translations so a row is never mistaken for another provider's.
pub const TRANSLATION_PROVIDER: &str = "google";

/// Request length, counted the way it is actually sent.
///
/// The text goes in the query string, and a URL past a few thousand characters
/// is where proxies truncate and servers refuse. Budgeting in *encoded*
/// characters rather than plain ones is what makes the number mean something:
/// every space, newline and accented letter costs more in the URL than in the
/// paragraph, and a chapter of Spanish or Turkish text costs three times what
/// its length suggests.
pub const BATCH_CHAR_BUDGET: usize = 3000;

/// Paragraphs (or paragraph pieces) per request, alongside the length budget.
pub const MAX_ITEMS_PER_BATCH: usize = 12;

/// Longest single piece of text sent in one request.
///
/// A few books write paragraphs of several thousand characters. One that long is
/// a long URL, so it goes as sentence-bounded pieces and is put back together
/// afterwards — split on a sentence boundary, because a request cut mid-sentence
/// comes back as a broken sentence.
pub const MAX_PIECE_CHARS: usize = 4000;

/// Hex characters of randomness in a request's marker.
const TOKEN_CHARS: usize = 6;

/// Pause between requests.
///
/// This endpoint limits by request, and what it answers with when a limit is
/// exceeded is a refusal for the whole address for a while — not a soft throttle.
const BATCH_PAUSE_MS: u64 = 300;

/// How many times one failed batch is tried before the whole job gives up.
const BATCH_ATTEMPTS: usize = 3;

/// What went wrong, which decides how the app tries to recover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureReason {
    Refused,
    Unreachable,
    Server,
    Shape,
}

/// A provider failure, carrying the words the reader should see.
#[derive(Debug, Clone)]
pub struct TranslationError {
    pub message: String,
    pub status: Option<u16>,
    /// Whether trying again shortly is worth the reader's time.
    pub retryable: bool,
    pub reason: FailureReason,
}

impl TranslationError {
    fn shape(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            status: None,
            retryable: false,
            reason: FailureReason::Shape,
        }
    }
}

impl std::fmt::Display for TranslationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for TranslationError {}

/// Failures worth repeating in place.
///
/// A refusal (429) is about the address rather than the request, and a batch
/// whose markers did not come back is not going to come back differently on a
/// second identical attempt — the answer to both is to ask for less at a time,
/// not to ask again. A dropped connection and a server error, on the other hand,
/// are the two that a moment's patience fixes.
fn retry_in_place(reason: FailureReason) -> bool {
    matches!(reason, FailureReason::Unreachable | FailureReason::Server)
}

/// A fresh token per request: `translate_batch` is called once per batch.
fn marker_token() -> String {
    let mut bytes = [0u8; TOKEN_CHARS / 2];
    rand::rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// The marker that keeps paragraphs apart inside one request.
///
/// The endpoint reads the first `q` of a request and ignores the rest, which is
/// why a batch is one string rather than several parameters. Punctuation, a
/// pilcrow and a number are exactly the kind of thing a translator passes
/// through untouched, so each paragraph after the first arrives prefixed with
/// one, and the answer is cut back into paragraphs on them.
///
/// Each request numbers its markers with a token of its own, because the text
/// being translated is a book's, and a book about brackets or footnotes can
/// contain something that looks like a marker. A token that changes every
/// request cannot collide with the text that request is carrying.
///
/// The marker goes in front of a paragraph rather than after one because a
/// marker at the very end of the request is trimmed off before the answer is
/// built, and the answer would then be one paragraph short of what was asked.
fn marker(token: &str, index: usize) -> String {
    format!("\n⟦¶{token}-{index}⟧\n")
}

/// Cut an answer back into its paragraphs.
///
/// Only the exact markers that went out are looked for, and the newlines around
/// them are not part of the search: a service is free to answer `\n` as a space,
/// and the whitespace is tidied away afterwards anyway. A marker that did not
/// come back means the answer is not the answer to this request, and `None`
/// says so rather than guessing where the paragraphs are.
fn split_on_markers(answer: &str, cores: &[String]) -> Option<Vec<String>> {
    let mut parts = Vec::with_capacity(cores.len() + 1);
    let mut rest = answer;
    for core in cores {
        let at = rest.find(core.as_str())?;
        parts.push(rest[..at].to_string());
        rest = &rest[at + core.len()..];
    }
    parts.push(rest.to_string());
    Some(parts)
}

/// `encodeURIComponent`, the JavaScript one, character for character.
///
/// Used only to measure what a request will cost: the budget is counted in
/// encoded characters, and the keep-set differs from a form encoder
/// (`!*'()` survive, `~` does not).
fn encode_component(text: &str) -> String {
    percent_encode(text, |c| {
        c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '!' | '~' | '*' | '\'' | '(' | ')')
    })
}

/// `URLSearchParams`'s `application/x-www-form-urlencoded` serialization:
/// space becomes `+`, `* - . _` survive, everything else is percent-encoded by
/// UTF-8 byte. This is the wire encoding the endpoint receives.
fn form_encode(text: &str) -> String {
    let encoded = percent_encode(text, |c| {
        c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '*')
    });
    encoded.replace("%20", "+")
}

fn percent_encode(text: &str, keep: impl Fn(char) -> bool) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if keep(c) {
            out.push(c);
        } else {
            let mut buffer = [0u8; 4];
            for byte in c.encode_utf8(&mut buffer).as_bytes() {
                let _ = write!(out, "%{byte:02X}");
            }
        }
    }
    out
}

/// What one piece costs in the request URL.
///
/// The encoded text, plus a marker for its neighbours. Charging the first piece
/// of a batch for a marker it will not have is deliberate: the budget is a
/// ceiling, and being one marker over on the first piece of every batch is not
/// worth the bookkeeping of not.
fn separator_url_cost() -> usize {
    static COST: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *COST.get_or_init(|| encode_component(marker("000000", 0).as_str()).len())
}

fn url_length(text: &str) -> usize {
    encode_component(text).len() + separator_url_cost()
}

/// Tidy one piece of translated text the way the source text was tidied.
///
/// The extractor collapses runs of spaces before a paragraph is sent, so a
/// double space in an answer is not a translator's punctuation — it is
/// whitespace that crept in at a fragment boundary, and left alone it shows up
/// as a hole in the line under justified text. Line breaks are kept: a stanza
/// is not one line.
fn tidy_translated(text: &str) -> String {
    // Horizontal whitespace becomes a plain space; line breaks survive.
    let mut collapsed = String::with_capacity(text.len());
    for c in text.chars() {
        if c == '\n' {
            collapsed.push('\n');
        } else if c.is_whitespace() {
            if !collapsed.ends_with(' ') {
                collapsed.push(' ');
            }
        } else {
            collapsed.push(c);
        }
    }

    // Spaces around a line break are not the translator's doing either.
    let mut trimmed = String::with_capacity(collapsed.len());
    for c in collapsed.chars() {
        if c == '\n' {
            while trimmed.ends_with(' ') {
                trimmed.pop();
            }
            trimmed.push('\n');
        } else if c == ' ' && !trimmed.ends_with('\n') {
            trimmed.push(' ');
        } else if c != ' ' {
            trimmed.push(c);
        }
    }

    trimmed
        .trim_matches(|c: char| c == ' ' || c == '\n')
        .to_string()
}

/// Cut one over-long block into pieces that each end at a sentence.
///
/// Falls back to a space, and then to a hard cut, because the alternative is
/// not sending the paragraph at all — and a paragraph the reader can see but
/// never has translated is worse than one translated slightly badly.
pub fn split_long_block(text: &str) -> Vec<String> {
    if text.chars().count() <= MAX_PIECE_CHARS {
        return vec![text.to_string()];
    }

    let chars: Vec<char> = text.chars().collect();
    let mut pieces = Vec::new();
    let mut start = 0;

    while chars.len() - start > MAX_PIECE_CHARS {
        let window: String = chars[start..start + MAX_PIECE_CHARS].iter().collect();
        let sentence = [". ", "! ", "? "]
            .iter()
            .filter_map(|pattern| window.rfind(pattern))
            .max();
        let space = window.rfind(' ');
        let cut = match (sentence, space) {
            (Some(at), _) => at + 1,
            (None, Some(at)) if at > 0 => at + 1,
            _ => MAX_PIECE_CHARS,
        };
        pieces.push(chars[start..start + cut].iter().collect::<String>());
        start += cut;
    }
    pieces.push(chars[start..].iter().collect());

    pieces
}

/// A piece of text to send, and the block it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Piece {
    pub block_index: usize,
    pub text: String,
}

/// Group a section into requests.
///
/// Both budgets are respected, and a piece is never shared between two requests
/// even when it would fit: one request per group of whole pieces means every
/// answer can be checked against what was asked for.
pub fn plan_requests(texts: &[String]) -> Vec<Vec<Piece>> {
    let mut pieces: Vec<Piece> = Vec::new();
    for (block_index, text) in texts.iter().enumerate() {
        for part in split_long_block(text) {
            pieces.push(Piece {
                block_index,
                text: part,
            });
        }
    }

    let mut batches: Vec<Vec<Piece>> = Vec::new();
    let mut batch: Vec<Piece> = Vec::new();
    let mut size = 0usize;

    for piece in pieces {
        let length = url_length(&piece.text);
        if !batch.is_empty() && (size + length > BATCH_CHAR_BUDGET || batch.len() >= MAX_ITEMS_PER_BATCH) {
            batches.push(std::mem::take(&mut batch));
            size = 0;
        }
        size += length;
        batch.push(piece);
    }

    if !batch.is_empty() {
        batches.push(batch);
    }
    batches
}

/// Say plainly that the endpoint refused this server.
///
/// A 429 here is nearly always the network and not the request: shared
/// datacentre addresses and residential NAT ranges are refused as a matter of
/// course. That is worth stating, because "try again" would otherwise be advice
/// that does not work for anyone stuck behind the same address.
fn refused(status: u16) -> TranslationError {
    if status == 429 {
        return TranslationError {
            message: "The translation service refused this server (HTTP 429). It declines requests \
                      from many shared and residential IP addresses, so this is usually about the \
                      connection rather than the request. A few minutes may clear it; running the \
                      app on a different network, or setting TRANSLATION_BASE_URL to a mirror, \
                      will."
                .to_string(),
            status: Some(429),
            retryable: true,
            reason: FailureReason::Refused,
        };
    }
    TranslationError {
        message: format!("The translation service answered with HTTP {status}."),
        status: Some(status),
        retryable: status >= 500,
        reason: if status >= 500 {
            FailureReason::Server
        } else {
            FailureReason::Shape
        },
    }
}

/// Read one batch's answer, refusing anything that is not the answer to this
/// exact request.
///
/// The answer is `[["translated,source",...], ...]`: one entry per sentence the
/// service chose to split on, with no schema version and no promise about how
/// many there will be. The fragments are joined with no separator, because the
/// service split them at sentence boundaries and rejoining recovers the text;
/// the batch is then split back into paragraphs on the markers it went out with.
///
/// A batch that comes back as one paragraph, or as the wrong number, is refused
/// rather than guessed at: reading it as one paragraph would put a whole
/// chapter in the wrong order, which is worse than an error the reader can
/// retry.
fn parse_batch_body(
    body: &serde_json::Value,
    cores: &[String],
    batch_len: usize,
) -> Result<Vec<String>, TranslationError> {
    let outer = body
        .as_array()
        .and_then(|rows| rows.first())
        .and_then(|row| row.as_array())
        .ok_or_else(|| {
            TranslationError::shape("The translation service returned a response that was not a translation.")
        })?;

    let answer: String = outer
        .iter()
        .map(|fragment| {
            fragment
                .as_array()
                .and_then(|parts| parts.first())
                .and_then(|part| part.as_str())
                .unwrap_or("")
        })
        .collect();

    let paragraphs = split_on_markers(&answer, cores);
    let count = paragraphs.as_ref().map(|parts| parts.len()).unwrap_or(0);
    if paragraphs.as_ref().map(|parts| parts.len()) != Some(batch_len) {
        return Err(TranslationError::shape(format!(
            "The translation service answered with {count} of {batch_len} paragraphs."
        )));
    }
    Ok(paragraphs.unwrap_or_default())
}

/// One request, carrying a whole batch of paragraphs as a single `q`.
async fn translate_batch(
    client: &reqwest::Client,
    cfg: &TranslationConfig,
    batch: &[Piece],
    target: &str,
) -> Result<Vec<String>, TranslationError> {
    let token = marker_token();
    let cores: Vec<String> = batch[1..]
        .iter()
        .enumerate()
        .map(|(index, _)| marker(&token, index).trim().to_string())
        .collect();

    let mut text = String::new();
    for (index, piece) in batch.iter().enumerate() {
        if index > 0 {
            text.push_str(&marker(&token, index - 1));
        }
        text.push_str(&piece.text);
    }

    let base = cfg.base_url.trim_end_matches('/');
    let url = format!(
        "{base}?client=gtx&sl=auto&tl={}&dt=t&q={}",
        form_encode(target),
        form_encode(&text),
    );
    let response = client
        .get(url)
        .header(reqwest::header::USER_AGENT, &cfg.user_agent)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(|error| TranslationError {
            message: format!("Could not reach the translation service: {error}"),
            status: None,
            retryable: true,
            reason: FailureReason::Unreachable,
        })?;

    if !response.status().is_success() {
        return Err(refused(response.status().as_u16()));
    }

    let body: serde_json::Value = response.json().await.map_err(|_| {
        TranslationError::shape("The translation service returned a response that was not a translation.")
    })?;

    parse_batch_body(&body, &cores, batch.len())
}

/// A batch, asked for again in halves if its markers do not come back.
///
/// Asking for less is the one thing that reliably changes the answer: with a
/// single paragraph there are no markers to lose, so the worst case is a slower
/// chapter rather than a wrong one. It costs one extra request per level and
/// only happens when the service has already given a bad answer.
async fn translate_batch_resilient(
    client: &reqwest::Client,
    cfg: &TranslationConfig,
    batch: &[Piece],
    target: &str,
) -> Result<Vec<String>, TranslationError> {
    match translate_batch(client, cfg, batch, target).await {
        Ok(paragraphs) => Ok(paragraphs),
        Err(error) => {
            if error.reason != FailureReason::Shape || batch.len() == 1 {
                return Err(error);
            }
            let middle = batch.len().div_ceil(2);
            // The two halves are awaited sequentially — the service is not
            // asked for both at once — so the recursion depth is bounded by
            // log2 of the batch size and each level is its own boxed future.
            let head = Box::pin(translate_batch_resilient(client, cfg, &batch[..middle], target)).await?;
            let tail = Box::pin(translate_batch_resilient(client, cfg, &batch[middle..], target)).await?;
            let mut paragraphs = head;
            paragraphs.extend(tail);
            Ok(paragraphs)
        }
    }
}

async fn wait(ms: u64) {
    tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
}

/// Translate a section's blocks, in order, into one string per block.
///
/// `on_progress` is called after each batch, in characters, so a long section
/// can report how far along it is.
pub async fn translate_blocks(
    texts: &[String],
    target: &str,
    client: &reqwest::Client,
    cfg: &TranslationConfig,
    mut on_progress: impl FnMut(i64, i64),
) -> Result<Vec<String>, TranslationError> {
    let total: i64 = texts.iter().map(|text| text.chars().count() as i64).sum();
    let batches = plan_requests(texts);

    let mut out: Vec<String> = texts.iter().map(|_| String::new()).collect();
    let mut done: i64 = 0;

    for (index, batch) in batches.iter().enumerate() {
        let mut failure: Option<TranslationError> = None;
        let mut translated: Option<Vec<String>> = None;

        let mut attempt = 1;
        while attempt <= BATCH_ATTEMPTS && translated.is_none() {
            match translate_batch_resilient(client, cfg, batch, target).await {
                Ok(paragraphs) => translated = Some(paragraphs),
                Err(error) => {
                    let worth_repeating = retry_in_place(error.reason) && attempt < BATCH_ATTEMPTS;
                    failure = Some(error);
                    if !worth_repeating {
                        break;
                    }
                    wait(600 * attempt as u64).await;
                }
            }
            attempt += 1;
        }

        let translated = translated.ok_or_else(|| {
            failure.unwrap_or_else(|| TranslationError::shape("The translation service gave no answer."))
        })?;

        for (position, piece) in batch.iter().enumerate() {
            let text = tidy_translated(translated.get(position).map(String::as_str).unwrap_or(""));
            let slot = &mut out[piece.block_index];
            if slot.is_empty() {
                *slot = text;
            } else {
                slot.push(' ');
                slot.push_str(&text);
            }
            done += piece.text.chars().count() as i64;
        }
        on_progress(done, total);

        if index + 1 < batches.len() {
            wait(BATCH_PAUSE_MS).await;
        }
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separator_cost_matches_a_real_marker() {
        // `\n⟦¶000000-0⟧\n`: three escapes for each newline, nine for each
        // bracket, six for the pilcrow, and the literal token in the middle.
        assert_eq!(separator_url_cost(), 38);
        assert_eq!(encode_component(marker("000000", 0).as_str()).len(), separator_url_cost());
    }

    #[test]
    fn encode_component_is_javascript_percent_encoding() {
        assert_eq!(encode_component("a b"), "a%20b");
        assert_eq!(encode_component("héllo"), "h%C3%A9llo");
        // `encodeURIComponent` leaves `~ ! ' ( ) *` alone.
        assert_eq!(encode_component("!*'()~"), "!*'()~");
        assert_eq!(encode_component("café 😀"), "caf%C3%A9%20%F0%9F%98%80");
    }

    #[test]
    fn answer_fixture_splits_on_markers() {
        let cores = vec!["⟦¶abc123-0⟧".to_string()];
        let answer = "Hola mundo.\n⟦¶abc123-0⟧\nSegundo párrafo.";
        let paragraphs = split_on_markers(answer, &cores).expect("markers survive");
        assert_eq!(paragraphs, vec!["Hola mundo.\n", "\nSegundo párrafo."]);

        let missing = split_on_markers("no markers here", &cores);
        assert!(missing.is_none());
    }

    #[test]
    fn batch_body_fixture_parses_translated_fragments() {
        // The shape the endpoint really answers with: an outer array whose
        // first element holds one `[translated, source, ...]` triple per
        // sentence (`google.test.ts`'s `[[['es…', '…']]]` fixture).
        let body: serde_json::Value = serde_json::from_str(
            r#"[[["Hola mundo.","Hello world.",null,null,1],[" ¿Cómo estás?"," How are you?",null,null,1]]]"#,
        )
        .expect("fixture is json");
        let paragraphs = parse_batch_body(&body, &[], 1).expect("parses");
        assert_eq!(paragraphs, vec!["Hola mundo. ¿Cómo estás?"]);
    }

    #[test]
    fn wrong_paragraph_count_is_refused_with_the_exact_words() {
        let body: serde_json::Value = serde_json::from_str(r#"[[["solo uno"]]]"#).expect("fixture is json");
        let cores = vec!["⟦¶aaaaaa-0⟧".to_string()];
        let error = parse_batch_body(&body, &cores, 2).expect_err("one of two is not two");
        assert_eq!(
            error.message,
            "The translation service answered with 0 of 2 paragraphs."
        );
        assert_eq!(error.reason, FailureReason::Shape);
    }

    #[test]
    fn a_body_that_is_not_a_translation_is_refused() {
        let body: serde_json::Value = serde_json::from_str(r#"{"error":"nope"}"#).expect("fixture is json");
        let error = parse_batch_body(&body, &[], 1).expect_err("an object is not a translation");
        assert_eq!(
            error.message,
            "The translation service returned a response that was not a translation."
        );
    }

    #[test]
    fn long_blocks_split_on_sentence_boundaries() {
        let text = "A".repeat(MAX_PIECE_CHARS - 2) + ". tail";
        let pieces = split_long_block(&text);
        assert_eq!(pieces.len(), 2);
        assert!(pieces[0].ends_with('.'));
        assert_eq!(pieces.join(""), text);

        let short = "short paragraph";
        assert_eq!(split_long_block(short), vec![short.to_string()]);
    }

    #[test]
    fn long_blocks_fall_back_to_spaces_then_hard_cuts() {
        let text = "word ".repeat(1200); // no sentence punctuation at all
        let pieces = split_long_block(text.trim_end());
        assert!(pieces.len() >= 2);
        assert!(pieces[0].len() <= MAX_PIECE_CHARS);

        let solid = "x".repeat(MAX_PIECE_CHARS * 2 + 10);
        let pieces = split_long_block(&solid);
        assert_eq!(pieces.len(), 3);
        assert_eq!(pieces.concat(), solid);
    }

    #[test]
    fn planner_respects_both_budgets() {
        let texts: Vec<String> = (0..30).map(|i| format!("paragraph {i}")).collect();
        let batches = plan_requests(&texts);
        assert!(batches.len() > 1);
        for batch in &batches {
            assert!(batch.len() <= MAX_ITEMS_PER_BATCH);
            let cost: usize = batch.iter().map(|piece| url_length(&piece.text)).sum();
            assert!(cost <= BATCH_CHAR_BUDGET);
        }
        let total: usize = batches.iter().map(|b| b.len()).sum();
        assert_eq!(total, texts.len());
    }

    #[test]
    fn planner_never_shares_a_piece_between_batches() {
        let texts = vec!["one".to_string(), "two".to_string()];
        let batches = plan_requests(&texts);
        assert_eq!(batches.len(), 1);
        assert_eq!(
            batches[0].iter().map(|p| p.block_index).collect::<Vec<_>>(),
            vec![0, 1]
        );
    }

    #[test]
    fn tidy_translated_collapses_whitespace_but_keeps_line_breaks() {
        assert_eq!(tidy_translated("  a   b  "), "a b");
        assert_eq!(tidy_translated("a \n b"), "a\nb");
        // `/ *\n */g` matches each newline separately (a newline is not a
        // space), so runs of blank lines pass through untouched — exactly as
        // JavaScript leaves them.
        assert_eq!(tidy_translated("a\n\n\nb"), "a\n\n\nb");
    }

    #[test]
    fn refused_429_explains_the_connection() {
        let error = refused(429);
        assert!(error.message.contains("HTTP 429"));
        assert!(error.retryable);
        assert_eq!(error.reason, FailureReason::Refused);

        let server = refused(503);
        assert_eq!(server.message, "The translation service answered with HTTP 503.");
        assert_eq!(server.reason, FailureReason::Server);

        let missing = refused(404);
        assert_eq!(missing.reason, FailureReason::Shape);
        assert!(!missing.retryable);
    }
}
