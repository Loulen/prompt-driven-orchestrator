//! The **Coût de l'arbre** of a Run, read (ADR-0101): the
//! Résumés give the tree, each member's own log gives its cost through the
//! memoized per-Run fold, and [`tree_cost`](crate::domain::run_cost::tree)
//! sums them. Nothing is written.

use std::collections::HashMap;

use crate::application::ports::{EventStore, LiveFacts};
use crate::domain::projection::project;
use crate::domain::run_cost::tree::{tree_cost, RunTree};
use crate::domain::run_summary::RunSummary;
use crate::domain::state::{CostStat, TreeCost};

/// Reads the tree costs of the Runs of one log, each member's cost folded at
/// most once per reader: a grandchild belongs to its parent's tree and to its
/// grandparent's, it is read once.
///
/// The tree is the log's, whoever reads it: a tree cost is the cost of the
/// whole orchestration, so it counts a descendant the caller may not open,
/// as an aggregate (its cost alone, never its content).
pub struct TreeCosts<'a> {
    store: &'a dyn EventStore,
    live_facts: &'a dyn LiveFacts,
    /// Every Run the log holds, archived ones included; a forgotten Run is
    /// not there.
    tree: RunTree,
    costs: HashMap<String, Option<CostStat>>,
}

impl<'a> TreeCosts<'a> {
    /// A reader over the tree the Résumés `summaries` describe.
    pub fn new(
        store: &'a dyn EventStore,
        live_facts: &'a dyn LiveFacts,
        summaries: &[RunSummary],
    ) -> Self {
        Self {
            store,
            live_facts,
            tree: RunTree::from_links(
                summaries
                    .iter()
                    .map(|s| (s.run_id.as_str(), s.parent_run_id.as_deref())),
            ),
            costs: HashMap::new(),
        }
    }

    /// The tree cost of `run_id` alone, the Run's own cost already known
    /// (`own`, the one its detail shows). `None` for a Run without child,
    /// found without reading the Résumés, and when the Résumés cannot be read:
    /// the Run then shows its own cost, as it did before tree costs.
    pub async fn of_run(
        store: &'a dyn EventStore,
        live_facts: &'a dyn LiveFacts,
        run_id: &str,
        own: Option<CostStat>,
    ) -> Option<TreeCost> {
        if !store.has_children(run_id).await {
            return None;
        }
        let summaries = store.summaries().await.ok()?;
        let mut reader = Self::new(store, live_facts, &summaries);
        reader.costs.insert(run_id.to_string(), own);
        reader.tree_cost(run_id).await
    }

    /// The tree cost of `run_id`; `None` when it has no descendant.
    pub async fn tree_cost(&mut self, run_id: &str) -> Option<TreeCost> {
        let members = self.tree.descendants(run_id);
        if members.is_empty() {
            return None;
        }
        let parent = self.cost(run_id).await;
        let mut costs = Vec::with_capacity(members.len());
        for (member, depth) in members {
            costs.push((depth, self.cost(&member).await));
        }
        tree_cost(parent, &costs)
    }

    /// The own cost of `run_id`, the Coût du Run parent; `None` when it has
    /// none, or its log no longer projects.
    pub async fn cost(&mut self, run_id: &str) -> Option<CostStat> {
        if let Some(cost) = self.costs.get(run_id) {
            return cost.clone();
        }
        let cost = match self.store.events(run_id).await {
            Ok(events) => match project(&events) {
                Some(run) => self.live_facts.cost(&run, &events).await,
                None => None,
            },
            Err(_) => None,
        };
        self.costs.insert(run_id.to_string(), cost.clone());
        cost
    }
}
