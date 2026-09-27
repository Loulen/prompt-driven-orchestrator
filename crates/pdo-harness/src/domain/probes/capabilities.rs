//! The capabilities of a harness as plain booleans — the read-friendly view the
//! gates consult.

use super::*;

/// The five capabilities of a harness as plain booleans — the read-friendly view
/// the gates consult. A harness with no probes (`None` from [`probes_for`]) is
/// absent on all five, so a data-declared harness resolves to
/// [`Capabilities::NONE`] without any per-harness code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
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
    pub const NONE: Capabilities = Capabilities {
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
pub fn capabilities(harness: &str) -> Capabilities {
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

/// The observed-execution-identity capability of `harness` (ADR-0065), or `None`
/// when the source is mute — the fold then falls back to the requested model and
/// effort and marks them `requested`. `pi`/`copilot` declare `None` explicitly
/// until their ticket, even though their sources could answer.
pub fn observed_identity_source(harness: &str) -> Option<ObservedIdentitySource> {
    probes_for(harness).and_then(|probes| probes.observed_identity_source())
}

/// Whether PDO can derive a Run's cost for `harness`: it needs both a cost source
/// and a way to find the transcript that source reads. A data-declared harness has
/// neither, so its Run's cost is "—" with a reason rather than a silent `$0`.
pub fn can_cost(harness: &str) -> bool {
    let c = capabilities(harness);
    c.cost && c.transcript
}

/// Whether PDO can measure `harness`'s context-window peak (#585): it needs both
/// a context-usage source and a way to find the transcript that source reads.
/// Mirrors [`can_cost`]'s shape — a harness with a source but no transcript
/// resolution (impossible today, but not structurally excluded) would still read
/// "absent", never a made-up zero.
pub fn can_measure_context(harness: &str) -> bool {
    let c = capabilities(harness);
    c.context_usage && c.transcript
}
