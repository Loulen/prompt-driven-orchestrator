//! A node is throttled, then starts an iteration.

use super::support::upsert_iteration;
use crate::domain::event::Event;
use crate::domain::state::*;

pub(super) fn node_waiting(state: &mut RunState, event: &Event) {
    // Throttled by the session cap: the node is ready but holds no
    // session yet. Mark it `Waiting`; a later `NodeStarted` promotes
    // it to `Running`. No iteration row is opened — the node has not
    // started executing.
    if let Some(ref node_id) = event.node_id {
        let iter = event.iter.unwrap_or(1);
        let node = state
            .nodes
            .entry(node_id.clone())
            .or_insert_with(|| NodeState {
                harness: None,
                isolated_worktree: None,
                skills: None,
                missing_skills: Vec::new(),
                skipped_skills: Vec::new(),
                cost: None,
                node_id: node_id.clone(),
                status: NodeStatus::Waiting,
                iter,
                started_at: None,
                completed_at: None,
                failure_reason: None,
                skip_reason: None,
                pruned: false,
                iterations: Vec::new(),
                frontmatter_retries: 0,
                frontmatter_violations: Vec::new(),
                missing_outputs: Vec::new(),
                delivery: None,
                awaiting: None,
            });
        node.status = NodeStatus::Waiting;
        node.iter = iter;
        // A pruned node launched by its ▶ runs its branch as if it was taken.
        node.pruned = false;
    }
}

pub(super) fn node_started(state: &mut RunState, event: &Event) {
    if let Some(ref node_id) = event.node_id {
        // Raw count of node sessions spawned (#100). Incremented per
        // `NodeStarted` (not per distinct `(node, iter)`), inside the
        // node-id guard so only real node spawns count — the manager
        // emits no `NodeStarted`.
        state.sessions_spawned += 1;
        let iter = event.iter.unwrap_or(1);
        let iteration = IterationInfo {
            iter,
            status: NodeStatus::Running,
            started_at: Some(event.ts.clone()),
            completed_at: None,
            interactive: event
                .payload
                .as_ref()
                .and_then(|p| p.get("interactive"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            completion_released: false,
            waited_ms: 0,
            open_wait_since: None,
        };
        let node = state
            .nodes
            .entry(node_id.clone())
            .or_insert_with(|| NodeState {
                harness: None,
                isolated_worktree: None,
                skills: None,
                missing_skills: Vec::new(),
                skipped_skills: Vec::new(),
                cost: None,
                node_id: node_id.clone(),
                status: NodeStatus::Running,
                iter,
                started_at: Some(event.ts.clone()),
                completed_at: None,
                failure_reason: None,
                skip_reason: None,
                pruned: false,
                iterations: Vec::new(),
                frontmatter_retries: 0,
                frontmatter_violations: Vec::new(),
                missing_outputs: Vec::new(),
                delivery: None,
                awaiting: None,
            });
        node.status = NodeStatus::Running;
        node.iter = iter;
        node.started_at = Some(event.ts.clone());
        node.completed_at = None;
        node.failure_reason = None;
        // The ▶ of a pruned node runs its branch as if it was taken
        // (« Forcer un nœud skipped »): from here on it is alive for routing.
        node.pruned = false;
        // #588: a re-spawn (retry / restart) is a fresh agent that knows
        // no declared wait.
        node.awaiting = None;
        // Reset the evidence vectors too, not just `completed_at` /
        // `failure_reason`: otherwise a *successful* retry leaves stale
        // violations on a green node.
        node.frontmatter_violations = Vec::new();
        node.missing_outputs = Vec::new();
        // #616/ADR-0046: freeze the harness this session ran on, from the
        // `NodeStarted` payload (node_spawn records the resolved harness
        // there). A re-spawn (retry / resume) re-freezes it; absent payload
        // (a pre-#616 event) leaves it `None`.
        if let Some(h) = event
            .payload
            .as_ref()
            .and_then(|p| p.get("harness"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
        {
            node.harness = Some(h.to_string());
        }
        // #653/ADR-0060: freeze where this NodeRun works, the same way.
        // A re-spawn of the same iteration re-poses the frozen value
        // rather than re-reading the (possibly edited) document, so the
        // recovery path lands back in the working directory it left.
        if let Some(isolated) = event
            .payload
            .as_ref()
            .and_then(|p| p.get("isolated_worktree"))
            .and_then(|v| v.as_bool())
        {
            node.isolated_worktree = Some(isolated);
        }
        // #669/ADR-0062: freeze the skills actifs this session received,
        // and the ids it was promised but the bank no longer had. A re-spawn
        // re-freezes; a `script` node / pre-#669 event leaves `None`.
        if let Some(skills) = event
            .payload
            .as_ref()
            .and_then(|p| p.get("skills"))
            .filter(|v| !v.is_null())
            .and_then(|v| serde_json::from_value(v.clone()).ok())
        {
            node.skills = Some(skills);
        }
        node.missing_skills = event
            .payload
            .as_ref()
            .and_then(|p| p.get("missing_skills"))
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        // #672: what the delivery could not write into the worktree.
        node.skipped_skills = event
            .payload
            .as_ref()
            .and_then(|p| p.get("skipped_skills"))
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        upsert_iteration(&mut node.iterations, iteration);
    }
}
