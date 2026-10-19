//! A partial edit of a Trigger, as one `UPDATE`.

use sqlx::SqlitePool;

use pdo_skill::domain::selection;

use crate::domain::trigger::UpdateTrigger;

/// Apply a partial config edit to a Trigger. A no-op when no field is set.
pub async fn update(
    db: &SqlitePool,
    trigger_id: &str,
    edit: UpdateTrigger,
) -> Result<(), sqlx::Error> {
    if edit.is_empty() {
        return Ok(());
    }

    // Bind in the SAME order the SET clause is built, so the positional
    // placeholders line up.
    let mut sets: Vec<&str> = Vec::new();
    if edit.name.is_some() {
        sets.push("name = ?");
    }
    if edit.pipeline_id.is_some() {
        sets.push("pipeline_id = ?");
    }
    if edit.pipeline_name.is_some() {
        sets.push("pipeline_name = ?");
    }
    if edit.target_repo.is_some() {
        sets.push("target_repo = ?");
    }
    if edit.project_id.is_some() {
        sets.push("project_id = ?");
    }
    if edit.target_repos.is_some() {
        sets.push("target_repos = ?");
    }
    if edit.source_branch.is_some() {
        sets.push("source_branch = ?");
    }
    if edit.input_template.is_some() {
        sets.push("input_template = ?");
    }
    if edit.variables.is_some() {
        sets.push("variables = ?");
    }
    if edit.cron.is_some() {
        sets.push("cron = ?");
    }
    if edit.guard_command.is_some() {
        sets.push("guard_command = ?");
    }
    if edit.overlap_policy.is_some() {
        sets.push("overlap_policy = ?");
    }
    if edit.max_concurrent.is_some() {
        sets.push("max_concurrent = ?");
    }
    if edit.sandbox.is_some() {
        sets.push("sandbox = ?");
    }
    if edit.harness.is_some() {
        sets.push("harness = ?");
    }
    if edit.agent_choice.is_some() {
        sets.push("agent_choice = ?");
    }
    if edit.skills.is_some() {
        sets.push("skills = ?");
    }
    if edit.auto_name.is_some() {
        sets.push("auto_name = ?");
    }
    if edit.next_fire_at.is_some() {
        sets.push("next_fire_at = ?");
    }
    if edit.enabled.is_some() {
        sets.push("enabled = ?");
    }
    if edit.last_editor.is_some() {
        sets.push("last_editor = ?");
    }

    let sql = format!("UPDATE triggers SET {} WHERE id = ?", sets.join(", "));
    let mut query = sqlx::query(&sql);
    if let Some(v) = &edit.name {
        query = query.bind(v.clone());
    }
    if let Some(v) = &edit.pipeline_id {
        query = query.bind(v.clone());
    }
    if let Some(v) = &edit.pipeline_name {
        query = query.bind(v.clone());
    }
    if let Some(v) = &edit.target_repo {
        query = query.bind(v.clone());
    }
    if let Some(v) = &edit.project_id {
        query = query.bind(v.clone());
    }
    if let Some(v) = &edit.target_repos {
        query = query.bind(v.clone());
    }
    if let Some(v) = &edit.source_branch {
        query = query.bind(v.clone());
    }
    if let Some(v) = &edit.input_template {
        query = query.bind(v.clone());
    }
    if let Some(v) = &edit.variables {
        query = query.bind(serde_json::to_string(v).unwrap_or_else(|_| "{}".to_string()));
    }
    if let Some(v) = &edit.cron {
        query = query.bind(v.clone());
    }
    if let Some(v) = &edit.guard_command {
        query = query.bind(v.clone());
    }
    if let Some(v) = &edit.overlap_policy {
        query = query.bind(v.clone());
    }
    if let Some(v) = &edit.max_concurrent {
        query = query.bind(*v);
    }
    if let Some(v) = &edit.sandbox {
        query = query.bind(v.clone());
    }
    if let Some(v) = &edit.harness {
        query = query.bind(v.clone());
    }
    if let Some(v) = &edit.agent_choice {
        query = query.bind(
            v.as_ref()
                .map(|c| serde_json::to_string(c).unwrap_or_default()),
        );
    }
    if let Some(v) = &edit.skills {
        query = query.bind(selection::to_stored_json(&selection::normalise(v.clone())));
    }
    if let Some(v) = &edit.auto_name {
        // Never NULL: the column is `NOT NULL DEFAULT 1`.
        query = query.bind(if *v { 1_i64 } else { 0_i64 });
    }
    if let Some(v) = &edit.next_fire_at {
        query = query.bind(v.clone());
    }
    if let Some(v) = &edit.enabled {
        query = query.bind(if *v { 1 } else { 0 });
    }
    if let Some(v) = &edit.last_editor {
        query = query.bind(super::stored_actor(Some(v)));
    }
    query = query.bind(trigger_id);
    query.execute(db).await?;
    Ok(())
}
