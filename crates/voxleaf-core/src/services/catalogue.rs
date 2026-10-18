//! Provider catalogue search.
//!
//! Port of `src/server/services/book-search/*`: one provider (Open Library),
//! results mirrored into the local catalogue so the app keeps working offline,
//! per-book library state merged in, and a local-only fallback when the
//! provider cannot be reached — a degraded store rather than an error page.

use std::time::Duration;

use serde_json::{json, to_value, Value};

use crate::config::Config;
use crate::db::Database;
use crate::error::{ApiError, ApiResult};
use crate::models::BookSummary;
use crate::repo;

use super::library::{
    block_db, collapse_whitespace, load_book_summary_by_external, sanitize_multiline,
    sanitize_text,
};

/// The provider-agnostic metadata shape (`ProviderBook` in the web app).
#[derive(Debug, Clone, Default)]
pub struct ProviderBook {
    /// Provider-native identifier, e.g. `/works/OL17930368W`.
    pub external_id: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub author: String,
    /// Short blurb for cards.
    pub summary: Option<String>,
    /// Full description, only guaranteed after `get_book`.
    pub description: Option<String>,
    pub cover_url: Option<String>,
    pub publisher: Option<String>,
    pub publication_date: Option<String>,
    pub isbn: Option<String>,
    pub categories: Vec<String>,
    pub page_count: Option<i64>,
    pub language: Option<String>,
    pub rating: Option<f64>,
    pub rating_count: Option<i64>,
}

/// One page of provider results, already filtered and de-duplicated.
struct ProviderSearchResult {
    books: Vec<ProviderBook>,
    total: i64,
    #[allow(dead_code)]
    has_more: bool,
}

/// Genre slugs and the provider subject each one searches
/// (`GENRE_FILTERS` in `src/utils/constants.ts`).
const GENRE_SUBJECTS: &[(&str, &str)] = &[
    ("all", ""),
    ("fiction", "fiction"),
    ("non-fiction", "nonfiction"),
    ("biography", "biography"),
    ("business", "business"),
    ("self-help", "self_help"),
    ("history", "history"),
    ("science", "science"),
    ("technology", "technology"),
    ("romance", "romance"),
    ("mystery", "mystery"),
    ("fantasy", "fantasy"),
    ("thriller", "thriller"),
    ("poetry", "poetry"),
    ("children", "juvenile_literature"),
    ("philosophy", "philosophy"),
    ("psychology", "psychology"),
    ("travel", "travel"),
];

/// The columns the search UI asks Open Library for, in one `fields=` list.
const SEARCH_FIELDS: &str = "key,title,subtitle,author_name,first_sentence,cover_i,\
     cover_edition_key,publisher,first_publish_year,publish_year,isbn,\
     number_of_pages_median,subject,ratings_average,ratings_count,language,edition_count";

const UNKNOWN_AUTHOR: &str = "Unknown author";
const MIN_PLAUSIBLE_YEAR: i64 = 1000;
const MAX_CATEGORY_COUNT: usize = 12;
/// Categories mirrored per book (`slice(0, 24)` in `upsertCatalogBook`).
const MAX_MIRRORED_CATEGORIES: usize = 24;
const PROVIDER_NAME: &str = "openlibrary";
/// Brief pause between attempts so a rate-limit window has a chance to clear.
const RETRY_PAUSE_MS: u64 = 350;

/// Subjects that read like database noise rather than a genre on a shop card.
const SUBJECT_BLOCKLIST: &[&str] = &[
    "accessible book",
    "protected daisy",
    "in library",
    "lending library",
    "overdrive",
    "large type books",
    "internet archive wishlist",
    "reading level-grade 9",
    "reading level-grade 8",
    "reading level-grade 7",
    "reading level-grade 6",
    "book",
    "books",
];

pub struct CatalogueService {
    base_url: String,
    cover_base_url: String,
    timeout: Duration,
    offline_fallback: bool,
    client: reqwest::Client,
}

impl CatalogueService {
    pub fn new(cfg: &Config) -> Self {
        Self {
            base_url: cfg.book.base_url.trim_end_matches('/').to_string(),
            cover_base_url: cfg.book.cover_base_url.to_string(),
            timeout: Duration::from_millis(cfg.book.timeout_ms.max(1)),
            offline_fallback: cfg.book.offline_fallback,
            client: reqwest::Client::new(),
        }
    }

    /* ---------------------------------------------------------------------- */
    /* Search                                                                 */
    /* ---------------------------------------------------------------------- */

    /// `searchBooks` — the store's single discovery entry point.
    pub async fn search(
        &self,
        db: &Database,
        q: &str,
        genre: &str,
        sort: &str,
        page: i64,
        page_size: i64,
    ) -> ApiResult<Value> {
        let page = page.max(1);
        let page_size = page_size.clamp(1, 50);
        let term = sanitize_text(q, 200);
        let subject = genre_subject(genre);

        // A browse with nothing to search for is not a provider question:
        // paging the catalogue we already hold is a normal, working state.
        if term.is_empty() && subject.is_empty() {
            let (books, total) = self.search_local(db, &term, page, page_size)?;
            return Ok(search_envelope(books, total, page, page_size, false, "local"));
        }

        match self.provider_search(&term, genre, sort, page, page_size).await {
            Ok(result) => {
                // Mirror the page into the local catalogue. Failures here must
                // never break search — the DB is a cache, not the source of
                // truth for discovery.
                let hit_query = if term.is_empty() { "(browse)" } else { term.as_str() };
                let local_ids = match self.mirror(db, &result.books, hit_query) {
                    Ok(ids) => ids,
                    Err(error) => {
                        tracing::error!(%error, "failed to mirror catalogue");
                        Vec::new()
                    }
                };

                let merged: Vec<BookSummary> = if local_ids.is_empty() {
                    Vec::new()
                } else {
                    self.load_by_ids(db, &local_ids)?
                };
                // Preserve provider ordering — the local rows come back unordered.
                let by_id: std::collections::HashMap<&str, &BookSummary> = merged
                    .iter()
                    .map(|book| (book.id.as_str(), book))
                    .collect();
                let mut books: Vec<Value> = Vec::new();
                for id in &local_ids {
                    if let Some(book) = by_id.get(id.as_str()) {
                        books.push(to_value(*book)?);
                    }
                }
                // Render provider books directly when the local write failed.
                if books.is_empty() {
                    books = result
                        .books
                        .iter()
                        .map(fallback_summary)
                        .collect::<ApiResult<Vec<_>>>()?;
                }
                Ok(search_envelope(
                    books,
                    result.total,
                    page,
                    page_size,
                    false,
                    PROVIDER_NAME,
                ))
            }
            Err(error) => {
                if !self.offline_fallback {
                    tracing::error!(%error, "book search provider failed");
                    return Err(ApiError::provider_unavailable());
                }
                tracing::error!(%error, "provider failed, using local catalogue");
                let (books, total) = self.search_local(db, &term, page, page_size)?;
                Ok(search_envelope(
                    books,
                    total,
                    page,
                    page_size,
                    true,
                    &format!("{PROVIDER_NAME} (offline)"),
                ))
            }
        }
    }

    /// Local catalogue search (`searchLocalCatalog`): the offline fallback.
    fn search_local(
        &self,
        db: &Database,
        term: &str,
        page: i64,
        page_size: i64,
    ) -> ApiResult<(Vec<Value>, i64)> {
        let term = term.to_string();
        block_db(db, move |conn| {
            let pattern = format!("%{term}%");
            let offset = (page - 1) * page_size;
            let (books, total): (Vec<Value>, i64) = if term.is_empty() {
                let sql = format!(
                    "SELECT {} {} ORDER BY b.created_at DESC LIMIT ?1 OFFSET ?2",
                    repo::BOOK_SUMMARY_COLUMNS,
                    repo::BOOK_FROM
                );
                let mut statement = conn.prepare(&sql)?;
                let rows = statement.query_map(
                    rusqlite::params![page_size, offset],
                    repo::map_book_summary,
                )?;
                let mut books: Vec<Value> = Vec::new();
                for row in rows {
                    books.push(to_value(row?)?);
                }
                let total: i64 =
                    conn.query_row("SELECT COUNT(*) FROM books", [], |row| row.get(0))?;
                (books, total)
            } else {
                let where_sql = "WHERE b.title LIKE ?1 OR b.author LIKE ?2 OR b.publisher LIKE ?3 OR b.isbn LIKE ?4";
                let sql = format!(
                    "SELECT {} {} {where_sql} ORDER BY b.created_at DESC LIMIT ?5 OFFSET ?6",
                    repo::BOOK_SUMMARY_COLUMNS,
                    repo::BOOK_FROM
                );
                let mut statement = conn.prepare(&sql)?;
                let rows = statement.query_map(
                    rusqlite::params![pattern, pattern, pattern, pattern, page_size, offset],
                    repo::map_book_summary,
                )?;
                let mut books: Vec<Value> = Vec::new();
                for row in rows {
                    books.push(to_value(row?)?);
                }
                let count_sql =
                    format!("SELECT COUNT(*) FROM books b {where_sql}");
                let total: i64 = conn.query_row(
                    &count_sql,
                    rusqlite::params![pattern, pattern, pattern, pattern],
                    |row| row.get(0),
                )?;
                (books, total)
            };
            Ok((books, total))
        })
    }

    /// Mirror a provider page into `books` + `catalog_hits` (`upsertCatalogBook`).
    fn mirror(
        &self,
        db: &Database,
        books: &[ProviderBook],
        hit_query: &str,
    ) -> ApiResult<Vec<String>> {
        let books = books.to_vec();
        let hit_query = hit_query.to_string();
        block_db(db, move |conn| {
            let mut local_ids: Vec<String> = Vec::new();
            for book in &books {
                local_ids.push(upsert_catalog_book(conn, book)?);
            }
            // De-duplicate before insert: Open Library can return the same
            // work more than once.
            let mut seen: Vec<&str> = Vec::new();
            let now = repo::now_iso();
            for id in &local_ids {
                if seen.contains(&id.as_str()) {
                    continue;
                }
                seen.push(id.as_str());
                conn.execute(
                    "INSERT INTO catalog_hits (id, book_id, query, created_at) VALUES (?1, ?2, ?3, ?4)",
                    rusqlite::params![repo::new_id("hit"), id, hit_query, now],
                )?;
            }
            Ok(local_ids)
        })
    }

    /// Re-read the mirrored rows so library state is attached
    /// (`mergeLibraryState`).
    fn load_by_ids(&self, db: &Database, ids: &[String]) -> ApiResult<Vec<BookSummary>> {
        let ids = ids.to_vec();
        block_db(db, move |conn| {
            if ids.is_empty() {
                return Ok(Vec::new());
            }
            let placeholders = (1..=ids.len())
                .map(|index| format!("?{index}"))
                .collect::<Vec<_>>()
                .join(", ");
            let sql = format!(
                "SELECT {} {} WHERE b.id IN ({placeholders})",
                repo::BOOK_SUMMARY_COLUMNS,
                repo::BOOK_FROM
            );
            let mut statement = conn.prepare(&sql)?;
            let rows = statement.query_map(rusqlite::params_from_iter(ids.iter()), repo::map_book_summary)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(ApiError::from)
        })
    }

    /* ---------------------------------------------------------------------- */
    /* Provider                                                               */
    /* ---------------------------------------------------------------------- */

    /// `OpenLibraryProvider.search`: one request, one retry for transient
    /// failures, then local filtering of the page.
    async fn provider_search(
        &self,
        term: &str,
        genre: &str,
        sort: &str,
        page: i64,
        page_size: i64,
    ) -> ApiResult<ProviderSearchResult> {
        let query = build_search_q(term, genre);
        let mut url = format!("{}/search.json", self.base_url);
        url.push_str(&format!(
            "?q={}&fields={}&limit={page_size}&offset={}",
            urlencoding::encode(&query),
            SEARCH_FIELDS,
            (page - 1) * page_size
        ));
        // Open Library only supports publication-order sorting server-side;
        // title order is applied locally on the returned page.
        if sort == "newest" {
            url.push_str("&sort=new");
        } else if sort == "oldest" {
            url.push_str("&sort=old");
        }

        let payload = self.request_json(&url, true).await?;
        let docs = payload
            .get("docs")
            .and_then(|docs| docs.as_array())
            .cloned()
            .unwrap_or_default();
        let total = payload
            .get("numFound")
            .and_then(|value| value.as_i64())
            .unwrap_or(docs.len() as i64);

        let mut books: Vec<ProviderBook> = docs
            .iter()
            .filter_map(|doc| to_provider_book(doc, &self.cover_base_url))
            .filter(is_plausible_book)
            .collect();

        // De-duplicate by title+author: Open Library often returns the same
        // work under several editions.
        let mut seen: Vec<(String, String)> = Vec::new();
        books.retain(|book| {
            let key = (book.title.to_lowercase(), book.author.to_lowercase());
            if seen.contains(&key) {
                return false;
            }
            seen.push(key);
            true
        });

        if sort == "title-asc" {
            books.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
        }

        Ok(ProviderSearchResult {
            books,
            total,
            has_more: page * page_size < total,
        })
    }

    /// Full detail for one book (`getBook` on the provider).
    pub async fn get_book(&self, external_id: &str) -> ApiResult<Option<ProviderBook>> {
        let key = sanitize_text(external_id, 120);
        if !key.starts_with("/works/") && !key.starts_with("/books/") {
            return Ok(None);
        }
        let id = key.trim_start_matches('/');
        let url = format!("{}/{id}.json", self.base_url);

        let payload = match self.request_json(&url, false).await {
            Ok(payload) => payload,
            Err(RequestError::NotFound) => return Ok(None),
            Err(error) => return Err(error.into()),
        };

        let description = match payload.get("description") {
            Some(Value::String(value)) => Some(value.clone()),
            Some(object) => object
                .get("value")
                .and_then(|value| value.as_str())
                .map(|value| value.to_string()),
            None => None,
        };
        let covers: Vec<i64> = payload
            .get("covers")
            .and_then(|covers| covers.as_array())
            .map(|covers| {
                covers
                    .iter()
                    .filter_map(|cover| cover.as_i64())
                    .filter(|cover| *cover > 0)
                    .collect()
            })
            .unwrap_or_default();

        Ok(Some(ProviderBook {
            external_id: key,
            title: sanitize_text(
                payload.get("title").and_then(|value| value.as_str()).unwrap_or(""),
                300,
            )
            .pipe(|title| if title.is_empty() { "Untitled".to_string() } else { title }),
            subtitle: None,
            author: UNKNOWN_AUTHOR.to_string(),
            description: description
                .map(|description| sanitize_multiline(&description, 8000))
                .filter(|description| !description.is_empty()),
            cover_url: build_cover_url(covers.first().copied(), &self.cover_base_url),
            publication_date: normalize_publication_date(
                payload.get("first_publish_date").and_then(|value| value.as_str()),
            ),
            categories: pick_categories(
                payload
                    .get("subjects")
                    .and_then(|subjects| subjects.as_array())
                    .map(|subjects| -> Vec<String> {
                        subjects
                            .iter()
                            .filter_map(|subject| subject.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .as_deref(),
            ),
            ..ProviderBook::default()
        }))
    }

    /// Full detail for one book as the API returns it (`getBookByExternalId`):
    /// local record first, then hydrate from the provider when the description
    /// or cover is still thin.
    /// Mirror one provider book into the local catalogue and return its id.
    pub fn upsert_catalog_book(&self, db: &Database, book: &ProviderBook) -> ApiResult<String> {
        let book = book.clone();
        block_db(db, move |conn| upsert_catalog_book(conn, &book))
    }

    pub async fn get_book_summary(&self, db: &Database, external_id: &str) -> ApiResult<Option<Value>> {
        let key = sanitize_text(external_id, 120);
        if key.is_empty() {
            return Ok(None);
        }

        let local: Option<BookSummary> = {
            let lookup = key.clone();
            block_db(db, move |conn| load_book_summary_by_external(conn, &lookup))?
        };

        if let Some(local) = &local {
            if local.description.is_some() && local.cover_url.is_some() {
                return Ok(Some(to_value(local)?));
            }
        }

        let provider_book = match self.get_book(&key).await {
            Ok(book) => book,
            Err(error) => {
                tracing::error!(%error, "book hydration failed");
                None
            }
        };
        let Some(provider_book) = provider_book else {
            return Ok(local.map(|local| to_value(local)).transpose()?);
        };

        // Keep richer fields we already have from the search result.
        let merged = ProviderBook {
            author: if provider_book.author == UNKNOWN_AUTHOR {
                local
                    .as_ref()
                    .map(|local| local.author.clone())
                    .filter(|author| !author.is_empty())
                    .unwrap_or_else(|| provider_book.author.clone())
            } else {
                provider_book.author.clone()
            },
            description: provider_book
                .description
                .clone()
                .or_else(|| local.as_ref().and_then(|local| local.description.clone())),
            cover_url: provider_book
                .cover_url
                .clone()
                .or_else(|| local.as_ref().and_then(|local| local.cover_url.clone())),
            ..provider_book
        };
        let id = {
            let book = merged.clone();
            block_db(db, move |conn| upsert_catalog_book(conn, &book))?
        };
        let fresh: Option<BookSummary> = {
            let lookup = id.clone();
            block_db(db, move |conn| -> ApiResult<Option<BookSummary>> {
                let sql = format!(
                    "SELECT {} {} WHERE b.id = ?1",
                    repo::BOOK_SUMMARY_COLUMNS,
                    repo::BOOK_FROM
                );
                let mut statement = conn.prepare(&sql)?;
                let mut rows = statement.query_map([lookup], repo::map_book_summary)?;
                match rows.next() {
                    Some(row) => Ok(Some(row.map_err(ApiError::from)?)),
                    None => Ok(None),
                }
            })?
        };
        Ok(fresh.map(|book| to_value(book)).transpose()?)
    }

    /// One HTTP GET as JSON, retrying transient failures once
    /// (`OpenLibraryProvider.request`).
    async fn request_json(&self, url: &str, _retry: bool) -> Result<Value, RequestError> {
        let mut last_error = RequestError::Unreachable;
        for attempt in 0..2 {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(RETRY_PAUSE_MS)).await;
            }
            let response = self
                .client
                .get(url)
                .timeout(self.timeout)
                .header("accept", "application/json")
                .header("user-agent", "VoxLeaf/1.0 (self-hosted audiobook library)")
                .send()
                .await;

            match response {
                Ok(response) => {
                    let status = response.status().as_u16();
                    if response.status().is_success() {
                        return response.json().await.map_err(|_| RequestError::Unreachable);
                    }
                    let error = if status == 404 {
                        RequestError::NotFound
                    } else {
                        RequestError::Status(status)
                    };
                    if !is_transient_status(Some(status)) {
                        return Err(error);
                    }
                    last_error = error;
                }
                Err(error) => {
                    if error.is_timeout() {
                        last_error = RequestError::Timeout;
                    } else {
                        last_error = RequestError::Unreachable;
                    }
                }
            }
        }
        Err(last_error)
    }
}

/// Insert or refresh a locally mirrored catalogue record
/// (`upsertCatalogBook`). Returns the local book id.
fn upsert_catalog_book(conn: &rusqlite::Connection, book: &ProviderBook) -> ApiResult<String> {
    let title = sanitize_text(&book.title, 300);
    let title = if title.is_empty() { "Untitled".to_string() } else { title };
    let subtitle = book
        .subtitle
        .as_deref()
        .map(|value| sanitize_text(value, 200))
        .filter(|value| !value.is_empty());
    let author = sanitize_text(&book.author, 200);
    let author = if author.is_empty() { UNKNOWN_AUTHOR } else { &author };
    let description = book
        .description
        .as_deref()
        .map(|value| sanitize_text(value, 8000))
        .or_else(|| {
            book.summary
                .as_deref()
                .map(|value| sanitize_text(value, 2000))
        })
        .filter(|value| !value.is_empty());
    let publisher = book
        .publisher
        .as_deref()
        .map(|value| sanitize_text(value, 200))
        .filter(|value| !value.is_empty());
    let categories: Vec<&str> = book
        .categories
        .iter()
        .take(MAX_MIRRORED_CATEGORIES)
        .map(|value| value.as_str())
        .collect();
    let raw = json!({
        "rating": book.rating,
        "ratingCount": book.rating_count,
        "coverRemote": book.cover_url,
    });

    conn.query_row(
        "INSERT INTO books \
            (id, external_id, title, subtitle, author, description, cover_url, publisher, \
             publication_date, isbn, categories, page_count, language, raw, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?15) \
         ON CONFLICT(external_id) DO UPDATE SET \
            title = excluded.title, \
            subtitle = excluded.subtitle, \
            author = excluded.author, \
            description = COALESCE(excluded.description, books.description), \
            cover_url = COALESCE(excluded.cover_url, books.cover_url), \
            publisher = excluded.publisher, \
            publication_date = excluded.publication_date, \
            isbn = excluded.isbn, \
            categories = excluded.categories, \
            page_count = excluded.page_count, \
            language = excluded.language, \
            raw = excluded.raw, \
            updated_at = excluded.updated_at \
         RETURNING id",
        rusqlite::params![
            repo::new_id("bk"),
            book.external_id,
            title,
            subtitle,
            author,
            description,
            book.cover_url,
            publisher,
            book.publication_date,
            book.isbn,
            serde_json::to_string(&categories).unwrap_or_else(|_| "[]".to_string()),
            book.page_count,
            book.language,
            raw.to_string(),
            repo::now_iso(),
        ],
        |row| row.get::<_, String>(0),
    )
    .map_err(ApiError::from)
}

/* -------------------------------------------------------------------------- */
/* Pure provider mapping                                                      */
/* -------------------------------------------------------------------------- */

fn search_envelope(
    books: Vec<Value>,
    total: i64,
    page: i64,
    page_size: i64,
    degraded: bool,
    provider: &str,
) -> Value {
    json!({
        "books": books,
        "total": total,
        "page": page,
        "pageSize": page_size,
        "hasMore": page * page_size < total,
        "degraded": degraded,
        "provider": provider,
    })
}

/// Render a provider book directly, for the rare case the local write failed
/// (`toFallbackSummary`).
fn fallback_summary(book: &ProviderBook) -> ApiResult<Value> {
    Ok(json!({
        "id": format!("external:{}", book.external_id),
        "externalId": book.external_id,
        "title": book.title,
        "subtitle": book.subtitle,
        "author": book.author,
        "description": book.description.clone().or_else(|| book.summary.clone()),
        "coverUrl": book.cover_url,
        "publisher": book.publisher,
        "publicationDate": book.publication_date,
        "isbn": book.isbn,
        "categories": book.categories,
        "pageCount": book.page_count,
        "language": book.language,
        "rating": book.rating,
        "ratingCount": book.rating_count,
        "inLibrary": false,
        "favorite": false,
        "libraryId": Value::Null,
    }))
}

fn genre_subject(genre: &str) -> &str {
    GENRE_SUBJECTS
        .iter()
        .find(|(slug, _)| *slug == genre)
        .map(|(_, subject)| *subject)
        .unwrap_or("")
}

/// `buildSearchQ`: an ISBN-looking query becomes a field query, a genre becomes
/// a subject filter, and an empty browse still asks for something.
fn build_search_q(term: &str, genre: &str) -> String {
    let subject = genre_subject(genre);
    let subject_term = if subject.is_empty() {
        String::new()
    } else {
        format!("subject:{subject}")
    };

    if term.is_empty() {
        if subject_term.is_empty() {
            return "the".to_string();
        }
        return subject_term;
    }
    let compact: String = term.chars().filter(|ch| !matches!(ch, ' ' | '-')).collect();
    let compact = compact.to_uppercase();
    if is_isbn(&compact) {
        return format!("isbn:{compact}");
    }
    if subject_term.is_empty() {
        return term.to_string();
    }
    format!("{term} AND {subject_term}")
}

/// The web app's `/^(97[89])?\d{9}[\dX]$/`, written out.
fn is_isbn(value: &str) -> bool {
    let digits: Vec<char> = value.chars().collect();
    let body = match digits.len() {
        10 => &digits[..],
        13 => match digits.starts_with(&['9', '7', '8']) || digits.starts_with(&['9', '7', '9']) {
            true => &digits[3..],
            false => return false,
        },
        _ => return false,
    };
    if digits.len() == 10 {
        let (head, last) = body.split_at(9);
        head.iter().all(|ch| ch.is_ascii_digit()) && (last[0].is_ascii_digit() || last[0] == 'X')
    } else {
        body.iter().all(|ch| ch.is_ascii_digit())
    }
}

/// Strip the junk ISBNs Open Library occasionally indexes (e.g. "ISBN 22").
fn pick_isbn(values: &[String]) -> Option<String> {
    values
        .iter()
        .map(|value| {
            let cleaned: String = sanitize_text(value, 32)
                .chars()
                .filter(|ch| !matches!(ch, ' ' | '-'))
                .collect::<String>()
                .to_uppercase();
            cleaned
        })
        .find(|value| is_isbn(value))
}

fn pick_sentence(value: Option<&Value>) -> Option<String> {
    let raw = match value? {
        Value::String(value) => value.clone(),
        Value::Array(values) => values.first()?.as_str()?.to_string(),
        _ => return None,
    };
    let cleaned = collapse_whitespace(&sanitize_multiline(&raw, 400));
    let cleaned = cleaned.trim().to_string();
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

fn pick_categories(subjects: Option<&[String]>) -> Vec<String> {
    let Some(subjects) = subjects else { return Vec::new() };
    let mut seen: Vec<String> = Vec::new();
    let mut out: Vec<String> = Vec::new();
    for raw in subjects {
        let subject = sanitize_text(raw, 60);
        if subject.is_empty() {
            continue;
        }
        let key = subject.to_lowercase();
        if SUBJECT_BLOCKLIST.contains(&key.as_str()) || seen.contains(&key) {
            continue;
        }
        seen.push(key);
        out.push(subject);
        if out.len() >= MAX_CATEGORY_COUNT {
            break;
        }
    }
    out
}

fn cover_id(doc: &Value) -> Option<i64> {
    match doc.get("cover_i").and_then(|value| value.as_i64()) {
        Some(id) if id > 0 => Some(id),
        _ => None,
    }
}

fn build_cover_url(cover: Option<i64>, base: &str) -> Option<String> {
    let cover = cover?;
    if cover == 0 {
        return None;
    }
    let base = base.trim_end_matches('/');
    Some(format!("{base}/b/id/{cover}-L.jpg"))
}

/// One Open Library search doc → a provider book (`toProviderBook`).
fn to_provider_book(doc: &Value, cover_base: &str) -> Option<ProviderBook> {
    let title = sanitize_text(doc.get("title")?.as_str().unwrap_or(""), 300);
    if title.is_empty() {
        return None;
    }
    let key = sanitize_text(doc.get("key").and_then(|value| value.as_str()).unwrap_or(""), 120);
    if key.is_empty() {
        return None;
    }

    let first_publish_year = doc
        .get("first_publish_year")
        .and_then(|value| value.as_i64())
        .filter(|year| *year > 0);
    let publish_year = doc
        .get("publish_year")
        .and_then(|value| value.as_array())
        .and_then(|years| {
            years
                .iter()
                .filter_map(|year| year.as_i64())
                .min()
        });
    let publish_year = first_publish_year.or(publish_year);

    let author = doc
        .get("author_name")
        .and_then(|value| value.as_array())
        .and_then(|authors| authors.first())
        .and_then(|author| author.as_str())
        .map(|author| sanitize_text(author, 200))
        .filter(|author| !author.is_empty())
        .unwrap_or_else(|| UNKNOWN_AUTHOR.to_string());

    let publisher = doc
        .get("publisher")
        .and_then(|value| value.as_array())
        .and_then(|values| values.first())
        .and_then(|value| value.as_str())
        .map(|value| sanitize_text(value, 200))
        .filter(|value| !value.is_empty());

    let language = doc
        .get("language")
        .and_then(|value| value.as_array())
        .and_then(|values| values.first())
        .and_then(|value| value.as_str())
        .map(|value| sanitize_text(value, 30))
        .filter(|value| !value.is_empty());

    let rating = doc
        .get("ratings_average")
        .and_then(|value| value.as_f64())
        .filter(|rating| *rating > 0.0)
        .map(|rating| (rating * 10.0).round() / 10.0);
    let rating_count = doc
        .get("ratings_count")
        .and_then(|value| value.as_i64())
        .filter(|count| *count > 0);
    let page_count = doc
        .get("number_of_pages_median")
        .and_then(|value| value.as_i64())
        .filter(|pages| *pages > 0);

    Some(ProviderBook {
        external_id: key,
        title,
        subtitle: doc
            .get("subtitle")
            .and_then(|value| value.as_str())
            .map(|value| sanitize_text(value, 200))
            .filter(|value| !value.is_empty()),
        author,
        summary: pick_sentence(doc.get("first_sentence")),
        description: None,
        cover_url: build_cover_url(cover_id(doc), &cover_base),
        publisher,
        publication_date: publish_year
            .map(|year| year.to_string())
            .and_then(|year| normalize_publication_date(Some(year.as_str()))),
        isbn: pick_isbn(
            &doc
                .get("isbn")
                .and_then(|value| value.as_array())
                .map(|values| -> Vec<String> {
                    values
                        .iter()
                        .filter_map(|value| value.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default(),
        ),
        categories: pick_categories(
            doc.get("subject").and_then(|value| value.as_array()).map(|values| {
                values
                    .iter()
                    .filter_map(|value| value.as_str().map(|s| s.to_string()))
                    .collect::<Vec<_>>()
            })
            .as_deref(),
        ),
        page_count,
        language,
        rating,
        rating_count,
    })
}

/// `isPlausibleBook`: Open Library's `sort=new` surfaces cataloguing errors
/// (e.g. year 9183) and placeholder editions.
fn is_plausible_book(book: &ProviderBook) -> bool {
    let Some(date) = &book.publication_date else { return true };
    let year: i64 = match date.get(0..4).and_then(|year| year.parse().ok()) {
        Some(year) => year,
        None => return true,
    };
    let current_year = i64::from(chrono::Utc::now().year());
    year >= MIN_PLAUSIBLE_YEAR && year <= current_year + 2
}

/// `normalizePublicationDate`: provider dates become `YYYY-MM-DD` when
/// possible, and a bare year becomes `YYYY-01-01`.
fn normalize_publication_date(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    if value.is_empty() {
        return None;
    }
    let year: i64 = first_four_digits(value)?.parse().ok()?;
    if !(-3000..=3000).contains(&year) {
        return None;
    }

    // A day/month-style date the runtime `Date` would have parsed.
    if contains_day_month(value) {
        for format in [
            "%Y-%m-%d",
            "%Y/%m/%d",
            "%m/%d/%Y",
            "%m-%d-%Y",
            "%d/%m/%Y",
            "%d-%m-%Y",
        ] {
            if let Ok(date) = chrono::NaiveDate::parse_from_str(value, format) {
                return Some(date.format("%Y-%m-%d").to_string());
            }
        }
    }
    Some(format!("{year}-01-01"))
}

fn first_four_digits(value: &str) -> Option<&str> {
    let bytes = value.as_bytes();
    for start in 0..bytes.len().saturating_sub(3) {
        let window = &value[start..start + 4];
        if window.bytes().all(|byte| byte.is_ascii_digit()) {
            return Some(window);
        }
    }
    None
}

/// The web app's `/\d{1,2}[/-]\d{1,2}/` probe.
fn contains_day_month(value: &str) -> bool {
    let bytes = value.as_bytes();
    for start in 0..bytes.len() {
        let mut digits = 0;
        while start + digits < bytes.len() && bytes[start + digits].is_ascii_digit() && digits < 2 {
            digits += 1;
        }
        if !(1..=2).contains(&digits) {
            continue;
        }
        let separator = start + digits;
        if separator >= bytes.len() {
            continue;
        }
        let after = &value[separator..];
        if let Some(rest) = after.strip_prefix(['/', '-']) {
            if rest.starts_with(|ch: char| ch.is_ascii_digit()) {
                return true;
            }
        }
    }
    false
}

/* -------------------------------------------------------------------------- */
/* Small plumbing                                                             */
/* -------------------------------------------------------------------------- */

/// A failed provider request, carrying the status when there was one.
enum RequestError {
    NotFound,
    Status(u16),
    Timeout,
    Unreachable,
}

impl std::fmt::Display for RequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RequestError::NotFound => write!(f, "provider responded 404"),
            RequestError::Status(status) => write!(f, "provider responded {status}"),
            RequestError::Timeout => write!(f, "provider timed out"),
            RequestError::Unreachable => write!(f, "provider is unreachable"),
        }
    }
}

impl From<RequestError> for ApiError {
    fn from(error: RequestError) -> Self {
        match error {
            RequestError::NotFound => ApiError::not_found("We could not find that book."),
            _ => ApiError::provider_unavailable(),
        }
    }
}

fn is_transient_status(status: Option<u16>) -> bool {
    match status {
        None => true,
        Some(status) => status == 408 || status == 429 || status >= 500,
    }
}

/// Tiny combinator to keep the local-search parameter building readable.
#[allow(dead_code)]
trait ChainParameters: Sized {
    fn chain_parameters<P: rusqlite::ToSql + Send + 'static>(self, params: [P; 2]) -> Self;
}

impl ChainParameters for rusqlite::ParamsFromIter<std::vec::IntoIter<&'static str>> {
    fn chain_parameters<P: rusqlite::ToSql + Send + 'static>(self, _params: [P; 2]) -> Self {
        self
    }
}

trait Pipe: Sized {
    fn pipe<T>(self, f: impl FnOnce(Self) -> T) -> T;
}

impl<T> Pipe for T {
    fn pipe<U>(self, f: impl FnOnce(Self) -> U) -> U {
        f(self)
    }
}

use chrono::Datelike;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::library::tests_support::fixture;

    fn offline_service(offline_fallback: bool) -> CatalogueService {
        CatalogueService {
            // RFC 5737's TEST-NET-1: nothing listens there, so connections
            // fail instantly and no real network is touched.
            base_url: "http://127.0.0.1:1".to_string(),
            cover_base_url: "https://covers.openlibrary.org".to_string(),
            timeout: Duration::from_millis(500),
            offline_fallback,
            client: reqwest::Client::new(),
        }
    }

    #[test]
    fn search_q_encodes_isbn_subjects_and_browses() {
        assert_eq!(build_search_q("", "all"), "the");
        assert_eq!(build_search_q("", "science"), "subject:science");
        assert_eq!(build_search_q("dune", "all"), "dune");
        assert_eq!(build_search_q("dune", "science"), "dune AND subject:science");
        assert_eq!(build_search_q("978-0441478125", "all"), "isbn:9780441478125");
        assert_eq!(build_search_q("0441478123", "history"), "isbn:0441478123");
    }

    #[test]
    fn junk_isbns_are_filtered() {
        let values = vec!["ISBN 22".to_string(), "978-0441478125".to_string()];
        assert_eq!(pick_isbn(&values).as_deref(), Some("9780441478125"));
        assert_eq!(pick_isbn(&["nonsense".to_string()]), None);
        assert!(is_isbn("0441478123"));
        assert!(is_isbn("044147812X"));
        assert!(!is_isbn("12345"));
        assert!(!is_isbn("9781234567890123"));
    }

    #[test]
    fn categories_drop_noise_and_cap() {
        let subjects = vec![
            "Accessible book".to_string(),
            "History".to_string(),
            "history".to_string(),
            "Politics".to_string(),
        ];
        assert_eq!(pick_categories(Some(&subjects)), vec!["History", "Politics"]);

        let many: Vec<String> = (0..20).map(|index| format!("Subject {index}")).collect();
        assert_eq!(pick_categories(Some(&many)).len(), MAX_CATEGORY_COUNT);
    }

    #[test]
    fn publication_dates_normalise_like_the_web_helper() {
        assert_eq!(normalize_publication_date(None), None);
        assert_eq!(normalize_publication_date(Some("")), None);
        assert_eq!(normalize_publication_date(Some("2020")), Some("2020-01-01".into()));
        assert_eq!(normalize_publication_date(Some("9183")), None);
        assert_eq!(
            normalize_publication_date(Some("March 5, 2020")),
            Some("2020-01-01".into())
        );
        assert_eq!(
            normalize_publication_date(Some("2020-03-05")),
            Some("2020-03-05".into())
        );
        assert_eq!(
            normalize_publication_date(Some("12/25/2020")),
            Some("2020-12-25".into())
        );
    }

    #[test]
    fn search_docs_map_to_provider_books() {
        let doc = json!({
            "key": "/works/OL1W",
            "title": "Dune",
            "author_name": ["Frank Herbert"],
            "first_sentence": ["A begin beginning."],
            "cover_i": 12345,
            "publisher": ["Ace"],
            "first_publish_year": 1965,
            "isbn": ["Junk", "0441478123"],
            "subject": ["Accessible book", "Science fiction"],
            "ratings_average": 4.26,
            "ratings_count": 10,
            "language": ["eng"],
            "number_of_pages_median": 412
        });
        let book = to_provider_book(&doc, "https://covers.openlibrary.org").expect("mapped");
        assert_eq!(book.external_id, "/works/OL1W");
        assert_eq!(book.author, "Frank Herbert");
        assert_eq!(book.isbn.as_deref(), Some("0441478123"));
        assert_eq!(book.categories, vec!["Science fiction"]);
        assert_eq!(book.rating, Some(4.3));
        assert_eq!(book.cover_url.as_deref(), Some("https://covers.openlibrary.org/b/id/12345-L.jpg"));
        assert_eq!(book.publication_date.as_deref(), Some("1965-01-01"));

        // Untitled or keyless docs are dropped.
        assert!(to_provider_book(&json!({ "key": "/works/OL1W" }), "https://covers.openlibrary.org").is_none());
        assert!(
            to_provider_book(&json!({ "key": "/works/OL1W", "title": "  " }), "https://covers.openlibrary.org")
                .is_none()
        );
    }

    /// The base is not optional in practice: an empty one produces a
    /// host-less `/b/id/…` path that the browser resolves against our own
    /// origin, and every store poster silently falls back to the placeholder.
    #[test]
    fn a_search_cover_url_is_absolute() {
        let doc = json!({ "key": "/works/OL1W", "title": "Dune", "cover_i": 7 });
        let base = Config::for_data_dir(std::env::temp_dir().join("vb-cover-base")).book.cover_base_url;
        let book = to_provider_book(&doc, &base).expect("mapped");
        let cover = book.cover_url.expect("a cover");
        assert!(
            cover.starts_with("https://"),
            "cover url must be absolute, got {cover}"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_browse_reads_the_local_catalogue_without_degrading() {
        let fixture = fixture();
        fixture.seed_book("bk1", 1, false);
        fixture.library_link("bk1", "lb1");
        let service = offline_service(true);

        let result = service
            .search(&fixture.db, "", "all", "relevance", 1, 20)
            .await.expect("browse");
        assert_eq!(result["provider"], "local");
        assert_eq!(result["degraded"], false);
        let books = result["books"].as_array().expect("books");
        assert_eq!(books.len(), 1);
        assert_eq!(books[0]["inLibrary"], true);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn an_unreachable_provider_degrades_to_local_results() {
        let fixture = fixture();
        fixture.seed_book("bk1", 1, false);
        let service = offline_service(true);

        let result = service
            .search(&fixture.db, "bk1", "all", "relevance", 1, 20)
            .await.expect("degraded search");
        assert_eq!(result["provider"], "openlibrary (offline)");
        assert_eq!(result["degraded"], true);
        assert_eq!(result["books"].as_array().expect("books").len(), 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn without_offline_fallback_a_dead_provider_is_an_error() {
        let fixture = fixture();
        let service = offline_service(false);

        let error = service
            .search(&fixture.db, "dune", "all", "relevance", 1, 20)
            .await.expect_err("provider down");
        assert_eq!(error.status, axum::http::StatusCode::BAD_GATEWAY);
        assert_eq!(error.code, "provider_unavailable");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn unknown_external_ids_are_not_found_locally() {
        let fixture = fixture();
        let service = offline_service(true);

        // A non-Open-Library key never reaches the network.
        assert!(service.get_book("OL123W").await.expect("get_book").is_none());
        let summary = service
            .get_book_summary(&fixture.db, "OL123W")
            .await
            .expect("summary");
        assert!(summary.is_none());
    }
}
