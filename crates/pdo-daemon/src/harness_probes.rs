//! The capability factory for agentic harnesses (#553, ADR-0045).
//!
//! Everything PDO knows how to do **beyond launching** a harness is a
//! **capability**, written harness by harness: estimate a cost, resolve a
//! transcript, constate an end of turn, spot a usage-limit menu, declare a staging
//! set. This module is the factory for those capabilities — and its whole
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
//!   [`HarnessProbes::staging_set`] is **said once, visibly** — it holds only by
//!   the user's image and the profile's `$HOME` exceptions, with no staged home and
//!   no autonomy fixups (ADR-0063).
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
    /// constant. `pi`'s: the `usage.cost.total` its session reports, **already in
    /// dollars** — constant **1.0** ([`crate::pi_session::REPORTED_USD_CONSTANT`]), so
    /// the surfaces show it without `~` (ADR-0052 §2 amended, #707).
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
    /// pi's `<sessions>/<encoded-cwd>/<timestamp>_<session-id>.jsonl` (#707): a
    /// directory keyed by working directory, a **file keyed by the session identity
    /// PDO imposed** — resolved by globbing `*_<id>.jsonl` inside the cwd's
    /// directory ([`crate::pi_session::resolve_by_id`]), never by newest mtime, so
    /// two nodes sharing a worktree have distinct files structurally.
    PiJsonlById,
}

impl TranscriptResolution {
    pub(crate) fn label(self) -> &'static str {
        match self {
            TranscriptResolution::ClaudeJsonl => "the JSONL transcript, keyed by working directory",
            TranscriptResolution::CopilotEventsJsonl => {
                "the event journal, keyed by the session identity PDO imposed"
            }
            TranscriptResolution::PiJsonlById => {
                "the session JSONL in the working directory's folder, keyed by the session \
                 identity PDO imposed"
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
    /// pi's **turn-end extension** (#707, ADR-0043 applied): an ephemeral extension
    /// the daemon writes per node and pi loads through `-e`, which runs
    /// `pdo complete --auto` on `agent_settled`
    /// ([`crate::pi_session::TURN_END_EXTENSION_TS`]). The sweep's fallback reads the
    /// session tail's `stopReason` ([`crate::pi_session::turn_state`]). Governed by
    /// the same `autocomplete_turn_end` setting as `claude`'s hook — no second switch.
    PiAgentSettled,
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
            TurnEndSubstrate::PiAgentSettled => {
                "an injected `agent_settled` extension, plus the session tail as the sweep's \
                 fallback"
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
// The shared `Peak` postfix is the point: each variant names WHOSE per-turn peak
// (claude's transcript, copilot's journal, pi's session), on the model of the other
// capability enums above.
#[allow(clippy::enum_variant_names)]
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
    /// pi's per-message `usage.totalTokens` (the turn's full occupancy), deduplicated
    /// by entry id and maxed — [`crate::pi_session::session_peak`]. The ceiling a
    /// reader holds it against is the model's context window as `pi --list-models`
    /// publishes it (#705, `Catalogue::model_contexts`).
    PiSessionPeak,
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
            ContextUsageSource::PiSessionPeak => {
                "derived — per-message `usage.totalTokens` from the session, deduplicated and \
                 maxed, read against the catalogue's context window"
            }
        }
    }
}

/// One `$HOME`-relative entry of a harness's staging set (ADR-0063 §1): copied
/// from the host into the Run's staging on the way in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StagingEntry {
    /// `$HOME`-relative path (`.claude/.credentials.json`, `.pi/agent/auth.json`).
    /// Routed by the same classifier as a profile entry
    /// ([`crate::sandbox_profile::landing`]), so the copy view and the mount view
    /// cannot drift.
    pub(crate) rel: &'static str,
    /// What to log at `info` when the host lacks the entry, or `None` for a silent
    /// skip. `Some` is for an entry whose absence is the **common** case yet worth a
    /// line (an org baseline on an install with no org): a `warn!` per Run would
    /// train the reader to ignore warnings.
    pub(crate) absent_note: Option<&'static str>,
}

/// A write that disarms a **blocking dialog** once a staging set is copied
/// (ADR-0063 §2). Distinct from the set itself: the set is what the harness
/// *reads* to behave as on the host; a fixup is what PDO *writes* so an unattended
/// session never waits for a human. `claude` has them; `copilot` and `pi` carry the
/// equivalent on their argv (`--allow-all`, `-a`) and declare none.
///
/// Each variant is applied by [`crate::sandbox_staging`], the only module that
/// writes into a staging — this enum only *names* the write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AutonomyFixup {
    /// `skipDangerousModePermissionPrompt: true` in the staged `.claude/settings.json`
    /// — merged into the host copy when the profile staged it, synthesised as a
    /// one-key file otherwise (ADR-0031 §1 G3).
    ClaudeBypassPermissions,
    /// The staged `.claude.json` baseline: `hasCompletedOnboarding`, plus trust
    /// pre-granted on the Run root when there is one (ADR-0031 §1 G4).
    ClaudeJsonBaseline,
}

/// The sandbox **staging set** a harness declares (ADR-0063 §1): the `$HOME`
/// entries and env that make a session in the container behave as on the host,
/// plus its **transcript sinks** and its [`AutonomyFixup`]s.
///
/// Absent ⇒ a sandboxed Run on this harness holds only by the user's image and
/// the profile's `$HOME` exceptions — and PDO says so once (see
/// [`staging_set_absence_note`]). `None` is a declared value (ADR-0051), published
/// in the support table.
///
/// The set is **data**; [`crate::sandbox_staging`] is the one interpreter. Today it
/// is applied at `prepare` (the Run's staging), #708 moves the copy to the spawn of
/// the first node that resolves the harness (ADR-0063 §3) — same data, later moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StagingSet {
    /// The support-table cell: what this harness stages, in the reader's words.
    pub(crate) label: &'static str,
    /// The harness's home root under `$HOME` (`.claude`, `.pi/agent`): the directory
    /// mounted (empty until filled) for every first-party harness (ADR-0063 §3).
    pub(crate) home_root: &'static str,
    /// Entries copied from the host, whole. Refused as profile entries: the set owns
    /// them in every profile.
    pub(crate) entries: &'static [StagingEntry],
    /// `$HOME`-relative sub-paths **skipped** when an entry above is a directory.
    pub(crate) excludes: &'static [&'static str],
    /// Env vars the harness needs inside the container (`PI_TELEMETRY=0`). Posed
    /// with the set; `claude` needs none.
    pub(crate) env: &'static [(&'static str, &'static str)],
    /// `$HOME`-relative transcript sinks: created **empty** on the way in (never
    /// copied — copying would break the merge-back idempotence and the cost fold),
    /// **harvested** at merge-back under the same encoded dirname. Refused as
    /// profile entries.
    pub(crate) transcripts: &'static [&'static str],
    /// The blocking-dialog disarms applied once the set is on disk.
    pub(crate) fixups: &'static [AutonomyFixup],
}

impl StagingSet {
    pub(crate) fn label(self) -> &'static str {
        self.label
    }
}

/// `claude`'s staging set — the five guarantees of ADR-0031 §1, byte for byte,
/// re-expressed as data (ADR-0063 amends §1: they are *claude's*, one set among
/// others):
///
/// - **G1 credentials** → entry `.claude/.credentials.json` (0600 preserved by
///   `std::fs::copy`; absent on the host → silent no-op, auth fails later and it is
///   not `prepare`'s call);
/// - **G2 org managed settings** → entry `.claude/remote-settings.json` (absent →
///   `info!` no-op, the majority case; present but uncopyable → hard error, a
///   compliance surprise);
/// - **G3 bypass permissions** → [`AutonomyFixup::ClaudeBypassPermissions`];
/// - **G4 `.claude.json` baseline** → [`AutonomyFixup::ClaudeJsonBaseline`];
/// - **G5 empty transcript sink** → transcripts `.claude/projects`.
///
/// The `~/.claude` allowlist itself (skills, plugins, settings…) is NOT here: it is
/// the user's `full` profile ([`crate::sandbox_profile::DEFAULT_FULL_ENTRIES`]),
/// the harness-agnostic diff of ADR-0063 §4.
pub(crate) const CLAUDE_STAGING_SET: StagingSet = StagingSet {
    label: "the `.claude` home — credentials and org managed settings copied, trust and \
            permissions bypass fixed up, transcripts harvested back",
    home_root: ".claude",
    entries: &[
        StagingEntry {
            rel: ".claude/.credentials.json",
            absent_note: None,
        },
        StagingEntry {
            rel: ".claude/remote-settings.json",
            absent_note: Some("nothing to consent to (no-op)"),
        },
    ],
    excludes: &[],
    env: &[],
    transcripts: &[".claude/projects"],
    fixups: &[
        AutonomyFixup::ClaudeBypassPermissions,
        AutonomyFixup::ClaudeJsonBaseline,
    ],
};

/// Every staging set a first-party harness declares, in registry order. The
/// generic consumers (the transcript merge-back, the profile grammar's refusals)
/// iterate this rather than naming `claude`, so a harness that declares a set is
/// harvested and protected with no second edit (ADR-0063).
pub(crate) fn staging_sets() -> Vec<(String, StagingSet)> {
    crate::harness_registry::embedded_floor()
        .into_iter()
        .filter_map(|d| {
            let set = probes_for(&d.name)?.staging_set()?;
            Some((d.name, set))
        })
        .collect()
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
    /// The sandbox staging set, or `None` (a sandboxed Run says its absence
    /// once). ADR-0063.
    fn staging_set(&self) -> Option<StagingSet> {
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
    fn staging_set(&self) -> Option<StagingSet> {
        Some(CLAUDE_STAGING_SET)
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
/// - the **staging set** is absent: configuring `copilot` is a documented
///   prerequisite, not PDO code (ADR-0063 / CONTEXT.md § "Harnais agentique").
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
    // usage_limit_anchor / staging_set stay `None` — declared absent (see above).

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

/// The `pi` capabilities (#705/#707, story #702; ADR-0051/0052/0043) — **five**
/// present, **two** declared absent with their motive:
///
/// - **cost**: a **reported** cost of constant **1.0** — pi writes `usage.cost.total`
///   in dollars on every assistant message, from its embedded catalogue; PDO sums
///   it, deduplicated by entry id, and never re-derives from tokens
///   ([`crate::pi_session::reported_cost`]). A message with tokens but no cost
///   makes the node's total unavailable, never `$0` (ADR-0052 §2 amended);
/// - **transcript**: the session JSONL, by the identity PDO imposed, globbed inside
///   the working directory's folder ([`crate::pi_session::resolve_by_id`]);
/// - **end of turn**: the injected `agent_settled` extension as the primary
///   substrate (ADR-0043 applied through the `{settings}` hole — see
///   [`turn_end_injection`]), the session tail's `stopReason` as the sweep's fallback;
/// - **context usage**: the per-message `usage.totalTokens` peak, read against the
///   catalogue's context window (#705);
/// - **hard error**: pi stays resident after a `stopReason: "error"`, so its exit
///   code is no verdict ([`HarnessProbes::exit_code_is_verdict`] is `false`) and the
///   session text is ([`crate::pi_session::hard_error`]);
/// - the **usage-limit menu anchor** is absent, explicitly: the probe is
///   informational, its anchor is claude's wording, and it triggers no recovery
///   (ADR-0012, ADR-0051 §"Limites");
/// - the **staging set** is absent in this ticket: #708 declares it (ADR-0063).
///
/// `subagent_transcripts` stays at the trait's default (empty): pi's session store is
/// flat per cwd (one file per session id, no nesting under a session to enumerate).
struct PiProbes;

impl HarnessProbes for PiProbes {
    /// A **reported** cost (ADR-0052) of constant 1.0: already in dollars.
    fn cost_source(&self) -> Option<CostSource> {
        Some(CostSource::ReportedByConstant)
    }
    fn transcript_resolution(&self) -> Option<TranscriptResolution> {
        Some(TranscriptResolution::PiJsonlById)
    }
    fn turn_end_substrate(&self) -> Option<TurnEndSubstrate> {
        Some(TurnEndSubstrate::PiAgentSettled)
    }
    /// Declared absent, explicitly (see above).
    fn usage_limit_anchor(&self) -> Option<UsageLimitAnchor> {
        None
    }
    /// Declared absent until #708 (ADR-0063).
    fn staging_set(&self) -> Option<StagingSet> {
        None
    }
    fn context_usage_source(&self) -> Option<ContextUsageSource> {
        Some(ContextUsageSource::PiSessionPeak)
    }

    /// pi's transcript resolution: `<store>/<encoded-cwd>/*_<session-id>.jsonl` — by
    /// the **session identity PDO imposed**, inside the working directory's folder.
    /// Without a pinned session id there is nothing to resolve (pi never
    /// blind-continues), so this returns `None`.
    fn resolve_transcript(
        &self,
        store_root: &Path,
        working_dir: &Path,
        session_id: Option<&str>,
    ) -> Option<PathBuf> {
        crate::pi_session::resolve_by_id(store_root, working_dir, session_id?)
    }

    /// The sweep's fallback: a tail whose last substantial record is an assistant
    /// message with `stopReason` `stop`/`length` and no tool call pending. A trailing
    /// `error` is never a finished turn.
    fn classify_turn_ended(&self, tail: &str) -> bool {
        crate::pi_session::turn_ended(tail)
    }

    fn context_peak(&self, text: &str) -> Option<u64> {
        crate::pi_session::session_peak(text)
    }

    /// pi survives a hard model failure (it stays resident and says
    /// `stopReason: "error"`), so its exit is not a verdict — the session text is.
    fn exit_code_is_verdict(&self) -> bool {
        false
    }

    /// The session's trailing `stopReason: "error"` with its `errorMessage`.
    fn classify_hard_error(&self, tail: &str) -> Option<String> {
        crate::pi_session::hard_error(tail)
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
/// file** — the library assistant's focus-hook JSON (#705) and, for the turn-end
/// hook, the claude half of [`turn_end_injection`].
///
/// The hole means "an injected settings file" (ADR-0043), but the *format* of what
/// PDO writes is claude's. `pi` has the hole too and fills it with `-e <extension>`
/// (CONTEXT.md § "Extension de fin de tour"): handed the claude JSON, pi would load
/// it as an extension and refuse to start. So the writers ask here before writing:
///
/// - a **first-party** harness takes the file only if its end-of-turn substrate is
///   claude's transcript (the `Stop` hook is that substrate) — `claude` yes, `pi`
///   no (its substrate is its own extension, [`TurnEndSubstrate::PiAgentSettled`]);
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

/// The per-node **turn-end file** a harness's `{settings}` hole takes when
/// `autocomplete_turn_end` is on (ADR-0043 and its application to `pi`, #707): what
/// to name it (a suffix after `<node>-iter-<n>`) and what to write in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TurnEndInjection {
    /// File-name suffix beside the prompt: `.settings.json` (claude), `.turn-end.ts`
    /// (pi). Distinct suffixes, so a resumed node on either harness rewrites its own
    /// file idempotently and never another harness's.
    pub(crate) suffix: &'static str,
    /// The file body, byte for byte.
    pub(crate) body: &'static str,
}

/// `claude`'s injection: the `Stop`-hook settings JSON of #433, through `--settings`.
const CLAUDE_TURN_END_INJECTION: TurnEndInjection = TurnEndInjection {
    suffix: ".settings.json",
    body: crate::tmux_session_manager::STOP_HOOK_SETTINGS_JSON,
};

/// `pi`'s injection: the `agent_settled` extension, through `-e`.
const PI_TURN_END_INJECTION: TurnEndInjection = TurnEndInjection {
    suffix: crate::pi_session::TURN_END_EXTENSION_SUFFIX,
    body: crate::pi_session::TURN_END_EXTENSION_TS,
};

/// What PDO writes into `harness`'s `{settings}` hole to arm turn-end
/// auto-completion, or `None` when nothing must be written (the token then drops at
/// render, so `-e`/`--settings` never reach the argv empty).
///
/// A dispatch point (ADR-0051): read off the declared end-of-turn substrate, never
/// off the name —
/// - [`TurnEndSubstrate::ClaudeTranscript`] ⇒ the claude `Stop`-hook JSON;
/// - [`TurnEndSubstrate::PiAgentSettled`] ⇒ the pi extension;
/// - [`TurnEndSubstrate::CopilotEventJournal`] ⇒ `None` (the journal needs no
///   injection, and `copilot` has no hole anyway);
/// - a **data-declared** harness (no probes) ⇒ the claude file, the pre-#705
///   behaviour: a user wrapping `claude` in their own descriptor keeps the hook.
///
/// One switch governs every harness: the caller checks `autocomplete_turn_end` once,
/// then asks here what the harness takes — no `pi`-specific setting.
pub(crate) fn turn_end_injection(harness: &str) -> Option<TurnEndInjection> {
    match probes_for(harness) {
        None => Some(CLAUDE_TURN_END_INJECTION),
        Some(p) => match p.turn_end_substrate() {
            Some(TurnEndSubstrate::ClaudeTranscript) => Some(CLAUDE_TURN_END_INJECTION),
            Some(TurnEndSubstrate::PiAgentSettled) => Some(PI_TURN_END_INJECTION),
            Some(TurnEndSubstrate::CopilotEventJournal) | None => None,
        },
    }
}

/// The one-time note for a node whose harness has **no turn-end substrate** while
/// turn-end auto-completion is enabled (ADR-0051 / correctif AC #7). `Some(msg)`
/// when the setting cannot be honoured for `harness`, `None` for a harness that
/// has the substrate (`claude`). Pure and testable, the twin of
/// [`staging_set_absence_note`]: the setting stops being a silent no-op.
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
        // #705/#707: `pi` — first-party; five capabilities present, usage-limit and
        // staging declared absent (the latter until #708).
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
            staging: p.staging_set().is_some(),
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
/// staging set (ADR-0063). `Some(msg)` when `harness` lacks a set, `None` when it
/// declares one (`claude`). Pure and testable, modelled on
/// `price_table::diagnostic`.
pub(crate) fn staging_set_absence_note(harness: &str) -> Option<String> {
    if capabilities(harness).staging {
        return None;
    }
    Some(format!(
        "sandbox: harness `{harness}` has no staging set (#553, ADR-0063) — this Run's session \
         holds only by the profile's image and its `$HOME` exceptions, without a staged home \
         (credentials, settings) or autonomy fixups (pre-granted trust, permissions bypass)"
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
        assert!(bare.staging_set().is_none());
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
        assert_eq!(p.staging_set(), Some(CLAUDE_STAGING_SET));
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
        assert!(capabilities(PI).context_usage);
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
        assert!(p.staging_set().is_none(), "staging set declared absent");

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
    fn pi_has_its_five_capabilities_and_declares_two_absent() {
        // #707 / ADR-0051: `pi` is instrumented — five capabilities dispatch to ITS
        // implementation (`crate::pi_session`), two are explicit `None`s.
        let p = probes_for(PI).expect("pi has probes (first-party)");
        assert_eq!(p.cost_source(), Some(CostSource::ReportedByConstant));
        assert_eq!(
            p.transcript_resolution(),
            Some(TranscriptResolution::PiJsonlById)
        );
        assert_eq!(
            p.turn_end_substrate(),
            Some(TurnEndSubstrate::PiAgentSettled)
        );
        assert_eq!(
            p.context_usage_source(),
            Some(ContextUsageSource::PiSessionPeak)
        );
        assert!(
            p.usage_limit_anchor().is_none(),
            "usage-limit anchor declared absent"
        );
        assert!(
            p.staging_set().is_none(),
            "staging declared absent (#708, ADR-0063)"
        );
        assert_eq!(
            capabilities(PI),
            Capabilities {
                cost: true,
                transcript: true,
                turn_end: true,
                usage_limit: false,
                staging: false,
                context_usage: true,
            }
        );
        // Consequences a reader sees: a pi Run is costable and measurable, the
        // turn-end setting is honoured (no absence note), the sandbox still says its
        // absence once.
        assert!(can_cost(PI));
        assert!(can_measure_context(PI));
        assert_eq!(turn_end_absence_note(PI), None);
        assert!(staging_set_absence_note(PI).is_some());
        // The exit code is no verdict: pi stays resident after a hard error.
        assert!(!exit_code_is_verdict(PI));
        // Behaviour is pi's own, never claude's parsers on pi's store.
        assert!(!turn_ended(PI, FIXTURE_CLAUDE_TURN_ENDED));
        assert!(!usage_limit_shown(PI, "wait for limit to reset"));
    }

    #[test]
    fn pi_dispatches_to_its_own_session_parsers_not_claudes_or_copilots() {
        let settled = concat!(
            r#"{"type":"message","id":"u1","message":{"role":"user","content":[{"type":"text","text":"hi"}]}}"#,
            "\n",
            r#"{"type":"message","id":"a1","message":{"role":"assistant","content":[{"type":"text","text":"done"}],"usage":{"input":10,"output":5,"cacheRead":0,"cacheWrite":0,"totalTokens":15,"cost":{"total":0.001}},"stopReason":"stop"}}"#,
            "\n"
        );
        assert!(turn_ended(PI, settled));
        // (claude's parser happens to accept this shape too — both stores carry a
        // `message.role` — which is exactly why the DISPATCH, not the parser, is
        // what keeps a claude verdict off a pi store: see the copilot control.)
        assert!(!turn_ended(COPILOT, settled), "not copilot's journal shape");
        assert_eq!(context_peak(PI, settled), Some(15));
        assert_eq!(context_peak(CLAUDE, settled), None);
        // A trailing hard error is a hard error for pi, and not a finished turn.
        let errored = concat!(
            r#"{"type":"message","id":"u1","message":{"role":"user","content":[{"type":"text","text":"hi"}]}}"#,
            "\n",
            r#"{"type":"message","id":"a1","message":{"role":"assistant","content":[],"usage":{"totalTokens":0,"cost":{"total":0}},"stopReason":"error","errorMessage":"boom"}}"#,
            "\n"
        );
        assert!(!turn_ended(PI, errored));
        assert_eq!(hard_error(PI, errored).as_deref(), Some("boom"));
        assert!(
            hard_error(CLAUDE, errored).is_none(),
            "claude's exit is its verdict"
        );
    }

    #[test]
    fn pi_resolves_its_session_by_identity_inside_the_cwd_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let store = tmp.path().join("sessions");
        let wd = Path::new("/shared/wt");
        let dir = store.join(crate::pi_session::session_dir_name(wd));
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("2026-09-05T15-00-00-000Z_sid-a.jsonl");
        let b = dir.join("2026-09-05T15-00-01-000Z_sid-b.jsonl");
        std::fs::write(&a, "").unwrap();
        std::fs::write(&b, "").unwrap();
        let p = probes_for(PI).unwrap();
        assert_eq!(p.resolve_transcript(&store, wd, Some("sid-a")), Some(a));
        assert_eq!(p.resolve_transcript(&store, wd, Some("sid-b")), Some(b));
        // No pinned identity ⇒ nothing to resolve (pi never blind-continues).
        assert!(p.resolve_transcript(&store, wd, None).is_none());
        // And never claude's `<uuid>.jsonl` under an encoded cwd.
        assert!(resolve_transcript(PI, &store, wd, Some("missing")).is_none());
    }

    #[test]
    fn turn_end_injection_is_read_off_the_substrate_never_the_name() {
        // #707: claude takes its Stop-hook JSON, pi its agent_settled extension,
        // copilot nothing (no hole, journal substrate), a data-declared harness the
        // claude file (pre-#705 behaviour: hole ⇒ file).
        let claude = turn_end_injection(CLAUDE).expect("claude is injected");
        assert_eq!(claude.suffix, ".settings.json");
        assert_eq!(
            claude.body,
            crate::tmux_session_manager::STOP_HOOK_SETTINGS_JSON
        );
        let pi = turn_end_injection(PI).expect("pi is injected");
        assert_eq!(pi.suffix, ".turn-end.ts");
        assert_eq!(pi.body, crate::pi_session::TURN_END_EXTENSION_TS);
        assert!(pi.body.contains("agent_settled"));
        assert_ne!(
            claude.suffix, pi.suffix,
            "distinct files, idempotent rewrites"
        );
        assert_eq!(turn_end_injection(COPILOT), None);
        // `opencode` and a user's descriptor have no probes: the hole-means-file
        // pre-#705 behaviour (inert for opencode, whose templates carry no hole).
        assert_eq!(turn_end_injection(OPENCODE), Some(claude));
        assert_eq!(
            turn_end_injection("my-claude-wrapper"),
            Some(claude),
            "a data-declared harness keeps the claude hook"
        );
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
        for name in [OPENCODE, "my-custom-harness", "not-a-harness"] {
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
        assert_eq!(
            turn_end_absence_note(PI),
            None,
            "pi has a substrate since #707"
        );
        let note = turn_end_absence_note(OPENCODE).expect("a note for a substrate-less harness");
        assert!(note.contains("`opencode`"));
        assert!(note.contains("no end-of-turn substrate"));
        assert!(note.contains("not be auto-completed"));
    }

    #[test]
    fn staging_set_absence_note_fires_only_for_a_harness_without_a_set() {
        assert_eq!(staging_set_absence_note(CLAUDE), None);
        let note = staging_set_absence_note("my-custom-harness").expect("a note");
        assert!(note.contains("`my-custom-harness`"));
        assert!(note.contains("no staging set"));
        assert!(note.contains("$HOME"));
    }

    /// ADR-0063: the five guarantees of ADR-0031 §1 are `claude`'s set, as data.
    /// Pinned here so a later edit to the set is a visible change of contract.
    #[test]
    fn claude_staging_set_carries_the_five_guarantees_of_adr_0031() {
        let set = CLAUDE_STAGING_SET;
        assert_eq!(set.home_root, ".claude");
        let entries: Vec<&str> = set.entries.iter().map(|e| e.rel).collect();
        assert_eq!(
            entries,
            [".claude/.credentials.json", ".claude/remote-settings.json"]
        );
        assert_eq!(set.transcripts, [".claude/projects"]);
        assert_eq!(
            set.fixups,
            [
                AutonomyFixup::ClaudeBypassPermissions,
                AutonomyFixup::ClaudeJsonBaseline
            ]
        );
        assert!(set.env.is_empty(), "claude needs no env in the container");
        assert!(set.excludes.is_empty());
    }

    /// Only `claude` declares a set today; `copilot`, `pi` (until #708) and
    /// `opencode` are explicit `None`s, so the generic consumers see exactly one set.
    #[test]
    fn staging_sets_lists_claude_only() {
        let sets = staging_sets();
        assert_eq!(sets.len(), 1, "{sets:?}");
        assert_eq!(sets[0].0, CLAUDE);
        assert_eq!(sets[0].1, CLAUDE_STAGING_SET);
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
