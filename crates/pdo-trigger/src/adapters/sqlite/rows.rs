//! Reading a row back into a Trigger or a fire.

use pdo_skill::domain::selection;
use sqlx::Row;

use crate::domain::trigger::{Trigger, TriggerFire};

pub(super) fn row_to_trigger(row: &sqlx::sqlite::SqliteRow) -> Trigger {
    let variables_str: String = row.get("variables");
    let variables = serde_json::from_str(&variables_str).unwrap_or(serde_json::json!({}));
    Trigger {
        id: row.get("id"),
        name: row.get("name"),
        pipeline_id: row.get("pipeline_id"),
        pipeline_name: row.get("pipeline_name"),
        target_repo: row.get("target_repo"),
        project_id: row.try_get("project_id").unwrap_or(None),
        // Tolerant of a legacy-NULL / pre-migration read: absence fires mono-repo.
        target_repos: row.try_get("target_repos").unwrap_or(None),
        source_branch: row.get("source_branch"),
        input_template: row.get("input_template"),
        variables,
        cron: row.get("cron"),
        guard_command: row.get("guard_command"),
        overlap_policy: row.get("overlap_policy"),
        max_concurrent: row.get("max_concurrent"),
        sandbox: row.get("sandbox"),
        // Absence (legacy NULL) inherits the instance default at fire time.
        harness: row.try_get("harness").unwrap_or(None),
        // Absence, or an unparseable value, inherits the legacy `harness` tier.
        agent_choice: row
            .try_get::<Option<String>, _>("agent_choice")
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str(&s).ok()),
        // #669: absence (legacy NULL) or an unparseable value adds no skill.
        skills: selection::from_stored_json(
            row.try_get::<Option<String>, _>("skills").unwrap_or(None),
        ),
        // A legacy NULL reads as auto-name ON.
        auto_name: row.try_get::<i64, _>("auto_name").unwrap_or(1) != 0,
        enabled: row.get::<i64, _>("enabled") != 0,
        next_fire_at: row.get("next_fire_at"),
        last_fired_at: row.get("last_fired_at"),
        last_outcome: row.get("last_outcome"),
        attributes: Default::default(),
        // Absence (legacy NULL), or an unreadable value, is no known editor.
        last_editor: row
            .try_get::<Option<String>, _>("last_editor")
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str(&s).ok()),
        suspension: None,
    }
}

pub(super) fn row_to_fire(row: &sqlx::sqlite::SqliteRow) -> TriggerFire {
    TriggerFire {
        id: row.get("id"),
        trigger_id: row.get("trigger_id"),
        ts: row.get("ts"),
        outcome: row.get("outcome"),
        reason: row.get("reason"),
        run_id: row.get("run_id"),
        guard_stdout: row.get("guard_stdout"),
        guard_stderr: row.get("guard_stderr"),
        guard_exit_code: row.get("guard_exit_code"),
        source: row.get("source"),
    }
}
