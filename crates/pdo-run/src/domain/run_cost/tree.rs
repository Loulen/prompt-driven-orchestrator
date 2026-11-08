//! The **Coût de l'arbre** (ADR draft-cout-de-l-arbre): a Run's own cost, the
//! **Coût du Run parent** (ADR-0058, untouched), plus the own cost of every
//! descendant, summed **on read**. Never persisted.
//!
//! Pure over already-folded [`CostStat`]s: the caller folds each Run of the
//! tree independently (the memoized per-Run fold) and hands the results here
//! with their depth ([`RunTree::descendants`]).
//!
//! The ADR-0052/0058 honesty rule holds over the tree:
//! - a Run whose cost is **not computable** (`None`) refuses the sum of every
//!   slice it belongs to (the total, the children, its depth); the other
//!   slices stay readable;
//! - a Run with a harness that has **no cost source** makes those sums
//!   unavailable too (`uncosted_harnesses` non-empty, "—" with the reason),
//!   while the per-harness `by_harness` slices are still cumulated;
//! - a **lower bound** (`partial`) propagates: the sum is a lower bound.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

use crate::domain::state::{CostStat, DepthCost, HarnessCost, TreeCost};

/// The tree of the log's Runs, from their `(run_id, parent_run_id)` links:
/// built once, walked from any Run. A Run whose parent is not among the links
/// (forgotten) roots its own tree, so a forgotten descendant takes its subtree
/// out of its ancestors'.
#[derive(Debug, Clone, Default)]
pub struct RunTree {
    children: HashMap<String, Vec<String>>,
}

impl RunTree {
    pub fn from_links<'a, I>(links: I) -> Self
    where
        I: IntoIterator<Item = (&'a str, Option<&'a str>)>,
    {
        let mut children: HashMap<String, Vec<String>> = HashMap::new();
        for (run_id, parent) in links {
            if let Some(parent) = parent {
                children
                    .entry(parent.to_string())
                    .or_default()
                    .push(run_id.to_string());
            }
        }
        for kids in children.values_mut() {
            kids.sort_unstable();
        }
        Self { children }
    }

    /// Every descendant of `root` with its depth (1 = direct child), breadth
    /// first. A cycle, which the daemon never writes, is cut rather than
    /// looped on.
    pub fn descendants(&self, root: &str) -> Vec<(String, u32)> {
        let mut seen: HashSet<&str> = HashSet::from([root]);
        let mut out = Vec::new();
        let mut queue: VecDeque<(&str, u32)> = VecDeque::from([(root, 0)]);
        while let Some((run_id, depth)) = queue.pop_front() {
            for kid in self
                .children
                .get(run_id)
                .map(Vec::as_slice)
                .unwrap_or_default()
            {
                if seen.insert(kid) {
                    out.push((kid.clone(), depth + 1));
                    queue.push_back((kid, depth + 1));
                }
            }
        }
        out
    }
}

/// Sum costs under the honesty rule: `None` when any member is `None`.
pub fn sum_costs<'a, I>(costs: I) -> Option<CostStat>
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
        // The shape of a single unavailable Run: no total, no lower bound, the
        // slices still say themselves.
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
/// `(depth, cost)`. `None` for a Run without descendant: its surfaces stay as
/// they were.
pub fn tree_cost(
    parent: Option<CostStat>,
    descendants: &[(u32, Option<CostStat>)],
) -> Option<TreeCost> {
    if descendants.is_empty() {
        return None;
    }
    let child_costs: Vec<Option<CostStat>> = descendants.iter().map(|(_, c)| c.clone()).collect();
    let children = sum_costs(&child_costs);
    let total = sum_costs(std::iter::once(&parent).chain(child_costs.iter()));

    let mut depths: BTreeMap<u32, Vec<Option<CostStat>>> = BTreeMap::new();
    for (depth, cost) in descendants {
        depths.entry(*depth).or_default().push(cost.clone());
    }
    let by_depth = depths
        .into_iter()
        .map(|(depth, costs)| DepthCost {
            depth,
            runs: costs.len() as u32,
            cost: sum_costs(&costs),
        })
        .collect();

    let unavailable_runs = std::iter::once(&parent)
        .chain(child_costs.iter())
        .filter(|c| c.is_none())
        .count() as u32;

    Some(TreeCost {
        total,
        parent,
        children,
        descendants: descendants.len() as u32,
        by_depth,
        unavailable_runs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::state::CostForm;

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

    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn descendants_walk_every_depth_and_leave_other_trees_out() {
        let links = [
            ("root", None),
            ("a", Some("root")),
            ("b", Some("root")),
            ("a1", Some("a")),
            ("a1x", Some("a1")),
            ("other", None),
            ("o1", Some("other")),
        ];
        let tree = RunTree::from_links(links);
        assert_eq!(
            tree.descendants("root"),
            vec![
                ("a".to_string(), 1),
                ("b".to_string(), 1),
                ("a1".to_string(), 2),
                ("a1x".to_string(), 3)
            ]
        );
        assert_eq!(tree.descendants("a1x"), vec![]);
        assert_eq!(
            tree.descendants("a"),
            vec![("a1".to_string(), 1), ("a1x".to_string(), 2)]
        );
    }

    #[test]
    fn a_forgotten_descendant_takes_its_subtree_out() {
        // `a` was forgotten: its log is gone, so it is not among the links,
        // and neither is its link to `root`. Its child `a1` keeps pointing at
        // it and roots its own tree.
        let links = [("root", None), ("b", Some("root")), ("a1", Some("a"))];
        assert_eq!(
            RunTree::from_links(links).descendants("root"),
            vec![("b".to_string(), 1)]
        );
    }

    #[test]
    fn a_cycle_is_cut() {
        let links = [("x", Some("y")), ("y", Some("x"))];
        assert_eq!(
            RunTree::from_links(links).descendants("x"),
            vec![("y".to_string(), 1)]
        );
    }

    #[test]
    fn a_run_without_descendant_has_no_tree_cost() {
        assert_eq!(tree_cost(Some(claude(1.0)), &[]), None);
    }

    #[test]
    fn the_tree_sums_the_parent_and_every_depth() {
        let t = tree_cost(
            Some(claude(0.8)),
            &[
                (1, Some(claude(1.0))),
                (1, Some(claude(1.1))),
                (2, Some(claude(1.3))),
            ],
        )
        .unwrap();
        assert!(approx(t.total.as_ref().unwrap().usd, 4.2));
        assert!(approx(t.parent.as_ref().unwrap().usd, 0.8));
        assert!(approx(t.children.as_ref().unwrap().usd, 3.4));
        assert_eq!(t.descendants, 3);
        assert_eq!(t.unavailable_runs, 0);
        assert_eq!(t.by_depth.len(), 2);
        assert_eq!((t.by_depth[0].depth, t.by_depth[0].runs), (1, 2));
        assert!(approx(t.by_depth[0].cost.as_ref().unwrap().usd, 2.1));
        assert_eq!((t.by_depth[1].depth, t.by_depth[1].runs), (2, 1));
        assert!(approx(t.by_depth[1].cost.as_ref().unwrap().usd, 1.3));
        // The per-harness slices are cumulated.
        let slices = &t.total.as_ref().unwrap().by_harness;
        assert_eq!(slices.len(), 1);
        assert!(approx(slices[0].usd, 4.2));
    }

    #[test]
    fn an_uncomputable_descendant_refuses_the_sums_it_belongs_to_only() {
        let t = tree_cost(Some(claude(1.0)), &[(1, Some(claude(2.0))), (2, None)]).unwrap();
        assert_eq!(t.total, None, "« — »: the tree is not summable");
        assert_eq!(t.children, None);
        assert!(
            approx(t.parent.as_ref().unwrap().usd, 1.0),
            "the parent slice stays"
        );
        assert!(
            approx(t.by_depth[0].cost.as_ref().unwrap().usd, 2.0),
            "depth 1 stays"
        );
        assert_eq!(t.by_depth[1].cost, None);
        assert_eq!(t.unavailable_runs, 1);
    }

    #[test]
    fn an_uncosted_harness_makes_the_sum_unavailable_but_keeps_the_slices() {
        let opencode = CostStat {
            usd: 0.0,
            partial: false,
            unpriced_models: Vec::new(),
            uncosted_harnesses: vec!["opencode".into()],
            by_harness: vec![claude(0.5).by_harness[0].clone()],
        };
        let t = tree_cost(Some(claude(1.0)), &[(1, Some(opencode))]).unwrap();
        let total = t.total.unwrap();
        assert_eq!(total.uncosted_harnesses, ["opencode"]);
        assert!(!total.partial);
        assert!(approx(total.by_harness[0].usd, 1.5));
        assert_eq!(t.unavailable_runs, 0);
    }

    #[test]
    fn a_lower_bound_descendant_makes_the_tree_a_lower_bound() {
        let t = tree_cost(Some(claude(1.0)), &[(1, Some(partial(0.5, "mystery")))]).unwrap();
        let total = t.total.unwrap();
        assert!(approx(total.usd, 1.5));
        assert!(total.partial);
        assert_eq!(total.unpriced_models, ["mystery"]);
        assert!(total.by_harness[0].partial);
        assert!(
            !t.parent.unwrap().partial,
            "the parent's own cost is untouched"
        );
    }
}
