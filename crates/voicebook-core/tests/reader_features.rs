//! End-to-end service tests for the reader features added in the web parity
//! pass: highlights and notes, in-book bookmarks, whole-book search, the
//! dictionary notebook, translation source mapping and the speech endpoints.
//!
//! These drive the same service functions the HTTP handlers call, against a
//! real SQLite database and a real (sanitised) section of markup, so a broken
//! anchor, an omitted image or a response shape copied wrong is caught here
//! rather than in the reader.

use serde_json::{json, Value};

use voicebook_core::config::Config;
use voicebook_core::db::Database;
use voicebook_core::repo;
use voicebook_core::services::dictionary;
use voicebook_core::services::epub_search::{self, SearchOptions};
use voicebook_core::services::reader_annotations;
use voicebook_core::services::reader_bookmarks;
use voicebook_core::services::translation::{self, TranslationService};
use voicebook_core::services::tts::TtsService;

struct Fixture {
    _dir: tempfile::TempDir,
    db: Database,
    config: Config,
}

/// A small book with one ready EPUB section per HTML string.
async fn fixture(sections: &[&str]) -> Fixture {
    let dir = tempfile::tempdir().expect("temp dir");
    let config = Config::for_data_dir(dir.path().to_path_buf());
    let db = Database::open(&config.database_path).expect("database");
    let now = repo::now_iso();
    let sections: Vec<String> = sections.iter().map(|html| (*html).to_string()).collect();

    db.call({
        let now = now.clone();
        move |conn| {
            conn.execute(
                "INSERT INTO books (id, external_id, title, author, language, created_at, updated_at) \
                 VALUES ('bk_test', 'ext-test', 'A Test Book', 'A Writer', 'fr', ?1, ?1)",
                [&now],
            )?;
            conn.execute(
                "INSERT INTO epub_files (id, book_id, status, language, created_at, updated_at) \
                 VALUES ('ep_1', 'bk_test', 'ready', 'fr', ?1, ?1)",
                [&now],
            )?;
            for (index, html) in sections.into_iter().enumerate() {
                conn.execute(
                    "INSERT INTO epub_chapters (id, epub_id, idx, href, title, html) \
                     VALUES (?1, 'ep_1', ?2, ?3, ?4, ?5)",
                    rusqlite::params![
                        format!("ec_{index}"),
                        index as i64,
                        format!("ch{index}.xhtml"),
                        format!("Section {}", index + 1),
                        html,
                    ],
                )?;
            }
            Ok::<(), rusqlite::Error>(())
        }
    })
    .await
    .expect("seed");

    Fixture {
        _dir: dir,
        db,
        config,
    }
}

fn options<'a>(
    query: &'a str,
    language: Option<&'a str>,
    limit: Option<i64>,
    case_sensitive: bool,
    match_diacritics: bool,
    whole_word: bool,
) -> SearchOptions<'a> {
    SearchOptions {
        query,
        language,
        limit,
        case_sensitive,
        match_diacritics,
        whole_word,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn annotations_are_created_clamped_edited_listed_and_deleted() {
    let fixture =
        fixture(&[r#"<img src="plate.jpg" alt="plate"/><p>Hello world.</p><p>Second line.</p>"#])
            .await;
    let db = &fixture.db;

    let created = reader_annotations::create(
        db,
        "bk_test",
        &json!({
            "chapterIndex": 0,
            "startBlock": 1,
            "startOffset": 0,
            "endBlock": 1,
            "endOffset": 5,
            "quote": "  Hello  ",
            "selectedText": "Hello",
            "language": "fr",
            "color": "yellow",
            "note": "  ",
        }),
    )
    .expect("create");

    // The service answers with the DTO itself; the route adds `{annotation}`.
    assert_eq!(created["chapterTitle"], "Section 1");
    assert_eq!(created["quote"], "Hello");
    assert_eq!(created["note"], Value::Null);
    assert_eq!(created["startBlock"], 1);
    let id = created["id"].as_str().expect("id").to_string();

    let listed = reader_annotations::list(db, "bk_test").expect("list");
    let annotations = listed["annotations"].as_array().expect("array");
    assert_eq!(annotations.len(), 1);
    assert_eq!(annotations[0]["id"], id);

    let updated = reader_annotations::update(
        db,
        "bk_test",
        &id,
        &json!({ "color": "green", "note": "A note" }),
    )
    .expect("update");
    assert_eq!(updated["color"], "green");
    assert_eq!(updated["note"], "A note");

    reader_annotations::remove(db, "bk_test", &id).expect("remove");
    assert_eq!(
        reader_annotations::list(db, "bk_test").expect("list")["annotations"]
            .as_array()
            .expect("array")
            .len(),
        0
    );

    let missing = reader_annotations::remove(db, "bk_test", &id).expect_err("gone");
    assert_eq!(missing.status.as_u16(), 404);

    // A block index past the end heals to the last block instead of storing an
    // unreachable marker, exactly as the web service did.
    let clamped = reader_annotations::create(
        db,
        "bk_test",
        &json!({
            "chapterIndex": 0,
            "startBlock": 99,
            "startOffset": 9999,
            "endBlock": 99,
            "endOffset": 9999,
            "quote": "Second",
            "color": "violet",
        }),
    )
    .expect("clamped");
    assert_eq!(clamped["startBlock"], 2);
    assert_eq!(clamped["endBlock"], 2);
    assert_eq!(clamped["startOffset"], "Second line.".len() as i64);
    assert_eq!(clamped["endOffset"], "Second line.".len() as i64);

    let no_epub = reader_annotations::list(db, "bk_missing").expect_err("no epub");
    assert_eq!(no_epub.status.as_u16(), 404);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bookmarks_are_ordered_by_place_and_labelled_per_book() {
    let fixture = fixture(&["<p>One</p>", "<p>Two</p>"]).await;
    let db = &fixture.db;

    let later = reader_bookmarks::create(
        db,
        "bk_test",
        &json!({ "chapterIndex": 1, "scrollPercent": 0.75, "label": "Later" }),
    )
    .expect("create later");
    let earlier = reader_bookmarks::create(
        db,
        "bk_test",
        &json!({ "chapterIndex": 0, "scrollPercent": 0.25, "label": "Earlier" }),
    )
    .expect("create earlier");
    assert_eq!(later["chapterTitle"], "Section 2");
    assert_eq!(earlier["chapterTitle"], "Section 1");

    let listed = reader_bookmarks::list(db, "bk_test").expect("list");
    let bookmarks = listed["bookmarks"].as_array().expect("array");
    assert_eq!(bookmarks.len(), 2);
    assert_eq!(bookmarks[0]["label"], "Earlier");
    assert_eq!(bookmarks[1]["label"], "Later");

    let id = earlier["id"].as_str().expect("id").to_string();
    let renamed = reader_bookmarks::update(db, "bk_test", &id, &json!({ "label": null }))
        .expect("clear label");
    assert_eq!(renamed["label"], Value::Null);

    reader_bookmarks::remove(db, "bk_test", &id).expect("delete");
    assert_eq!(
        reader_bookmarks::list(db, "bk_test").expect("list")["bookmarks"]
            .as_array()
            .expect("array")
            .len(),
        1
    );

    let missing_chapter = reader_bookmarks::create(
        db,
        "bk_test",
        &json!({ "chapterIndex": 9, "scrollPercent": 0.5 }),
    )
    .expect_err("no section");
    assert_eq!(missing_chapter.status.as_u16(), 404);

    let bad_percent = reader_bookmarks::create(
        db,
        "bk_test",
        &json!({ "chapterIndex": 0, "scrollPercent": 2.5 }),
    )
    .expect_err("out of range");
    assert_eq!(bad_percent.status.as_u16(), 400);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn search_uses_the_translation_the_reader_is_reading() {
    let fixture = fixture(&[
        "<p>Café society</p><p>Other words</p>",
        "<p>Nothing here</p>",
    ])
    .await;
    let db = &fixture.db;

    let now = repo::now_iso();
    db.call({
        let now = now.clone();
        move |conn| {
            conn.execute(
                "INSERT INTO epub_translations \
                 (id, book_id, epub_index, language, language_name, blocks, title, \
                  char_count, source_hash, provider, created_at, updated_at) \
                 VALUES ('tr_test', 'bk_test', 0, 'es', 'Spanish', ?1, 'Section 1', \
                  24, 'stale', 'google', ?2, ?2)",
                rusqlite::params![
                    r#"[{"tag":"p","text":"La sociedad del café"},{"tag":"p","text":"Otras palabras"}]"#,
                    now
                ],
            )?;
            Ok::<(), rusqlite::Error>(())
        }
    })
    .await
    .expect("translation");

    let original = epub_search::search(
        db,
        "bk_test",
        &options("cafe", None, None, false, false, false),
    )
    .expect("search original");
    assert_eq!(original["total"], 1);
    assert_eq!(original["results"][0]["source"], "original");
    assert_eq!(original["results"][0]["excerpt"]["match"], "Café");

    // With diacritics required, unaccented "cafe" no longer matches "Café".
    let strict = epub_search::search(
        db,
        "bk_test",
        &options("cafe", None, None, false, true, false),
    )
    .expect("search strict");
    assert_eq!(strict["total"], 0);

    let translated = epub_search::search(
        db,
        "bk_test",
        &options("café", Some("es"), None, false, false, false),
    )
    .expect("search translation");
    assert_eq!(translated["total"], 1);
    assert_eq!(translated["results"][0]["source"], "translation");
    assert_eq!(translated["results"][0]["language"], "es");
    assert_eq!(translated["results"][0]["languageName"], "Spanish");
    assert_eq!(translated["results"][0]["chapterTitle"], "Section 1");

    let whole = epub_search::search(
        db,
        "bk_test",
        &options("caf", None, None, false, false, true),
    )
    .expect("whole word");
    assert_eq!(
        whole["total"], 0,
        "whole-word search must not match a fragment"
    );

    let capped = epub_search::search(
        db,
        "bk_test",
        &options("o", None, Some(1), false, false, false),
    )
    .expect("capped");
    assert_eq!(capped["results"].as_array().expect("results").len(), 1);
    assert!(capped["total"].as_i64().expect("total") > 1);
    assert_eq!(capped["truncated"], true);

    let no_epub = epub_search::search(
        db,
        "bk_missing",
        &options("anything", None, None, false, false, false),
    )
    .expect_err("no epub");
    assert_eq!(no_epub.status.as_u16(), 404);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn translations_put_images_back_at_their_source_position() {
    let fixture =
        fixture(&[r#"<p>Hello world.</p><img src="plate.jpg" alt="plate"/><p>Again.</p>"#]).await;
    let db = &fixture.db;
    let now = repo::now_iso();

    db.call({
        let now = now.clone();
        move |conn| {
            conn.execute(
                "INSERT INTO epub_translations \
                 (id, book_id, epub_index, language, language_name, blocks, title, \
                  char_count, source_hash, provider, created_at, updated_at) \
                 VALUES ('tr_es', 'bk_test', 0, 'es', 'Spanish', ?1, 'Section 1', \
                  20, 'stale', 'google', ?2, ?2)",
                rusqlite::params![
                    r#"[{"tag":"p","text":"Hola mundo."},{"tag":"p","text":"Otra vez."}]"#,
                    now
                ],
            )?;
            Ok::<(), rusqlite::Error>(())
        }
    })
    .await
    .expect("translation");

    let service = TranslationService::new(&fixture.config);
    let entry = service.get(db, "bk_test", 0, "es").expect("entry");

    assert_eq!(entry["languageName"], "Spanish");
    assert_eq!(entry["stale"], true);

    let blocks = entry["blocks"].as_array().expect("blocks");
    let shape: Vec<(&str, i64)> = blocks
        .iter()
        .map(|block| {
            (
                block["tag"].as_str().expect("tag"),
                block["sourceIndex"].as_i64().expect("sourceIndex"),
            )
        })
        .collect();
    assert_eq!(
        shape,
        vec![("p", 0), ("img", 1), ("p", 2)],
        "the plate returns between the two paragraphs, indexed against the source"
    );
    assert_eq!(blocks[1]["html"], r#"<img src="plate.jpg" alt="plate"/>"#);

    // A language with no saved row is a 404, not an empty translation.
    let missing = service.get(db, "bk_test", 0, "fr").expect_err("no row");
    assert_eq!(missing.status.as_u16(), 404);

    // The summary names the language the way the reader does.
    let listed = service.list(db, "bk_test", Some(0)).expect("list");
    assert_eq!(listed["entries"][0]["languageName"], "Spanish");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn language_detection_falls_back_to_the_epub_metadata() {
    let fixture = fixture(&["<p>Bonjour tout le monde.</p>"]).await;
    let mut config = fixture.config.clone();
    // An unreachable provider makes detection fail fast; the book's own
    // metadata, French, is the answer that remains.
    config.translation.base_url = "http://127.0.0.1:9/language".to_string();
    config.translation.timeout_ms = 200;
    let service = TranslationService::new(&config);

    let detection = service
        .detect(&fixture.db, "bk_test", 0)
        .await
        .expect("detect");
    assert_eq!(detection["source"], "metadata");
    assert_eq!(detection["language"], "fr");
    assert_eq!(detection["languageName"], "French");

    let missing = service
        .detect(&fixture.db, "bk_test", 99)
        .await
        .expect_err("no section");
    assert_eq!(missing.status.as_u16(), 404);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn saved_words_upsert_by_word_and_language_and_delete() {
    let fixture = fixture(&["<p>Text</p>"]).await;
    let db = &fixture.db;

    let body = json!({
        "word": "Ephemeral",
        "language": "en",
        "phonetic": " /ɪˈfɛm(ə)rəl/ ",
        "senses": [{
            "partOfSpeech": "adjective",
            "definitions": [{
                "definition": " Lasting for a very short time. ",
                "example": " ephemeral pleasures "
            }]
        }],
        "sourceName": "Test",
        "sourceUrl": "https://example.com/word",
    });
    let saved = dictionary::save_word(db, &body).expect("save");
    assert_eq!(saved["word"], "ephemeral");
    assert_eq!(saved["phonetic"], "/ɪˈfɛm(ə)rəl/");
    assert_eq!(
        saved["senses"][0]["definitions"][0]["definition"],
        "Lasting for a very short time."
    );
    let id = saved["id"].as_str().expect("id").to_string();

    // Saving the same word refreshes the entry instead of duplicating it.
    let mut changed = body.clone();
    changed["senses"][0]["definitions"][0]["definition"] = json!("Updated definition.");
    let refreshed = dictionary::save_word(db, &changed).expect("refresh");
    assert_eq!(refreshed["id"], id);
    assert_eq!(
        refreshed["senses"][0]["definitions"][0]["definition"],
        "Updated definition."
    );

    let listed = dictionary::list_saved(db).expect("list");
    assert_eq!(listed["words"].as_array().expect("words").len(), 1);

    let invalid =
        dictionary::save_word(db, &json!({ "word": "x", "language": "en", "senses": [] }))
            .expect_err("empty senses");
    assert_eq!(invalid.status.as_u16(), 400);

    dictionary::delete_saved(db, &id).expect("delete");
    assert_eq!(
        dictionary::list_saved(db).expect("list")["words"]
            .as_array()
            .expect("words")
            .len(),
        0
    );
    let gone = dictionary::delete_saved(db, &id).expect_err("gone");
    assert_eq!(gone.status.as_u16(), 404);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn speech_endpoints_reject_an_empty_request_without_calling_the_service() {
    let fixture = fixture(&["<p>Text</p>"]).await;
    let tts = TtsService::new(&fixture.config);

    let no_text = tts
        .speak(&fixture.db, &json!({}))
        .await
        .expect_err("no text");
    assert_eq!(no_text.status.as_u16(), 400);
    assert_eq!(no_text.message, "Choose some text to read aloud.");

    let too_long = tts
        .speak(&fixture.db, &json!({ "text": "x".repeat(1501) }))
        .await
        .expect_err("too long");
    assert_eq!(too_long.status.as_u16(), 400);

    let bad_language = tts
        .speak(
            &fixture.db,
            &json!({ "text": "hello", "language": "not a language!" }),
        )
        .await
        .expect_err("bad language");
    assert_eq!(bad_language.status.as_u16(), 400);

    let no_voice = tts.preview(&json!({})).await.expect_err("no voice");
    assert_eq!(no_voice.status.as_u16(), 400);
    assert_eq!(no_voice.message, "Choose a voice to preview.");
}

/// Translation naming is part of the reader's visible contract: English — the
/// language most sections are detected as — must not come back as `en`.
#[test]
fn language_names_cover_english_and_regional_variants() {
    assert_eq!(translation::translation_language_name("en"), "English");
    assert_eq!(
        translation::translation_language_name("pt-PT"),
        "Portuguese"
    );
    assert_eq!(translation::translation_language_name("sw-KE"), "Swahili");
    assert_eq!(translation::translation_language_name(""), "Unknown");
}
