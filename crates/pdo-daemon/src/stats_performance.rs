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
//! Wire shape is [`StatsPerformance`]: `harnesses` (the columns), `total` (the
//! whole cohort pooled — see its doc comment), `by_pipeline` (Pipeline →
//! Nodes → subagent groups, [`PerformanceEntity`]'s recursive shape), a
//! separate `infrastructure` branch (no Project view — out of scope) and
//! `infrastructure_total` (both Infrastructure roles pooled raw, the
//! `total`/`by_pipeline` split's own mirror one level up — see its doc
//! comment, #585 review follow-up). The overall tree (`StatsPerformance` /
//! `StatsPerformanceEntity` / `StatsHarnessPerformance`) is the **agreed seam
//! with the frontend** (`frontend/src/types.ts`) — that frontend code was this
//! module's original reference. [`StatsDistribution`] itself has since been
//! revised past what `frontend/src/types.ts` currently declares (see its own
//! doc comment: an always-present coverage object with a nullable `stats`
//! payload, not an all-or-nothing nullable cell) — a change coordinated with,
//! not silently made against, the frontend owner; `frontend/src/types.ts` is
//! expected to catch up to this shape, along with the new `infrastructure_total`
//! field, rather than the other way around. A Node's row
//! also carries its discovered subagent groups as children — grouped by
//! declared name, "Unidentified subagent" when none reads as stable (see
//! [`claude_subagent_group`]) — which never feed their parent's own
//! distribution (Implementation Decisions: "Le
//! contexte d'une exécution de Node porte uniquement sur sa session
//! principale"). Duration is in **milliseconds** (matches the frontend's own
//! `value / 1_000` seconds conversion); Context is a raw peak token count.
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
//! `<copilot_root>/<session_id>/events.jsonl`).
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

/// One metric's (Context or Duration) coverage for one row × one harness:
/// `measured` readable values out of `expected` (successful attempts, whether
/// or not this metric could be read for them), why any gap between the two
/// exists, and the R-7 six-stat summary — present only when `measured > 0`.
///
/// This object is **never null on the wire**: coverage (`measured`/`expected`/
/// `missing_reasons`) is a property of the metric itself and must stay visible
/// even when nothing was measured at all — "une mesure absente ne devient
/// jamais zéro" means the absence must be *explained*, not that the coverage
/// bookkeeping disappears with it. Only `stats` — the six numbers a boxplot
/// needs — becomes `null`, and only when `measured == 0` (there is nothing to
/// summarize). This was changed from an earlier all-or-nothing `Option<...>`
/// cell precisely because that shape silently dropped `expected`/
/// `missing_reasons` for a fully absent metric, which the frontend cannot
/// honestly render ("never ran" vs "no reliable bounds" need to stay
/// distinguishable even with zero readable values).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct StatsDistribution {
    pub stats: Option<crate::distribution::SixStats>,
    pub measured: i64,
    pub expected: i64,
    pub missing_reasons: Vec<String>,
}

/// Context + Duration for one row, under one harness. Matches the frontend's
/// `StatsHarnessPerformance` — `context`/`duration` are always a
/// [`StatsDistribution`] object, never `null` (see its doc comment).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct StatsHarnessPerformance {
    pub harness: String,
    pub context: StatsDistribution,
    pub duration: StatsDistribution,
}

/// One row's metrics across every harness it saw at least one successful
/// observation under. Matches the frontend's `StatsPerformanceAggregate`.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub(crate) struct PerformanceAggregate {
    pub harnesses: Vec<StatsHarnessPerformance>,
}

/// One node in the Pipeline→Node / Infrastructure→role / subagent-group tree —
/// a single recursive shape for all three, matching the frontend's
/// `StatsPerformanceEntity` (`extends StatsPerformanceAggregate`, i.e. an `id`
/// and `name` plus the same `harnesses` field, plus `nodes` and `subagents`
/// children). A Pipeline's `subagents` is always empty (subagents are declared
/// on Nodes, never Pipelines); a Node's/subagent-group's `nodes` is always
/// empty (no further nesting below a Node); an Infrastructure role's `nodes`
/// is always empty, but its `subagents` is populated exactly like a Node's
/// (issue user story #36 — see the module doc's "Infrastructure subagents"
/// section for how a role's own session is found).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub(crate) struct PerformanceEntity {
    pub id: String,
    pub name: String,
    pub harnesses: Vec<StatsHarnessPerformance>,
    pub nodes: Vec<PerformanceEntity>,
    pub subagents: Vec<PerformanceEntity>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub(crate) struct StatsPerformance {
    /// Every harness with at least one successful observation in the cohort —
    /// the columns the client renders (Claude/Copilot today; ADR-0051 leaves
    /// room for more without a client change).
    pub harnesses: Vec<String>,
    /// Every successful main-session Node observation pooled across the WHOLE
    /// cohort (every Pipeline, every Node) — the same "pool the raw
    /// observations, don't average the averages" rule the issue applies to a
    /// Pipeline's own Nodes, just one level up. Deliberately excludes
    /// Infrastructure roles (Pipeline Manager / Merge resolver measure a
    /// different kind of thing — a whole Run's or resolver's wall-clock, not a
    /// Node execution) — a scoped assumption, since the issue does not spell
    /// out whether the headline figure should blend the two.
    pub total: PerformanceAggregate,
    pub by_pipeline: Vec<PerformanceEntity>,
    pub infrastructure: Vec<PerformanceEntity>,
    /// Every successful Infrastructure-role observation (Pipeline Manager's own
    /// occurrences and Merge resolver's own occurrences) pooled together at the
    /// **raw-observation** level — never a mean of the two roles' own means.
    /// This exists precisely so the client's master/detail view (the whole
    /// Infrastructure branch's own boxplot, alongside its two rows'
    /// individually) never has to fabricate one by averaging averages, the
    /// same failure mode [`Self::total`] avoids for Pipeline → Node (#585
    /// review follow-up, blocker 3). Never pools a role's `subagents` — those
    /// stay scoped to their own role, exactly like a Node's subagents are
    /// never folded into [`Self::total`] either.
    pub infrastructure_total: PerformanceAggregate,
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
/// transcript corpora), fetched lazily on-demand only (issue: "Performance ne
/// charge ses données qu'à son ouverture ou lors d'un Refresh explicite" — the
/// client, not this handler, owns the "on demand" half; this handler simply
/// never runs on a timer).
pub(crate) async fn stats_performance(
    State(state): State<Arc<AppState>>,
    Query(q): Query<PerformanceQuery>,
) -> Response {
    match compute_performance(&state, &q.from, &q.to, q.refresh).await {
        Ok(payload) => Json(payload).into_response(),
        Err(error) => error.into_response(),
    }
}

// --- Accumulation --------------------------------------------------------------

/// Running fold for one metric on one row × harness: raw readable values (for
/// the eventual R-7 fold) plus the expected/absent bookkeeping.
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

    /// Always returns a coverage object; `stats` is `None` only when nothing
    /// was readable at all (`measured == 0`) — see [`StatsDistribution`]'s doc
    /// comment.
    fn finish(self) -> StatsDistribution {
        StatsDistribution {
            stats: r7_distribution(&self.values),
            measured: self.values.len() as i64,
            expected: self.expected,
            missing_reasons: self.absence_reasons.into_iter().collect(),
        }
    }
}

#[derive(Debug, Default)]
struct HarnessAcc {
    context: MetricAcc,
    duration: MetricAcc,
}

impl HarnessAcc {
    fn finish(self, harness: String) -> StatsHarnessPerformance {
        StatsHarnessPerformance {
            harness,
            context: self.context.finish(),
            duration: self.duration.finish(),
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
    }
}

/// Same shape as [`finish_node`], for an Infrastructure role — `id`/`name` are
/// the role's own fixed identity, not `acc.name` (an infra `NodeAcc` never
/// sets it).
fn finish_infra_role(id: &str, name: &str, acc: NodeAcc) -> PerformanceEntity {
    PerformanceEntity {
        id: id.to_string(),
        name: name.to_string(),
        harnesses: finish_by_harness(acc.by_harness),
        nodes: Vec::new(),
        subagents: finish_subagents(acc.subagents),
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
    }
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

// --- Subagent declared-group heuristic ------------------------------------------

/// Whether `stem` reads as an opaque, machine-generated identifier (a UUID, or
/// a long run of hex digits) rather than a human-legible declared name.
fn looks_like_opaque_id(stem: &str) -> bool {
    let is_uuid = stem.len() == 36
        && stem.as_bytes().iter().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => *b == b'-',
            _ => b.is_ascii_hexdigit(),
        });
    let is_long_hex = stem.len() >= 16 && stem.chars().all(|c| c.is_ascii_hexdigit());
    is_uuid || is_long_hex
}

/// The declared group a discovered subagent transcript file falls under
/// (issue user stories #27/#28): the file's own name, verbatim, unless it reads
/// as an opaque generated id — in which case the harness declared no stable
/// identity and the file falls into the explicit "Unidentified subagent"
/// fallback bucket, so it is never silently dropped.
///
/// **Scoped assumption**: no real Claude Code subagent transcript naming
/// convention is confirmed anywhere in this repository (only anonymous
/// filenames like `side.jsonl` appear in existing fixtures/tests, see
/// `run_cost.rs`). Filename is the only signal available without inventing an
/// unconfirmed in-transcript field, so it is the one used here; if a harness's
/// subagent files are later found to embed a `subagent_type` (or similar) key,
/// this is the seam to extend.
fn claude_subagent_group(file_stem: &str) -> String {
    if file_stem.is_empty() || looks_like_opaque_id(file_stem) {
        "Unidentified subagent".to_string()
    } else {
        file_stem.to_string()
    }
}

// --- Source readability (visible endpoint error) --------------------------------

/// Confirm `root` (a harness's whole transcript/journal source) is listable, or
/// name it in a visible error. `NotFound` is not an error — it means "this
/// harness has never written a session yet", not "the source is broken".
/// Checked once per distinct root the cohort touches (memoized by the caller),
/// never per session — a single unreadable session degrades to a local,
/// silent absence instead (issue's explicit source-vs-session distinction).
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

// --- Context/duration resolution -------------------------------------------------

fn parse_ts(ts: &str) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    chrono::DateTime::parse_from_rfc3339(ts).ok()
}

fn duration_millis(started_at: &str, completed_at: &str) -> Option<f64> {
    let start = parse_ts(started_at)?;
    let end = parse_ts(completed_at)?;
    Some((end - start).num_milliseconds() as f64)
}

/// One harness's storage root to resolve a session's transcript against — the
/// one remaining harness-name check in this module, and deliberately not one
/// [`crate::harness_probes`] absorbs: it picks between two independently
/// **computed** roots (a per-Run, sandbox-aware Claude root vs a global,
/// home-based Copilot root, see [`compute_performance`]), not between two
/// **behaviours**. ADR-0051 governs the latter — which parser reads a
/// transcript, which directory convention finds a subagent — and both of
/// those now dispatch exclusively through `crate::harness_probes` below, never
/// through a `match harness.as_str() { .. }` written in this module.
fn source_root<'a>(harness: &str, claude_root: &'a Path, copilot_root: &'a Path) -> &'a Path {
    if harness == crate::harness_registry::COPILOT {
        copilot_root
    } else {
        claude_root
    }
}

/// One harness's context peak for one main session, or the absence reason.
/// Dispatches exclusively through [`crate::harness_probes`] (ADR-0051): this
/// function never names `claude_session_peak`/`copilot_session_peak`, nor
/// `claude`/`copilot`'s own transcript path convention — it asks
/// [`crate::harness_probes::resolve_transcript`] for the file and
/// [`crate::harness_probes::context_peak`] for the reading, gated on
/// [`crate::harness_probes::can_measure_context`]. The source's readability is
/// checked here too (once per distinct root, via `seen_roots`) — only for a
/// harness that actually has a context-usage source, so an unrelated harness
/// (e.g. `opencode`) never triggers a Claude/Copilot root check it doesn't need.
fn main_session_context(
    harness: &str,
    root: &Path,
    working_dir: &Path,
    session_id: Option<&str>,
    seen_roots: &mut HashSet<PathBuf>,
) -> Result<(Option<f64>, Option<&'static str>), PerformanceError> {
    if !crate::harness_probes::can_measure_context(harness) {
        return Ok((None, Some("harness has no context-usage source")));
    }
    check_root_once(root, &harness_display_label(harness), seen_roots)?;
    let Some(session_id) = session_id.filter(|s| !s.is_empty()) else {
        return Ok((None, Some("no session identity")));
    };
    let Some(path) =
        crate::harness_probes::resolve_transcript(harness, root, working_dir, Some(session_id))
    else {
        return Ok((None, Some("no attributable transcript")));
    };
    Ok(match std::fs::read_to_string(&path) {
        Ok(text) => match crate::harness_probes::context_peak(harness, &text) {
            Some(peak) => (Some(peak as f64), None),
            None => (None, Some("no readable context usage in transcript")),
        },
        Err(_) => (None, Some("no attributable transcript")),
    })
}

/// The subagent transcript files discovered for one main session under
/// `harness`, already grouped into their declared label ([`claude_subagent_group`]).
/// Dispatches exclusively through [`crate::harness_probes::subagent_transcripts`]
/// (ADR-0051): a harness with no nested-subagent convention (every harness but
/// `claude` today, `copilot` included — see [`crate::harness_probes`]'s
/// `CopilotProbes` doc comment for the investigated motive) answers an empty
/// `Vec` from the dispatch itself, never a `match`/`if harness == ..` written
/// here.
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

// --- The per-run walk ------------------------------------------------------------

struct PendingNode {
    started_at: String,
    harness: String,
    session_id: Option<String>,
    /// Where the NodeRun works (#653) — its own sub-worktree, or the Run's.
    isolated: bool,
}

/// A `MergeResolverStarted` awaiting its pairing `MergeResolverCompleted` —
/// `conflicting_node_id`/`iter` (its own payload fields) resolve the working
/// directory an attributed session is looked for in (see the module doc's
/// "Infrastructure subagents" section).
struct MergeResolverPending {
    started_at: String,
    conflicting_node_id: Option<String>,
    iter: Option<i64>,
}

/// Fold every discovered subagent transcript (already labelled by
/// [`subagent_groups`]) into `subagents`' declared groups, under `harness`.
/// Shared between a Node's own subagents and an Infrastructure role's (issue
/// user story #36 — a role's subagents are grouped exactly like a Node's).
///
/// The context peak dispatches through [`crate::harness_probes::context_peak`]
/// (ADR-0051), like every other reading in this module. The duration span does
/// not: no capability marker exists yet for "start/end bounds from a
/// transcript" (only Claude's subagent convention exists at all today, see
/// [`crate::harness_probes`]'s `CopilotProbes` doc comment), so
/// [`crate::context_peak::claude_transcript_time_span`] stays a direct call —
/// the seam to extend if a second harness ever grows subagent transcripts of
/// its own with a different span shape.
fn fold_subagents(
    subagents: &mut BTreeMap<String, SubagentAcc>,
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
    }
}

/// Resolve one successful main-session Node execution's Context/Duration into
/// `node_acc` (its Node), `pipeline_by_harness` (its Pipeline, pooled) and
/// `total_by_harness` (the whole cohort, pooled one level further — see
/// [`StatsPerformance::total`]'s doc comment) alike, and fold every discovered
/// subagent transcript into the Node's own subagent groups (never into any of
/// the three harness accumulators — Implementation Decisions).
#[allow(clippy::too_many_arguments)]
fn record_node_success(
    node_acc: &mut NodeAcc,
    pipeline_by_harness: &mut BTreeMap<String, HarnessAcc>,
    total_by_harness: &mut BTreeMap<String, HarnessAcc>,
    claude_root: &Path,
    copilot_root: &Path,
    working_dir: &Path,
    pending: &PendingNode,
    completed_at: &str,
    seen_roots: &mut HashSet<PathBuf>,
) -> Result<(), PerformanceError> {
    let harness = pending.harness.clone();
    let root = source_root(&harness, claude_root, copilot_root);
    let (context, reason) = main_session_context(
        &harness,
        root,
        working_dir,
        pending.session_id.as_deref(),
        seen_roots,
    )?;
    let duration = duration_millis(&pending.started_at, completed_at);

    for acc in [
        node_acc.by_harness.entry(harness.clone()).or_default(),
        pipeline_by_harness.entry(harness.clone()).or_default(),
        total_by_harness.entry(harness.clone()).or_default(),
    ] {
        acc.context.observe(context, reason);
        acc.duration.observe(
            duration,
            duration.is_none().then_some("unparseable timestamp"),
        );
    }

    let files = subagent_groups(&harness, root, working_dir, pending.session_id.as_deref());
    fold_subagents(&mut node_acc.subagents, files, &harness);

    Ok(())
}

/// Capitalize `harness`'s first letter for a human-facing message (`"claude"`
/// → `"Claude"`, `"copilot"` → `"Copilot"`) — display prose, not a capability
/// decision, so it needs no `match`/dispatch: every harness name capitalizes
/// the same way.
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

// --- Infrastructure role session resolution (by exclusion) -----------------------

/// Every Claude `session_id` a Node is known to have opened at `(node_id,
/// iter)`, across every attempt (including a stopped-then-restarted one that
/// shares the same working directory) — the exclusion list an Infrastructure
/// role's own session is resolved against. Populated for every `NodeStarted`
/// seen, regardless of node type or outcome, since even a failed/stopped
/// attempt's session file still occupies the shared directory.
#[derive(Debug, Clone)]
struct NodeStartRecord {
    node_type: String,
    /// Where the NodeRun works (#653) — its own sub-worktree, or the Run's.
    isolated: bool,
    harness: String,
    session_id: Option<String>,
}

/// One directory's worth of Claude sessions, resolved down to the ONE not
/// already known to belong to a Node — or why that couldn't be done. Mirrors
/// `run_cost.rs`'s `ambiguous_shared_claude` guard, but at session (not
/// dollar) granularity: an ambiguous directory is never attributed by path
/// alone (see the module doc's "Infrastructure subagents" section).
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
/// Claude session, or the motivated absence when none could be attributed.
/// Read exactly like a Node's own session (`main_session_context`/
/// `subagent_groups`) — a subagent still counts even if the role's own
/// attempt around it later failed (same semantics as a Node's subagents).
/// `harness` is threaded through to the dispatch calls (ADR-0051) even though
/// the caller only ever reaches this for `claude` today (the resolve-by-
/// exclusion directory scan above it is structurally Claude-only, see the
/// module doc's "Infrastructure subagents" section) — so this stays correct
/// unchanged if a second harness ever grows the same directory convention.
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

/// Resolve and fold one Infrastructure role's Context/Duration/subagents for
/// one occurrence into `acc` (its own role) and `infra_total_by_harness` (both
/// roles pooled, raw — [`StatsPerformance::infrastructure_total`]'s doc
/// comment) alike — shared by the Pipeline Manager and Merge resolver arms of
/// the per-run walk (they differ only in which directory and exclusion set
/// apply).
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

    let total_acc = infra_total_by_harness
        .entry(run_harness.to_string())
        .or_default();
    total_acc.context.observe(context, reason);
    total_acc.duration.observe(
        duration,
        duration.is_none().then_some("unparseable timestamp"),
    );

    if run_harness == crate::harness_registry::CLAUDE {
        fold_subagents(&mut acc.subagents, subs, run_harness);
    }

    Ok(())
}

// --- Whole-cohort memo -------------------------------------------------------
//
// The issue requires the result "cached in memory according to events and
// observed sources" (not persisted — ADR-0029) — the same posture
// `run_cost.rs`'s per-Run breakdown memo already established, just pitched one
// level up: `compute_performance` folds the WHOLE cohort into shared mutable
// accumulators in a single entangled pass (unlike a per-Run breakdown, no
// piece of it is an independent, separately-memoizable result), so this memos
// the **whole computed [`StatsPerformance`]** rather than a per-Run slice.
//
// The key is `(from, to, fingerprint)`, where `fingerprint` folds — for every
// Run whose `RunStarted` falls in `[from, to)` — that Run's own event-log
// fingerprint ([`crate::run_cost::event_fingerprint`], reused verbatim so the
// two memos can never disagree about what "the events changed" means) and the
// max mtime across every transcript/journal file the Run's own events could
// possibly reference, Claude ([`crate::run_cost::max_transcript_mtime_millis`])
// and Copilot ([`crate::run_cost::copilot_mtime_millis`]) alike. A new event
// appended to any Run in range, a rewritten transcript, or a widened `[from,
// to)` all change the key and force a recompute; nothing outside that
// footprint can silently go stale, and nothing inside it can silently serve a
// stale answer. `[from, to)` is part of the key (not just an input folded into
// the fingerprint) so two different windows can never collide even if their
// event footprints coincided.
//
// A subtlety worth spelling out: this key intentionally does **not** cover a
// residual Infrastructure session discovered by exclusion (Pipeline
// Manager/Merge resolver's own directory scan) with its own separate mtime —
// that discovery only ever walks a directory this same Run's own Claude root
// already covers via `max_transcript_mtime_millis`'s recursive walk, so a new
// or rewritten residual file already bumps the same max.
//
// No explicit cache-busting query parameter exists (or is needed): the issue
// describes "Refresh" as the client re-issuing the identical `[from, to)`
// query on demand (Implementation Decisions: "l'utilisateur déclenche
// Refresh"), and the natural key already recomputes whenever anything in that
// window actually changed — a same-window, no-new-data Refresh reusing the
// memoized answer is the cache doing its job, not staleness.
const PERFORMANCE_MEMO_CAP: usize = 256;

type PerformanceMemoKey = (String, String, u64);
type PerformanceMemoMap = HashMap<PerformanceMemoKey, StatsPerformance>;

static PERFORMANCE_MEMO: OnceLock<Mutex<PerformanceMemoMap>> = OnceLock::new();

fn performance_memo() -> &'static Mutex<PerformanceMemoMap> {
    PERFORMANCE_MEMO.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Test-only recompute counter, keyed by the exact memo key — proves a memo
/// hit truly skips [`fold_performance`] rather than merely producing an
/// identical answer by coincidence. Keyed (not a bare total) so tests running
/// concurrently in the same process never interfere with each other as long
/// as each picks its own `[from, to)` window (the convention every test below
/// follows). `#[cfg(test)]`-gated: zero footprint in the production binary.
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

/// Total number of times [`fold_performance`] actually ran for requests whose
/// `[from, to)` matched `(from, to)` — regardless of which fingerprint hash
/// accompanied each call. `pub(crate)` so `lib.rs`'s HTTP-contract tests can
/// assert a repeat request was served from the memo (count unchanged) or
/// correctly forced a recompute (count incremented) without reaching into the
/// map's internals.
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

/// One Run's own pre-loaded material for the heavy fold — split out of the
/// cheap event-log + mtime pass (`compute_performance`) so a memo hit never
/// has to pay for [`fold_performance`]'s transcript reads (context peak
/// parsing, subagent directory scans) at all, only the cheap DB reads and
/// `stat` calls needed to know whether anything changed.
struct RunContext {
    run_id: String,
    payload: Value,
    events: Vec<crate::event_log::Event>,
    claude_root: PathBuf,
    repo_root: PathBuf,
}

// --- Whole-cohort computation ------------------------------------------------------

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
    let copilot_root = crate::sandbox_run::copilot_store_root(&home_root);

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

        let events = crate::load_events(&state.db, &run_id)
            .await
            .map_err(|e| PerformanceError::Db(e.to_string()))?;

        run_id.hash(&mut key_hasher);
        crate::run_cost::event_fingerprint(&events).hash(&mut key_hasher);
        crate::run_cost::max_transcript_mtime_millis(&claude_root, &repo_root, &run_id)
            .hash(&mut key_hasher);
        crate::run_cost::copilot_mtime_millis(&events, &copilot_root).hash(&mut key_hasher);

        contexts.push(RunContext {
            run_id,
            payload,
            events,
            claude_root,
            repo_root,
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

    let value = fold_performance(contexts, &copilot_root)?;

    let mut guard = performance_memo()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if guard.len() >= PERFORMANCE_MEMO_CAP {
        guard.clear();
    }
    guard.insert(key, value.clone());
    Ok(value)
}

/// The heavy fold itself — every transcript read, every subagent directory
/// scan, gated behind the memo in [`compute_performance`]. Pure/synchronous:
/// every Run's events are already loaded ([`RunContext`]), so nothing here
/// touches `state.db` again.
fn fold_performance(
    contexts: Vec<RunContext>,
    copilot_root: &Path,
) -> Result<StatsPerformance, PerformanceError> {
    let mut seen_roots: HashSet<PathBuf> = HashSet::new();
    let mut pipelines: BTreeMap<String, PipelineAcc> = BTreeMap::new();
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
        pipeline_acc.name = pipeline_name;

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
                        let node_acc = pipeline_acc.nodes.entry(node_id).or_default();
                        node_acc.name = node_name;
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
                            &claude_root,
                            copilot_root,
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
    })
}
