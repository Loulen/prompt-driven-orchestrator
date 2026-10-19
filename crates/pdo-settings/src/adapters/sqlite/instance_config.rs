//! The stored tier over SQLite: the singleton `instance_config` row (`id = 1`)
//! of `pdo.db` — config and mutable state, not a canvas-backed artifact, hence
//! a table (CONTEXT.md *Persistence — table SQLite*).

use pdo_sandbox::domain::retired_settings::RETIRED_SANDBOX_IMAGE_COLUMNS;
use pdo_skill::domain::selection::{from_stored_json, normalise, to_stored_json};
use sqlx::{Row, SqlitePool};

use super::schema::has_column;
use crate::domain::instance_config::{InstanceConfig, UpdateInstanceConfig};

fn row_to_config(row: &sqlx::sqlite::SqliteRow) -> InstanceConfig {
    InstanceConfig {
        session_cap: row.get("session_cap"),
        reaper_ttl_secs: row.get("reaper_ttl_secs"),
        libassist_idle_ttl_secs: row.get("libassist_idle_ttl_secs"),
        guard_timeout_secs: row.get("guard_timeout_secs"),
        default_model: row.get("default_model"),
        default_sandbox: row.get("default_sandbox"),
        autocomplete_turn_end: row.get("autocomplete_turn_end"),
        default_auto_name: row.get("default_auto_name"),
        default_harness: row.get("default_harness"),
        // A JSON object in a TEXT column. NULL / empty / unparseable ⇒ an empty
        // map, so a fresh row and a corrupt one both fall through to env → floor.
        default_harness_model: row
            .get::<Option<String>, _>("default_harness_model")
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default(),
        auto_fail: row.get("auto_fail"),
        // `try_get` so a row read before the column migration still maps.
        update_check: row.try_get("update_check").unwrap_or(None),
        review_agent_can_resolve: row.try_get("review_agent_can_resolve").unwrap_or(None),
        // NULL / unparseable ⇒ `None`: the tier is transparent, never an error.
        agent_choice: row
            .get::<Option<String>, _>("agent_choice")
            .and_then(|s| serde_json::from_str(&s).ok()),
        // NULL / unparseable ⇒ empty selection, never an error.
        skills: from_stored_json(row.try_get::<Option<String>, _>("skills").unwrap_or(None)),
        manager_enabled: row.try_get("manager_enabled").unwrap_or(None),
        manager_profile: row.try_get("manager_profile").unwrap_or(None),
        max_attachments_mb: row.try_get("max_attachments_mb").unwrap_or(None),
        telemetry_otlp: row.try_get("telemetry_otlp").unwrap_or(None),
        telemetry_otlp_endpoint: row.try_get("telemetry_otlp_endpoint").unwrap_or(None),
        // NULL / unparseable ⇒ no header, never an error.
        telemetry_otlp_headers: row
            .try_get::<Option<String>, _>("telemetry_otlp_headers")
            .unwrap_or(None)
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default(),
        telemetry_prometheus: row.try_get("telemetry_prometheus").unwrap_or(None),
        telemetry_include_actor: row.try_get("telemetry_include_actor").unwrap_or(None),
        updated_at: row.get("updated_at"),
    }
}

/// The singleton row. Always present after `init`.
pub async fn get(db: &SqlitePool) -> Result<InstanceConfig, sqlx::Error> {
    let row = sqlx::query("SELECT * FROM instance_config WHERE id = 1")
        .fetch_optional(db)
        .await?;
    row.as_ref()
        .map(row_to_config)
        .ok_or(sqlx::Error::RowNotFound)
}

/// A value to bind into the `SET` clause.
enum Bound {
    Int(Option<i64>),
    Text(Option<String>),
}

/// `""` is the clear sentinel of a text knob: it stores SQL `NULL`, because a
/// stored `""` would win the precedence and shadow the env and the default.
fn text(value: String) -> Bound {
    Bound::Text((!value.is_empty()).then_some(value))
}

/// A checkbox stores `0`/`1`, never `NULL`: unticking must persist a stored
/// `0` that beats the env's `1`, not fall through to it.
fn flag(value: bool) -> Bound {
    Bound::Int(Some(i64::from(value)))
}

/// The columns an edit writes, in order, with their value.
fn assignments(edit: UpdateInstanceConfig) -> Vec<(&'static str, Bound)> {
    let mut sets = Vec::new();
    let mut int = |column, value: Option<i64>| {
        if let Some(v) = value {
            sets.push((column, Bound::Int(Some(v))));
        }
    };
    int("session_cap", edit.session_cap);
    int("reaper_ttl_secs", edit.reaper_ttl_secs);
    int("libassist_idle_ttl_secs", edit.libassist_idle_ttl_secs);
    int("guard_timeout_secs", edit.guard_timeout_secs);
    let texts = [
        ("default_model", edit.default_model),
        ("default_sandbox", edit.default_sandbox),
    ];
    sets.extend(texts.into_iter().filter_map(|(c, v)| Some((c, text(v?)))));
    let flags = [
        ("autocomplete_turn_end", edit.autocomplete_turn_end),
        ("default_auto_name", edit.default_auto_name),
    ];
    sets.extend(flags.into_iter().filter_map(|(c, v)| Some((c, flag(v?)))));
    if let Some(v) = edit.default_harness {
        sets.push(("default_harness", text(v)));
    }
    if let Some(v) = edit.default_harness_model {
        // An empty map stores `{}`: a real "no defaults" decision.
        let json = serde_json::to_string(&v).unwrap_or_else(|_| "{}".to_string());
        sets.push(("default_harness_model", Bound::Text(Some(json))));
    }
    let flags = [
        ("auto_fail", edit.auto_fail),
        ("update_check", edit.update_check),
        ("review_agent_can_resolve", edit.review_agent_can_resolve),
    ];
    sets.extend(flags.into_iter().filter_map(|(c, v)| Some((c, flag(v?)))));
    if let Some(v) = edit.agent_choice {
        // `Some(None)` clears back to the legacy default harness and model.
        let json = v.map(|c| serde_json::to_string(&c).unwrap_or_default());
        sets.push(("agent_choice", Bound::Text(json)));
    }
    if let Some(v) = edit.skills {
        // An empty selection stores NULL: "nothing selected" and "never set"
        // read the same, and the tier stays transparent.
        sets.push(("skills", Bound::Text(to_stored_json(&normalise(v)))));
    }
    if let Some(v) = edit.manager_enabled {
        sets.push(("manager_enabled", flag(v)));
    }
    if let Some(v) = edit.manager_profile {
        sets.push(("manager_profile", text(v)));
    }
    if let Some(v) = edit.max_attachments_mb {
        sets.push(("max_attachments_mb", Bound::Int(Some(v))));
    }
    if let Some(v) = edit.telemetry_otlp {
        sets.push(("telemetry_otlp", flag(v)));
    }
    if let Some(v) = edit.telemetry_otlp_endpoint {
        sets.push(("telemetry_otlp_endpoint", text(v.trim().to_string())));
    }
    if let Some(headers) = edit.telemetry_otlp_headers {
        // The use case resolved every kept value before the write; an empty
        // list stores NULL.
        let map: std::collections::BTreeMap<String, String> = headers
            .into_iter()
            .map(|h| (h.name, h.value.unwrap_or_default()))
            .collect();
        let json = (!map.is_empty()).then(|| serde_json::to_string(&map).unwrap_or_default());
        sets.push(("telemetry_otlp_headers", Bound::Text(json)));
    }
    if let Some(v) = edit.telemetry_prometheus {
        sets.push(("telemetry_prometheus", flag(v)));
    }
    if let Some(v) = edit.telemetry_include_actor {
        sets.push(("telemetry_include_actor", flag(v)));
    }
    sets
}

/// Apply a partial edit and return the row. A no-op edit (no field set)
/// returns the row unchanged and does not bump `updated_at`.
pub async fn update(
    db: &SqlitePool,
    edit: UpdateInstanceConfig,
) -> Result<InstanceConfig, sqlx::Error> {
    if edit.is_empty() {
        return get(db).await;
    }
    let sets = assignments(edit);
    let columns: Vec<String> = sets
        .iter()
        .map(|(column, _)| format!("{column} = ?"))
        .chain(std::iter::once("updated_at = ?".to_string()))
        .collect();
    let sql = format!(
        "UPDATE instance_config SET {} WHERE id = 1",
        columns.join(", ")
    );
    let mut query = sqlx::query(&sql);
    for (_, value) in sets {
        query = match value {
            Bound::Int(v) => query.bind(v),
            Bound::Text(v) => query.bind(v),
        };
    }
    query.bind(pdo_kernel::clock::now_iso()).execute(db).await?;
    get(db).await
}

/// The global Trigger pause (#348): `NULL`/`0` ≡ not paused, `1` ≡ paused. A
/// daemon-wide scheduler gate kept OUTSIDE the settings page — a kill-switch
/// has no env tier and no default other than "off" — that rides the singleton
/// row purely for storage.
pub async fn triggers_paused(db: &SqlitePool) -> Result<bool, sqlx::Error> {
    let v: Option<i64> =
        sqlx::query_scalar("SELECT triggers_paused FROM instance_config WHERE id = 1")
            .fetch_optional(db)
            .await?
            .flatten();
    Ok(v == Some(1))
}

/// Set (or clear) the pause. Unpausing stores `NULL` (≡ not paused), keeping
/// the "unset ≡ off" convention of the table. Bumps `updated_at`.
pub async fn set_triggers_paused(db: &SqlitePool, paused: bool) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE instance_config SET triggers_paused = ?, updated_at = ? WHERE id = 1")
        .bind(if paused { Some(1_i64) } else { None })
        .bind(pdo_kernel::clock::now_iso())
        .execute(db)
        .await?;
    Ok(())
}

/// Whatever is left in the columns #471 retired, so the boot can **say so
/// once**. Not two more fields of [`InstanceConfig`]: the point is precisely
/// that they are NOT config any more, and something would eventually consult
/// one.
///
/// PRAGMA-guarded per column: a database created before #471 HAS the columns
/// (`DROP COLUMN` on a live install buys nothing and can fail on old engines),
/// one created after does not — an absent column is silence, not an error.
/// Empty strings are skipped: `""` was the clear sentinel, never a value.
pub async fn retired_sandbox_values(
    db: &SqlitePool,
) -> Result<Vec<(&'static str, String)>, sqlx::Error> {
    let mut found = Vec::new();
    for (column, _) in RETIRED_SANDBOX_IMAGE_COLUMNS {
        if !has_column(db, column).await? {
            continue;
        }
        // The column name comes from the constant, never from a request.
        let value: Option<String> = sqlx::query_scalar(&format!(
            "SELECT {column} FROM instance_config WHERE id = 1"
        ))
        .fetch_optional(db)
        .await?
        .flatten();
        if let Some(v) = value.filter(|v| !v.is_empty()) {
            found.push((*column, v));
        }
    }
    Ok(found)
}
