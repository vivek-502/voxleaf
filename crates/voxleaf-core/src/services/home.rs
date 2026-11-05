//! The aggregated home payload.
//!
//! Port of `src/app/api/home/route.ts`: one round trip feeds the whole
//! dashboard, and recommendations are seeded from the genres the user actually
//! keeps — never recommending back something already on the shelf.

use serde_json::{json, Value};

use crate::db::Database;
use crate::error::ApiResult;
use crate::services::library::{block_db, entries_where, library_stats};
use crate::services::Services;

/// Items per shelf strip on the dashboard (`listContinueListening(12)` and
/// friends in the web route).
const STRIP_LIMIT: i64 = 12;
/// The `library` strip is the newest twenty additions.
const LIBRARY_LIMIT: i64 = 20;
/// Recommendations fill one catalogue page (`PAGE_SIZE`).
const PAGE_SIZE: i64 = 20;

/// GET /api/home — continue-listening, newest additions, favourites, stats
/// and personalised recommendations in a single payload.
pub async fn home_payload(db: &Database, services: &Services) -> ApiResult<Value> {
    let (continue_listening, recently_added, library, favorites, stats) =
        block_db(db, move |conn| -> ApiResult<_> {
            let continue_listening = entries_where(
                conn,
                "lb.last_played_at IS NOT NULL AND lb.total_audio_duration > 0",
                "lb.last_played_at DESC",
                STRIP_LIMIT,
            )?;
            let recently_added = entries_where(conn, "", "lb.added_at DESC", STRIP_LIMIT)?;
            let library = entries_where(conn, "", "lb.added_at DESC", LIBRARY_LIMIT)?;
            let favorites = entries_where(conn, "lb.favorite = 1", "lb.added_at DESC", LIBRARY_LIMIT)?;
            let stats = library_stats(conn)?;
            Ok((continue_listening, recently_added, library, favorites, stats))
        })?;

    // Seed recommendations from what the user actually owns: favourite genres
    // first, then the newest addition. Ties keep first-seen order, matching the
    // web app's stable sort over map entries.
    let mut counts: Vec<(String, i64)> = Vec::new();
    for item in favorites.iter().chain(recently_added.iter()) {
        for category in &item.book.categories {
            let key = category.to_lowercase();
            match counts.iter_mut().find(|(seen, _)| *seen == key) {
                Some(entry) => entry.1 += 1,
                None => counts.push((key, 1)),
            }
        }
    }
    // Highest count wins; ties keep first-seen order, matching the web app's
    // stable sort over map entries.
    let seed = counts
        .iter()
        .enumerate()
        .max_by(|(index_a, (_, count_a)), (index_b, (_, count_b))| {
            count_a.cmp(count_b).then(index_b.cmp(index_a))
        })
        .map(|(_, (key, _))| key.clone())
        .unwrap_or_default();

    // A failing provider degrades the shelf, it never breaks the homepage.
    let (recommended, recommended_degraded) = match services
        .catalogue
        .search(db, &seed, "all", "relevance", 1, PAGE_SIZE)
        .await
    {
        Ok(result) => (
            result
                .get("books")
                .and_then(|books| books.as_array())
                .cloned()
                .unwrap_or_default(),
            result
                .get("degraded")
                .and_then(|degraded| degraded.as_bool())
                .unwrap_or(false),
        ),
        Err(_) => (Vec::new(), true),
    };

    // Never recommend something already on the shelf.
    let owned: Vec<&str> = library
        .iter()
        .map(|item| item.book_id.as_str())
        .collect();
    let recommended: Vec<Value> = recommended
        .into_iter()
        .filter(|book| {
            let id = book.get("id").and_then(|id| id.as_str()).unwrap_or_default();
            !owned.contains(&id)
        })
        .collect();

    Ok(json!({
        "continueListening": continue_listening,
        "recentlyAdded": recently_added,
        "library": library,
        "favorites": favorites,
        "stats": stats,
        "recommended": recommended,
        "recommendedDegraded": recommended_degraded,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::services::library::tests_support::fixture;

    #[tokio::test(flavor = "multi_thread")]
    async fn buckets_are_built_from_the_shelf() {
        let fixture = fixture();
        fixture.seed_book("bk1", 1, true);
        fixture.seed_book("bk2", 1, true);
        fixture.seed_book("bk3", 1, false);
        fixture.library_link("bk1", "lb1");
        fixture.library_link("bk2", "lb2");
        fixture.library_link("bk3", "lb3");
        fixture
            .raw()
            .execute(
                "UPDATE library_books SET favorite = 1, added_at = '2026-01-02T00:00:00.000Z' WHERE id = 'lb1'",
                [],
            )
            .expect("favorite");
        // bk2 has been played, bk3 has no audio so it can never be resumed.
        crate::services::progress::save(
            &fixture.db,
            &json!({ "chapterId": "chbk2-0", "position": 10.0, "duration": 100.0 }),
        )
        .expect("save");

        // bk1 carries a genre, so the shelf seeds a real recommendation query.
        fixture
            .raw()
            .execute(
                "UPDATE books SET categories = '[\"Fiction\"]' WHERE id = 'bk1'",
                [],
            )
            .expect("categorise");

        let payload = home_payload(&fixture.db, &fixture_services(&fixture))
            .await
            .expect("home");

        let continue_listening = payload["continueListening"].as_array().expect("list");
        assert_eq!(continue_listening.len(), 1);
        assert_eq!(continue_listening[0]["bookId"], "bk2");

        let favorites = payload["favorites"].as_array().expect("list");
        assert_eq!(favorites.len(), 1);
        assert_eq!(favorites[0]["bookId"], "bk1");

        assert_eq!(payload["library"].as_array().expect("list").len(), 3);
        assert_eq!(payload["stats"]["bookCount"], 3);

        // The provider is unreachable in tests: the page degrades, not errors.
        assert_eq!(payload["recommendedDegraded"], true);
        assert_eq!(payload["recommended"].as_array().expect("list").len(), 0);
    }

    /// The fixture keeps its temp dir private, so rebuild the config beside it.
    ///
    /// The book provider is aimed at an unroutable address so the
    /// recommendation shelf exercises its real degradation path on every run,
    /// rather than depending on a live Open Library.
    fn fixture_services(fixture: &crate::services::library::tests_support::Fixture) -> Services {
        let mut config = Config::for_data_dir(fixture.data_dir());
        config.book.base_url = "http://127.0.0.1:9".to_string();
        config.book.timeout_ms = 250;
        Services::new(&config)
    }
}
