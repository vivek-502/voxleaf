//! Durable playback preferences, read and written as one row.

use rusqlite::Row;
use serde_json::{json, Value};

use crate::db::Database;
use crate::error::{ApiError, ApiResult};
use crate::repo::now_iso;
use crate::models::AppSettingsInfo;

const SELECT: &str = "playback_speed, theme, volume, muted, sleep_timer_preference, \
     skip_forward_seconds, skip_backward_seconds, completion_threshold, auto_advance, \
     bookmark_note, tts_voice";

fn map(row: &Row<'_>) -> rusqlite::Result<AppSettingsInfo> {
    Ok(AppSettingsInfo {
        playback_speed: row.get(0)?,
        theme: row.get(1)?,
        volume: row.get(2)?,
        muted: row.get::<_, i64>(3)? != 0,
        sleep_timer_preference: row.get(4)?,
        skip_forward_seconds: row.get(5)?,
        skip_backward_seconds: row.get(6)?,
        completion_threshold: row.get(7)?,
        auto_advance: row.get::<_, i64>(8)? != 0,
        bookmark_note: row.get(9)?,
        tts_voice: row.get(10)?,
    })
}

/// The singleton settings row, created on first read so a fresh install never
/// 404s on `/settings`.
pub async fn get(db: &Database) -> ApiResult<AppSettingsInfo> {
    let settings = db.call(move |conn| {
        conn.execute(
            "INSERT OR IGNORE INTO app_settings (id, created_at, updated_at) VALUES ('singleton', ?1, ?2)",
            [now_iso(), now_iso()],
        )?;
        conn.query_row(
            &format!("SELECT {SELECT} FROM app_settings WHERE id = 'singleton'"),
            [],
            map,
        )
    })
    .await?;

    Ok(settings)
}

/// Which settings the client is allowed to write, and how. The web app's
/// `updateSettingsSchema` bounds every field; this is that schema, expressed as
/// a merge rather than a struct so unknown keys are ignored, not fatal.
pub async fn update(db: &Database, body: &Value) -> ApiResult<AppSettingsInfo> {
    let object = body
        .as_object()
        .ok_or_else(|| ApiError::invalid("body", "Expected a JSON object."))?;

    // (column, kind) — kinds carry their own bounds, matching the Zod schema.
    let mut assignments: Vec<String> = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::ToSql + Send>> = Vec::new();

    let mut push = |column: &str, value: Box<dyn rusqlite::ToSql + Send>| {
        assignments.push(format!("{column} = ?{}", params.len() + 1));
        params.push(value);
    };

    if let Some(v) = object.get("playbackSpeed").and_then(|v| v.as_f64()) {
        if (0.25..=4.0).contains(&v) {
            push("playback_speed", Box::new(v));
        }
    }
    if let Some(v) = object.get("theme").and_then(|v| v.as_str()) {
        if matches!(v, "dark" | "light" | "system") {
            push("theme", Box::new(v.to_string()));
        }
    }
    if let Some(v) = object.get("volume").and_then(|v| v.as_f64()) {
        if (0.0..=1.0).contains(&v) {
            push("volume", Box::new(v));
        }
    }
    if let Some(v) = object.get("muted").and_then(|v| v.as_bool()) {
        push("muted", Box::new(v as i64));
    }
    if let Some(v) = object.get("sleepTimerPreference").and_then(|v| v.as_str()) {
        if crate::services::tts::SLEEP_TIMER_IDS.contains(&v) {
            push("sleep_timer_preference", Box::new(v.to_string()));
        }
    }
    if let Some(v) = object.get("skipForwardSeconds").and_then(|v| v.as_i64()) {
        if (5..=120).contains(&v) {
            push("skip_forward_seconds", Box::new(v));
        }
    }
    if let Some(v) = object.get("skipBackwardSeconds").and_then(|v| v.as_i64()) {
        if (5..=120).contains(&v) {
            push("skip_backward_seconds", Box::new(v));
        }
    }
    if let Some(v) = object.get("completionThreshold").and_then(|v| v.as_f64()) {
        if (0.5..=0.999).contains(&v) {
            push("completion_threshold", Box::new(v));
        }
    }
    if let Some(v) = object.get("autoAdvance").and_then(|v| v.as_bool()) {
        push("auto_advance", Box::new(v as i64));
    }
    if let Some(v) = object.get("bookmarkNote").and_then(|v| v.as_str()) {
        push("bookmark_note", Box::new(v.chars().take(2000).collect::<String>()));
    }
    if let Some(v) = object.get("ttsVoice").and_then(|v| v.as_str()) {
        let trimmed = v.trim();
        if !trimmed.is_empty() && trimmed.chars().count() <= 200 {
            push("tts_voice", Box::new(trimmed.to_string()));
        }
    }

    if assignments.is_empty() {
        return get(db).await;
    }

    let sql = format!(
        "UPDATE app_settings SET {}, updated_at = ?{} WHERE id = 'singleton'",
        assignments.join(", "),
        params.len() + 1
    );
    params.push(Box::new(now_iso()));

    db.call(move |conn| {
        let refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref() as _).collect();
        conn.execute(&sql, rusqlite::params_from_iter(refs))?;
        conn.query_row(
            &format!("SELECT {SELECT} FROM app_settings WHERE id = 'singleton'"),
            [],
            map,
        )
    })
    .await?;

    get(db).await
}

/// The wire shape the frontend expects.
pub fn to_json(settings: &AppSettingsInfo) -> Value {
    json!({
        "playbackSpeed": settings.playback_speed,
        "theme": settings.theme,
        "volume": settings.volume,
        "muted": settings.muted,
        "sleepTimerPreference": settings.sleep_timer_preference,
        "skipForwardSeconds": settings.skip_forward_seconds,
        "skipBackwardSeconds": settings.skip_backward_seconds,
        "completionThreshold": settings.completion_threshold,
        "autoAdvance": settings.auto_advance,
        "bookmarkNote": settings.bookmark_note,
        "ttsVoice": settings.tts_voice,
    })
}
