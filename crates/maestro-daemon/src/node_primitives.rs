use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::Serialize;

use crate::event_log::{self, EventKind, NodeStatus};
use crate::pipeline::{self, PipelineDef};
use crate::{blackboard, tmux_session_manager};

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PrimitiveOutcome {
    Executed,
    AlreadyDone,
    Rejected { reason: String },
}

// ---------------------------------------------------------------------------
// start_node
// ---------------------------------------------------------------------------

pub struct StartNodeParams<'a> {
    pub run_id: &'a str,
    pub node_id: &'a str,
    pub iter: i64,
    pub overrides: Option<HashMap<String, PathBuf>>,
    pub pipeline: &'a PipelineDef,
    pub run_state: &'a event_log::RunState,
    pub artifacts_dir: &'a Path,
    pub worktree_dir: &'a Path,
    pub repo_root: &'a Path,
    pub pipeline_path: &'a Path,
    pub resolved_vars: &'a HashMap<String, serde_yaml::Value>,
    pub daemon_port: u16,
}

pub struct StartNodeResult {
    pub outcome: PrimitiveOutcome,
    pub events: Vec<event_log::Event>,
}

pub fn start_node(params: &StartNodeParams<'_>) -> StartNodeResult {
    let node = match params
        .pipeline
        .nodes
        .iter()
        .find(|n| n.id == params.node_id)
    {
        Some(n) => n,
        None => {
            return StartNodeResult {
                outcome: PrimitiveOutcome::Rejected {
                    reason: format!("node '{}' not found in pipeline", params.node_id),
                },
                events: vec![],
            }
        }
    };

    if has_node_started_event(params.run_state, params.node_id, params.iter) {
        return StartNodeResult {
            outcome: PrimitiveOutcome::AlreadyDone,
            events: vec![],
        };
    }

    let input_paths = resolve_inputs(params, node);

    let has_sub_worktree = node.node_type == pipeline::NodeType::CodeMutating
        || node.node_type == pipeline::NodeType::Merge;

    let working_dir = if has_sub_worktree {
        let sub_wt_dir =
            sub_worktree_path(params.repo_root, params.run_id, params.node_id, params.iter);
        let sub_branch = sub_worktree_branch(params.run_id, params.node_id, params.iter);
        let pipeline_branch = format!("maestro/run-{}", params.run_id);

        if let Err(e) =
            create_sub_worktree(params.repo_root, &sub_wt_dir, &sub_branch, &pipeline_branch)
        {
            return StartNodeResult {
                outcome: PrimitiveOutcome::Rejected {
                    reason: format!("failed to create sub-worktree: {e}"),
                },
                events: vec![],
            };
        }
        sub_wt_dir
    } else {
        params.worktree_dir.to_path_buf()
    };

    let canonical_path = pipeline::canonical_prompt_path(params.pipeline_path, params.node_id);
    let role_prompt = std::fs::read_to_string(&canonical_path).unwrap_or_default();

    let aug_ctx = crate::prompt_augmenter::AugmentContext {
        pipeline: params.pipeline,
        node,
        run_id: params.run_id,
        iter: params.iter,
        artifacts_dir: params.artifacts_dir,
        variables: params.resolved_vars,
        daemon_url: &format!("http://localhost:{}", params.daemon_port),
        foreach_context: None,
        source_worktree_dir: has_sub_worktree.then_some(working_dir.as_path()),
        input_images: Vec::new(),
    };

    let full_prompt = crate::prompt_augmenter::build_full_prompt(&aug_ctx, &role_prompt);

    let node_started = event_log::Event {
        id: None,
        run_id: params.run_id.to_string(),
        ts: event_log::now_iso(),
        kind: EventKind::NodeStarted,
        node_id: Some(params.node_id.to_string()),
        iter: Some(params.iter),
        payload: Some(serde_json::json!({
            "prompt_preview": full_prompt.chars().take(500).collect::<String>(),
            "node_type": node_type_str(&node.node_type),
            "input_paths": input_paths,
        })),
    };

    let session_name =
        tmux_session_manager::node_session_name(params.run_id, params.node_id, params.iter);
    if let Err(e) = tmux_session_manager::spawn(
        &session_name,
        &full_prompt,
        &working_dir,
        params.run_id,
        params.node_id,
        params.iter,
        params.daemon_port,
    ) {
        return StartNodeResult {
            outcome: PrimitiveOutcome::Rejected {
                reason: format!("failed to spawn tmux session: {e}"),
            },
            events: vec![],
        };
    }

    let mut events = vec![node_started];

    if node.interactive {
        events.push(event_log::Event {
            id: None,
            run_id: params.run_id.to_string(),
            ts: event_log::now_iso(),
            kind: EventKind::NodeAwaitingUser,
            node_id: Some(params.node_id.to_string()),
            iter: Some(params.iter),
            payload: None,
        });
    }

    StartNodeResult {
        outcome: PrimitiveOutcome::Executed,
        events,
    }
}

fn has_node_started_event(run_state: &event_log::RunState, node_id: &str, iter: i64) -> bool {
    if let Some(node) = run_state.nodes.get(node_id) {
        if node.iter == iter && node.status != NodeStatus::Pending {
            return true;
        }
        if node
            .iterations
            .iter()
            .any(|it| it.iter == iter && it.status != NodeStatus::Pending)
        {
            return true;
        }
    }
    false
}

fn resolve_inputs(
    params: &StartNodeParams<'_>,
    node: &pipeline::NodeDef,
) -> HashMap<String, String> {
    let mut input_paths = HashMap::new();

    for input_port in &node.inputs {
        if let Some(override_path) = params
            .overrides
            .as_ref()
            .and_then(|o| o.get(&input_port.name))
        {
            input_paths.insert(
                input_port.name.clone(),
                override_path.to_string_lossy().to_string(),
            );
            continue;
        }

        let matching_edge = params
            .pipeline
            .edges
            .iter()
            .find(|e| e.target.node == params.node_id && e.target.port == input_port.name);

        if let Some(edge) = matching_edge {
            let resolved = if input_port.repeated {
                let source_dir = params.artifacts_dir.join(&edge.source.node);
                format!(
                    "{}/iter-*/{}/output.md",
                    source_dir.to_string_lossy(),
                    edge.source.port
                )
            } else {
                let source_iter = latest_iter_for_node(params.run_state, &edge.source.node);
                blackboard::artifact_path(
                    params.artifacts_dir,
                    &edge.source.node,
                    source_iter,
                    &edge.source.port,
                )
                .to_string_lossy()
                .to_string()
            };
            input_paths.insert(input_port.name.clone(), resolved);
        } else if input_port.name == "task" {
            let path = blackboard::input_path(params.artifacts_dir);
            input_paths.insert(input_port.name.clone(), path.to_string_lossy().to_string());
        }
    }

    input_paths
}

fn latest_iter_for_node(run_state: &event_log::RunState, node_id: &str) -> i64 {
    run_state.nodes.get(node_id).map(|n| n.iter).unwrap_or(1)
}

fn node_type_str(nt: &pipeline::NodeType) -> &'static str {
    match nt {
        pipeline::NodeType::DocOnly => "doc-only",
        pipeline::NodeType::CodeMutating => "code-mutating",
        pipeline::NodeType::Start => "start",
        pipeline::NodeType::End => "end",
        pipeline::NodeType::Switch => "switch",
        pipeline::NodeType::Loop => "loop",
        pipeline::NodeType::ForEach => "for-each",
        pipeline::NodeType::Merge => "merge",
    }
}

// ---------------------------------------------------------------------------
// stop_node
// ---------------------------------------------------------------------------

pub struct StopNodeParams<'a> {
    pub run_id: &'a str,
    pub node_id: &'a str,
    pub iter: i64,
    pub tmux_socket: &'a str,
}

pub struct StopNodeResult {
    pub outcome: PrimitiveOutcome,
    pub events: Vec<event_log::Event>,
}

pub fn stop_node(params: &StopNodeParams<'_>) -> StopNodeResult {
    let session_name =
        tmux_session_manager::node_session_name(params.run_id, params.node_id, params.iter);

    tmux_session_manager::kill(params.tmux_socket, &session_name);

    let stopped_event = event_log::Event {
        id: None,
        run_id: params.run_id.to_string(),
        ts: event_log::now_iso(),
        kind: EventKind::NodeStopped,
        node_id: Some(params.node_id.to_string()),
        iter: Some(params.iter),
        payload: Some(serde_json::json!({
            "reason": "stopped_by_user",
        })),
    };

    StopNodeResult {
        outcome: PrimitiveOutcome::Executed,
        events: vec![stopped_event],
    }
}

// ---------------------------------------------------------------------------
// invalidate_nodes
// ---------------------------------------------------------------------------

pub struct InvalidateNodesParams<'a> {
    pub run_id: &'a str,
    pub node_ids: &'a [String],
    pub artifacts_dir: &'a Path,
}

pub struct InvalidateNodesResult {
    pub outcome: PrimitiveOutcome,
    pub events: Vec<event_log::Event>,
    pub deleted_dirs: Vec<PathBuf>,
}

pub fn invalidate_nodes(params: &InvalidateNodesParams<'_>) -> InvalidateNodesResult {
    if params.node_ids.is_empty() {
        return InvalidateNodesResult {
            outcome: PrimitiveOutcome::Executed,
            events: vec![],
            deleted_dirs: vec![],
        };
    }

    let mut events = Vec::new();
    let mut deleted_dirs = Vec::new();

    for node_id in params.node_ids {
        events.push(event_log::Event {
            id: None,
            run_id: params.run_id.to_string(),
            ts: event_log::now_iso(),
            kind: EventKind::NodeInvalidated,
            node_id: Some(node_id.clone()),
            iter: None,
            payload: Some(serde_json::json!({
                "reason": "invalidated",
            })),
        });

        let artifact_dir = params.artifacts_dir.join(node_id);
        if artifact_dir.exists() {
            if let Err(e) = std::fs::remove_dir_all(&artifact_dir) {
                tracing::warn!("failed to remove artifacts for {node_id}: {e}");
            } else {
                deleted_dirs.push(artifact_dir);
            }
        }
    }

    InvalidateNodesResult {
        outcome: PrimitiveOutcome::Executed,
        events,
        deleted_dirs,
    }
}

// ---------------------------------------------------------------------------
// inject_outputs
// ---------------------------------------------------------------------------

pub struct InjectOutputsParams<'a> {
    pub node_id: &'a str,
    pub iter: i64,
    pub artifacts: &'a HashMap<String, String>,
    pub artifacts_dir: &'a Path,
}

pub struct InjectOutputsResult {
    pub outcome: PrimitiveOutcome,
    pub written_paths: Vec<PathBuf>,
}

pub fn inject_outputs(params: &InjectOutputsParams<'_>) -> InjectOutputsResult {
    if params.artifacts.is_empty() {
        return InjectOutputsResult {
            outcome: PrimitiveOutcome::Executed,
            written_paths: vec![],
        };
    }

    let mut written_paths = Vec::new();

    for (port_name, content) in params.artifacts {
        let port_d =
            blackboard::port_dir(params.artifacts_dir, params.node_id, params.iter, port_name);
        if let Err(e) = std::fs::create_dir_all(&port_d) {
            return InjectOutputsResult {
                outcome: PrimitiveOutcome::Rejected {
                    reason: format!("failed to create port directory for {port_name}: {e}"),
                },
                written_paths,
            };
        }

        let file_path = port_d.join("output.md");
        if let Err(e) = std::fs::write(&file_path, content) {
            return InjectOutputsResult {
                outcome: PrimitiveOutcome::Rejected {
                    reason: format!("failed to write artifact for {port_name}: {e}"),
                },
                written_paths,
            };
        }

        written_paths.push(file_path);
    }

    InjectOutputsResult {
        outcome: PrimitiveOutcome::Executed,
        written_paths,
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn sub_worktree_path(repo_root: &Path, run_id: &str, node_id: &str, iter: i64) -> PathBuf {
    repo_root
        .join(".maestro")
        .join("runs")
        .join(run_id)
        .join("nodes")
        .join(node_id)
        .join(format!("iter-{iter}"))
}

fn sub_worktree_branch(run_id: &str, node_id: &str, iter: i64) -> String {
    format!("maestro/sub-{run_id}-{node_id}-iter-{iter}")
}

fn create_sub_worktree(
    repo_root: &Path,
    sub_worktree_dir: &Path,
    sub_branch: &str,
    base_branch: &str,
) -> Result<()> {
    std::fs::create_dir_all(sub_worktree_dir.parent().unwrap_or(Path::new(".")))?;

    let output = std::process::Command::new("git")
        .args(["worktree", "add", "-b", sub_branch])
        .arg(sub_worktree_dir)
        .arg(base_branch)
        .current_dir(repo_root)
        .output()
        .map_err(|e| anyhow::anyhow!("failed to run git worktree add for sub-worktree: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git worktree add (sub) failed: {stderr}");
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_log::{IterationInfo, NodeState, RunState};
    use crate::pipeline::{EdgeDef, EdgeEndpoint, NodeDef, NodeType, Port, PortType};
    use pretty_assertions::assert_eq;
    use std::collections::HashMap;

    fn make_node(id: &str, node_type: NodeType, inputs: &[&str], outputs: &[&str]) -> NodeDef {
        NodeDef {
            id: id.into(),
            name: id.into(),
            node_type,
            inputs: inputs
                .iter()
                .map(|n| Port {
                    name: (*n).into(),
                    repeated: false,
                    side: None,
                    port_type: PortType::Markdown,
                    frontmatter: None,
                    when: None,
                    description: None,
                })
                .collect(),
            outputs: outputs
                .iter()
                .map(|n| Port {
                    name: (*n).into(),
                    repeated: false,
                    side: None,
                    port_type: PortType::Markdown,
                    frontmatter: None,
                    when: None,
                    description: None,
                })
                .collect(),
            interactive: false,
            view: None,
            max_iter: None,
            over: None,
        }
    }

    fn make_node_with_repeated_input(id: &str, port_name: &str) -> NodeDef {
        NodeDef {
            id: id.into(),
            name: id.into(),
            node_type: NodeType::DocOnly,
            inputs: vec![Port {
                name: port_name.into(),
                repeated: true,
                side: None,
                port_type: PortType::Markdown,
                frontmatter: None,
                when: None,
                description: None,
            }],
            outputs: vec![Port {
                name: "out".into(),
                repeated: false,
                side: None,
                port_type: PortType::Markdown,
                frontmatter: None,
                when: None,
                description: None,
            }],
            interactive: false,
            view: None,
            max_iter: None,
            over: None,
        }
    }

    fn make_edge(src_node: &str, src_port: &str, tgt_node: &str, tgt_port: &str) -> EdgeDef {
        EdgeDef {
            source: EdgeEndpoint {
                node: src_node.into(),
                port: src_port.into(),
            },
            target: EdgeEndpoint {
                node: tgt_node.into(),
                port: tgt_port.into(),
            },
            reason: None,
        }
    }

    fn empty_run_state() -> RunState {
        RunState::new("run-1".into(), "test".into())
    }

    fn running_node(id: &str, iter: i64) -> NodeState {
        NodeState {
            node_id: id.into(),
            status: NodeStatus::Running,
            iter,
            started_at: Some("t0".into()),
            completed_at: None,
            failure_reason: None,
            iterations: vec![IterationInfo {
                iter,
                status: NodeStatus::Running,
                started_at: Some("t0".into()),
                completed_at: None,
            }],
            frontmatter_retries: 0,
            frontmatter_violations: Vec::new(),
        }
    }

    fn completed_node(id: &str, iter: i64) -> NodeState {
        NodeState {
            node_id: id.into(),
            status: NodeStatus::Completed,
            iter,
            started_at: Some("t0".into()),
            completed_at: Some("t1".into()),
            failure_reason: None,
            iterations: vec![IterationInfo {
                iter,
                status: NodeStatus::Completed,
                started_at: Some("t0".into()),
                completed_at: Some("t1".into()),
            }],
            frontmatter_retries: 0,
            frontmatter_violations: Vec::new(),
        }
    }

    fn pending_node(id: &str) -> NodeState {
        NodeState {
            node_id: id.into(),
            status: NodeStatus::Pending,
            iter: 1,
            started_at: None,
            completed_at: None,
            failure_reason: None,
            iterations: Vec::new(),
            frontmatter_retries: 0,
            frontmatter_violations: Vec::new(),
        }
    }

    // -----------------------------------------------------------------------
    // start_node — idempotency
    // -----------------------------------------------------------------------

    #[test]
    fn start_node_already_started_returns_already_done() {
        let pipeline = PipelineDef {
            name: "test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![make_node("worker", NodeType::DocOnly, &["task"], &["out"])],
            edges: vec![],
        };

        let mut run_state = empty_run_state();
        run_state
            .nodes
            .insert("worker".into(), running_node("worker", 1));

        let tmp = tempfile::tempdir().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();

        let params = StartNodeParams {
            run_id: "run-1",
            node_id: "worker",
            iter: 1,
            overrides: None,
            pipeline: &pipeline,
            run_state: &run_state,
            artifacts_dir: &artifacts_dir,
            worktree_dir: tmp.path(),
            repo_root: tmp.path(),
            pipeline_path: &tmp.path().join("pipeline.yaml"),
            resolved_vars: &HashMap::new(),
            daemon_port: 5172,
        };

        let result = start_node(&params);
        assert_eq!(result.outcome, PrimitiveOutcome::AlreadyDone);
        assert!(result.events.is_empty());
    }

    #[test]
    fn start_node_unknown_node_returns_rejected() {
        let pipeline = PipelineDef {
            name: "test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![],
            edges: vec![],
        };

        let run_state = empty_run_state();
        let tmp = tempfile::tempdir().unwrap();

        let params = StartNodeParams {
            run_id: "run-1",
            node_id: "nonexistent",
            iter: 1,
            overrides: None,
            pipeline: &pipeline,
            run_state: &run_state,
            artifacts_dir: &tmp.path().join("artifacts"),
            worktree_dir: tmp.path(),
            repo_root: tmp.path(),
            pipeline_path: &tmp.path().join("pipeline.yaml"),
            resolved_vars: &HashMap::new(),
            daemon_port: 5172,
        };

        let result = start_node(&params);
        assert!(matches!(result.outcome, PrimitiveOutcome::Rejected { .. }));
    }

    // -----------------------------------------------------------------------
    // start_node — input resolution
    // -----------------------------------------------------------------------

    #[test]
    fn start_node_resolves_inputs_from_blackboard() {
        let pipeline = PipelineDef {
            name: "test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("planner", NodeType::DocOnly, &["task"], &["plan"]),
                make_node("implementer", NodeType::DocOnly, &["plan"], &["summary"]),
            ],
            edges: vec![make_edge("planner", "plan", "implementer", "plan")],
        };

        let mut run_state = empty_run_state();
        run_state
            .nodes
            .insert("planner".into(), completed_node("planner", 1));

        let tmp = tempfile::tempdir().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");

        let plan_dir = artifacts_dir.join("planner").join("iter-1").join("plan");
        std::fs::create_dir_all(&plan_dir).unwrap();
        std::fs::write(plan_dir.join("output.md"), "# Plan\nDo the thing").unwrap();

        let node = pipeline
            .nodes
            .iter()
            .find(|n| n.id == "implementer")
            .unwrap();
        let params = StartNodeParams {
            run_id: "run-1",
            node_id: "implementer",
            iter: 1,
            overrides: None,
            pipeline: &pipeline,
            run_state: &run_state,
            artifacts_dir: &artifacts_dir,
            worktree_dir: tmp.path(),
            repo_root: tmp.path(),
            pipeline_path: &tmp.path().join("pipeline.yaml"),
            resolved_vars: &HashMap::new(),
            daemon_port: 5172,
        };

        let input_paths = resolve_inputs(&params, node);
        let plan_path = blackboard::artifact_path(&artifacts_dir, "planner", 1, "plan");
        assert_eq!(
            input_paths.get("plan").unwrap(),
            &plan_path.to_string_lossy().to_string()
        );
    }

    #[test]
    fn start_node_with_overrides_uses_override_path() {
        let pipeline = PipelineDef {
            name: "test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("planner", NodeType::DocOnly, &["task"], &["plan"]),
                make_node("implementer", NodeType::DocOnly, &["plan"], &["summary"]),
            ],
            edges: vec![make_edge("planner", "plan", "implementer", "plan")],
        };

        let run_state = empty_run_state();
        let tmp = tempfile::tempdir().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();

        let override_path = tmp.path().join("custom_plan.md");
        std::fs::write(&override_path, "# Custom plan").unwrap();

        let mut overrides = HashMap::new();
        overrides.insert("plan".to_string(), override_path.clone());

        let node = pipeline
            .nodes
            .iter()
            .find(|n| n.id == "implementer")
            .unwrap();
        let params = StartNodeParams {
            run_id: "run-1",
            node_id: "implementer",
            iter: 1,
            overrides: Some(overrides),
            pipeline: &pipeline,
            run_state: &run_state,
            artifacts_dir: &artifacts_dir,
            worktree_dir: tmp.path(),
            repo_root: tmp.path(),
            pipeline_path: &tmp.path().join("pipeline.yaml"),
            resolved_vars: &HashMap::new(),
            daemon_port: 5172,
        };

        let input_paths = resolve_inputs(&params, node);
        assert_eq!(
            input_paths.get("plan").unwrap(),
            &override_path.to_string_lossy().to_string()
        );
    }

    #[test]
    fn start_node_resolves_task_port_from_input_artifact() {
        let pipeline = PipelineDef {
            name: "test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![make_node("entry", NodeType::DocOnly, &["task"], &["out"])],
            edges: vec![],
        };

        let run_state = empty_run_state();
        let tmp = tempfile::tempdir().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();

        let node = pipeline.nodes.iter().find(|n| n.id == "entry").unwrap();
        let params = StartNodeParams {
            run_id: "run-1",
            node_id: "entry",
            iter: 1,
            overrides: None,
            pipeline: &pipeline,
            run_state: &run_state,
            artifacts_dir: &artifacts_dir,
            worktree_dir: tmp.path(),
            repo_root: tmp.path(),
            pipeline_path: &tmp.path().join("pipeline.yaml"),
            resolved_vars: &HashMap::new(),
            daemon_port: 5172,
        };

        let input_paths = resolve_inputs(&params, node);
        let expected = blackboard::input_path(&artifacts_dir);
        assert_eq!(
            input_paths.get("task").unwrap(),
            &expected.to_string_lossy().to_string()
        );
    }

    #[test]
    fn start_node_resolves_fan_in_inputs() {
        let pipeline = PipelineDef {
            name: "test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("planner", NodeType::DocOnly, &["task"], &["plan"]),
                make_node("researcher", NodeType::DocOnly, &["task"], &["research"]),
                make_node(
                    "implementer",
                    NodeType::DocOnly,
                    &["plan", "research"],
                    &["summary"],
                ),
            ],
            edges: vec![
                make_edge("planner", "plan", "implementer", "plan"),
                make_edge("researcher", "research", "implementer", "research"),
            ],
        };

        let mut run_state = empty_run_state();
        run_state
            .nodes
            .insert("planner".into(), completed_node("planner", 1));
        run_state
            .nodes
            .insert("researcher".into(), completed_node("researcher", 1));

        let tmp = tempfile::tempdir().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();

        let node = pipeline
            .nodes
            .iter()
            .find(|n| n.id == "implementer")
            .unwrap();
        let params = StartNodeParams {
            run_id: "run-1",
            node_id: "implementer",
            iter: 1,
            overrides: None,
            pipeline: &pipeline,
            run_state: &run_state,
            artifacts_dir: &artifacts_dir,
            worktree_dir: tmp.path(),
            repo_root: tmp.path(),
            pipeline_path: &tmp.path().join("pipeline.yaml"),
            resolved_vars: &HashMap::new(),
            daemon_port: 5172,
        };

        let input_paths = resolve_inputs(&params, node);
        assert!(input_paths.contains_key("plan"));
        assert!(input_paths.contains_key("research"));
    }

    #[test]
    fn start_node_resolves_repeated_port_with_glob() {
        let pipeline = PipelineDef {
            name: "test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("reviewer", NodeType::DocOnly, &["task"], &["review"]),
                make_node_with_repeated_input("implementer", "reviews"),
            ],
            edges: vec![make_edge("reviewer", "review", "implementer", "reviews")],
        };

        let run_state = empty_run_state();
        let tmp = tempfile::tempdir().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();

        let node = pipeline
            .nodes
            .iter()
            .find(|n| n.id == "implementer")
            .unwrap();
        let params = StartNodeParams {
            run_id: "run-1",
            node_id: "implementer",
            iter: 1,
            overrides: None,
            pipeline: &pipeline,
            run_state: &run_state,
            artifacts_dir: &artifacts_dir,
            worktree_dir: tmp.path(),
            repo_root: tmp.path(),
            pipeline_path: &tmp.path().join("pipeline.yaml"),
            resolved_vars: &HashMap::new(),
            daemon_port: 5172,
        };

        let input_paths = resolve_inputs(&params, node);
        let reviews_path = input_paths.get("reviews").unwrap();
        assert!(reviews_path.contains("iter-*"));
        assert!(reviews_path.contains("review"));
    }

    #[test]
    fn start_node_uses_latest_iter_for_upstream() {
        let pipeline = PipelineDef {
            name: "test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("reviewer", NodeType::DocOnly, &["task"], &["review"]),
                make_node("implementer", NodeType::DocOnly, &["review"], &["summary"]),
            ],
            edges: vec![make_edge("reviewer", "review", "implementer", "review")],
        };

        let mut run_state = empty_run_state();
        run_state
            .nodes
            .insert("reviewer".into(), completed_node("reviewer", 3));

        let tmp = tempfile::tempdir().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();

        let node = pipeline
            .nodes
            .iter()
            .find(|n| n.id == "implementer")
            .unwrap();
        let params = StartNodeParams {
            run_id: "run-1",
            node_id: "implementer",
            iter: 1,
            overrides: None,
            pipeline: &pipeline,
            run_state: &run_state,
            artifacts_dir: &artifacts_dir,
            worktree_dir: tmp.path(),
            repo_root: tmp.path(),
            pipeline_path: &tmp.path().join("pipeline.yaml"),
            resolved_vars: &HashMap::new(),
            daemon_port: 5172,
        };

        let input_paths = resolve_inputs(&params, node);
        let review_path = input_paths.get("review").unwrap();
        assert!(
            review_path.contains("iter-3"),
            "should resolve to iter-3 (latest), got: {review_path}"
        );
    }

    // -----------------------------------------------------------------------
    // stop_node
    // -----------------------------------------------------------------------

    #[test]
    fn stop_node_emits_node_stopped_event() {
        let params = StopNodeParams {
            run_id: "run-1",
            node_id: "worker",
            iter: 1,
            tmux_socket: "maestro-test",
        };

        let result = stop_node(&params);
        assert_eq!(result.outcome, PrimitiveOutcome::Executed);
        assert_eq!(result.events.len(), 1);

        let event = &result.events[0];
        assert_eq!(event.kind, EventKind::NodeStopped);
        assert_eq!(event.node_id.as_deref(), Some("worker"));
        assert_eq!(event.iter, Some(1));

        let payload = event.payload.as_ref().unwrap();
        assert_eq!(payload["reason"], "stopped_by_user");
    }

    #[test]
    fn stop_node_does_not_trigger_scheduler() {
        let params = StopNodeParams {
            run_id: "run-1",
            node_id: "worker",
            iter: 1,
            tmux_socket: "maestro-test",
        };

        let result = stop_node(&params);
        assert_eq!(result.outcome, PrimitiveOutcome::Executed);
        for event in &result.events {
            assert_ne!(event.kind, EventKind::RunCompleted);
            assert_ne!(event.kind, EventKind::RunFailed);
        }
    }

    // -----------------------------------------------------------------------
    // invalidate_nodes
    // -----------------------------------------------------------------------

    #[test]
    fn invalidate_nodes_resets_to_pending_and_deletes_artifacts() {
        let tmp = tempfile::tempdir().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");

        let node_a_dir = artifacts_dir.join("node-a").join("iter-1").join("out");
        std::fs::create_dir_all(&node_a_dir).unwrap();
        std::fs::write(node_a_dir.join("output.md"), "# Output A").unwrap();

        let node_b_dir = artifacts_dir.join("node-b").join("iter-1").join("out");
        std::fs::create_dir_all(&node_b_dir).unwrap();
        std::fs::write(node_b_dir.join("output.md"), "# Output B").unwrap();

        let params = InvalidateNodesParams {
            run_id: "run-1",
            node_ids: &["node-a".to_string(), "node-b".to_string()],
            artifacts_dir: &artifacts_dir,
        };

        let result = invalidate_nodes(&params);
        assert_eq!(result.outcome, PrimitiveOutcome::Executed);
        assert_eq!(result.events.len(), 2);

        for event in &result.events {
            assert_eq!(event.kind, EventKind::NodeInvalidated);
        }

        assert!(!artifacts_dir.join("node-a").exists());
        assert!(!artifacts_dir.join("node-b").exists());
        assert_eq!(result.deleted_dirs.len(), 2);
    }

    #[test]
    fn invalidate_nodes_empty_list_is_noop() {
        let tmp = tempfile::tempdir().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();

        let params = InvalidateNodesParams {
            run_id: "run-1",
            node_ids: &[],
            artifacts_dir: &artifacts_dir,
        };

        let result = invalidate_nodes(&params);
        assert_eq!(result.outcome, PrimitiveOutcome::Executed);
        assert!(result.events.is_empty());
        assert!(result.deleted_dirs.is_empty());
    }

    #[test]
    fn invalidate_already_pending_node_still_emits_stale() {
        let tmp = tempfile::tempdir().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();

        let params = InvalidateNodesParams {
            run_id: "run-1",
            node_ids: &["clean-node".to_string()],
            artifacts_dir: &artifacts_dir,
        };

        let result = invalidate_nodes(&params);
        assert_eq!(result.outcome, PrimitiveOutcome::Executed);
        assert_eq!(result.events.len(), 1);
        assert_eq!(result.events[0].kind, EventKind::NodeInvalidated);
        assert!(result.deleted_dirs.is_empty());
    }

    // -----------------------------------------------------------------------
    // inject_outputs
    // -----------------------------------------------------------------------

    #[test]
    fn inject_outputs_writes_files_to_port_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();

        let mut artifacts = HashMap::new();
        artifacts.insert(
            "review".to_string(),
            "---\nverdict: PASS\n---\n\nLooks good.".to_string(),
        );
        artifacts.insert("summary".to_string(), "# Summary\nAll done.".to_string());

        let params = InjectOutputsParams {
            node_id: "reviewer",
            iter: 2,
            artifacts: &artifacts,
            artifacts_dir: &artifacts_dir,
        };

        let result = inject_outputs(&params);
        assert_eq!(result.outcome, PrimitiveOutcome::Executed);
        assert_eq!(result.written_paths.len(), 2);

        let review_path = blackboard::artifact_path(&artifacts_dir, "reviewer", 2, "review");
        assert!(review_path.exists());
        let content = std::fs::read_to_string(&review_path).unwrap();
        assert!(content.contains("verdict: PASS"));

        let summary_path = blackboard::artifact_path(&artifacts_dir, "reviewer", 2, "summary");
        assert!(summary_path.exists());
        let content = std::fs::read_to_string(&summary_path).unwrap();
        assert!(content.contains("All done"));
    }

    #[test]
    fn inject_outputs_empty_artifacts_is_noop() {
        let tmp = tempfile::tempdir().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();

        let artifacts = HashMap::new();

        let params = InjectOutputsParams {
            node_id: "reviewer",
            iter: 1,
            artifacts: &artifacts,
            artifacts_dir: &artifacts_dir,
        };

        let result = inject_outputs(&params);
        assert_eq!(result.outcome, PrimitiveOutcome::Executed);
        assert!(result.written_paths.is_empty());
    }

    #[test]
    fn inject_outputs_overwrites_existing_artifact() {
        let tmp = tempfile::tempdir().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");

        let port_dir = artifacts_dir.join("worker").join("iter-1").join("out");
        std::fs::create_dir_all(&port_dir).unwrap();
        std::fs::write(port_dir.join("output.md"), "old content").unwrap();

        let mut artifacts = HashMap::new();
        artifacts.insert("out".to_string(), "new content".to_string());

        let params = InjectOutputsParams {
            node_id: "worker",
            iter: 1,
            artifacts: &artifacts,
            artifacts_dir: &artifacts_dir,
        };

        let result = inject_outputs(&params);
        assert_eq!(result.outcome, PrimitiveOutcome::Executed);

        let content = std::fs::read_to_string(blackboard::artifact_path(
            &artifacts_dir,
            "worker",
            1,
            "out",
        ))
        .unwrap();
        assert_eq!(content, "new content");
    }

    // -----------------------------------------------------------------------
    // Idempotency edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn double_start_returns_already_done() {
        let pipeline = PipelineDef {
            name: "test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![make_node("worker", NodeType::DocOnly, &["task"], &["out"])],
            edges: vec![],
        };

        let mut run_state = empty_run_state();
        run_state
            .nodes
            .insert("worker".into(), completed_node("worker", 1));

        let tmp = tempfile::tempdir().unwrap();
        let params = StartNodeParams {
            run_id: "run-1",
            node_id: "worker",
            iter: 1,
            overrides: None,
            pipeline: &pipeline,
            run_state: &run_state,
            artifacts_dir: &tmp.path().join("artifacts"),
            worktree_dir: tmp.path(),
            repo_root: tmp.path(),
            pipeline_path: &tmp.path().join("pipeline.yaml"),
            resolved_vars: &HashMap::new(),
            daemon_port: 5172,
        };

        let result = start_node(&params);
        assert_eq!(result.outcome, PrimitiveOutcome::AlreadyDone);
    }

    #[test]
    fn start_node_pending_status_allows_start() {
        let mut run_state = empty_run_state();
        run_state
            .nodes
            .insert("worker".into(), pending_node("worker"));

        let result = has_node_started_event(&run_state, "worker", 1);
        assert!(!result, "pending node should be startable");
    }
}
