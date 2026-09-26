//! The capability markers a harness declares, each with its support-table label.

/// A harness's cost source — how PDO turns a live Run into a dollar figure.
///
/// A marker, present or absent. Absent ⇒ the Run's cost is "—" with a reason.
/// `claude`'s is the only source today; a second harness's would be a distinct
/// variant (measured: `opencode` writes its own per-message cost into a SQLite
/// its HTTP API exposes, four buckets that do not map onto `claude`'s — so cost
/// stays code, never a declared mini-language, ADR-0045).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CostSource {
    /// PDO derives the cost itself: per-message token usage in the harness's
    /// transcript × the resolved price table (ADR-0034), as `run_cost`
    /// already does for `claude`. A **derived** cost (ADR-0052 §1).
    DerivedFromTranscript,
    /// The harness counts its own cost, in its own billing unit, and PDO converts
    /// it by a **published constant** — never the price table (ADR-0052 §2). A
    /// **reported** cost: it cannot produce an `unpriced_models` signal, and it does
    /// not re-derive from tokens (which would double-count the cache). `copilot`'s:
    /// the `totalNanoAiu` its event journal reports, × [`crate::domain::copilot_journal`]'s
    /// constant. `pi`'s: the `usage.cost.total` its session reports, **already in
    /// dollars** — constant **1.0** ([`crate::domain::pi_session::REPORTED_USD_CONSTANT`]), so
    /// the surfaces show it without `~` (ADR-0052 §2 amended, #707).
    ReportedByConstant,
}

/// The **observed execution identity** capability (ADR-0065): what of the
/// model × effort pair a harness's **source** reports for a running execution.
/// The startup event is never this — it is the *requested* fallback, and Stats
/// always says which of the two it read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObservedIdentitySource {
    /// The source names the model **per message** (claude's transcript); the
    /// effort is never written by the source, so it stays requested.
    ModelPerMessage,
    /// The source names both the model and the effort — `pi` per message plus
    /// its thinking-level change events, `copilot` at the usage points with the
    /// reasoning effort then in force (#736). The observed values win over the
    /// requested ones and say so (`ModelEffortSlice::effort_observed`).
    ModelAndEffort,
}

/// One observed model × effort identity a harness's source reports **in one
/// session file's text** (ADR-0065 §1): the verbatim model id, the effort the
/// source named with it, and the provider (tooltip only, never part of the
/// identity — ADR-0065 §2). The raw material of the Performance « By model »
/// axis, where the context peak follows the session **file**, so the identity
/// is read per file, not per execution (#737).
#[derive(Debug, Clone, PartialEq)]
pub struct ObservedIdentity {
    pub model: String,
    pub effort: Option<String>,
    pub provider: Option<String>,
}

impl ObservedIdentitySource {
    /// How this source reads in the published support table
    /// ([`crate::domain::support`]). The label lives on the variant so the table
    /// can never describe a mechanism the code no longer dispatches to.
    pub fn label(self) -> &'static str {
        match self {
            ObservedIdentitySource::ModelPerMessage => {
                "observed — the model per message; the effort stays requested"
            }
            ObservedIdentitySource::ModelAndEffort => {
                "observed — the model and the effort, from the source"
            }
        }
    }
}

impl CostSource {
    /// How this source reads in the published support table
    /// ([`crate::domain::support`]). The label lives on the variant so the table
    /// can never describe a mechanism the code no longer dispatches to — same for
    /// every other capability's `label` below.
    pub fn label(self) -> &'static str {
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
pub enum TranscriptResolution {
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
    /// directory (`adapters::transcripts::pi_session_by_id`), never by newest mtime, so
    /// two nodes sharing a worktree have distinct files structurally.
    PiJsonlById,
}

impl TranscriptResolution {
    pub fn label(self) -> &'static str {
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
pub enum TurnEndSubstrate {
    /// The Claude Code JSONL transcript tail, classified by
    /// [`crate::domain::claude_transcript::parse_turn_state`].
    ClaudeTranscript,
    /// GitHub Copilot's event journal, whose explicit `assistant.turn_end` event is
    /// classified by [`crate::domain::copilot_journal::turn_ended`] (#615). Depends on no
    /// instance setting and writes nothing into the user's config.
    CopilotEventJournal,
    /// pi's **turn-end extension** (#707, ADR-0043 applied): an ephemeral extension
    /// the daemon writes per node and pi loads through `-e`, which runs
    /// `pdo complete --auto` on `agent_settled`
    /// ([`crate::domain::pi_session::TURN_END_EXTENSION_TS`]). The sweep's fallback reads the
    /// session tail's `stopReason` ([`crate::domain::pi_session::turn_state`]). Governed by
    /// the same `autocomplete_turn_end` setting as `claude`'s hook — no second switch.
    PiAgentSettled,
}

impl TurnEndSubstrate {
    pub fn label(self) -> &'static str {
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
/// `stale_detector` (they drift with Claude Code and are updated there).
/// Absent ⇒ the usage-limit probe never runs for this harness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageLimitAnchor {
    /// Claude Code's interactive "stop and wait for limit to reset" menu, matched
    /// by [`crate::domain::claude_transcript::detect_usage_limit`].
    ClaudePaneMenu,
}

impl UsageLimitAnchor {
    pub fn label(self) -> &'static str {
        match self {
            UsageLimitAnchor::ClaudePaneMenu => {
                "the interactive \"wait for limit to reset\" menu, matched in a pane capture"
            }
        }
    }
}

/// How PDO measures a harness's **context-window peak** — the maximum per-turn
/// occupancy a session reached, in tokens (#585, Stats → Performance). A
/// marker, not the parser itself: [`crate::domain::context_peak`] holds the actual
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
pub enum ContextUsageSource {
    /// Claude Code's per-message `usage` (input + both cache buckets + output),
    /// deduplicated across resume/compaction replays, maxed over the session's
    /// turns — [`crate::domain::context_peak::claude_session_peak`].
    ClaudeTranscriptPeak,
    /// GitHub Copilot's event journal `usage` readings, converted from their
    /// cumulative-since-session-start counters to a per-turn contribution before
    /// the max is sought — [`crate::domain::context_peak::copilot_session_peak`].
    CopilotJournalPeak,
    /// pi's per-message `usage.totalTokens` (the turn's full occupancy), deduplicated
    /// by entry id and maxed — [`crate::domain::pi_session::session_peak`]. The ceiling a
    /// reader holds it against is the model's context window as `pi --list-models`
    /// publishes it (#705, `Catalogue::model_contexts`).
    PiSessionPeak,
}

impl ContextUsageSource {
    pub fn label(self) -> &'static str {
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

/// How PDO counts a harness's **steering messages** — the turns a human typed
/// into a session after its launch (#792, Stats → Performance's third metric,
/// CONTEXT.md « Message de pilotage »). A marker, not the parser itself:
/// [`crate::domain::steering`] holds the per-harness turn reading, so this enum can
/// never describe a mechanism that module does not implement.
///
/// Absent ⇒ Performance shows « — » with a reason for this harness's Steering
/// (never `0`: an unreadable transcript is not an autonomous execution).
/// `opencode` declares it absent — its session is not resolvable by identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SteeringSource {
    /// Claude Code's `type:"user"` transcript turns with typed text, minus the
    /// launch prompt, `isMeta`, tool results, compaction summaries and
    /// runtime-prefixed turns — [`crate::domain::steering::claude_steering_count`].
    ClaudeTranscriptTurns,
    /// GitHub Copilot's `user.message` journal events, minus the first and the
    /// runtime-prefixed ones — [`crate::domain::steering::copilot_steering_count`].
    CopilotJournalUserMessages,
    /// pi's `type:"message"` entries of role `user`, minus the first and the
    /// runtime-prefixed ones — [`crate::domain::steering::pi_steering_count`].
    PiSessionUserMessages,
}

impl SteeringSource {
    pub fn label(self) -> &'static str {
        match self {
            SteeringSource::ClaudeTranscriptTurns => {
                "derived — typed user turns of the transcript, launch prompt and runtime \
                 messages excluded"
            }
            SteeringSource::CopilotJournalUserMessages => {
                "derived — the journal's `user.message` events, launch prompt and runtime \
                 messages excluded"
            }
            SteeringSource::PiSessionUserMessages => {
                "derived — the session's user-role messages, launch prompt and runtime \
                 messages excluded"
            }
        }
    }
}

/// Where a harness keeps the transcripts of a session's own **subagents**
/// (#585, Stats → Performance's subagent breakdown). A marker: the adapter that
/// reads the disk is its one interpreter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubagentLayout {
    /// Claude Code's `<projects_root>/<encoded-cwd>/<session-id>/subagents/`,
    /// every `*.jsonl` beneath it, recursively.
    ClaudeSessionSubagentsDir,
}
