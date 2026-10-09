//! What the extensions contribute to the launch of a Node (API 0.8) and of the
//! Run's Pipeline Manager (API 0.13): the variables added to its environment
//! and the text placed at the top of its prompt, asked once per launch with who
//! starts the session, who created the Run and in which Projet.

use pdo_extension_api::{
    Actor, NodeKind, NodeLaunch, SessionKind, SessionLaunch, SessionRef, TriggerProvenance,
};
use pdo_kernel::extension_host::NodeContribution;
use pdo_session::domain::names::{manager_session_name, node_session_name};

use crate::application::paths::effective_repo_root;
use crate::domain::state::RunState;
use crate::Runs;

/// The name of the variable that moves Claude Code's configuration folder,
/// hence where its transcripts are written (`$CLAUDE_CONFIG_DIR/projects/`).
pub const CLAUDE_CONFIG_DIR: &str = "CLAUDE_CONFIG_DIR";

/// What the extensions add to the launch of iteration `iter` of `node_id`, a
/// `kind` Node of `run`, started by the Run's creator. The variables the Run's
/// sandbox profile sets are the core's: they are dropped here (the session
/// drops the others it sets).
pub async fn contribution(
    runs: &Runs,
    run: &RunState,
    node_id: &str,
    iter: i64,
    kind: NodeKind,
) -> NodeContribution {
    let launch = session_launch(runs, run, node_id, iter, kind).await;
    without_sandbox_names(runs.extensions().session_launch(&launch), run)
}

/// What the extensions add to the launch of `run`'s Pipeline Manager, started
/// by `started_by` (API 0.13).
pub async fn manager_contribution(
    runs: &Runs,
    run: &RunState,
    started_by: Option<Actor>,
) -> NodeContribution {
    let (_, project) = repository_and_project(runs, run).await;
    let mut launch = SessionLaunch::new(
        SessionRef::new(manager_session_name(&run.run_id), SessionKind::Manager)
            .in_run(&run.run_id),
    )
    .in_projects(project);
    if let Some(actor) = started_by {
        launch = launch.started_by(actor);
    }
    without_sandbox_names(runs.extensions().session_launch(&launch), run)
}

/// The launch of a Node's session as the extensions see it: the Node's
/// launch, started by the Run's creator, in the Projet of its repository.
pub async fn session_launch(
    runs: &Runs,
    run: &RunState,
    node_id: &str,
    iter: i64,
    kind: NodeKind,
) -> SessionLaunch {
    let node = node_launch(runs, run, node_id, iter, kind).await;
    let mut launch = SessionLaunch::new(
        SessionRef::new(
            node_session_name(&run.run_id, node_id, iter),
            SessionKind::Node,
        )
        .in_run(&run.run_id)
        .of_node(node_id, iter),
    )
    .in_projects(node.project.clone());
    if let Some(actor) = &run.created_by {
        launch = launch.started_by(actor.clone());
    }
    launch.of_node(node)
}

/// `contribution` without the variables `run`'s sandbox profile sets.
fn without_sandbox_names(mut contribution: NodeContribution, run: &RunState) -> NodeContribution {
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
    let (repository, project) = repository_and_project(runs, run).await;
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
        launch = launch.in_project(project);
    }
    launch
}

/// The Run's primary repository, and the id of the Projet owning it if one
/// does.
async fn repository_and_project(runs: &Runs, run: &RunState) -> (String, Option<String>) {
    let repository = effective_repo_root(runs, run)
        .to_string_lossy()
        .into_owned();
    let project = runs
        .projects
        .owner_of(&repository)
        .await
        .ok()
        .flatten()
        .map(|project| project.id);
    (repository, project)
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
