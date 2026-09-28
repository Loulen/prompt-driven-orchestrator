//! The session context's instance defaults, as `GET /settings` discloses them
//! (ADR-0015): the reaper's TTLs and the model and harness a session launches
//! with when nothing more specific names one. Each `effective` comes from the
//! resolver the sweep or the spawn calls; the stored tier is handed in by the
//! settings context, which keeps it.

use std::collections::BTreeMap;

use pdo_harness::registry::CLAUDE;
use pdo_kernel::instance_default::{Disclosed, Number, Source, Text};
use serde::Serialize;

use super::env::{
    default_harness_with, default_model_with, env_default_harness, env_default_model,
    env_libassist_idle_ttl_secs, env_reaper_ttl_secs, libassist_idle_ttl_with, reaper_ttl_with,
};
use crate::domain::settings::{DEFAULT_LIBASSIST_IDLE_TTL, DEFAULT_REAPER_TTL};

/// A TTL in seconds. A stored value below `1` is disclosed but never wins (a
/// zero TTL would reap a session the instant it completes).
fn ttl(stored: Option<i64>, env: Option<u64>, effective: u64, default: u64) -> Number {
    let env = env.map(|n| n as i64);
    Disclosed {
        effective: effective as i64,
        source: Source::of(stored.filter(|&n| n >= 1).is_some(), env.is_some()),
        stored,
        env,
        default: default as i64,
    }
}

/// How long a completed node's session survives before the reaper takes it.
pub fn reaper_ttl_secs(stored: Option<i64>) -> Number {
    ttl(
        stored,
        env_reaper_ttl_secs(),
        reaper_ttl_with(stored.map(|n| n as u64)).as_secs(),
        DEFAULT_REAPER_TTL.as_secs(),
    )
}

/// How long the library assistant survives with nobody using it. Its own knob,
/// deliberately not a reuse of [`reaper_ttl_secs`]: that one bounds a completed
/// node's session (1 h), this one a REPL nobody is using (120 s).
pub fn libassist_idle_ttl_secs(stored: Option<i64>) -> Number {
    ttl(
        stored,
        env_libassist_idle_ttl_secs(),
        libassist_idle_ttl_with(stored.map(|n| n as u64)).as_secs(),
        DEFAULT_LIBASSIST_IDLE_TTL.as_secs(),
    )
}

/// A free text whose `""` means unset, on either tier.
fn text(stored: Option<&str>, env: Option<String>, effective: Option<String>) -> Text {
    let stored = stored.filter(|s| !s.is_empty()).map(str::to_string);
    Disclosed::new(effective, stored, env, None)
}

/// The instance-wide default model. `effective = null` is the account default
/// (no `--model`).
pub fn default_model(stored: Option<&str>) -> Text {
    text(
        stored,
        env_default_model(),
        default_model_with(stored.map(str::to_string)),
    )
}

/// The instance-wide default harness. `effective = null` is the `claude` floor.
pub fn default_harness(stored: Option<&str>) -> Text {
    text(
        stored,
        env_default_harness(),
        default_harness_with(stored.map(str::to_string)),
    )
}

/// The per-harness default models, as stored and as the resolver's instance
/// tier reads them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HarnessModels {
    pub effective: BTreeMap<String, String>,
    pub stored: BTreeMap<String, String>,
}

/// The stored map, plus the legacy default model folded under `claude` when the
/// map does not name one — what a `claude` spawn falling through to the
/// instance reads.
pub fn default_harness_model(
    stored: &BTreeMap<String, String>,
    stored_default_model: Option<&str>,
) -> HarnessModels {
    let mut effective = stored.clone();
    if !effective.contains_key(CLAUDE) {
        if let Some(model) = default_model_with(stored_default_model.map(str::to_string)) {
            effective.insert(CLAUDE.to_string(), model);
        }
    }
    HarnessModels {
        effective,
        stored: stored.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_stored_text_is_unset() {
        let model = default_model(Some(""));
        assert_eq!(model.stored, None);
        assert_ne!(model.source, Source::Stored);
    }

    #[test]
    fn a_stored_claude_model_is_kept_over_the_legacy_default() {
        let stored = BTreeMap::from([(CLAUDE.to_string(), "opus".to_string())]);
        let models = default_harness_model(&stored, Some("sonnet"));
        assert_eq!(models.effective[CLAUDE], "opus");
    }

    #[test]
    fn the_legacy_default_model_folds_under_claude() {
        let models = default_harness_model(&BTreeMap::new(), Some("sonnet"));
        assert_eq!(models.effective[CLAUDE], "sonnet");
        assert!(models.stored.is_empty());
    }
}
