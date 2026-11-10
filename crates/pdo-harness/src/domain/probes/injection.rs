//! What PDO writes into a harness's `{settings}` hole, and the notes it says
//! once when a capability is absent.

use super::*;

/// The settings injected via `claude --settings` to arm a turn-end `Stop` hook
/// (#433, ADR-0043). On every turn end the hook runs `pdo complete --auto` and
/// then `exit 0` **unconditionally**: a `Stop` hook only forces the turn to
/// continue on `exit 2` / `{"decision":"block"}`, so the `; exit 0` swallows the
/// recoverable `exit 3` of a still-missing output (ADR-0035 — nothing recorded)
/// and the hook can never loop or complete prematurely. `--settings` MERGES
/// additively over `~/.claude` (confirmed against the pinned CLI: "load
/// additional settings"), so this clobbers no user hook. `pdo` is on the session
/// PATH (host and container) and the hook inherits the `PDO_*`/`PDO_DAEMON_URL`
/// exports the session wrapper sets, so no env has to be threaded into the JSON.
pub const STOP_HOOK_SETTINGS_JSON: &str = r#"{"hooks":{"Stop":[{"matcher":"","hooks":[{"type":"command","command":"pdo complete --auto; exit 0"}]}]}}"#;

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
pub fn settings_hole_takes_claude_file(harness: &str) -> bool {
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
pub struct TurnEndInjection {
    /// File-name suffix beside the prompt: `.settings.json` (claude), `.turn-end.ts`
    /// (pi). Distinct suffixes, so a resumed node on either harness rewrites its own
    /// file idempotently and never another harness's.
    pub suffix: &'static str,
    /// The file body, byte for byte.
    pub body: &'static str,
}

/// `claude`'s injection: the `Stop`-hook settings JSON of #433, through `--settings`.
const CLAUDE_TURN_END_INJECTION: TurnEndInjection = TurnEndInjection {
    suffix: ".settings.json",
    body: STOP_HOOK_SETTINGS_JSON,
};

/// `pi`'s injection: the `agent_settled` extension, through `-e`.
const PI_TURN_END_INJECTION: TurnEndInjection = TurnEndInjection {
    suffix: crate::domain::pi_session::TURN_END_EXTENSION_SUFFIX,
    body: crate::domain::pi_session::TURN_END_EXTENSION_TS,
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
pub fn turn_end_injection(harness: &str) -> Option<TurnEndInjection> {
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
pub fn turn_end_absence_note(harness: &str) -> Option<String> {
    if capabilities(harness).turn_end {
        return None;
    }
    Some(format!(
        "turn-end auto-completion is enabled but harness `{harness}` has no end-of-turn substrate \
         (#613, ADR-0051) — this node will not be auto-completed on turn end; complete it by \
         signalling `pdo complete` or leave it attached"
    ))
}

/// The one-time note for a sandboxed Run whose node runs on a harness with no
/// staging set (ADR-0063). `Some(msg)` when `harness` lacks a set, `None` when it
/// declares one (`claude`). Pure and testable, modelled on
/// `price_table::diagnostic`.
pub fn staging_set_absence_note(harness: &str) -> Option<String> {
    if capabilities(harness).staging {
        return None;
    }
    Some(format!(
        "sandbox: harness `{harness}` has no staging set (#553, ADR-0063) — this Run's session \
         holds only by the profile's image and its `$HOME` exceptions, without a staged home \
         (credentials, settings) or autonomy fixups (pre-granted trust, permissions bypass)"
    ))
}
