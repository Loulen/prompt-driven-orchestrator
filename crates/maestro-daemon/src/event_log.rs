use std::collections::HashMap;

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
pub struct NodeState {
    pub node_id: String,
    pub status: NodeStatus,
    pub iter: i64,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub failure_reason: Option<String>,
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
        }
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
                }
            }
            EventKind::NodeStarted => {
                if let Some(ref node_id) = event.node_id {
                    state.nodes.insert(
                        node_id.clone(),
                        NodeState {
                            node_id: node_id.clone(),
                            status: NodeStatus::Running,
                            iter: event.iter.unwrap_or(1),
                            started_at: Some(event.ts.clone()),
                            completed_at: None,
                            failure_reason: None,
                        },
                    );
                }
            }
            EventKind::NodeCompleted => {
                if let Some(ref node_id) = event.node_id {
                    if let Some(node) = state.nodes.get_mut(node_id) {
                        node.status = NodeStatus::Completed;
                        node.completed_at = Some(event.ts.clone());
                    }
                }
            }
            EventKind::NodeAwaitingUser => {
                if let Some(ref node_id) = event.node_id {
                    if let Some(node) = state.nodes.get_mut(node_id) {
                        node.status = NodeStatus::AwaitingUser;
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
                    }
                }
            }
            EventKind::MergeConflictDetected => {
                // Informational event — the run is halted via a subsequent RunFailed
            }
            EventKind::PipelineModified => {
                // Informational — the run-scoped pipeline changed on disk.
                // RunState.node_defs/edges are re-parsed from the file at
                // augmentation time (see augment_run_state_from_disk), not here.
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
            }
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
    fn run_id_format() {
        let id = generate_run_id();
        // Format: YYYYMMDD-HHMMSS-<7char>
        assert!(id.len() >= 22, "run-id too short: {id}");
        assert!(id.contains('-'));
    }
}
