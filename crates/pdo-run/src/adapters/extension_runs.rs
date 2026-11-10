//! The Runs an extension creates through its `Host` (API 0.9): the same
//! create as `POST /runs`, for the Actor the extension names. And a Run as
//! an extension sees it (API 0.12): its Presences, and its cost as its Info
//! tab shows it, the cost of its tree of child Runs included.

use std::sync::{Arc, Weak};

use pdo_extension_api::{
    NewRun, Presence, RunCost, RunCostRead, RunCreation, RunDepthCost, RunNotCreated, RunTreeCost,
};
use pdo_kernel::extension_host::{RunCreator, RunViews};
use pdo_kernel::Actor;

use crate::application::create_run::{reject_unknown_create_run_fields, CreateRunRequest};
use crate::application::tree_cost::TreeCosts;
use crate::domain::projection::project;
use crate::domain::state::{CostStat, TreeCost};
use crate::Runs;

/// The run context, as what creates the extensions' Runs. Weak: the
/// extensions must not keep the Runs alive past the daemon.
pub struct ExtensionRuns(pub Weak<Runs>);

impl ExtensionRuns {
    pub fn of(runs: &Arc<Runs>) -> Self {
        Self(Arc::downgrade(runs))
    }
}

impl RunCreator for ExtensionRuns {
    fn create_run(&self, actor: Actor, run: NewRun) -> RunCreation {
        let runs = self.0.upgrade();
        Box::pin(async move {
            let Some(runs) = runs else {
                return Err(RunNotCreated::unavailable("the daemon is stopping"));
            };
            // The body `POST /runs` would receive, read as it reads it.
            let body = run.to_request_body();
            reject_unknown_create_run_fields(&body)
                .map_err(|(status, body)| RunNotCreated::new(status.as_u16(), body))?;
            let request: CreateRunRequest = serde_json::from_value(body).map_err(|e| {
                RunNotCreated::new(
                    400,
                    serde_json::json!({ "error": format!("invalid run: {e}") }),
                )
            })?;
            // A root Run (no node session), created by `actor`.
            let response = runs.create_run(&actor, request, Vec::new(), None).await;
            let status = response.status();
            let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap_or_default();
            let answer: serde_json::Value =
                serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
            if status.is_success() {
                match answer.get("run_id").and_then(|id| id.as_str()) {
                    Some(id) => Ok(id.to_string()),
                    None => Err(RunNotCreated::new(500, answer)),
                }
            } else {
                Err(RunNotCreated::new(status.as_u16(), answer))
            }
        })
    }
}

impl RunViews for ExtensionRuns {
    fn presences(&self, run_id: &str) -> Vec<Presence> {
        self.0
            .upgrade()
            .map(|runs| runs.presences(run_id))
            .unwrap_or_default()
    }

    /// The cost `GET /runs/{id}` serves the Info tab, read the same way: the
    /// Run's projection, then its sessions on disk, then its tree when it has
    /// child Runs.
    fn run_cost(&self, run_id: &str) -> RunCostRead {
        let runs = self.0.upgrade();
        let run_id = run_id.to_string();
        Box::pin(async move {
            let runs = runs?;
            let events = runs.store().events(&run_id).await.ok()?;
            let run = project(&events)?;
            let own = runs.live_facts.cost(&run, &events).await;
            let tree = TreeCosts::of_run(
                runs.store().as_ref(),
                runs.live_facts.as_ref(),
                &run_id,
                own.clone(),
            )
            .await;
            let cost = run_cost_of(own);
            Some(match tree {
                Some(tree) => cost.with_tree(run_tree_cost_of(tree)),
                None => cost,
            })
        })
    }
}

/// A Run's cost as the extension API carries it; `None` (no session left a
/// cost) is an unknown amount, never `0`.
fn run_cost_of(cost: Option<CostStat>) -> RunCost {
    let Some(cost) = cost else {
        return RunCost::unknown();
    };
    let mut read = RunCost::new(cost.usd)
        .without_prices_for(cost.unpriced_models)
        .uncosted_by(cost.uncosted_harnesses);
    read.partial = cost.partial;
    read
}

/// A Run's tree cost as the extension API carries it, every amount read as
/// [`run_cost_of`] reads a Run's.
fn run_tree_cost_of(tree: TreeCost) -> RunTreeCost {
    RunTreeCost::new(
        run_cost_of(tree.total),
        run_cost_of(tree.parent),
        run_cost_of(tree.children),
        tree.descendants,
    )
    .by_depth(
        tree.by_depth
            .into_iter()
            .map(|d| RunDepthCost::new(d.depth, d.runs, run_cost_of(d.cost))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stat(usd: f64) -> CostStat {
        CostStat {
            usd,
            partial: false,
            unpriced_models: Vec::new(),
            uncosted_harnesses: Vec::new(),
            by_harness: Vec::new(),
        }
    }

    #[test]
    fn a_cost_reads_as_the_info_tab_shows_it() {
        assert_eq!(run_cost_of(None), RunCost::unknown());
        assert_eq!(run_cost_of(Some(stat(0.5))), RunCost::new(0.5));
        let partial = run_cost_of(Some(CostStat {
            partial: true,
            unpriced_models: vec!["mystery".into()],
            ..stat(0.5)
        }));
        assert_eq!(partial.usd, Some(0.5));
        assert!(partial.partial);
        assert_eq!(partial.unpriced_models, ["mystery"]);
        let unknown = run_cost_of(Some(CostStat {
            uncosted_harnesses: vec!["opencode".into()],
            ..stat(0.0)
        }));
        assert_eq!(unknown.usd, None, "« — », never $0");
        assert_eq!(unknown.uncosted_harnesses, ["opencode"]);
    }

    #[test]
    fn a_tree_cost_reads_every_amount_as_a_run_cost() {
        let tree = run_tree_cost_of(TreeCost {
            total: None,
            parent: Some(stat(0.8)),
            children: None,
            descendants: 2,
            by_depth: vec![crate::domain::state::DepthCost {
                depth: 1,
                runs: 2,
                cost: None,
            }],
            unavailable_runs: 1,
        });
        assert_eq!(tree.total, RunCost::unknown(), "« — », never $0");
        assert_eq!(tree.parent, RunCost::new(0.8));
        assert_eq!(tree.children, RunCost::unknown());
        assert_eq!(tree.descendants, 2);
        assert_eq!(tree.by_depth, [RunDepthCost::new(1, 2, RunCost::unknown())]);
    }
}
