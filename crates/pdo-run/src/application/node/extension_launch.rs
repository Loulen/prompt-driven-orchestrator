//! What the extensions contribute to a Node's launch (API 0.8): the variables
//! added to its environment and the text placed at the top of its preamble,
//! asked once per launch with who created the Run and in which Projet.

use pdo_extension_api::{NodeKind, NodeLaunch, TriggerProvenance};
use pdo_kernel::extension_host::NodeContribution;

use crate::application::paths::effective_repo_root;
use crate::domain::state::RunState;
use crate::Runs;

/// The name of the variable that moves Claude Code's configuration folder,
/// hence where its transcripts are written (`$CLAUDE_CONFIG_DIR/projects/`).
pub const CLAUDE_CONFIG_DIR: &str = "CLAUDE_CONFIG_DIR";

/// What the extensions add to the launch of iteration `iter` of `node_id`, a
/// `kind` Node of `run`. The variables the Run's sandbox profile sets are the
/// core's: they are dropped here (the session drops the others it sets).
pub async fn contribution(
    runs: &Runs,
    run: &RunState,
    node_id: &str,
    iter: i64,
    kind: NodeKind,
) -> NodeContribution {
    let mut contribution = runs
        .extensions()
        .node_launch(&node_launch(runs, run, node_id, iter, kind).await);
    if let Some(profile_env) = &run.sandbox_env {
        contribution.env.retain(|name, _| {
            let taken = profile_env.contains_key(name);
            if taken {
                tracing::warn!(
                    "an extension's variable `{name}` is the sandbox's: not overwritten"
                );
            }
            !taken
        });
    }
    contribution
}

/// The launch as the extensions see it.
pub async fn node_launch(
    runs: &Runs,
    run: &RunState,
    node_id: &str,
    iter: i64,
    kind: NodeKind,
) -> NodeLaunch {
    let repository = effective_repo_root(runs, run)
        .to_string_lossy()
        .into_owned();
    let project = runs.projects.owner_of(&repository).await.ok().flatten();
    let mut launch = NodeLaunch::new(&run.run_id, node_id, iter, kind, repository);
    if let Some(actor) = &run.created_by {
        launch = launch.created_by(actor.clone());
    }
    if let Some(trigger_id) = &run.triggered_by {
        launch = launch.fired_by(TriggerProvenance::new(
            trigger_id,
            run.trigger_editor.clone(),
        ));
    }
    if let Some(project) = project {
        launch = launch.in_project(project.id);
    }
    launch
}

/// The kind of a pipeline node, as the launch hooks name it.
pub fn node_kind(node_type: &pdo_pipeline::domain::pipeline::NodeType) -> NodeKind {
    if *node_type == pdo_pipeline::domain::pipeline::NodeType::Script {
        NodeKind::Script
    } else {
        NodeKind::Agent
    }
}

/// The variables as the session takes them, in name order.
pub fn env_pairs(contribution: &NodeContribution) -> Vec<(String, String)> {
    contribution
        .env
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}
