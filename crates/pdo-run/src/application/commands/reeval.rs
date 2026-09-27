//! The re-evaluation a command drives once it changed the Run: what it really
//! did (spawned, skipped, terminal), which the command answers instead of an
//! unconditional `{ok: true}`.

use super::*;

/// The run reached a terminal state during a post-command re-evaluation.
#[derive(Debug, Clone)]
pub(super) enum ReEvalTerminal {
    Completed,
    Halted(String),
    /// An `unrouted` convergence parked the run `AwaitingUser` — not terminal, but
    /// the re-evaluation had nothing left to dispatch this pass.
    Interrupted(String),
}

/// The real effect of a post-command re-evaluation: which nodes were spawned,
/// which candidate spawns were skipped and why, and whether the run went terminal.
/// Handlers surface this instead of an unconditional `{ok:true}`.
#[derive(Debug, Default)]
pub(super) struct ReEvalSummary {
    /// `(node_id, iter)` pairs whose spawn genuinely launched a session.
    pub(super) spawned: Vec<(String, i64)>,
    /// Human-readable reasons for candidate spawns that did NOT launch
    /// (guard skips, throttling, spawn failures).
    pub(super) skipped: Vec<String>,
    pub(super) terminal: Option<ReEvalTerminal>,
}

impl ReEvalSummary {
    pub(super) fn record_spawn(&mut self, node_id: &str, iter: i64, outcome: SpawnOutcome) {
        match outcome {
            SpawnOutcome::Spawned { .. } => self.spawned.push((node_id.to_string(), iter)),
            SpawnOutcome::Throttled => self.skipped.push(format!(
                "node '{node_id}' iter {iter} throttled into waiting (session cap)"
            )),
            // `Deferred` joins the two rather than getting its own arm: its reason
            // already reads as the operator sentence, and every consumer of `skipped`
            // is a "why did nothing start" message.
            SpawnOutcome::Refused { reason }
            | SpawnOutcome::Deferred { reason }
            | SpawnOutcome::Failed { reason } => self.skipped.push(reason),
        }
    }

    /// The truthful command-response body: spawns happened → `{ok, spawned}`,
    /// nothing launched → `{ok, noop, reason}`.
    pub(super) fn into_response_body(self) -> serde_json::Value {
        if !self.spawned.is_empty() {
            let spawned: Vec<serde_json::Value> = self
                .spawned
                .into_iter()
                .map(|(node_id, iter)| serde_json::json!({ "node_id": node_id, "iter": iter }))
                .collect();
            return serde_json::json!({ "ok": true, "spawned": spawned });
        }
        let reason = match &self.terminal {
            Some(ReEvalTerminal::Completed) => "run completed".to_string(),
            Some(ReEvalTerminal::Halted(msg)) => format!("run halted: {msg}"),
            Some(ReEvalTerminal::Interrupted(msg)) => {
                format!("run interrupted (awaiting user): {msg}")
            }
            None => {
                if self.skipped.is_empty() {
                    "no eligible spawn".to_string()
                } else {
                    self.skipped.join("; ")
                }
            }
        };
        serde_json::json!({ "ok": true, "noop": true, "reason": reason })
    }
}

/// Post-command re-evaluation that also re-drives the admission queue when the
/// command drove the run to a **terminal** state.
///
/// A run going terminal stops counting its still-session-holding nodes against the
/// global session cap, so a slot frees — and the site that frees a slot must
/// re-drive the queue, because `retry_waiting_nodes` has no timer of its own and a
/// throttled node in another run would starve. The sweep is global and idempotent.
pub(super) async fn re_evaluate_after_command(state: &Runs, run_id: &str) -> ReEvalSummary {
    let summary = re_evaluate_after_command_inner(state, run_id).await;
    if summary.terminal.is_some() {
        retry_waiting_nodes(state).await;
    }
    summary
}

/// Extract variable references ($name) from when clauses on Switch output ports
/// reachable from outgoing edges of a node.
pub(super) fn extract_variable_refs_from_outgoing_edges(
    pipeline: &pipeline::PipelineDef,
    node_id: &str,
) -> Vec<String> {
    let mut refs = Vec::new();
    for edge in &pipeline.edges {
        if edge.source.node != node_id {
            continue;
        }
        let target_node = pipeline.nodes.iter().find(|n| n.id == edge.target.node);
        if let Some(node) = target_node {
            if node.node_type == pipeline::NodeType::Switch {
                for port in &node.outputs {
                    if let Some(ref when) = port.when {
                        collect_yaml_var_refs(when, &mut refs);
                    }
                }
            }
            if let Some(ref max_iter) = node.max_iter {
                collect_yaml_var_refs(max_iter, &mut refs);
            }
        }
    }
    refs.sort();
    refs.dedup();
    refs
}

pub(super) fn collect_yaml_var_refs(val: &serde_yaml::Value, refs: &mut Vec<String>) {
    match val {
        serde_yaml::Value::String(s) if s.starts_with('$') => {
            refs.push(s[1..].to_string());
        }
        serde_yaml::Value::Mapping(map) => {
            for (_, v) in map {
                collect_yaml_var_refs(v, refs);
            }
        }
        serde_yaml::Value::Sequence(seq) => {
            for v in seq {
                collect_yaml_var_refs(v, refs);
            }
        }
        _ => {}
    }
}
