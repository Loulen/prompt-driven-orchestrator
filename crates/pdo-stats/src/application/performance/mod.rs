//! `GET /stats/performance` (#585): compare context and duration by Node and
//! harness across Claude/Copilot. The fifth Stats section, alongside
//! the Overview and Cost reads — same cohort rule (Runs
//! selected by `run_started` date, full execution retained), same derive-at-read
//! posture (ADR-0029: no snapshot table, no persisted aggregate, an in-memory
//! memo only), but its own success-only filter and its own two units (a token
//! peak, a wall-clock duration) instead of a dollar figure.
//!
//! ## Shape
//!
//! The `StatsPerformance` tree is the **agreed seam with the frontend**
//! (`frontend/src/types.ts`); `StatsDistribution` has since been revised past
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
//! read by `pdo_harness::steering` through the `harness_probes` dispatch) and a
//! `steered { steered, readable }` aggregate (executions with ≥ 1 message over
//! executions whose count is readable). A readable transcript with no message
//! counts `0` and enters `readable` without entering `steered`; a harness with
//! no steering source (`opencode`) is an absence **with a reason**, never `0`;
//! an Infrastructure role is « runtime messages only » (the review batches the
//! daemon pastes to the manager are not human piloting); a subagent is never
//! steered (nobody types in one), so its `steering` is a declared absence with
//! `expected = 0`.
//!
//! ### The three duration readings (#810, #819, ADR-0069)
//!
//! Every level carries `duration`, `active_duration` and `wait_duration` side
//! by side — the wall-clock, the wall-clock minus the execution's **declared
//! wait**, and that wait alone. One fold produces the three, so the client
//! switches « mode de durée » (Total / Active / Waiting) without a refetch and
//! without a coverage change: the three share the wall-clock's absence reason,
//! so `n=` is identical in all three modes. What opens and closes a wait is
//! `declared_waits`; a subagent and a Merge resolver measure `0` (nobody can
//! declare a wait on one), and the Pipeline Manager carries the sum of its
//! Run's Node waits, so Infrastructure reconciles with the rows under it.
//! Beside them, `waited_executions` / `executions` (#819) say how many Node
//! executions declared at least one wait — the coverage the client shows
//! behind its « i ». Nothing is inferred from a transcript or a silence.
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
//! the raw event log exactly like `pdo_run::adapters::run_cost::compute_run_cost_breakdown`
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
//! Context usage is resolved via `pdo_harness::context_peak` from the same
//! transcript/journal locations `pdo_run::adapters::run_cost` already reads (Claude:
//! `<projects_root>/<encoded_cwd>/<session_id>.jsonl`; Copilot:
//! `<copilot_root>/<session_id>/events.jsonl`; `pi`'s session by identity inside its
//! cwd folder, #707 — the host-home roots travel as a `HarnessStores`).
//!
//! ## Infrastructure subagents — a resolved-by-exclusion session identity
//!
//! Neither `RunStarted`/`RunCompleted` nor `MergeResolverStarted`/`Completed`
//! ever freezes a `session_id` for its role (unlike `NodeStarted`), so an
//! Infrastructure role's own Context/subagents cannot be read off an event
//! field the way a Node's can. This module resolves one instead, by exclusion,
//! narrowly scoped to the one directory each role's turns are known to land
//! in — mirroring `pdo_run::adapters::run_cost`'s existing "residual Claude cost"
//! technique for the same role (whole-run cost minus every attributed Node's
//! own cost) but working at session granularity, in one directory at a time,
//! so a discovered session can be traced to its own `subagents/` the same way
//! a Node's can:
//!
//! - **Pipeline Manager** orchestrates from the Run's own top-level working
//!   directory (`pdo_workspace::domain::worktree_layout::worktree_dir_for_run`) — the same
//!   directory every non-isolated Node also runs in (an isolated
//!   Node gets its own disjoint `pdo_workspace::domain::worktree_layout::sub_worktree_path`, so
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

pub mod fold;
pub mod infra;
pub mod record;
pub mod sessions;

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::Actor;
use pdo_run::application::ports::EventWindow;
use pdo_run::domain::event::{Event, EventKind};
use serde_json::Value;

use super::absorptions::load_resolver;
use super::ports::{AbsorptionStore, RunEvents, RunFiles, RunFilesRoots, SessionTranscripts};
use super::readable::readable_runs;
use crate::domain::performance::StatsPerformance;

/// One read of the Performance section. No `bucket`: Performance has no
/// time-series axis.
#[derive(Debug, Clone)]
pub struct PerformanceRead {
    pub from: String,
    pub to: String,
    /// An explicit user refresh bypasses an otherwise-current memo entry.
    pub refresh: bool,
    /// Only the Runs that reached `RunCompleted` (#810).
    pub completed_only: bool,
    /// No absorption applied (#888, ADR-0077).
    pub uncombined: bool,
}

/// Why the whole request failed, distinct from a per-observation absence
/// (issue: "Une source entière illisible produit une erreur visible... Une
/// session isolée sans télémétrie exploitable produit une absence locale").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PerformanceError {
    /// A harness's entire transcript/journal root exists but could not be
    /// listed (permissions, corrupted mount, …) — as opposed to `NotFound`,
    /// which just means "no sessions yet" and is not an error.
    SourceUnreadable(String),
    Db(String),
}

/// One Run's pre-loaded material for the heavy fold — split out of the cheap
/// event-log + mtime pass so a memo hit pays only for the log reads and the
/// `stat` calls needed to know whether anything changed, never a transcript
/// read.
pub struct RunContext {
    pub run_id: String,
    pub started_at: String,
    pub payload: Value,
    pub events: Vec<Event>,
    /// Where the Run's files live: its transcripts (sandbox-aware), its
    /// repository, and the reported stores FOR THIS RUN (#708).
    pub roots: RunFilesRoots,
}

// Whole-cohort memo (in memory only, ADR-0029). The fold takes the WHOLE
// cohort in one entangled pass, so no piece of it is independently memoizable
// — this memos the whole `StatsPerformance`, not a per-Run slice.
//
// Key = `(from, to, fingerprint)`. `fingerprint` folds the cohort switch, the
// absorptions, and per Run in range its footprint: its event-log fingerprint
// (the one the cost memo reads, so the two memos can never disagree about what
// "the events changed" means) plus the max Claude and reported-store mtimes.
// `[from, to)` is keyed separately, not folded into the fingerprint, so two
// windows cannot collide on a coincidental footprint.
//
// The key deliberately carries no separate mtime for a residual Infrastructure
// session found by exclusion: that scan only walks a directory the transcript
// mtime already covers recursively.
const PERFORMANCE_MEMO_CAP: usize = 256;

type PerformanceMemoKey = (String, String, u64);

#[derive(Default)]
struct PerformanceMemo {
    entries: HashMap<PerformanceMemoKey, StatsPerformance>,
    /// How many times the fold actually ran per `[from, to)` window — proves a
    /// memo hit truly skips the fold rather than coincidentally producing an
    /// equal answer. Cleared with the entries.
    recomputes: HashMap<(String, String), u32>,
}

pub struct ReadPerformance {
    events: Arc<dyn RunEvents>,
    absorptions: Arc<dyn AbsorptionStore>,
    files: Arc<dyn RunFiles>,
    transcripts: Arc<dyn SessionTranscripts>,
    extensions: Arc<dyn ExtensionHost>,
    memo: Mutex<PerformanceMemo>,
}

impl ReadPerformance {
    pub fn new(
        events: Arc<dyn RunEvents>,
        absorptions: Arc<dyn AbsorptionStore>,
        files: Arc<dyn RunFiles>,
        transcripts: Arc<dyn SessionTranscripts>,
        extensions: Arc<dyn ExtensionHost>,
    ) -> Self {
        Self {
            events,
            absorptions,
            files,
            transcripts,
            extensions,
            memo: Mutex::new(PerformanceMemo::default()),
        }
    }

    /// The performance of the Runs `actor` may read. The memo is keyed by
    /// the Runs folded, so two readers of different Runs never share one.
    pub async fn execute(
        &self,
        actor: &Actor,
        read: &PerformanceRead,
    ) -> Result<StatsPerformance, PerformanceError> {
        let db = |e: anyhow::Error| PerformanceError::Db(e.to_string());
        let resolver = load_resolver(self.absorptions.as_ref(), read.uncombined)
            .await
            .map_err(db)?;
        let readable = readable_runs(self.extensions.as_ref(), self.events.as_ref(), actor)
            .await
            .map_err(db)?;
        let window = EventWindow {
            from: &read.from,
            to: &read.to,
            completed_only: false,
            runs: readable.as_ref(),
        };
        let rows = self.events.run_starts(None, window).await.map_err(db)?;

        let mut contexts: Vec<RunContext> = Vec::new();
        let mut key_hasher = DefaultHasher::new();
        // #810: the cohort switch is part of the memo identity. Two cohorts over
        // the same window read the same event logs, so their footprints can
        // coincide — without this, toggling « completed runs only » could serve
        // the other cohort's answer.
        read.completed_only.hash(&mut key_hasher);
        // #890 / ADR-0077 §2: this memo keeps already-aggregated trees, so the
        // absorptions they were folded under are part of its identity — a
        // Combine or an Uncombine recomputes.
        resolver.fingerprint().hash(&mut key_hasher);
        for row in rows {
            let roots = self.files.roots(&row.run_id, &row.payload);
            let events = self.events.events(&row.run_id).await.map_err(db)?;

            // #810 « Runs terminés seulement »: the cohort becomes the Runs
            // started in the window that reached `RunCompleted`. A Run left out
            // here is left out whole — its successful Node executions included;
            // « completed » is a property of the Run, not of the execution.
            if read.completed_only && !events.iter().any(|e| e.kind == EventKind::RunCompleted) {
                continue;
            }

            row.run_id.hash(&mut key_hasher);
            self.files
                .footprint(&events, &roots, &row.run_id)
                .hash(&mut key_hasher);

            contexts.push(RunContext {
                run_id: row.run_id,
                started_at: row.ts,
                payload: row.payload,
                events,
                roots,
            });
        }
        let key: PerformanceMemoKey = (read.from.clone(), read.to.clone(), key_hasher.finish());

        if !read.refresh {
            if let Some(hit) = self.memo().entries.get(&key) {
                return Ok(hit.clone());
            }
        }

        *self
            .memo()
            .recomputes
            .entry((read.from.clone(), read.to.clone()))
            .or_insert(0) += 1;
        let value = fold::fold_performance(self.transcripts.as_ref(), contexts, &resolver)?;

        let mut memo = self.memo();
        if memo.entries.len() >= PERFORMANCE_MEMO_CAP {
            memo.entries.clear();
            memo.recomputes.clear();
        }
        memo.entries.insert(key, value.clone());
        Ok(value)
    }

    /// How many times the fold actually ran for `[from, to)`, whatever
    /// fingerprint accompanied each call: a test asserts a memo hit or a forced
    /// recompute with it.
    pub fn recomputes(&self, from: &str, to: &str) -> u32 {
        self.memo()
            .recomputes
            .get(&(from.to_string(), to.to_string()))
            .copied()
            .unwrap_or(0)
    }

    fn memo(&self) -> std::sync::MutexGuard<'_, PerformanceMemo> {
        self.memo.lock().unwrap_or_else(|error| error.into_inner())
    }
}
