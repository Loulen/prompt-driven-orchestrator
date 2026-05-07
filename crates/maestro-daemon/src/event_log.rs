use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeInfo {
    pub source_node: String,
    pub source_port: String,
    pub target_node: String,
    pub target_port: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub halt_message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when_clause: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeDefInfo {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub node_type: String,
    pub view_x: Option<f64>,
    pub view_y: Option<f64>,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    RunStarted,
    NodeStarted,
    NodeAwaitingUser,
    NodeCompleted,
    NodeFailed,
    MergeConflictDetected,
    PipelineModified,
    RunCompleted,
    RunFailed,
    RunHalted,
    RunArchived,
    CommandIssued,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: Option<i64>,
    pub run_id: String,
    pub ts: String,
    pub kind: EventKind,
    pub node_id: Option<String>,
    pub iter: Option<i64>,
    pub payload: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Running,
    AwaitingUser,
    Completed,
    Failed,
    Halted,
    Archived,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeStatus {
    Pending,
    Running,
    AwaitingUser,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IterationInfo {
    pub iter: i64,
    pub status: NodeStatus,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeState {
    pub node_id: String,
    pub status: NodeStatus,
    pub iter: i64,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub failure_reason: Option<String>,
    #[serde(default)]
    pub iterations: Vec<IterationInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartNodeInfo {
    pub input_path: String,
    pub started_at: String,
    pub target_node_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunState {
    pub run_id: String,
    pub status: RunStatus,
    pub pipeline_name: String,
    pub input: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub nodes: HashMap<String, NodeState>,
    #[serde(default)]
    pub edges: Vec<EdgeInfo>,
    #[serde(default)]
    pub node_defs: Vec<NodeDefInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_node: Option<StartNodeInfo>,
}

impl RunState {
    pub fn new(run_id: String, pipeline_name: String) -> Self {
        Self {
            run_id,
            status: RunStatus::Running,
            pipeline_name,
            input: None,
            started_at: None,
            completed_at: None,
            nodes: HashMap::new(),
            edges: Vec::new(),
            node_defs: Vec::new(),
            start_node: None,
        }
    }
}

fn entry_node_ids(edges: &[EdgeInfo], node_defs: &[NodeDefInfo]) -> Vec<String> {
    let nodes_with_unconditional_incoming: HashSet<&str> = edges
        .iter()
        .filter(|e| e.target_node != "__halt__" && e.when_clause.is_none())
        .map(|e| e.target_node.as_str())
        .collect();

    node_defs
        .iter()
        .filter(|n| !nodes_with_unconditional_incoming.contains(n.id.as_str()))
        .map(|n| n.id.clone())
        .collect()
}

fn upsert_iteration(iterations: &mut Vec<IterationInfo>, new: IterationInfo) {
    if let Some(existing) = iterations.iter_mut().find(|i| i.iter == new.iter) {
        existing.status = new.status;
        if new.started_at.is_some() {
            existing.started_at = new.started_at;
        }
        if new.completed_at.is_some() {
            existing.completed_at = new.completed_at;
        }
    } else {
        iterations.push(new);
    }
}

pub fn project(events: &[Event]) -> Option<RunState> {
    if events.is_empty() {
        return None;
    }

    let run_id = events[0].run_id.clone();
    let mut state = RunState::new(run_id, String::new());

    for event in events {
        match event.kind {
            EventKind::RunStarted => {
                state.started_at = Some(event.ts.clone());
                state.status = RunStatus::Running;
                if let Some(ref payload) = event.payload {
                    if let Some(name) = payload.get("pipeline_name").and_then(|v| v.as_str()) {
                        state.pipeline_name = name.to_string();
                    }
                    if let Some(input) = payload.get("input").and_then(|v| v.as_str()) {
                        state.input = Some(input.to_string());
                    }
                    if let Some(edges) = payload.get("edges") {
                        if let Ok(parsed) = serde_json::from_value::<Vec<EdgeInfo>>(edges.clone()) {
                            state.edges = parsed;
                        }
                    }
                    if let Some(node_defs) = payload.get("node_defs") {
                        if let Ok(parsed) =
                            serde_json::from_value::<Vec<NodeDefInfo>>(node_defs.clone())
                        {
                            state.node_defs = parsed;
                        }
                    }

                    state.start_node = Some(StartNodeInfo {
                        input_path: "_input.md".to_string(),
                        started_at: event.ts.clone(),
                        target_node_ids: entry_node_ids(&state.edges, &state.node_defs),
                    });
                }
            }
            EventKind::NodeStarted => {
                if let Some(ref node_id) = event.node_id {
                    let iter = event.iter.unwrap_or(1);
                    let iteration = IterationInfo {
                        iter,
                        status: NodeStatus::Running,
                        started_at: Some(event.ts.clone()),
                        completed_at: None,
                    };
                    let node = state
                        .nodes
                        .entry(node_id.clone())
                        .or_insert_with(|| NodeState {
                            node_id: node_id.clone(),
                            status: NodeStatus::Running,
                            iter,
                            started_at: Some(event.ts.clone()),
                            completed_at: None,
                            failure_reason: None,
                            iterations: Vec::new(),
                        });
                    node.status = NodeStatus::Running;
                    node.iter = iter;
                    node.started_at = Some(event.ts.clone());
                    node.completed_at = None;
                    node.failure_reason = None;
                    upsert_iteration(&mut node.iterations, iteration);
                }
            }
            EventKind::NodeCompleted => {
                if let Some(ref node_id) = event.node_id {
                    if let Some(node) = state.nodes.get_mut(node_id) {
                        node.status = NodeStatus::Completed;
                        node.completed_at = Some(event.ts.clone());
                        let iter = event.iter.unwrap_or(node.iter);
                        if let Some(it) = node.iterations.iter_mut().find(|i| i.iter == iter) {
                            it.status = NodeStatus::Completed;
                            it.completed_at = Some(event.ts.clone());
                        }
                    }
                }
            }
            EventKind::NodeAwaitingUser => {
                if let Some(ref node_id) = event.node_id {
                    if let Some(node) = state.nodes.get_mut(node_id) {
                        node.status = NodeStatus::AwaitingUser;
                        let iter = event.iter.unwrap_or(node.iter);
                        if let Some(it) = node.iterations.iter_mut().find(|i| i.iter == iter) {
                            it.status = NodeStatus::AwaitingUser;
                        }
                    }
                }
            }
            EventKind::NodeFailed => {
                if let Some(ref node_id) = event.node_id {
                    if let Some(node) = state.nodes.get_mut(node_id) {
                        node.status = NodeStatus::Failed;
                        node.completed_at = Some(event.ts.clone());
                        if let Some(ref payload) = event.payload {
                            node.failure_reason = payload
                                .get("reason")
                                .and_then(|v| v.as_str())
                                .map(String::from);
                        }
                        let iter = event.iter.unwrap_or(node.iter);
                        if let Some(it) = node.iterations.iter_mut().find(|i| i.iter == iter) {
                            it.status = NodeStatus::Failed;
                            it.completed_at = Some(event.ts.clone());
                        }
                    }
                }
            }
            EventKind::MergeConflictDetected => {
                // Informational event — the run is halted via a subsequent RunFailed
            }
            EventKind::PipelineModified => {
                // The run-scoped pipeline changed on disk. Node_defs/edges are
                // re-parsed from the file at augmentation time. However, if the
                // run was already Completed, reopen it so the scheduler can
                // evaluate whether newly-added nodes need spawning.
                if state.status == RunStatus::Completed {
                    state.status = RunStatus::Running;
                    state.completed_at = None;
                }
            }
            EventKind::RunCompleted => {
                state.status = RunStatus::Completed;
                state.completed_at = Some(event.ts.clone());
            }
            EventKind::RunFailed => {
                state.status = RunStatus::Failed;
                state.completed_at = Some(event.ts.clone());
            }
            EventKind::RunHalted => {
                state.status = RunStatus::Halted;
                state.completed_at = Some(event.ts.clone());
            }
            EventKind::RunArchived => {
                state.status = RunStatus::Archived;
                state.start_node = None;
            }
            EventKind::CommandIssued => {
                if let Some(ref payload) = event.payload {
                    let cmd = payload.get("command").and_then(|v| v.as_str());
                    if cmd == Some("resume_run")
                        && (state.status == RunStatus::Halted || state.status == RunStatus::Failed)
                    {
                        state.status = RunStatus::Running;
                        state.completed_at = None;
                    }
                }
            }
        }
    }

    // Sort iterations by iter number and reconcile top-level iter
    // (handles out-of-order events)
    for node in state.nodes.values_mut() {
        node.iterations.sort_by_key(|i| i.iter);
        if let Some(max_iter) = node.iterations.last() {
            node.iter = max_iter.iter;
        }
    }

    // Derive run-level awaiting_user from node states
    if state.status == RunStatus::Running
        && state
            .nodes
            .values()
            .any(|n| n.status == NodeStatus::AwaitingUser)
    {
        state.status = RunStatus::AwaitingUser;
    }

    Some(state)
}

pub fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

pub fn generate_run_id() -> String {
    let now = chrono::Utc::now();
    let ts = now.format("%Y%m%d-%H%M%S");
    let short = &uuid::Uuid::new_v4().to_string()[..7];
    format!("{ts}-{short}")
}

pub fn collect_cycle_extensions(events: &[Event]) -> HashMap<String, i64> {
    let mut extensions: HashMap<String, i64> = HashMap::new();
    for event in events {
        if event.kind != EventKind::CommandIssued {
            continue;
        }
        if let Some(ref payload) = event.payload {
            let cmd = payload.get("command").and_then(|v| v.as_str());
            if cmd == Some("extend_cycle") {
                if let (Some(node_id), Some(additional)) = (
                    payload.get("node_id").and_then(|v| v.as_str()),
                    payload.get("additional_iter").and_then(|v| v.as_i64()),
                ) {
                    *extensions.entry(node_id.to_string()).or_insert(0) += additional;
                }
            }
        }
    }
    extensions
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn make_event(kind: EventKind, node_id: Option<&str>, iter: Option<i64>) -> Event {
        Event {
            id: None,
            run_id: "run-1".into(),
            ts: "2026-01-01T00:00:00.000Z".into(),
            kind,
            node_id: node_id.map(String::from),
            iter,
            payload: None,
        }
    }

    fn make_event_with_payload(
        kind: EventKind,
        node_id: Option<&str>,
        payload: serde_json::Value,
    ) -> Event {
        Event {
            id: None,
            run_id: "run-1".into(),
            ts: "2026-01-01T00:00:00.000Z".into(),
            kind,
            node_id: node_id.map(String::from),
            iter: None,
            payload: Some(payload),
        }
    }

    #[test]
    fn projects_empty_events_to_none() {
        assert!(project(&[]).is_none());
    }

    #[test]
    fn projects_full_lifecycle() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "test-pipe", "input": "do the thing" }),
            ),
            make_event(EventKind::NodeStarted, Some("planner"), Some(1)),
            make_event(EventKind::NodeCompleted, Some("planner"), Some(1)),
            make_event(EventKind::RunCompleted, None, None),
        ];

        let state = project(&events).unwrap();
        assert_eq!(state.run_id, "run-1");
        assert_eq!(state.status, RunStatus::Completed);
        assert_eq!(state.pipeline_name, "test-pipe");
        assert_eq!(state.input.as_deref(), Some("do the thing"));
        assert_eq!(state.nodes.len(), 1);

        let node = &state.nodes["planner"];
        assert_eq!(node.status, NodeStatus::Completed);
        assert_eq!(node.iter, 1);
    }

    #[test]
    fn projects_failed_node() {
        let events = vec![
            make_event(EventKind::RunStarted, None, None),
            make_event(EventKind::NodeStarted, Some("worker"), Some(1)),
            make_event_with_payload(
                EventKind::NodeFailed,
                Some("worker"),
                serde_json::json!({ "reason": "could not complete" }),
            ),
            make_event(EventKind::RunFailed, None, None),
        ];

        let state = project(&events).unwrap();
        assert_eq!(state.status, RunStatus::Failed);
        let node = &state.nodes["worker"];
        assert_eq!(node.status, NodeStatus::Failed);
        assert_eq!(node.failure_reason.as_deref(), Some("could not complete"));
    }

    #[test]
    fn projects_running_state_mid_execution() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "wip" }),
            ),
            make_event(EventKind::NodeStarted, Some("planner"), Some(1)),
        ];

        let state = project(&events).unwrap();
        assert_eq!(state.status, RunStatus::Running);
        assert_eq!(state.nodes["planner"].status, NodeStatus::Running);
    }

    #[test]
    fn projects_interactive_node_awaiting_user() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "interactive-pipe" }),
            ),
            make_event(EventKind::NodeStarted, Some("griller"), Some(1)),
            make_event(EventKind::NodeAwaitingUser, Some("griller"), Some(1)),
        ];

        let state = project(&events).unwrap();
        assert_eq!(state.status, RunStatus::AwaitingUser);
        assert_eq!(state.nodes["griller"].status, NodeStatus::AwaitingUser);
    }

    #[test]
    fn mark_node_done_completes_awaiting_node() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "interactive-pipe" }),
            ),
            make_event(EventKind::NodeStarted, Some("griller"), Some(1)),
            make_event(EventKind::NodeAwaitingUser, Some("griller"), Some(1)),
            make_event(EventKind::NodeCompleted, Some("griller"), Some(1)),
            make_event(EventKind::RunCompleted, None, None),
        ];

        let state = project(&events).unwrap();
        assert_eq!(state.status, RunStatus::Completed);
        assert_eq!(state.nodes["griller"].status, NodeStatus::Completed);
    }

    #[test]
    fn projects_archived_run() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "archival-test", "input": "test input" }),
            ),
            make_event(EventKind::NodeStarted, Some("worker"), Some(1)),
            make_event(EventKind::NodeCompleted, Some("worker"), Some(1)),
            make_event(EventKind::RunCompleted, None, None),
            make_event(EventKind::RunArchived, None, None),
        ];

        let state = project(&events).unwrap();
        assert_eq!(state.status, RunStatus::Archived);
        assert_eq!(state.pipeline_name, "archival-test");
        assert_eq!(state.nodes.len(), 1);
        assert_eq!(state.nodes["worker"].status, NodeStatus::Completed);
    }

    #[test]
    fn projects_halted_run() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "halt-test" }),
            ),
            make_event(EventKind::NodeStarted, Some("reviewer"), Some(1)),
            make_event(EventKind::NodeCompleted, Some("reviewer"), Some(1)),
            make_event_with_payload(
                EventKind::RunHalted,
                None,
                serde_json::json!({ "message": "Blocked after 3 iterations" }),
            ),
        ];

        let state = project(&events).unwrap();
        assert_eq!(state.status, RunStatus::Halted);
        assert!(state.completed_at.is_some());
    }

    #[test]
    fn projects_merge_conflict_halts_run() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "merge-test" }),
            ),
            make_event(EventKind::NodeStarted, Some("impl-1"), Some(1)),
            make_event(EventKind::NodeCompleted, Some("impl-1"), Some(1)),
            Event {
                id: None,
                run_id: "run-1".into(),
                ts: "2026-01-01T00:00:00.000Z".into(),
                kind: EventKind::MergeConflictDetected,
                node_id: Some("impl-1".into()),
                iter: Some(1),
                payload: Some(serde_json::json!({
                    "reason": "conflict merging impl-1 into pipeline branch"
                })),
            },
            make_event(EventKind::RunFailed, None, None),
        ];

        let state = project(&events).unwrap();
        assert_eq!(state.status, RunStatus::Failed);
    }

    #[test]
    fn pipeline_modified_after_completed_reopens_run() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "test-pipe", "input": "do the thing" }),
            ),
            make_event(EventKind::NodeStarted, Some("planner"), Some(1)),
            make_event(EventKind::NodeCompleted, Some("planner"), Some(1)),
            make_event(EventKind::RunCompleted, None, None),
            make_event(EventKind::PipelineModified, None, None),
        ];

        let state = project(&events).unwrap();
        assert_eq!(
            state.status,
            RunStatus::Running,
            "PipelineModified after RunCompleted should reopen the run"
        );
    }

    #[test]
    fn pipeline_modified_during_running_stays_running() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "test-pipe" }),
            ),
            make_event(EventKind::NodeStarted, Some("planner"), Some(1)),
            make_event(EventKind::PipelineModified, None, None),
        ];

        let state = project(&events).unwrap();
        assert_eq!(state.status, RunStatus::Running);
    }

    #[test]
    fn pipeline_modified_after_failed_stays_failed() {
        let events = vec![
            make_event(EventKind::RunStarted, None, None),
            make_event(EventKind::NodeStarted, Some("worker"), Some(1)),
            make_event(EventKind::NodeFailed, None, None),
            make_event(EventKind::RunFailed, None, None),
            make_event(EventKind::PipelineModified, None, None),
        ];

        let state = project(&events).unwrap();
        assert_eq!(
            state.status,
            RunStatus::Failed,
            "PipelineModified should not reopen a Failed run"
        );
    }

    #[test]
    fn run_id_format() {
        let id = generate_run_id();
        // Format: YYYYMMDD-HHMMSS-<7char>
        assert!(id.len() >= 22, "run-id too short: {id}");
        assert!(id.contains('-'));
    }

    // --- start_node projection (issue #30) ---

    fn node_def(id: &str) -> serde_json::Value {
        serde_json::json!({ "id": id, "node_type": "doc-only", "inputs": ["task"], "outputs": ["out"] })
    }

    fn edge_info(src: &str, tgt: &str) -> serde_json::Value {
        serde_json::json!({
            "source_node": src, "source_port": "out",
            "target_node": tgt, "target_port": "task"
        })
    }

    fn edge_info_conditional(src: &str, tgt: &str, when: serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "source_node": src, "source_port": "out",
            "target_node": tgt, "target_port": "task",
            "when_clause": when
        })
    }

    #[test]
    fn start_node_single_entry() {
        let events = vec![make_event_with_payload(
            EventKind::RunStarted,
            None,
            serde_json::json!({
                "pipeline_name": "linear",
                "input": "hello world",
                "node_defs": [node_def("planner"), node_def("implementer")],
                "edges": [edge_info("planner", "implementer")],
            }),
        )];

        let state = project(&events).unwrap();
        let start = state.start_node.as_ref().unwrap();
        assert_eq!(start.input_path, "_input.md");
        assert_eq!(start.started_at, "2026-01-01T00:00:00.000Z");
        assert_eq!(start.target_node_ids, vec!["planner"]);
    }

    #[test]
    fn start_node_multiple_entry_nodes_fan_out() {
        let events = vec![make_event_with_payload(
            EventKind::RunStarted,
            None,
            serde_json::json!({
                "pipeline_name": "fan-out",
                "input": "build two things",
                "node_defs": [node_def("impl-a"), node_def("impl-b"), node_def("merger")],
                "edges": [
                    edge_info("impl-a", "merger"),
                    edge_info("impl-b", "merger"),
                ],
            }),
        )];

        let state = project(&events).unwrap();
        let start = state.start_node.as_ref().unwrap();
        let mut targets = start.target_node_ids.clone();
        targets.sort();
        assert_eq!(targets, vec!["impl-a", "impl-b"]);
    }

    #[test]
    fn start_node_conditional_back_edge_not_counted() {
        let events = vec![make_event_with_payload(
            EventKind::RunStarted,
            None,
            serde_json::json!({
                "pipeline_name": "cycle",
                "input": "iterate",
                "node_defs": [node_def("implementer"), node_def("reviewer")],
                "edges": [
                    edge_info("implementer", "reviewer"),
                    edge_info_conditional("reviewer", "implementer", serde_json::json!({"iter": {"lt": 3}})),
                ],
            }),
        )];

        let state = project(&events).unwrap();
        let start = state.start_node.as_ref().unwrap();
        assert_eq!(start.target_node_ids, vec!["implementer"]);
    }

    #[test]
    fn start_node_null_on_archived_run() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({
                    "pipeline_name": "archived-test",
                    "input": "test input",
                    "node_defs": [node_def("only")],
                    "edges": [],
                }),
            ),
            make_event(EventKind::NodeStarted, Some("only"), Some(1)),
            make_event(EventKind::NodeCompleted, Some("only"), Some(1)),
            make_event(EventKind::RunCompleted, None, None),
            make_event(EventKind::RunArchived, None, None),
        ];

        let state = project(&events).unwrap();
        assert_eq!(state.status, RunStatus::Archived);
        assert!(state.start_node.is_none());
    }

    #[test]
    fn start_node_all_nodes_are_entry_when_no_edges() {
        let events = vec![make_event_with_payload(
            EventKind::RunStarted,
            None,
            serde_json::json!({
                "pipeline_name": "isolated",
                "input": "go",
                "node_defs": [node_def("a"), node_def("b")],
                "edges": [],
            }),
        )];

        let state = project(&events).unwrap();
        let start = state.start_node.as_ref().unwrap();
        assert_eq!(start.target_node_ids, vec!["a", "b"]);
    }

    #[test]
    fn start_node_halt_edges_dont_block_entry() {
        let events = vec![make_event_with_payload(
            EventKind::RunStarted,
            None,
            serde_json::json!({
                "pipeline_name": "with-halt",
                "input": "test",
                "node_defs": [node_def("reviewer")],
                "edges": [{
                    "source_node": "reviewer", "source_port": "review",
                    "target_node": "__halt__", "target_port": "",
                    "halt_message": "Blocked",
                    "when_clause": {"iter": {"gte": 3}}
                }],
            }),
        )];

        let state = project(&events).unwrap();
        let start = state.start_node.as_ref().unwrap();
        assert_eq!(start.target_node_ids, vec!["reviewer"]);
    }

    // --- Multi-iteration projection tests (issue #29) ---

    fn make_event_ts(kind: EventKind, node_id: Option<&str>, iter: Option<i64>, ts: &str) -> Event {
        Event {
            id: None,
            run_id: "run-1".into(),
            ts: ts.into(),
            kind,
            node_id: node_id.map(String::from),
            iter,
            payload: None,
        }
    }

    #[test]
    fn single_iter_node_has_one_iteration_entry() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "test" }),
            ),
            make_event_ts(
                EventKind::NodeStarted,
                Some("planner"),
                Some(1),
                "2026-01-01T00:01:00.000Z",
            ),
            make_event_ts(
                EventKind::NodeCompleted,
                Some("planner"),
                Some(1),
                "2026-01-01T00:02:00.000Z",
            ),
        ];

        let state = project(&events).unwrap();
        let node = &state.nodes["planner"];
        assert_eq!(node.iterations.len(), 1);
        assert_eq!(node.iterations[0].iter, 1);
        assert_eq!(node.iterations[0].status, NodeStatus::Completed);
        assert_eq!(
            node.iterations[0].started_at.as_deref(),
            Some("2026-01-01T00:01:00.000Z")
        );
        assert_eq!(
            node.iterations[0].completed_at.as_deref(),
            Some("2026-01-01T00:02:00.000Z")
        );
    }

    #[test]
    fn multi_iter_cycle_produces_ordered_iterations() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "cycle-test" }),
            ),
            make_event_ts(
                EventKind::NodeStarted,
                Some("reviewer"),
                Some(1),
                "2026-01-01T00:01:00.000Z",
            ),
            make_event_ts(
                EventKind::NodeCompleted,
                Some("reviewer"),
                Some(1),
                "2026-01-01T00:02:00.000Z",
            ),
            make_event_ts(
                EventKind::NodeStarted,
                Some("reviewer"),
                Some(2),
                "2026-01-01T00:03:00.000Z",
            ),
            make_event_ts(
                EventKind::NodeCompleted,
                Some("reviewer"),
                Some(2),
                "2026-01-01T00:04:00.000Z",
            ),
            make_event_ts(
                EventKind::NodeStarted,
                Some("reviewer"),
                Some(3),
                "2026-01-01T00:05:00.000Z",
            ),
        ];

        let state = project(&events).unwrap();
        let node = &state.nodes["reviewer"];

        assert_eq!(node.iter, 3, "top-level iter should be the latest");
        assert_eq!(
            node.status,
            NodeStatus::Running,
            "current status is running"
        );
        assert_eq!(node.iterations.len(), 3);

        assert_eq!(node.iterations[0].iter, 1);
        assert_eq!(node.iterations[0].status, NodeStatus::Completed);

        assert_eq!(node.iterations[1].iter, 2);
        assert_eq!(node.iterations[1].status, NodeStatus::Completed);

        assert_eq!(node.iterations[2].iter, 3);
        assert_eq!(node.iterations[2].status, NodeStatus::Running);
        assert!(node.iterations[2].completed_at.is_none());
    }

    #[test]
    fn multi_iter_with_failed_iteration() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "fail-iter" }),
            ),
            make_event_ts(
                EventKind::NodeStarted,
                Some("impl"),
                Some(1),
                "2026-01-01T00:01:00.000Z",
            ),
            make_event_ts(
                EventKind::NodeCompleted,
                Some("impl"),
                Some(1),
                "2026-01-01T00:02:00.000Z",
            ),
            make_event_ts(
                EventKind::NodeStarted,
                Some("impl"),
                Some(2),
                "2026-01-01T00:03:00.000Z",
            ),
            {
                let mut e = make_event_ts(
                    EventKind::NodeFailed,
                    Some("impl"),
                    Some(2),
                    "2026-01-01T00:04:00.000Z",
                );
                e.payload = Some(serde_json::json!({ "reason": "test failure" }));
                e
            },
        ];

        let state = project(&events).unwrap();
        let node = &state.nodes["impl"];

        assert_eq!(node.iterations.len(), 2);
        assert_eq!(node.iterations[0].status, NodeStatus::Completed);
        assert_eq!(node.iterations[1].status, NodeStatus::Failed);
    }

    #[test]
    fn out_of_order_node_started_events_still_project_correctly() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "ooo" }),
            ),
            // iter 2 event arrives before iter 1 completes (out-of-order)
            make_event_ts(
                EventKind::NodeStarted,
                Some("worker"),
                Some(2),
                "2026-01-01T00:03:00.000Z",
            ),
            make_event_ts(
                EventKind::NodeStarted,
                Some("worker"),
                Some(1),
                "2026-01-01T00:01:00.000Z",
            ),
            make_event_ts(
                EventKind::NodeCompleted,
                Some("worker"),
                Some(1),
                "2026-01-01T00:02:00.000Z",
            ),
        ];

        let state = project(&events).unwrap();
        let node = &state.nodes["worker"];

        // iterations should be sorted by iter number
        assert_eq!(node.iterations.len(), 2);
        assert_eq!(node.iterations[0].iter, 1);
        assert_eq!(node.iterations[0].status, NodeStatus::Completed);
        assert_eq!(node.iterations[1].iter, 2);
        assert_eq!(node.iterations[1].status, NodeStatus::Running);

        // top-level iter reflects the highest
        assert_eq!(node.iter, 2);
    }

    #[test]
    fn existing_tests_still_get_empty_iterations_for_single_iter() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "compat" }),
            ),
            make_event(EventKind::NodeStarted, Some("worker"), Some(1)),
            make_event(EventKind::NodeCompleted, Some("worker"), Some(1)),
        ];

        let state = project(&events).unwrap();
        let node = &state.nodes["worker"];
        // Even single-iter nodes have exactly 1 iteration entry
        assert_eq!(node.iterations.len(), 1);
    }

    #[test]
    fn resume_run_transitions_halted_to_running() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "p" }),
            ),
            make_event(EventKind::RunHalted, None, None),
            Event {
                kind: EventKind::CommandIssued,
                payload: Some(serde_json::json!({ "command": "resume_run" })),
                ..make_event(EventKind::CommandIssued, None, None)
            },
        ];
        let state = project(&events).unwrap();
        assert_eq!(state.status, RunStatus::Running);
    }

    #[test]
    fn resume_run_transitions_failed_to_running() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "p" }),
            ),
            make_event(EventKind::RunFailed, None, None),
            Event {
                kind: EventKind::CommandIssued,
                payload: Some(serde_json::json!({ "command": "resume_run" })),
                ..make_event(EventKind::CommandIssued, None, None)
            },
        ];
        let state = project(&events).unwrap();
        assert_eq!(state.status, RunStatus::Running);
    }

    #[test]
    fn resume_run_noop_on_already_running() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "p" }),
            ),
            Event {
                kind: EventKind::CommandIssued,
                payload: Some(serde_json::json!({ "command": "resume_run" })),
                ..make_event(EventKind::CommandIssued, None, None)
            },
        ];
        let state = project(&events).unwrap();
        assert_eq!(state.status, RunStatus::Running);
    }

    #[test]
    fn collect_cycle_extensions_accumulates() {
        let events = vec![
            Event {
                kind: EventKind::CommandIssued,
                payload: Some(serde_json::json!({
                    "command": "extend_cycle",
                    "node_id": "review",
                    "additional_iter": 2
                })),
                ..make_event(EventKind::CommandIssued, None, None)
            },
            Event {
                kind: EventKind::CommandIssued,
                payload: Some(serde_json::json!({
                    "command": "extend_cycle",
                    "node_id": "review",
                    "additional_iter": 3
                })),
                ..make_event(EventKind::CommandIssued, None, None)
            },
            Event {
                kind: EventKind::CommandIssued,
                payload: Some(serde_json::json!({
                    "command": "extend_cycle",
                    "node_id": "other",
                    "additional_iter": 1
                })),
                ..make_event(EventKind::CommandIssued, None, None)
            },
        ];
        let ext = collect_cycle_extensions(&events);
        assert_eq!(ext["review"], 5);
        assert_eq!(ext["other"], 1);
    }

    #[test]
    fn command_issued_unknown_command_is_noop() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "p" }),
            ),
            Event {
                kind: EventKind::CommandIssued,
                payload: Some(serde_json::json!({ "command": "something_unknown" })),
                ..make_event(EventKind::CommandIssued, None, None)
            },
        ];
        let state = project(&events).unwrap();
        assert_eq!(state.status, RunStatus::Running);
    }
}
