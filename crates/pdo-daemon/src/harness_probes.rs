//! The capability factory for agentic harnesses (#553, ADR-0045).
//!
//! Everything PDO knows how to do **beyond launching** a harness is a
//! **capability**, written harness by harness: estimate a cost, resolve a
//! transcript, constate an end of turn, spot a usage-limit menu, hold a staging
//! floor. This module is the factory for those capabilities — and its whole
//! point is to make their **absence legible**: never supplied, never silent.
//!
//! The shape is deliberate and was settled at the grilling (do not redraw it):
//!
//! - a trait ([`HarnessProbes`]) whose **every method defaults to "absent"**;
//! - one implementation per harness that **overrides only what it can do**
//!   ([`ClaudeProbes`] overrides all five);
//! - a factory ([`probes_for`]) that returns `Option<&'static dyn …>`, so a
//!   harness **declared in data** (an `opencode`, or a user's disk descriptor)
//!   gets `None` — "no capability" — for free and un-forgettably.
//!
//! A closed `enum Harness` with compiled exhaustiveness was **explicitly
//! rejected**: a data-declared harness has no variant, so every `match` would
//! carry a catch-all arm. Here it costs nothing — an unknown name simply does not
//! match, and the caller reads "absent" off the `None`.
//!
//! ## A capability is a **dispatch point**, not a presence guard (ADR-0051)
//!
//! Each capability is the site where the **implementation of the resolved harness
//! is chosen** — not a boolean the caller reads before running `claude`'s function
//! anyway. So the trait carries **behaviour**, not only markers: the sweep asks
//! [`HarnessProbes::classify_turn_ended`] / [`HarnessProbes::detect_usage_limit`] /
//! [`HarnessProbes::resolve_transcript`], and gets *this harness's* answer. The
//! claude-proper functions (transcript resolution, the turn-state parser, the
//! usage-limit anchors) are `claude`'s implementation of these methods — no longer
//! reachable from a generic consumer, which holds a `&dyn HarnessProbes` and never
//! names them. A harness that declares an implementation gets **its** behaviour;
//! the regression ADR-0051 exists to kill (declare a variant, silently read
//! claude's paths) is now impossible.
//!
//! [`resolved`] is **total** — every name resolves to a `&'static dyn HarnessProbes`
//! ([`ClaudeProbes`] for `claude`, [`NullProbes`] for a data-declared harness) — so
//! there is always a dispatch, and "absent" is a **method returning `None`/`false`**,
//! distinguishable from a missing dispatch (ADR-0051 §2).
//!
//! ## What "absent is said, never supplied" buys each caller
//! - **cost** ([`crate::run_cost`]): a harness with no [`HarnessProbes::cost_source`]
//!   contributes "—" **and a reason naming it**, never `$0`, never a silent
//!   `partial` — the same vein as `unpriced_models` (#425).
//! - **turn-end / usage-limit** ([`crate::stale_detector`]): the two sweep probes
//!   dispatch through [`turn_ended`] / [`usage_limit_shown`], gated on
//!   [`HarnessProbes::turn_end_substrate`] / [`HarnessProbes::usage_limit_anchor`].
//!   Absent ⇒ the probe does not run, and no node is ever auto-completed on an
//!   invented heuristic, nor its pane matched against another harness's menu.
//! - **turn-end setting** ([`crate::node_spawn`]): enabling turn-end completion on a
//!   harness with no substrate is **said once** ([`turn_end_absence_note`]) rather
//!   than being a silent no-op.
//! - **sandbox** ([`crate::node_spawn`]): a sandboxed Run on a harness with no
//!   [`HarnessProbes::staging_floor`] is **said once, visibly** — it holds only by
//!   the user's image and the profile's `$HOME` exceptions, without the plancher's
//!   guarantees (ADR-0031).
//!
//! This module is narrow on purpose: it fabricates **capabilities, never a
//! launch** (a launch is data — the argv template of [`crate::harness_registry`]).

use crate::harness_registry;
use std::path::{Path, PathBuf};

/// A harness's cost source — how PDO turns a live Run into a dollar figure.
///
/// A marker, present or absent. Absent ⇒ the Run's cost is "—" with a reason.
/// `claude`'s is the only source today; a second harness's would be a distinct
/// variant (measured: `opencode` writes its own per-message cost into a SQLite
/// its HTTP API exposes, four buckets that do not map onto `claude`'s — so cost
/// stays code, never a declared mini-language, ADR-0045).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CostSource {
    /// PDO derives the cost itself: per-message token usage in the harness's
    /// transcript × the resolved price table (ADR-0034), as [`crate::run_cost`]
    /// already does for `claude`. A **derived** cost (ADR-0052 §1).
    DerivedFromTranscript,
    /// The harness counts its own cost, in its own billing unit, and PDO converts
    /// it by a **published constant** — never the price table (ADR-0052 §2). A
    /// **reported** cost: it cannot produce an `unpriced_models` signal, and it does
    /// not re-derive from tokens (which would double-count the cache). `copilot`'s:
    /// the `totalNanoAiu` its event journal reports, × [`crate::copilot_journal`]'s
    /// constant.
    ReportedByConstant,
}

impl CostSource {
    /// How this source reads in the published support table
    /// ([`crate::harness_support`]). The label lives on the variant so the table
    /// can never describe a mechanism the code no longer dispatches to — same for
    /// every other capability's `label` below.
    pub(crate) fn label(self) -> &'static str {
        match self {
            CostSource::DerivedFromTranscript => {
                "derived — per-message token usage × the price table"
            }
            CostSource::ReportedByConstant => {
                "reported — the harness's own billing unit × a published constant"
            }
        }
    }
}

/// How PDO resolves a harness's transcript on disk.
///
/// A cost or a turn-end read needs to find the right file first; a harness whose
/// store PDO cannot map has neither. (Measured: `opencode` migrated its sessions
/// into a SQLite and left months of dead JSON on disk — a store is not a contract,
/// ADR-0045 — so PDO declares no resolution for it rather than reading zeros.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TranscriptResolution {
    /// Claude Code's `<projects_root>/<encoded-cwd>/<session-id>.jsonl` (#473),
    /// newest-mtime for a pre-#473 row.
    ClaudeJsonl,
    /// GitHub Copilot's `<store>/<session-id>/events.jsonl` (#615) — indexed by the
    /// **session identity PDO imposed** at launch, with **no** working-directory
    /// encoding, so two nodes sharing a worktree have distinct journals structurally
    /// (the #473 collision has no equivalent here).
    CopilotEventsJsonl,
}

impl TranscriptResolution {
    pub(crate) fn label(self) -> &'static str {
        match self {
            TranscriptResolution::ClaudeJsonl => "the JSONL transcript, keyed by working directory",
            TranscriptResolution::CopilotEventsJsonl => {
                "the event journal, keyed by the session identity PDO imposed"
            }
        }
    }
}

/// The substrate PDO reads to constate an end of turn (#469 §2, ADR-0043).
///
/// Absent ⇒ the turn-end auto-completion probe never runs for this harness, so no
/// node is auto-completed on an invented heuristic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TurnEndSubstrate {
    /// The Claude Code JSONL transcript tail, classified by
    /// [`crate::stale_detector::parse_turn_state`].
    ClaudeTranscript,
    /// GitHub Copilot's event journal, whose explicit `assistant.turn_end` event is
    /// classified by [`crate::copilot_journal::turn_ended`] (#615). Depends on no
    /// instance setting and writes nothing into the user's config.
    CopilotEventJournal,
}

impl TurnEndSubstrate {
    pub(crate) fn label(self) -> &'static str {
        match self {
            TurnEndSubstrate::ClaudeTranscript => {
                "an injected `Stop` hook, plus the transcript tail as the sweep's fallback"
            }
            TurnEndSubstrate::CopilotEventJournal => {
                "the journal's explicit `assistant.turn_end` event"
            }
        }
    }
}

/// A harness's on-screen usage-limit menu — the anchor PDO matches in a pane
/// capture (#290). **Proper to a harness**: the wording is claude's, so the probe
/// is gated on this capability being present.
///
/// A marker, not a carrier of the substrings: those live next to their matcher in
/// [`crate::stale_detector`] (they drift with Claude Code and are updated there).
/// Absent ⇒ the usage-limit probe never runs for this harness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UsageLimitAnchor {
    /// Claude Code's interactive "stop and wait for limit to reset" menu, matched
    /// by [`crate::stale_detector::detect_usage_limit`].
    ClaudePaneMenu,
}

impl UsageLimitAnchor {
    pub(crate) fn label(self) -> &'static str {
        match self {
            UsageLimitAnchor::ClaudePaneMenu => {
                "the interactive \"wait for limit to reset\" menu, matched in a pane capture"
            }
        }
    }
}

/// How PDO measures a harness's **context-window peak** — the maximum per-turn
/// occupancy a session reached, in tokens (#585, Stats → Performance). A
/// marker, not the parser itself: [`crate::context_peak`] holds the actual
/// per-harness token math, so this enum can never describe a mechanism that
/// module does not implement.
///
/// Absent ⇒ Performance shows no Context column for this harness at all (it
/// never invents a boxplot from a metric it cannot read).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContextUsageSource {
    /// Claude Code's per-message `usage` (input + both cache buckets + output),
    /// deduplicated across resume/compaction replays, maxed over the session's
    /// turns — [`crate::context_peak::claude_session_peak`].
    ClaudeTranscriptPeak,
    /// GitHub Copilot's event journal `usage` readings, converted from their
    /// cumulative-since-session-start counters to a per-turn contribution before
    /// the max is sought — [`crate::context_peak::copilot_session_peak`].
    CopilotJournalPeak,
}

impl ContextUsageSource {
    pub(crate) fn label(self) -> &'static str {
        match self {
            ContextUsageSource::ClaudeTranscriptPeak => {
                "derived — per-turn token usage from the transcript, deduplicated and maxed"
            }
            ContextUsageSource::CopilotJournalPeak => {
                "derived — the journal's cumulative usage counters, converted to a per-turn \
                 contribution and maxed"
            }
        }
    }
}

/// The sandbox staging floor a harness guarantees (ADR-0031).
///
/// Absent ⇒ a sandboxed Run on this harness holds only by the user's image and
/// the profile's `$HOME` exceptions, without the plancher's guarantees — and PDO
/// says so once (see [`staging_floor_absence_note`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StagingFloor {
    /// The `.claude` staged home: valid credentials, org managed settings, bypass
    /// permissions accepted, trust pre-granted to the Run root, empty `projects/`.
    ClaudeDotClaude,
}

impl StagingFloor {
    pub(crate) fn label(self) -> &'static str {
        match self {
            StagingFloor::ClaudeDotClaude => {
                "a staged `.claude` home — credentials, org managed settings, pre-granted trust"
            }
        }
    }
}

/// The capabilities of one harness. **Every method defaults to "absent"**
/// (`None`); an implementation overrides only what its harness can do.
///
/// `Sync` so a single `&'static` instance can be handed out from [`probes_for`]
/// across the daemon's threads (each impl is a stateless zero-sized type).
pub(crate) trait HarnessProbes: Sync {
    /// Cost source, or `None` (contribute "—" + a reason, never `$0`).
    fn cost_source(&self) -> Option<CostSource> {
        None
    }
    /// How PDO finds this harness's transcript, or `None`.
    fn transcript_resolution(&self) -> Option<TranscriptResolution> {
        None
    }
    /// The end-of-turn substrate, or `None` (the sweep's turn-end probe is gated
    /// on this).
    fn turn_end_substrate(&self) -> Option<TurnEndSubstrate> {
        None
    }
    /// The usage-limit menu anchor, or `None` (the sweep's usage-limit probe is
    /// gated on this).
    fn usage_limit_anchor(&self) -> Option<UsageLimitAnchor> {
        None
    }
    /// The sandbox staging floor, or `None` (a sandboxed Run says its absence
    /// once).
    fn staging_floor(&self) -> Option<StagingFloor> {
        None
    }
    /// The context-usage source, or `None` (Performance shows no Context column
    /// for this harness). #585.
    fn context_usage_source(&self) -> Option<ContextUsageSource> {
        None
    }

    // These are the methods a generic consumer actually calls. The default is
    // "absent" — a data-declared harness ([`NullProbes`]) resolves no transcript,
    // never constates an end of turn, never matches a usage-limit menu — so a
    // caller that dispatches through [`resolved`] can never reach `claude`'s
    // implementation for a harness that did not declare it.

    /// Resolve this node's transcript file on disk, or `None`.
    ///
    /// Gated by [`Self::transcript_resolution`] being present; the default (a
    /// harness whose store PDO cannot map) returns `None`, so a cost or turn-end
    /// read finds no file and the consumer treats it as "no signal". `claude`
    /// resolves by pinned session id (`<uuid>.jsonl`), else newest-mtime.
    fn resolve_transcript(
        &self,
        _projects_root: &Path,
        _working_dir: &Path,
        _session_id: Option<&str>,
    ) -> Option<PathBuf> {
        None
    }

    /// Whether the transcript `tail` shows this harness's end-of-turn signature.
    ///
    /// The default is `false` (no substrate ⇒ never a constated end of turn). This
    /// is the parser that must be **this harness's own**: reading `claude`'s JSONL
    /// turn-state on another harness's store is exactly the ADR-0051 regression.
    fn classify_turn_ended(&self, _tail: &str) -> bool {
        false
    }

    /// Whether the captured `pane` shows this harness's usage-limit menu.
    ///
    /// The default is `false` (no anchor ⇒ the wording is proper to another
    /// harness, so a generic consumer never matches it). `claude` matches its
    /// interactive "stop and wait for limit to reset" menu.
    fn detect_usage_limit(&self, _pane: &str) -> bool {
        false
    }

    /// Whether this harness's **process exit is a verdict** on the turn's success.
    ///
    /// Default `true`: for `claude` (and a data-declared harness), the session
    /// dying IS the failure signal (ADR-0032), so a consumer need not read the
    /// journal to explain a death. `copilot` overrides it to `false`: it **exits 0
    /// on a hard model failure** (ADR-0052), so the exit is not a verdict and the
    /// journal must be consulted ([`Self::classify_hard_error`]). Gates that read,
    /// so a harness whose death speaks for itself pays no journal I/O on death.
    fn exit_code_is_verdict(&self) -> bool {
        true
    }

    /// The **hard error** this harness's transcript `tail` carries, if any — a
    /// failure the harness's *exit code* cannot report (#615, ADR-0052). The
    /// default is `None` (a harness whose store PDO cannot map, or one whose exit
    /// code IS its verdict). `copilot` overrides it: it **exits 0 on a hard model
    /// failure**, so the exit code is not a verdict; the journal's `session.error`
    /// is. A generic consumer reads this to say the failure *as such* rather than
    /// off a code that lies.
    fn classify_hard_error(&self, _tail: &str) -> Option<String> {
        None
    }

    /// This harness's context-window peak for one transcript/journal's `text`, in
    /// tokens (#585). The default is `None` — a harness with no
    /// [`Self::context_usage_source`] never reaches a parser at all, so
    /// `crate::stats_performance` never has to name `claude`'s or `copilot`'s
    /// parser itself (that was exactly the ADR-0051 regression: a generic caller
    /// `match`ing on the harness string to pick a parsing function). `claude`
    /// dispatches to [`crate::context_peak::claude_session_peak`], `copilot` to
    /// [`crate::context_peak::copilot_session_peak`] — each pure, injected text
    /// in, `Option<u64>` out.
    fn context_peak(&self, _text: &str) -> Option<u64> {
        None
    }

    /// This session's own **subagent** transcripts — declared-group discovery
    /// under one main session, for Stats → Performance's subagent breakdown
    /// (#585, issue user stories #27/#28/#36). Each entry is `(file_stem,
    /// transcript_text)`; the caller (`crate::stats_performance`) decides the
    /// declared-group label from the stem — this method's only job is "where do
    /// this session's subagent transcripts live, if this harness has that
    /// concept at all".
    ///
    /// The default is an **empty `Vec`**, not a `match harness { .. }` the caller
    /// has to write: a harness with no nested-subagent convention (every harness
    /// but `claude` today) answers "none" from the dispatch itself, so its
    /// absence is a value, never a silently-skipped branch (ADR-0051 §2). A
    /// future harness that DOES expose declared subagent identity overrides this
    /// with its own discovery — never a shared heuristic.
    fn subagent_transcripts(
        &self,
        _project_root: &Path,
        _working_dir: &Path,
        _session_id: &str,
    ) -> Vec<(String, String)> {
        Vec::new()
    }
}

/// The `claude` capabilities — all five, exactly as they are today. This slice is
/// a **dispatch refactor**, not a behaviour change: every method returns the
/// mechanism `claude` has always used.
struct ClaudeProbes;

impl HarnessProbes for ClaudeProbes {
    fn cost_source(&self) -> Option<CostSource> {
        Some(CostSource::DerivedFromTranscript)
    }
    fn transcript_resolution(&self) -> Option<TranscriptResolution> {
        Some(TranscriptResolution::ClaudeJsonl)
    }
    fn turn_end_substrate(&self) -> Option<TurnEndSubstrate> {
        Some(TurnEndSubstrate::ClaudeTranscript)
    }
    fn usage_limit_anchor(&self) -> Option<UsageLimitAnchor> {
        Some(UsageLimitAnchor::ClaudePaneMenu)
    }
    fn staging_floor(&self) -> Option<StagingFloor> {
        Some(StagingFloor::ClaudeDotClaude)
    }

    fn context_usage_source(&self) -> Option<ContextUsageSource> {
        Some(ContextUsageSource::ClaudeTranscriptPeak)
    }

    /// `claude`'s transcript resolution: by the pinned session id when the node
    /// recorded one (`<uuid>.jsonl` — this node's own transcript, #473), else the
    /// legacy newest-mtime pick for a pre-#473 row. This is the sole reachable
    /// caller of [`crate::stale_detector::session_jsonl_by_id`] /
    /// [`crate::stale_detector::find_session_jsonl`] from outside the sweep — they
    /// are `claude`'s implementation now, not a generic transcript resolver.
    fn resolve_transcript(
        &self,
        projects_root: &Path,
        working_dir: &Path,
        session_id: Option<&str>,
    ) -> Option<PathBuf> {
        match session_id {
            Some(sid) => {
                crate::stale_detector::session_jsonl_by_id(projects_root, working_dir, sid)
            }
            None => crate::stale_detector::find_session_jsonl(projects_root, working_dir),
        }
    }

    fn classify_turn_ended(&self, tail: &str) -> bool {
        crate::stale_detector::parse_turn_state(tail) == crate::stale_detector::TurnState::TurnEnded
    }

    fn detect_usage_limit(&self, pane: &str) -> bool {
        crate::stale_detector::detect_usage_limit(pane)
    }

    fn context_peak(&self, text: &str) -> Option<u64> {
        crate::context_peak::claude_session_peak(text)
    }

    /// `claude` is the only harness with a confirmed nested-subagent convention
    /// today (#585): `<project_root>/<encoded_cwd>/<session_id>/subagents/`.
    fn subagent_transcripts(
        &self,
        project_root: &Path,
        working_dir: &Path,
        session_id: &str,
    ) -> Vec<(String, String)> {
        let dir = project_root
            .join(crate::run_cost::cc_project_dirname(working_dir))
            .join(session_id)
            .join("subagents");
        let mut out = Vec::new();
        collect_jsonl_stems(&dir, &mut out);
        out
    }
}

/// Recurse `dir`, pairing every `*.jsonl` file's stem with its text — the raw
/// discovery step behind [`ClaudeProbes::subagent_transcripts`]. No grouping
/// heuristic lives here: labelling a stem as a declared group or falling back to
/// "Unidentified subagent" is `crate::stats_performance`'s own concern, not a
/// harness capability.
fn collect_jsonl_stems(dir: &Path, out: &mut Vec<(String, String)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl_stems(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if let Ok(text) = std::fs::read_to_string(&path) {
                out.push((stem.to_string(), text));
            }
        }
    }
}

/// The single `claude` instance handed out by [`probes_for`]. Zero-sized, so this
/// `static` costs nothing and needs no lazy init.
static CLAUDE_PROBES: ClaudeProbes = ClaudeProbes;

/// The `copilot` capabilities (#615, ADR-0051/0052) — **three** present (a
/// reported cost, a transcript resolution, an end-of-turn substrate), **two**
/// declared absent with their motive:
///
/// - the **usage-limit menu anchor** is absent: it is an informational probe whose
///   own documentation admits the textual anchor drifts each version, and it
///   triggers no recovery (ADR-0012) — a second harness declaring it absent
///   degrades nothing actionable (ADR-0051 §"Limites");
/// - the **staging floor** is absent: configuring a harness is a documented
///   prerequisite, not PDO code (ADR-0031 / CONTEXT.md § "Harnais agentique").
///
/// The three present capabilities dispatch to `copilot`'s own implementation — its
/// event journal, never `claude`'s cwd-keyed JSONL store.
///
/// `subagent_transcripts` is left at the trait's default (empty) — investigated,
/// not assumed: `copilot`'s journal declares no delegate/subagent event kind
/// ([`crate::copilot_journal`]'s module doc lists all four it does carry), and its
/// store is flat (`<store>/<session-id>/events.jsonl`, no directory nesting under
/// a session to enumerate).
struct CopilotProbes;

impl HarnessProbes for CopilotProbes {
    /// A **reported** cost (ADR-0052): the harness counts itself, PDO converts by a
    /// published constant. Distinct from `claude`'s derived cost — never through the
    /// price table.
    fn cost_source(&self) -> Option<CostSource> {
        Some(CostSource::ReportedByConstant)
    }
    fn transcript_resolution(&self) -> Option<TranscriptResolution> {
        Some(TranscriptResolution::CopilotEventsJsonl)
    }
    fn turn_end_substrate(&self) -> Option<TurnEndSubstrate> {
        Some(TurnEndSubstrate::CopilotEventJournal)
    }
    // usage_limit_anchor / staging_floor stay `None` — declared absent (see above).

    fn context_usage_source(&self) -> Option<ContextUsageSource> {
        Some(ContextUsageSource::CopilotJournalPeak)
    }

    /// `copilot`'s transcript resolution: the session's event journal, at
    /// `<store>/<session-id>/events.jsonl` — by the **session identity PDO
    /// imposed**, ignoring the working directory (#615). Without a pinned session id
    /// there is no journal to resolve (`copilot` never blind-continues, so a live
    /// node always has one), so this returns `None`.
    fn resolve_transcript(
        &self,
        store_root: &Path,
        _working_dir: &Path,
        session_id: Option<&str>,
    ) -> Option<PathBuf> {
        let sid = session_id?;
        if sid.is_empty() {
            return None;
        }
        Some(store_root.join(sid).join("events.jsonl"))
    }

    /// A journal trailing on a `session.error` (a hard failure the harness exits 0
    /// on) is **not** a finished turn, so an errored node is never auto-completed.
    fn classify_turn_ended(&self, tail: &str) -> bool {
        crate::copilot_journal::turn_ended(tail)
    }

    fn context_peak(&self, text: &str) -> Option<u64> {
        crate::context_peak::copilot_session_peak(text)
    }

    /// `copilot` exits 0 on a hard model failure (ADR-0052), so its exit is not a
    /// verdict — the journal is.
    fn exit_code_is_verdict(&self) -> bool {
        false
    }

    /// The journal's trailing `session.error` — the signal the harness's exit code
    /// (zero) cannot give.
    fn classify_hard_error(&self, tail: &str) -> Option<String> {
        crate::copilot_journal::hard_error(tail)
    }
}

/// The single `copilot` instance handed out by [`probes_for`]. Zero-sized.
static COPILOT_PROBES: CopilotProbes = CopilotProbes;

/// The `pi` capabilities (#705, story #702; ADR-0051) — **all six declared absent**,
/// explicitly, in this ticket: cost, transcript resolution, end-of-turn substrate,
/// usage-limit anchor, context usage, staging set. `pi` is first-party (it has a
/// row in the support table and code here) but its instrumentation lands in the
/// follow-up tickets — the reported dollar cost from the session JSONL's
/// `usage.cost.total` (CONTEXT.md § "Coût rapporté en dollars", ADR-0052), the
/// turn-end extension on `agent_settled` (#707, CONTEXT.md § "Extension de fin de
/// tour"), the staging set (ADR-0063). Until then each is a `None` a reader can see
/// — in the table, in Stats ("—" with its reason), in the sandbox note — never a
/// `$0`, never a silent no-op.
///
/// Why a type rather than the [`NullProbes`] default: ADR-0051 §2 — `None` is a
/// *declared* value. Falling through to the data-declared arm would make pi's
/// absences indistinguishable from "PDO carries no code for this name", and the
/// support table would have no row to publish.
struct PiProbes;

impl HarnessProbes for PiProbes {
    // Every capability method stays at the trait's `None` default — declared here
    // by naming each one, so the next ticket has to *change* a line, not add one.
    fn cost_source(&self) -> Option<CostSource> {
        None
    }
    fn transcript_resolution(&self) -> Option<TranscriptResolution> {
        None
    }
    fn turn_end_substrate(&self) -> Option<TurnEndSubstrate> {
        None
    }
    fn usage_limit_anchor(&self) -> Option<UsageLimitAnchor> {
        None
    }
    fn staging_floor(&self) -> Option<StagingFloor> {
        None
    }
    fn context_usage_source(&self) -> Option<ContextUsageSource> {
        None
    }
}

/// The single `pi` instance handed out by [`probes_for`]. Zero-sized.
static PI_PROBES: PiProbes = PiProbes;

/// The capabilities of a **data-declared** harness (a user's disk descriptor, or
/// `opencode` in v1): every method inherits the trait's "absent" default. This is
/// the dispatch target that makes ADR-0051 §2 hold — a harness PDO carries no code
/// for still resolves to *something*, and that something answers "absent" on every
/// capability rather than routing to `claude`'s implementation.
struct NullProbes;
impl HarnessProbes for NullProbes {}

/// The single all-absent instance handed out by [`resolved`] for any harness with
/// no code. Zero-sized, like [`CLAUDE_PROBES`].
static NULL_PROBES: NullProbes = NullProbes;

/// The dispatch target for `harness` — **total**: `claude` gets [`ClaudeProbes`],
/// every other name (a data-declared harness) gets [`NullProbes`]. Never `None`:
/// there is always a dispatch, and absence is a method answering `None`/`false`,
/// not a missing implementation (ADR-0051 §2). This is what a generic consumer
/// holds instead of naming a claude-proper function.
pub(crate) fn resolved(harness: &str) -> &'static dyn HarnessProbes {
    match probes_for(harness) {
        Some(p) => p,
        None => &NULL_PROBES,
    }
}

/// Resolve `harness`'s transcript file, dispatched to its implementation (ADR-0051).
/// A data-declared harness resolves `None` — never `claude`'s `<uuid>.jsonl` path.
pub(crate) fn resolve_transcript(
    harness: &str,
    projects_root: &Path,
    working_dir: &Path,
    session_id: Option<&str>,
) -> Option<PathBuf> {
    resolved(harness).resolve_transcript(projects_root, working_dir, session_id)
}

/// `harness`'s context-window peak for this transcript/journal `text`, dispatched
/// to its implementation (ADR-0051 §585) — the seam
/// [`crate::stats_performance`] calls instead of matching the harness string
/// itself to pick `claude_session_peak` vs `copilot_session_peak`. A harness with
/// no [`HarnessProbes::context_usage_source`] answers `None`.
pub(crate) fn context_peak(harness: &str, text: &str) -> Option<u64> {
    resolved(harness).context_peak(text)
}

/// `harness`'s subagent transcripts for one main session, dispatched to its
/// implementation (ADR-0051 §585) — `claude` discovers them under
/// `subagents/`, every other harness answers an empty `Vec` (a value, not a
/// silently-skipped branch). [`crate::stats_performance`] applies its own
/// declared-group labelling to whatever this returns.
pub(crate) fn subagent_transcripts(
    harness: &str,
    project_root: &Path,
    working_dir: &Path,
    session_id: &str,
) -> Vec<(String, String)> {
    resolved(harness).subagent_transcripts(project_root, working_dir, session_id)
}

/// Whether `harness` constates an end of turn from this transcript `tail`,
/// dispatched to its implementation (ADR-0051). A harness with no substrate
/// answers `false` — never `claude`'s JSONL parser on a foreign store.
pub(crate) fn turn_ended(harness: &str, tail: &str) -> bool {
    resolved(harness).classify_turn_ended(tail)
}

/// Whether `harness`'s usage-limit menu is showing in this `pane`, dispatched to
/// its implementation (ADR-0051). A harness with no anchor answers `false`.
pub(crate) fn usage_limit_shown(harness: &str, pane: &str) -> bool {
    resolved(harness).detect_usage_limit(pane)
}

/// The hard error `harness`'s transcript `tail` carries, dispatched to its
/// implementation (#615, ADR-0052). A harness whose exit code IS its verdict (or
/// one PDO carries no code for) answers `None`; `copilot` answers with its
/// journal's trailing `session.error`, because it exits 0 on a hard failure.
pub(crate) fn hard_error(harness: &str, tail: &str) -> Option<String> {
    resolved(harness).classify_hard_error(tail)
}

/// Whether `harness`'s process exit is a verdict on the turn (ADR-0032) — `true`
/// for `claude`, `false` for `copilot` (which exits 0 on a hard failure, ADR-0052).
/// A death consumer gates its journal read on the negation, so a harness whose
/// death speaks for itself pays no extra I/O.
pub(crate) fn exit_code_is_verdict(harness: &str) -> bool {
    resolved(harness).exit_code_is_verdict()
}

/// Whether `harness`'s `{settings}` hole takes PDO's **claude-format settings
/// file** — the `Stop`-hook JSON of #433 and the library assistant's focus-hook
/// JSON (#705).
///
/// The hole means "an injected settings file" (ADR-0043), but the *format* of what
/// PDO writes is claude's. `pi` has the hole too and fills it with `-e <extension>`
/// (CONTEXT.md § "Extension de fin de tour"): handed the claude JSON, pi would load
/// it as an extension and refuse to start. So the writers ask here before writing:
///
/// - a **first-party** harness takes the file only if its end-of-turn substrate is
///   claude's transcript (the `Stop` hook is that substrate) — `claude` yes, `pi`
///   no (its substrate is `None` until #707, and will be its own extension then);
/// - a **data-declared** harness (no probes) keeps the pre-#705 behaviour: a hole
///   means the file — a user wrapping `claude` in their own descriptor still gets
///   the hook.
///
/// A dispatch point, not a presence guard (ADR-0051): the answer is read off the
/// harness's declared substrate, never off its name.
pub(crate) fn settings_hole_takes_claude_file(harness: &str) -> bool {
    match probes_for(harness) {
        None => true,
        Some(p) => matches!(
            p.turn_end_substrate(),
            Some(TurnEndSubstrate::ClaudeTranscript)
        ),
    }
}

/// The one-time note for a node whose harness has **no turn-end substrate** while
/// turn-end auto-completion is enabled (ADR-0051 / correctif AC #7). `Some(msg)`
/// when the setting cannot be honoured for `harness`, `None` for a harness that
/// has the substrate (`claude`). Pure and testable, the twin of
/// [`staging_floor_absence_note`]: the setting stops being a silent no-op.
pub(crate) fn turn_end_absence_note(harness: &str) -> Option<String> {
    if capabilities(harness).turn_end {
        return None;
    }
    Some(format!(
        "turn-end auto-completion is enabled but harness `{harness}` has no end-of-turn substrate \
         (#613, ADR-0051) — this node will not be auto-completed on turn end; complete it by \
         signalling `pdo complete` or leave it attached"
    ))
}

/// The capabilities of `harness`, or `None` for a harness PDO carries no code for.
///
/// `claude` is the only harness with any capability today; `opencode` and every
/// **data-declared** harness (a user's disk descriptor) get `None` — "no
/// capability" — which is exactly what makes the absence free and un-forgettable.
/// A caller that wants per-capability booleans uses [`capabilities`], which reads
/// the same `None` as "all absent".
pub(crate) fn probes_for(harness: &str) -> Option<&'static dyn HarnessProbes> {
    match harness {
        harness_registry::CLAUDE => Some(&CLAUDE_PROBES),
        // #615: `copilot`'s three capabilities (reported cost, transcript, turn-end)
        // — the second first-party harness. Its two others are declared absent.
        harness_registry::COPILOT => Some(&COPILOT_PROBES),
        // #705: `pi` — first-party, every capability explicitly absent for now.
        harness_registry::PI => Some(&PI_PROBES),
        // `opencode` (resident but un-instrumented in v1) and every data-declared
        // harness: no capability. A launch is data; a capability is code (ADR-0045).
        _ => None,
    }
}

/// The five capabilities of a harness as plain booleans — the read-friendly view
/// the gates consult. A harness with no probes (`None` from [`probes_for`]) is
/// absent on all five, so a data-declared harness resolves to
/// [`Capabilities::NONE`] without any per-harness code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Capabilities {
    pub cost: bool,
    pub transcript: bool,
    pub turn_end: bool,
    pub usage_limit: bool,
    pub staging: bool,
    pub context_usage: bool,
}

impl Capabilities {
    /// Every capability absent — a data-declared harness, un-instrumented and
    /// blind, but still launchable/attachable/completable (ADR-0045).
    pub(crate) const NONE: Capabilities = Capabilities {
        cost: false,
        transcript: false,
        turn_end: false,
        usage_limit: false,
        staging: false,
        context_usage: false,
    };
}

/// Resolve `harness` to its six capability booleans. `None` from the factory ⇒
/// [`Capabilities::NONE`], so an unknown or data-declared harness is absent on all
/// six — the property the whole slice rests on.
pub(crate) fn capabilities(harness: &str) -> Capabilities {
    match probes_for(harness) {
        None => Capabilities::NONE,
        Some(p) => Capabilities {
            cost: p.cost_source().is_some(),
            transcript: p.transcript_resolution().is_some(),
            turn_end: p.turn_end_substrate().is_some(),
            usage_limit: p.usage_limit_anchor().is_some(),
            staging: p.staging_floor().is_some(),
            context_usage: p.context_usage_source().is_some(),
        },
    }
}

/// Whether PDO can derive a Run's cost for `harness`: it needs both a cost source
/// and a way to find the transcript that source reads. A data-declared harness has
/// neither, so its Run's cost is "—" with a reason rather than a silent `$0`.
pub(crate) fn can_cost(harness: &str) -> bool {
    let c = capabilities(harness);
    c.cost && c.transcript
}

/// Whether PDO can measure `harness`'s context-window peak (#585): it needs both
/// a context-usage source and a way to find the transcript that source reads.
/// Mirrors [`can_cost`]'s shape — a harness with a source but no transcript
/// resolution (impossible today, but not structurally excluded) would still read
/// "absent", never a made-up zero.
pub(crate) fn can_measure_context(harness: &str) -> bool {
    let c = capabilities(harness);
    c.context_usage && c.transcript
}

/// The one-time note for a sandboxed Run whose node runs on a harness with no
/// staging floor (ADR-0031). `Some(msg)` when `harness` lacks the floor,
/// `None` when it has it (`claude`). Pure and testable, modelled on
/// `price_table::diagnostic`.
pub(crate) fn staging_floor_absence_note(harness: &str) -> Option<String> {
    if capabilities(harness).staging {
        return None;
    }
    Some(format!(
        "sandbox: harness `{harness}` has no staging floor (#553, ADR-0031) — this Run's session \
         holds only by the profile's image and its `$HOME` exceptions, without the plancher's \
         guarantees (credentials, org managed settings, pre-granted trust, empty projects/)"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness_registry::{CLAUDE, COPILOT, OPENCODE, PI};

    /// A struct that overrides nothing: the demonstration that **every trait
    /// method defaults to "absent"**. This is the shape of a harness declared in
    /// data — it has no code, so it inherits every default.
    struct BareHarness;
    impl HarnessProbes for BareHarness {}

    #[test]
    fn every_default_is_absent() {
        let bare = BareHarness;
        assert!(bare.cost_source().is_none());
        assert!(bare.transcript_resolution().is_none());
        assert!(bare.turn_end_substrate().is_none());
        assert!(bare.usage_limit_anchor().is_none());
        assert!(bare.staging_floor().is_none());
        assert!(bare.context_peak("anything").is_none());
        assert!(bare
            .subagent_transcripts(Path::new("/root"), Path::new("/wd"), "sid")
            .is_empty());
    }

    #[test]
    fn claude_keeps_its_five_capabilities() {
        // The control: `claude`'s five capabilities are intact — this slice is a
        // dispatch refactor, not a behaviour change.
        let p = probes_for(CLAUDE).expect("claude has probes");
        assert_eq!(p.cost_source(), Some(CostSource::DerivedFromTranscript));
        assert_eq!(
            p.transcript_resolution(),
            Some(TranscriptResolution::ClaudeJsonl)
        );
        assert_eq!(
            p.turn_end_substrate(),
            Some(TurnEndSubstrate::ClaudeTranscript)
        );
        assert_eq!(
            p.usage_limit_anchor(),
            Some(UsageLimitAnchor::ClaudePaneMenu)
        );
        assert_eq!(p.staging_floor(), Some(StagingFloor::ClaudeDotClaude));
    }

    #[test]
    fn claude_capabilities_are_all_present() {
        let c = capabilities(CLAUDE);
        assert_eq!(
            c,
            Capabilities {
                cost: true,
                transcript: true,
                turn_end: true,
                usage_limit: true,
                staging: true,
                context_usage: true,
            }
        );
        assert!(can_cost(CLAUDE));
    }

    #[test]
    fn claude_and_copilot_declare_context_usage_others_do_not() {
        assert_eq!(
            probes_for(CLAUDE).unwrap().context_usage_source(),
            Some(ContextUsageSource::ClaudeTranscriptPeak)
        );
        assert_eq!(
            probes_for(COPILOT).unwrap().context_usage_source(),
            Some(ContextUsageSource::CopilotJournalPeak)
        );
        assert!(capabilities(CLAUDE).context_usage);
        assert!(capabilities(COPILOT).context_usage);
        assert!(!capabilities(OPENCODE).context_usage);
        assert!(!capabilities("never-seen").context_usage);
    }

    #[test]
    fn copilot_has_its_three_capabilities_and_declares_two_absent() {
        let p = probes_for(COPILOT).expect("copilot has probes");
        assert_eq!(p.cost_source(), Some(CostSource::ReportedByConstant));
        assert_eq!(
            p.transcript_resolution(),
            Some(TranscriptResolution::CopilotEventsJsonl)
        );
        assert_eq!(
            p.turn_end_substrate(),
            Some(TurnEndSubstrate::CopilotEventJournal)
        );
        // Declared absent, with their motive (see `CopilotProbes` doc).
        assert!(
            p.usage_limit_anchor().is_none(),
            "usage-limit declared absent"
        );
        assert!(p.staging_floor().is_none(), "staging floor declared absent");

        assert_eq!(
            capabilities(COPILOT),
            Capabilities {
                cost: true,
                transcript: true,
                turn_end: true,
                usage_limit: false,
                staging: false,
                context_usage: true,
            }
        );
        // A reported cost is still a cost PDO can produce (source + a resolvable
        // journal), so a copilot Run is costable — never "—" for lack of a source.
        assert!(can_cost(COPILOT));
    }

    #[test]
    fn pi_is_first_party_and_declares_every_capability_absent() {
        // #705 / ADR-0051: `pi` has probes (a row in the support table) and answers
        // `None` on all six — an explicit absence, not a missing dispatch.
        let p = probes_for(PI).expect("pi has probes (first-party)");
        assert!(p.cost_source().is_none(), "cost declared absent");
        assert!(
            p.transcript_resolution().is_none(),
            "transcript declared absent"
        );
        assert!(
            p.turn_end_substrate().is_none(),
            "end of turn declared absent"
        );
        assert!(
            p.usage_limit_anchor().is_none(),
            "usage-limit anchor declared absent"
        );
        assert!(
            p.staging_floor().is_none(),
            "staging declared absent (ADR-0063 later)"
        );
        assert!(
            p.context_usage_source().is_none(),
            "context usage declared absent"
        );
        assert_eq!(capabilities(PI), Capabilities::NONE);
        // Consequences a reader sees: Stats says "—" (never $0), no turn-end probe,
        // and the turn-end setting says so once instead of silently doing nothing.
        assert!(!can_cost(PI));
        assert!(!can_measure_context(PI));
        assert!(turn_end_absence_note(PI).is_some());
        assert!(staging_floor_absence_note(PI).is_some());
        // Behaviour stays absent too: never claude's parsers on pi's store.
        assert!(!turn_ended(PI, "{\"type\":\"assistant\"}"));
        assert!(!usage_limit_shown(PI, "wait for limit to reset"));
        assert!(exit_code_is_verdict(PI));
    }

    #[test]
    fn the_claude_settings_file_goes_only_to_a_hole_that_takes_it() {
        // #705: `claude` takes the Stop-hook JSON; `pi` has a `{settings}` hole but
        // fills it with `-e <extension>`, so the claude file must never be written
        // for it; a data-declared harness keeps hole ⇒ file.
        assert!(settings_hole_takes_claude_file(CLAUDE));
        assert!(!settings_hole_takes_claude_file(PI));
        assert!(!settings_hole_takes_claude_file(COPILOT));
        assert!(settings_hole_takes_claude_file("my-claude-wrapper"));
    }

    #[test]
    fn copilot_resolves_its_journal_by_session_identity_ignoring_cwd() {
        // #615 AC: resolved from the imposed session identity, no cwd encoding — so
        // two nodes sharing a worktree get distinct journals.
        let p = probes_for(COPILOT).unwrap();
        let a = p
            .resolve_transcript(Path::new("/store"), Path::new("/shared/wt"), Some("sid-a"))
            .unwrap();
        let b = p
            .resolve_transcript(Path::new("/store"), Path::new("/shared/wt"), Some("sid-b"))
            .unwrap();
        assert_eq!(a, PathBuf::from("/store/sid-a/events.jsonl"));
        assert_eq!(b, PathBuf::from("/store/sid-b/events.jsonl"));
        assert_ne!(a, b, "distinct sessions ⇒ distinct journals, same worktree");
        // No pinned identity ⇒ no journal (copilot never blind-continues).
        assert!(p
            .resolve_transcript(Path::new("/store"), Path::new("/wt"), None)
            .is_none());
    }

    #[test]
    fn copilot_turn_end_dispatches_to_its_journal_parser_not_claudes() {
        let copilot_tail =
            "{\"type\":\"assistant.turn_start\",\"data\":{}}\n{\"type\":\"assistant.turn_end\",\"data\":{}}\n";
        assert!(turn_ended(COPILOT, copilot_tail));
        assert!(
            !turn_ended(CLAUDE, copilot_tail),
            "not claude's JSONL shape"
        );
        // A trailing hard error is not a finished turn (harness exits 0 on it).
        let errored =
            "{\"type\":\"assistant.turn_start\",\"data\":{}}\n{\"type\":\"session.error\",\"data\":{\"message\":\"boom\"}}\n";
        assert!(!turn_ended(COPILOT, errored));
    }

    #[test]
    fn a_data_declared_harness_is_absent_on_all_five() {
        // AC: a harness declared in data returns "absent" on the five. `opencode`
        // (embedded but un-instrumented in v1) and any never-seen name both take
        // the factory's `None`.
        for name in [OPENCODE, "my-custom-harness", "not-a-harness"] {
            assert!(probes_for(name).is_none(), "{name} must have no probes");
            assert_eq!(capabilities(name), Capabilities::NONE, "{name}");
            assert!(!can_cost(name), "{name} cannot be costed");
        }
    }

    /// A fictional harness that **declares its own implementation** of three
    /// capabilities. It is the negative image of `claude`: it resolves a transcript
    /// to a fixed sentinel path, calls a turn ended on a marker `claude` would never
    /// emit, and matches its own usage-limit wording. If dispatch ever fell back to
    /// `claude` for a declared harness, every assertion below would flip.
    struct TestProbes;
    impl HarnessProbes for TestProbes {
        fn transcript_resolution(&self) -> Option<TranscriptResolution> {
            Some(TranscriptResolution::ClaudeJsonl) // marker present ⇒ capability declared
        }
        fn turn_end_substrate(&self) -> Option<TurnEndSubstrate> {
            Some(TurnEndSubstrate::ClaudeTranscript)
        }
        fn resolve_transcript(
            &self,
            _projects_root: &Path,
            _working_dir: &Path,
            _session_id: Option<&str>,
        ) -> Option<PathBuf> {
            Some(PathBuf::from("/test-harness/own-transcript.log"))
        }
        fn classify_turn_ended(&self, tail: &str) -> bool {
            tail == "TEST-HARNESS-DONE"
        }
        fn detect_usage_limit(&self, pane: &str) -> bool {
            pane.contains("test-harness rate limit")
        }
    }

    #[test]
    fn a_declared_implementation_gets_its_own_behaviour_not_claudes() {
        // AC #4, the regression this ticket makes impossible: a harness that
        // declares an implementation is dispatched to ITS behaviour, never claude's.
        let t = TestProbes;

        // Its turn-end parser fires on ITS marker and rejects what claude would
        // accept (a real `assistant`-terminated JSONL tail).
        assert!(t.classify_turn_ended("TEST-HARNESS-DONE"));
        let claude = probes_for(CLAUDE).unwrap();
        // The claude parser would NOT call this tail ended (not valid JSONL), and
        // the test harness's own parser is what runs — the two disagree, which is
        // the whole point.
        assert!(!claude.classify_turn_ended("TEST-HARNESS-DONE"));

        // Its transcript resolves to its own path, not a `<uuid>.jsonl` under the
        // claude projects root.
        let p = t
            .resolve_transcript(Path::new("/proj"), Path::new("/wd"), Some("abc"))
            .unwrap();
        assert_eq!(p, PathBuf::from("/test-harness/own-transcript.log"));

        // Its usage-limit anchor is its own wording; claude's menu text does not
        // match it and vice-versa.
        assert!(t.detect_usage_limit("test-harness rate limit reached"));
        assert!(!t.detect_usage_limit("Stop and wait for limit to reset"));
    }

    #[test]
    fn a_data_declared_harness_dispatches_to_absent_never_to_claude() {
        // AC #1/#4: the generic by-name dispatch for a harness PDO carries no code
        // for resolves to "absent" on every behaviour — it must NOT reach claude's
        // implementation. This is the path a liveness sweep takes.
        for name in [OPENCODE, "pi", "not-a-harness"] {
            assert!(
                resolve_transcript(name, Path::new("/proj"), Path::new("/wd"), Some("abc"))
                    .is_none(),
                "{name}: no transcript resolution leaks from claude"
            );
            // A tail claude WOULD call ended is not enough — a data-declared harness
            // answers `false`, so no node on it is ever auto-completed.
            assert!(
                !turn_ended(name, FIXTURE_CLAUDE_TURN_ENDED),
                "{name}: no turn-end via claude's parser"
            );
            assert!(
                !usage_limit_shown(name, "Stop and wait for limit to reset"),
                "{name}: no usage-limit match via claude's anchor"
            );
        }
    }

    #[test]
    fn claude_dispatches_to_its_own_behaviour() {
        // The control: `claude`'s by-name dispatch DOES run its parser/anchor — this
        // slice is a dispatch refactor, not a behaviour change (AC #5).
        assert!(turn_ended(CLAUDE, FIXTURE_CLAUDE_TURN_ENDED));
        assert!(usage_limit_shown(
            CLAUDE,
            "❯ 1. Stop and wait for limit to reset"
        ));
    }

    #[test]
    fn resolved_is_total_and_absence_is_a_value_not_a_missing_dispatch() {
        // ADR-0051 §2: every name resolves to *some* dispatch; "absent" is a method
        // answering None/false on that dispatch, distinguishable from claude's.
        assert!(resolved(CLAUDE).cost_source().is_some());
        assert!(resolved(OPENCODE).cost_source().is_none());
        assert!(resolved("never-seen").turn_end_substrate().is_none());
    }

    /// A minimal, valid claude JSONL tail whose last substantial record is an
    /// `assistant` message with no pending `tool_use` — the one shape
    /// [`crate::stale_detector::parse_turn_state`] calls `TurnEnded`.
    const FIXTURE_CLAUDE_TURN_ENDED: &str = concat!(
        r#"{"type":"user","message":{"role":"user","content":"hi"}}"#,
        "\n",
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"done"}]}}"#,
        "\n"
    );

    #[test]
    fn turn_end_absence_note_fires_only_for_a_harness_without_the_substrate() {
        assert_eq!(turn_end_absence_note(CLAUDE), None);
        let note = turn_end_absence_note("pi").expect("a note for a substrate-less harness");
        assert!(note.contains("`pi`"));
        assert!(note.contains("no end-of-turn substrate"));
        assert!(note.contains("not be auto-completed"));
    }

    #[test]
    fn staging_floor_absence_note_fires_only_for_a_harness_without_the_floor() {
        assert_eq!(staging_floor_absence_note(CLAUDE), None);
        let note = staging_floor_absence_note("my-custom-harness").expect("a note");
        assert!(note.contains("`my-custom-harness`"));
        assert!(note.contains("no staging floor"));
        assert!(note.contains("$HOME"));
    }

    fn claude_turn(id: &str, input: u64, output: u64) -> String {
        serde_json::json!({
            "type": "assistant",
            "requestId": format!("r-{id}"),
            "message": {
                "id": id,
                "model": "claude-opus-4-8",
                "usage": { "input_tokens": input, "output_tokens": output }
            }
        })
        .to_string()
    }

    fn copilot_checkpoint(input: u64, output: u64) -> String {
        serde_json::json!({
            "type": "session.usage_checkpoint",
            "data": {
                "totalNanoAiu": 1,
                "usage": { "inputTokens": input, "outputTokens": output, "cacheReadTokens": 0, "cacheCreationTokens": 0 }
            }
        })
        .to_string()
    }

    #[test]
    fn context_peak_dispatches_to_each_harnesss_own_parser_not_the_others() {
        // The public dispatch (`crate::harness_probes::context_peak`), not the
        // `ClaudeProbes`/`CopilotProbes` methods directly — this is exactly the
        // seam `crate::stats_performance` calls instead of a hard-coded
        // `match harness.as_str() { .. }` (ADR-0051 review follow-up).
        let claude_text = claude_turn("m1", 100, 20);
        assert_eq!(context_peak(CLAUDE, &claude_text), Some(120));
        // Claude's parser on a Copilot-shaped journal finds nothing (proves the
        // dispatch, not a shared heuristic, is what's under test).
        let copilot_text = copilot_checkpoint(500, 100);
        assert_eq!(context_peak(CLAUDE, &copilot_text), None);
        assert_eq!(context_peak(COPILOT, &copilot_text), Some(600));
        // A harness with no context-usage source answers `None` from the
        // dispatch itself, never a guess.
        assert_eq!(context_peak(OPENCODE, &claude_text), None);
        assert_eq!(context_peak("never-seen", &claude_text), None);
    }

    #[test]
    fn subagent_transcripts_dispatches_only_claude_to_its_directory_convention() {
        let tmp = tempfile::tempdir().unwrap();
        let project_root = tmp.path().join("projects");
        let working_dir = Path::new("/home/user/project");
        let encoded = crate::stale_detector::encode_working_dir(working_dir);
        let subagents_dir = project_root.join(&encoded).join("sid-1").join("subagents");
        std::fs::create_dir_all(&subagents_dir).unwrap();
        std::fs::write(
            subagents_dir.join("reviewer.jsonl"),
            claude_turn("m2", 10, 5),
        )
        .unwrap();

        // `claude` discovers the file, stem verbatim (grouping is the caller's
        // job, not this dispatch's — see the trait method's doc comment).
        let claude_found = subagent_transcripts(CLAUDE, &project_root, working_dir, "sid-1");
        assert_eq!(claude_found.len(), 1);
        assert_eq!(claude_found[0].0, "reviewer");

        // `copilot` has no nested-subagent convention: the SAME directory
        // existing on disk is never picked up for it — a motivated absence
        // (the dispatch answers empty), never a silently-skipped branch.
        let copilot_found = subagent_transcripts(COPILOT, &project_root, working_dir, "sid-1");
        assert!(copilot_found.is_empty());

        // A data-declared harness: empty too, from the trait default.
        assert!(subagent_transcripts(OPENCODE, &project_root, working_dir, "sid-1").is_empty());
    }
}
