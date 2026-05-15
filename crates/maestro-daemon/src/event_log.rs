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
pub struct PortBrief {
    pub name: String,
    pub side: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeDefInfo {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub node_type: String,
    pub view_x: Option<f64>,
    pub view_y: Option<f64>,
    pub inputs: Vec<PortBrief>,
    pub outputs: Vec<PortBrief>,
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
    MergeResolverStarted,
    MergeResolverCompleted,
    MergeResolverFailed,
    SwitchRouted,
    LoopIterStarted,
    LoopBreakReceived,
    LoopMaxReached,
    LoopDone,
    FrontmatterRetryPending,
    ForEachStarted,
    ForEachEmpty,
    ForEachBreakReceived,
    ForEachDone,
    NodeStopped,
    NodeAutoCompleted,
    NodeStale,
    PipelineLint,
    PipelineModified,
    RunCompleted,
    RunFailed,
    RunHalted,
    RunPaused,
    RunResumed,
    RunArchived,
    RunRenamed,
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
    Paused,
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
    Stopped,
    Stale,
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
    #[serde(default)]
    pub frontmatter_retries: i64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub frontmatter_violations: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartNodeInfo {
    pub input_path: String,
    pub started_at: String,
    pub target_node_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndPortStatus {
    pub port_name: String,
    pub status: String,
    pub reason: Option<String>,
    pub fired_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndNodeInfo {
    pub id: String,
    pub ports: Vec<EndPortStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeResolverInfo {
    pub status: NodeStatus,
    pub conflicting_node_id: String,
    pub iter: i64,
    pub session_name: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub failure_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopState {
    pub loop_node_id: String,
    pub current_iter: i64,
    pub max_iter: i64,
    pub break_received: bool,
    pub done: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForEachState {
    pub foreach_node_id: String,
    pub total_items: i64,
    pub break_received: bool,
    pub done: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwitchState {
    pub switch_node_id: String,
    pub chosen_branch: String,
    pub evaluated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunState {
    pub run_id: String,
    pub status: RunStatus,
    pub pipeline_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_node: Option<EndNodeInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge_resolver: Option<MergeResolverInfo>,
    #[serde(default)]
    pub loop_states: HashMap<String, LoopState>,
    #[serde(default)]
    pub foreach_states: HashMap<String, ForEachState>,
    #[serde(default)]
    pub switch_states: HashMap<String, SwitchState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_repo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_branch: Option<String>,
}

impl RunState {
    pub fn new(run_id: String, pipeline_name: String) -> Self {
        Self {
            run_id,
            status: RunStatus::Running,
            pipeline_name,
            name: None,
            input: None,
            started_at: None,
            completed_at: None,
            nodes: HashMap::new(),
            edges: Vec::new(),
            node_defs: Vec::new(),
            start_node: None,
            end_node: None,
            merge_resolver: None,
            loop_states: HashMap::new(),
            foreach_states: HashMap::new(),
            switch_states: HashMap::new(),
            target_repo: None,
            source_branch: None,
        }
    }
}

fn entry_node_ids(edges: &[EdgeInfo], node_defs: &[NodeDefInfo]) -> Vec<String> {
    let start_id = node_defs
        .iter()
        .find(|n| n.node_type == "start")
        .map(|n| n.id.as_str());

    if let Some(start_id) = start_id {
        edges
            .iter()
            .filter(|e| e.source_node == start_id)
            .map(|e| e.target_node.clone())
            .collect()
    } else {
        let nodes_with_unconditional_incoming: HashSet<&str> = edges
            .iter()
            .filter(|e| e.when_clause.is_none())
            .map(|e| e.target_node.as_str())
            .collect();

        node_defs
            .iter()
            .filter(|n| !nodes_with_unconditional_incoming.contains(n.id.as_str()))
            .map(|n| n.id.clone())
            .collect()
    }
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
                    if let Some(run_name) = payload.get("name").and_then(|v| v.as_str()) {
                        if !run_name.is_empty() {
                            state.name = Some(run_name.to_string());
                        }
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
                    if let Some(tr) = payload.get("target_repo").and_then(|v| v.as_str()) {
                        state.target_repo = Some(tr.to_string());
                    }
                    if let Some(sb) = payload.get("source_branch").and_then(|v| v.as_str()) {
                        state.source_branch = Some(sb.to_string());
                    }

                    state.start_node = Some(StartNodeInfo {
                        input_path: "_input/output.md".to_string(),
                        started_at: event.ts.clone(),
                        target_node_ids: entry_node_ids(&state.edges, &state.node_defs),
                    });

                    if let Some(end_def) = state.node_defs.iter().find(|n| n.node_type == "end") {
                        state.end_node = Some(EndNodeInfo {
                            id: end_def.id.clone(),
                            ports: end_def
                                .inputs
                                .iter()
                                .map(|port| EndPortStatus {
                                    port_name: port.name.clone(),
                                    status: "pending".to_string(),
                                    reason: None,
                                    fired_at: None,
                                })
                                .collect(),
                        });
                    }
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
                            frontmatter_retries: 0,
                            frontmatter_violations: Vec::new(),
                        });
                    node.status = NodeStatus::Running;
                    node.iter = iter;
                    node.started_at = Some(event.ts.clone());
                    node.completed_at = None;
                    node.failure_reason = None;
                    upsert_iteration(&mut node.iterations, iteration);
                }
            }
            EventKind::NodeCompleted | EventKind::NodeAutoCompleted => {
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
                            if let Some(arr) = payload.get("violations").and_then(|v| v.as_array())
                            {
                                node.frontmatter_violations = arr.clone();
                            }
                        }
                        let iter = event.iter.unwrap_or(node.iter);
                        if let Some(it) = node.iterations.iter_mut().find(|i| i.iter == iter) {
                            it.status = NodeStatus::Failed;
                            it.completed_at = Some(event.ts.clone());
                        }
                    }
                }
            }
            EventKind::NodeStopped => {
                if let Some(ref node_id) = event.node_id {
                    if let Some(node) = state.nodes.get_mut(node_id) {
                        node.status = NodeStatus::Stopped;
                        node.completed_at = Some(event.ts.clone());
                        if let Some(ref payload) = event.payload {
                            node.failure_reason = payload
                                .get("reason")
                                .and_then(|v| v.as_str())
                                .map(String::from);
                        }
                        let iter = event.iter.unwrap_or(node.iter);
                        if let Some(it) = node.iterations.iter_mut().find(|i| i.iter == iter) {
                            it.status = NodeStatus::Stopped;
                            it.completed_at = Some(event.ts.clone());
                        }
                    }
                }
            }
            EventKind::NodeStale => {
                if let Some(ref node_id) = event.node_id {
                    if let Some(node) = state.nodes.get_mut(node_id) {
                        node.status = NodeStatus::Stale;
                        let iter = event.iter.unwrap_or(node.iter);
                        if let Some(it) = node.iterations.iter_mut().find(|i| i.iter == iter) {
                            it.status = NodeStatus::Stale;
                        }
                    }
                }
            }
            EventKind::FrontmatterRetryPending => {
                if let Some(ref node_id) = event.node_id {
                    if let Some(node) = state.nodes.get_mut(node_id) {
                        node.frontmatter_retries += 1;
                    }
                }
            }
            EventKind::MergeConflictDetected => {
                // Informational — the run either spawns a resolver or fails
            }
            EventKind::SwitchRouted => {
                if let Some(ref payload) = event.payload {
                    if let Some(node_id) = payload.get("node_id").and_then(|v| v.as_str()) {
                        let chosen_branch = payload
                            .get("chosen_branch")
                            .and_then(|v| v.as_str())
                            .unwrap_or("default")
                            .to_string();

                        state.switch_states.insert(
                            node_id.to_string(),
                            SwitchState {
                                switch_node_id: node_id.to_string(),
                                chosen_branch: chosen_branch.clone(),
                                evaluated_at: event.ts.clone(),
                            },
                        );

                        let iter = event.iter.unwrap_or(1);
                        let node =
                            state
                                .nodes
                                .entry(node_id.to_string())
                                .or_insert_with(|| NodeState {
                                    node_id: node_id.to_string(),
                                    status: NodeStatus::Completed,
                                    iter,
                                    started_at: Some(event.ts.clone()),
                                    completed_at: Some(event.ts.clone()),
                                    failure_reason: None,
                                    iterations: Vec::new(),
                                    frontmatter_retries: 0,
                                    frontmatter_violations: Vec::new(),
                                });
                        node.status = NodeStatus::Completed;
                        node.completed_at = Some(event.ts.clone());
                        node.iter = iter;
                        upsert_iteration(
                            &mut node.iterations,
                            IterationInfo {
                                iter,
                                status: NodeStatus::Completed,
                                started_at: Some(event.ts.clone()),
                                completed_at: Some(event.ts.clone()),
                            },
                        );
                    }
                }
            }
            EventKind::LoopIterStarted => {
                if let Some(ref payload) = event.payload {
                    if let Some(loop_node_id) = payload.get("loop_node_id").and_then(|v| v.as_str())
                    {
                        let iter = payload.get("iter").and_then(|v| v.as_i64()).unwrap_or(1);
                        let max_iter = payload
                            .get("max_iter")
                            .and_then(|v| v.as_i64())
                            .unwrap_or(5);
                        let ls = state
                            .loop_states
                            .entry(loop_node_id.to_string())
                            .or_insert_with(|| LoopState {
                                loop_node_id: loop_node_id.to_string(),
                                current_iter: 1,
                                max_iter,
                                break_received: false,
                                done: false,
                            });
                        ls.current_iter = iter;
                        ls.max_iter = max_iter;
                    }
                }
            }
            EventKind::LoopBreakReceived => {
                if let Some(ref payload) = event.payload {
                    if let Some(loop_node_id) = payload.get("loop_node_id").and_then(|v| v.as_str())
                    {
                        if let Some(ls) = state.loop_states.get_mut(loop_node_id) {
                            ls.break_received = true;
                        }
                    }
                }
            }
            EventKind::LoopMaxReached => {
                // Informational
            }
            EventKind::LoopDone => {
                if let Some(ref payload) = event.payload {
                    if let Some(loop_node_id) = payload.get("loop_node_id").and_then(|v| v.as_str())
                    {
                        if let Some(ls) = state.loop_states.get_mut(loop_node_id) {
                            ls.done = true;
                        }
                    }
                }
            }
            EventKind::ForEachStarted => {
                if let Some(ref payload) = event.payload {
                    if let Some(foreach_node_id) =
                        payload.get("foreach_node_id").and_then(|v| v.as_str())
                    {
                        let total_items = payload
                            .get("total_items")
                            .and_then(|v| v.as_i64())
                            .unwrap_or(0);
                        state
                            .foreach_states
                            .entry(foreach_node_id.to_string())
                            .or_insert_with(|| ForEachState {
                                foreach_node_id: foreach_node_id.to_string(),
                                total_items,
                                break_received: false,
                                done: false,
                            });
                    }
                }
            }
            EventKind::ForEachEmpty => {
                if let Some(ref payload) = event.payload {
                    if let Some(foreach_node_id) =
                        payload.get("foreach_node_id").and_then(|v| v.as_str())
                    {
                        let fs = state
                            .foreach_states
                            .entry(foreach_node_id.to_string())
                            .or_insert_with(|| ForEachState {
                                foreach_node_id: foreach_node_id.to_string(),
                                total_items: 0,
                                break_received: false,
                                done: false,
                            });
                        fs.done = true;
                    }
                }
            }
            EventKind::ForEachBreakReceived => {
                if let Some(ref payload) = event.payload {
                    if let Some(foreach_node_id) =
                        payload.get("foreach_node_id").and_then(|v| v.as_str())
                    {
                        if let Some(fs) = state.foreach_states.get_mut(foreach_node_id) {
                            fs.break_received = true;
                        }
                    }
                }
            }
            EventKind::ForEachDone => {
                if let Some(ref payload) = event.payload {
                    if let Some(foreach_node_id) =
                        payload.get("foreach_node_id").and_then(|v| v.as_str())
                    {
                        if let Some(fs) = state.foreach_states.get_mut(foreach_node_id) {
                            fs.done = true;
                        }
                    }
                }
            }
            EventKind::MergeResolverStarted => {
                if let Some(ref payload) = event.payload {
                    let conflicting_node_id = payload
                        .get("conflicting_node_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let iter = payload.get("iter").and_then(|v| v.as_i64()).unwrap_or(1);
                    let session_name = payload
                        .get("session_name")
                        .and_then(|v| v.as_str())
                        .map(String::from);
                    state.merge_resolver = Some(MergeResolverInfo {
                        status: NodeStatus::Running,
                        conflicting_node_id,
                        iter,
                        session_name,
                        started_at: Some(event.ts.clone()),
                        completed_at: None,
                        failure_reason: None,
                    });
                }
            }
            EventKind::MergeResolverCompleted => {
                if let Some(ref mut mr) = state.merge_resolver {
                    mr.status = NodeStatus::Completed;
                    mr.completed_at = Some(event.ts.clone());
                }
            }
            EventKind::MergeResolverFailed => {
                if let Some(ref mut mr) = state.merge_resolver {
                    mr.status = NodeStatus::Failed;
                    mr.completed_at = Some(event.ts.clone());
                    if let Some(ref payload) = event.payload {
                        mr.failure_reason = payload
                            .get("reason")
                            .and_then(|v| v.as_str())
                            .map(String::from);
                    }
                }
            }
            EventKind::PipelineLint => {
                // Informational — records lint diagnostics for the pipeline
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
                if let Some(ref mut end_node) = state.end_node {
                    for port in &mut end_node.ports {
                        if port.status == "pending" {
                            port.status = "received".to_string();
                            port.fired_at = Some(event.ts.clone());
                        }
                    }
                }
            }
            EventKind::RunFailed => {
                state.status = RunStatus::Failed;
                state.completed_at = Some(event.ts.clone());
            }
            EventKind::RunHalted => {
                state.status = RunStatus::Halted;
                state.completed_at = Some(event.ts.clone());
                if let Some(ref mut end_node) = state.end_node {
                    let reason = event
                        .payload
                        .as_ref()
                        .and_then(|p| p.get("message"))
                        .and_then(|v| v.as_str())
                        .map(String::from);
                    for port in &mut end_node.ports {
                        port.status = "received".to_string();
                        port.reason = reason.clone();
                        port.fired_at = Some(event.ts.clone());
                    }
                }
            }
            EventKind::RunPaused => {
                if state.status == RunStatus::Running || state.status == RunStatus::AwaitingUser {
                    state.status = RunStatus::Paused;
                }
            }
            EventKind::RunResumed => {
                if state.status == RunStatus::Paused {
                    state.status = RunStatus::Running;
                }
            }
            EventKind::RunRenamed => {
                if let Some(ref payload) = event.payload {
                    if let Some(new_name) = payload.get("name").and_then(|v| v.as_str()) {
                        if new_name.is_empty() {
                            state.name = None;
                        } else {
                            state.name = Some(new_name.to_string());
                        }
                    }
                }
            }
            EventKind::RunArchived => {
                state.status = RunStatus::Archived;
                state.start_node = None;
                state.end_node = None;
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
        assert!(node.frontmatter_violations.is_empty());
    }

    #[test]
    fn projects_frontmatter_violations_on_failed_node() {
        let violations = serde_json::json!([
            { "port": "review", "field": "verdict", "reason": "value 'MAYBE' not in allowed" },
            { "port": "review", "field": "score", "reason": "expected int" },
        ]);
        let events = vec![
            make_event(EventKind::RunStarted, None, None),
            make_event(EventKind::NodeStarted, Some("reviewer"), Some(1)),
            make_event_with_payload(
                EventKind::NodeFailed,
                Some("reviewer"),
                serde_json::json!({
                    "reason": "output validation failed",
                    "violations": violations,
                }),
            ),
            make_event(EventKind::RunFailed, None, None),
        ];

        let state = project(&events).unwrap();
        let node = &state.nodes["reviewer"];
        assert_eq!(node.status, NodeStatus::Failed);
        assert_eq!(
            node.failure_reason.as_deref(),
            Some("output validation failed")
        );
        assert_eq!(node.frontmatter_violations.len(), 2);
        assert_eq!(node.frontmatter_violations[0]["field"], "verdict");
        assert_eq!(node.frontmatter_violations[1]["field"], "score");
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

    // --- start_node projection (issue #30, updated for #39) ---

    fn start_node_def() -> serde_json::Value {
        serde_json::json!({ "id": "start", "node_type": "start", "inputs": [], "outputs": [{"name": "user_prompt", "side": "right"}] })
    }

    fn end_node_def() -> serde_json::Value {
        serde_json::json!({ "id": "end", "node_type": "end", "inputs": [{"name": "result", "side": "left"}], "outputs": [] })
    }

    fn node_def(id: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id, "node_type": "doc-only",
            "inputs": [{"name": "task", "side": "left"}],
            "outputs": [{"name": "out", "side": "right"}]
        })
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
                "node_defs": [start_node_def(), end_node_def(), node_def("planner"), node_def("implementer")],
                "edges": [
                    edge_info("start", "planner"),
                    edge_info("planner", "implementer"),
                    edge_info("implementer", "end"),
                ],
            }),
        )];

        let state = project(&events).unwrap();
        let start = state.start_node.as_ref().unwrap();
        assert_eq!(start.input_path, "_input/output.md");
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
                "node_defs": [start_node_def(), end_node_def(), node_def("impl-a"), node_def("impl-b"), node_def("merger")],
                "edges": [
                    edge_info("start", "impl-a"),
                    edge_info("start", "impl-b"),
                    edge_info("impl-a", "merger"),
                    edge_info("impl-b", "merger"),
                    edge_info("merger", "end"),
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
                "node_defs": [start_node_def(), end_node_def(), node_def("implementer"), node_def("reviewer")],
                "edges": [
                    edge_info("start", "implementer"),
                    edge_info("implementer", "reviewer"),
                    edge_info_conditional("reviewer", "implementer", serde_json::json!({"iter": {"lt": 3}})),
                    edge_info("reviewer", "end"),
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
                    "node_defs": [start_node_def(), end_node_def(), node_def("only")],
                    "edges": [edge_info("start", "only"), edge_info("only", "end")],
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
    fn start_node_all_nodes_are_entry_when_no_inter_edges() {
        let events = vec![make_event_with_payload(
            EventKind::RunStarted,
            None,
            serde_json::json!({
                "pipeline_name": "isolated",
                "input": "go",
                "node_defs": [start_node_def(), end_node_def(), node_def("a"), node_def("b")],
                "edges": [edge_info("start", "a"), edge_info("start", "b")],
            }),
        )];

        let state = project(&events).unwrap();
        let start = state.start_node.as_ref().unwrap();
        let mut targets = start.target_node_ids.clone();
        targets.sort();
        assert_eq!(targets, vec!["a", "b"]);
    }

    #[test]
    fn start_node_end_edges_dont_block_entry() {
        let events = vec![make_event_with_payload(
            EventKind::RunStarted,
            None,
            serde_json::json!({
                "pipeline_name": "with-end",
                "input": "test",
                "node_defs": [start_node_def(), end_node_def(), node_def("reviewer")],
                "edges": [
                    edge_info("start", "reviewer"),
                    {
                        "source_node": "reviewer", "source_port": "review",
                        "target_node": "end", "target_port": "result",
                        "halt_message": "Blocked",
                        "when_clause": {"iter": {"gte": 3}}
                    },
                ],
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

    // --- End node projection tests (issue #39) ---

    #[test]
    fn end_node_pending_while_running() {
        let events = vec![make_event_with_payload(
            EventKind::RunStarted,
            None,
            serde_json::json!({
                "pipeline_name": "with-end",
                "input": "go",
                "node_defs": [start_node_def(), end_node_def(), node_def("worker")],
                "edges": [
                    edge_info("start", "worker"),
                    edge_info("worker", "end"),
                ],
            }),
        )];

        let state = project(&events).unwrap();
        let end = state.end_node.as_ref().expect("end_node should be present");
        assert_eq!(end.id, "end");
        assert_eq!(end.ports.len(), 1);
        assert_eq!(end.ports[0].port_name, "result");
        assert_eq!(end.ports[0].status, "pending");
        assert!(end.ports[0].reason.is_none());
        assert!(end.ports[0].fired_at.is_none());
    }

    #[test]
    fn end_node_received_on_run_completed() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({
                    "pipeline_name": "complete-test",
                    "input": "go",
                    "node_defs": [start_node_def(), end_node_def(), node_def("worker")],
                    "edges": [
                        edge_info("start", "worker"),
                        edge_info("worker", "end"),
                    ],
                }),
            ),
            make_event(EventKind::NodeStarted, Some("worker"), Some(1)),
            make_event(EventKind::NodeCompleted, Some("worker"), Some(1)),
            make_event(EventKind::RunCompleted, None, None),
        ];

        let state = project(&events).unwrap();
        let end = state.end_node.as_ref().expect("end_node should be present");
        assert_eq!(end.ports[0].status, "received");
        assert!(end.ports[0].reason.is_none());
        assert!(end.ports[0].fired_at.is_some());
    }

    #[test]
    fn end_node_received_with_reason_on_halt() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({
                    "pipeline_name": "halt-end-test",
                    "input": "iterate",
                    "node_defs": [start_node_def(), end_node_def(), node_def("reviewer")],
                    "edges": [
                        edge_info("start", "reviewer"),
                        {
                            "source_node": "reviewer", "source_port": "review",
                            "target_node": "end", "target_port": "result",
                            "halt_message": "Blocked after 3 iterations",
                            "when_clause": {"iter": {"gte": 3}}
                        },
                    ],
                }),
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
        let end = state.end_node.as_ref().expect("end_node should be present");
        assert_eq!(end.ports[0].status, "received");
        assert_eq!(
            end.ports[0].reason.as_deref(),
            Some("Blocked after 3 iterations")
        );
        assert!(end.ports[0].fired_at.is_some());
    }

    #[test]
    fn end_node_cleared_on_archived() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({
                    "pipeline_name": "archive-end-test",
                    "input": "go",
                    "node_defs": [start_node_def(), end_node_def(), node_def("worker")],
                    "edges": [
                        edge_info("start", "worker"),
                        edge_info("worker", "end"),
                    ],
                }),
            ),
            make_event(EventKind::NodeStarted, Some("worker"), Some(1)),
            make_event(EventKind::NodeCompleted, Some("worker"), Some(1)),
            make_event(EventKind::RunCompleted, None, None),
            make_event(EventKind::RunArchived, None, None),
        ];

        let state = project(&events).unwrap();
        assert!(state.end_node.is_none());
    }

    // --- Merge Resolver projection tests (issue #8) ---

    #[test]
    fn merge_resolver_full_lifecycle_conflict_to_completion() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "fan-in" }),
            ),
            make_event(EventKind::NodeStarted, Some("impl-a"), Some(1)),
            make_event(EventKind::NodeStarted, Some("impl-b"), Some(1)),
            make_event(EventKind::NodeCompleted, Some("impl-a"), Some(1)),
            make_event(EventKind::NodeCompleted, Some("impl-b"), Some(1)),
            make_event_with_payload(
                EventKind::MergeConflictDetected,
                Some("impl-b"),
                serde_json::json!({
                    "reason": "conflict merging impl-b into pipeline branch"
                }),
            ),
            make_event_with_payload(
                EventKind::MergeResolverStarted,
                None,
                serde_json::json!({
                    "conflicting_node_id": "impl-b",
                    "iter": 1,
                    "session_name": "maestro-run-1-__merge_resolver__-iter-1"
                }),
            ),
            make_event(EventKind::MergeResolverCompleted, None, None),
            make_event(EventKind::RunCompleted, None, None),
        ];

        let state = project(&events).unwrap();
        assert_eq!(state.status, RunStatus::Completed);
        assert_eq!(state.nodes["impl-a"].status, NodeStatus::Completed);
        assert_eq!(state.nodes["impl-b"].status, NodeStatus::Completed);

        let mr = state.merge_resolver.as_ref().unwrap();
        assert_eq!(mr.status, NodeStatus::Completed);
        assert_eq!(mr.conflicting_node_id, "impl-b");
        assert_eq!(mr.iter, 1);
        assert_eq!(
            mr.session_name.as_deref(),
            Some("maestro-run-1-__merge_resolver__-iter-1")
        );
        assert!(mr.completed_at.is_some());
        assert!(mr.failure_reason.is_none());
    }

    #[test]
    fn merge_resolver_failure_preserves_info_on_run_failed() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "fan-in" }),
            ),
            make_event(EventKind::NodeStarted, Some("impl-a"), Some(1)),
            make_event(EventKind::NodeCompleted, Some("impl-a"), Some(1)),
            make_event_with_payload(
                EventKind::MergeConflictDetected,
                Some("impl-a"),
                serde_json::json!({ "reason": "conflict" }),
            ),
            make_event_with_payload(
                EventKind::MergeResolverStarted,
                None,
                serde_json::json!({
                    "conflicting_node_id": "impl-a",
                    "iter": 1,
                    "session_name": "resolver-session"
                }),
            ),
            make_event_with_payload(
                EventKind::MergeResolverFailed,
                None,
                serde_json::json!({
                    "reason": "conflict markers remain"
                }),
            ),
            make_event(EventKind::RunFailed, None, None),
        ];

        let state = project(&events).unwrap();
        assert_eq!(state.status, RunStatus::Failed);

        let mr = state.merge_resolver.as_ref().unwrap();
        assert_eq!(mr.status, NodeStatus::Failed);
        assert_eq!(mr.conflicting_node_id, "impl-a");
        assert_eq!(mr.session_name.as_deref(), Some("resolver-session"));
        assert_eq!(
            mr.failure_reason.as_deref(),
            Some("conflict markers remain")
        );
    }

    #[test]
    fn merge_conflict_without_resolver_has_no_merge_resolver_info() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "no-resolver" }),
            ),
            make_event(EventKind::NodeStarted, Some("impl-1"), Some(1)),
            make_event(EventKind::NodeCompleted, Some("impl-1"), Some(1)),
            make_event_with_payload(
                EventKind::MergeConflictDetected,
                Some("impl-1"),
                serde_json::json!({ "reason": "conflict" }),
            ),
            make_event(EventKind::RunFailed, None, None),
        ];

        let state = project(&events).unwrap();
        assert_eq!(state.status, RunStatus::Failed);
        assert!(
            state.merge_resolver.is_none(),
            "no resolver should be present when merge is handled by Merge node"
        );
    }

    // --- ForEach integration tests ---

    #[test]
    fn foreach_full_lifecycle_3_items() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "foreach-test", "input": "go" }),
            ),
            make_event(EventKind::NodeStarted, Some("upstream"), Some(1)),
            make_event(EventKind::NodeCompleted, Some("upstream"), Some(1)),
            make_event_with_payload(
                EventKind::ForEachStarted,
                None,
                serde_json::json!({ "foreach_node_id": "fe1", "total_items": 3 }),
            ),
            make_event(EventKind::NodeStarted, Some("worker"), Some(1)),
            make_event(EventKind::NodeStarted, Some("worker"), Some(2)),
            make_event(EventKind::NodeStarted, Some("worker"), Some(3)),
            make_event(EventKind::NodeCompleted, Some("worker"), Some(1)),
            make_event(EventKind::NodeCompleted, Some("worker"), Some(2)),
            make_event(EventKind::NodeCompleted, Some("worker"), Some(3)),
            make_event_with_payload(
                EventKind::ForEachDone,
                None,
                serde_json::json!({ "foreach_node_id": "fe1" }),
            ),
            make_event(EventKind::RunCompleted, None, None),
        ];

        let state = project(&events).unwrap();
        assert_eq!(state.status, RunStatus::Completed);

        let fe_state = &state.foreach_states["fe1"];
        assert_eq!(fe_state.total_items, 3);
        assert!(fe_state.done);
        assert!(!fe_state.break_received);

        let worker = &state.nodes["worker"];
        assert_eq!(worker.iter, 3);
        assert_eq!(worker.iterations.len(), 3);
        for it in &worker.iterations {
            assert_eq!(it.status, NodeStatus::Completed);
        }
    }

    #[test]
    fn foreach_empty_list_completes_immediately() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "foreach-empty" }),
            ),
            make_event(EventKind::NodeStarted, Some("upstream"), Some(1)),
            make_event(EventKind::NodeCompleted, Some("upstream"), Some(1)),
            make_event_with_payload(
                EventKind::ForEachEmpty,
                None,
                serde_json::json!({ "foreach_node_id": "fe1" }),
            ),
            make_event_with_payload(
                EventKind::ForEachDone,
                None,
                serde_json::json!({ "foreach_node_id": "fe1" }),
            ),
            make_event(EventKind::RunCompleted, None, None),
        ];

        let state = project(&events).unwrap();
        assert_eq!(state.status, RunStatus::Completed);

        let fe_state = &state.foreach_states["fe1"];
        assert!(fe_state.done);
        assert_eq!(fe_state.total_items, 0);
    }

    #[test]
    fn foreach_break_received_sets_flag() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "foreach-break" }),
            ),
            make_event_with_payload(
                EventKind::ForEachStarted,
                None,
                serde_json::json!({ "foreach_node_id": "fe1", "total_items": 3 }),
            ),
            make_event(EventKind::NodeStarted, Some("worker"), Some(1)),
            make_event(EventKind::NodeCompleted, Some("worker"), Some(1)),
            make_event_with_payload(
                EventKind::ForEachBreakReceived,
                None,
                serde_json::json!({ "foreach_node_id": "fe1" }),
            ),
        ];

        let state = project(&events).unwrap();
        let fe_state = &state.foreach_states["fe1"];
        assert!(fe_state.break_received);
        assert!(!fe_state.done);
    }

    // --- Run display labels (issue #115) ---

    #[test]
    fn run_started_with_name_sets_display_name() {
        let events = vec![make_event_with_payload(
            EventKind::RunStarted,
            None,
            serde_json::json!({
                "pipeline_name": "test-pipe",
                "input": "do stuff",
                "name": "My Feature Run"
            }),
        )];

        let state = project(&events).unwrap();
        assert_eq!(state.name.as_deref(), Some("My Feature Run"));
        assert_eq!(state.pipeline_name, "test-pipe");
    }

    #[test]
    fn run_started_without_name_has_none() {
        let events = vec![make_event_with_payload(
            EventKind::RunStarted,
            None,
            serde_json::json!({
                "pipeline_name": "test-pipe",
                "input": "do stuff"
            }),
        )];

        let state = project(&events).unwrap();
        assert!(state.name.is_none());
    }

    #[test]
    fn run_started_with_empty_name_has_none() {
        let events = vec![make_event_with_payload(
            EventKind::RunStarted,
            None,
            serde_json::json!({
                "pipeline_name": "test-pipe",
                "input": "do stuff",
                "name": ""
            }),
        )];

        let state = project(&events).unwrap();
        assert!(state.name.is_none());
    }

    #[test]
    fn run_renamed_updates_display_name() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({
                    "pipeline_name": "test-pipe",
                    "input": "do stuff"
                }),
            ),
            make_event_with_payload(
                EventKind::RunRenamed,
                None,
                serde_json::json!({ "name": "Better Name" }),
            ),
        ];

        let state = project(&events).unwrap();
        assert_eq!(state.name.as_deref(), Some("Better Name"));
    }

    #[test]
    fn run_renamed_overwrites_previous_name() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({
                    "pipeline_name": "test-pipe",
                    "name": "First Name"
                }),
            ),
            make_event_with_payload(
                EventKind::RunRenamed,
                None,
                serde_json::json!({ "name": "Second Name" }),
            ),
        ];

        let state = project(&events).unwrap();
        assert_eq!(state.name.as_deref(), Some("Second Name"));
    }

    #[test]
    fn run_renamed_to_empty_clears_name() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({
                    "pipeline_name": "test-pipe",
                    "name": "Had a Name"
                }),
            ),
            make_event_with_payload(
                EventKind::RunRenamed,
                None,
                serde_json::json!({ "name": "" }),
            ),
        ];

        let state = project(&events).unwrap();
        assert!(state.name.is_none());
    }

    // --- New event kinds and statuses (issue #112) ---

    #[test]
    fn node_stopped_sets_stopped_status() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "stop-test" }),
            ),
            make_event(EventKind::NodeStarted, Some("worker"), Some(1)),
            {
                let mut e = make_event(EventKind::NodeStopped, Some("worker"), Some(1));
                e.payload = Some(serde_json::json!({ "reason": "user killed it" }));
                e
            },
        ];

        let state = project(&events).unwrap();
        let node = &state.nodes["worker"];
        assert_eq!(node.status, NodeStatus::Stopped);
        assert_eq!(node.failure_reason.as_deref(), Some("user killed it"));
        assert!(node.completed_at.is_some());
        assert_eq!(node.iterations[0].status, NodeStatus::Stopped);
    }

    #[test]
    fn node_stopped_does_not_fail_the_run() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "stop-run-test" }),
            ),
            make_event(EventKind::NodeStarted, Some("worker"), Some(1)),
            {
                let mut e = make_event(EventKind::NodeStopped, Some("worker"), Some(1));
                e.payload = Some(serde_json::json!({ "reason": "deliberate stop" }));
                e
            },
        ];

        let state = project(&events).unwrap();
        assert_eq!(
            state.status,
            RunStatus::Running,
            "NodeStopped must NOT transition the run to failed"
        );
    }

    #[test]
    fn node_auto_completed_sets_completed_status() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "auto-complete-test" }),
            ),
            make_event(EventKind::NodeStarted, Some("worker"), Some(1)),
            make_event(EventKind::NodeAutoCompleted, Some("worker"), Some(1)),
        ];

        let state = project(&events).unwrap();
        let node = &state.nodes["worker"];
        assert_eq!(node.status, NodeStatus::Completed);
        assert!(node.completed_at.is_some());
        assert_eq!(node.iterations[0].status, NodeStatus::Completed);
    }

    #[test]
    fn node_stale_sets_stale_status() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "stale-test" }),
            ),
            make_event(EventKind::NodeStarted, Some("worker"), Some(1)),
            make_event(EventKind::NodeStale, Some("worker"), Some(1)),
        ];

        let state = project(&events).unwrap();
        let node = &state.nodes["worker"];
        assert_eq!(node.status, NodeStatus::Stale);
        assert!(node.completed_at.is_none(), "stale nodes are not completed");
        assert_eq!(node.iterations[0].status, NodeStatus::Stale);
    }

    #[test]
    fn run_paused_sets_paused_status() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "pause-test" }),
            ),
            make_event(EventKind::NodeStarted, Some("worker"), Some(1)),
            make_event(EventKind::RunPaused, None, None),
        ];

        let state = project(&events).unwrap();
        assert_eq!(state.status, RunStatus::Paused);
    }

    #[test]
    fn run_resumed_returns_to_running() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "resume-test" }),
            ),
            make_event(EventKind::NodeStarted, Some("worker"), Some(1)),
            make_event(EventKind::RunPaused, None, None),
            make_event(EventKind::RunResumed, None, None),
        ];

        let state = project(&events).unwrap();
        assert_eq!(state.status, RunStatus::Running);
    }

    #[test]
    fn run_paused_from_awaiting_user() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "pause-await" }),
            ),
            make_event(EventKind::NodeStarted, Some("griller"), Some(1)),
            make_event(EventKind::NodeAwaitingUser, Some("griller"), Some(1)),
            make_event(EventKind::RunPaused, None, None),
        ];

        let state = project(&events).unwrap();
        assert_eq!(state.status, RunStatus::Paused);
    }

    #[test]
    fn run_paused_noop_when_already_completed() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "p" }),
            ),
            make_event(EventKind::RunCompleted, None, None),
            make_event(EventKind::RunPaused, None, None),
        ];

        let state = project(&events).unwrap();
        assert_eq!(
            state.status,
            RunStatus::Completed,
            "RunPaused should not affect a completed run"
        );
    }

    #[test]
    fn run_resumed_noop_when_not_paused() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "p" }),
            ),
            make_event(EventKind::RunResumed, None, None),
        ];

        let state = project(&events).unwrap();
        assert_eq!(
            state.status,
            RunStatus::Running,
            "RunResumed on non-paused run is a no-op"
        );
    }

    #[test]
    fn event_kind_serialization_roundtrip() {
        let kinds = vec![
            EventKind::NodeStopped,
            EventKind::NodeAutoCompleted,
            EventKind::NodeStale,
            EventKind::RunPaused,
            EventKind::RunResumed,
        ];
        let expected_strings = vec![
            "\"node_stopped\"",
            "\"node_auto_completed\"",
            "\"node_stale\"",
            "\"run_paused\"",
            "\"run_resumed\"",
        ];
        for (kind, expected) in kinds.into_iter().zip(expected_strings) {
            let serialized = serde_json::to_string(&kind).unwrap();
            assert_eq!(serialized, expected);
            let deserialized: EventKind = serde_json::from_str(&serialized).unwrap();
            assert_eq!(deserialized, kind);
        }
    }

    #[test]
    fn node_status_serialization_roundtrip() {
        let statuses = vec![NodeStatus::Stopped, NodeStatus::Stale];
        let expected = vec!["\"stopped\"", "\"stale\""];
        for (status, exp) in statuses.into_iter().zip(expected) {
            let s = serde_json::to_string(&status).unwrap();
            assert_eq!(s, exp);
            let d: NodeStatus = serde_json::from_str(&s).unwrap();
            assert_eq!(d, status);
        }
    }

    // --- SwitchRouted projection (issue #118) ---

    #[test]
    fn switch_routed_creates_synthetic_completed_node() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "switch-test" }),
            ),
            make_event(EventKind::NodeStarted, Some("reviewer"), Some(1)),
            make_event(EventKind::NodeCompleted, Some("reviewer"), Some(1)),
            make_event_with_payload(
                EventKind::SwitchRouted,
                Some("sw"),
                serde_json::json!({
                    "node_id": "sw",
                    "chosen_branch": "pass",
                }),
            ),
        ];

        let state = project(&events).unwrap();

        // Switch should have synthetic Completed status
        let sw_node = &state.nodes["sw"];
        assert_eq!(sw_node.status, NodeStatus::Completed);
        assert!(sw_node.started_at.is_some());
        assert!(sw_node.completed_at.is_some());

        // SwitchState should track chosen branch
        let sw_state = &state.switch_states["sw"];
        assert_eq!(sw_state.chosen_branch, "pass");
        assert_eq!(sw_state.switch_node_id, "sw");
    }

    #[test]
    fn switch_routed_updates_on_re_evaluation() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "switch-test" }),
            ),
            make_event_with_payload(
                EventKind::SwitchRouted,
                Some("sw"),
                serde_json::json!({
                    "node_id": "sw",
                    "chosen_branch": "default",
                }),
            ),
            make_event_with_payload(
                EventKind::SwitchRouted,
                Some("sw"),
                serde_json::json!({
                    "node_id": "sw",
                    "chosen_branch": "pass",
                }),
            ),
        ];

        let state = project(&events).unwrap();
        let sw_state = &state.switch_states["sw"];
        assert_eq!(
            sw_state.chosen_branch, "pass",
            "re-evaluation should update chosen_branch"
        );
    }

    #[test]
    fn run_status_paused_serialization_roundtrip() {
        let s = serde_json::to_string(&RunStatus::Paused).unwrap();
        assert_eq!(s, "\"paused\"");
        let d: RunStatus = serde_json::from_str(&s).unwrap();
        assert_eq!(d, RunStatus::Paused);
    }
}
