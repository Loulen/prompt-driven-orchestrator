//! The regions of a Run: bounded loops, for-each fan-outs, collection
//! barriers, and the switches' routing record.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::RunState;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, rename = "LoopStateInfo")]
pub struct LoopState {
    pub loop_node_id: String,
    pub current_iter: i64,
    pub max_iter: i64,
    pub break_received: bool,
    pub done: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, rename = "ForEachStateInfo")]
pub struct ForEachState {
    pub foreach_node_id: String,
    pub total_items: i64,
    pub break_received: bool,
    pub done: bool,
}

/// Barrier accounting for a `kind: collection` loop region (ADR-0011 / #269),
/// keyed by region id — the region twin of [`ForEachState`]. Tracks the
/// resolved collection size and whether the barrier has fired.
///
/// `entry` and `members` (#453) make the region's **shape** readable from the
/// projection alone, not just from the pipeline file. The transition guard needs
/// them: a collection region fans its entry out in parallel (one live lap per
/// item), which is the exact opposite of the "at most one live iteration per
/// node" invariant the guard enforces everywhere else. Carrying the shape in the
/// projection keeps `transition_guard` pure — no pipeline plumbing through every
/// caller — and scopes the exemption to the region's own members while its
/// barrier is open.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, rename = "CollectionStateInfo")]
pub struct CollectionState {
    pub region_id: String,
    pub total_items: i64,
    pub done: bool,
    /// The member fanned out once per item. Empty for a `CollectionEmpty`
    /// region (no fan-out happened) and for runs whose `CollectionStarted`
    /// predates #453.
    #[serde(default)]
    pub entry: String,
    /// Every member of the region. Falls back to `[entry]` on a pre-#453
    /// payload; empty when even `entry` is unknown.
    #[serde(default)]
    pub members: Vec<String>,
}

impl CollectionState {
    /// Is `node_id` a node whose iterations this region governs? Such a node is
    /// spawned once per item, so several of its iterations are legitimately live
    /// at the same time (#453).
    pub fn governs(&self, node_id: &str) -> bool {
        self.members.iter().any(|m| m == node_id) || self.entry == node_id
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SwitchState {
    pub switch_node_id: String,
    pub chosen_branch: String,
    pub evaluated_at: String,
}

/// Every projected region-state map on a [`RunState`] that carries an "open"
/// (not-yet-`done`) lifecycle (#601). The point of this enum is a **compile-time
/// exhaustiveness anchor**: the `match` in [`RegionStateKind::open_ids`] has no
/// wildcard arm, so adding a new region-state map to `RunState` means adding a
/// variant here — and every consumer that iterates [`RegionStateKind::ALL`]
/// (notably `run_stall_reason`'s open-region defer) then covers it automatically.
/// Without it, a new region kind falls through to a silent `stalled = false`.
///
/// `switch_states` is deliberately **absent**: a switch is a routing *record*
/// (`SwitchState` has no `done` flag and no open/close lifecycle), not a region
/// that can hold a run open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegionStateKind {
    Loop,
    ForEach,
    Collection,
}

impl RegionStateKind {
    /// Every variant. A new variant added to the enum must be added here too; the
    /// `region_state_kind_all_is_total` test asserts this list is complete, and
    /// [`open_ids`](Self::open_ids)'s wildcard-free `match` fails to compile until
    /// the variant is classified.
    pub const ALL: [RegionStateKind; 3] = [
        RegionStateKind::Loop,
        RegionStateKind::ForEach,
        RegionStateKind::Collection,
    ];

    /// The ids of the currently-open (not `done`) regions of this kind. Exhaustive
    /// `match`, NO wildcard — the compile-time anchor of #601.
    pub(crate) fn open_ids<'a>(self, run: &'a RunState) -> Box<dyn Iterator<Item = String> + 'a> {
        match self {
            RegionStateKind::Loop => Box::new(
                run.loop_states
                    .values()
                    .filter(|s| !s.done)
                    .map(|s| s.loop_node_id.clone()),
            ),
            RegionStateKind::ForEach => Box::new(
                run.foreach_states
                    .values()
                    .filter(|s| !s.done)
                    .map(|s| s.foreach_node_id.clone()),
            ),
            RegionStateKind::Collection => Box::new(
                run.collection_states
                    .values()
                    .filter(|s| !s.done)
                    .map(|s| s.region_id.clone()),
            ),
        }
    }
}
