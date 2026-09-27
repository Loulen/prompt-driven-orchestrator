//! One fold over the cohort's cost rows.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use pdo_run::domain::prices::PriceTable;
use pdo_run::domain::run_cost::{CostContribution, CostScope};
use pdo_workspace::domain::project::Project;

use super::metric::CostAggregateAcc;
use super::pairs::{CostEntityAcc, ModelAcc};
use super::wire::{wire_by_model, wire_cost_pipeline, wire_periods, CostProjectAcc};
use super::{ResolvedPriceRow, StatsCost, StatsCostEntity, StatsProjectCostEntity};
use crate::domain::absorption::effort_key;
use crate::domain::absorption::identity::{NodeIdentity, RunIdentity};
use crate::domain::absorption::resolver::AbsorptionResolver;

pub fn project_identity(
    payload: &serde_json::Value,
    daemon_root: &Path,
    projects: &[Project],
) -> (String, String) {
    let root = cost_project_root(payload, daemon_root);
    let root_text = root.to_string_lossy().into_owned();
    if let Some(project) = projects
        .iter()
        .find(|project| project.members.iter().any(|member| member == &root_text))
    {
        return (project.id.clone(), project.name.clone());
    }
    let name = root
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(&root_text)
        .to_string();
    (root_text, name)
}

/// Project the resolved price table into wire rows (#528): one entry per family
/// key — the winning tier and the `$/MTok` in force — in `BTreeMap` order. Reads
/// the SAME `resolved` map `price_for` bills from, so the Cost tab can never
/// enumerate a set the pricer would price otherwise (#373). Pure; the handler
/// injects the live table. `PriceTable::load` always seeds the embedded floor, so
/// this never yields `[]` even with no HOME state (D9).
pub fn resolved_price_rows(prices: &PriceTable) -> Vec<ResolvedPriceRow> {
    prices
        .resolved_entries()
        .map(|(key, price, tier)| ResolvedPriceRow {
            key: key.to_string(),
            tier,
            input: price.input,
            output: price.output,
        })
        .collect()
}

/// The "by project" bucket of a cost row: the Run's `target_repo`, else the
/// daemon repo root. No "Unassigned" bucket (invariant #6, #258).
///
/// This reads the raw `run_started` payload rather than projecting a full
/// `RunState`, so the SQL fold stays cheap — it is a deliberate inline copy of
/// `effective_repo_root`, named here so it can be tested.
///
/// #470/ADR-0033: do NOT symmetrise this with the hardened write boundary.
/// `run_started` events recorded before #470 legitimately carry
/// `target_repo: null` (≈ 46 of 101 dev runs), and resolving them here is exactly
/// what buys the "no Unassigned bucket" invariant. The asymmetry is the design:
/// required where there is a caller to answer 400 to, resolved where there is
/// only a past record to interpret.
pub fn cost_project_root(payload: &serde_json::Value, daemon_root: &Path) -> PathBuf {
    payload
        .get("target_repo")
        .and_then(|v| v.as_str())
        .map(PathBuf::from)
        .unwrap_or_else(|| daemon_root.to_path_buf())
}

pub struct CostRunRow {
    pub run_id: String,
    pub started_at: String,
    pub bucket: String,
    pub identity: RunIdentity,
    pub project_id: String,
    pub project_name: String,
    pub contributions: Vec<CostContribution>,
}

pub fn fold_harness_cost(
    runs: &[CostRunRow],
    resolved: Vec<ResolvedPriceRow>,
    resolver: &AbsorptionResolver,
) -> StatsCost {
    let mut total = CostAggregateAcc::default();
    let mut periods = BTreeMap::<String, CostAggregateAcc>::new();
    let mut pipelines = BTreeMap::<String, CostEntityAcc>::new();
    let mut projects = BTreeMap::<String, CostProjectAcc>::new();
    let mut models_axis = BTreeMap::<String, ModelAcc>::new();
    let mut active_harnesses = BTreeSet::new();

    for run in runs {
        total.add_run(&run.contributions);
        periods
            .entry(run.bucket.clone())
            .or_default()
            .add_run(&run.contributions);

        // A pipeline's headline is its nodes' work (ADR-0029 as amended by
        // #742): the Infrastructure bucket — the run's own manager and
        // merge-resolver sessions, plus attributable leftover transcripts —
        // and the Unassigned orphans stay visible as node rows below the
        // headline, but they are orchestration overhead, not pipeline work.
        // The instance totals above keep every contribution (ADR-0058: a
        // run's total is the sum of what it actually spent).
        let node_contributions: Vec<CostContribution> = run
            .contributions
            .iter()
            .filter(|c| c.scope == CostScope::Node)
            .cloned()
            .collect();

        let identity = &run.identity;
        let pipeline_key = &identity.pipeline_key;
        let pipeline = pipelines.entry(pipeline_key.clone()).or_default();
        identity.adopt_name(&mut pipeline.name);
        pipeline
            .tallies
            .record_run(identity, &run.run_id, &run.started_at);
        pipeline.aggregate.add_run(&node_contributions);
        pipeline
            .periods
            .entry(run.bucket.clone())
            .or_default()
            .add_run(&node_contributions);

        let project = projects.entry(run.project_id.clone()).or_default();
        project.name = run.project_name.clone();
        project.aggregate.add_run(&node_contributions);
        project
            .periods
            .entry(run.bucket.clone())
            .or_default()
            .add_run(&node_contributions);
        let project_pipeline = project.pipelines.entry(pipeline_key.clone()).or_default();
        identity.adopt_name(&mut project_pipeline.name);
        project_pipeline
            .tallies
            .record_run(identity, &run.run_id, &run.started_at);
        project_pipeline.aggregate.add_run(&node_contributions);
        project_pipeline
            .periods
            .entry(run.bucket.clone())
            .or_default()
            .add_run(&node_contributions);

        for contribution in &run.contributions {
            if contribution.executions > 0 {
                active_harnesses.insert(contribution.harness.clone());
            }
            // #892: a Node absorbed under this Pipeline row counts under its
            // absorbent Node, in every tree the Node shows in.
            let node_identity = match contribution.scope {
                CostScope::Node => {
                    identity.node(contribution.node_id.as_deref().unwrap_or("(unknown)"))
                }
                CostScope::Infrastructure => NodeIdentity {
                    key: format!("{pipeline_key}:infrastructure"),
                    raw_key: String::new(),
                    name: "Infrastructure".to_string(),
                    own_name: true,
                },
                CostScope::Unassigned => NodeIdentity {
                    key: format!("{pipeline_key}:unassigned"),
                    raw_key: String::new(),
                    name: "Unassigned".to_string(),
                    own_name: true,
                },
            };
            let id = node_identity.key.clone();
            let is_node = contribution.scope == CostScope::Node;
            let tally = |node: &mut CostEntityAcc| {
                node_identity.adopt_name(&mut node.name);
                if is_node {
                    node.tallies.record_run_as(
                        &node_identity.raw_key,
                        &run.run_id,
                        &run.started_at,
                    );
                }
            };
            let node = pipeline.nodes.entry(id.clone()).or_default();
            tally(node);
            node.aggregate.add_contribution(contribution);
            node.periods
                .entry(run.bucket.clone())
                .or_default()
                .add_contribution(contribution);

            let project_node = project_pipeline.nodes.entry(id.clone()).or_default();
            tally(project_node);
            project_node.aggregate.add_contribution(contribution);
            project_node
                .periods
                .entry(run.bucket.clone())
                .or_default()
                .add_contribution(contribution);

            // ADR-0065: the Node leaf's model × effort pairs, plus the whole
            // « By model » tree — one pass over the same slices, the same node
            // identity as the axes above. Slices land in every level of both
            // hierarchies so headline, cards and period bars all re-scope.
            for slice in &contribution.model_slices {
                // #906: the Node's couple goes through the global absorptions
                // (effort on the raw model, then model), then the couple
                // absorptions of its raw Node — the « By model » axis below
                // stops before that last step.
                let couple = identity.couple(
                    is_node.then_some(node_identity.raw_key.as_str()),
                    &slice.model,
                    slice.effort.as_deref(),
                );
                let pair_key = (couple.model.clone(), couple.effort.clone());
                for pairs in [&mut node.pairs, &mut project_node.pairs] {
                    let pair = pairs.entry(pair_key.clone()).or_default();
                    pair.add_slice(&contribution.harness, slice);
                    pair.record(&couple, &run.run_id, &run.started_at);
                }

                // #892: an absorbed model id counts under its absorbent on
                // this axis only; its efforts meet the absorbent's by effort.
                let model = models_axis.entry(identity.model(&slice.model)).or_default();
                model
                    .tallies
                    .record_run_as(&slice.model, &run.run_id, &run.started_at);
                model
                    .tallies
                    .record_provenance_as(&slice.model, slice.model_observed);
                model.aggregate.add_slice(&contribution.harness, slice);
                model
                    .periods
                    .entry(run.bucket.clone())
                    .or_default()
                    .add_slice(&contribution.harness, slice);
                if slice.model_observed {
                    model.model_observed += 1;
                } else {
                    model.model_requested += 1;
                }

                // #906: an effort absorption resolves on the RAW model id.
                let effort = model
                    .efforts
                    .entry(identity.effort(&slice.model, slice.effort.as_deref()))
                    .or_default();
                effort.tallies.record_run_as(
                    &effort_key(slice.effort.as_deref()),
                    &run.run_id,
                    &run.started_at,
                );
                effort.aggregate.add_slice(&contribution.harness, slice);
                effort
                    .periods
                    .entry(run.bucket.clone())
                    .or_default()
                    .add_slice(&contribution.harness, slice);
                if slice.model_observed {
                    effort.model_observed += 1;
                } else {
                    effort.model_requested += 1;
                }
                match slice.effort_observed {
                    Some(true) => effort.effort_observed += 1,
                    Some(false) => effort.effort_requested += 1,
                    None => {}
                }

                // Same rule as the pipeline headline above: only node work
                // sums into a pipeline on the « By model » axis. The slices
                // still land in the model/effort aggregates and in the node
                // leaves below (infrastructure sessions do burn model tokens
                // — they are just not pipeline work).
                let axis_pipeline = effort.pipelines.entry(pipeline_key.clone()).or_default();
                identity.adopt_name(&mut axis_pipeline.entity.name);
                axis_pipeline
                    .entity
                    .tallies
                    .record_run(identity, &run.run_id, &run.started_at);
                if contribution.scope == CostScope::Node {
                    axis_pipeline
                        .entity
                        .aggregate
                        .add_slice(&contribution.harness, slice);
                    axis_pipeline
                        .entity
                        .periods
                        .entry(run.bucket.clone())
                        .or_default()
                        .add_slice(&contribution.harness, slice);
                }

                let axis_node = axis_pipeline.nodes.entry(id.clone()).or_default();
                tally(axis_node);
                axis_node.aggregate.add_slice(&contribution.harness, slice);
                axis_node
                    .periods
                    .entry(run.bucket.clone())
                    .or_default()
                    .add_slice(&contribution.harness, slice);
            }
        }
    }

    let mut by_pipeline: Vec<_> = pipelines
        .into_iter()
        .map(|(id, pipeline)| wire_cost_pipeline(id, pipeline, resolver))
        .collect();
    by_pipeline.sort_by(|a, b| {
        b.aggregate
            .usd
            .unwrap_or(-1.0)
            .partial_cmp(&a.aggregate.usd.unwrap_or(-1.0))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.id.cmp(&b.id))
    });

    let mut by_project: Vec<_> = projects
        .into_iter()
        .map(|(id, project)| {
            let mut pipelines: Vec<_> = project
                .pipelines
                .into_iter()
                .map(|(id, pipeline)| wire_cost_pipeline(id, pipeline, resolver))
                .collect();
            pipelines.sort_by(|a, b| {
                b.aggregate
                    .usd
                    .unwrap_or(-1.0)
                    .partial_cmp(&a.aggregate.usd.unwrap_or(-1.0))
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| a.id.cmp(&b.id))
            });
            StatsProjectCostEntity {
                entity: StatsCostEntity {
                    id,
                    name: project.name,
                    aggregate: project.aggregate.wire(),
                    by_period: wire_periods(project.periods),
                    nodes: Vec::new(),
                    models: Vec::new(),
                    runs: None,
                    last_run: None,
                    absorbed: Vec::new(),
                },
                pipelines,
            }
        })
        .collect();
    by_project.sort_by(|a, b| {
        b.entity
            .aggregate
            .usd
            .unwrap_or(-1.0)
            .partial_cmp(&a.entity.aggregate.usd.unwrap_or(-1.0))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.entity.id.cmp(&b.entity.id))
    });

    StatsCost {
        harnesses: active_harnesses.into_iter().collect(),
        total: total.wire(),
        by_period: wire_periods(periods),
        by_pipeline,
        by_project,
        by_model: wire_by_model(models_axis, resolver),
        resolved,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::cost::StatsProvenance;

    #[test]
    fn resolved_price_rows_project_the_floor_faithfully() {
        // The projection `resolved_entries -> ResolvedPriceRow` is faithful: the
        // fourteen embedded families (since #527 floored the current generation),
        // every one `Embedded`, with the single most error-prone distinction
        // surviving (opus-4-8 5/25 ≠ opus-4-1 15/75), in BTreeMap key order. The
        // winning-tier PRECEDENCE (manual > fetched > embedded) is exercised by
        // `price_table::resolved_entries_*` and end-to-end over `/stats/cost` in
        // `tests/cost_prices.rs`.
        use pdo_run::domain::prices::{PriceTable, PriceTier};
        let floor = resolved_price_rows(&PriceTable::builtin());
        assert_eq!(floor.len(), 14);
        assert!(floor.iter().all(|r| r.tier == PriceTier::Embedded));
        let by = |key: &str| floor.iter().find(|r| r.key == key).unwrap();
        assert_eq!(
            (by("claude-opus-4-8").input, by("claude-opus-4-8").output),
            (5.0, 25.0)
        );
        assert_eq!(
            (by("claude-opus-4-1").input, by("claude-opus-4-1").output),
            (15.0, 75.0)
        );
        // Rows come out in BTreeMap key order (families grouped for free, D4).
        let keys: Vec<&str> = floor.iter().map(|r| r.key.as_str()).collect();
        let mut sorted = keys.clone();
        sorted.sort_unstable();
        assert_eq!(keys, sorted);
        // The sentinel is a `price_for` short-circuit, never a table row.
        assert!(floor.iter().all(|r| r.key != "<synthetic>"));
    }

    #[test]
    fn cost_project_root_uses_the_runs_target_repo() {
        let payload = serde_json::json!({ "target_repo": "/proj/A" });
        assert_eq!(
            cost_project_root(&payload, Path::new("/daemon/root")),
            PathBuf::from("/proj/A")
        );
    }

    #[test]
    fn unnamed_project_uses_the_primary_repository_identity_and_readable_name() {
        let payload = serde_json::json!({ "target_repo": "/repos/product-api" });
        assert_eq!(
            project_identity(&payload, Path::new("/daemon/root"), &[]),
            ("/repos/product-api".to_string(), "product-api".to_string())
        );
    }

    #[test]
    fn harness_cost_fold_keeps_lower_bounds_and_unknown_columns_honest() {
        let run = CostRunRow {
            run_id: "r".to_string(),
            started_at: "2026-07-15T09:00:00Z".to_string(),
            bucket: "2026-07-15".to_string(),
            identity: test_identity("p", "Pipeline", "n", "Node"),
            project_id: "/repo".to_string(),
            project_name: "repo".to_string(),
            contributions: vec![
                pdo_run::domain::run_cost::CostContribution {
                    harness: "claude".to_string(),
                    scope: pdo_run::domain::run_cost::CostScope::Node,
                    node_id: Some("n".to_string()),
                    executions: 1,
                    readable_executions: 1,
                    usd: Some(0.0),
                    form: Some(pdo_run::domain::state::CostForm::Derived),
                    reported_in_usd: false,
                    partial: true,
                    unpriced_models: vec!["claude-fable-5".to_string()],
                    unavailable_reasons: Vec::new(),
                    model_slices: Vec::new(),
                },
                pdo_run::domain::run_cost::CostContribution {
                    harness: "future".to_string(),
                    scope: pdo_run::domain::run_cost::CostScope::Node,
                    node_id: Some("n".to_string()),
                    executions: 1,
                    readable_executions: 0,
                    usd: None,
                    form: None,
                    reported_in_usd: false,
                    partial: false,
                    unpriced_models: Vec::new(),
                    unavailable_reasons: vec!["harness has no cost source".to_string()],
                    model_slices: Vec::new(),
                },
            ],
        };

        let stats = fold_harness_cost(&[run], Vec::new(), &Default::default());
        assert_eq!(stats.harnesses, vec!["claude", "future"]);
        assert_eq!(stats.total.usd, Some(0.0));
        assert!(stats.total.partial);
        assert_eq!(stats.total.executions, 1);
        assert_eq!(stats.total.readable, 0);
        assert_eq!(stats.total.unknown, 1);
        assert_eq!(stats.total.average_usd, None);
        assert_eq!(stats.total.unpriced_models, vec!["claude-fable-5"]);
        let claude = &stats.total.harnesses[0];
        assert_eq!(claude.average_usd, Some(0.0));
        assert_eq!(claude.readable, 1);
        let future = &stats.total.harnesses[1];
        assert_eq!(future.usd, None);
        assert_eq!(future.unknown, 1);
        assert_eq!(future.missing_reasons, vec!["harness has no cost source"]);
    }

    /// #811 — « Médiane, jamais la moyenne »: a skewed cohort (one execution an
    /// order of magnitude above the others, exactly the story's shape) where the
    /// median and the average disagree, mixed with an execution whose cost is
    /// unreadable. Both figures read the SAME readable population: the unreadable
    /// execution enters neither, and it is a median of 2 against an average of 11,
    /// not a second average under another name.
    #[test]
    fn cost_median_reads_only_readable_executions_and_resists_one_outlier() {
        fn run(bucket: &str, usd: Option<f64>) -> CostRunRow {
            CostRunRow {
                run_id: format!("r-{bucket}"),
                started_at: format!("{bucket}T09:00:00Z"),
                bucket: bucket.to_string(),
                identity: test_identity("p", "Pipeline", "n", "Node"),
                project_id: "/repo".to_string(),
                project_name: "repo".to_string(),
                contributions: vec![pdo_run::domain::run_cost::CostContribution {
                    harness: "claude".to_string(),
                    scope: pdo_run::domain::run_cost::CostScope::Node,
                    node_id: Some("n".to_string()),
                    executions: 1,
                    readable_executions: i64::from(usd.is_some()),
                    usd,
                    form: Some(pdo_run::domain::state::CostForm::Derived),
                    reported_in_usd: false,
                    partial: false,
                    unpriced_models: Vec::new(),
                    unavailable_reasons: match usd {
                        Some(_) => Vec::new(),
                        None => vec!["no readable transcript".to_string()],
                    },
                    model_slices: Vec::new(),
                }],
            }
        }

        let stats = fold_harness_cost(
            &[
                run("2026-07-15", Some(1.0)),
                run("2026-07-16", Some(2.0)),
                run("2026-07-17", Some(30.0)),
                run("2026-07-18", None),
            ],
            Vec::new(),
            &Default::default(),
        );

        assert_eq!(stats.total.executions, 4);
        assert_eq!(stats.total.readable, 3);
        assert_eq!(stats.total.unknown, 1);
        assert_eq!(stats.total.median_usd, Some(2.0));
        assert_eq!(stats.total.average_usd, Some(11.0));
        // The per-harness slice tells the same story, and so does the Node row —
        // which folds contributions one by one rather than Run by Run.
        let claude = &stats.total.harnesses[0];
        assert_eq!(claude.harness, "claude");
        assert_eq!(claude.median_usd, Some(2.0));
        assert_eq!(claude.average_usd, Some(11.0));
        let node = &stats.by_pipeline[0].nodes[0];
        assert_eq!(node.name, "Node");
        assert_eq!(node.aggregate.median_usd, Some(2.0));
        assert_eq!(node.aggregate.average_usd, Some(11.0));
    }

    #[test]
    fn a_cohort_without_one_readable_cost_has_no_median_rather_than_zero() {
        // Same discipline as `average_usd`: « un coût inconnu reste visible comme
        // « — », jamais comme `$0` » (CONTEXT.md).
        let stats = fold_harness_cost(
            &[CostRunRow {
                run_id: "r".to_string(),
                started_at: "2026-07-15T09:00:00Z".to_string(),
                bucket: "2026-07-15".to_string(),
                identity: AbsorptionResolver::default().run_identity(
                    &serde_json::json!({"pipeline_id": "p", "pipeline_name": "Pipeline"}),
                ),
                project_id: "/repo".to_string(),
                project_name: "repo".to_string(),
                contributions: vec![pdo_run::domain::run_cost::CostContribution {
                    harness: "future".to_string(),
                    scope: pdo_run::domain::run_cost::CostScope::Node,
                    node_id: Some("n".to_string()),
                    executions: 1,
                    readable_executions: 0,
                    usd: None,
                    form: None,
                    reported_in_usd: false,
                    partial: false,
                    unpriced_models: Vec::new(),
                    unavailable_reasons: vec!["harness has no cost source".to_string()],
                    model_slices: Vec::new(),
                }],
            }],
            Vec::new(),
            &Default::default(),
        );
        assert_eq!(stats.total.median_usd, None);
        assert_eq!(stats.total.average_usd, None);
    }

    #[test]
    fn cost_project_root_buckets_a_legacy_null_target_run_under_the_daemon_root() {
        // #470/ADR-0033: the write boundary is hardened, this READ is not. A
        // `run_started` from before the change carries no `target_repo`, and the
        // "by project" axis must still place it — there is no "Unassigned" bucket
        // (invariant #6, #258). Removing this fallback would make ~46 of 101 dev
        // runs vanish from the cost cockpit.
        let payload = serde_json::json!({ "pipeline_name": "alpha" });
        assert_eq!(
            cost_project_root(&payload, Path::new("/daemon/root")),
            PathBuf::from("/daemon/root")
        );
    }

    // --- « By model » axis (ADR-0065, #735) ---

    fn model_slice(
        model: &str,
        observed: bool,
        effort: Option<&str>,
        usd: Option<f64>,
    ) -> pdo_run::domain::run_cost::ModelEffortSlice {
        pdo_run::domain::run_cost::ModelEffortSlice {
            model: model.to_string(),
            model_observed: observed,
            effort: effort.map(String::from),
            effort_observed: effort.map(|_| false),
            provider: None,
            usd,
            estimated: usd.is_some(),
            partial: false,
            executions: 1,
            unpriced_models: Vec::new(),
            missing_reasons: usd
                .is_none()
                .then(|| "no attributable Claude transcript".to_string())
                .into_iter()
                .collect(),
        }
    }

    fn test_identity(
        pipeline_id: &str,
        pipeline_name: &str,
        node_id: &str,
        node_name: &str,
    ) -> RunIdentity {
        AbsorptionResolver::default().run_identity(&serde_json::json!({
            "pipeline_id": pipeline_id,
            "pipeline_name": pipeline_name,
            "node_defs": [{"id": node_id, "name": node_name, "node_type": "agent"}],
        }))
    }

    fn node_run_row(
        bucket: &str,
        node_id: &str,
        node_name: &str,
        contributions: Vec<pdo_run::domain::run_cost::CostContribution>,
    ) -> CostRunRow {
        CostRunRow {
            run_id: format!("r-{bucket}-{node_id}"),
            started_at: format!("{bucket}T09:00:00Z"),
            bucket: bucket.to_string(),
            identity: test_identity("p", "Impl", node_id, node_name),
            project_id: "/repo".to_string(),
            project_name: "repo".to_string(),
            contributions,
        }
    }

    fn claude_node_contribution(
        node_id: &str,
        usd: Option<f64>,
        slices: Vec<pdo_run::domain::run_cost::ModelEffortSlice>,
    ) -> pdo_run::domain::run_cost::CostContribution {
        pdo_run::domain::run_cost::CostContribution {
            harness: "claude".to_string(),
            scope: pdo_run::domain::run_cost::CostScope::Node,
            node_id: Some(node_id.to_string()),
            executions: 1,
            readable_executions: i64::from(usd.is_some()),
            usd,
            form: usd.map(|_| pdo_run::domain::state::CostForm::Derived),
            reported_in_usd: false,
            partial: false,
            unpriced_models: Vec::new(),
            unavailable_reasons: usd
                .is_none()
                .then(|| "no attributable Claude transcript".to_string())
                .into_iter()
                .collect(),
            model_slices: slices,
        }
    }

    #[test]
    fn by_model_tree_ventilates_models_efforts_pipelines_nodes_and_names_provenance() {
        // Run A: one execution, two observed models (opus@high, sonnet@medium).
        let run_a = node_run_row(
            "2026-09-01",
            "n",
            "Worker",
            vec![claude_node_contribution(
                "n",
                Some(8.0),
                vec![
                    model_slice("claude-opus-4-8", true, Some("high"), Some(5.0)),
                    model_slice("claude-sonnet-5", true, Some("medium"), Some(3.0)),
                ],
            )],
        );
        // Run B: a mute-source execution falling back to the requested sonnet@high.
        let run_b = node_run_row(
            "2026-09-02",
            "n2",
            "Other",
            vec![claude_node_contribution(
                "n2",
                None,
                vec![model_slice("claude-sonnet-5", false, Some("high"), None)],
            )],
        );

        let stats = fold_harness_cost(&[run_a, run_b], Vec::new(), &Default::default());
        let models = &stats.by_model;
        assert_eq!(
            models
                .iter()
                .map(|m| m.entity.id.as_str())
                .collect::<Vec<_>>(),
            vec!["claude-opus-4-8", "claude-sonnet-5"],
            "ranked by cost, ids verbatim"
        );

        let opus = &models[0];
        assert_eq!(opus.provenance, StatsProvenance::Observed);
        assert_eq!(opus.entity.aggregate.usd, Some(5.0));
        assert_eq!(opus.entity.aggregate.executions, 1);

        let sonnet = &models[1];
        assert_eq!(
            sonnet.provenance,
            StatsProvenance::Mixed,
            "one execution observed it, one fell back to the requested id"
        );
        assert_eq!(sonnet.entity.aggregate.usd, Some(3.0));
        assert_eq!(
            sonnet.entity.aggregate.executions, 2,
            "one execution per bucket"
        );
        let effort_names: Vec<&str> = sonnet
            .efforts
            .iter()
            .map(|e| e.entity.name.as_str())
            .collect();
        assert_eq!(effort_names, vec!["medium", "high"], "ranked by cost");
        let medium = &sonnet.efforts[0];
        assert_eq!(medium.entity.id, "medium");
        assert_eq!(medium.effort.as_deref(), Some("medium"));
        assert_eq!(medium.provenance, Some(StatsProvenance::Requested));
        let pipeline = &medium.pipelines[0];
        assert_eq!(pipeline.id, "p");
        assert_eq!(pipeline.nodes.len(), 1);
        assert_eq!(pipeline.nodes[0].name, "Worker");
        let high = &sonnet.efforts[1];
        assert_eq!(high.entity.aggregate.usd, None);
        assert_eq!(high.entity.aggregate.unknown, 1);
        assert_eq!(
            high.entity.aggregate.missing_reasons,
            vec!["no attributable Claude transcript"]
        );
        assert!(high.pipelines[0]
            .nodes
            .iter()
            .any(|node| node.name == "Other"));

        // The by_pipeline leaves carry the pairs; the by_model leaves do not.
        let pipeline_row = stats.by_pipeline.iter().find(|row| row.id == "p").unwrap();
        let worker = pipeline_row
            .nodes
            .iter()
            .find(|node| node.id == "n")
            .unwrap();
        assert_eq!(worker.models.len(), 2);
        let pair = &worker.models[0];
        assert_eq!(pair.model, "claude-opus-4-8");
        assert_eq!(pair.model_provenance, StatsProvenance::Observed);
        assert_eq!(pair.effort.as_deref(), Some("high"));
        assert_eq!(pair.effort_provenance, Some(StatsProvenance::Requested));
        assert_eq!(pair.aggregate.usd, Some(5.0));
        assert_eq!(pair.aggregate.harnesses[0].harness, "claude");
        let other = pipeline_row
            .nodes
            .iter()
            .find(|node| node.id == "n2")
            .unwrap();
        assert_eq!(other.models.len(), 1);
        assert_eq!(other.models[0].model_provenance, StatsProvenance::Requested);
    }

    #[test]
    fn by_model_merges_one_id_across_harnesses_and_says_where_each_half_was_read() {
        // The same id via `claude` (observed per message, effort requested) and
        // via `pi` (observed per message, effort observed from the thinking
        // level, provider openrouter) is ONE row with TWO harness columns
        // (#736, ADR-0065 §2); the totals add up (ADR-0052).
        let mut claude = claude_node_contribution(
            "n",
            Some(3.0),
            vec![model_slice(
                "claude-sonnet-5",
                true,
                Some("high"),
                Some(3.0),
            )],
        );
        claude.harness = "claude".to_string();
        let mut pi_slice = model_slice("claude-sonnet-5", true, Some("low"), Some(0.5));
        pi_slice.effort_observed = Some(true);
        pi_slice.estimated = false;
        pi_slice.provider = Some("openrouter".to_string());
        let pi = pdo_run::domain::run_cost::CostContribution {
            harness: "pi".to_string(),
            ..claude.clone()
        };
        let pi = pdo_run::domain::run_cost::CostContribution {
            model_slices: vec![pi_slice],
            ..pi
        };
        let stats = fold_harness_cost(
            &[node_run_row("2026-09-01", "n", "Worker", vec![claude, pi])],
            Vec::new(),
            &Default::default(),
        );

        assert_eq!(stats.by_model.len(), 1, "one id, one row across harnesses");
        let row = &stats.by_model[0];
        assert_eq!(row.entity.id, "claude-sonnet-5");
        assert_eq!(row.entity.aggregate.usd, Some(3.5), "the totals add up");
        assert_eq!(row.entity.aggregate.executions, 2);
        let harnesses = &row.entity.aggregate.harnesses;
        assert_eq!(
            harnesses
                .iter()
                .map(|h| h.harness.as_str())
                .collect::<Vec<_>>(),
            vec!["claude", "pi"]
        );
        let claude_entry = &harnesses[0];
        assert_eq!(claude_entry.usd, Some(3.0));
        assert_eq!(claude_entry.provenance, Some(StatsProvenance::Observed));
        assert_eq!(
            claude_entry.effort_provenance,
            Some(StatsProvenance::Requested),
            "claude's source never writes the effort"
        );
        assert_eq!(claude_entry.provider, None);
        let pi_entry = &harnesses[1];
        assert_eq!(pi_entry.usd, Some(0.5));
        assert_eq!(pi_entry.provenance, Some(StatsProvenance::Observed));
        assert_eq!(
            pi_entry.effort_provenance,
            Some(StatsProvenance::Observed),
            "the thinking level is observed"
        );
        assert_eq!(
            pi_entry.provider.as_deref(),
            Some("openrouter"),
            "the provider rides the harness entry, tooltip only"
        );
        assert!(!pi_entry.estimated, "a reported slice is not an estimate");

        // Two efforts under the one model row: low (observed) and high
        // (requested), each carrying its per-harness provenance.
        let efforts: Vec<&str> = row.efforts.iter().map(|e| e.entity.id.as_str()).collect();
        assert_eq!(efforts, vec!["high", "low"], "ranked by cost");
        let low = &row.efforts[1];
        assert_eq!(low.provenance, Some(StatsProvenance::Observed));
        assert_eq!(
            low.entity
                .aggregate
                .harnesses
                .iter()
                .find(|h| h.harness == "pi")
                .unwrap()
                .effort_provenance,
            Some(StatsProvenance::Observed)
        );
    }

    #[test]
    fn by_model_keeps_not_set_effort_distinct_and_puts_a_dollar_value_on_it() {
        // Infrastructure (leftover) sessions carry observed models but no effort.
        let infra = pdo_run::domain::run_cost::CostContribution {
            harness: "claude".to_string(),
            scope: pdo_run::domain::run_cost::CostScope::Infrastructure,
            node_id: None,
            executions: 1,
            readable_executions: 1,
            usd: Some(2.0),
            form: Some(pdo_run::domain::state::CostForm::Derived),
            reported_in_usd: false,
            partial: false,
            unpriced_models: Vec::new(),
            unavailable_reasons: Vec::new(),
            model_slices: vec![model_slice("claude-opus-4-8", true, None, Some(2.0))],
        };
        let run = node_run_row("2026-09-01", "n", "Worker", vec![infra]);

        let stats = fold_harness_cost(&[run], Vec::new(), &Default::default());
        let opus = stats
            .by_model
            .iter()
            .find(|m| m.entity.id == "claude-opus-4-8")
            .unwrap();
        assert_eq!(opus.efforts.len(), 1);
        let not_set = &opus.efforts[0];
        assert_eq!(not_set.entity.id, "", "not set is the empty effort id");
        assert_eq!(not_set.entity.name, "not set");
        assert_eq!(not_set.effort, None);
        assert_eq!(
            not_set.provenance, None,
            "not set has no provenance to show"
        );
        assert_eq!(not_set.entity.aggregate.usd, Some(2.0));
        assert_eq!(not_set.pipelines[0].nodes[0].name, "Infrastructure");
    }

    #[test]
    fn by_model_never_counts_a_harness_without_cost_source() {
        let run = node_run_row(
            "2026-09-01",
            "n",
            "Worker",
            vec![pdo_run::domain::run_cost::CostContribution {
                harness: "opencode".to_string(),
                scope: pdo_run::domain::run_cost::CostScope::Node,
                node_id: Some("n".to_string()),
                executions: 1,
                readable_executions: 0,
                usd: None,
                form: None,
                reported_in_usd: false,
                partial: false,
                unpriced_models: Vec::new(),
                unavailable_reasons: vec!["harness has no cost source".to_string()],
                model_slices: Vec::new(),
            }],
        );
        let stats = fold_harness_cost(&[run], Vec::new(), &Default::default());
        assert!(
            stats.by_model.is_empty(),
            "a harness without a cost source is absent, never a \"default of X\" bucket"
        );
        // The existing axes keep saying the absence.
        assert_eq!(stats.total.unknown, 1);
        assert_eq!(
            stats.total.missing_reasons,
            vec!["harness has no cost source"]
        );
    }

    #[test]
    fn by_model_periods_follow_the_drill_for_the_harness_bars() {
        let run = node_run_row(
            "2026-09-01",
            "n",
            "Worker",
            vec![claude_node_contribution(
                "n",
                Some(5.0),
                vec![model_slice(
                    "claude-opus-4-8",
                    true,
                    Some("high"),
                    Some(5.0),
                )],
            )],
        );
        let stats = fold_harness_cost(&[run], Vec::new(), &Default::default());
        let opus = &stats.by_model[0];
        assert_eq!(opus.entity.by_period.len(), 1);
        assert_eq!(opus.entity.by_period[0].bucket, "2026-09-01");
        assert_eq!(opus.entity.by_period[0].aggregate.usd, Some(5.0));
        assert_eq!(opus.efforts[0].entity.by_period[0].aggregate.usd, Some(5.0));
        assert_eq!(
            opus.efforts[0].pipelines[0].by_period[0].aggregate.usd,
            Some(5.0)
        );
        assert_eq!(
            opus.efforts[0].pipelines[0].nodes[0].by_period[0]
                .aggregate
                .usd,
            Some(5.0)
        );
    }

    #[test]
    fn pipeline_headline_excludes_infrastructure_and_unassigned_cost() {
        // #742: a pipeline's headline is its nodes' work. The Infrastructure
        // bucket (manager + merge-resolver sessions) and the Unassigned
        // orphans stay visible as node rows under the pipeline, but they no
        // longer sum into its aggregate, its periods, the project levels or
        // the « By model » axis pipeline. The instance total keeps every
        // contribution (ADR-0058: a run's total is what it actually spent).
        let node = claude_node_contribution(
            "n",
            Some(5.0),
            vec![model_slice(
                "claude-opus-4-8",
                true,
                Some("high"),
                Some(5.0),
            )],
        );
        let infra = pdo_run::domain::run_cost::CostContribution {
            harness: "claude".to_string(),
            scope: pdo_run::domain::run_cost::CostScope::Infrastructure,
            node_id: None,
            executions: 1,
            readable_executions: 1,
            usd: Some(2.0),
            form: Some(pdo_run::domain::state::CostForm::Derived),
            reported_in_usd: false,
            partial: false,
            unpriced_models: Vec::new(),
            unavailable_reasons: Vec::new(),
            model_slices: vec![model_slice(
                "claude-opus-4-8",
                true,
                Some("high"),
                Some(2.0),
            )],
        };
        let unassigned = pdo_run::domain::run_cost::CostContribution {
            usd: Some(1.0),
            readable_executions: 1,
            form: Some(pdo_run::domain::state::CostForm::Derived),
            scope: pdo_run::domain::run_cost::CostScope::Unassigned,
            model_slices: Vec::new(),
            ..node.clone()
        };
        let run = node_run_row("2026-09-01", "n", "Worker", vec![node, infra, unassigned]);

        let stats = fold_harness_cost(&[run], Vec::new(), &Default::default());
        assert_eq!(stats.total.usd, Some(8.0), "the run total is unchanged");

        let pipeline = &stats.by_pipeline[0];
        assert_eq!(pipeline.id, "p");
        assert_eq!(pipeline.aggregate.usd, Some(5.0), "headline = node work");
        assert_eq!(pipeline.by_period[0].aggregate.usd, Some(5.0));
        let rows: Vec<(&str, Option<f64>)> = pipeline
            .nodes
            .iter()
            .map(|n| (n.id.as_str(), n.aggregate.usd))
            .collect();
        assert_eq!(
            rows,
            vec![
                ("n", Some(5.0)),
                ("p:infrastructure", Some(2.0)),
                ("p:unassigned", Some(1.0))
            ],
            "infrastructure/unassigned stay visible as node rows"
        );

        let project = &stats.by_project[0];
        assert_eq!(project.entity.aggregate.usd, Some(5.0));
        assert_eq!(project.pipelines[0].aggregate.usd, Some(5.0));

        // The « By model » axis: the model level keeps every contribution
        // (infrastructure sessions burn model tokens), the pipeline level
        // under an effort does not.
        let opus = &stats.by_model[0];
        assert_eq!(opus.entity.aggregate.usd, Some(7.0));
        let high = &opus.efforts[0];
        assert_eq!(high.entity.id, "high");
        let axis_pipeline = &high.pipelines[0];
        assert_eq!(axis_pipeline.id, "p");
        assert_eq!(
            axis_pipeline.aggregate.usd,
            Some(5.0),
            "model-axis pipeline = node work too"
        );
        assert!(axis_pipeline
            .nodes
            .iter()
            .any(|n| n.id == "p:infrastructure" && n.aggregate.usd == Some(2.0)));
    }
}
