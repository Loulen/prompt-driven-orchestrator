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
//! tree** written by `pdo_workspace::adapters::git::revisions::snapshot_worktree` (not a commit): a
//! comment anchored on it keeps a stable object to re-map from (#752).

use serde::Serialize;
use ts_rs::TS;

use crate::domain::event::{Event, EventKind};
use crate::domain::state::{NodeStatus, RunState};

mod collect;
pub use collect::{collect, is_default_pair, resolve_id, Resolved};

pub const FORK_ID: &str = "fork";
pub const TIP_ID: &str = "tip";
/// The Run's shared working tree — commits plus uncommitted edits (#835).
pub const WORKTREE_ID: &str = "worktree";

#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq, TS)]
#[ts(export, rename = "RunRefKind")]
#[serde(rename_all = "snake_case")]
pub enum RefKind {
    Fork,
    Tip,
    Before,
    After,
    Live,
    /// The Run's shared working tree, uncommitted edits included (#835).
    Worktree,
}

/// One Run ref, as the UI and the CLI see it.
#[derive(Serialize, Debug, Clone, PartialEq, Eq, TS)]
#[ts(export)]
pub struct RunRef {
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
    #[ts(optional)]
    pub node_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub node_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub iter: Option<i64>,
}

#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq, TS)]
#[ts(export, rename = "RunDeliveryStatus")]
#[serde(rename_all = "snake_case")]
pub enum DeliveryStatus {
    Delivered,
    Running,
}

/// One node delivery (or one running node's live branch), as a ready-made pair
/// the Review page can select in one click.
#[derive(Serialize, Debug, Clone, PartialEq, Eq, TS)]
#[ts(export, rename = "RunDelivery")]
pub struct Delivery {
    pub node_id: String,
    pub node_name: String,
    pub iter: i64,
    pub status: DeliveryStatus,
    /// Ref id of the pair's source.
    pub before: String,
    /// Ref id of the pair's destination when delivered.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub after: Option<String>,
    /// Ref id of the live sub-worktree branch when running.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub live: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub delivered_at: Option<String>,
}

/// The answer of `GET /runs/<id>/refs`.
#[derive(Serialize, Debug, Clone, TS)]
#[ts(export)]
pub struct RunRefs {
    /// Order: fork, deliveries in delivery order, live branches, tip, worktree
    /// (when the Run's worktree exists).
    pub refs: Vec<RunRef>,
    pub deliveries: Vec<Delivery>,
    pub default_from: &'static str,
    pub default_to: &'static str,
}

/// The Run's branch — the tip.
pub fn tip_branch(run_id: &str) -> String {
    format!("pdo/run-{run_id}")
}

/// What the `worktree` ref shows as its git side: the Run's worktree, relative
/// to the target repository (the path `worktree_ops::worktree_dir_for_run` builds).
pub fn worktree_display(run_id: &str) -> String {
    format!(".pdo/runs/{run_id}/worktree")
}

/// The Run's stable base: its frozen fork SHA, else the source branch, else
/// `HEAD` (pre-fork-sha logs only). Never the shared checkout's wandering HEAD
/// when a fork point is known (#417).
pub fn fork_base(run_state: &RunState) -> &str {
    run_state
        .fork_sha
        .as_deref()
        .or(run_state.source_branch.as_deref())
        .unwrap_or("HEAD")
}

pub fn before_id(node_id: &str, iter: i64) -> String {
    format!("node:{node_id}:{iter}:before")
}

pub fn after_id(node_id: &str, iter: i64) -> String {
    format!("node:{node_id}:{iter}:after")
}

pub fn live_id(node_id: &str) -> String {
    format!("live:{node_id}")
}
