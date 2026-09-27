//! What the operator asks a profile to become: the **diff**, never a snapshot
//! (ADR-0031 §2), and the pure rules that normalise it before it is stored.

use std::collections::BTreeMap;

use super::validate::{validate_env_key, validate_env_value};
use super::DEFAULT_FULL_ENTRIES;
use crate::domain::image::ProfileImage;

/// The requested content of a profile. Every field is a FULL replacement: an empty
/// list, an empty map or `None` clears what was stored, which is what makes "remove a
/// variable" or "go back to the default image" expressible at all.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProfileDraft {
    pub disabled: Vec<String>,
    pub extras: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub image: Option<ProfileImage>,
}

/// Normalise the `disabled` names: trimmed, deduplicated, sorted, each one a member of
/// the built-in default.
///
/// Validated by MEMBERSHIP in the authored default, not by `validate_entry`. Two
/// consequences, both wanted: the default's `.claude/*.md` glob is uncheckable but
/// never hand-writable, and an *extra* can only be removed by dropping it from
/// `extras` — unchecking is for defaults only, which keeps the diff unambiguous.
pub fn normalise_disabled(raw: &[String]) -> Result<Vec<String>, String> {
    let mut disabled: Vec<String> = Vec::with_capacity(raw.len());
    for raw in raw {
        let candidate = raw.trim().to_string();
        if candidate.is_empty() {
            return Err("a `disabled` entry cannot be blank".to_string());
        }
        if !DEFAULT_FULL_ENTRIES.iter().any(|d| d.path == candidate) {
            return Err(format!(
                "`{candidate}` is not a built-in default entry, so it cannot be unchecked \
                 — remove it from the extras instead"
            ));
        }
        if !disabled.contains(&candidate) {
            disabled.push(candidate);
        }
    }
    disabled.sort();
    Ok(disabled)
}

/// `extras ∩ disabled = ∅`. A contradictory intention resolved silently would freeze
/// into `RunStarted` a winner the user never picked.
pub fn check_no_clash(disabled: &[String], extras: &[String]) -> Result<(), String> {
    match disabled.iter().find(|d| extras.contains(d)) {
        Some(clash) => Err(format!(
            "`{clash}` is both unchecked and added as an extra — pick one"
        )),
        None => Ok(()),
    }
}

/// Normalise the env: each key and value validated, the run-constant keys PDO poses
/// itself refused by name.
///
/// The NORMALISED key is what gets stored, so `  FOO  ` and `FOO` cannot become two
/// rows that render identically. A collision after trimming is a 400 for the same
/// reason the `extras ∩ disabled` clash is: resolving it silently would freeze a
/// winner the user never picked.
pub fn normalise_env(raw: &BTreeMap<String, String>) -> Result<BTreeMap<String, String>, String> {
    let mut env: BTreeMap<String, String> = BTreeMap::new();
    for (raw_key, raw_value) in raw {
        let key = validate_env_key(raw_key)?;
        let value = validate_env_value(&key, raw_value)?;
        if env.insert(key.clone(), value).is_some() {
            return Err(format!(
                "`{key}` is set twice (once as `{raw_key}`) — an environment variable can \
                 only carry one value, and picking one for you would drop the other in silence"
            ));
        }
    }
    Ok(env)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn disabled_names_must_be_default_entries_and_come_out_sorted_once() {
        assert_eq!(
            normalise_disabled(&v(&[
                " .claude/plugins ",
                ".claude.json",
                ".claude/plugins"
            ])),
            Ok(v(&[".claude.json", ".claude/plugins"]))
        );
        assert!(normalise_disabled(&v(&["  "])).is_err());
        let err = normalise_disabled(&v(&[".gitconfig"])).unwrap_err();
        assert!(err.contains(".gitconfig"), "{err}");
    }

    #[test]
    fn an_entry_both_unchecked_and_added_is_refused() {
        assert!(check_no_clash(&v(&[".claude/plugins"]), &v(&[".gitconfig"])).is_ok());
        let err = check_no_clash(&v(&[".claude/plugins"]), &v(&[".claude/plugins"])).unwrap_err();
        assert!(err.contains(".claude/plugins"), "{err}");
    }

    #[test]
    fn env_keys_are_trimmed_and_a_collision_after_trimming_is_refused() {
        let raw = BTreeMap::from([(" FOO ".to_string(), "1".to_string())]);
        assert_eq!(
            normalise_env(&raw),
            Ok(BTreeMap::from([("FOO".to_string(), "1".to_string())]))
        );
        let clash = BTreeMap::from([
            ("FOO".to_string(), "1".to_string()),
            (" FOO".to_string(), "2".to_string()),
        ]);
        assert!(normalise_env(&clash).unwrap_err().contains("set twice"));
        let reserved = BTreeMap::from([("HOME".to_string(), "/x".to_string())]);
        assert!(normalise_env(&reserved).unwrap_err().contains("HOME"));
    }
}
