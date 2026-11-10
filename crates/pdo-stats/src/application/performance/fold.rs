//! The heavy fold — every transcript read and subagent directory scan, gated
//! behind the memo. Synchronous: the events are already loaded into the
//! [`RunContext`]s, so nothing here reads the event log again.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::PathBuf;

use pdo_run::domain::event::EventKind;
use pdo_workspace::domain::worktree_layout::{sub_worktree_path, worktree_dir_for_run};

use super::infra::{record_infra_success, NodeStartRecord};
use super::record::{record_node_success, MergeResolverPending, PendingNode};
use super::{PerformanceError, RunContext};
use crate::application::ports::{RunFilesRoots, SessionTranscripts};
use crate::domain::absorption::resolver::AbsorptionResolver;
use crate::domain::performance::acc::{
    finish_by_harness, finish_infra_role, finish_pipeline, HarnessAcc, NodeAcc, NodeKind,
    PipelineAcc,
};
use crate::domain::performance::model_axis::{wire_model_axis, ModelAxisModelAcc};
use crate::domain::performance::node_defs::node_defs_from_payload;
use crate::domain::performance::waits::{declared_waits, duration_millis};
use crate::domain::performance::{PerformanceAggregate, PerformanceEntity, StatsPerformance};

/// The heavy fold — every transcript read and subagent directory scan, gated
/// behind the memo. Synchronous: events are already loaded into [`RunContext`],
/// so nothing here touches `state.db` again.
pub fn fold_performance(
    sources: &dyn SessionTranscripts,
    contexts: Vec<RunContext>,
    resolver: &AbsorptionResolver,
) -> Result<StatsPerformance, PerformanceError> {
    let mut seen_roots: HashSet<PathBuf> = HashSet::new();
    let mut pipelines: BTreeMap<String, PipelineAcc> = BTreeMap::new();
    let mut model_axis: BTreeMap<String, ModelAxisModelAcc> = BTreeMap::new();
    let mut total_by_harness: BTreeMap<String, HarnessAcc> = BTreeMap::new();
    let mut infrastructure_total_by_harness: BTreeMap<String, HarnessAcc> = BTreeMap::new();
    let mut pipeline_manager_acc = NodeAcc::default();
    let mut merge_resolver_acc = NodeAcc::default();
    let mut harnesses_seen: BTreeSet<String> = BTreeSet::new();
    // #819 — the wait coverage, counted in this very fold over the Node
    // executions it records (Infrastructure and subagents excluded: neither
    // declares a wait). One increment per execution, whatever its harness or
    // how many model buckets it lands in.
    let mut executions: i64 = 0;
    let mut waited_executions: i64 = 0;

    for RunContext {
        run_id,
        started_at,
        payload,
        events,
        roots:
            RunFilesRoots {
                claude_root,
                repo_root,
                stores,
            },
    } in contexts
    {
        // The Run's identity, absorptions applied (#890, ADR-0077).
        let identity = resolver
            .run_identity(&payload)
            .for_run(&run_id, &started_at);
        let node_defs = node_defs_from_payload(&payload);
        let run_harness = payload
            .get("harness")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(pdo_harness::registry::CLAUDE)
            .to_string();

        let pipeline_acc = pipelines.entry(identity.pipeline_key.clone()).or_default();
        identity.adopt_name(&mut pipeline_acc.name);
        pipeline_acc
            .tallies
            .record_run(&identity, &run_id, &started_at);

        // #810: the declared wait of every execution of this Run, resolved in
        // one pass BEFORE the fold — a wait closed by the very `NodeCompleted`
        // that records the observation must already be counted when it lands.
        let waits = declared_waits(&events);
        // The Run's own active duration subtracts every Node wait it contains
        // (ADR-0069 / CONTEXT.md § Durée active): the Infrastructure row stays
        // consistent with the nodes shown under it.
        let run_waited: f64 = waits.values().sum();

        let mut pending: HashMap<(String, i64), PendingNode> = HashMap::new();
        let mut node_starts: HashMap<(String, i64), Vec<NodeStartRecord>> = HashMap::new();
        let mut pipeline_manager_started: Option<String> = None;
        let mut merge_resolver_started: Option<MergeResolverPending> = None;

        for event in &events {
            let iter = event.iter.unwrap_or(1);
            match event.kind {
                EventKind::RunStarted => {
                    pipeline_manager_started = Some(event.ts.clone());
                }
                EventKind::NodeStarted => {
                    let Some(node_id) = event.node_id.clone() else {
                        continue;
                    };
                    let event_payload = event.payload.as_ref();
                    let node_type = event_payload
                        .and_then(|p| p.get("node_type"))
                        .and_then(|v| v.as_str())
                        .or_else(|| node_defs.get(&node_id).map(|d| d.node_type.as_str()))
                        .unwrap_or("")
                        .to_string();
                    // #653/ADR-0060: the FROZEN isolation of this attempt, else
                    // the snapshot's. This decides which directory its sessions
                    // live in, which the type used to imply.
                    let isolated = event_payload
                        .and_then(|p| p.get("isolated_worktree"))
                        .and_then(serde_json::Value::as_bool)
                        .or_else(|| node_defs.get(&node_id).map(|d| d.isolated))
                        .unwrap_or(false);
                    // #810/ADR-0007: the genre this attempt was launched with.
                    // `interactive` has been frozen since #588; `orchestrator`
                    // since #810 — an older payload falls back to the Run's
                    // pipeline snapshot, which has carried it since #723, so
                    // history stays classified.
                    let kind = NodeKind {
                        interactive: event_payload
                            .and_then(|p| p.get("interactive"))
                            .and_then(serde_json::Value::as_bool)
                            .unwrap_or(false),
                        orchestrator: event_payload
                            .and_then(|p| p.get("orchestrator"))
                            .and_then(serde_json::Value::as_bool)
                            .or_else(|| node_defs.get(&node_id).map(|d| d.orchestrator))
                            .unwrap_or(false),
                    };
                    let harness = event_payload
                        .and_then(|p| p.get("harness"))
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                        .unwrap_or(pdo_harness::registry::CLAUDE)
                        .to_string();
                    let session_id = event_payload
                        .and_then(|p| p.get("session_id"))
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                        .map(str::to_string);
                    // The requested model × effort, frozen at spawn (ADR-0046) —
                    // the « By model » axis's fallback when the source is mute
                    // (ADR-0065 §1). Same payload fields the cost fold reads.
                    let requested_model = event_payload
                        .and_then(|p| p.get("model"))
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                        .map(str::to_string);
                    let requested_effort = event_payload
                        .and_then(|p| p.get("effort"))
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                        .map(str::to_string);
                    node_starts
                        .entry((node_id.clone(), iter))
                        .or_default()
                        .push(NodeStartRecord {
                            node_type: node_type.clone(),
                            isolated,
                            harness: harness.clone(),
                            session_id: session_id.clone(),
                        });
                    if node_type == "script" {
                        continue;
                    }
                    pending.insert(
                        (node_id, iter),
                        PendingNode {
                            started_at: event.ts.clone(),
                            harness,
                            session_id,
                            isolated,
                            kind,
                            requested_model,
                            requested_effort,
                        },
                    );
                }
                EventKind::NodeCompleted | EventKind::NodeAutoCompleted => {
                    let Some(node_id) = event.node_id.clone() else {
                        continue;
                    };
                    if let Some(p) = pending.remove(&(node_id.clone(), iter)) {
                        // #892: a Node absorbed under this Pipeline row counts
                        // under its absorbent Node, on both axes.
                        let node = identity.node(&node_id);
                        let node_acc = pipeline_acc.nodes.entry(node.key.clone()).or_default();
                        node.adopt_name(&mut node_acc.name);
                        node_acc
                            .tallies
                            .record_run_as(&node.raw_key, &run_id, &started_at);
                        node_acc.kind = p.kind;
                        let working_dir = if p.isolated {
                            sub_worktree_path(
                                &repo_root,
                                &run_id,
                                event.node_id.as_deref().unwrap_or(""),
                                iter,
                            )
                        } else {
                            worktree_dir_for_run(&repo_root, &run_id)
                        };
                        harnesses_seen.insert(p.harness.clone());
                        let waited = waits.get(&(node_id.clone(), iter)).copied().unwrap_or(0.0);
                        executions += 1;
                        if waited > 0.0 {
                            waited_executions += 1;
                        }
                        record_node_success(
                            sources,
                            node_acc,
                            &mut pipeline_acc.by_harness,
                            &mut total_by_harness,
                            &mut model_axis,
                            &identity,
                            &node,
                            &claude_root,
                            &stores,
                            &working_dir,
                            &p,
                            &event.ts,
                            waited,
                            &mut seen_roots,
                        )?;
                    }
                }
                EventKind::NodeFailed
                | EventKind::NodeStopped
                | EventKind::NodeInterrupted
                | EventKind::NodeStale => {
                    if let Some(node_id) = &event.node_id {
                        pending.remove(&(node_id.clone(), iter));
                    }
                }
                EventKind::MergeResolverStarted => {
                    let payload = event.payload.as_ref();
                    merge_resolver_started = Some(MergeResolverPending {
                        started_at: event.ts.clone(),
                        conflicting_node_id: payload
                            .and_then(|p| p.get("conflicting_node_id"))
                            .and_then(|v| v.as_str())
                            .map(str::to_string),
                        iter: payload.and_then(|p| p.get("iter")).and_then(|v| v.as_i64()),
                    });
                }
                EventKind::MergeResolverCompleted => {
                    if let Some(started) = merge_resolver_started.take() {
                        harnesses_seen.insert(run_harness.clone());
                        let duration = duration_millis(&started.started_at, &event.ts);
                        let (working_dir, excluded, ambiguous) =
                            match (&started.conflicting_node_id, started.iter) {
                                (Some(conflicting_node_id), Some(conflict_iter)) => {
                                    let dir = sub_worktree_path(
                                        &repo_root,
                                        &run_id,
                                        conflicting_node_id,
                                        conflict_iter,
                                    );
                                    let records = node_starts
                                        .get(&(conflicting_node_id.clone(), conflict_iter))
                                        .cloned()
                                        .unwrap_or_default();
                                    let excluded: HashSet<String> = records
                                        .iter()
                                        .filter_map(|r| r.session_id.clone())
                                        .collect();
                                    let ambiguous = records.iter().any(|r| {
                                        r.harness == pdo_harness::registry::CLAUDE
                                            && r.session_id.is_none()
                                    });
                                    (Some(dir), excluded, ambiguous)
                                }
                                _ => (None, HashSet::new(), false),
                            };
                        record_infra_success(
                            sources,
                            &mut merge_resolver_acc,
                            &mut infrastructure_total_by_harness,
                            &run_harness,
                            &claude_root,
                            working_dir.as_deref(),
                            &excluded,
                            ambiguous,
                            duration,
                            // #810: a Merge resolver cannot declare a wait —
                            // its active duration IS its wall-clock.
                            0.0,
                            &mut seen_roots,
                        )?;
                    }
                }
                EventKind::MergeResolverFailed => {
                    merge_resolver_started = None;
                }
                EventKind::RunCompleted => {
                    if let Some(started_at) = pipeline_manager_started.take() {
                        harnesses_seen.insert(run_harness.clone());
                        let duration = duration_millis(&started_at, &event.ts);
                        let topdir = worktree_dir_for_run(&repo_root, &run_id);
                        let excluded: HashSet<String> = node_starts
                            .values()
                            .flatten()
                            .filter_map(|r| r.session_id.clone())
                            .collect();
                        let ambiguous = node_starts.values().flatten().any(|r| {
                            r.node_type != "script"
                                && !r.isolated
                                && r.harness == pdo_harness::registry::CLAUDE
                                && r.session_id.is_none()
                        });
                        record_infra_success(
                            sources,
                            &mut pipeline_manager_acc,
                            &mut infrastructure_total_by_harness,
                            &run_harness,
                            &claude_root,
                            Some(&topdir),
                            &excluded,
                            ambiguous,
                            duration,
                            // #810: the Run's own span carries every declared
                            // wait its Nodes accumulated — so the three modes
                            // reconcile at the Infrastructure level too.
                            run_waited,
                            &mut seen_roots,
                        )?;
                    }
                }
                EventKind::RunFailed | EventKind::RunSkipped => {
                    pipeline_manager_started = None;
                }
                _ => {}
            }
        }
    }

    let mut pipeline_rows: Vec<PerformanceEntity> = pipelines
        .into_iter()
        .map(|(id, acc)| finish_pipeline(id, acc, resolver))
        .collect();
    pipeline_rows.sort_by(|a, b| a.id.cmp(&b.id));

    let mut infrastructure = Vec::new();
    if !pipeline_manager_acc.by_harness.is_empty() {
        infrastructure.push(finish_infra_role(
            "pipeline-manager",
            "Pipeline Manager",
            pipeline_manager_acc,
        ));
    }
    if !merge_resolver_acc.by_harness.is_empty() {
        infrastructure.push(finish_infra_role(
            "merge-resolver",
            "Merge resolver",
            merge_resolver_acc,
        ));
    }

    Ok(StatsPerformance {
        harnesses: harnesses_seen.into_iter().collect(),
        total: PerformanceAggregate {
            harnesses: finish_by_harness(total_by_harness),
        },
        by_pipeline: pipeline_rows,
        infrastructure,
        infrastructure_total: PerformanceAggregate {
            harnesses: finish_by_harness(infrastructure_total_by_harness),
        },
        by_model: wire_model_axis(model_axis, resolver),
        waited_executions,
        executions,
    })
}
