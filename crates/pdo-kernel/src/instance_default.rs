//! An instance default as `GET /settings` discloses it (ADR-0015): the value
//! the daemon uses, the tier it came from, and every tier's raw value, so the
//! page can *reveal* a shadowed environment variable instead of ignoring it.
//!
//! Each context owns its instance defaults (their environment variable, their
//! built-in value, their `stored → env → default` resolver) and discloses them
//! with this shape; the settings context only assembles the page.

use serde::Serialize;

/// The tier a value came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Stored,
    Env,
    Default,
}

impl Source {
    /// The stored tier wins when it holds a value, then the environment's,
    /// then the built-in default.
    pub fn of(stored: bool, env: bool) -> Self {
        if stored {
            Source::Stored
        } else if env {
            Source::Env
        } else {
            Source::Default
        }
    }
}

/// One instance default, disclosed: `effective` is what the daemon uses,
/// produced by the resolver the daemon calls, and `stored` / `env` / `default`
/// are the tiers it chose between. The values are in the knob's own unit.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Disclosed<E, S, D> {
    pub effective: E,
    pub source: Source,
    pub stored: Option<S>,
    pub env: Option<S>,
    pub default: D,
}

impl<E, S, D> Disclosed<E, S, D> {
    /// Disclose `effective`, choosing the source from which tiers hold a value.
    pub fn new(effective: E, stored: Option<S>, env: Option<S>, default: D) -> Self {
        Self {
            source: Source::of(stored.is_some(), env.is_some()),
            effective,
            stored,
            env,
            default,
        }
    }
}

/// A number: the cap, a TTL, a timeout, a budget.
pub type Number = Disclosed<i64, i64, i64>;
/// A checkbox.
pub type Flag = Disclosed<bool, bool, bool>;
/// A free text with no built-in value (`default` is always `null`).
pub type Text = Disclosed<Option<String>, String, Option<String>>;

/// `1`/`true`/`yes`/`on` (case-insensitive), the truthy vocabulary of the
/// boolean environment variables that have no parser of their own.
pub fn truthy(raw: &str) -> bool {
    let t = raw.trim();
    ["1", "true", "yes", "on"]
        .iter()
        .any(|word| t.eq_ignore_ascii_case(word))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_tier_holding_a_value_wins() {
        assert_eq!(Source::of(true, true), Source::Stored);
        assert_eq!(Source::of(false, true), Source::Env);
        assert_eq!(Source::of(false, false), Source::Default);
    }

    #[test]
    fn a_disclosure_serializes_every_tier() {
        let cap: Number = Disclosed::new(3, None, Some(3), 20);
        assert_eq!(
            serde_json::to_value(cap).unwrap(),
            serde_json::json!({
                "effective": 3, "source": "env", "stored": null, "env": 3, "default": 20,
            })
        );
    }

    #[test]
    fn truthy_reads_the_usual_words() {
        for yes in ["1", "true", " YES ", "On"] {
            assert!(truthy(yes), "{yes}");
        }
        for no in ["0", "false", "", "enabled"] {
            assert!(!truthy(no), "{no}");
        }
    }
}
