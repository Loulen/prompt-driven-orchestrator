//! What a node's launch froze on its `NodeStarted`: harness, effort, session
//! id, isolation, provisioning.

use std::path::Path;

use anyhow::{Context, Result};

use crate::domain::merge_action;
use crate::domain::{event, state};
use pdo_workspace::domain::provisioning;

/// The effort level a node's iteration was **launched** with, read back from its
/// `NodeStarted` payload.
///
/// The event log is the source of truth here, deliberately **not** the current YAML:
/// ADR-0007 makes a live node's in-flight iteration immutable to edits, so
/// re-resolving from the pipeline would make resume the one path that applies a
/// mid-iteration edit. A missing key yields `None`, i.e. a bare `--continue`.
pub fn find_launch_effort(events: &[event::Event], node_id: &str, iter: i64) -> Option<String> {
    events
        .iter()
        .rev()
        .find(|e| {
            e.kind == event::EventKind::NodeStarted
                && e.node_id.as_deref() == Some(node_id)
                && e.iter == Some(iter)
        })
        .and_then(|e| e.payload.as_ref())
        .and_then(|p| p.get("effort"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// The agent session id a node's iteration was launched with, from the **latest**
/// `NodeStarted` payload for `(node_id, iter)`.
///
/// `.rev()` is load-bearing: a `restart_node` that re-spawned the same iteration
/// must resolve to the *fresh* id it pinned, not the dead session's. A missing key
/// yields `None`, i.e. newest-mtime transcript resolution and a bare `--continue`.
pub fn find_launch_session_id(events: &[event::Event], node_id: &str, iter: i64) -> Option<String> {
    events
        .iter()
        .rev()
        .find(|e| {
            e.kind == event::EventKind::NodeStarted
                && e.node_id.as_deref() == Some(node_id)
                && e.iter == Some(iter)
        })
        .and_then(|e| e.payload.as_ref())
        .and_then(|p| p.get("session_id"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// The harness a node's iteration was **launched** with (ADR-0046), from its
/// `NodeStarted` payload — never re-resolved from the current YAML or the tiers
/// above (ADR-0007: a live iteration is immutable to edits). A missing key yields
/// `None`, which the resume seam treats as the `claude` floor.
pub fn find_launch_harness(events: &[event::Event], node_id: &str, iter: i64) -> Option<String> {
    events
        .iter()
        .rev()
        .find(|e| {
            e.kind == event::EventKind::NodeStarted
                && e.node_id.as_deref() == Some(node_id)
                && e.iter == Some(iter)
        })
        .and_then(|e| e.payload.as_ref())
        .and_then(|p| p.get("harness"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// The **latest** `NodeStarted` payload for `(node_id, iter)`, cloned whole.
///
/// The `/pane` resurrection appends a fresh `NodeStarted` as a log trace, and REUSING
/// the original payload is what keeps `session_id` / `harness` / `effort` /
/// `base_sha` intact — a bare re-marker with an empty payload would strand the
/// transcript id on the next resume.
pub fn find_launch_node_started_payload(
    events: &[event::Event],
    node_id: &str,
    iter: i64,
) -> Option<serde_json::Value> {
    events
        .iter()
        .rev()
        .find(|e| {
            e.kind == event::EventKind::NodeStarted
                && e.node_id.as_deref() == Some(node_id)
                && e.iter == Some(iter)
        })
        .and_then(|e| e.payload.clone())
}

pub fn find_node_type<'a>(run_state: &'a state::RunState, node_id: &str) -> Option<&'a str> {
    run_state
        .node_defs
        .iter()
        .find(|nd| nd.id == node_id)
        .map(|nd| nd.node_type.as_str())
}

/// Where a node works according to the Run's **pipeline snapshot** (#653,
/// ADR-0060) — the answer for an iteration that has not started, and the
/// fallback for one whose events predate #653.
///
/// A snapshot that states the isolation is authoritative. Absent it, the type's
/// default stands in: an `agent` is isolated, a `script` shares the Run
/// worktree, and a structural node has no worktree of its own.
pub fn snapshot_isolation(run_state: &state::RunState, node_id: &str) -> bool {
    let Some(def) = run_state.node_defs.iter().find(|nd| nd.id == node_id) else {
        return false;
    };
    def.isolated_worktree.unwrap_or(def.node_type == "agent")
}

/// Where a node's iteration `iter` works (#653, ADR-0060): its FROZEN answer
/// when it has one, the Run snapshot's otherwise.
///
/// Every reader that has the event log on hand goes through here, so a node that
/// started isolated is still read as isolated after someone edits the document
/// mid-run — the freeze is a property of the NodeRun, not of the file.
pub fn node_run_isolation(
    events: &[event::Event],
    run_state: &state::RunState,
    node_id: &str,
    iter: i64,
) -> bool {
    merge_action::frozen_isolation(events, node_id, iter)
        .unwrap_or_else(|| snapshot_isolation(run_state, node_id))
}

/// The provisioning rules the `NodeStarted` of `node_id`/`iter` froze.
pub fn frozen_node_rules(
    events: &[event::Event],
    node_id: &str,
    iter: i64,
) -> Option<provisioning::ProvisioningRules> {
    events.iter().rev().find_map(|event| {
        if event.kind != event::EventKind::NodeStarted
            || event.node_id.as_deref() != Some(node_id)
            || event.iter != Some(iter)
        {
            return None;
        }
        event
            .payload
            .as_ref()
            .and_then(|payload| payload.get("provisioning"))
            .cloned()
            .and_then(|rules| serde_json::from_value(rules).ok())
    })
}

pub fn frozen_node_plan(
    events: &[event::Event],
    node_id: &str,
    iter: i64,
) -> Option<provisioning::ProvisioningPlan> {
    events.iter().rev().find_map(|event| {
        if event.kind != event::EventKind::NodeStarted
            || event.node_id.as_deref() != Some(node_id)
            || event.iter != Some(iter)
        {
            return None;
        }
        event
            .payload
            .as_ref()
            .and_then(|payload| payload.get("provisioning_plan"))
            .and_then(|value| serde_json::from_value(value.clone()).ok())
    })
}

/// Read the node extension directly from the frozen pipeline document. Keeping
/// provisioning as an extension avoids widening the heavily shared NodeDef
/// runtime shape while still preserving it through YAML authoring.
pub fn node_rules_from_pipeline(
    pipeline_path: &Path,
    node_id: &str,
) -> Result<provisioning::ProvisioningRules> {
    let yaml = match std::fs::read_to_string(pipeline_path) {
        Ok(yaml) => yaml,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            tracing::warn!(
                "pipeline {} is absent; node {node_id} has no provisioning extension",
                pipeline_path.display()
            );
            return Ok(provisioning::ProvisioningRules::default());
        }
        Err(error) => {
            return Err(error)
                .with_context(|| format!("read pipeline {}", pipeline_path.display()));
        }
    };
    let document: serde_yaml::Value = serde_yaml::from_str(&yaml)
        .with_context(|| format!("parse pipeline {}", pipeline_path.display()))?;
    let Some(nodes) = document
        .get("nodes")
        .and_then(serde_yaml::Value::as_sequence)
    else {
        return Ok(provisioning::ProvisioningRules::default());
    };
    for node in nodes {
        if node.get("id").and_then(serde_yaml::Value::as_str) == Some(node_id) {
            return node
                .get("provisioning")
                .cloned()
                .map(serde_yaml::from_value)
                .transpose()
                .context("parse node provisioning rules")
                .map(Option::unwrap_or_default);
        }
    }
    Ok(provisioning::ProvisioningRules::default())
}

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;
    use crate::domain::scheduler;
    use crate::domain::scheduler::dispatcher as scheduler_dispatcher;
    use pdo_pipeline::domain::pipeline;
    use pretty_assertions::assert_eq;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    /// A Run's events, its state and their projection, as these tests read them.
    mod event_log {
        pub use crate::domain::event::*;
        pub use crate::domain::projection::*;
        pub use crate::domain::state::*;
    }

    #[test]
    fn find_launch_effort_reads_the_matching_node_started() {
        // #424 (slice C): the resume path re-poses the LAUNCH-time level, read from
        // the `NodeStarted` payload — never re-resolved from the current YAML
        // (ADR-0007: a live node's in-flight iteration is immutable to edits).
        let ev = |node: &str, iter: i64, payload: Option<serde_json::Value>| event_log::Event {
            id: None,
            run_id: "r1".to_string(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::NodeStarted,
            node_id: Some(node.to_string()),
            iter: Some(iter),
            payload,
        };
        let events = vec![
            ev("probe", 1, Some(serde_json::json!({"effort": "low"}))),
            ev("probe", 2, Some(serde_json::json!({"effort": "xhigh"}))),
            // No effort recorded → the legacy / account-default path.
            ev("control", 1, Some(serde_json::json!({"model": "opus"}))),
            // Historical row: the payload predates #424 entirely.
            ev(
                "legacy",
                1,
                Some(serde_json::json!({"node_type": "agent", "isolated_worktree": false})),
            ),
            // Guard-probe shape (never appended in production, but harmless here).
            ev("nopayload", 1, None),
            // A null effort must read as unset, not as the string "null".
            ev("nulled", 1, Some(serde_json::json!({"effort": null}))),
            // An empty string collapses to unset — same rule as everywhere else.
            ev("blank", 1, Some(serde_json::json!({"effort": ""}))),
        ];

        // Keyed on (node_id, iter): iter 2 must not leak into iter 1.
        assert_eq!(
            find_launch_effort(&events, "probe", 1).as_deref(),
            Some("low")
        );
        assert_eq!(
            find_launch_effort(&events, "probe", 2).as_deref(),
            Some("xhigh")
        );
        for (node, iter) in [
            ("control", 1),
            ("legacy", 1),
            ("nopayload", 1),
            ("nulled", 1),
            ("blank", 1),
            ("probe", 3),
            ("absent", 1),
        ] {
            assert_eq!(
                find_launch_effort(&events, node, iter),
                None,
                "{node} iter {iter} must yield None (bare --continue)"
            );
        }

        // Only `NodeStarted` counts — a same-node event of another kind must not
        // be mistaken for a launch record.
        let mut other = ev("probe", 5, Some(serde_json::json!({"effort": "max"})));
        other.kind = event_log::EventKind::NodeCompleted;
        assert_eq!(find_launch_effort(&[other], "probe", 5), None);
    }

    #[test]
    fn frozen_node_provisioning_uses_the_started_iteration_recipe_and_plan() {
        let event = event_log::Event {
            id: None,
            run_id: "run".into(),
            ts: "2026-09-01T12:00:00Z".into(),
            kind: event_log::EventKind::NodeStarted,
            node_id: Some("worker".into()),
            iter: Some(2),
            payload: Some(serde_json::json!({
                "provisioning": {
                    "copy": [".env"],
                    "hardlink": [],
                    "symlink": []
                },
                "provisioning_plan": {
                    "entries": [{
                        "relative_path": ".env",
                        "mode": "copy",
                        "origin_scope": "isolated_node",
                        "pattern": ".env",
                        "provided_by_git": false
                    }],
                    "rules": [],
                    "conflicts": []
                }
            })),
        };

        assert_eq!(
            frozen_node_rules(std::slice::from_ref(&event), "worker", 2)
                .unwrap()
                .copy,
            vec![".env"]
        );
        assert_eq!(
            frozen_node_plan(&[event], "worker", 2).unwrap().entries[0].relative_path,
            ".env"
        );
        assert!(frozen_node_rules(&[], "worker", 2).is_none());
        assert!(frozen_node_plan(&[], "worker", 2).is_none());
    }
}
