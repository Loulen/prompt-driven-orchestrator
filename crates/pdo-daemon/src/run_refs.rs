//! Refs of the Run (#749, ADR-0067 §1; CONTEXT.md § "Ref de Run").
//!
//! A **Run ref** is a persistent git point the Run knows about, offered as the
//! source or destination of the Review page: the **fork point**, the **Run tip**,
//! and for every node delivery its **`before`/`after`** SHAs (read from the
//! `NodeDelivered` events of the log, labelled by node name and `iter`). A
//! running isolated node adds its **live** sub-worktree branch.
//!
//! Every ref carries a **stable id** — `fork`, `tip`, `node:<id>:<iter>:before`,
//! `node:<id>:<iter>:after`, `live:<id>` — that the Review page puts in its URL
//! instead of a SHA: a link keeps its meaning while the Run moves, and the
//! `pdo/sub-*` branches, which die at merge-back, never become an identity
//! (ADR-0067 §1). The diff and file-at-ref endpoints accept those ids and
//! resolve them here, so the labels and the resolution have one source.
//!
//! While the Run's shared worktree exists, a **`worktree`** ref (#835) points at
//! its working tree — commits *and* uncommitted edits, untracked files included.
//! It is the default destination: a running node does not have to commit for
//! its work to show in the Review page. Its `sha` is the id of a **snapshot
//! tree** written by `structured_diff::snapshot_worktree` (not a commit): a
//! comment anchored on it keeps a stable object to re-map from (#752).

use serde::Serialize;

use crate::event_log::{Event, EventKind, NodeStatus, RunState};

pub(crate) const FORK_ID: &str = "fork";
pub(crate) const TIP_ID: &str = "tip";
/// The Run's shared working tree — commits plus uncommitted edits (#835).
pub(crate) const WORKTREE_ID: &str = "worktree";

#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RefKind {
    Fork,
    Tip,
    Before,
    After,
    Live,
    /// The Run's shared working tree, uncommitted edits included (#835).
    Worktree,
}

/// One Run ref, as the UI and the CLI see it.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub(crate) struct RunRef {
    /// Stable id, the only thing a URL should carry.
    pub id: String,
    pub kind: RefKind,
    /// Human label built server-side, e.g. `implement · iter 1 · after`.
    pub label: String,
    /// What the id resolves to for git: a SHA for a delivery, a branch for the
    /// tip and a live node, the fork SHA (or source branch) for the fork point.
    pub git_ref: String,
    /// `git_ref` resolved to a commit SHA; `None` when it does not resolve any
    /// more (an archived Run, a live branch already merged back).
    pub sha: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iter: Option<i64>,
}

#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DeliveryStatus {
    Delivered,
    Running,
}

/// One node delivery (or one running node's live branch), as a ready-made pair
/// the Review page can select in one click.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub(crate) struct Delivery {
    pub node_id: String,
    pub node_name: String,
    pub iter: i64,
    pub status: DeliveryStatus,
    /// Ref id of the pair's source.
    pub before: String,
    /// Ref id of the pair's destination when delivered.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
    /// Ref id of the live sub-worktree branch when running.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub live: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivered_at: Option<String>,
}

/// The answer of `GET /runs/<id>/refs`.
#[derive(Serialize, Debug, Clone)]
pub(crate) struct RunRefs {
    /// Order: fork, deliveries in delivery order, live branches, tip, worktree
    /// (when the Run's worktree exists).
    pub refs: Vec<RunRef>,
    pub deliveries: Vec<Delivery>,
    pub default_from: &'static str,
    pub default_to: &'static str,
}

/// The Run's branch — the tip.
pub(crate) fn tip_branch(run_id: &str) -> String {
    format!("pdo/run-{run_id}")
}

/// What the `worktree` ref shows as its git side: the Run's worktree, relative
/// to the target repository (the path `worktree_ops::worktree_dir_for_run` builds).
pub(crate) fn worktree_display(run_id: &str) -> String {
    format!(".pdo/runs/{run_id}/worktree")
}

/// The Run's stable base: its frozen fork SHA, else the source branch, else
/// `HEAD` (pre-fork-sha logs only). Never the shared checkout's wandering HEAD
/// when a fork point is known (#417).
pub(crate) fn fork_base(run_state: &RunState) -> &str {
    run_state
        .fork_sha
        .as_deref()
        .or(run_state.source_branch.as_deref())
        .unwrap_or("HEAD")
}

pub(crate) fn before_id(node_id: &str, iter: i64) -> String {
    format!("node:{node_id}:{iter}:before")
}

pub(crate) fn after_id(node_id: &str, iter: i64) -> String {
    format!("node:{node_id}:{iter}:after")
}

pub(crate) fn live_id(node_id: &str) -> String {
    format!("live:{node_id}")
}

fn node_name(run_state: &RunState, node_id: &str) -> String {
    run_state
        .node_defs
        .iter()
        .find(|d| d.id == node_id)
        .and_then(|d| d.name.clone())
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| node_id.to_string())
}

/// One `NodeDelivered` event, read back from the log.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DeliveredEvent {
    node_id: String,
    iter: i64,
    before: String,
    after: String,
    ts: String,
}

/// Every delivery in the log, in delivery order; a re-delivery of the same
/// (node, iter) replaces the earlier one in place (the last one is what the
/// branch carries).
fn deliveries_in_log(events: &[Event]) -> Vec<DeliveredEvent> {
    let mut out: Vec<DeliveredEvent> = Vec::new();
    for e in events {
        if e.kind != EventKind::NodeDelivered {
            continue;
        }
        let (Some(node_id), Some(payload)) = (&e.node_id, &e.payload) else {
            continue;
        };
        let (Some(before), Some(after)) = (
            payload.get("before").and_then(|v| v.as_str()),
            payload.get("after").and_then(|v| v.as_str()),
        ) else {
            continue;
        };
        let iter = e.iter.unwrap_or(1);
        let d = DeliveredEvent {
            node_id: node_id.clone(),
            iter,
            before: before.to_string(),
            after: after.to_string(),
            ts: e.ts.clone(),
        };
        if let Some(slot) = out
            .iter_mut()
            .find(|x| x.node_id == d.node_id && x.iter == d.iter)
        {
            *slot = d;
        } else {
            out.push(d);
        }
    }
    out
}

/// A running node whose sub-worktree branch may still exist: isolated, and
/// live. Its branch name is deterministic (`worktree_ops::sub_worktree_branch`).
fn live_nodes(run_id: &str, run_state: &RunState) -> Vec<(String, i64, String)> {
    let mut out: Vec<(String, i64, String)> = run_state
        .nodes
        .values()
        .filter(|n| matches!(n.status, NodeStatus::Running | NodeStatus::AwaitingUser))
        .filter(|n| n.isolated_worktree == Some(true))
        .map(|n| {
            (
                n.node_id.clone(),
                n.iter,
                crate::worktree_ops::sub_worktree_branch(run_id, &n.node_id, n.iter),
            )
        })
        .collect();
    // HashMap order is arbitrary; the list must be stable for the UI.
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Build the Run's refs. `sha_of` resolves a git ref to a commit SHA (`None`
/// when it does not exist) — injected so the listing is testable without a
/// repository; the HTTP handler passes `structured_diff::rev_parse`.
/// `worktree` is `Some` while the Run's worktree directory exists, `None` once
/// it is gone: with it the `worktree` ref is listed and is the default
/// destination (#835). The inner value is the worktree's snapshot tree id
/// (`structured_diff::snapshot_worktree`), `None` when snapshotting failed — the
/// ref is still listed (without a `sha`) so this listing and the diff endpoint,
/// which defaults on the directory alone, never disagree on the default.
///
/// A live branch that no longer resolves (merged back between two projections)
/// is dropped: offering it would make the picker lie.
pub(crate) fn collect(
    run_id: &str,
    run_state: &RunState,
    events: &[Event],
    sha_of: &dyn Fn(&str) -> Option<String>,
    worktree: Option<Option<&str>>,
) -> RunRefs {
    let mut refs: Vec<RunRef> = Vec::new();
    let mut deliveries: Vec<Delivery> = Vec::new();

    let fork = fork_base(run_state).to_string();
    refs.push(RunRef {
        id: FORK_ID.to_string(),
        kind: RefKind::Fork,
        label: "Fork point".to_string(),
        sha: sha_of(&fork),
        git_ref: fork,
        node_id: None,
        node_name: None,
        iter: None,
    });

    for d in deliveries_in_log(events) {
        let name = node_name(run_state, &d.node_id);
        refs.push(RunRef {
            id: before_id(&d.node_id, d.iter),
            kind: RefKind::Before,
            label: format!("{name} · iter {} · before", d.iter),
            sha: Some(d.before.clone()),
            git_ref: d.before.clone(),
            node_id: Some(d.node_id.clone()),
            node_name: Some(name.clone()),
            iter: Some(d.iter),
        });
        refs.push(RunRef {
            id: after_id(&d.node_id, d.iter),
            kind: RefKind::After,
            label: format!("{name} · iter {} · after", d.iter),
            sha: Some(d.after.clone()),
            git_ref: d.after.clone(),
            node_id: Some(d.node_id.clone()),
            node_name: Some(name.clone()),
            iter: Some(d.iter),
        });
        deliveries.push(Delivery {
            node_id: d.node_id.clone(),
            node_name: name,
            iter: d.iter,
            status: DeliveryStatus::Delivered,
            before: before_id(&d.node_id, d.iter),
            after: Some(after_id(&d.node_id, d.iter)),
            live: None,
            delivered_at: Some(d.ts),
        });
    }

    for (node_id, iter, branch) in live_nodes(run_id, run_state) {
        let Some(sha) = sha_of(&branch) else { continue };
        let name = node_name(run_state, &node_id);
        refs.push(RunRef {
            id: live_id(&node_id),
            kind: RefKind::Live,
            label: format!("{name} · iter {iter} · live"),
            sha: Some(sha),
            git_ref: branch,
            node_id: Some(node_id.clone()),
            node_name: Some(name.clone()),
            iter: Some(iter),
        });
        deliveries.push(Delivery {
            node_id: node_id.clone(),
            node_name: name,
            iter,
            status: DeliveryStatus::Running,
            // What the node was cut from is the Run's branch; its live branch is
            // what it has not merged back yet.
            before: TIP_ID.to_string(),
            after: None,
            live: Some(live_id(&node_id)),
            delivered_at: None,
        });
    }

    let tip = tip_branch(run_id);
    refs.push(RunRef {
        id: TIP_ID.to_string(),
        kind: RefKind::Tip,
        label: "Run tip".to_string(),
        sha: sha_of(&tip),
        git_ref: tip,
        node_id: None,
        node_name: None,
        iter: None,
    });

    let default_to = match worktree {
        Some(snapshot) => {
            refs.push(RunRef {
                id: WORKTREE_ID.to_string(),
                kind: RefKind::Worktree,
                label: "Working tree".to_string(),
                sha: snapshot.map(str::to_string),
                git_ref: worktree_display(run_id),
                node_id: None,
                node_name: None,
                iter: None,
            });
            WORKTREE_ID
        }
        None => TIP_ID,
    };

    RunRefs {
        refs,
        deliveries,
        default_from: FORK_ID,
        default_to,
    }
}

/// What a `from`/`to` query value turned out to be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Resolved {
    /// A Run ref id, resolved to the git ref to hand to `git`.
    Git(String),
    /// The `worktree` id (#835): the Run's working tree, snapshotted by the
    /// caller (`structured_diff::snapshot_worktree`) — a 404 when it is gone.
    Worktree,
    /// Shaped like a Run ref id but the Run knows no such ref (a node that never
    /// delivered at that iter, a node that is not running, …).
    Unknown,
    /// Not a Run ref id at all: the caller decides (a raw ref for compatibility).
    NotAnId,
}

/// Resolve a stable ref id against the Run's log. Pure: no git call.
pub(crate) fn resolve_id(
    run_id: &str,
    run_state: &RunState,
    events: &[Event],
    id: &str,
) -> Resolved {
    if id == FORK_ID {
        return Resolved::Git(fork_base(run_state).to_string());
    }
    if id == TIP_ID {
        return Resolved::Git(tip_branch(run_id));
    }
    if id == WORKTREE_ID {
        return Resolved::Worktree;
    }
    if let Some(rest) = id.strip_prefix("node:") {
        // `<node id>:<iter>:<side>` — the node id may itself contain `:`, so
        // split from the right.
        let Some((head, side)) = rest.rsplit_once(':') else {
            return Resolved::Unknown;
        };
        let Some((node_id, iter)) = head.rsplit_once(':') else {
            return Resolved::Unknown;
        };
        let Ok(iter) = iter.parse::<i64>() else {
            return Resolved::Unknown;
        };
        let Some(d) = deliveries_in_log(events)
            .into_iter()
            .find(|d| d.node_id == node_id && d.iter == iter)
        else {
            return Resolved::Unknown;
        };
        return match side {
            "before" => Resolved::Git(d.before),
            "after" => Resolved::Git(d.after),
            _ => Resolved::Unknown,
        };
    }
    if let Some(node_id) = id.strip_prefix("live:") {
        return match live_nodes(run_id, run_state)
            .into_iter()
            .find(|(n, _, _)| n == node_id)
        {
            Some((_, _, branch)) => Resolved::Git(branch),
            None => Resolved::Unknown,
        };
    }
    Resolved::NotAnId
}

/// `true` when the pair is the fork → tip (or fork → worktree, #835) default,
/// which is compared three-dot (merge-base) like the LOC stat. `None` = the
/// side was not given.
pub(crate) fn is_default_pair(from: Option<&str>, to: Option<&str>) -> bool {
    matches!(from, None | Some(FORK_ID)) && matches!(to, None | Some(TIP_ID) | Some(WORKTREE_ID))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_log::{now_iso, project};

    fn run_started(run_id: &str, node_defs: serde_json::Value) -> Event {
        Event {
            id: None,
            run_id: run_id.into(),
            ts: now_iso(),
            kind: EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({
                "pipeline_name": "p",
                "fork_sha": "f0f0f0f",
                "source_branch": "main",
                "node_defs": node_defs,
                "edges": [],
            })),
        }
    }

    fn node_started(run_id: &str, node: &str, iter: i64, isolated: bool) -> Event {
        Event {
            id: None,
            run_id: run_id.into(),
            ts: now_iso(),
            kind: EventKind::NodeStarted,
            node_id: Some(node.into()),
            iter: Some(iter),
            payload: Some(serde_json::json!({ "isolated_worktree": isolated })),
        }
    }

    fn delivered(run_id: &str, node: &str, iter: i64, before: &str, after: &str) -> Event {
        Event {
            id: None,
            run_id: run_id.into(),
            ts: format!("2026-09-09T10:00:0{iter}Z"),
            kind: EventKind::NodeDelivered,
            node_id: Some(node.into()),
            iter: Some(iter),
            payload: Some(serde_json::json!({ "before": before, "after": after })),
        }
    }

    fn completed(run_id: &str, node: &str, iter: i64) -> Event {
        Event {
            id: None,
            run_id: run_id.into(),
            ts: now_iso(),
            kind: EventKind::NodeCompleted,
            node_id: Some(node.into()),
            iter: Some(iter),
            payload: None,
        }
    }

    const RUN: &str = "20260909-run";

    fn two_deliveries_and_a_live_node() -> Vec<Event> {
        vec![
            run_started(
                RUN,
                serde_json::json!([
                    { "id": "design", "name": "Design", "node_type": "agent", "inputs": [], "outputs": [] },
                    { "id": "impl", "name": "Implement", "node_type": "agent", "inputs": [], "outputs": [] },
                    { "id": "review", "name": "Review", "node_type": "agent", "inputs": [], "outputs": [] },
                ]),
            ),
            node_started(RUN, "design", 1, true),
            delivered(RUN, "design", 1, "aaa1", "bbb2"),
            completed(RUN, "design", 1),
            node_started(RUN, "impl", 1, false),
            delivered(RUN, "impl", 1, "bbb2", "ccc3"),
            completed(RUN, "impl", 1),
            node_started(RUN, "review", 1, true),
        ]
    }

    #[test]
    fn lists_fork_deliveries_live_and_tip_in_order_with_labels() {
        let events = two_deliveries_and_a_live_node();
        let state = project(&events).unwrap();
        let sha_of = |r: &str| -> Option<String> {
            match r {
                "f0f0f0f" => Some("f0f0f0f".into()),
                "pdo/run-20260909-run" => Some("ccc3".into()),
                "pdo/sub-20260909-run-review-iter-1" => Some("ddd4".into()),
                _ => None,
            }
        };
        let refs = collect(RUN, &state, &events, &sha_of, None);

        let ids: Vec<&str> = refs.refs.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                "fork",
                "node:design:1:before",
                "node:design:1:after",
                "node:impl:1:before",
                "node:impl:1:after",
                "live:review",
                "tip",
            ]
        );
        let labels: Vec<&str> = refs.refs.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(
            labels,
            vec![
                "Fork point",
                "Design · iter 1 · before",
                "Design · iter 1 · after",
                "Implement · iter 1 · before",
                "Implement · iter 1 · after",
                "Review · iter 1 · live",
                "Run tip",
            ]
        );
        // Delivery refs are the frozen SHAs of the log; the tip and the live
        // branch resolve through git.
        assert_eq!(refs.refs[2].sha.as_deref(), Some("bbb2"));
        assert_eq!(refs.refs[2].git_ref, "bbb2");
        assert_eq!(refs.refs[5].git_ref, "pdo/sub-20260909-run-review-iter-1");
        assert_eq!(refs.refs[5].sha.as_deref(), Some("ddd4"));
        assert_eq!(refs.refs[6].git_ref, "pdo/run-20260909-run");
        assert_eq!(refs.refs[0].git_ref, "f0f0f0f");

        assert_eq!(refs.deliveries.len(), 3);
        assert_eq!(refs.deliveries[0].node_name, "Design");
        assert_eq!(refs.deliveries[0].status, DeliveryStatus::Delivered);
        assert_eq!(refs.deliveries[0].before, "node:design:1:before");
        assert_eq!(
            refs.deliveries[0].after.as_deref(),
            Some("node:design:1:after")
        );
        assert_eq!(refs.deliveries[2].status, DeliveryStatus::Running);
        assert_eq!(refs.deliveries[2].before, "tip");
        assert_eq!(refs.deliveries[2].live.as_deref(), Some("live:review"));
        assert_eq!(refs.default_from, "fork");
        assert_eq!(refs.default_to, "tip");
    }

    #[test]
    fn a_live_branch_that_no_longer_resolves_is_dropped() {
        let events = two_deliveries_and_a_live_node();
        let state = project(&events).unwrap();
        let sha_of = |_: &str| -> Option<String> { None };
        let refs = collect(RUN, &state, &events, &sha_of, None);
        assert!(refs.refs.iter().all(|r| r.kind != RefKind::Live));
        assert_eq!(refs.deliveries.len(), 2);
        // Fork and tip stay listed even unresolved (an archived Run): the UI
        // says "not preserved", the picker does not lie about a missing ref.
        assert_eq!(refs.refs[0].sha, None);
        assert_eq!(refs.refs.last().unwrap().sha, None);
    }

    #[test]
    fn a_redelivery_of_the_same_iter_replaces_in_place() {
        let mut events = two_deliveries_and_a_live_node();
        events.push(delivered(RUN, "design", 1, "aaa1", "eee5"));
        let state = project(&events).unwrap();
        let refs = collect(RUN, &state, &events, &|_| None, None);
        let design_after = refs
            .refs
            .iter()
            .find(|r| r.id == "node:design:1:after")
            .unwrap();
        assert_eq!(design_after.sha.as_deref(), Some("eee5"));
        // Still first among the deliveries.
        assert_eq!(refs.deliveries[0].node_id, "design");
        assert_eq!(
            refs.deliveries
                .iter()
                .filter(|d| d.node_id == "design")
                .count(),
            1
        );
    }

    #[test]
    fn a_node_without_a_name_is_labelled_by_its_id() {
        let events = vec![
            run_started(
                RUN,
                serde_json::json!([{ "id": "w-1", "node_type": "agent", "inputs": [], "outputs": [] }]),
            ),
            node_started(RUN, "w-1", 2, false),
            delivered(RUN, "w-1", 2, "a", "b"),
        ];
        let state = project(&events).unwrap();
        let refs = collect(RUN, &state, &events, &|_| None, None);
        assert_eq!(refs.refs[1].label, "w-1 · iter 2 · before");
    }

    #[test]
    fn resolves_stable_ids_and_refuses_unknown_ones() {
        let events = two_deliveries_and_a_live_node();
        let state = project(&events).unwrap();
        let r = |id: &str| resolve_id(RUN, &state, &events, id);
        assert_eq!(r("fork"), Resolved::Git("f0f0f0f".into()));
        assert_eq!(r("tip"), Resolved::Git("pdo/run-20260909-run".into()));
        assert_eq!(r("worktree"), Resolved::Worktree);
        assert_eq!(r("node:design:1:before"), Resolved::Git("aaa1".into()));
        assert_eq!(r("node:design:1:after"), Resolved::Git("bbb2".into()));
        assert_eq!(r("node:impl:1:after"), Resolved::Git("ccc3".into()));
        assert_eq!(
            r("live:review"),
            Resolved::Git("pdo/sub-20260909-run-review-iter-1".into())
        );
        // Unknown shapes of a known prefix are refused, never passed to git.
        assert_eq!(r("node:design:2:after"), Resolved::Unknown);
        assert_eq!(r("node:ghost:1:after"), Resolved::Unknown);
        assert_eq!(r("node:design:1:sideways"), Resolved::Unknown);
        assert_eq!(r("node:design"), Resolved::Unknown);
        assert_eq!(r("node:design:x:after"), Resolved::Unknown);
        assert_eq!(r("live:impl"), Resolved::Unknown, "impl is not running");
        assert_eq!(r("live:nobody"), Resolved::Unknown);
        // A raw ref is not an id: the caller keeps the compatibility path.
        assert_eq!(r("HEAD~1"), Resolved::NotAnId);
        assert_eq!(r("deadbeef"), Resolved::NotAnId);
    }

    #[test]
    fn fork_base_prefers_the_frozen_fork_sha() {
        let events = two_deliveries_and_a_live_node();
        let mut state = project(&events).unwrap();
        assert_eq!(fork_base(&state), "f0f0f0f");
        state.fork_sha = None;
        assert_eq!(fork_base(&state), "main");
        state.source_branch = None;
        assert_eq!(fork_base(&state), "HEAD");
    }

    #[test]
    fn a_present_worktree_is_listed_last_and_becomes_the_default_destination() {
        // #835: the Run's working tree is a ref while it exists, and the default
        // `to`; its sha is the snapshot tree id handed in, not a commit.
        let events = two_deliveries_and_a_live_node();
        let state = project(&events).unwrap();
        let refs = collect(RUN, &state, &events, &|_| None, Some(Some("7ee7ee7")));
        let last = refs.refs.last().unwrap();
        assert_eq!(last.id, "worktree");
        assert_eq!(last.kind, RefKind::Worktree);
        assert_eq!(last.label, "Working tree");
        assert_eq!(last.sha.as_deref(), Some("7ee7ee7"));
        assert_eq!(last.git_ref, ".pdo/runs/20260909-run/worktree");
        assert_eq!(
            refs.refs[refs.refs.len() - 2].id,
            "tip",
            "tip stays offered"
        );
        assert_eq!(refs.default_from, "fork");
        assert_eq!(refs.default_to, "worktree");
        // Worktree present but unsnapshottable: still listed (no sha) and still
        // the default, so the listing agrees with the diff endpoint.
        let unsnapped = collect(RUN, &state, &events, &|_| None, Some(None));
        let last = unsnapped.refs.last().unwrap();
        assert_eq!(last.id, "worktree");
        assert_eq!(last.sha, None);
        assert_eq!(unsnapped.default_to, "worktree");
        // Without a worktree (finished, archived): no such ref, tip is the default.
        let gone = collect(RUN, &state, &events, &|_| None, None);
        assert!(gone.refs.iter().all(|r| r.id != "worktree"));
        assert_eq!(gone.default_to, "tip");
    }

    #[test]
    fn default_pair_is_fork_to_tip_or_nothing() {
        assert!(is_default_pair(None, None));
        assert!(is_default_pair(Some("fork"), Some("tip")));
        assert!(is_default_pair(Some("fork"), Some("worktree")));
        assert!(is_default_pair(None, Some("worktree")));
        assert!(!is_default_pair(Some("tip"), Some("worktree")));
        assert!(is_default_pair(Some("fork"), None));
        assert!(!is_default_pair(Some("tip"), Some("fork")));
        assert!(!is_default_pair(
            Some("node:a:1:before"),
            Some("node:a:1:after")
        ));
        assert!(!is_default_pair(None, Some("abc123")));
    }
}
