//! The run context's instance defaults, as `GET /settings` discloses them
//! (ADR-0015): each `effective` comes from the resolver the run context itself
//! calls at create, spawn or sweep time, so the page cannot drift from what a
//! Run gets. The stored tier is handed in by the settings context, which keeps
//! it.

use pdo_kernel::instance_default::{Disclosed, Flag, Number, Source, Text};

use super::env::{
    configured_cap_with, env_auto_fail, env_cap, env_manager_enabled, env_max_attachments_mb,
    resolve_instance_auto_fail, resolve_manager_enabled, resolve_max_attachments_mb,
    MANAGER_ENABLED_DEFAULT, MAX_ATTACHMENTS_MB_DEFAULT,
};
use crate::application::node::prompt as manager_prompt;
use crate::application::stale::detector;
use crate::domain::admission::DEFAULT_SESSION_CAP;

/// A stored `0`/`1` as the flag it records.
fn stored_flag(stored: Option<i64>) -> Option<bool> {
    stored.map(|v| v != 0)
}

/// The global session cap. A stored `0` is disclosed but never wins (a zero
/// cap would deadlock every Run).
pub fn session_cap(stored: Option<i64>) -> Number {
    let env = env_cap().map(|n| n as i64);
    Disclosed {
        effective: configured_cap_with(stored.map(|n| n as usize)) as i64,
        source: Source::of(stored.filter(|&n| n >= 1).is_some(), env.is_some()),
        stored,
        env,
        default: DEFAULT_SESSION_CAP as i64,
    }
}

/// Turn-end auto-completion, as the stale sweep resolves it. Both directions
/// of an explicit save are a stored decision that beats the env.
pub fn autocomplete_turn_end(stored: Option<i64>) -> Flag {
    Disclosed::new(
        detector::autocomplete_turn_end_with(stored),
        stored_flag(stored),
        detector::env_autocomplete_turn_end(),
        detector::AUTOCOMPLETE_TURN_END_DEFAULT,
    )
}

/// The manager's auto-naming of a nameless Run, as the create resolves it.
pub fn default_auto_name(stored: Option<i64>) -> Flag {
    Disclosed::new(
        manager_prompt::default_auto_name_with(stored),
        stored_flag(stored),
        manager_prompt::env_default_auto_name(),
        manager_prompt::DEFAULT_AUTO_NAME_DEFAULT,
    )
}

/// The global tier of `auto_fail`. Default `false`: an agent `pdo fail` parks
/// the Run for a human to confirm.
pub fn auto_fail(stored: Option<i64>) -> Flag {
    Disclosed::new(
        resolve_instance_auto_fail(stored),
        stored_flag(stored),
        env_auto_fail(),
        false,
    )
}

/// Whether a new Run spawns its Pipeline Manager on its own. Off by default:
/// the Manager tab's Start button brings one up.
pub fn manager_enabled(stored: Option<i64>) -> Flag {
    Disclosed::new(
        resolve_manager_enabled(stored),
        stored_flag(stored),
        env_manager_enabled(),
        MANAGER_ENABLED_DEFAULT,
    )
}

/// The agentic profile NAME the manager is pinned to, or `null` (« Follow the
/// Run »). A pure stored decision: no env tier. A stored name whose profile was
/// deleted dangles: the page renders the tombstone, the spawn falls back.
pub fn manager_profile(stored: Option<&str>) -> Text {
    let stored = stored.filter(|s| !s.is_empty()).map(str::to_string);
    Disclosed::new(stored.clone(), stored, None, None)
}

/// The per-Run attachment budget in MB: it bounds the multipart body of
/// `POST /runs` and drives the launch modal's client-side gate.
pub fn max_attachments_mb(stored: Option<i64>) -> Number {
    Disclosed::new(
        resolve_max_attachments_mb(stored),
        stored.filter(|&n| n >= 1),
        env_max_attachments_mb(),
        MAX_ATTACHMENTS_MB_DEFAULT,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stored_zero_cap_is_disclosed_but_does_not_win() {
        let cap = session_cap(Some(0));
        assert_eq!(cap.stored, Some(0));
        assert_ne!(cap.source, Source::Stored);
    }

    #[test]
    fn a_stored_flag_is_a_decision_both_ways() {
        for (stored, on) in [(Some(1), true), (Some(0), false)] {
            let flag = autocomplete_turn_end(stored);
            assert_eq!((flag.effective, flag.stored), (on, Some(on)));
            assert_eq!(flag.source, Source::Stored);
        }
    }

    #[test]
    fn an_empty_manager_pin_is_unset() {
        let pin = manager_profile(Some(""));
        assert_eq!((pin.effective, pin.source), (None, Source::Default));
        let pin = manager_profile(Some("Reviewer"));
        assert_eq!(pin.effective.as_deref(), Some("Reviewer"));
        assert_eq!(pin.source, Source::Stored);
    }
}
