//! `GET /stats/performance` (#585): compare context and duration by Node and
//! harness across Claude/Copilot. The fifth Stats section, alongside
//! `stats::stats_overview` and `stats::stats_cost` — same cohort rule (Runs
//! selected by `run_started` date, full execution retained), same derive-at-read
//! posture (ADR-0029: no snapshot table, no persisted aggregate, an in-memory
//! memo only), but its own success-only filter and its own two units (a token
//! peak, a wall-clock duration) instead of a dollar figure.
//!
//! ## Shape
//!
//! The [`StatsPerformance`] tree is the **agreed seam with the frontend**
//! (`frontend/src/types.ts`); [`StatsDistribution`] has since been revised past
//! what that file declares (see its doc comment) — the frontend is expected to
//! catch up to this shape, along with `infrastructure_total`, not the reverse.
//! A Node's subagent groups never feed their parent's own distribution ("Le
//! contexte d'une exécution de Node porte uniquement sur sa session
//! principale"). Duration is in **milliseconds** (matching the frontend's
//! `value / 1_000`); Context is a raw peak token count.
//!
//! ### Steering (#792, story #790)
//!
//! The third metric, additive on the same seam: at every level where `context`
//! and `duration` exist, a `steering` distribution (unit: **messages** — the
//! steering messages a human typed into the main session after its launch,
//! read by [`crate::steering`] through the `harness_probes` dispatch) and a
//! `steered { steered, readable }` aggregate (executions with ≥ 1 message over
//! executions whose count is readable). A readable transcript with no message
//! counts `0` and enters `readable` without entering `steered`; a harness with
//! no steering source (`opencode`) is an absence **with a reason**, never `0`;
//! an Infrastructure role is « runtime messages only » (the review batches the
//! daemon pastes to the manager are not human piloting); a subagent is never
//! steered (nobody types in one), so its `steering` is a declared absence with
//! `expected = 0`.
//!
//! ### The « By model » axis (#737, ADR-0065)
//!
//! `by_model` is a second, additive tree over the SAME observations — Modèle →
//! effort → Pipeline → Node — where a session **file**'s peak (and its
//! duration) is attributed to the model of **that file**, a subagent having
//! its own (ADR-0065 §3, "le pic de contexte suit le fichier de session"). The
//! identity is read source-first (ADR-0065 §1): `claude`'s per-message ids,
//! `pi`'s per-message model + thinking level, `copilot`'s usage-point model +
//! effort; a mute source falls back to the startup event's requested model +
//! effort, marked `requested`; a mute source with **no** requested model (and
//! a subagent file — which has no startup event at all) invents nothing and
//! lands in no bucket. A file naming several models counts in **each** bucket
//! (the same "une exécution compte dans chaque bucket où elle a coûté" rule as
//! Cost, one bucket per model here). Node leaves of `by_pipeline` carry their
//! `models` couples — the drill "Node → modèle × effort" — with the same
//! distributions, so the two groupings never disagree about a bucket. Every
//! level pools **raw observations** (never a mean of means) and says its
//! provenance (`observed` / `requested` / `mixed`). Infrastructure roles stay
//! out of the axis: their sessions are residual-by-exclusion, carry no model
//! identity of their own, and the axis compares **Node** executions — the
//! by_pipeline tree keeps them, as today. Duration buckets follow the same
//! per-file attribution: a main session contributes its execution's
//! wall-clock, a subagent file its own transcript time span.
//!
//! ## What counts as an observation
//!
//! Only a **successful** attempt is an observation, mirroring the issue's
//! success-only filter: a `NodeStarted` for `(node_id, iter)` must be answered by
//! a `NodeCompleted`/`NodeAutoCompleted` for the *same* `(node_id, iter)` before
//! any other terminal event for that key. This single rule, applied by walking
//! the raw event log exactly like [`crate::run_cost::compute_run_cost_breakdown`]
//! does for cost, is what naturally satisfies three separate acceptance criteria
//! with no extra bookkeeping:
//!
//! - a loop lap or a restart is a distinct `(node_id, iter)` key → two
//!   observations, not one;
//! - an attempt stopped before its restart (`NodeStopped`/`NodeFailed` on that
//!   key) is simply never matched to a `NodeCompleted` → excluded;
//! - a script node (`node_type == "script"`) never opens a pending attempt at
//!   all → never appears in Performance (out of scope, like a failed attempt).
//!
//! Infrastructure mirrors the same idea one level up: the Pipeline Manager's one
//! observation per Run is `RunStarted` → `RunCompleted` (never `RunFailed`); a
//! Merge resolver's is `MergeResolverStarted` → the next `MergeResolverCompleted`
//! (resolvers carry no `node_id`/`iter`, so pairing is chronological — only one
//! resolver runs at a time per Run).
//!
//! ## Context resolution
//!
//! Context usage is resolved via [`crate::context_peak`] from the same
//! transcript/journal locations [`crate::run_cost`] already reads (Claude:
//! `<projects_root>/<encoded_cwd>/<session_id>.jsonl`; Copilot:
//! `<copilot_root>/<session_id>/events.jsonl`; `pi`'s session by identity inside its
//! cwd folder, #707 — the host-home roots travel as a [`HarnessStores`]).
//!
//! ## Infrastructure subagents — a resolved-by-exclusion session identity
//!
//! Neither `RunStarted`/`RunCompleted` nor `MergeResolverStarted`/`Completed`
//! ever freezes a `session_id` for its role (unlike `NodeStarted`), so an
//! Infrastructure role's own Context/subagents cannot be read off an event
//! field the way a Node's can. This module resolves one instead, by exclusion,
//! narrowly scoped to the one directory each role's turns are known to land
//! in — mirroring [`crate::run_cost`]'s existing "residual Claude cost"
//! technique for the same role (whole-run cost minus every attributed Node's
//! own cost) but working at session granularity, in one directory at a time,
//! so a discovered session can be traced to its own `subagents/` the same way
//! a Node's can:
//!
//! - **Pipeline Manager** orchestrates from the Run's own top-level working
//!   directory ([`crate::worktree_ops::worktree_dir_for_run`]) — the same
//!   directory every non-isolated Node also runs in (an isolated
//!   Node gets its own disjoint [`crate::worktree_ops::sub_worktree_path`], so
//!   it never collides). Every Claude `.jsonl` file in that directory that
//!   isn't a known Node's own `session_id` is a Pipeline Manager candidate.
//! - **Merge resolver** operates on the conflicting Node's own worktree
//!   (`MergeResolverStarted`'s `conflicting_node_id`/`iter` payload fields
//!   resolve the same `sub_worktree_path` that Node ran in) — every Claude
//!   `.jsonl` file there that isn't one of that Node's own known
//!   `session_id`s (across every attempt at that `(node_id, iter)`, including
//!   a stopped-then-restarted one) is a Merge resolver candidate.
//!
//! Exactly one remaining candidate is attributed to the role (its own Context
//! peak, its own `subagents/`, read exactly like a Node's — a subagent still
//! counts even if the role's own attempt around it later failed, matching a
//! Node's own subagent semantics). Zero candidates is an ordinary absence (the
//! role made no attributable Claude calls, or ran under a harness with no
//! session-file source at all — Copilot's session store has no per-directory
//! nesting to exclude from, so it can never resolve one). More than one
//! candidate — or a shared directory containing a non-isolated,
//! non-script Claude Node with **no** recorded `session_id` at all, so it
//! can't be excluded by name — is an ambiguous result, never guessed at by
//! path or timing alone (issue: "Une session historique sans identité fiable
//! n'est jamais attribuée par proximité temporelle ou par chemin").

use crate::sandbox_run::HarnessStores;
use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::distribution::r7_distribution;
use crate::event_log::EventKind;
use crate::AppState;

/// Query string: an ISO-8601 `[from, to)` window — the same cohort rule as
/// `/stats/overview` and `/stats/cost`, but no `bucket`: Performance has no
/// time-series axis (out of scope, "Afficher des graphes temporels").
#[derive(Debug, Deserialize)]
pub(crate) struct PerformanceQuery {
    pub from: String,
    pub to: String,
    /// Explicit user refresh bypasses an otherwise-current memo entry.
    #[serde(default)]
    pub refresh: bool,
}

/// One metric's coverage for one row × one harness. **Never null on the wire**:
/// an all-or-nothing `Option` would drop `expected`/`missing_reasons` for a
/// fully absent metric, leaving the frontend unable to distinguish "never ran"
/// from "no reliable bounds". Only `stats` goes `null`, when `measured == 0`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct StatsDistribution {
    pub stats: Option<crate::distribution::SixStats>,
    pub measured: i64,
    pub expected: i64,
    pub missing_reasons: Vec<String>,
}

/// The steered-executions rate's raw material (#792): `steered` executions with
/// at least one steering message, over `readable` executions whose count could
/// be read. The client renders « n % · steered/readable », or « — » when
/// `readable == 0` — so the numbers travel, never a pre-computed percentage.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub(crate) struct SteeredRate {
    pub steered: u32,
    pub readable: u32,
}

/// Matches the frontend's `StatsHarnessPerformance` — `context`/`duration`/
/// `steering` are never `null` (see [`StatsDistribution`]).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct StatsHarnessPerformance {
    pub harness: String,
    pub context: StatsDistribution,
    pub duration: StatsDistribution,
    /// Steering messages per execution (#792) — additive, unit « messages ».
    pub steering: StatsDistribution,
    /// Share of executions with ≥ 1 steering message (#792).
    pub steered: SteeredRate,
}

/// Matches the frontend's `StatsPerformanceAggregate`.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub(crate) struct PerformanceAggregate {
    pub harnesses: Vec<StatsHarnessPerformance>,
}

/// One node in the Pipeline→Node / Infrastructure→role / subagent-group tree —
/// one recursive shape for all three, matching the frontend's
/// `StatsPerformanceEntity`. A Pipeline's `subagents` is always empty (they are
/// declared on Nodes); a Node's / subagent-group's / infra role's `nodes` is
/// always empty.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub(crate) struct PerformanceEntity {
    pub id: String,
    pub name: String,
    pub harnesses: Vec<StatsHarnessPerformance>,
    pub nodes: Vec<PerformanceEntity>,
    pub subagents: Vec<PerformanceEntity>,
    /// The Node's observations split into model × effort couples (ADR-0065) —
    /// Node leaves only, so the drill-down ends there; omitted (never an empty
    /// array) on the other levels, and on the `by_model` Node rows (the path is
    /// already the drill).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<PerformanceModelEffortPair>,
}

/// One model × effort couple of a Node leaf (ADR-0065): the performance
/// analogue of `stats::StatsModelEffortPair` — the same aggregate shape plus
/// the pair identity and where each half was read from. `effort = None` is the
/// "not set" bucket — never merged with a real effort.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct PerformanceModelEffortPair {
    #[serde(flatten)]
    pub aggregate: PerformanceAggregate,
    pub model: String,
    pub model_provenance: crate::stats::StatsProvenance,
    pub effort: Option<String>,
    pub effort_provenance: Option<crate::stats::StatsProvenance>,
}

/// One effort level under a model, in the Performance « By model » tree: the
/// entity's `id` is the effort string ("" for not set) and its `name` the
/// effort or "not set".
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct PerformanceEffortEntity {
    #[serde(flatten)]
    pub entity: PerformanceEntity,
    pub effort: Option<String>,
    pub provenance: Option<crate::stats::StatsProvenance>,
    pub pipelines: Vec<PerformanceEntity>,
}

/// One model (verbatim id) of the Performance « By model » axis, with its
/// effort tree: Model → Effort → Pipeline → Node. Both harnesses run on the
/// same id are one row, the harness staying a column (ADR-0065 §2).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct StatsModelPerformanceEntity {
    #[serde(flatten)]
    pub entity: PerformanceEntity,
    pub provenance: crate::stats::StatsProvenance,
    pub efforts: Vec<PerformanceEffortEntity>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub(crate) struct StatsPerformance {
    /// Every harness with at least one successful observation in the cohort.
    pub harnesses: Vec<String>,
    /// Every successful main-session Node observation pooled across the WHOLE
    /// cohort — pool the raw observations, never average the averages.
    /// Deliberately excludes Infrastructure roles: a whole Run's or resolver's
    /// wall-clock is not a Node execution. Scoped assumption, unspecified by
    /// the issue.
    pub total: PerformanceAggregate,
    pub by_pipeline: Vec<PerformanceEntity>,
    pub infrastructure: Vec<PerformanceEntity>,
    /// Both Infrastructure roles pooled at the **raw-observation** level, never
    /// a mean of the two roles' means — so the client's master/detail boxplot
    /// never has to fabricate one by averaging averages. Never pools a role's
    /// `subagents` (same rule as [`Self::total`]).
    pub infrastructure_total: PerformanceAggregate,
    /// The « By model » axis (ADR-0065): the SAME successful main-session Node
    /// observations as `total`, bucketed by the model of the session **file**
    /// each observation came from (a subagent file keeps its own bucket). Node
    /// executions only — Infrastructure roles carry no model identity and stay
    /// on `by_pipeline` (module doc, « The « By model » axis »).
    pub by_model: Vec<StatsModelPerformanceEntity>,
}

/// Why the whole request failed, distinct from a per-observation absence
/// (issue: "Une source entière illisible produit une erreur visible... Une
/// session isolée sans télémétrie exploitable produit une absence locale").
pub(crate) enum PerformanceError {
    /// A harness's entire transcript/journal root exists but could not be
    /// listed (permissions, corrupted mount, …) — as opposed to `NotFound`,
    /// which just means "no sessions yet" and is not an error.
    SourceUnreadable(String),
    Db(String),
}

impl IntoResponse for PerformanceError {
    fn into_response(self) -> Response {
        let message = match self {
            PerformanceError::SourceUnreadable(msg) => msg,
            PerformanceError::Db(e) => format!("stats performance failed: {e}"),
        };
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": message })),
        )
            .into_response()
    }
}

/// `GET /stats/performance` — Class B heavy read (fans over the harness
/// transcript corpora). The client owns the "load on demand / Refresh" half;
/// this handler never runs on a timer.
pub(crate) async fn stats_performance(
    State(state): State<Arc<AppState>>,
    Query(q): Query<PerformanceQuery>,
) -> Response {
    match compute_performance(&state, &q.from, &q.to, q.refresh).await {
        Ok(payload) => Json(payload).into_response(),
        Err(error) => error.into_response(),
    }
}

#[derive(Debug, Default)]
struct MetricAcc {
    values: Vec<f64>,
    expected: i64,
    absence_reasons: BTreeSet<String>,
}

impl MetricAcc {
    fn observe(&mut self, value: Option<f64>, absence_reason: Option<&str>) {
        self.expected += 1;
        match value {
            Some(v) => self.values.push(v),
            None => {
                if let Some(reason) = absence_reason {
                    self.absence_reasons.insert(reason.to_string());
                }
            }
        }
    }

    /// Pool another accumulator's RAW observations into this one — never a mean
    /// of means (the whole module pools at the raw level).
    fn fold(&mut self, other: &MetricAcc) {
        self.values.extend_from_slice(&other.values);
        self.expected += other.expected;
        self.absence_reasons
            .extend(other.absence_reasons.iter().cloned());
    }

    /// Name why this metric is structurally absent for the row, without
    /// counting an expected observation: a subagent's Steering (nobody types in
    /// one) is « — <reason> », not a `0 of n` coverage gap.
    fn declare_absent(&mut self, reason: &str) {
        self.absence_reasons.insert(reason.to_string());
    }

    /// The steered-executions rate this accumulator's raw values carry (#792):
    /// every readable count is one `readable`, every count ≥ 1 one `steered`.
    fn steered(&self) -> SteeredRate {
        SteeredRate {
            steered: self.values.iter().filter(|v| **v >= 1.0).count() as u32,
            readable: self.values.len() as u32,
        }
    }

    fn finish(self) -> StatsDistribution {
        StatsDistribution {
            stats: r7_distribution(&self.values),
            measured: self.values.len() as i64,
            expected: self.expected,
            missing_reasons: self.absence_reasons.into_iter().collect(),
        }
    }
}

/// One metric's reading for one observation: the value, or the reason it is
/// absent. `Steering` for a subagent file is [`Reading::NotApplicable`] — a
/// declared absence that counts no expected observation.
#[derive(Debug, Clone, Copy)]
enum Reading<'a> {
    Observed(Option<f64>, Option<&'a str>),
    NotApplicable(&'a str),
}

impl MetricAcc {
    fn record(&mut self, reading: Reading<'_>) {
        match reading {
            Reading::Observed(value, reason) => self.observe(value, reason),
            Reading::NotApplicable(reason) => self.declare_absent(reason),
        }
    }
}

/// Why a subagent file carries no Steering (#792): no human types in one.
const SUBAGENT_STEERING_REASON: &str = "subagents are never steered";
/// Why an Infrastructure role carries no Steering (#792): the daemon's own
/// review batches reach the manager, no human pilots it.
const INFRA_STEERING_REASON: &str = "runtime messages only";

#[derive(Debug, Default)]
struct HarnessAcc {
    context: MetricAcc,
    duration: MetricAcc,
    steering: MetricAcc,
}

impl HarnessAcc {
    fn finish(self, harness: String) -> StatsHarnessPerformance {
        StatsHarnessPerformance {
            harness,
            context: self.context.finish(),
            duration: self.duration.finish(),
            steered: self.steering.steered(),
            steering: self.steering.finish(),
        }
    }
}

fn finish_by_harness(acc: BTreeMap<String, HarnessAcc>) -> Vec<StatsHarnessPerformance> {
    acc.into_iter().map(|(h, a)| a.finish(h)).collect()
}

#[derive(Debug, Default)]
struct SubagentAcc {
    by_harness: BTreeMap<String, HarnessAcc>,
}

#[derive(Debug, Default)]
struct NodeAcc {
    name: String,
    by_harness: BTreeMap<String, HarnessAcc>,
    subagents: BTreeMap<String, SubagentAcc>,
    /// The Node's model × effort couples (ADR-0065): the same observations as
    /// `by_harness`, bucketed by the model of the session **file** each came
    /// from — the drill "Node → modèle × effort" of `by_pipeline` (#737).
    pairs: BTreeMap<ModelEffortKey, ModelPairAcc>,
}

fn finish_subagents(subagents: BTreeMap<String, SubagentAcc>) -> Vec<PerformanceEntity> {
    subagents
        .into_iter()
        .map(|(group, acc)| PerformanceEntity {
            id: group.clone(),
            name: group,
            harnesses: finish_by_harness(acc.by_harness),
            nodes: Vec::new(),
            subagents: Vec::new(),
            models: Vec::new(),
        })
        .collect()
}

fn finish_node(id: String, acc: NodeAcc) -> PerformanceEntity {
    PerformanceEntity {
        id,
        name: acc.name,
        harnesses: finish_by_harness(acc.by_harness),
        nodes: Vec::new(),
        subagents: finish_subagents(acc.subagents),
        models: wire_model_pairs(acc.pairs),
    }
}

/// Like [`finish_node`], but `id`/`name` are the role's fixed identity: an
/// infra `NodeAcc` never sets `acc.name`.
fn finish_infra_role(id: &str, name: &str, acc: NodeAcc) -> PerformanceEntity {
    PerformanceEntity {
        id: id.to_string(),
        name: name.to_string(),
        harnesses: finish_by_harness(acc.by_harness),
        nodes: Vec::new(),
        subagents: finish_subagents(acc.subagents),
        // Infrastructure carries no model identity (module doc): an empty
        // `models` is omitted on the wire, never an empty array.
        models: Vec::new(),
    }
}

#[derive(Debug, Default)]
struct PipelineAcc {
    name: String,
    by_harness: BTreeMap<String, HarnessAcc>,
    nodes: BTreeMap<String, NodeAcc>,
}

fn finish_pipeline(id: String, acc: PipelineAcc) -> PerformanceEntity {
    PerformanceEntity {
        id,
        name: acc.name,
        harnesses: finish_by_harness(acc.by_harness),
        nodes: acc
            .nodes
            .into_iter()
            .map(|(id, node)| finish_node(id, node))
            .collect(),
        subagents: Vec::new(),
        models: Vec::new(),
    }
}

// --- « By model » axis (#737, ADR-0065) ---------------------------------------

/// A model × effort bucket key: the **verbatim** model id (ADR-0065 §2, no
/// normalisation, no alias table) and the effort it ran with — `None` is the
/// "not set" bucket, never merged with a real effort.
type ModelEffortKey = (String, Option<String>);

/// The model × effort a session file's observation is attributed to
/// (ADR-0065 §1): observed from the harness's source when it speaks, else the
/// startup event's requested identity. Provenance is decided per observation —
/// a bucket whose observations mix both says « mixed ».
struct ModelIdentity {
    model: String,
    effort: Option<String>,
    model_observed: bool,
    effort_observed: Option<bool>,
}

/// One model × effort bucket, accumulated over the session-file observations
/// that landed in it. The observed/requested counters decide the wire's
/// provenance, including the « mixed » case.
#[derive(Debug, Default)]
struct ModelPairAcc {
    by_harness: BTreeMap<String, HarnessAcc>,
    model_observed: i64,
    model_requested: i64,
    effort_observed: i64,
    effort_requested: i64,
}

impl ModelPairAcc {
    /// Observe one session file's context peak + duration + steering into this
    /// bucket.
    #[allow(clippy::too_many_arguments)]
    fn observe(
        &mut self,
        harness: &str,
        context: (Option<f64>, Option<&str>),
        duration: (Option<f64>, Option<&str>),
        steering: Reading<'_>,
        identity: &ModelIdentity,
    ) {
        let acc = self.by_harness.entry(harness.to_string()).or_default();
        acc.context.observe(context.0, context.1);
        acc.duration.observe(duration.0, duration.1);
        acc.steering.record(steering);
        if identity.model_observed {
            self.model_observed += 1;
        } else {
            self.model_requested += 1;
        }
        match identity.effort_observed {
            Some(true) => self.effort_observed += 1,
            Some(false) => self.effort_requested += 1,
            None => {}
        }
    }
}

/// One (pipeline, Node) slot of the by_model tree: the Node's rows under the
/// model buckets it provably ran on.
#[derive(Debug, Default)]
struct ModelAxisNodeAcc {
    name: String,
    pairs: BTreeMap<ModelEffortKey, ModelPairAcc>,
}

/// One pipeline level of the by_model tree.
#[derive(Debug, Default)]
struct ModelAxisPipelineAcc {
    name: String,
    pairs: BTreeMap<ModelEffortKey, ModelPairAcc>,
    nodes: BTreeMap<String, ModelAxisNodeAcc>,
}

/// One effort level under a model. `effort: None` is the "not set" bucket.
#[derive(Debug, Default)]
struct ModelAxisEffortAcc {
    pairs: BTreeMap<ModelEffortKey, ModelPairAcc>,
    pipelines: BTreeMap<String, ModelAxisPipelineAcc>,
}

/// One model (verbatim id) of the by_model axis.
#[derive(Debug, Default)]
struct ModelAxisModelAcc {
    pairs: BTreeMap<ModelEffortKey, ModelPairAcc>,
    efforts: BTreeMap<Option<String>, ModelAxisEffortAcc>,
}

/// Attribute one session file's observation to every model × effort bucket its
/// identity names (ADR-0065 §3: the peak follows the file; a file naming two
/// models counts in both). Walks the by_model tree — Node, Pipeline, Effort
/// and Model levels each keep their own raw pool — plus the Node's own couples
/// on the `by_pipeline` side.
#[allow(clippy::too_many_arguments)]
fn record_model_observation(
    model_axis: &mut BTreeMap<String, ModelAxisModelAcc>,
    node_pairs: &mut BTreeMap<ModelEffortKey, ModelPairAcc>,
    pipeline_id: &str,
    pipeline_name: &str,
    node_id: &str,
    node_name: &str,
    harness: &str,
    identity: &ModelIdentity,
    context: (Option<f64>, Option<&str>),
    duration: (Option<f64>, Option<&str>),
    steering: Reading<'_>,
) {
    let key = (identity.model.clone(), identity.effort.clone());
    node_pairs
        .entry(key.clone())
        .or_default()
        .observe(harness, context, duration, steering, identity);

    let model_acc = model_axis.entry(identity.model.clone()).or_default();
    let effort_acc = model_acc
        .efforts
        .entry(identity.effort.clone())
        .or_default();
    let pipeline_acc = effort_acc
        .pipelines
        .entry(pipeline_id.to_string())
        .or_default();
    pipeline_acc.name = pipeline_name.to_string();
    let node_acc = pipeline_acc.nodes.entry(node_id.to_string()).or_default();
    node_acc.name = node_name.to_string();
    for acc in [
        model_acc.pairs.entry(key.clone()).or_default(),
        effort_acc.pairs.entry(key.clone()).or_default(),
        pipeline_acc.pairs.entry(key.clone()).or_default(),
        node_acc.pairs.entry(key).or_default(),
    ] {
        acc.observe(harness, context, duration, steering, identity);
    }
}

/// Pool a set of model × effort buckets back into one raw-observation
/// aggregate — the level rows of the by_model tree, never a mean of means.
fn pool_pairs(pairs: &BTreeMap<ModelEffortKey, ModelPairAcc>) -> PerformanceAggregate {
    let mut by_harness: BTreeMap<String, HarnessAcc> = BTreeMap::new();
    for pair in pairs.values() {
        for (harness, acc) in &pair.by_harness {
            let target = by_harness.entry(harness.clone()).or_default();
            target.context.fold(&acc.context);
            target.duration.fold(&acc.duration);
            target.steering.fold(&acc.steering);
        }
    }
    PerformanceAggregate {
        harnesses: finish_by_harness(by_harness),
    }
}

/// Observed/requested totals over a set of buckets — the provenance a level
/// row reports.
fn provenance_totals(pairs: &BTreeMap<ModelEffortKey, ModelPairAcc>) -> (i64, i64, i64, i64) {
    pairs.values().fold((0, 0, 0, 0), |(mo, mr, eo, er), acc| {
        (
            mo + acc.model_observed,
            mr + acc.model_requested,
            eo + acc.effort_observed,
            er + acc.effort_requested,
        )
    })
}

fn wire_model_pairs(
    pairs: BTreeMap<ModelEffortKey, ModelPairAcc>,
) -> Vec<PerformanceModelEffortPair> {
    pairs
        .into_iter()
        .map(|((model, effort), acc)| PerformanceModelEffortPair {
            model_provenance: crate::stats::provenance(acc.model_observed, acc.model_requested),
            effort_provenance: effort
                .as_ref()
                .map(|_| crate::stats::provenance(acc.effort_observed, acc.effort_requested)),
            aggregate: PerformanceAggregate {
                harnesses: finish_by_harness(acc.by_harness),
            },
            model,
            effort,
        })
        .collect()
}

/// Wire the whole Performance « By model » tree: Model → Effort → Pipeline →
/// Node. Every level's aggregate pools its own raw observations; the Node rows
/// carry no `models` — the path is already the drill.
fn wire_model_axis(
    models: BTreeMap<String, ModelAxisModelAcc>,
) -> Vec<StatsModelPerformanceEntity> {
    models
        .into_iter()
        .map(|(model, model_acc)| {
            let (model_observed, model_requested, _, _) = provenance_totals(&model_acc.pairs);
            let efforts = model_acc
                .efforts
                .into_iter()
                .map(|(effort, effort_acc)| {
                    let (_, _, effort_observed, effort_requested) =
                        provenance_totals(&effort_acc.pairs);
                    let effort_provenance = effort
                        .as_ref()
                        .map(|_| crate::stats::provenance(effort_observed, effort_requested));
                    let pipelines = effort_acc
                        .pipelines
                        .into_iter()
                        .map(|(id, pipeline_acc)| {
                            let nodes = pipeline_acc
                                .nodes
                                .into_iter()
                                .map(|(id, node_acc)| PerformanceEntity {
                                    id,
                                    name: node_acc.name,
                                    harnesses: pool_pairs(&node_acc.pairs).harnesses,
                                    nodes: Vec::new(),
                                    subagents: Vec::new(),
                                    models: Vec::new(),
                                })
                                .collect();
                            PerformanceEntity {
                                id,
                                name: pipeline_acc.name,
                                harnesses: pool_pairs(&pipeline_acc.pairs).harnesses,
                                nodes,
                                subagents: Vec::new(),
                                models: Vec::new(),
                            }
                        })
                        .collect();
                    PerformanceEffortEntity {
                        entity: PerformanceEntity {
                            id: effort.clone().unwrap_or_default(),
                            name: effort.clone().unwrap_or_else(|| "not set".to_string()),
                            harnesses: pool_pairs(&effort_acc.pairs).harnesses,
                            nodes: Vec::new(),
                            subagents: Vec::new(),
                            models: Vec::new(),
                        },
                        effort,
                        provenance: effort_provenance,
                        pipelines,
                    }
                })
                .collect();
            StatsModelPerformanceEntity {
                entity: PerformanceEntity {
                    id: model.clone(),
                    name: model,
                    harnesses: pool_pairs(&model_acc.pairs).harnesses,
                    nodes: Vec::new(),
                    subagents: Vec::new(),
                    models: Vec::new(),
                },
                provenance: crate::stats::provenance(model_observed, model_requested),
                efforts,
            }
        })
        .collect()
}

// --- Node identity/type resolution ---------------------------------------------

fn node_defs_from_payload(payload: &Value) -> BTreeMap<String, (String, String, bool)> {
    let mut defs = BTreeMap::new();
    if let Some(list) = payload.get("node_defs").and_then(|v| v.as_array()) {
        for def in list {
            let Some(id) = def.get("id").and_then(|v| v.as_str()) else {
                continue;
            };
            let name = def
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or(id)
                .to_string();
            let node_type = def
                .get("node_type")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            // #653/ADR-0060: where the node works. The snapshot states it for an
            // `agent`/`script`; the type's default stands in for a pre-#653 Run.
            let isolated = def
                .get("isolated_worktree")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(matches!(node_type.as_str(), "agent" | "merge"));
            defs.insert(id.to_string(), (name, node_type, isolated));
        }
    }
    defs
}

fn looks_like_opaque_id(stem: &str) -> bool {
    let is_uuid = stem.len() == 36
        && stem.as_bytes().iter().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => *b == b'-',
            _ => b.is_ascii_hexdigit(),
        });
    let is_long_hex = stem.len() >= 16 && stem.chars().all(|c| c.is_ascii_hexdigit());
    is_uuid || is_long_hex
}

/// The declared group a discovered subagent transcript file falls under: its
/// filename, unless that reads as an opaque generated id — then the explicit
/// "Unidentified subagent" bucket, so it is never silently dropped.
///
/// **Scoped assumption**: no Claude Code subagent naming convention is
/// confirmed anywhere in this repo, so filename is the only signal short of
/// inventing an unconfirmed in-transcript field. This is the seam to extend if
/// subagent files are later found to embed a `subagent_type` key.
fn claude_subagent_group(file_stem: &str) -> String {
    if file_stem.is_empty() || looks_like_opaque_id(file_stem) {
        "Unidentified subagent".to_string()
    } else {
        file_stem.to_string()
    }
}

/// Confirm a harness's whole transcript source is listable, or name it in a
/// visible error. `NotFound` is not an error — it means "no session written
/// yet". Checked once per distinct root, never per session: a single unreadable
/// session must degrade to a local absence, not fail the whole request.
fn ensure_source_readable(root: &Path, harness_label: &str) -> Result<(), PerformanceError> {
    if !root.exists() {
        return Ok(());
    }
    match std::fs::read_dir(root) {
        Ok(_) => Ok(()),
        Err(error) => Err(PerformanceError::SourceUnreadable(format!(
            "{harness_label} transcript source at {} is unreadable: {error}",
            root.display()
        ))),
    }
}

fn parse_ts(ts: &str) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    chrono::DateTime::parse_from_rfc3339(ts).ok()
}

fn duration_millis(started_at: &str, completed_at: &str) -> Option<f64> {
    let start = parse_ts(started_at)?;
    let end = parse_ts(completed_at)?;
    Some((end - start).num_milliseconds() as f64)
}

/// Pick the store root a harness's transcript is read from: the per-Run,
/// sandbox-aware Claude root, or one of the host-home stores (`copilot`'s journal
/// store, `pi`'s sessions store, #707). Picks between independently computed
/// roots, not between behaviours — behaviour dispatch goes through
/// `harness_probes` (ADR-0051); the name match lives once, in
/// [`HarnessStores::root_for`].
fn source_root<'a>(harness: &str, claude_root: &'a Path, stores: &'a HarnessStores) -> &'a Path {
    stores.root_for(harness, claude_root)
}

/// One main session's observation: `(context peak, absence reason, file text)`.
type MainObservation = (Option<f64>, Option<&'static str>, Option<String>);

/// One main session's Steering reading (#792): `Some(n)` typed human turns after
/// the launch prompt, or the named reason the count is unavailable — a harness
/// with no steering source (`opencode`: its session is not resolvable by
/// identity), no session identity, no transcript, or a transcript with no
/// readable human turn. Never `0` for an unreadable one. Reads the text the
/// context peak already loaded when there is one, so the file is read once.
fn main_session_steering(
    harness: &str,
    root: &Path,
    working_dir: &Path,
    session_id: Option<&str>,
    already_read: Option<&str>,
) -> (Option<f64>, Option<String>) {
    if !crate::harness_probes::can_count_steering(harness) {
        return (None, Some(format!("session not resolvable on {harness}")));
    }
    let Some(session_id) = session_id.filter(|s| !s.is_empty()) else {
        return (None, Some("no session identity".to_string()));
    };
    let text = match already_read {
        Some(text) => Some(text.to_string()),
        None => session_file_text(harness, root, working_dir, Some(session_id)),
    };
    let Some(text) = text else {
        return (None, Some("no attributable transcript".to_string()));
    };
    match crate::harness_probes::steering_count(harness, &text) {
        Some(count) => (Some(f64::from(count)), None),
        None => (
            None,
            Some("no readable human turn in transcript".to_string()),
        ),
    }
}

/// One harness's context peak for one main session, or the absence reason,
/// plus the session file's text — the same read feeds the peak and the
/// observed model identity (#737), so the file is read once. The root
/// readability check sits *after* the `can_measure_context` gate, so a
/// harness with no context source (e.g. `opencode`) never triggers a
/// Claude/Copilot root check it does not need.
fn main_session_observation(
    harness: &str,
    root: &Path,
    working_dir: &Path,
    session_id: Option<&str>,
    seen_roots: &mut HashSet<PathBuf>,
) -> Result<MainObservation, PerformanceError> {
    if !crate::harness_probes::can_measure_context(harness) {
        return Ok((None, Some("harness has no context-usage source"), None));
    }
    check_root_once(root, &harness_display_label(harness), seen_roots)?;
    let Some(session_id) = session_id.filter(|s| !s.is_empty()) else {
        return Ok((None, Some("no session identity"), None));
    };
    let Some(path) =
        crate::harness_probes::resolve_transcript(harness, root, working_dir, Some(session_id))
    else {
        return Ok((None, Some("no attributable transcript"), None));
    };
    Ok(match std::fs::read_to_string(&path) {
        Ok(text) => match crate::harness_probes::context_peak(harness, &text) {
            Some(peak) => (Some(peak as f64), None, Some(text)),
            None => (
                None,
                Some("no readable context usage in transcript"),
                Some(text),
            ),
        },
        Err(_) => (None, Some("no attributable transcript"), None),
    })
}

/// One session file's transcript text, by the harness's own resolution — the
/// identity-only read for a harness whose context gate answered before the
/// file was read (`copilot`'s journal is still worth reading for its observed
/// model, #737).
fn session_file_text(
    harness: &str,
    root: &Path,
    working_dir: &Path,
    session_id: Option<&str>,
) -> Option<String> {
    crate::harness_probes::resolve_transcript(harness, root, working_dir, session_id)
        .and_then(|path| std::fs::read_to_string(path).ok())
}

/// The model × effort identities one MAIN session's observation is attributed
/// to (ADR-0065 §1): the observed identities the harness's source reports when
/// it speaks — `effort_observed` only where the source carries the effort
/// (`pi`, `copilot`); `claude`'s effort stays the requested one. A mute source
/// (or no readable file) falls back to the startup event's requested identity,
/// marked `requested`; a mute source with **no** requested model invents
/// nothing and yields no bucket.
fn main_session_identities(
    harness: &str,
    root: &Path,
    working_dir: &Path,
    session_id: Option<&str>,
    requested_model: Option<&str>,
    requested_effort: Option<&str>,
    already_read: Option<&str>,
) -> Vec<ModelIdentity> {
    let source = crate::harness_probes::observed_identity_source(harness);
    let mut observed = Vec::new();
    if source.is_some() {
        let text = match already_read {
            Some(text) => Some(text.to_string()),
            None => session_file_text(harness, root, working_dir, session_id),
        };
        if let Some(text) = text {
            observed = crate::harness_probes::observed_identities(harness, &text)
                .into_iter()
                .map(|identity| {
                    // The source carries the effort (`pi`, `copilot`) — an
                    // absent one is « not set », never refilled from the
                    // startup event (source-first, ADR-0065 §1). `claude`'s
                    // source never writes one: the requested effort stands.
                    let (effort, effort_observed) = match source {
                        Some(crate::harness_probes::ObservedIdentitySource::ModelAndEffort) => {
                            let observed_effort = identity.effort;
                            let observed = observed_effort.clone();
                            (observed_effort, Some(observed.is_some()))
                        }
                        _ => (
                            requested_effort.map(str::to_string),
                            requested_effort.map(|_| false),
                        ),
                    };
                    ModelIdentity {
                        model: identity.model,
                        effort,
                        model_observed: true,
                        effort_observed,
                    }
                })
                .collect();
        }
    }
    if !observed.is_empty() {
        return observed;
    }
    requested_model
        .map(|model| {
            vec![ModelIdentity {
                model: model.to_string(),
                effort: requested_effort.map(str::to_string),
                model_observed: false,
                effort_observed: requested_effort.map(|_| false),
            }]
        })
        .unwrap_or_default()
}

/// The subagent transcript files discovered for one main session, grouped by
/// declared label. A harness with no nested-subagent convention (every one but
/// `claude` today) answers an empty `Vec` from the `harness_probes` dispatch
/// itself — never from an `if harness == ..` written here (ADR-0051).
fn subagent_groups(
    harness: &str,
    root: &Path,
    working_dir: &Path,
    session_id: Option<&str>,
) -> Vec<(String, String)> {
    let Some(session_id) = session_id.filter(|s| !s.is_empty()) else {
        return Vec::new();
    };
    crate::harness_probes::subagent_transcripts(harness, root, working_dir, session_id)
        .into_iter()
        .map(|(stem, text)| (claude_subagent_group(&stem), text))
        .collect()
}

struct PendingNode {
    started_at: String,
    harness: String,
    session_id: Option<String>,
    /// Where the NodeRun works (#653) — its own sub-worktree, or the Run's.
    isolated: bool,
    /// The requested model × effort, frozen in the startup event (ADR-0046) —
    /// the fallback when the source is mute (ADR-0065 §1).
    requested_model: Option<String>,
    requested_effort: Option<String>,
}

/// A `MergeResolverStarted` awaiting its pairing `MergeResolverCompleted`.
/// `conflicting_node_id`/`iter` resolve the working directory an attributed
/// session is looked for in (module doc, "Infrastructure subagents").
struct MergeResolverPending {
    started_at: String,
    conflicting_node_id: Option<String>,
    iter: Option<i64>,
}

/// Fold discovered subagent transcripts into their declared groups (the
/// `by_pipeline` tree) and into the « By model » axis. Shared by a Node's
/// subagents and an Infrastructure role's.
///
/// The duration span alone bypasses the `harness_probes` dispatch: no capability
/// marker exists yet for "start/end bounds from a transcript", so
/// [`crate::context_peak::claude_transcript_time_span`] stays a direct call —
/// the seam to extend when a second harness grows subagent transcripts.
///
/// `node_pairs` / `model_axis` are `Some` only for a Node's own subagents —
/// the `by_pipeline` drill's couples and the « By model » axis are Node
/// executions; an Infrastructure role's residual subagents stay off both
/// (module doc, « The « By model » axis »).
#[allow(clippy::too_many_arguments)]
fn fold_subagents(
    subagents: &mut BTreeMap<String, SubagentAcc>,
    mut node_pairs: Option<&mut BTreeMap<ModelEffortKey, ModelPairAcc>>,
    mut model_axis: Option<&mut BTreeMap<String, ModelAxisModelAcc>>,
    pipeline_id: &str,
    pipeline_name: &str,
    node_id: &str,
    node_name: &str,
    files: Vec<(String, String)>,
    harness: &str,
) {
    for (group, text) in files {
        let sub = subagents.entry(group).or_default();
        let sub_acc = sub.by_harness.entry(harness.to_string()).or_default();
        let peak = crate::harness_probes::context_peak(harness, &text);
        sub_acc.context.observe(
            peak.map(|p| p as f64),
            (peak.is_none()).then_some("no readable context usage in transcript"),
        );
        let span = crate::context_peak::claude_transcript_time_span(&text);
        let sub_duration = span.and_then(|(a, b)| duration_millis(&a, &b));
        sub_acc.duration.observe(
            sub_duration,
            sub_duration
                .is_none()
                .then_some("no reliable start/end bounds in subagent transcript"),
        );
        // #792: nobody types in a subagent — a declared absence, no expected
        // observation, so a Node's steered rate is never diluted by its
        // subagents.
        sub_acc.steering.declare_absent(SUBAGENT_STEERING_REASON);

        // ADR-0065 §3: the file's peak (and span) follows the file's own model —
        // a subagent on another model lands in its own bucket. A subagent has no
        // startup event, so ONLY the observed identities count: a mute file
        // invents nothing (never a "default of X" bucket).
        if let (Some(node_pairs), Some(model_axis)) =
            (node_pairs.as_deref_mut(), model_axis.as_deref_mut())
        {
            let source = crate::harness_probes::observed_identity_source(harness);
            if source.is_some() {
                for identity in crate::harness_probes::observed_identities(harness, &text) {
                    // Same source-first rule as a main session (ADR-0065 §1):
                    // `pi`/`copilot` subagents would carry their own observed
                    // effort; a mute effort is « not set », never requested.
                    let (effort, effort_observed) = match source {
                        Some(crate::harness_probes::ObservedIdentitySource::ModelAndEffort) => {
                            let observed_effort = identity.effort;
                            let observed = observed_effort.clone();
                            (observed_effort, Some(observed.is_some()))
                        }
                        _ => (None, None),
                    };
                    let model_identity = ModelIdentity {
                        model: identity.model,
                        effort,
                        model_observed: true,
                        effort_observed,
                    };
                    record_model_observation(
                        model_axis,
                        node_pairs,
                        pipeline_id,
                        pipeline_name,
                        node_id,
                        node_name,
                        harness,
                        &model_identity,
                        (
                            peak.map(|p| p as f64),
                            (peak.is_none()).then_some("no readable context usage in transcript"),
                        ),
                        (
                            sub_duration,
                            sub_duration
                                .is_none()
                                .then_some("no reliable start/end bounds in subagent transcript"),
                        ),
                        Reading::NotApplicable(SUBAGENT_STEERING_REASON),
                    );
                }
            }
        }
    }
}

/// Record one successful Node execution into its Node, its Pipeline and the
/// cohort total alike, plus the « By model » axis (#737): the main session's
/// peak + duration goes to the model × effort bucket its own session file
/// names (observed from the source, the startup event's requested identity in
/// fallback). Discovered subagent transcripts go into the Node's own subagent
/// groups and NEVER into any of the three harness accumulators.
#[allow(clippy::too_many_arguments)]
fn record_node_success(
    node_acc: &mut NodeAcc,
    pipeline_by_harness: &mut BTreeMap<String, HarnessAcc>,
    total_by_harness: &mut BTreeMap<String, HarnessAcc>,
    model_axis: &mut BTreeMap<String, ModelAxisModelAcc>,
    pipeline_id: &str,
    pipeline_name: &str,
    node_id: &str,
    node_name: &str,
    claude_root: &Path,
    stores: &HarnessStores,
    working_dir: &Path,
    pending: &PendingNode,
    completed_at: &str,
    seen_roots: &mut HashSet<PathBuf>,
) -> Result<(), PerformanceError> {
    let harness = pending.harness.clone();
    let root = source_root(&harness, claude_root, stores);
    let (context, reason, main_text) = main_session_observation(
        &harness,
        root,
        working_dir,
        pending.session_id.as_deref(),
        seen_roots,
    )?;
    let duration = duration_millis(&pending.started_at, completed_at);
    let duration_reason = duration.is_none().then_some("unparseable timestamp");
    let (steering, steering_reason) = main_session_steering(
        &harness,
        root,
        working_dir,
        pending.session_id.as_deref(),
        main_text.as_deref(),
    );
    let steering_reading = Reading::Observed(steering, steering_reason.as_deref());

    for acc in [
        node_acc.by_harness.entry(harness.clone()).or_default(),
        pipeline_by_harness.entry(harness.clone()).or_default(),
        total_by_harness.entry(harness.clone()).or_default(),
    ] {
        acc.context.observe(context, reason);
        acc.duration.observe(duration, duration_reason);
        acc.steering.record(steering_reading);
    }

    // ADR-0065 §1: the main session's observation is attributed to the model(s)
    // its own session file names — observed first, the requested identity in
    // fallback, nothing invented when neither speaks.
    for identity in main_session_identities(
        &harness,
        root,
        working_dir,
        pending.session_id.as_deref(),
        pending.requested_model.as_deref(),
        pending.requested_effort.as_deref(),
        main_text.as_deref(),
    ) {
        record_model_observation(
            model_axis,
            &mut node_acc.pairs,
            pipeline_id,
            pipeline_name,
            node_id,
            node_name,
            &harness,
            &identity,
            (context, reason),
            (duration, duration_reason),
            steering_reading,
        );
    }

    let files = subagent_groups(&harness, root, working_dir, pending.session_id.as_deref());
    fold_subagents(
        &mut node_acc.subagents,
        Some(&mut node_acc.pairs),
        Some(model_axis),
        pipeline_id,
        pipeline_name,
        node_id,
        node_name,
        files,
        &harness,
    );

    Ok(())
}

/// Display prose, not a capability decision — so no `harness_probes` dispatch.
fn harness_display_label(harness: &str) -> String {
    let mut chars = harness.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn check_root_once(
    root: &Path,
    label: &str,
    seen: &mut HashSet<PathBuf>,
) -> Result<(), PerformanceError> {
    if seen.insert(root.to_path_buf()) {
        ensure_source_readable(root, label)?;
    }
    Ok(())
}

/// The exclusion list an Infrastructure role's own session is resolved against.
/// Populated for EVERY `NodeStarted`, whatever its type or outcome: even a
/// failed attempt's session file still occupies the shared directory.
#[derive(Debug, Clone)]
struct NodeStartRecord {
    node_type: String,
    /// Where the NodeRun works (#653) — its own sub-worktree, or the Run's.
    isolated: bool,
    harness: String,
    session_id: Option<String>,
}

/// One directory's Claude sessions resolved down to the ONE not already known
/// to belong to a Node — or why that could not be done. An ambiguous directory
/// is never attributed by path alone.
enum InfraSession {
    /// Exactly one unattributed `.jsonl` file: `(project_dir, session_id)`.
    One(PathBuf, String),
    /// The directory doesn't exist, or every file in it is already a known
    /// Node's own session — an ordinary, non-ambiguous absence.
    None,
    /// More than one unattributed file, or a Claude Node shares this directory
    /// with no recorded `session_id` at all (so it can't be excluded by name)
    /// — cannot be safely attributed to anything.
    Ambiguous,
}

fn resolve_infra_session(
    claude_root: &Path,
    working_dir: &Path,
    excluded_session_ids: &HashSet<String>,
    directory_ambiguous: bool,
) -> InfraSession {
    if directory_ambiguous {
        return InfraSession::Ambiguous;
    }
    let project_dir = claude_root.join(crate::run_cost::cc_project_dirname(working_dir));
    let Ok(entries) = std::fs::read_dir(&project_dir) else {
        return InfraSession::None;
    };
    let mut candidates = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if excluded_session_ids.contains(stem) {
            continue;
        }
        candidates.push(stem.to_string());
    }
    match candidates.len() {
        0 => InfraSession::None,
        1 => InfraSession::One(project_dir, candidates.remove(0)),
        _ => InfraSession::Ambiguous,
    }
}

/// Context peak + subagent transcripts for one resolved Infrastructure-role
/// session, or the motivated absence. A subagent still counts even if the
/// role's own attempt around it later failed (same as a Node's subagents).
/// `harness` is threaded through though only `claude` reaches here today — the
/// resolve-by-exclusion scan above is structurally Claude-only.
fn infra_claude_observation(
    harness: &str,
    claude_root: &Path,
    working_dir: &Path,
    excluded_session_ids: &HashSet<String>,
    directory_ambiguous: bool,
) -> (Option<f64>, Option<&'static str>, Vec<(String, String)>) {
    match resolve_infra_session(
        claude_root,
        working_dir,
        excluded_session_ids,
        directory_ambiguous,
    ) {
        InfraSession::Ambiguous => (
            None,
            Some("ambiguous session identity in a directory shared with a Node"),
            Vec::new(),
        ),
        InfraSession::None => (
            None,
            Some("no attributable session for this infrastructure role"),
            Vec::new(),
        ),
        InfraSession::One(project_dir, session_id) => {
            let path = project_dir.join(format!("{session_id}.jsonl"));
            match std::fs::read_to_string(&path) {
                Ok(text) => {
                    let peak = crate::harness_probes::context_peak(harness, &text);
                    let subs =
                        subagent_groups(harness, claude_root, working_dir, Some(&session_id));
                    (
                        peak.map(|p| p as f64),
                        peak.is_none()
                            .then_some("no readable context usage in transcript"),
                        subs,
                    )
                }
                Err(_) => (None, Some("no attributable transcript"), Vec::new()),
            }
        }
    }
}

/// Fold one Infrastructure-role occurrence into its own role and the raw infra
/// total alike. Shared by the Pipeline Manager and Merge resolver arms, which
/// differ only in which directory and exclusion set apply.
#[allow(clippy::too_many_arguments)]
fn record_infra_success(
    acc: &mut NodeAcc,
    infra_total_by_harness: &mut BTreeMap<String, HarnessAcc>,
    run_harness: &str,
    claude_root: &Path,
    working_dir: Option<&Path>,
    excluded_session_ids: &HashSet<String>,
    directory_ambiguous: bool,
    duration: Option<f64>,
    seen_roots: &mut HashSet<PathBuf>,
) -> Result<(), PerformanceError> {
    let (context, reason, subs) = if run_harness != crate::harness_registry::CLAUDE {
        (
            None,
            Some("harness has no context-usage source"),
            Vec::new(),
        )
    } else if let Some(working_dir) = working_dir {
        check_root_once(claude_root, &harness_display_label(run_harness), seen_roots)?;
        infra_claude_observation(
            run_harness,
            claude_root,
            working_dir,
            excluded_session_ids,
            directory_ambiguous,
        )
    } else {
        (
            None,
            Some("no attributable session for this infrastructure role"),
            Vec::new(),
        )
    };

    let harness_acc = acc.by_harness.entry(run_harness.to_string()).or_default();
    harness_acc.context.observe(context, reason);
    harness_acc.duration.observe(
        duration,
        duration.is_none().then_some("unparseable timestamp"),
    );
    // #792: what reaches an Infrastructure role's session is the daemon's own
    // text (review batches), never human piloting — unavailable, with the reason.
    harness_acc
        .steering
        .observe(None, Some(INFRA_STEERING_REASON));

    let total_acc = infra_total_by_harness
        .entry(run_harness.to_string())
        .or_default();
    total_acc.context.observe(context, reason);
    total_acc.duration.observe(
        duration,
        duration.is_none().then_some("unparseable timestamp"),
    );
    total_acc
        .steering
        .observe(None, Some(INFRA_STEERING_REASON));

    if run_harness == crate::harness_registry::CLAUDE {
        // Infrastructure stays off the « By model » axis and its Node couples
        // (module doc): the role's residual sessions carry no model identity of
        // their own, and the axis compares Node executions.
        fold_subagents(
            &mut acc.subagents,
            None,
            None,
            "",
            "",
            "",
            "",
            subs,
            run_harness,
        );
    }

    Ok(())
}

// Whole-cohort memo (in memory only, ADR-0029). `compute_performance` folds the
// WHOLE cohort in one entangled pass, so no piece of it is independently
// memoizable — this memos the whole `StatsPerformance`, not a per-Run slice.
//
// Key = `(from, to, fingerprint)`. `fingerprint` folds, per Run in range, its
// event-log fingerprint (reused verbatim from `run_cost` so the two memos can
// never disagree about what "the events changed" means) plus the max Claude and
// Copilot transcript mtimes. `[from, to)` is keyed separately, not folded into
// the fingerprint, so two windows cannot collide on a coincidental footprint.
//
// The key deliberately carries no separate mtime for a residual Infrastructure
// session found by exclusion: that scan only walks a directory
// `max_transcript_mtime_millis` already covers recursively.
const PERFORMANCE_MEMO_CAP: usize = 256;

type PerformanceMemoKey = (String, String, u64);
type PerformanceMemoMap = HashMap<PerformanceMemoKey, StatsPerformance>;

static PERFORMANCE_MEMO: OnceLock<Mutex<PerformanceMemoMap>> = OnceLock::new();

fn performance_memo() -> &'static Mutex<PerformanceMemoMap> {
    PERFORMANCE_MEMO.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Test-only recompute counter — proves a memo hit truly skips
/// [`fold_performance`] rather than coincidentally producing an equal answer.
/// Keyed by memo key so concurrent tests do not interfere, provided each picks
/// its own `[from, to)` window (the convention every test below follows).
#[cfg(test)]
static RECOMPUTE_COUNTS: OnceLock<Mutex<HashMap<PerformanceMemoKey, u32>>> = OnceLock::new();

#[cfg(test)]
fn record_recompute_for_test(key: &PerformanceMemoKey) {
    let mut counts = RECOMPUTE_COUNTS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    *counts.entry(key.clone()).or_insert(0) += 1;
}

/// How many times [`fold_performance`] actually ran for `[from, to)`, whatever
/// fingerprint accompanied each call. `pub(crate)` so `lib.rs`'s HTTP-contract
/// tests can assert memo-hit vs forced-recompute without reaching into the map.
#[cfg(test)]
pub(crate) fn recompute_count_for_test(from: &str, to: &str) -> u32 {
    RECOMPUTE_COUNTS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .iter()
        .filter(|((f, t, _), _)| f == from && t == to)
        .map(|(_, count)| *count)
        .sum()
}

/// One Run's pre-loaded material for the heavy fold — split out of the cheap
/// event-log + mtime pass so a memo hit pays only for the DB reads and `stat`
/// calls needed to know whether anything changed, never a transcript read.
struct RunContext {
    run_id: String,
    payload: Value,
    events: Vec<crate::event_log::Event>,
    claude_root: PathBuf,
    repo_root: PathBuf,
    /// The reported-cost stores FOR THIS RUN (#708): `pi`'s follows the same
    /// sandbox-aware seam as `claude_root` (staged sink while a sandboxed Run lives,
    /// host store after merge-back), `copilot`'s is always the host journal.
    stores: HarnessStores,
}

async fn compute_performance(
    state: &AppState,
    from: &str,
    to: &str,
    refresh: bool,
) -> Result<StatsPerformance, PerformanceError> {
    let rows = sqlx::query_as::<_, (String, Option<String>)>(
        "SELECT run_id, payload FROM events \
         WHERE kind = 'run_started' AND ts >= ? AND ts < ? ORDER BY ts",
    )
    .bind(from)
    .bind(to)
    .fetch_all(&state.db)
    .await
    .map_err(|e| PerformanceError::Db(e.to_string()))?;

    let (home_root, sandbox_root) =
        crate::sandbox_run::sandbox_home_roots(state).unwrap_or_else(|_| {
            let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
            let sandbox = home.join(".pdo").join("sandbox");
            (home, sandbox)
        });

    let mut contexts: Vec<RunContext> = Vec::new();
    let mut key_hasher = DefaultHasher::new();
    for (run_id, payload) in rows {
        let payload: Value = payload
            .as_deref()
            .and_then(|p| serde_json::from_str(p).ok())
            .unwrap_or(Value::Null);
        let repo_root = payload
            .get("target_repo")
            .and_then(|v| v.as_str())
            .map(PathBuf::from)
            .unwrap_or_else(|| state.repo_root.clone());
        let sandboxed = payload
            .get("sandbox")
            .and_then(|v| v.as_str())
            .is_some_and(|s| {
                let t = s.trim();
                !t.is_empty() && !t.eq_ignore_ascii_case(crate::event_log::SandboxMode::OFF_WIRE)
            });
        let claude_root =
            crate::sandbox_run::transcripts_root(sandboxed, &run_id, &home_root, &sandbox_root);
        // #708: pi's store follows the same sandbox-aware seam as the Claude root.
        let stores = HarnessStores::for_run(sandboxed, &run_id, &home_root, &sandbox_root);

        let events = crate::load_events(&state.db, &run_id)
            .await
            .map_err(|e| PerformanceError::Db(e.to_string()))?;

        run_id.hash(&mut key_hasher);
        crate::run_cost::event_fingerprint(&events).hash(&mut key_hasher);
        crate::run_cost::max_transcript_mtime_millis(&claude_root, &repo_root, &run_id)
            .hash(&mut key_hasher);
        crate::run_cost::reported_stores_mtime_millis(
            &events,
            &claude_root,
            &stores,
            &repo_root,
            &run_id,
        )
        .hash(&mut key_hasher);

        contexts.push(RunContext {
            run_id,
            payload,
            events,
            claude_root,
            repo_root,
            stores,
        });
    }
    let key: PerformanceMemoKey = (from.to_string(), to.to_string(), key_hasher.finish());

    if !refresh {
        let guard = performance_memo()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(hit) = guard.get(&key) {
            return Ok(hit.clone());
        }
    }

    #[cfg(test)]
    record_recompute_for_test(&key);

    let value = fold_performance(contexts)?;

    let mut guard = performance_memo()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if guard.len() >= PERFORMANCE_MEMO_CAP {
        guard.clear();
    }
    guard.insert(key, value.clone());
    Ok(value)
}

/// The heavy fold — every transcript read and subagent directory scan, gated
/// behind the memo. Synchronous: events are already loaded into [`RunContext`],
/// so nothing here touches `state.db` again.
fn fold_performance(contexts: Vec<RunContext>) -> Result<StatsPerformance, PerformanceError> {
    let mut seen_roots: HashSet<PathBuf> = HashSet::new();
    let mut pipelines: BTreeMap<String, PipelineAcc> = BTreeMap::new();
    let mut model_axis: BTreeMap<String, ModelAxisModelAcc> = BTreeMap::new();
    let mut total_by_harness: BTreeMap<String, HarnessAcc> = BTreeMap::new();
    let mut infrastructure_total_by_harness: BTreeMap<String, HarnessAcc> = BTreeMap::new();
    let mut pipeline_manager_acc = NodeAcc::default();
    let mut merge_resolver_acc = NodeAcc::default();
    let mut harnesses_seen: BTreeSet<String> = BTreeSet::new();

    for RunContext {
        run_id,
        payload,
        events,
        claude_root,
        repo_root,
        stores,
    } in contexts
    {
        let pipeline_id = payload
            .get("pipeline_id")
            .and_then(|v| v.as_str())
            .or_else(|| payload.get("pipeline_name").and_then(|v| v.as_str()))
            .unwrap_or("(unknown)")
            .to_string();
        let pipeline_name = payload
            .get("pipeline_name")
            .and_then(|v| v.as_str())
            .unwrap_or(&pipeline_id)
            .to_string();
        let node_defs = node_defs_from_payload(&payload);
        let run_harness = payload
            .get("harness")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(crate::harness_registry::CLAUDE)
            .to_string();

        let pipeline_acc = pipelines.entry(pipeline_id.clone()).or_default();
        pipeline_acc.name = pipeline_name.clone();

        let mut pending: HashMap<(String, i64), PendingNode> = HashMap::new();
        let mut node_starts: HashMap<(String, i64), Vec<NodeStartRecord>> = HashMap::new();
        let mut pipeline_manager_started: Option<String> = None;
        let mut merge_resolver_started: Option<MergeResolverPending> = None;

        for event in &events {
            let iter = event.iter.unwrap_or(1);
            match event.kind {
                EventKind::RunStarted => {
                    pipeline_manager_started = Some(event.ts.clone());
                }
                EventKind::NodeStarted => {
                    let Some(node_id) = event.node_id.clone() else {
                        continue;
                    };
                    let event_payload = event.payload.as_ref();
                    let node_type = event_payload
                        .and_then(|p| p.get("node_type"))
                        .and_then(|v| v.as_str())
                        .or_else(|| node_defs.get(&node_id).map(|(_, ty, _)| ty.as_str()))
                        .unwrap_or("")
                        .to_string();
                    // #653/ADR-0060: the FROZEN isolation of this attempt, else
                    // the snapshot's. This decides which directory its sessions
                    // live in, which the type used to imply.
                    let isolated = event_payload
                        .and_then(|p| p.get("isolated_worktree"))
                        .and_then(serde_json::Value::as_bool)
                        .or_else(|| node_defs.get(&node_id).map(|(_, _, iso)| *iso))
                        .unwrap_or(false);
                    let harness = event_payload
                        .and_then(|p| p.get("harness"))
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                        .unwrap_or(crate::harness_registry::CLAUDE)
                        .to_string();
                    let session_id = event_payload
                        .and_then(|p| p.get("session_id"))
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                        .map(str::to_string);
                    // The requested model × effort, frozen at spawn (ADR-0046) —
                    // the « By model » axis's fallback when the source is mute
                    // (ADR-0065 §1). Same payload fields the cost fold reads.
                    let requested_model = event_payload
                        .and_then(|p| p.get("model"))
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                        .map(str::to_string);
                    let requested_effort = event_payload
                        .and_then(|p| p.get("effort"))
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                        .map(str::to_string);
                    node_starts
                        .entry((node_id.clone(), iter))
                        .or_default()
                        .push(NodeStartRecord {
                            node_type: node_type.clone(),
                            isolated,
                            harness: harness.clone(),
                            session_id: session_id.clone(),
                        });
                    if node_type == "script" {
                        continue;
                    }
                    pending.insert(
                        (node_id, iter),
                        PendingNode {
                            started_at: event.ts.clone(),
                            harness,
                            session_id,
                            isolated,
                            requested_model,
                            requested_effort,
                        },
                    );
                }
                EventKind::NodeCompleted | EventKind::NodeAutoCompleted => {
                    let Some(node_id) = event.node_id.clone() else {
                        continue;
                    };
                    if let Some(p) = pending.remove(&(node_id.clone(), iter)) {
                        let node_name = node_defs
                            .get(&node_id)
                            .map(|(name, ..)| name.clone())
                            .unwrap_or_else(|| node_id.clone());
                        let node_acc = pipeline_acc.nodes.entry(node_id.clone()).or_default();
                        node_acc.name = node_name.clone();
                        let working_dir = if p.isolated {
                            crate::worktree_ops::sub_worktree_path(
                                &repo_root,
                                &run_id,
                                event.node_id.as_deref().unwrap_or(""),
                                iter,
                            )
                        } else {
                            crate::worktree_ops::worktree_dir_for_run(&repo_root, &run_id)
                        };
                        harnesses_seen.insert(p.harness.clone());
                        record_node_success(
                            node_acc,
                            &mut pipeline_acc.by_harness,
                            &mut total_by_harness,
                            &mut model_axis,
                            &pipeline_id,
                            &pipeline_name,
                            &node_id,
                            &node_name,
                            &claude_root,
                            &stores,
                            &working_dir,
                            &p,
                            &event.ts,
                            &mut seen_roots,
                        )?;
                    }
                }
                EventKind::NodeFailed
                | EventKind::NodeStopped
                | EventKind::NodeInterrupted
                | EventKind::NodeStale => {
                    if let Some(node_id) = &event.node_id {
                        pending.remove(&(node_id.clone(), iter));
                    }
                }
                EventKind::MergeResolverStarted => {
                    let payload = event.payload.as_ref();
                    merge_resolver_started = Some(MergeResolverPending {
                        started_at: event.ts.clone(),
                        conflicting_node_id: payload
                            .and_then(|p| p.get("conflicting_node_id"))
                            .and_then(|v| v.as_str())
                            .map(str::to_string),
                        iter: payload.and_then(|p| p.get("iter")).and_then(|v| v.as_i64()),
                    });
                }
                EventKind::MergeResolverCompleted => {
                    if let Some(started) = merge_resolver_started.take() {
                        harnesses_seen.insert(run_harness.clone());
                        let duration = duration_millis(&started.started_at, &event.ts);
                        let (working_dir, excluded, ambiguous) =
                            match (&started.conflicting_node_id, started.iter) {
                                (Some(conflicting_node_id), Some(conflict_iter)) => {
                                    let dir = crate::worktree_ops::sub_worktree_path(
                                        &repo_root,
                                        &run_id,
                                        conflicting_node_id,
                                        conflict_iter,
                                    );
                                    let records = node_starts
                                        .get(&(conflicting_node_id.clone(), conflict_iter))
                                        .cloned()
                                        .unwrap_or_default();
                                    let excluded: HashSet<String> = records
                                        .iter()
                                        .filter_map(|r| r.session_id.clone())
                                        .collect();
                                    let ambiguous = records.iter().any(|r| {
                                        r.harness == crate::harness_registry::CLAUDE
                                            && r.session_id.is_none()
                                    });
                                    (Some(dir), excluded, ambiguous)
                                }
                                _ => (None, HashSet::new(), false),
                            };
                        record_infra_success(
                            &mut merge_resolver_acc,
                            &mut infrastructure_total_by_harness,
                            &run_harness,
                            &claude_root,
                            working_dir.as_deref(),
                            &excluded,
                            ambiguous,
                            duration,
                            &mut seen_roots,
                        )?;
                    }
                }
                EventKind::MergeResolverFailed => {
                    merge_resolver_started = None;
                }
                EventKind::RunCompleted => {
                    if let Some(started_at) = pipeline_manager_started.take() {
                        harnesses_seen.insert(run_harness.clone());
                        let duration = duration_millis(&started_at, &event.ts);
                        let topdir = crate::worktree_ops::worktree_dir_for_run(&repo_root, &run_id);
                        let excluded: HashSet<String> = node_starts
                            .values()
                            .flatten()
                            .filter_map(|r| r.session_id.clone())
                            .collect();
                        let ambiguous = node_starts.values().flatten().any(|r| {
                            r.node_type != "script"
                                && !r.isolated
                                && r.harness == crate::harness_registry::CLAUDE
                                && r.session_id.is_none()
                        });
                        record_infra_success(
                            &mut pipeline_manager_acc,
                            &mut infrastructure_total_by_harness,
                            &run_harness,
                            &claude_root,
                            Some(&topdir),
                            &excluded,
                            ambiguous,
                            duration,
                            &mut seen_roots,
                        )?;
                    }
                }
                EventKind::RunFailed | EventKind::RunSkipped => {
                    pipeline_manager_started = None;
                }
                _ => {}
            }
        }
    }

    let mut pipeline_rows: Vec<PerformanceEntity> = pipelines
        .into_iter()
        .map(|(id, acc)| finish_pipeline(id, acc))
        .collect();
    pipeline_rows.sort_by(|a, b| a.id.cmp(&b.id));

    let mut infrastructure = Vec::new();
    if !pipeline_manager_acc.by_harness.is_empty() {
        infrastructure.push(finish_infra_role(
            "pipeline-manager",
            "Pipeline Manager",
            pipeline_manager_acc,
        ));
    }
    if !merge_resolver_acc.by_harness.is_empty() {
        infrastructure.push(finish_infra_role(
            "merge-resolver",
            "Merge resolver",
            merge_resolver_acc,
        ));
    }

    Ok(StatsPerformance {
        harnesses: harnesses_seen.into_iter().collect(),
        total: PerformanceAggregate {
            harnesses: finish_by_harness(total_by_harness),
        },
        by_pipeline: pipeline_rows,
        infrastructure,
        infrastructure_total: PerformanceAggregate {
            harnesses: finish_by_harness(infrastructure_total_by_harness),
        },
        by_model: wire_model_axis(model_axis),
    })
}
