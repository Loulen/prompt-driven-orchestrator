//! The **Coût de boucle** (CONTEXT.md): what each loop region's member nodes
//! cost since the loop's last entry, folded from the very contributions the
//! Run total and the node costs sum (ADR-0058), so the three figures can never
//! diverge. Pure: the breakdown and the event log in, one [`LoopCost`] per
//! region out. Derived on read, never persisted (ADR-0022/0029).

use std::collections::{BTreeSet, HashMap};

use pdo_pipeline::domain::pipeline::{LoopKind, LoopRegion};

use super::{CostScope, RunCostBreakdown};
use crate::domain::event::{Event, EventKind};
use crate::domain::state::{LoopCost, LoopMemberCost};

/// Index of the event that opened the **current window** of a loop region: the
/// loop's iteration counter last going back to 1. A bounded region's counter
/// rides `LoopIterStarted`; a collection's entry is `CollectionStarted`. The
/// Run's first entry opens the window at the very start of the log, so lap-1
/// executions logged before a first `LoopIterStarted` still count; a later
/// entry whose counter went *back* (a re-entry of the loop) opens a fresh
/// window. A repeated `iter` (a re-emitted lap) is not a re-entry.
fn loop_window_start(events: &[Event], region: &LoopRegion) -> usize {
    let mut start = 0;
    let mut previous: Option<i64> = None;
    for (index, event) in events.iter().enumerate() {
        let Some(payload) = event.payload.as_ref() else {
            continue;
        };
        let iter = match (region.kind, &event.kind) {
            (LoopKind::Bounded, EventKind::LoopIterStarted)
                if payload.get("loop_node_id").and_then(|v| v.as_str())
                    == Some(region.id.as_str()) =>
            {
                payload.get("iter").and_then(|v| v.as_i64()).unwrap_or(1)
            }
            (LoopKind::Collection, EventKind::CollectionStarted)
                if payload.get("region_id").and_then(|v| v.as_str())
                    == Some(region.id.as_str()) =>
            {
                1
            }
            _ => continue,
        };
        let reentry = match (region.kind, previous) {
            (_, None) => false,
            (LoopKind::Bounded, Some(previous)) => iter < previous,
            // Every `CollectionStarted` after the first is a fresh fan-out.
            (LoopKind::Collection, Some(_)) => true,
        };
        if reentry {
            start = index;
        }
        previous = Some(iter);
    }
    start
}

/// The **Coût de boucle** of every loop region. A region counts the executions
/// of its **member** nodes (sub-agents ride their parent's contribution) that
/// started inside its current window ([`loop_window_start`]); a node member of
/// two regions counts in both. An unreadable member execution keeps the known
/// part and flags `partial`, never a `$0` for the unknown. A region with no
/// member execution in its window yet is absent. `cap_usd` is left `None`: the
/// caller, which knows the Run's variables, sets the effective cap.
pub fn loop_costs(
    breakdown: &RunCostBreakdown,
    events: &[Event],
    regions: &[LoopRegion],
) -> HashMap<String, LoopCost> {
    let mut out = HashMap::new();
    for region in regions {
        let start = loop_window_start(events, region);
        let mut members: Vec<LoopMemberCost> = Vec::new();
        let mut usd: Option<f64> = None;
        let mut exact = true;
        let mut partial = false;
        let mut unpriced = BTreeSet::new();
        let mut reasons = BTreeSet::new();
        let mut executions = 0;
        let mut readable = 0;
        for member in &region.members {
            let mut share = LoopMemberCost {
                node_id: member.clone(),
                usd: None,
                estimated: false,
                partial: false,
                executions: 0,
            };
            let mut share_exact = true;
            for contribution in breakdown.contributions.iter().filter(|c| {
                c.scope == CostScope::Node
                    && c.node_id.as_deref() == Some(member.as_str())
                    && c.started_event.is_some_and(|at| at >= start)
            }) {
                share.executions += contribution.executions;
                executions += contribution.executions;
                readable += contribution.readable_executions;
                unpriced.extend(contribution.unpriced_models.iter().cloned());
                reasons.extend(contribution.unavailable_reasons.iter().cloned());
                match contribution.usd {
                    Some(amount) => {
                        share.usd = Some(share.usd.unwrap_or_default() + amount);
                        usd = Some(usd.unwrap_or_default() + amount);
                        exact &= contribution.reported_in_usd;
                        share_exact &= contribution.reported_in_usd;
                        share.partial |= contribution.partial;
                    }
                    None => share.partial = true,
                }
            }
            share.estimated = share.usd.is_some() && !share_exact;
            partial |= share.partial;
            if share.executions > 0 {
                members.push(share);
            }
        }
        if executions == 0 {
            continue;
        }
        out.insert(
            region.id.clone(),
            LoopCost {
                usd,
                estimated: usd.is_some() && !exact,
                partial: partial && usd.is_some(),
                unpriced_models: unpriced.into_iter().collect(),
                unavailable_reasons: reasons.into_iter().collect(),
                executions,
                readable_executions: readable,
                members,
                cap_usd: None,
            },
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::run_cost::CostContribution;
    use crate::domain::state::CostForm;

    fn region(id: &str, kind: LoopKind, members: &[&str]) -> LoopRegion {
        LoopRegion {
            max_cost_usd: None,
            id: id.into(),
            kind,
            members: members.iter().map(|m| m.to_string()).collect(),
            max_iter: None,
            over: None,
        }
    }

    fn bounded(id: &str, members: &[&str]) -> LoopRegion {
        region(id, LoopKind::Bounded, members)
    }

    fn event(kind: EventKind, payload: serde_json::Value) -> Event {
        Event {
            id: None,
            run_id: "r".into(),
            ts: "2026-11-08T00:00:00Z".into(),
            kind,
            node_id: None,
            iter: None,
            payload: Some(payload),
        }
    }

    fn lap(region_id: &str, iter: i64) -> Event {
        event(
            EventKind::LoopIterStarted,
            serde_json::json!({ "loop_node_id": region_id, "iter": iter, "max_iter": 5 }),
        )
    }

    fn other() -> Event {
        event(EventKind::NodeCompleted, serde_json::json!({}))
    }

    /// A node contribution opened by the event at `at`; `usd: None` = unreadable.
    fn node_share(node: &str, at: usize, usd: Option<f64>) -> CostContribution {
        CostContribution {
            harness: "claude".into(),
            scope: CostScope::Node,
            node_id: Some(node.into()),
            executions: 1,
            readable_executions: i64::from(usd.is_some()),
            usd,
            form: usd.map(|_| CostForm::Derived),
            reported_in_usd: false,
            partial: false,
            unpriced_models: Vec::new(),
            unavailable_reasons: if usd.is_none() {
                vec!["harness has no cost source".into()]
            } else {
                Vec::new()
            },
            model_slices: Vec::new(),
            started_event: Some(at),
        }
    }

    fn breakdown_of(contributions: Vec<CostContribution>) -> RunCostBreakdown {
        RunCostBreakdown {
            contributions,
            cost: None,
        }
    }

    fn filler(n: usize) -> Vec<Event> {
        (0..n).map(|_| other()).collect()
    }

    #[test]
    fn loop_cost_sums_member_executions_and_excludes_outsiders_and_infrastructure() {
        let events = filler(5);
        let mut infra = node_share("x", 0, Some(9.0));
        infra.scope = CostScope::Infrastructure;
        infra.node_id = None;
        infra.started_event = None;
        let breakdown = breakdown_of(vec![
            node_share("outer", 0, Some(5.0)),
            node_share("write", 1, Some(5.0)),
            node_share("review", 2, Some(5.0)),
            node_share("write", 3, Some(10.0)),
            infra,
        ]);

        let costs = loop_costs(&breakdown, &events, &[bounded("L", &["write", "review"])]);

        let cost = &costs["L"];
        assert!((cost.usd.unwrap() - 20.0).abs() < 1e-9, "{cost:?}");
        assert!(cost.estimated);
        assert!(!cost.partial);
        assert_eq!(cost.executions, 3);
        assert_eq!(cost.members.len(), 2);
        assert_eq!(cost.members[0].node_id, "write");
        assert!((cost.members[0].usd.unwrap() - 15.0).abs() < 1e-9);
        assert_eq!(cost.members[0].executions, 2);
        assert!(cost.members[0].estimated);
        assert!((cost.members[1].usd.unwrap() - 5.0).abs() < 1e-9);
        assert_eq!(cost.cap_usd, None);
    }

    #[test]
    fn loop_cost_keeps_the_known_part_and_flags_partial_for_an_unreadable_member() {
        let events = filler(3);
        let breakdown = breakdown_of(vec![
            node_share("write", 1, Some(1.2)),
            node_share("lint", 2, None),
        ]);

        let costs = loop_costs(&breakdown, &events, &[bounded("L", &["write", "lint"])]);

        let cost = &costs["L"];
        assert!((cost.usd.unwrap() - 1.2).abs() < 1e-9, "known part counts");
        assert!(cost.partial);
        assert_eq!(cost.executions, 2);
        assert_eq!(cost.readable_executions, 1);
        assert_eq!(cost.members[1].usd, None, "never $0 for the unknown");
        assert!(cost.members[1].partial);
        assert_eq!(
            cost.unavailable_reasons,
            vec!["harness has no cost source".to_string()]
        );
    }

    #[test]
    fn loop_cost_is_partial_when_a_member_used_an_unpriced_model() {
        let events = filler(2);
        let mut unpriced = node_share("write", 1, Some(0.5));
        unpriced.partial = true;
        unpriced.unpriced_models = vec!["gpt-x".into()];

        let costs = loop_costs(
            &breakdown_of(vec![unpriced]),
            &events,
            &[bounded("L", &["write"])],
        );

        assert!(costs["L"].partial);
        assert_eq!(costs["L"].unpriced_models, vec!["gpt-x".to_string()]);
    }

    #[test]
    fn loop_cost_is_unknown_not_zero_when_no_member_execution_is_readable() {
        let events = filler(2);
        let costs = loop_costs(
            &breakdown_of(vec![node_share("lint", 1, None)]),
            &events,
            &[bounded("L", &["lint"])],
        );
        assert_eq!(costs["L"].usd, None);
        assert!(!costs["L"].partial, "nothing known to be a lower bound of");
        assert_eq!(costs["L"].executions, 1);
    }

    #[test]
    fn loop_cost_is_absent_before_any_member_execution() {
        let events = filler(2);
        let costs = loop_costs(
            &breakdown_of(vec![node_share("outer", 1, Some(3.0))]),
            &events,
            &[bounded("L", &["write"])],
        );
        assert!(costs.is_empty());
    }

    #[test]
    fn a_node_member_of_two_loops_counts_in_both() {
        let events = filler(2);
        let breakdown = breakdown_of(vec![node_share("shared", 1, Some(2.0))]);

        let costs = loop_costs(
            &breakdown,
            &events,
            &[
                bounded("A", &["shared"]),
                bounded("B", &["shared", "other"]),
            ],
        );

        assert!((costs["A"].usd.unwrap() - 2.0).abs() < 1e-9);
        assert!((costs["B"].usd.unwrap() - 2.0).abs() < 1e-9);
    }

    #[test]
    fn loop_cost_restarts_when_the_iteration_counter_goes_back_to_one() {
        let events = vec![
            lap("L", 1), // 0
            other(),     // 1: write lap 1
            lap("L", 2), // 2
            other(),     // 3: write lap 2
            lap("L", 1), // 4: re-entry of the loop
            other(),     // 5: write lap 1'
        ];
        let breakdown = breakdown_of(vec![
            node_share("write", 1, Some(1.0)),
            node_share("write", 3, Some(2.0)),
            node_share("write", 5, Some(4.0)),
        ]);

        let costs = loop_costs(&breakdown, &events, &[bounded("L", &["write"])]);

        assert!(
            (costs["L"].usd.unwrap() - 4.0).abs() < 1e-9,
            "{:?}",
            costs["L"]
        );
        assert_eq!(costs["L"].executions, 1);
    }

    #[test]
    fn a_repeated_lap_is_not_a_reentry() {
        let events = vec![lap("L", 1), other(), lap("L", 2), lap("L", 2), other()];
        let breakdown = breakdown_of(vec![
            node_share("write", 1, Some(1.0)),
            node_share("write", 4, Some(2.0)),
        ]);
        let costs = loop_costs(&breakdown, &events, &[bounded("L", &["write"])]);
        assert!((costs["L"].usd.unwrap() - 3.0).abs() < 1e-9);
    }

    #[test]
    fn loop_cost_window_includes_lap_one_logged_before_the_first_lap_event() {
        let events = vec![other(), lap("L", 2), other()];
        let breakdown = breakdown_of(vec![
            node_share("write", 0, Some(1.0)),
            node_share("write", 2, Some(2.0)),
        ]);

        let costs = loop_costs(&breakdown, &events, &[bounded("L", &["write"])]);

        assert!((costs["L"].usd.unwrap() - 3.0).abs() < 1e-9);
    }

    #[test]
    fn collection_loop_cost_windows_on_its_last_fan_out() {
        let fan_out = || {
            event(
                EventKind::CollectionStarted,
                serde_json::json!({ "region_id": "C", "entry": "translate", "total_items": 2 }),
            )
        };
        let events = vec![fan_out(), other(), other(), fan_out(), other()];
        let breakdown = breakdown_of(vec![
            node_share("translate", 1, Some(0.3)),
            node_share("translate", 2, Some(0.4)),
            node_share("translate", 4, Some(0.8)),
        ]);
        let collection = region("C", LoopKind::Collection, &["translate"]);

        let first = breakdown_of(breakdown.contributions[..2].to_vec());
        let costs = loop_costs(&first, &events[..3], std::slice::from_ref(&collection));
        assert!((costs["C"].usd.unwrap() - 0.7).abs() < 1e-9);

        let costs = loop_costs(&breakdown, &events, &[collection]);
        assert!((costs["C"].usd.unwrap() - 0.8).abs() < 1e-9);
    }

    #[test]
    fn loop_cost_of_exact_reported_dollars_is_not_an_estimate() {
        let events = filler(2);
        let mut pi = node_share("write", 1, Some(0.25));
        pi.harness = "pi".into();
        pi.form = Some(CostForm::Reported);
        pi.reported_in_usd = true;

        let costs = loop_costs(
            &breakdown_of(vec![pi]),
            &events,
            &[bounded("L", &["write"])],
        );

        assert!(!costs["L"].estimated);
        assert!(!costs["L"].members[0].estimated);
    }
}
