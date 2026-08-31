//! Resolve a node's effective harness, model and effort — **pure** (ADR-0046).
//!
//! The harness is an axis with four tiers, coarsest last:
//! `node → Run → Projet → Configuration d'instance → plancher (claude)`. The
//! finest tier that names a harness wins, and a **pinned** node harness shields
//! that choice from every coarser tier. The model and effort are **not** axes:
//! they carry no precedence of their own — they are read from the winning
//! harness's entry in the node's per-harness map (a slug means nothing outside
//! the harness that accepts it, ADR-0046).
//!
//! Pure by contract (an AC): tier values in, a [`ResolvedHarness`] out — no
//! `$HOME`, no disk, no clock — so the whole precedence matrix is unit-tested
//! without a fixture.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// A node's settings for one harness (`harnesses.<name>` in the YAML). Model and
/// effort are free-text pass-through — don't close them into an enum, it would
/// perish at every model release (ADR-0001). Keyed by harness name so a node stays
/// executable on either harness rather than carrying one slug the other rejects
/// (ADR-0046).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct HarnessEntry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
}

/// The harness named at each tier, coarsest last. `None` = that tier names no
/// harness.
#[derive(Debug, Default, Clone)]
pub(crate) struct HarnessTiers<'a> {
    /// `pin_harness`: a pin both **selects** the harness and shields it from every
    /// coarser tier (ADR-0046: épinglage ≠ paramétrage).
    pub node_pin: Option<&'a str>,
    /// The tier that re-runs the same pipeline on another harness (#551).
    pub run: Option<&'a str>,
    pub project: Option<&'a str>,
    /// ADR-0015, amended by ADR-0046.
    pub instance_default: Option<&'a str>,
}

/// What the spawn seam launches with, frozen into the node's start event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedHarness {
    pub harness: String,
    pub model: Option<String>,
    pub effort: Option<String>,
}

/// The finest tier naming a non-empty harness wins, with `claude` as the floor.
///
/// `""` means "unset" everywhere: a blank `pin_harness:` or instance default must
/// fall through to the floor, not resolve to an unknown harness (the `Some("")`
/// trap of #347).
pub(crate) fn resolve_harness(tiers: &HarnessTiers<'_>) -> String {
    [
        tiers.node_pin,
        tiers.run,
        tiers.project,
        tiers.instance_default,
    ]
    .into_iter()
    .flatten()
    .find(|s| !s.is_empty())
    .unwrap_or(crate::harness_registry::CLAUDE)
    .to_string()
}

/// The node's entry for the winning harness, else the instance per-harness
/// default, else `None` — no `--model`, i.e. the harness account default, which is
/// what keeps the `claude` launch byte-identical to the legacy one. Empty collapses
/// to the next tier (#347).
pub(crate) fn resolve_model(
    node_entry_model: Option<&str>,
    instance_default_model: Option<&str>,
) -> Option<String> {
    node_entry_model
        .filter(|s| !s.is_empty())
        .or(instance_default_model.filter(|s| !s.is_empty()))
        .map(str::to_string)
}

/// The node's entry for the winning harness, empty → `None` (#424). One tier only:
/// effort has no instance default.
pub(crate) fn resolve_effort(node_entry_effort: Option<&str>) -> Option<String> {
    node_entry_effort
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// The harness an **infra** session (Pipeline Manager, merge resolver) runs on
/// (#551, ADR-0046). Infra sessions have no NodeDef, hence no `node` tier and no
/// model/effort: `Run → instance → plancher`. Don't give them their own tier —
/// "ce Run tourne sur X" must hold with no exception to remember, so an A/B on a
/// new harness also exercises the unblocking tool.
pub(crate) fn resolve_infra_harness(run: Option<&str>, instance_default: Option<&str>) -> String {
    resolve_harness(&HarnessTiers {
        node_pin: None,
        run,
        project: None,
        instance_default,
    })
}

/// The single precedence point both spawn seams call: resolve the harness, then
/// read model and effort from the **winning harness's** entry.
pub(crate) fn resolve(
    tiers: &HarnessTiers<'_>,
    node_entries: &BTreeMap<String, HarnessEntry>,
    instance_default_models: &BTreeMap<String, String>,
) -> ResolvedHarness {
    let harness = resolve_harness(tiers);
    let entry = node_entries.get(&harness);
    let node_model = entry.and_then(|e| e.model.as_deref());
    let node_effort = entry.and_then(|e| e.effort.as_deref());
    let default_model = instance_default_models.get(&harness).map(String::as_str);
    ResolvedHarness {
        model: resolve_model(node_model, default_model),
        effort: resolve_effort(node_effort),
        harness,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness_registry::{CLAUDE, OPENCODE};

    fn entries(pairs: &[(&str, Option<&str>, Option<&str>)]) -> BTreeMap<String, HarnessEntry> {
        pairs
            .iter()
            .map(|(name, model, effort)| {
                (
                    name.to_string(),
                    HarnessEntry {
                        model: model.map(str::to_string),
                        effort: effort.map(str::to_string),
                    },
                )
            })
            .collect()
    }

    fn no_defaults() -> BTreeMap<String, String> {
        BTreeMap::new()
    }

    #[test]
    fn floor_is_claude_when_no_tier_names_one() {
        assert_eq!(resolve_harness(&HarnessTiers::default()), CLAUDE);
    }

    #[test]
    fn instance_default_beats_the_floor() {
        let tiers = HarnessTiers {
            instance_default: Some(OPENCODE),
            ..Default::default()
        };
        assert_eq!(resolve_harness(&tiers), OPENCODE);
    }

    #[test]
    fn node_pin_beats_every_coarser_tier() {
        let tiers = HarnessTiers {
            node_pin: Some(CLAUDE),
            run: Some(OPENCODE),
            project: Some(OPENCODE),
            instance_default: Some(OPENCODE),
        };
        assert_eq!(resolve_harness(&tiers), CLAUDE);
    }

    #[test]
    fn run_beats_project_and_instance() {
        let tiers = HarnessTiers {
            node_pin: None,
            run: Some("run-harness"),
            project: Some("project-harness"),
            instance_default: Some(OPENCODE),
        };
        assert_eq!(resolve_harness(&tiers), "run-harness");
    }

    #[test]
    fn project_beats_instance() {
        let tiers = HarnessTiers {
            project: Some("project-harness"),
            instance_default: Some(OPENCODE),
            ..Default::default()
        };
        assert_eq!(resolve_harness(&tiers), "project-harness");
    }

    #[test]
    fn empty_string_never_wins_a_tier() {
        let tiers = HarnessTiers {
            node_pin: Some(""),
            instance_default: Some(""),
            ..Default::default()
        };
        assert_eq!(resolve_harness(&tiers), CLAUDE);
    }

    #[test]
    fn infra_harness_follows_the_run_then_instance_then_floor() {
        assert_eq!(
            resolve_infra_harness(Some(OPENCODE), Some(CLAUDE)),
            OPENCODE
        );
        assert_eq!(resolve_infra_harness(None, Some(OPENCODE)), OPENCODE);
        assert_eq!(resolve_infra_harness(None, None), CLAUDE);
    }

    #[test]
    fn infra_harness_ignores_a_blank_run_choice() {
        assert_eq!(resolve_infra_harness(Some(""), Some(OPENCODE)), OPENCODE);
        assert_eq!(resolve_infra_harness(Some(""), Some("")), CLAUDE);
    }

    #[test]
    fn model_and_effort_come_from_the_winning_harness_entry() {
        let tiers = HarnessTiers {
            node_pin: Some(OPENCODE),
            ..Default::default()
        };
        let node_entries = entries(&[
            (CLAUDE, Some("opus"), Some("high")),
            (OPENCODE, Some("openrouter/foo"), None),
        ]);
        let r = resolve(&tiers, &node_entries, &no_defaults());
        assert_eq!(r.harness, OPENCODE);
        assert_eq!(r.model.as_deref(), Some("openrouter/foo"));
        assert_eq!(r.effort, None);
    }

    #[test]
    fn a_run_switched_to_another_harness_reads_the_new_harness_entry_not_the_old() {
        // AC (#551): a Run switched to another harness never inherits the slug written
        // for the previous one.
        let node_entries = entries(&[
            (CLAUDE, Some("opus"), Some("high")),
            (OPENCODE, Some("openrouter/foo"), None),
        ]);

        let on_claude = resolve(&HarnessTiers::default(), &node_entries, &no_defaults());
        assert_eq!(on_claude.harness, CLAUDE);
        assert_eq!(on_claude.model.as_deref(), Some("opus"));
        assert_eq!(on_claude.effort.as_deref(), Some("high"));

        let run_opencode = HarnessTiers {
            run: Some(OPENCODE),
            ..Default::default()
        };
        let on_opencode = resolve(&run_opencode, &node_entries, &no_defaults());
        assert_eq!(on_opencode.harness, OPENCODE);
        assert_eq!(on_opencode.model.as_deref(), Some("openrouter/foo"));
        assert_eq!(on_opencode.effort, None);
    }

    #[test]
    fn a_node_without_an_entry_for_the_winning_harness_runs_without_a_model() {
        let tiers = HarnessTiers {
            node_pin: Some(OPENCODE),
            ..Default::default()
        };
        let node_entries = entries(&[(CLAUDE, Some("opus"), Some("high"))]);
        let r = resolve(&tiers, &node_entries, &no_defaults());
        assert_eq!(r.harness, OPENCODE);
        assert_eq!(r.model, None, "no claude slug leaks onto opencode");
        assert_eq!(r.effort, None);
    }

    #[test]
    fn instance_per_harness_default_model_backs_a_missing_node_model() {
        let tiers = HarnessTiers {
            instance_default: Some(OPENCODE),
            ..Default::default()
        };
        let defaults: BTreeMap<String, String> =
            [(OPENCODE.to_string(), "openrouter/def".to_string())]
                .into_iter()
                .collect();
        let r = resolve(&tiers, &BTreeMap::new(), &defaults);
        assert_eq!(r.harness, OPENCODE);
        assert_eq!(r.model.as_deref(), Some("openrouter/def"));
    }

    #[test]
    fn node_entry_model_beats_the_instance_default() {
        let defaults: BTreeMap<String, String> = [(CLAUDE.to_string(), "sonnet".to_string())]
            .into_iter()
            .collect();
        let node_entries = entries(&[(CLAUDE, Some("opus"), None)]);
        let r = resolve(&HarnessTiers::default(), &node_entries, &defaults);
        assert_eq!(r.model.as_deref(), Some("opus"));
    }

    #[test]
    fn empty_node_model_falls_through_to_the_instance_default() {
        let defaults: BTreeMap<String, String> = [(CLAUDE.to_string(), "sonnet".to_string())]
            .into_iter()
            .collect();
        let node_entries = entries(&[(CLAUDE, Some(""), Some(""))]);
        let r = resolve(&HarnessTiers::default(), &node_entries, &defaults);
        assert_eq!(r.model.as_deref(), Some("sonnet"), "\"\" is unset");
        assert_eq!(r.effort, None, "empty effort collapses to None");
    }

    #[test]
    fn claude_node_with_no_settings_resolves_to_the_byte_identical_launch() {
        let r = resolve(&HarnessTiers::default(), &BTreeMap::new(), &no_defaults());
        assert_eq!(r.harness, CLAUDE);
        assert_eq!(r.model, None);
        assert_eq!(r.effort, None);
    }
}
