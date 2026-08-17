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
//! ## What "absent is said, never supplied" buys each caller
//! - **cost** ([`crate::run_cost`]): a harness with no [`HarnessProbes::cost_source`]
//!   contributes "—" **and a reason naming it**, never `$0`, never a silent
//!   `partial` — the same vein as `unpriced_models` (#425).
//! - **turn-end / usage-limit** ([`crate::stale_detector`]): the two sweep probes
//!   are **gated** on [`HarnessProbes::turn_end_substrate`] /
//!   [`HarnessProbes::usage_limit_anchor`]. Absent ⇒ the probe does not run, and
//!   no node is ever auto-completed on an invented heuristic.
//! - **sandbox** ([`crate::node_spawn`]): a sandboxed Run on a harness with no
//!   [`HarnessProbes::staging_floor`] is **said once, visibly** — it holds only by
//!   the user's image and the profile's `$HOME` exceptions, without the plancher's
//!   guarantees (ADR-0031).
//!
//! This module is narrow on purpose: it fabricates **capabilities, never a
//! launch** (a launch is data — the argv template of [`crate::harness_registry`]).

use crate::harness_registry;

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
    /// already does for `claude`.
    DerivedFromTranscript,
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
}

/// The single `claude` instance handed out by [`probes_for`]. Zero-sized, so this
/// `static` costs nothing and needs no lazy init.
static CLAUDE_PROBES: ClaudeProbes = ClaudeProbes;

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
    };
}

/// Resolve `harness` to its five capability booleans. `None` from the factory ⇒
/// [`Capabilities::NONE`], so an unknown or data-declared harness is absent on all
/// five — the property the whole slice rests on.
pub(crate) fn capabilities(harness: &str) -> Capabilities {
    match probes_for(harness) {
        None => Capabilities::NONE,
        Some(p) => Capabilities {
            cost: p.cost_source().is_some(),
            transcript: p.transcript_resolution().is_some(),
            turn_end: p.turn_end_substrate().is_some(),
            usage_limit: p.usage_limit_anchor().is_some(),
            staging: p.staging_floor().is_some(),
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
    use crate::harness_registry::{CLAUDE, OPENCODE};

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
            }
        );
        assert!(can_cost(CLAUDE));
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

    #[test]
    fn staging_floor_absence_note_fires_only_for_a_harness_without_the_floor() {
        // `claude` has the floor → no note.
        assert_eq!(staging_floor_absence_note(CLAUDE), None);
        // A data-declared harness has none → one visible note that names it and
        // says what is NOT guaranteed.
        let note = staging_floor_absence_note("my-custom-harness").expect("a note");
        assert!(note.contains("`my-custom-harness`"));
        assert!(note.contains("no staging floor"));
        assert!(note.contains("$HOME"));
    }
}
