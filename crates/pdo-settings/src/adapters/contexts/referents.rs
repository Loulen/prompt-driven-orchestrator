//! Who names an agentic profile, read where each tier lives: the Projets (the
//! workspace's store), the Triggers (the trigger context's store) and the
//! nodes of the instance's pipelines (on disk).

use std::path::PathBuf;

use pdo_harness::agent_choice::AgentChoice;
use pdo_pipeline::domain::pipeline::parse_pipeline;
use pdo_run::application::paths::instance_pipeline_dir;
use sqlx::SqlitePool;

use crate::application::ports::{BoxFuture, ProfileReferents};
use crate::domain::referents::{names_profile, PipelineReferent, ProjectReferent, TriggerReferent};

pub struct ContextReferents {
    pub db: SqlitePool,
    pub repo_root: PathBuf,
}

impl ProfileReferents for ContextReferents {
    fn projects<'a>(&'a self, profile_id: &'a str) -> BoxFuture<'a, Vec<ProjectReferent>> {
        Box::pin(async move {
            pdo_workspace::adapters::sqlite::list(&self.db)
                .await
                .unwrap_or_default()
                .into_iter()
                .filter(|project| {
                    let choice = project.agent_choice.as_ref().and_then(|stored| {
                        serde_json::from_str::<AgentChoice>(stored.as_str()).ok()
                    });
                    names_profile(choice.as_ref(), profile_id)
                })
                .map(|project| ProjectReferent {
                    id: project.id,
                    name: project.name,
                })
                .collect()
        })
    }

    fn triggers<'a>(&'a self, profile_id: &'a str) -> BoxFuture<'a, Vec<TriggerReferent>> {
        Box::pin(async move {
            pdo_trigger::adapters::sqlite::list(&self.db)
                .await
                .unwrap_or_default()
                .into_iter()
                .filter(|trigger| names_profile(trigger.agent_choice.as_ref(), profile_id))
                .map(|trigger| TriggerReferent {
                    id: trigger.id,
                    name: trigger.name,
                    pipeline_id: trigger.pipeline_id,
                })
                .collect()
        })
    }

    fn pipelines(&self, profile_id: &str) -> Vec<PipelineReferent> {
        let entries = instance_pipeline_dir(&self.repo_root)
            .map(|directory| pdo_pipeline::adapters::registry::pipeline_files(&directory))
            .unwrap_or_default();
        entries
            .into_iter()
            .flat_map(|entry| {
                let Ok(yaml) = std::fs::read_to_string(&entry.path) else {
                    return Vec::new();
                };
                let Ok(parsed) = parse_pipeline(&yaml) else {
                    return Vec::new();
                };
                let name = parsed.pipeline.name.clone();
                parsed
                    .pipeline
                    .nodes
                    .into_iter()
                    .filter(|node| names_profile(node.agent_choice.as_ref(), profile_id))
                    .map(|node| PipelineReferent {
                        id: entry.id.clone(),
                        name: name.clone(),
                        node_id: node.id,
                        scope: "instance".to_string(),
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }
}
