//! What a Run's detail reads live from disk on top of its projection ([`LiveFacts`]):
//! its lines-of-code delta (`git diff --numstat`), its cost (the harnesses'
//! own files, priced by the table's disk tiers), and the run-scoped pipeline's
//! graph. Never fails: an unreadable source leaves its field as the projection
//! had it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use pdo_sandbox::adapters::staging::stores::{transcripts_root, HarnessStores};

use crate::adapters::run_cost::{compute_run_cost_breakdown_cached, CostScope};
use crate::application::paths::run_scoped_pipeline_path;
use crate::application::ports::{BoxFuture, LiveFacts};
use crate::domain::event::{edge_info_from_pipeline, node_def_from_pipeline, Event};
use crate::domain::prices::PriceTable;
use crate::domain::run_cost::loop_cost::loop_costs;
use crate::domain::run_refs;
use crate::domain::state::{CostStat, LocStat, LoopCost, NodeCost, RunState};

/// The disk of this daemon's Runs.
pub struct DiskLiveFacts {
    /// The daemon's repo root: the repository of a historical Run that names
    /// none (ADR-0033).
    repo_root: PathBuf,
    /// Replaces `$HOME` for the sandbox staging and the price files (the
    /// per-daemon test seam).
    home_override: Option<PathBuf>,
}

impl DiskLiveFacts {
    pub fn new(repo_root: PathBuf, home_override: Option<PathBuf>) -> Self {
        Self {
            repo_root,
            home_override,
        }
    }

    fn repo_root(&self, run: &RunState) -> PathBuf {
        run.target_repo
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| self.repo_root.clone())
    }

    /// Where a Run's cost is read from: the transcripts (the staged home while a
    /// sandboxed Run lives, `~/.claude/projects/` otherwise), the reported
    /// stores, and the price table of the HOST home — prices are an instance
    /// concept. Resolved per read, never cached: a price edit needs no restart
    /// (ADR-0034). HOME absent → the host root degrades; never fail a read.
    fn cost_sources(&self, run: &RunState) -> (PathBuf, HarnessStores, PriceTable) {
        let (home_root, sandbox_root) = match &self.home_override {
            Some(home) => pdo_sandbox::adapters::host::roots_under(home),
            None => pdo_sandbox::adapters::host::default_roots_from_env().unwrap_or_else(|| {
                let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
                let sandbox = home.join(".pdo").join("sandbox");
                (home, sandbox)
            }),
        };
        let sandboxed = !run.sandbox.is_off();
        let projects_root = transcripts_root(sandboxed, &run.run_id, &home_root, &sandbox_root);
        let stores = HarnessStores::for_run(sandboxed, &run.run_id, &home_root, &sandbox_root);
        (projects_root, stores, PriceTable::load(&home_root))
    }
}

impl DiskLiveFacts {
    /// The Run's cost, ventilated by execution and by model × effort, with
    /// the tokens the fold counted: what the instance's telemetry records once
    /// the Run ended. Reads the disk: call it off the runtime.
    pub fn cost_breakdown(
        &self,
        run: &RunState,
        events: &[Event],
    ) -> crate::domain::run_cost::RunCostBreakdown {
        let (projects_root, stores, prices) = self.cost_sources(run);
        compute_run_cost_breakdown_cached(
            events,
            &projects_root,
            &stores,
            &self.repo_root(run),
            &run.run_id,
            &prices,
        )
    }
}

impl LiveFacts for DiskLiveFacts {
    fn fill<'a>(&'a self, run: &'a mut RunState, events: &'a [Event]) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            let repo_root = self.repo_root(run);
            let (projects_root, stores, prices) = self.cost_sources(run);
            augment_run_state_from_disk(run, events, &projects_root, &stores, &repo_root, &prices);
        })
    }

    fn cost<'a>(
        &'a self,
        run: &'a RunState,
        events: &'a [Event],
    ) -> BoxFuture<'a, Option<CostStat>> {
        Box::pin(async move {
            let (projects_root, stores, prices) = self.cost_sources(run);
            compute_run_cost_breakdown_cached(
                events,
                &projects_root,
                &stores,
                &self.repo_root(run),
                &run.run_id,
                &prices,
            )
            .cost
        })
    }

    fn loop_costs<'a>(
        &'a self,
        run: &'a RunState,
        events: &'a [Event],
        regions: &'a [pdo_pipeline::domain::pipeline::LoopRegion],
    ) -> BoxFuture<'a, HashMap<String, LoopCost>> {
        Box::pin(async move {
            let breakdown = self.cost_breakdown(run, events);
            loop_costs(&breakdown, events, regions)
        })
    }
}

/// Parse `git diff --numstat` output into a [`LocStat`].
///
/// numstat rows are `added\tdeleted\tpath`. For **binary** files git emits `-` in
/// the count columns, so their +/- contribution is skipped but the file is still
/// counted. An empty input yields `{0, 0, 0}` — the caller distinguishes that clean
/// diff from an uncomputable one (`None`).
pub fn parse_numstat(stdout: &str) -> LocStat {
    let mut insertions = 0u64;
    let mut deletions = 0u64;
    let mut files_changed = 0u64;
    for line in stdout.lines() {
        let mut cols = line.splitn(3, '\t');
        let added = cols.next().unwrap_or("");
        let deleted = cols.next().unwrap_or("");
        // A valid numstat row has a third (path) column.
        if cols.next().is_none() {
            continue;
        }
        files_changed += 1;
        if let Ok(a) = added.parse::<u64>() {
            insertions += a;
        }
        if let Ok(d) = deleted.parse::<u64>() {
            deletions += d;
        }
    }
    LocStat {
        insertions,
        deletions,
        files_changed,
    }
}

/// The base ref for a Run's "what changed" diff: the frozen fork SHA, else the
/// recorded source branch, else `HEAD`.
///
/// The call sites use a uniform 3-dot range: because `pdo/run-<id>` is cut from
/// `fork_sha` and only gains commits on top, `merge-base(fork_sha, run) == fork_sha`,
/// so one range form is correct for all three tiers and never re-introduces the
/// wandering-HEAD defect. NB: this is the Run's fork point, NOT the per-node
/// `NodeStarted.base_sha` (ADR-0036).
pub fn run_diff_base(run_state: &RunState) -> &str {
    run_refs::fork_base(run_state)
}

/// Lines changed for a Run, with `.pdo/` excluded. `None` when the diff is
/// uncomputable (git missing, not a repo, run branch gone after cleanup) so the UI
/// renders "—"; a successful but empty diff returns `Some({0, 0, 0})`.
///
/// `base_ref` must be the Run's stable fork point, not the shared checkout's `HEAD`.
/// The **three-dot** range makes the base the merge-base, so the count stays correct
/// as `main` advances past the fork — two-dot would report later main commits as
/// deletions.
pub fn compute_run_loc(repo_root: &Path, run_id: &str, base_ref: &str) -> Option<LocStat> {
    let range = format!("{base_ref}...pdo/run-{run_id}");
    let output = std::process::Command::new("git")
        // `:(exclude).pdo/` is a literal pathspec argv element, no shell. `.pdo/` is
        // gitignored here, but external target repos may commit it.
        .args(["diff", "--numstat", &range, "--", ".", ":(exclude).pdo/"])
        .current_dir(repo_root)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Some(parse_numstat(&stdout))
}

pub fn augment_run_state_from_disk(
    run_state: &mut RunState,
    events: &[Event],
    projects_root: &Path,
    stores: &HarnessStores,
    repo_root: &Path,
    prices: &PriceTable,
) {
    // LOC is independent of the run YAML: the run branch lives in `repo_root`, not
    // the run dir, so a missing/unparseable YAML must not suppress an otherwise-valid
    // LOC — hence computing it before the YAML early-return below. The base is
    // resolved into an owned `String` so the immutable borrow of `run_state` ends
    // before the mutable assignment to `.loc`.
    let base = run_diff_base(run_state).to_string();
    run_state.loc = compute_run_loc(repo_root, &run_state.run_id, &base);
    let breakdown = compute_run_cost_breakdown_cached(
        events,
        projects_root,
        stores,
        repo_root,
        &run_state.run_id,
        prices,
    );
    run_state.cost = breakdown.cost.clone();

    let mut node_costs: HashMap<String, (NodeCost, bool)> = HashMap::new();
    for contribution in breakdown
        .contributions
        .iter()
        .filter(|contribution| contribution.scope == CostScope::Node)
    {
        let Some(node_id) = contribution.node_id.as_ref() else {
            continue;
        };
        let (cost, has_unknown) = node_costs.entry(node_id.clone()).or_insert_with(|| {
            (
                NodeCost {
                    usd: Some(0.0),
                    form: contribution.form,
                    // ANDed over the node's contributions below (#707): a node is
                    // "in dollars" only if every execution's reported cost is.
                    reported_in_usd: true,
                    partial: false,
                    unpriced_models: Vec::new(),
                    unavailable_reasons: Vec::new(),
                    executions: 0,
                    readable_executions: 0,
                },
                false,
            )
        });
        if cost.executions > 0 && cost.form != contribution.form {
            cost.form = None;
        } else if cost.executions == 0 {
            cost.form = contribution.form;
        }
        cost.reported_in_usd &= contribution.reported_in_usd;
        if let Some(usd) = contribution.usd {
            if !*has_unknown {
                cost.usd = Some(cost.usd.unwrap_or_default() + usd);
            }
        } else {
            *has_unknown = true;
            cost.usd = None;
            cost.form = None;
            cost.reported_in_usd = false;
        }
        cost.partial |= contribution.partial;
        cost.executions += contribution.executions;
        cost.readable_executions += contribution.readable_executions;
        cost.unpriced_models
            .extend(contribution.unpriced_models.iter().cloned());
        cost.unavailable_reasons
            .extend(contribution.unavailable_reasons.iter().cloned());
    }
    for (node_id, (mut cost, _)) in node_costs {
        cost.unpriced_models.sort();
        cost.unpriced_models.dedup();
        cost.unavailable_reasons.sort();
        cost.unavailable_reasons.dedup();
        if let Some(node) = run_state.nodes.get_mut(&node_id) {
            node.cost = Some(cost);
        }
    }

    let yaml_path = run_scoped_pipeline_path(repo_root, &run_state.run_id);
    let Ok(yaml) = std::fs::read_to_string(&yaml_path) else {
        return;
    };
    let Ok(parse_result) = pdo_pipeline::domain::pipeline::parse_pipeline(&yaml) else {
        return;
    };
    let pipe = &parse_result.pipeline;

    run_state.node_defs = pipe.nodes.iter().map(node_def_from_pipeline).collect();
    run_state.edges = pipe.edges.iter().map(edge_info_from_pipeline).collect();

    // The Coût de boucle of each region, folded from the SAME breakdown the Run
    // total and the node costs above were read from (ADR-0058), with the
    // effective cap the scheduler applies at re-entry (`$var` resolved against
    // the Run's variables).
    run_state.loop_costs = loop_costs(&breakdown, events, &pipe.loops);
    let vars = crate::application::scheduler::frontmatter::resolve_run_variables(pipe, events);
    // The live `set_region_max_cost` override wins, as it does at re-entry.
    let caps: Vec<(String, Option<f64>)> = pipe
        .loops
        .iter()
        .map(|region| {
            (
                region.id.clone(),
                crate::domain::scheduler::effective_region_max_cost(run_state, region, &vars),
            )
        })
        .collect();
    for (region_id, cap) in caps {
        if let Some(cost) = run_state.loop_costs.get_mut(&region_id) {
            cost.cap_usd = cap;
        }
    }
}
