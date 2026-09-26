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
//! the transcript adapter (keyed on [`HarnessProbes::transcript_resolution`]), and gets *this harness's* answer. The
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
//! - **cost** (`run_cost`): a harness with no [`HarnessProbes::cost_source`]
//!   contributes "—" **and a reason naming it**, never `$0`, never a silent
//!   `partial` — the same vein as `unpriced_models` (#425).
//! - **turn-end / usage-limit** (`stale_detector`): the two sweep probes
//!   dispatch through [`turn_ended`] / [`usage_limit_shown`], gated on
//!   [`HarnessProbes::turn_end_substrate`] / [`HarnessProbes::usage_limit_anchor`].
//!   Absent ⇒ the probe does not run, and no node is ever auto-completed on an
//!   invented heuristic, nor its pane matched against another harness's menu.
//! - **turn-end setting** (`node_spawn`): enabling turn-end completion on a
//!   harness with no substrate is **said once** ([`turn_end_absence_note`]) rather
//!   than being a silent no-op.
//! - **sandbox** (`node_spawn`): a sandboxed Run on a harness with no
//!   [`HarnessProbes::staging_set`] is **said once, visibly** — it holds only by
//!   the user's image and the profile's `$HOME` exceptions, with no staged home and
//!   no autonomy fixups (ADR-0063).
//!
//! This module is narrow on purpose: it fabricates **capabilities, never a
//! launch** (a launch is data — the argv template of [`crate::domain::registry`]).

mod capabilities;
mod claude;
mod copilot;
mod injection;
mod pi;
mod sources;
mod staging;

use claude::CLAUDE_PROBES;
use copilot::COPILOT_PROBES;
use pi::PI_PROBES;

pub use capabilities::{
    can_cost, can_measure_context, capabilities, observed_identity_source, Capabilities,
};
pub use injection::{
    settings_hole_takes_claude_file, staging_set_absence_note, turn_end_absence_note,
    turn_end_injection, TurnEndInjection, STOP_HOOK_SETTINGS_JSON,
};
pub use sources::{
    ContextUsageSource, CostSource, ObservedIdentity, ObservedIdentitySource, SteeringSource,
    SubagentLayout, TranscriptResolution, TurnEndSubstrate, UsageLimitAnchor,
};
pub use staging::{
    staging_sets, AutonomyFixup, StagingEntry, StagingSet, CLAUDE_STAGING_SET, PI_STAGING_SET,
};

use crate::domain::registry;

/// The capabilities of one harness. **Every method defaults to "absent"**
/// (`None`); an implementation overrides only what its harness can do.
///
/// `Sync` so a single `&'static` instance can be handed out from [`probes_for`]
/// across the daemon's threads (each impl is a stateless zero-sized type).
pub trait HarnessProbes: Sync {
    /// Cost source, or `None` (contribute "—" + a reason, never `$0`).
    fn cost_source(&self) -> Option<CostSource> {
        None
    }
    /// What of the model × effort pair the harness's source reports, or `None`
    /// (both values fall back to the requested ones — declared absence,
    /// ADR-0065 §1).
    fn observed_identity_source(&self) -> Option<ObservedIdentitySource> {
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
    /// The steering-message source, or `None` (Performance shows « — » with a
    /// reason for this harness's Steering, never `0`). #792.
    fn steering_source(&self) -> Option<SteeringSource> {
        None
    }

    // These are the methods a generic consumer actually calls. The default is
    // "absent" — a data-declared harness ([`NullProbes`]) resolves no transcript,
    // never constates an end of turn, never matches a usage-limit menu — so a
    // caller that dispatches through [`resolved`] can never reach `claude`'s
    // implementation for a harness that did not declare it.

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
    /// `stats_performance` never has to name `claude`'s or `copilot`'s
    /// parser itself (that was exactly the ADR-0051 regression: a generic caller
    /// `match`ing on the harness string to pick a parsing function). `claude`
    /// dispatches to [`crate::domain::context_peak::claude_session_peak`], `copilot` to
    /// [`crate::domain::context_peak::copilot_session_peak`] — each pure, injected text
    /// in, `Option<u64>` out.
    fn context_peak(&self, _text: &str) -> Option<u64> {
        None
    }

    /// Where this harness keeps a session's own **subagent** transcripts, or
    /// `None` — declared-group discovery under one main session, for Stats →
    /// Performance's subagent breakdown (#585). The layout is data; the adapter
    /// that reads the disk (`adapters::transcripts::subagent_transcripts`) is its
    /// one interpreter.
    ///
    /// The default is `None`, not a `match harness { .. }` the caller has to
    /// write: a harness with no nested-subagent convention (every harness but
    /// `claude` today) answers "none" from the dispatch itself, so its absence is
    /// a value, never a silently-skipped branch (ADR-0051 §2).
    fn subagent_layout(&self) -> Option<SubagentLayout> {
        None
    }

    /// The model × effort identities this harness's source reports **in one
    /// session file's text** (ADR-0065 §1), in first-appearance order — one
    /// entry per model the source names, a subagent file carrying its own (#737:
    /// the context peak follows the file, so the identity is read per file).
    /// The default is an **empty `Vec`**, not a `match harness { .. }`: a harness
    /// whose source is mute (or unknown) answers "nothing observed" from the
    /// dispatch itself, and the caller falls back to the requested identity
    /// (or invents nothing when there is none — ADR-0065 §1).
    fn observed_identities(&self, _text: &str) -> Vec<ObservedIdentity> {
        Vec::new()
    }

    /// This harness's **steering message count** for one main session's `text`
    /// (#792): `Some(n)` typed human turns after the launch prompt, `None` when
    /// no human turn is readable. The default is `None` — a harness with no
    /// [`Self::steering_source`] never reaches a parser, so
    /// `stats_performance` never names one itself (ADR-0051).
    fn steering_count(&self, _text: &str) -> Option<u32> {
        None
    }
}

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
pub fn resolved(harness: &str) -> &'static dyn HarnessProbes {
    match probes_for(harness) {
        Some(p) => p,
        None => &NULL_PROBES,
    }
}

/// `harness`'s context-window peak for this transcript/journal `text`, dispatched
/// to its implementation (ADR-0051 §585) — the seam
/// `stats_performance` calls instead of matching the harness string
/// itself to pick `claude_session_peak` vs `copilot_session_peak`. A harness with
/// no [`HarnessProbes::context_usage_source`] answers `None`.
pub fn context_peak(harness: &str, text: &str) -> Option<u64> {
    resolved(harness).context_peak(text)
}

/// Whether `harness` declares a steering-message source (#792) — the gate
/// `stats_performance` reads before asking for a count, so a harness
/// with none (`opencode`) gets « — » with a reason, never `0`.
pub fn can_count_steering(harness: &str) -> bool {
    resolved(harness).steering_source().is_some()
}

/// `harness`'s steering-message count for one main session's `text`, dispatched
/// to its implementation (ADR-0051, #792). A harness with no
/// [`HarnessProbes::steering_source`] answers `None`.
pub fn steering_count(harness: &str, text: &str) -> Option<u32> {
    resolved(harness).steering_count(text)
}

/// The observed model × effort identities one session file's `text` reports,
/// dispatched to the harness's own reader (ADR-0065 §1, #737) — `claude`'s
/// per-message ids, `pi`'s per-message model + thinking level, `copilot`'s
/// usage-point model + effort. A harness with no
/// [`HarnessProbes::observed_identity_source`] answers an empty `Vec`: the
/// caller falls back to the requested identity, or invents nothing.
pub fn observed_identities(harness: &str, text: &str) -> Vec<ObservedIdentity> {
    resolved(harness).observed_identities(text)
}

/// Whether `harness` constates an end of turn from this transcript `tail`,
/// dispatched to its implementation (ADR-0051). A harness with no substrate
/// answers `false` — never `claude`'s JSONL parser on a foreign store.
pub fn turn_ended(harness: &str, tail: &str) -> bool {
    resolved(harness).classify_turn_ended(tail)
}

/// Whether `harness`'s usage-limit menu is showing in this `pane`, dispatched to
/// its implementation (ADR-0051). A harness with no anchor answers `false`.
pub fn usage_limit_shown(harness: &str, pane: &str) -> bool {
    resolved(harness).detect_usage_limit(pane)
}

/// The hard error `harness`'s transcript `tail` carries, dispatched to its
/// implementation (#615, ADR-0052). A harness whose exit code IS its verdict (or
/// one PDO carries no code for) answers `None`; `copilot` answers with its
/// journal's trailing `session.error`, because it exits 0 on a hard failure.
pub fn hard_error(harness: &str, tail: &str) -> Option<String> {
    resolved(harness).classify_hard_error(tail)
}

/// Whether `harness`'s process exit is a verdict on the turn (ADR-0032) — `true`
/// for `claude`, `false` for `copilot` (which exits 0 on a hard failure, ADR-0052).
/// A death consumer gates its journal read on the negation, so a harness whose
/// death speaks for itself pays no extra I/O.
pub fn exit_code_is_verdict(harness: &str) -> bool {
    resolved(harness).exit_code_is_verdict()
}

/// The capabilities of `harness`, or `None` for a harness PDO carries no code for.
///
/// `claude` is the only harness with any capability today; `opencode` and every
/// **data-declared** harness (a user's disk descriptor) get `None` — "no
/// capability" — which is exactly what makes the absence free and un-forgettable.
/// A caller that wants per-capability booleans uses [`capabilities`], which reads
/// the same `None` as "all absent".
pub fn probes_for(harness: &str) -> Option<&'static dyn HarnessProbes> {
    match harness {
        registry::CLAUDE => Some(&CLAUDE_PROBES),
        // #615: `copilot`'s three capabilities (reported cost, transcript, turn-end)
        // — the second first-party harness. Its two others are declared absent.
        registry::COPILOT => Some(&COPILOT_PROBES),
        // #705/#707: `pi` — first-party; five capabilities present, usage-limit and
        // staging declared absent (the latter until #708).
        registry::PI => Some(&PI_PROBES),
        // `opencode` (resident but un-instrumented in v1) and every data-declared
        // harness: no capability. A launch is data; a capability is code (ADR-0045).
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::registry::{CLAUDE, COPILOT, OPENCODE, PI};

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
        assert!(bare.subagent_layout().is_none());
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
    fn only_claude_declares_the_observed_identity_capability() {
        // ADR-0065: the observed model/effort is a capability, read source-first.
        // `claude` implements it here (model per message); `pi` and `copilot`
        // return `None` EXPLICITLY until their ticket — a declared absence, not a
        // missing dispatch (ADR-0051) — so both fall back to the requested values.
        assert_eq!(
            observed_identity_source(CLAUDE),
            Some(ObservedIdentitySource::ModelPerMessage)
        );
        assert_eq!(
            observed_identity_source(COPILOT),
            Some(ObservedIdentitySource::ModelAndEffort),
            "the journal names the model and the effort (measured 1.0.83, #736)"
        );
        assert_eq!(
            observed_identity_source(PI),
            Some(ObservedIdentitySource::ModelAndEffort),
            "the session names the model per message and the thinking level (measured 0.85.1, #736)"
        );
        assert_eq!(observed_identity_source(OPENCODE), None);
        assert_eq!(observed_identity_source("never-seen"), None);
    }

    /// #737: the per-file reader behind the Performance « By model » axis —
    /// each harness answers from its own source, a mute one answers empty.
    #[test]
    fn observed_identities_read_each_harnesss_own_source_per_file() {
        // claude: the verbatim ids of the transcript's own assistant messages,
        // in first-appearance order; API-error and synthetic lines are not
        // readings; the effort is never written by this source.
        let claude_text = concat!(
            r#"{"type":"assistant","message":{"model":"claude-opus-4-8","usage":{"input_tokens":10}}}"#,
            "\n",
            r#"{"type":"assistant","message":{"model":"claude-opus-4-8","usage":{"input_tokens":10}}}"#,
            "\n",
            r#"{"type":"assistant","isApiErrorMessage":true,"message":{"model":"claude-opus-4-8"}}"#,
            "\n",
            r#"{"type":"assistant","message":{"model":"<synthetic>","usage":{"input_tokens":10}}}"#,
            "\n",
            r#"{"type":"assistant","message":{"model":"claude-sonnet-4-5","usage":{"input_tokens":10}}}"#,
            "\n",
            "{not json\n",
        );
        assert_eq!(
            observed_identities(CLAUDE, claude_text),
            vec![
                ObservedIdentity {
                    model: "claude-opus-4-8".into(),
                    effort: None,
                    provider: None,
                },
                ObservedIdentity {
                    model: "claude-sonnet-4-5".into(),
                    effort: None,
                    provider: None,
                },
            ]
        );
        assert!(observed_identities(CLAUDE, "{not json\n").is_empty());

        // pi: the session names the model, the provider and the thinking level
        // per message (#736's own reader).
        let turn = "\
{\"type\":\"message\",\"message\":{\"role\":\"assistant\",\"model\":\"claude-opus-4-8\",\"provider\":\"openrouter\",\"usage\":{\"totalTokens\":10,\"cost\":{\"total\":0.01}}}}\n";
        let pi = observed_identities(PI, turn);
        assert_eq!(pi.len(), 1);
        assert_eq!(pi[0].model, "claude-opus-4-8");
        assert_eq!(pi[0].provider.as_deref(), Some("openrouter"));

        // copilot: the model in effect at the usage points with the effort in
        // force (#736's own reader).
        let journal = concat!(
            r#"{"type":"session.start","data":{"selectedModel":"gpt-5","reasoningEffort":"medium"}}"#,
            "\n",
            r#"{"type":"session.usage_checkpoint","data":{"totalNanoAiu":5000000000,"modelCacheState":[{"modelId":"gpt-5"}]}}"#,
        );
        let copilot = observed_identities(COPILOT, journal);
        assert_eq!(copilot.len(), 1);
        assert_eq!(copilot[0].model, "gpt-5");
        assert_eq!(copilot[0].effort.as_deref(), Some("medium"));

        // A data-declared harness answers empty from the dispatch itself —
        // never `claude`'s parser.
        assert!(observed_identities(OPENCODE, claude_text).is_empty());
        assert!(observed_identities("never-seen", claude_text).is_empty());
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
    fn pi_has_its_six_capabilities_and_declares_one_absent() {
        // #707/#708 / ADR-0051: `pi` is instrumented — five capabilities dispatch to
        // ITS implementation (`crate::domain::pi_session`), the staging set is its own data,
        // one capability is an explicit `None`.
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
        assert_eq!(
            p.staging_set(),
            Some(PI_STAGING_SET),
            "pi's own staging set (#708, ADR-0063)"
        );
        assert_eq!(
            capabilities(PI),
            Capabilities {
                cost: true,
                transcript: true,
                turn_end: true,
                usage_limit: false,
                staging: true,
                context_usage: true,
            }
        );
        // Consequences a reader sees: a pi Run is costable and measurable, the
        // turn-end setting is honoured (no absence note), and a sandboxed Run stages
        // its home (no absence note either).
        assert!(can_cost(PI));
        assert!(can_measure_context(PI));
        assert_eq!(turn_end_absence_note(PI), None);
        assert_eq!(staging_set_absence_note(PI), None);
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
    fn turn_end_injection_is_read_off_the_substrate_never_the_name() {
        // #707: claude takes its Stop-hook JSON, pi its agent_settled extension,
        // copilot nothing (no hole, journal substrate), a data-declared harness the
        // claude file (pre-#705 behaviour: hole ⇒ file).
        let claude = turn_end_injection(CLAUDE).expect("claude is injected");
        assert_eq!(claude.suffix, ".settings.json");
        assert_eq!(claude.body, STOP_HOOK_SETTINGS_JSON);
        let pi = turn_end_injection(PI).expect("pi is injected");
        assert_eq!(pi.suffix, ".turn-end.ts");
        assert_eq!(pi.body, crate::domain::pi_session::TURN_END_EXTENSION_TS);
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
    fn copilot_turn_end_dispatches_to_its_journal_parser_not_claudes() {
        let copilot_tail = "{\"type\":\"assistant.turn_start\",\"data\":{}}\n{\"type\":\"assistant.turn_end\",\"data\":{}}\n";
        assert!(turn_ended(COPILOT, copilot_tail));
        assert!(
            !turn_ended(CLAUDE, copilot_tail),
            "not claude's JSONL shape"
        );
        // A trailing hard error is not a finished turn (harness exits 0 on it).
        let errored = "{\"type\":\"assistant.turn_start\",\"data\":{}}\n{\"type\":\"session.error\",\"data\":{\"message\":\"boom\"}}\n";
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
                resolved(name).transcript_resolution().is_none(),
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
    /// [`crate::domain::claude_transcript::parse_turn_state`] calls `TurnEnded`.
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

    /// `claude` and `pi` declare a set; `copilot` and `opencode` are explicit
    /// `None`s, so the generic consumers (mounts, merge-back, profile refusals) see
    /// exactly two sets, in registry order.
    #[test]
    fn staging_sets_lists_claude_then_pi() {
        let sets = staging_sets();
        assert_eq!(sets.len(), 2, "{sets:?}");
        assert_eq!(sets[0].0, CLAUDE);
        assert_eq!(sets[0].1, CLAUDE_STAGING_SET);
        assert_eq!(sets[1].0, PI);
        assert_eq!(sets[1].1, PI_STAGING_SET);
    }

    /// ADR-0063 / #708: pi's set is the whole `.pi/agent` minus `sessions/`, two env
    /// vars, no fixup, and never `PI_OFFLINE`. Pinned so a later edit is a visible
    /// change of contract.
    #[test]
    fn pi_staging_set_is_the_agent_home_minus_sessions_with_two_env_vars_and_no_fixup() {
        let set = PI_STAGING_SET;
        assert_eq!(set.home_root, ".pi/agent");
        let entries: Vec<&str> = set.entries.iter().map(|e| e.rel).collect();
        assert_eq!(entries, [".pi/agent"]);
        assert_eq!(set.transcripts, [".pi/agent/sessions"]);
        assert!(
            set.fixups.is_empty(),
            "`-a` on the argv disarms pi's dialogs"
        );
        assert_eq!(
            set.env,
            [("PI_SKIP_VERSION_CHECK", "1"), ("PI_TELEMETRY", "0")]
        );
        assert!(
            set.env.iter().all(|(k, _)| *k != "PI_OFFLINE"),
            "the catalogue is copied, the network is not cut"
        );
        assert!(set.label.contains("auth"));
        assert!(set.label.contains("sessions harvested back"));
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
        // The public dispatch (`crate::domain::probes::context_peak`), not the
        // `ClaudeProbes`/`CopilotProbes` methods directly — this is exactly the
        // seam `stats_performance` calls instead of a hard-coded
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
    fn stop_hook_settings_json_is_valid_and_wraps_with_exit_zero() {
        // #433 / ADR-0043: the injected settings must be valid JSON that arms a
        // `Stop` hook whose command swallows any non-zero exit with `; exit 0`
        // (so the recoverable `exit 3` of a missing output can never force-loop
        // the turn or complete prematurely).
        let v: serde_json::Value =
            serde_json::from_str(STOP_HOOK_SETTINGS_JSON).expect("settings JSON must parse");
        let cmd = v["hooks"]["Stop"][0]["hooks"][0]["command"]
            .as_str()
            .expect("the Stop hook must carry a command");
        assert_eq!(cmd, "pdo complete --auto; exit 0");
        assert_eq!(
            v["hooks"]["Stop"][0]["hooks"][0]["type"].as_str(),
            Some("command")
        );
    }
}
