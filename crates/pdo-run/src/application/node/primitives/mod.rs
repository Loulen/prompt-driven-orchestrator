//! The node primitives (ADR-0009, layer 2): start, stop, invalidate, inject
//! outputs — atomic, no scheduling.

use crate::domain::event;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use pdo_session::application::ports::Tmux;

use crate::domain::blackboard;
use crate::domain::event::EventKind;
use pdo_pipeline::domain::pipeline::PipelineDef;

mod start;
mod start_parts;
mod start_prep;

pub use start::start_node;
pub use start_parts::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PrimitiveOutcome {
    Executed,
    AlreadyDone,
    Rejected { reason: String },
}

pub struct StopNodeParams<'a> {
    pub run_id: &'a str,
    pub node_id: &'a str,
    pub iter: i64,
    pub tmux: &'a dyn Tmux,
}

#[allow(dead_code)]
pub struct StopNodeResult {
    pub outcome: PrimitiveOutcome,
    pub events: Vec<event::Event>,
}

pub fn stop_node(params: &StopNodeParams<'_>) -> StopNodeResult {
    let session_name =
        pdo_session::domain::names::node_session_name(params.run_id, params.node_id, params.iter);

    params.tmux.kill(&session_name);

    let stopped_event = event::Event {
        id: None,
        run_id: params.run_id.to_string(),
        ts: pdo_kernel::clock::now_iso(),
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

pub struct InvalidateNodesParams<'a> {
    pub run_id: &'a str,
    pub node_ids: &'a [String],
    pub artifacts_dir: &'a Path,
}

#[allow(dead_code)]
pub struct InvalidateNodesResult {
    pub outcome: PrimitiveOutcome,
    pub events: Vec<event::Event>,
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
        events.push(event::Event {
            id: None,
            run_id: params.run_id.to_string(),
            ts: pdo_kernel::clock::now_iso(),
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

pub struct InjectOutputsParams<'a> {
    pub node_id: &'a str,
    pub iter: i64,
    pub artifacts: &'a HashMap<String, String>,
    pub artifacts_dir: &'a Path,
}

#[allow(dead_code)]
pub struct InjectOutputsResult {
    pub outcome: PrimitiveOutcome,
    pub written_paths: Vec<PathBuf>,
}

#[allow(dead_code)]
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

/// Deposits a *skipped* node's outputs so a downstream resolver finds a concrete
/// (if empty) artifact rather than a missing file that reads as "not produced".
/// The port set is every declared ordinary output port plus every distinct source
/// port the node's outgoing edges read; a node with neither still gets a single
/// default `output` port. An *Output intermédiaire* gets nothing — an empty file
/// would read as content — and an override naming one is ignored.
pub fn write_skip_outputs(
    pipeline: &PipelineDef,
    node_id: &str,
    iter: i64,
    overrides: &HashMap<String, String>,
    artifacts_dir: &Path,
) -> Result<Vec<String>, String> {
    let mut ports: Vec<String> = Vec::new();
    if let Some(node) = pipeline.nodes.iter().find(|n| n.id == node_id) {
        for out in node.outputs.iter().filter(|port| !port.intermediate) {
            if !ports.contains(&out.name) {
                ports.push(out.name.clone());
            }
        }
    }
    // Every port an outgoing edge CARRIES needs a skip artifact, not just the
    // primary one: a multi-port edge (ADR-0073) resolves one input per port
    // downstream, so skipping the node must leave one file per port behind.
    for e in pipeline.edges.iter().filter(|e| e.source.node == node_id) {
        for port in &e.source.ports {
            if !ports.contains(port) {
                ports.push(port.clone());
            }
        }
    }
    if ports.is_empty() {
        ports.push("output".to_string());
    }

    let artifacts: HashMap<String, String> = ports
        .iter()
        .map(|port| {
            (
                port.clone(),
                overrides.get(port).cloned().unwrap_or_default(),
            )
        })
        .collect();
    let inject = inject_outputs(&InjectOutputsParams {
        node_id,
        iter,
        artifacts: &artifacts,
        artifacts_dir,
    });
    match inject.outcome {
        PrimitiveOutcome::Rejected { reason } => Err(reason),
        _ => Ok(ports),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::state::IterationInfo;
    use crate::domain::state::NodeState;
    use crate::domain::state::NodeStatus;
    use crate::domain::state::RunState;
    use pdo_pipeline::domain::pipeline::{
        EdgeDef, EdgeEndpoint, EdgeSource, NodeDef, NodeType, Port, PortType,
    };
    use pretty_assertions::assert_eq;
    use std::collections::HashMap;

    fn make_node(id: &str, node_type: NodeType, inputs: &[&str], outputs: &[&str]) -> NodeDef {
        NodeDef {
            skills: Vec::new(),
            // #653: these fixtures exercise graph wiring, not worktrees — a
            // shared-worktree node keeps them out of `git worktree add` on a
            // scratch dir. Isolation-sensitive tests state `Some(true)`.
            isolated_worktree: Some(false),
            id: id.into(),
            name: id.into(),
            node_type,
            inputs: inputs
                .iter()
                .map(|n| Port {
                    name: (*n).into(),
                    port_type: PortType::Markdown,
                    frontmatter: None,
                    when: None,
                    description: None,
                    instructions: None,
                    required: false,
                    intermediate: false,
                })
                .collect(),
            outputs: outputs
                .iter()
                .map(|n| Port {
                    name: (*n).into(),
                    port_type: PortType::Markdown,
                    frontmatter: None,
                    when: None,
                    description: None,
                    instructions: None,
                    required: false,
                    intermediate: false,
                })
                .collect(),
            interactive: false,
            view: None,
            max_iter: None,
            over: None,
            pin_harness: None,
            harnesses: Default::default(),
            agent_choice: None,
            auto_fail: None,
            orchestrator: false,
            on_demand: false,
        }
    }

    fn make_node_with_repeated_input(id: &str, port_name: &str) -> NodeDef {
        NodeDef {
            skills: Vec::new(),
            isolated_worktree: None,
            id: id.into(),
            name: id.into(),
            node_type: NodeType::Agent,
            inputs: vec![Port {
                name: port_name.into(),
                port_type: PortType::Markdown,
                frontmatter: None,
                when: None,
                description: None,
                instructions: None,
                required: false,
                intermediate: false,
            }],
            outputs: vec![Port {
                name: "out".into(),
                port_type: PortType::Markdown,
                frontmatter: None,
                when: None,
                description: None,
                instructions: None,
                required: false,
                intermediate: false,
            }],
            interactive: false,
            view: None,
            max_iter: None,
            over: None,
            pin_harness: None,
            harnesses: Default::default(),
            agent_choice: None,
            auto_fail: None,
            orchestrator: false,
            on_demand: false,
        }
    }

    fn make_edge(src_node: &str, src_port: &str, tgt_node: &str, tgt_port: &str) -> EdgeDef {
        EdgeDef {
            source: EdgeSource::single(src_node, src_port),
            target: EdgeEndpoint {
                node: tgt_node.into(),
                port: tgt_port.into(),
            },
            reason: None,
            when: None,
            is_else: false,
            repeated: false,
            ..Default::default()
        }
    }

    fn empty_run_state() -> RunState {
        RunState::new("run-1".into(), "test".into())
    }

    fn running_node(id: &str, iter: i64) -> NodeState {
        NodeState {
            missing_skills: Vec::new(),
            skipped_skills: Vec::new(),
            skills: None,
            isolated_worktree: None,
            harness: None,
            cost: None,
            node_id: id.into(),
            status: NodeStatus::Running,
            iter,
            started_at: Some("t0".into()),
            completed_at: None,
            failure_reason: None,
            skip_reason: None,
            iterations: vec![IterationInfo {
                iter,
                status: NodeStatus::Running,
                started_at: Some("t0".into()),
                completed_at: None,
                interactive: false,
                completion_released: false,
            }],
            frontmatter_retries: 0,
            frontmatter_violations: Vec::new(),
            missing_outputs: Vec::new(),
            delivery: None,
            awaiting: None,
        }
    }

    fn completed_node(id: &str, iter: i64) -> NodeState {
        NodeState {
            missing_skills: Vec::new(),
            skipped_skills: Vec::new(),
            skills: None,
            isolated_worktree: None,
            harness: None,
            cost: None,
            node_id: id.into(),
            status: NodeStatus::Completed,
            iter,
            started_at: Some("t0".into()),
            completed_at: Some("t1".into()),
            failure_reason: None,
            skip_reason: None,
            iterations: vec![IterationInfo {
                iter,
                status: NodeStatus::Completed,
                started_at: Some("t0".into()),
                completed_at: Some("t1".into()),
                interactive: false,
                completion_released: false,
            }],
            frontmatter_retries: 0,
            frontmatter_violations: Vec::new(),
            missing_outputs: Vec::new(),
            delivery: None,
            awaiting: None,
        }
    }

    fn pending_node(id: &str) -> NodeState {
        NodeState {
            missing_skills: Vec::new(),
            skipped_skills: Vec::new(),
            skills: None,
            isolated_worktree: None,
            harness: None,
            cost: None,
            node_id: id.into(),
            status: NodeStatus::Pending,
            iter: 1,
            started_at: None,
            completed_at: None,
            failure_reason: None,
            skip_reason: None,
            iterations: Vec::new(),
            frontmatter_retries: 0,
            frontmatter_violations: Vec::new(),
            missing_outputs: Vec::new(),
            delivery: None,
            awaiting: None,
        }
    }

    #[test]
    fn start_node_already_started_returns_already_done() {
        let pipeline = PipelineDef {
            name: "test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![make_node("worker", NodeType::Agent, &["task"], &["out"])],
            edges: vec![],
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: true,
            grid_size: None,
            wait_for_started_nodes: false,
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
            tmux_cmd_override: Some("exec true"),
            docker_cmd_override: None,
            default_model: None,
            default_harness: None,
            default_harness_models: Default::default(),
            project_harness: None,
            inject_hook: false,
            harness_registry: None,
            sandbox_home_root: None,
            extension: Default::default(),
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
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: true,
            grid_size: None,
            wait_for_started_nodes: false,
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
            tmux_cmd_override: Some("exec true"),
            docker_cmd_override: None,
            default_model: None,
            default_harness: None,
            default_harness_models: Default::default(),
            project_harness: None,
            inject_hook: false,
            harness_registry: None,
            sandbox_home_root: None,
            extension: Default::default(),
        };

        let result = start_node(&params);
        assert!(matches!(result.outcome, PrimitiveOutcome::Rejected { .. }));
    }

    #[test]
    fn start_node_arms_the_stop_hook_for_an_agent_when_enabled() {
        let pipeline = PipelineDef {
            name: "test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![make_node("worker", NodeType::Agent, &[], &["out"])],
            edges: vec![],
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: true,
            grid_size: None,
            wait_for_started_nodes: false,
        };
        let run_state = empty_run_state();
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
            tmux_cmd_override: Some("exec true"),
            docker_cmd_override: None,
            default_model: None,
            default_harness: None,
            default_harness_models: Default::default(),
            project_harness: None,
            inject_hook: true,
            harness_registry: None,
            sandbox_home_root: None,
            extension: Default::default(),
        };
        let result = start_node(&params);
        assert_eq!(result.outcome, PrimitiveOutcome::Executed);
        assert!(
            result.spawn.unwrap().inject_hook,
            "an agent node must arm the Stop hook when the setting is on"
        );
    }

    /// #810 / ADR-0069 §1 — the start primitive emits ONE event for an
    /// interactive node: its `NodeStarted`. The spawn wait of #588 is gone, so a
    /// fresh interactive node projects `running` and Stats never sees an
    /// attente with no cause. The same payload freezes the node's **genre**
    /// (`interactive` + `orchestrator`, ADR-0007), which Performance filters on.
    #[test]
    fn start_node_emits_no_wait_at_spawn_and_freezes_the_node_kind() {
        let mut interactive_orchestrator = make_node("griller", NodeType::Agent, &[], &["out"]);
        interactive_orchestrator.interactive = true;
        interactive_orchestrator.orchestrator = true;
        let pipeline = PipelineDef {
            name: "test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![interactive_orchestrator],
            edges: vec![],
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: true,
            grid_size: None,
            wait_for_started_nodes: false,
        };
        let run_state = empty_run_state();
        let tmp = tempfile::tempdir().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();

        let params = StartNodeParams {
            run_id: "run-1",
            node_id: "griller",
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
            tmux_cmd_override: Some("exec true"),
            docker_cmd_override: None,
            default_model: None,
            default_harness: None,
            default_harness_models: Default::default(),
            project_harness: None,
            inject_hook: true,
            harness_registry: None,
            sandbox_home_root: None,
            extension: Default::default(),
        };
        let result = start_node(&params);

        assert_eq!(result.outcome, PrimitiveOutcome::Executed);
        assert_eq!(
            result
                .events
                .iter()
                .map(|e| e.kind.clone())
                .collect::<Vec<_>>(),
            vec![EventKind::NodeStarted],
            "no NodeAwaitingUser at spawn, interactive or not (ADR-0069 §1)"
        );
        let payload = result.events[0].payload.as_ref().unwrap();
        assert_eq!(payload["interactive"], serde_json::json!(true));
        assert_eq!(payload["orchestrator"], serde_json::json!(true));
    }

    #[test]
    fn start_node_never_arms_the_stop_hook_for_a_script() {
        // A `script` node must never carry the hook, whatever the setting says.
        let pipeline = PipelineDef {
            name: "test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![make_node("builder", NodeType::Script, &[], &["out"])],
            edges: vec![],
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: true,
            grid_size: None,
            wait_for_started_nodes: false,
        };
        let run_state = empty_run_state();
        let tmp = tempfile::tempdir().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();
        let pipeline_path = tmp.path().join("pipeline.yaml");
        let body_file =
            pdo_pipeline::domain::pipeline::canonical_prompt_path(&pipeline_path, "builder");
        std::fs::create_dir_all(body_file.parent().unwrap()).unwrap();
        std::fs::write(&body_file, "echo hi > \"$PDO_OUTPUT_OUT\"\n").unwrap();

        let params = StartNodeParams {
            run_id: "run-1",
            node_id: "builder",
            iter: 1,
            overrides: None,
            pipeline: &pipeline,
            run_state: &run_state,
            artifacts_dir: &artifacts_dir,
            worktree_dir: tmp.path(),
            repo_root: tmp.path(),
            pipeline_path: &pipeline_path,
            resolved_vars: &HashMap::new(),
            daemon_port: 5172,
            tmux_cmd_override: Some("exec true"),
            docker_cmd_override: None,
            default_model: None,
            default_harness: None,
            default_harness_models: Default::default(),
            project_harness: None,
            inject_hook: true,
            harness_registry: None,
            sandbox_home_root: None,
            extension: Default::default(),
        };
        let result = start_node(&params);
        assert_eq!(result.outcome, PrimitiveOutcome::Executed);
        assert!(
            !result.spawn.unwrap().inject_hook,
            "a script node must never arm the Stop hook"
        );
    }

    #[test]
    fn start_node_resolves_inputs_from_blackboard() {
        let pipeline = PipelineDef {
            name: "test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("planner", NodeType::Agent, &["task"], &["plan"]),
                make_node("implementer", NodeType::Agent, &["plan"], &["summary"]),
            ],
            edges: vec![make_edge("planner", "plan", "implementer", "plan")],
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: true,
            grid_size: None,
            wait_for_started_nodes: false,
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
            tmux_cmd_override: Some("exec true"),
            docker_cmd_override: None,
            default_model: None,
            default_harness: None,
            default_harness_models: Default::default(),
            project_harness: None,
            inject_hook: false,
            harness_registry: None,
            sandbox_home_root: None,
            extension: Default::default(),
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
                make_node("planner", NodeType::Agent, &["task"], &["plan"]),
                make_node("implementer", NodeType::Agent, &["plan"], &["summary"]),
            ],
            edges: vec![make_edge("planner", "plan", "implementer", "plan")],
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: true,
            grid_size: None,
            wait_for_started_nodes: false,
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
            tmux_cmd_override: Some("exec true"),
            docker_cmd_override: None,
            default_model: None,
            default_harness: None,
            default_harness_models: Default::default(),
            project_harness: None,
            inject_hook: false,
            harness_registry: None,
            sandbox_home_root: None,
            extension: Default::default(),
        };

        let input_paths = resolve_inputs(&params, node);
        assert_eq!(
            input_paths.get("plan").unwrap(),
            &override_path.to_string_lossy().to_string()
        );
    }

    #[test]
    fn override_applies_to_an_emergent_input_port() {
        // #486 / #600: an `agent` node declares NO input ports (its
        // inputs are emergent from edges), so an override keyed on the edge's target
        // port must still attach — otherwise the operator's dummy input is silently
        // dropped on exactly those nodes.
        let pipeline = PipelineDef {
            name: "test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("up", NodeType::Agent, &["task"], &["code"]),
                // `impl` declares no inputs — the `code` input is emergent.
                make_node("impl", NodeType::Agent, &[], &["out"]),
            ],
            edges: vec![make_edge("up", "code", "impl", "code")],
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: true,
            grid_size: None,
            wait_for_started_nodes: false,
        };
        let run_state = empty_run_state();
        let tmp = tempfile::tempdir().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();
        let override_path = tmp.path().join("dummy.md");
        std::fs::write(&override_path, "# dummy code").unwrap();
        let mut overrides = HashMap::new();
        overrides.insert("code".to_string(), override_path.clone());

        let node = pipeline.nodes.iter().find(|n| n.id == "impl").unwrap();
        let params = StartNodeParams {
            run_id: "run-1",
            node_id: "impl",
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
            tmux_cmd_override: Some("exec true"),
            docker_cmd_override: None,
            default_model: None,
            default_harness: None,
            default_harness_models: Default::default(),
            project_harness: None,
            inject_hook: false,
            harness_registry: None,
            sandbox_home_root: None,
            extension: Default::default(),
        };
        let input_paths = resolve_inputs(&params, node);
        assert_eq!(
            input_paths.get("code").unwrap(),
            &override_path.to_string_lossy().to_string(),
            "override attaches to the emergent `code` port"
        );
    }

    #[test]
    fn write_skip_outputs_deposits_empty_declared_and_edge_ports() {
        let pipeline = PipelineDef {
            name: "test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("skipme", NodeType::Agent, &["task"], &["decl_out"]),
                make_node("down", NodeType::Agent, &["edge_out"], &["z"]),
            ],
            edges: vec![make_edge("skipme", "edge_out", "down", "edge_out")],
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: true,
            grid_size: None,
            wait_for_started_nodes: false,
        };
        let tmp = tempfile::tempdir().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();

        let ports = write_skip_outputs(&pipeline, "skipme", 1, &HashMap::new(), &artifacts_dir)
            .expect("write ok");
        assert!(ports.contains(&"decl_out".to_string()));
        assert!(ports.contains(&"edge_out".to_string()));
        for port in ["decl_out", "edge_out"] {
            let p = blackboard::artifact_path(&artifacts_dir, "skipme", 1, port);
            assert!(p.exists(), "empty artifact written for port {port}");
            assert_eq!(std::fs::read_to_string(&p).unwrap(), "");
        }
    }

    #[test]
    fn write_skip_outputs_writes_nothing_for_an_intermediate_output() {
        let mut node = make_node("skipme", NodeType::Agent, &["task"], &["report", "preview"]);
        node.outputs[1].intermediate = true;
        let pipeline = PipelineDef {
            name: "test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![node],
            edges: vec![],
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: true,
            grid_size: None,
            wait_for_started_nodes: false,
        };
        let tmp = tempfile::tempdir().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();
        // An override naming the intermediate output is not a port to force.
        let mut overrides = HashMap::new();
        overrides.insert("preview".to_string(), "forced".to_string());

        let ports =
            write_skip_outputs(&pipeline, "skipme", 1, &overrides, &artifacts_dir).expect("ok");
        assert_eq!(ports, vec!["report".to_string()]);
        assert!(blackboard::artifact_path(&artifacts_dir, "skipme", 1, "report").exists());
        assert!(!blackboard::port_dir(&artifacts_dir, "skipme", 1, "preview").exists());
    }

    #[test]
    fn write_skip_outputs_falls_back_to_the_default_port_when_every_output_is_intermediate() {
        let mut node = make_node("skipme", NodeType::Agent, &["task"], &["preview"]);
        node.outputs[0].intermediate = true;
        let pipeline = PipelineDef {
            name: "test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![node],
            edges: vec![],
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: true,
            grid_size: None,
            wait_for_started_nodes: false,
        };
        let tmp = tempfile::tempdir().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();

        let ports = write_skip_outputs(&pipeline, "skipme", 1, &HashMap::new(), &artifacts_dir)
            .expect("ok");
        assert_eq!(ports, vec!["output".to_string()]);
        assert!(!blackboard::port_dir(&artifacts_dir, "skipme", 1, "preview").exists());
    }

    #[test]
    fn write_skip_outputs_uses_override_content_and_a_default_port() {
        let pipeline = PipelineDef {
            name: "test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![make_node("lonely", NodeType::Agent, &["task"], &[])],
            edges: vec![],
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: true,
            grid_size: None,
            wait_for_started_nodes: false,
        };
        let tmp = tempfile::tempdir().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();
        let mut overrides = HashMap::new();
        overrides.insert("output".to_string(), "provided".to_string());

        let ports =
            write_skip_outputs(&pipeline, "lonely", 1, &overrides, &artifacts_dir).expect("ok");
        assert_eq!(ports, vec!["output".to_string()]);
        let p = blackboard::artifact_path(&artifacts_dir, "lonely", 1, "output");
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "provided");
    }

    #[test]
    fn start_node_resolves_task_port_from_input_artifact() {
        let pipeline = PipelineDef {
            name: "test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![make_node("entry", NodeType::Agent, &["task"], &["out"])],
            edges: vec![],
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: true,
            grid_size: None,
            wait_for_started_nodes: false,
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
            tmux_cmd_override: Some("exec true"),
            docker_cmd_override: None,
            default_model: None,
            default_harness: None,
            default_harness_models: Default::default(),
            project_harness: None,
            inject_hook: false,
            harness_registry: None,
            sandbox_home_root: None,
            extension: Default::default(),
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
                make_node("planner", NodeType::Agent, &["task"], &["plan"]),
                make_node("researcher", NodeType::Agent, &["task"], &["research"]),
                make_node(
                    "implementer",
                    NodeType::Agent,
                    &["plan", "research"],
                    &["summary"],
                ),
            ],
            edges: vec![
                make_edge("planner", "plan", "implementer", "plan"),
                make_edge("researcher", "research", "implementer", "research"),
            ],
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: true,
            grid_size: None,
            wait_for_started_nodes: false,
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
            tmux_cmd_override: Some("exec true"),
            docker_cmd_override: None,
            default_model: None,
            default_harness: None,
            default_harness_models: Default::default(),
            project_harness: None,
            inject_hook: false,
            harness_registry: None,
            sandbox_home_root: None,
            extension: Default::default(),
        };

        let input_paths = resolve_inputs(&params, node);
        assert!(input_paths.contains_key("plan"));
        assert!(input_paths.contains_key("research"));
    }

    fn completed_node_iters(id: &str, iters: &[(i64, NodeStatus)]) -> NodeState {
        let (head_iter, head_status) = iters.last().cloned().unwrap_or((1, NodeStatus::Pending));
        NodeState {
            missing_skills: Vec::new(),
            skipped_skills: Vec::new(),
            skills: None,
            isolated_worktree: None,
            harness: None,
            cost: None,
            node_id: id.into(),
            status: head_status,
            iter: head_iter,
            started_at: Some("t0".into()),
            completed_at: None,
            failure_reason: None,
            skip_reason: None,
            iterations: iters
                .iter()
                .map(|(iter, status)| IterationInfo {
                    iter: *iter,
                    status: status.clone(),
                    started_at: Some("t0".into()),
                    completed_at: None,
                    interactive: false,
                    completion_released: false,
                })
                .collect(),
            frontmatter_retries: 0,
            frontmatter_violations: Vec::new(),
            missing_outputs: Vec::new(),
            delivery: None,
            awaiting: None,
        }
    }

    #[test]
    fn start_node_resolves_repeated_port_via_projection() {
        // A `repeated` edge resolves to concrete completed-iteration paths, NOT a
        // raw `iter-*` glob; `repeated` is read off the EDGE, and a failed iter is
        // quarantined even when its files are on disk.
        let pipeline = PipelineDef {
            name: "test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("reviewer", NodeType::Agent, &["task"], &["review"]),
                make_node_with_repeated_input("implementer", "reviews"),
            ],
            edges: vec![EdgeDef {
                repeated: true,
                ..make_edge("reviewer", "review", "implementer", "reviews")
            }],
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: true,
            grid_size: None,
            wait_for_started_nodes: false,
        };

        let mut run_state = empty_run_state();
        run_state.nodes.insert(
            "reviewer".into(),
            completed_node_iters(
                "reviewer",
                &[
                    (1, NodeStatus::Completed),
                    (2, NodeStatus::Failed),
                    (3, NodeStatus::Completed),
                ],
            ),
        );
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
            tmux_cmd_override: Some("exec true"),
            docker_cmd_override: None,
            default_model: None,
            default_harness: None,
            default_harness_models: Default::default(),
            project_harness: None,
            inject_hook: false,
            harness_registry: None,
            sandbox_home_root: None,
            extension: Default::default(),
        };

        let input_paths = resolve_inputs(&params, node);
        let reviews_path = input_paths.get("reviews").unwrap();
        assert!(
            !reviews_path.contains("iter-*"),
            "no raw glob: {reviews_path}"
        );
        assert!(reviews_path.contains("iter-1/review/output.md"));
        assert!(reviews_path.contains("iter-3/review/output.md"));
        assert!(
            !reviews_path.contains("iter-2"),
            "failed iter-2 is quarantined: {reviews_path}"
        );
        assert_eq!(reviews_path.lines().count(), 2);
    }

    #[test]
    fn start_node_uses_latest_iter_for_upstream() {
        let pipeline = PipelineDef {
            name: "test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("reviewer", NodeType::Agent, &["task"], &["review"]),
                make_node("implementer", NodeType::Agent, &["review"], &["summary"]),
            ],
            edges: vec![make_edge("reviewer", "review", "implementer", "review")],
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: true,
            grid_size: None,
            wait_for_started_nodes: false,
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
            tmux_cmd_override: Some("exec true"),
            docker_cmd_override: None,
            default_model: None,
            default_harness: None,
            default_harness_models: Default::default(),
            project_harness: None,
            inject_hook: false,
            harness_registry: None,
            sandbox_home_root: None,
            extension: Default::default(),
        };

        let input_paths = resolve_inputs(&params, node);
        let review_path = input_paths.get("review").unwrap();
        assert!(
            review_path.contains("iter-3"),
            "should resolve to iter-3 (latest), got: {review_path}"
        );
    }

    #[test]
    fn resolve_inputs_reads_latest_completed_not_failed_iter() {
        // A non-repeated cross-iteration edge whose source failed at iter-1 then
        // completed at iter-2 must record iter-2 — never the failed iter-1, and
        // never the consumer's positional iter.
        let pipeline = PipelineDef {
            name: "test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("reviewer", NodeType::Agent, &["task"], &["review"]),
                make_node("implementer", NodeType::Agent, &["review"], &["summary"]),
            ],
            edges: vec![make_edge("reviewer", "review", "implementer", "review")],
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: true,
            grid_size: None,
            wait_for_started_nodes: false,
        };

        let mut run_state = empty_run_state();
        run_state.nodes.insert(
            "reviewer".into(),
            completed_node_iters(
                "reviewer",
                &[(1, NodeStatus::Failed), (2, NodeStatus::Completed)],
            ),
        );

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
            tmux_cmd_override: Some("exec true"),
            docker_cmd_override: None,
            default_model: None,
            default_harness: None,
            default_harness_models: Default::default(),
            project_harness: None,
            inject_hook: false,
            harness_registry: None,
            sandbox_home_root: None,
            extension: Default::default(),
        };

        let input_paths = resolve_inputs(&params, node);
        let review_path = input_paths.get("review").unwrap();
        assert!(
            review_path.contains("iter-2/review/output.md"),
            "resolves to the latest-completed iter-2, got: {review_path}"
        );
        assert!(
            !review_path.contains("iter-1"),
            "the failed iter-1 is quarantined, got: {review_path}"
        );
    }

    #[test]
    fn stop_node_emits_node_stopped_event() {
        let params = StopNodeParams {
            run_id: "run-1",
            node_id: "worker",
            iter: 1,
            tmux: &pdo_session::adapters::tmux::TmuxCli::on_socket("pdo-test"),
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
            tmux: &pdo_session::adapters::tmux::TmuxCli::on_socket("pdo-test"),
        };

        let result = stop_node(&params);
        assert_eq!(result.outcome, PrimitiveOutcome::Executed);
        for event in &result.events {
            assert_ne!(event.kind, EventKind::RunCompleted);
            assert_ne!(event.kind, EventKind::RunFailed);
        }
    }

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

    #[test]
    fn double_start_returns_already_done() {
        let pipeline = PipelineDef {
            name: "test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![make_node("worker", NodeType::Agent, &["task"], &["out"])],
            edges: vec![],
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: true,
            grid_size: None,
            wait_for_started_nodes: false,
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
            tmux_cmd_override: Some("exec true"),
            docker_cmd_override: None,
            default_model: None,
            default_harness: None,
            default_harness_models: Default::default(),
            project_harness: None,
            inject_hook: false,
            harness_registry: None,
            sandbox_home_root: None,
            extension: Default::default(),
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
