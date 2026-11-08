//! The Run's cost breakdown: every execution's contribution, attributed and
//! ventilated by harness (ADR-0052).

use super::*;

pub fn compute_run_cost_breakdown(
    events: &[crate::domain::event::Event],
    claude_root: &Path,
    stores: &HarnessStores,
    repo_root: &Path,
    run_id: &str,
    prices: &PriceTable,
) -> RunCostBreakdown {
    let executions = collect_executions(events, claude_root, repo_root, run_id);

    let run_dir = repo_root.join(".pdo").join("runs").join(run_id);
    let prefix = format!("{}-", cc_project_dirname(&run_dir));
    let mut project_dirs: Vec<PathBuf> = claude_roots(events, claude_root)
        .iter()
        .flat_map(std::fs::read_dir)
        .flatten()
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(&prefix))
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    project_dirs.sort();

    let mut owned_lines: Vec<Vec<Line>> = (0..executions.len()).map(|_| Vec::new()).collect();
    let mut owned_files = vec![false; executions.len()];
    let mut leftover_lines = Vec::new();
    let mut leftover_files = false;
    for project in &project_dirs {
        let mut files = Vec::new();
        collect_jsonl_paths(project, &mut files);
        files.sort();
        for file in files {
            let owner = executions
                .iter()
                .enumerate()
                .find_map(|(index, execution)| {
                    if execution.harness != pdo_harness::registry::CLAUDE
                        || execution.project.as_ref() != Some(project)
                    {
                        return None;
                    }
                    if let Some(session_id) = execution.session_id.as_deref() {
                        let main = project.join(format!("{session_id}.jsonl"));
                        let subagents = project.join(session_id).join("subagents");
                        (file == main || file.starts_with(subagents)).then_some(index)
                    } else {
                        execution.legacy_claim.then_some(index)
                    }
                });
            let mut lines = Vec::new();
            collect_jsonl_file(&file, &mut lines);
            if let Some(index) = owner {
                owned_files[index] = true;
                owned_lines[index].extend(lines);
            } else {
                leftover_files = true;
                leftover_lines.extend(lines);
            }
        }
    }

    let mut contributions = Vec::new();
    let mut seen_messages = HashSet::new();
    for (index, execution) in executions.iter().enumerate() {
        let mut reported_in_usd = false;
        let mut observed_models: Option<BTreeMap<String, ModelUsage>> = None;
        let mut observed_slices: Vec<ModelEffortSlice> = Vec::new();
        let (cost, form, reason) = if !pdo_harness::probes::can_cost(&execution.harness) {
            (None, None, Some("harness has no cost source".to_string()))
        } else if pdo_harness::probes::probes_for(&execution.harness).and_then(|p| p.cost_source())
            == Some(pdo_harness::probes::CostSource::ReportedByConstant)
        {
            // #615/#707, ADR-0052: a reported cost — `copilot`'s journal, `pi`'s
            // session — read by identity, never through the price table. The
            // same read yields the observed model × effort slices (#736).
            let (usd, unavailable, in_usd, slices) = reported_execution(execution, stores);
            reported_in_usd = in_usd;
            observed_slices = slices;
            (
                usd.map(|usd| CostStat {
                    usd,
                    partial: false,
                    unpriced_models: Vec::new(),
                    uncosted_harnesses: Vec::new(),
                    by_harness: Vec::new(),
                }),
                Some(CostForm::Reported),
                unavailable,
            )
        } else if execution.harness == pdo_harness::registry::CLAUDE
            && execution.unavailable_reason.is_none()
            && owned_files[index]
        {
            let (stat, by_model) =
                aggregate_by_model(owned_lines[index].drain(..), prices, &mut seen_messages);
            observed_models = Some(by_model);
            (Some(stat), Some(CostForm::Derived), None)
        } else {
            (
                None,
                Some(CostForm::Derived),
                execution
                    .unavailable_reason
                    .clone()
                    .or_else(|| Some("no attributable Claude transcript".to_string())),
            )
        };
        let usd = cost.as_ref().map(|cost| cost.usd);
        let unavailable_reasons: Vec<String> = cost
            .is_none()
            .then_some(reason)
            .flatten()
            .into_iter()
            .collect();
        let model_slices = execution_model_slices(
            execution,
            observed_slices,
            DerivedFold {
                models: observed_models,
                usd,
                form,
                partial: cost.as_ref().map(|cost| cost.partial).unwrap_or(false),
                unpriced_models: cost
                    .as_ref()
                    .map(|cost| cost.unpriced_models.clone())
                    .unwrap_or_default(),
                missing_reasons: unavailable_reasons.clone(),
            },
        );
        contributions.push(CostContribution {
            harness: execution.harness.clone(),
            scope: CostScope::Node,
            node_id: execution.node_id.clone(),
            executions: 1,
            readable_executions: i64::from(usd.is_some()),
            usd,
            form: usd.and(form),
            reported_in_usd: usd.is_some() && reported_in_usd,
            partial: cost.as_ref().is_some_and(|cost| cost.partial),
            unpriced_models: cost
                .as_ref()
                .map(|cost| cost.unpriced_models.clone())
                .unwrap_or_default(),
            unavailable_reasons,
            model_slices,
            started_event: Some(execution.started_event),
        });
    }

    let run_harness = events
        .iter()
        .find(|event| event.kind == crate::domain::event::EventKind::RunStarted)
        .and_then(|event| event.payload.as_ref())
        .and_then(|payload| payload.get("harness"))
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty())
        .unwrap_or(pdo_harness::registry::CLAUDE);
    let infrastructure_executions = i64::from(
        events
            .iter()
            .any(|event| event.kind == crate::domain::event::EventKind::RunStarted),
    ) + events
        .iter()
        .filter(|event| event.kind == crate::domain::event::EventKind::MergeResolverStarted)
        .count() as i64;
    let ambiguous_shared_claude = executions.iter().any(|execution| {
        execution.harness == pdo_harness::registry::CLAUDE
            && execution.session_id.is_none()
            && !execution.legacy_claim
    });
    let leftover = leftover_files
        .then(|| aggregate_by_model(leftover_lines.into_iter(), prices, &mut seen_messages));
    if infrastructure_executions > 0 {
        let attributable = !ambiguous_shared_claude;
        contributions.push(CostContribution {
            harness: if attributable && leftover.is_some() {
                pdo_harness::registry::CLAUDE.to_string()
            } else {
                run_harness.to_string()
            },
            scope: CostScope::Infrastructure,
            node_id: None,
            executions: infrastructure_executions,
            readable_executions: if attributable && leftover.is_some() {
                infrastructure_executions
            } else {
                0
            },
            usd: attributable
                .then(|| leftover.as_ref().map(|(cost, _)| cost.usd))
                .flatten(),
            form: (attributable && leftover.is_some()).then_some(CostForm::Derived),
            reported_in_usd: false,
            partial: attributable && leftover.as_ref().is_some_and(|(cost, _)| cost.partial),
            unpriced_models: if attributable {
                leftover
                    .as_ref()
                    .map(|(cost, _)| cost.unpriced_models.clone())
                    .unwrap_or_default()
            } else {
                Vec::new()
            },
            unavailable_reasons: if attributable && leftover.is_some() {
                Vec::new()
            } else {
                vec!["no attributable infrastructure cost".to_string()]
            },
            // The infrastructure sessions' models are observed off the same
            // leftover lines; they carry no effort ("not set"). Unattributable
            // leftovers go to the Unassigned bucket below instead.
            model_slices: if attributable {
                leftover
                    .as_ref()
                    .map(|(_, models)| leftover_model_slices(models, infrastructure_executions))
                    .unwrap_or_default()
            } else {
                Vec::new()
            },
            started_event: None,
        });
    }
    if let Some((cost, models)) =
        leftover.filter(|_| ambiguous_shared_claude || infrastructure_executions == 0)
    {
        contributions.push(CostContribution {
            harness: pdo_harness::registry::CLAUDE.to_string(),
            scope: CostScope::Unassigned,
            node_id: None,
            executions: 0,
            readable_executions: 0,
            usd: Some(cost.usd),
            form: Some(CostForm::Derived),
            reported_in_usd: false,
            partial: cost.partial,
            unpriced_models: cost.unpriced_models,
            unavailable_reasons: vec![
                "Claude transcript cannot be matched to a node or infrastructure".to_string(),
            ],
            // The orphan lines' models are still known (observed) even though
            // their node is not: they ventilate inside the model axis, under the
            // pipeline's "Unassigned" node, effort "not set".
            model_slices: leftover_model_slices(&models, 0),
            started_event: None,
        });
    }

    let any_readable = contributions.iter().any(|c| c.usd.is_some());
    let unpriced_models: Vec<String> = contributions
        .iter()
        .flat_map(|c| c.unpriced_models.iter().cloned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let uncosted_harnesses: Vec<String> = contributions
        .iter()
        .filter(|contribution| {
            contribution.usd.is_none()
                && contribution
                    .unavailable_reasons
                    .iter()
                    .any(|reason| reason == "harness has no cost source")
        })
        .map(|contribution| contribution.harness.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let mut harnesses: BTreeMap<String, HarnessCost> = BTreeMap::new();
    for contribution in &contributions {
        let (Some(usd), Some(form)) = (contribution.usd, contribution.form) else {
            continue;
        };
        let entry = harnesses
            .entry(contribution.harness.clone())
            .or_insert_with(|| HarnessCost {
                harness: contribution.harness.clone(),
                usd: 0.0,
                form,
                partial: false,
                unpriced_models: Vec::new(),
                // A slice is "in dollars" only if every contribution is: ANDed
                // below, seeded true so the first one decides.
                reported_in_usd: true,
            });
        entry.usd += usd;
        entry.reported_in_usd &= contribution.reported_in_usd;
        entry.partial |= contribution.partial;
        entry
            .unpriced_models
            .extend(contribution.unpriced_models.iter().cloned());
        entry.unpriced_models.sort();
        entry.unpriced_models.dedup();
    }
    let slices: Vec<HarnessCost> = harnesses.into_values().collect();
    let cost = if !uncosted_harnesses.is_empty() {
        Some(cost_unavailable(uncosted_harnesses, slices))
    } else {
        any_readable.then(|| CostStat {
            usd: contributions.iter().filter_map(|c| c.usd).sum(),
            partial: !unpriced_models.is_empty(),
            unpriced_models,
            uncosted_harnesses: Vec::new(),
            by_harness: slices,
        })
    };
    RunCostBreakdown {
        contributions,
        cost,
    }
}

/// A Run whose cost is **unavailable** because a node ran on a harness with no
/// cost source — "—" with a reason, never a `$0`.
///
/// `slices` are the per-harness costs PDO could still compute for the Run's
/// *other* nodes; they **survive** the unavailable total (ADR-0052 §3). What is
/// refused is the sum, not the knowledge.
pub fn cost_unavailable(uncosted: Vec<String>, slices: Vec<HarnessCost>) -> CostStat {
    CostStat {
        // Not a total, and never rendered as one: `uncosted_harnesses` non-empty is
        // what makes every surface print "—". The slices say themselves; this field
        // stays 0.0 rather than half a sum, which would read as a total.
        usd: 0.0,
        // NOT `partial`: `partial` means "priced, but a lower bound" and still
        // shows a dollar figure. This is a categorically different state — the
        // aggregate is unavailable, not merely incomplete — so it stays out of
        // the `partial ⟺ !unpriced_models.is_empty()` invariant (both empty).
        // A slice keeps its own `partial`; the Run-level one is about the total.
        partial: false,
        unpriced_models: Vec::new(),
        uncosted_harnesses: uncosted,
        by_harness: slices,
    }
}
