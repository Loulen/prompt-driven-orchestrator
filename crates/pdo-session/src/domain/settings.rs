//! The session knobs' defaults and the per-node launch resolution (model,
//! effort). The `stored → env → default` resolvers that read the environment
//! live in `adapters::env`.

use std::time::Duration;

/// Default TTL after node completion before the session is reaped.
pub const DEFAULT_REAPER_TTL: Duration = Duration::from_secs(3600);

/// Default sweep interval for the reaper background task.
pub const DEFAULT_REAPER_INTERVAL: Duration = Duration::from_secs(60);

/// How long the shared library assistant survives with no edit view open and no
/// terminal attached, before the orphan sweep reaps it (#594, ADR-0051).
///
/// Short on purpose, and unrelated to [`DEFAULT_REAPER_TTL`]: that one bounds a
/// *completed node's* session, which an operator may want to read hours later.
/// This one bounds a session nobody is looking at and nobody is editing against
/// — the state a browser reload leaves behind, which before #594 was unbounded.
/// The UI re-declares its focus every 20 s, so 120 s tolerates a background tab
/// whose timers Chrome has throttled to ~1/min.
pub const DEFAULT_LIBASSIST_IDLE_TTL: Duration = Duration::from_secs(120);

/// Resolve the model a work node launches with: the node's own `model:`
/// override wins, else the instance `default_effective` (#296/#347). An empty
/// string on *either* side collapses to the next tier — "" means "unset"
/// everywhere, so a hand-authored `model: ""` in YAML falls through to the
/// default instead of reaching the tail as an empty `--model`.
///
/// This is the single precedence point both spawn seams (`spawn_node` and
/// `start_node`) call, so the `node → instance` merge is defined and tested in
/// exactly one place.
pub fn resolve_node_model<'a>(
    node_model: Option<&'a str>,
    default_effective: Option<&'a str>,
) -> Option<&'a str> {
    node_model
        .filter(|s| !s.is_empty())
        .or(default_effective.filter(|s| !s.is_empty()))
}

/// Resolve the effort a work node launches with (#424): the node's own `effort:`,
/// with an empty string collapsing to `None` — "" means "unset" everywhere, so a
/// hand-authored `effort: ""` in YAML falls through instead of reaching the tail
/// as an empty `--effort`.
///
/// One tier today, on purpose: there is **no** instance-wide `default_effort`
/// (that is slice B of #424). This is the seam that gains the second tier when it
/// lands — keep it as the single precedence point both spawn seams call, exactly
/// like [`resolve_node_model`].
pub fn resolve_node_effort(node_effort: Option<&str>) -> Option<&str> {
    node_effort.filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_node_effort_collapses_empty() {
        // #424: pure, single-tier — there is no instance-wide `default_effort`
        // (slice B). An empty string means "unset" everywhere, so a hand-authored
        // `effort: ""` falls through to `None` instead of reaching the tail.
        assert_eq!(
            resolve_node_effort(Some("low")),
            Some("low"),
            "a set level passes through"
        );
        assert_eq!(
            resolve_node_effort(Some("turbo")),
            Some("turbo"),
            "an unknown level passes through too — the wire is open (ADR-0001)"
        );
        assert_eq!(
            resolve_node_effort(Some("")),
            None,
            "empty string collapses to unset"
        );
        assert_eq!(resolve_node_effort(None), None, "nothing set → None");
    }

    #[test]
    fn resolve_node_model_precedence() {
        // Pure (no env): the node override wins; an empty string on either side
        // collapses to the next tier; both empty/absent → None (account default).
        assert_eq!(
            resolve_node_model(Some("haiku"), Some("opus")),
            Some("haiku"),
            "node override wins over the instance default"
        );
        assert_eq!(
            resolve_node_model(None, Some("opus")),
            Some("opus"),
            "no node override → instance default"
        );
        assert_eq!(
            resolve_node_model(Some(""), Some("opus")),
            Some("opus"),
            "empty node override falls through to the default"
        );
        assert_eq!(
            resolve_node_model(Some("haiku"), None),
            Some("haiku"),
            "node override with no default"
        );
        assert_eq!(resolve_node_model(None, None), None, "nothing set → None");
        assert_eq!(
            resolve_node_model(Some(""), Some("")),
            None,
            "empty on both sides → None (byte-identity floor)"
        );
    }
}
