//! A Run as an extension sees it (API 0.12): who has it open (its
//! [`Presence`]s) and what it cost ([`RunCost`], its tree of child Runs
//! included), read through the
//! [`Host`](crate::Host) or announced on the bus.

use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};

use crate::Actor;

/// The event announcing that an Actor opened a Run nobody under that Actor
/// had open: `{"type": "presence_arrived", "run_id", "actor", "presences"}`,
/// `presences` being the Run's [`Presence`]s once it arrived.
pub const PRESENCE_ARRIVED: &str = "presence_arrived";

/// The event announcing that an Actor no longer has a Run open anywhere (the
/// last of its tabs left the Run, or closed): `{"type": "presence_left",
/// "run_id", "actor", "presences"}`, `presences` being what remains.
pub const PRESENCE_LEFT: &str = "presence_left";

/// An Actor who has a Run open in a browser: the Run is shown in one of its
/// tabs, through that tab's connection to the daemon. Two tabs of one Actor
/// make one Presence; it ends when the last of them leaves the Run or closes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[non_exhaustive]
pub struct Presence {
    pub actor: Actor,
    /// When the Actor opened the Run (RFC 3339, UTC): the arrival of its
    /// first tab on it.
    pub since: String,
}

impl Presence {
    pub fn new(actor: Actor, since: impl Into<String>) -> Self {
        Self {
            actor,
            since: since.into(),
        }
    }
}

/// The cost of a Run, as its Info tab shows it: an estimate from the
/// transcripts, or the figures its harnesses reported (ADR-0022, ADR-0052).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct RunCost {
    /// The amount in US dollars; `None` when it is unknown (no session left
    /// a cost, or a harness has no cost source): show « — », never `0`.
    pub usd: Option<f64>,
    /// Whether `usd` is a lower bound: a model without a price was left out.
    pub partial: bool,
    /// The models left out for want of a price, sorted.
    #[serde(default)]
    pub unpriced_models: Vec<String>,
    /// The harnesses without a cost source the Run ran a node on, sorted:
    /// why `usd` is unknown.
    #[serde(default)]
    pub uncosted_harnesses: Vec<String>,
    /// The **Coût de l'arbre** of a Run that has child Runs: this cost (the
    /// Coût du Run parent) plus every descendant's own, archived ones
    /// included. `None` for a Run without child Run, and on the costs inside
    /// a [`RunTreeCost`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tree: Option<Box<RunTreeCost>>,
}

/// The tree cost of a Run that has child Runs, as its Info tab shows it:
/// the total, then « Parent run » and « Child runs (N) », and the descendants
/// per depth. Each amount follows the rules of a [`RunCost`]: one descendant
/// without a cost makes every sum it belongs to unknown, one lower bound makes
/// them lower bounds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct RunTreeCost {
    /// The parent Run and every descendant.
    pub total: RunCost,
    /// The parent Run alone: the same amount as the [`RunCost`] carrying it.
    pub parent: RunCost,
    /// Every descendant, every depth.
    pub children: RunCost,
    /// How many descendants, every depth.
    pub descendants: u32,
    /// The descendants per depth, the direct children (depth 1) first.
    #[serde(default)]
    pub by_depth: Vec<RunDepthCost>,
}

impl RunTreeCost {
    pub fn new(total: RunCost, parent: RunCost, children: RunCost, descendants: u32) -> Self {
        Self {
            total,
            parent,
            children,
            descendants,
            by_depth: Vec::new(),
        }
    }

    /// The same tree, with its descendants per depth.
    pub fn by_depth(mut self, depths: impl IntoIterator<Item = RunDepthCost>) -> Self {
        self.by_depth = depths.into_iter().collect();
        self
    }
}

/// One depth of a Run's tree: 1 is the direct children, 2 the grandchildren.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct RunDepthCost {
    pub depth: u32,
    /// How many Runs sit at this depth.
    pub runs: u32,
    /// Their summed cost.
    pub cost: RunCost,
}

impl RunDepthCost {
    pub fn new(depth: u32, runs: u32, cost: RunCost) -> Self {
        Self { depth, runs, cost }
    }
}

impl RunCost {
    /// A known amount, complete.
    pub fn new(usd: f64) -> Self {
        Self {
            usd: Some(usd),
            partial: false,
            unpriced_models: Vec::new(),
            uncosted_harnesses: Vec::new(),
            tree: None,
        }
    }

    /// An amount nobody knows.
    pub fn unknown() -> Self {
        Self {
            usd: None,
            partial: false,
            unpriced_models: Vec::new(),
            uncosted_harnesses: Vec::new(),
            tree: None,
        }
    }

    /// The same cost, a lower bound for want of a price for `models`.
    pub fn without_prices_for(mut self, models: impl IntoIterator<Item = String>) -> Self {
        self.unpriced_models = models.into_iter().collect();
        self.partial = !self.unpriced_models.is_empty();
        self
    }

    /// The same cost, for a Run whose child Runs cost `tree`.
    pub fn with_tree(mut self, tree: RunTreeCost) -> Self {
        self.tree = Some(Box::new(tree));
        self
    }

    /// The same cost, unknown because `harnesses` have no cost source.
    pub fn uncosted_by(mut self, harnesses: impl IntoIterator<Item = String>) -> Self {
        self.uncosted_harnesses = harnesses.into_iter().collect();
        if !self.uncosted_harnesses.is_empty() {
            self.usd = None;
        }
        self
    }
}

/// The answer of [`Host::run_cost`](crate::Host::run_cost): the Run's cost,
/// or `None` for a Run the daemon does not know. Await it from a route or a
/// task; from a thread, drive it with `tokio::runtime::Handle::block_on`.
pub type RunCostRead = Pin<Box<dyn Future<Output = Option<RunCost>> + Send>>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_presence_names_its_actor_and_its_arrival() {
        assert_eq!(
            serde_json::to_value(Presence::new(
                Actor::Identified {
                    namespace: "test".into(),
                    id: "alice".into()
                },
                "2026-10-09T10:00:00Z"
            ))
            .unwrap(),
            serde_json::json!({
                "actor": { "kind": "identified", "namespace": "test", "id": "alice" },
                "since": "2026-10-09T10:00:00Z",
            })
        );
    }

    #[test]
    fn a_cost_says_when_it_is_a_lower_bound_or_unknown_and_what_its_tree_costs() {
        let partial = RunCost::new(1.5).without_prices_for(["mystery".to_string()]);
        assert_eq!(partial.usd, Some(1.5));
        assert!(partial.partial);
        let unknown = RunCost::new(1.5).uncosted_by(["opencode".to_string()]);
        assert_eq!(unknown.usd, None);
        assert_eq!(unknown.uncosted_harnesses, ["opencode"]);
        let tree = RunCost::new(0.8).with_tree(
            RunTreeCost::new(RunCost::new(4.2), RunCost::new(0.8), RunCost::new(3.4), 4)
                .by_depth([RunDepthCost::new(1, 3, RunCost::new(2.1))]),
        );
        let json = serde_json::to_value(&tree).unwrap();
        assert_eq!(json["tree"]["total"]["usd"], 4.2);
        assert_eq!(json["tree"]["children"]["usd"], 3.4);
        assert_eq!(json["tree"]["descendants"], 4);
        assert_eq!(json["tree"]["by_depth"][0]["runs"], 3);
        assert!(json["tree"]["parent"].get("tree").is_none());
        assert_eq!(serde_json::from_value::<RunCost>(json).unwrap(), tree);
        assert_eq!(
            serde_json::to_value(RunCost::unknown()).unwrap(),
            serde_json::json!({
                "usd": null, "partial": false, "unpriced_models": [], "uncosted_harnesses": [],
            })
        );
    }
}
