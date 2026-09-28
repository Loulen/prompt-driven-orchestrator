//! Who selects a skill, read where each tier lives: the stored instance
//! settings, the Projets (the workspace's store), the Triggers (the trigger
//! context's store), the nodes of the instance's pipelines (on disk), and the
//! Runs not started yet (the run context's log) (#669).

use std::path::{Path, PathBuf};

use pdo_pipeline::domain::pipeline;
use pdo_run::application::paths::instance_pipeline_dir;
use pdo_skill::application::ports::{
    BoxFuture, NamedRef, PipelineReferent, Referents, RunReferent, SkillReferents, TriggerReferent,
};
use pdo_skill::domain::selection as skill_selection;
use tracing::error;

use super::project_tiers::project_skills;

/// The tiers of the daemon's database and repository root.
pub struct SkillTierReferents {
    pub db: sqlx::SqlitePool,
    pub repo_root: PathBuf,
}

impl SkillReferents for SkillTierReferents {
    fn referents<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Referents> {
        Box::pin(skill_referents(&self.db, &self.repo_root, id))
    }
}

async fn skill_referents(db: &sqlx::SqlitePool, repo_root: &Path, id: &str) -> Referents {
    let instance = crate::adapters::sqlite::instance_config::get(db)
        .await
        .map(|config| skill_selection::selects(&config.skills, id))
        .unwrap_or(false);

    let projects = pdo_workspace::adapters::sqlite::list(db)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|project| skill_selection::selects(&project_skills(project), id))
        .map(|project| NamedRef {
            id: project.id,
            name: project.name,
        })
        .collect();

    let triggers = pdo_trigger::adapters::sqlite::list(db)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|trigger| skill_selection::selects(&trigger.skills, id))
        .map(|trigger| TriggerReferent {
            id: trigger.id,
            name: trigger.name,
            pipeline_id: trigger.pipeline_id,
        })
        .collect();

    let pipeline_entries = instance_pipeline_dir(repo_root)
        .map(|directory| pdo_pipeline::adapters::registry::pipeline_files(&directory))
        .unwrap_or_default();
    let pipelines = pipeline_entries
        .into_iter()
        .flat_map(|entry| {
            let Ok(yaml) = std::fs::read_to_string(&entry.path) else {
                return Vec::new();
            };
            let Ok(parsed) = pipeline::parse_pipeline(&yaml) else {
                return Vec::new();
            };
            let name = parsed.pipeline.name.clone();
            parsed
                .pipeline
                .nodes
                .into_iter()
                .filter(|node| skill_selection::selects(&node.skills, id))
                .map(|node| PipelineReferent {
                    id: entry.id.clone(),
                    name: name.clone(),
                    node_id: node.id,
                    scope: "instance".to_string(),
                })
                .collect::<Vec<_>>()
        })
        .collect();

    // One indexed pass over `run_started` payloads: non-terminal Runs that have not
    // spawned a single node yet (same terminality test as the sandbox-profile
    // referents, plus the "no node_started" clause that makes them *not started*).
    let rows: Result<Vec<(String, String)>, _> = sqlx::query_as(
        "SELECT e.run_id, e.payload FROM events e \
         WHERE e.kind = 'run_started' AND e.payload IS NOT NULL \
           AND NOT EXISTS ( \
             SELECT 1 FROM events t WHERE t.run_id = e.run_id AND t.kind IN \
               ('run_completed', 'run_failed', 'run_skipped', 'run_halted', 'run_archived', \
                'node_started') \
           ) \
         ORDER BY e.ts DESC",
    )
    .fetch_all(db)
    .await;
    let runs = match rows {
        Ok(rows) => rows
            .iter()
            .filter_map(|(run_id, payload)| {
                let val = serde_json::from_str::<serde_json::Value>(payload).ok()?;
                let skills: Vec<skill_selection::SkillRef> =
                    serde_json::from_value(val.get("skills")?.clone()).ok()?;
                if !skill_selection::selects(&skills, id) {
                    return None;
                }
                let text = |key: &str| val.get(key).and_then(|v| v.as_str()).map(str::to_string);
                Some(RunReferent {
                    run_id: run_id.clone(),
                    pipeline_name: text("pipeline_name"),
                    name: text("name"),
                })
            })
            .collect(),
        Err(e) => {
            error!("failed to query runs for skill referents: {e}");
            Vec::new()
        }
    };

    Referents {
        instance,
        projects,
        triggers,
        pipelines,
        runs,
    }
}
