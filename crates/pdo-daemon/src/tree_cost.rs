//! The **Coût de l'arbre** (pdo-issues#3, ADR-0081): a Run's own cost (the
//! **Coût du Run parent**, ADR-0058 — untouched) plus the own cost of every
//! descendant, summed **on read**. Never persisted (ADR-0022/0029).
//!
//! Pure over already-folded [`CostStat`]s: the caller folds each Run of the
//! tree independently (the memoized per-Run fold) and hands the results here
//! with their depth ([`crate::run_tree`]).
//!
//! The ADR-0052/0058 honesty rule holds over the tree:
//! - a Run whose cost is **not computable** (`None`) refuses the sum of every
//!   slice it belongs to (the total, `children`, its depth) — the other
//!   slices stay readable;
//! - a Run with a harness that has **no cost source** makes those sums
//!   unavailable too (`uncosted_harnesses` non-empty ⇒ "—" with the reason),
//!   while the per-harness `by_harness` slices are still cumulated;
//! - a **lower bound** (`partial`) propagates: the sum is a lower bound.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::event_log::{CostStat, HarnessCost};

/// One depth level of the tree (1 = direct children).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct DepthCost {
    pub depth: usize,
    /// How many Runs sit at this depth.
    pub runs: usize,
    /// Their summed cost; `None` when one of them is not computable ("—").
    pub cost: Option<CostStat>,
}

/// The Coût de l'arbre of a Run that has at least one descendant.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct TreeCost {
    /// Coût du Run parent + every descendant. `None` ⇒ "—".
    pub total: Option<CostStat>,
    /// The Run's own cost — the Coût du Run parent, the same figure as `cost`.
    pub parent: Option<CostStat>,
    /// Every descendant's cost summed (« Child runs (N) »).
    pub children: Option<CostStat>,
    /// N: the number of descendants, every depth included.
    pub descendants: usize,
    /// The descendants per depth, shallowest first.
    pub by_depth: Vec<DepthCost>,
    /// How many Runs of the tree (the parent included) have no computable cost
    /// at all — what a `null` total owes its "—" to.
    pub unavailable_runs: usize,
}

/// Sum costs under the honesty rule. `None` when any member is `None`.
pub(crate) fn sum_costs<'a, I>(costs: I) -> Option<CostStat>
where
    I: IntoIterator<Item = &'a Option<CostStat>>,
{
    let mut usd = 0.0;
    let mut unpriced: BTreeSet<String> = BTreeSet::new();
    let mut uncosted: BTreeSet<String> = BTreeSet::new();
    let mut harnesses: BTreeMap<String, HarnessCost> = BTreeMap::new();
    for cost in costs {
        let cost = cost.as_ref()?;
        usd += cost.usd;
        unpriced.extend(cost.unpriced_models.iter().cloned());
        uncosted.extend(cost.uncosted_harnesses.iter().cloned());
        for slice in &cost.by_harness {
            harnesses
                .entry(slice.harness.clone())
                .and_modify(|acc| {
                    acc.usd += slice.usd;
                    acc.partial |= slice.partial;
                    acc.reported_in_usd &= slice.reported_in_usd;
                    acc.unpriced_models
                        .extend(slice.unpriced_models.iter().cloned());
                    acc.unpriced_models.sort();
                    acc.unpriced_models.dedup();
                })
                .or_insert_with(|| slice.clone());
        }
    }
    let by_harness: Vec<HarnessCost> = harnesses.into_values().collect();
    if !uncosted.is_empty() {
        // Same shape as a single unavailable Run: no total, no lower bound —
        // the slices still say themselves.
        return Some(CostStat {
            usd: 0.0,
            partial: false,
            unpriced_models: Vec::new(),
            uncosted_harnesses: uncosted.into_iter().collect(),
            by_harness,
        });
    }
    let unpriced_models: Vec<String> = unpriced.into_iter().collect();
    Some(CostStat {
        usd,
        partial: !unpriced_models.is_empty(),
        unpriced_models,
        uncosted_harnesses: Vec::new(),
        by_harness,
    })
}

/// The Coût de l'arbre of a Run, from its own cost and each descendant's
/// `(depth, cost)`. `None` for a Run without descendant: its surfaces stay
/// strictly as they were.
pub(crate) fn tree_cost(
    parent: Option<CostStat>,
    descendants: &[(usize, Option<CostStat>)],
) -> Option<TreeCost> {
    if descendants.is_empty() {
        return None;
    }
    let child_costs: Vec<Option<CostStat>> = descendants.iter().map(|(_, c)| c.clone()).collect();
    let children = sum_costs(&child_costs);
    let total = sum_costs(std::iter::once(&parent).chain(child_costs.iter()));

    let mut depths: BTreeMap<usize, Vec<Option<CostStat>>> = BTreeMap::new();
    for (depth, cost) in descendants {
        depths.entry(*depth).or_default().push(cost.clone());
    }
    let by_depth = depths
        .into_iter()
        .map(|(depth, costs)| DepthCost {
            depth,
            runs: costs.len(),
            cost: sum_costs(&costs),
        })
        .collect();

    let unavailable_runs = std::iter::once(&parent)
        .chain(child_costs.iter())
        .filter(|c| c.is_none())
        .count();

    Some(TreeCost {
        total,
        parent,
        children,
        descendants: descendants.len(),
        by_depth,
        unavailable_runs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_log::CostForm;

    fn claude(usd: f64) -> CostStat {
        CostStat {
            usd,
            partial: false,
            unpriced_models: Vec::new(),
            uncosted_harnesses: Vec::new(),
            by_harness: vec![HarnessCost {
                harness: "claude".into(),
                usd,
                form: CostForm::Derived,
                partial: false,
                unpriced_models: Vec::new(),
                reported_in_usd: false,
            }],
        }
    }

    fn partial(usd: f64, model: &str) -> CostStat {
        let mut c = claude(usd);
        c.partial = true;
        c.unpriced_models = vec![model.into()];
        c.by_harness[0].partial = true;
        c.by_harness[0].unpriced_models = vec![model.into()];
        c
    }

    fn uncosted(harness: &str, slices: Vec<HarnessCost>) -> CostStat {
        CostStat {
            usd: 0.0,
            partial: false,
            unpriced_models: Vec::new(),
            uncosted_harnesses: vec![harness.into()],
            by_harness: slices,
        }
    }

    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn a_run_without_descendant_has_no_tree_cost() {
        assert_eq!(tree_cost(Some(claude(1.0)), &[]), None);
    }

    #[test]
    fn one_level_tree_sums_parent_and_children() {
        let t = tree_cost(
            Some(claude(3.0)),
            &[(1, Some(claude(1.0))), (1, Some(claude(0.5)))],
        )
        .unwrap();
        assert!(approx(t.total.as_ref().unwrap().usd, 4.5));
        assert!(approx(t.parent.as_ref().unwrap().usd, 3.0));
        assert!(approx(t.children.as_ref().unwrap().usd, 1.5));
        assert_eq!(t.descendants, 2);
        assert_eq!(t.by_depth.len(), 1);
        assert_eq!(t.by_depth[0].runs, 2);
        let total = t.total.unwrap();
        assert_eq!(
            total.by_harness.len(),
            1,
            "by_harness cumulated per harness"
        );
        assert!(approx(total.by_harness[0].usd, 4.5));
        assert_eq!(t.unavailable_runs, 0);
    }

    #[test]
    fn two_level_tree_ventilates_by_depth() {
        let t = tree_cost(
            Some(claude(1.0)),
            &[
                (1, Some(claude(2.0))),
                (2, Some(claude(0.25))),
                (1, Some(claude(1.0))),
            ],
        )
        .unwrap();
        let depths: Vec<(usize, usize, f64)> = t
            .by_depth
            .iter()
            .map(|d| (d.depth, d.runs, d.cost.as_ref().unwrap().usd))
            .collect();
        assert_eq!(depths, vec![(1, 2, 3.0), (2, 1, 0.25)]);
        assert!(approx(t.total.unwrap().usd, 4.25));
    }

    #[test]
    fn an_uncomputable_descendant_refuses_the_sum_but_keeps_other_slices() {
        let t = tree_cost(Some(claude(1.0)), &[(1, Some(claude(2.0))), (2, None)]).unwrap();
        assert_eq!(t.total, None);
        assert_eq!(t.children, None);
        assert!(approx(t.parent.unwrap().usd, 1.0));
        assert!(approx(t.by_depth[0].cost.as_ref().unwrap().usd, 2.0));
        assert_eq!(t.by_depth[1].cost, None);
        assert_eq!(t.unavailable_runs, 1);
    }

    #[test]
    fn a_descendant_without_cost_source_makes_the_total_unavailable_with_slices() {
        let t = tree_cost(
            Some(claude(1.0)),
            &[(1, Some(uncosted("opencode", Vec::new())))],
        )
        .unwrap();
        let total = t.total.unwrap();
        assert_eq!(total.uncosted_harnesses, vec!["opencode".to_string()]);
        assert_eq!(total.usd, 0.0, "never half a sum");
        assert!(!total.partial);
        assert_eq!(total.by_harness.len(), 1, "the claude slice survives");
        assert!(approx(total.by_harness[0].usd, 1.0));
        assert!(approx(t.parent.unwrap().usd, 1.0));
        assert_eq!(
            t.children.unwrap().uncosted_harnesses,
            vec!["opencode".to_string()]
        );
    }

    #[test]
    fn a_partial_descendant_makes_the_tree_a_lower_bound() {
        let t = tree_cost(Some(claude(1.0)), &[(1, Some(partial(0.5, "mystery")))]).unwrap();
        let total = t.total.unwrap();
        assert!(total.partial);
        assert_eq!(total.unpriced_models, vec!["mystery".to_string()]);
        assert!(
            total.by_harness[0].partial,
            "the claude slice is a lower bound too"
        );
        assert!(approx(total.usd, 1.5));
        assert!(!t.parent.unwrap().partial);
        assert!(t.children.unwrap().partial);
    }

    #[test]
    fn mixed_harnesses_keep_their_form_per_slice() {
        let copilot = CostStat {
            usd: 2.0,
            partial: false,
            unpriced_models: Vec::new(),
            uncosted_harnesses: Vec::new(),
            by_harness: vec![HarnessCost {
                harness: "copilot".into(),
                usd: 2.0,
                form: CostForm::Reported,
                partial: false,
                unpriced_models: Vec::new(),
                reported_in_usd: false,
            }],
        };
        let total = tree_cost(Some(claude(1.0)), &[(1, Some(copilot))])
            .unwrap()
            .total
            .unwrap();
        let forms: Vec<(&str, CostForm)> = total
            .by_harness
            .iter()
            .map(|h| (h.harness.as_str(), h.form))
            .collect();
        assert_eq!(
            forms,
            vec![
                ("claude", CostForm::Derived),
                ("copilot", CostForm::Reported)
            ]
        );
    }
}
