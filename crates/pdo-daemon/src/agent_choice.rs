//! The **profil agentique** union each tier carries, and its pure resolver
//! (#563, ADR-0057).
//!
//! Every tier of the precedence chain `Node → Run → Projet → Configuration
//! d'instance → Default` now carries one [`AgentChoice`]: `Inherit` (continue
//! down the chain), a named `Profile` reference (a live pointer, resolved from
//! an atomic [`crate::agent_profile::snapshot`] taken once per resolution), or
//! `Custom` (a complete inline combination that does not reactivate the old
//! per-harness-map resolver — ADR-0057 ¶1). The first tier that is **explicit**
//! (`Profile` that resolves, or `Custom`) wins and supplies harness, model and
//! effort atomically — no field ever merges across tiers (#563 AC21).
//!
//! **Backward compatibility, by construction, not by migration.** #563 is
//! explicit that no pipeline is migrated automatically (out of scope). Every
//! tier this resolver reads still carries its **pre-#563** legacy signal too —
//! a node's `pin_harness` + per-harness `harnesses` map, a Run/Projet/instance's
//! bare `harness` string. [`resolve`] walks BOTH signals tier by tier: an
//! `AgentChoice` at a tier wins outright over anything coarser the moment it is
//! explicit; a tier with no `AgentChoice` (or `Inherit`) falls back to its
//! legacy harness-only signal, and if that names a harness, the OLD model/effort
//! rule applies — read from the node's own per-harness map, then the instance's
//! per-harness default map (exactly [`crate::harness_resolver::resolve`]'s
//! behaviour). A pipeline that never sets an `AgentChoice` anywhere therefore
//! resolves byte-identically to before #563.
//!
//! A `Profile` reference to an id absent from the snapshot **warns and behaves
//! as `Inherit`** (#563 AC13/AC14): the walk continues at that same tier's
//! legacy signal, then the next tier, never stopping there.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::harness_resolver::HarnessEntry;

/// One profile's combination: a required harness, an optional model, an
/// optional effort. The shape both a stored [`crate::agent_profile::AgentProfile`]
/// and an inline [`AgentChoice::Custom`] carry (ADR-0057 ¶1: "Custom porte la
/// même forme qu'un profil").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ResolvedCombo {
    pub harness: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
}

/// The exclusive union a tier carries (#563 AC: "chaque tier porte une union
/// exclusive"). Tagged on the wire by `mode`, so a stored/transmitted value is
/// self-describing and a missing/legacy value simply deserializes absent
/// (`Option<AgentChoice>` at the call site) rather than as some 4th variant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub(crate) enum AgentChoice {
    /// Continue the precedence chain at the next coarser tier.
    Inherit,
    /// A live reference to a named profile, resolved from the atomic snapshot
    /// at resolution time — never copied (ADR-0057 ¶0).
    Profile { profile_id: String },
    /// A complete, non-reusable inline combination. `model`/`effort` are
    /// optional like a stored profile's.
    Custom {
        harness: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        effort: Option<String>,
    },
}

/// Which tier ultimately supplied the resolved combination — frozen into the
/// start event so a resume can say where its combination came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Tier {
    Node,
    Run,
    Project,
    Instance,
    /// The reserved Default profile — the floor when every tier is transparent.
    Default,
}

/// How the winning tier supplied its combination.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Via {
    /// A resolved, existing named profile.
    Profile { profile_id: String },
    /// An inline `Custom` combination.
    Custom,
    /// The pre-#563 legacy signal (a bare harness string, model/effort read
    /// from the node's per-harness map / the instance's per-harness default).
    Legacy,
}

/// A profile reference that pointed at nothing in the snapshot — the walk
/// warned, then behaved as `Inherit` for that tier (#563 AC13).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MissingProfileWarning {
    pub tier: Tier,
    pub profile_id: String,
}

/// What the spawn seam launches with — the same shape
/// [`crate::harness_resolver::ResolvedHarness`] has, plus provenance and any
/// missing-profile warnings collected along the way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Resolved {
    pub combo: ResolvedCombo,
    pub tier: Tier,
    pub via: Via,
    pub warnings: Vec<MissingProfileWarning>,
}

/// Every tier's signal, new and legacy side by side. `node_harnesses` /
/// `instance_default_models` are the pre-#563 per-harness maps — consulted only
/// when the walk falls back to a *legacy* harness-only tier (never when an
/// `AgentChoice` wins explicitly). `None` reads the same as an empty map.
#[derive(Debug, Default, Clone)]
pub(crate) struct Tiers<'a> {
    pub node_choice: Option<&'a AgentChoice>,
    pub node_pin: Option<&'a str>,
    pub node_harnesses: Option<&'a BTreeMap<String, HarnessEntry>>,
    pub run_choice: Option<&'a AgentChoice>,
    pub run_harness: Option<&'a str>,
    pub project_choice: Option<&'a AgentChoice>,
    pub project_harness: Option<&'a str>,
    pub instance_choice: Option<&'a AgentChoice>,
    pub instance_default_harness: Option<&'a str>,
    pub instance_default_models: Option<&'a BTreeMap<String, String>>,
}

fn normalise(s: Option<&str>) -> Option<String> {
    s.map(str::trim).filter(|s| !s.is_empty()).map(String::from)
}

/// Resolve the effective combination for a spawn, walking the four tiers
/// finest-to-coarsest and falling through to the reserved Default profile.
///
/// `profiles` is the ONE atomic snapshot the caller took before this call
/// (ADR-0057 ¶4) — a `Profile` reference is looked up in it, never re-queried.
/// `default_profile_id` names the reserved floor profile
/// ([`crate::agent_profile::DEFAULT_PROFILE_ID`] in production code; a plain
/// literal in tests that don't want the whole `agent_profile` module).
pub(crate) fn resolve(
    tiers: &Tiers<'_>,
    profiles: &BTreeMap<String, ResolvedCombo>,
    default_profile_id: &str,
) -> Resolved {
    let mut warnings = Vec::new();
    let mut legacy_winner: Option<(Tier, String)> = None;

    let slots: [(Tier, Option<&AgentChoice>, Option<&str>); 4] = [
        (Tier::Node, tiers.node_choice, tiers.node_pin),
        (Tier::Run, tiers.run_choice, tiers.run_harness),
        (Tier::Project, tiers.project_choice, tiers.project_harness),
        (
            Tier::Instance,
            tiers.instance_choice,
            tiers.instance_default_harness,
        ),
    ];

    for (tier, choice, legacy_harness) in slots {
        match choice {
            Some(AgentChoice::Custom {
                harness,
                model,
                effort,
            }) if !harness.trim().is_empty() => {
                return Resolved {
                    combo: ResolvedCombo {
                        harness: harness.trim().to_string(),
                        model: normalise(model.as_deref()),
                        effort: normalise(effort.as_deref()),
                    },
                    tier,
                    via: Via::Custom,
                    warnings,
                };
            }
            Some(AgentChoice::Profile { profile_id }) if !profile_id.trim().is_empty() => {
                let profile_id = profile_id.trim();
                if let Some(combo) = profiles.get(profile_id) {
                    return Resolved {
                        combo: combo.clone(),
                        tier,
                        via: Via::Profile {
                            profile_id: profile_id.to_string(),
                        },
                        warnings,
                    };
                }
                // Absent: warn, then behave as Inherit for this tier (AC13/AC14).
                warnings.push(MissingProfileWarning {
                    tier,
                    profile_id: profile_id.to_string(),
                });
            }
            // Inherit, None, or a blank Custom/Profile payload: transparent.
            _ => {}
        }
        if legacy_winner.is_none() {
            if let Some(h) = legacy_harness {
                if !h.is_empty() {
                    legacy_winner = Some((tier, h.to_string()));
                    break;
                }
            }
        }
    }

    if let Some((tier, harness)) = legacy_winner {
        let entry = tiers.node_harnesses.and_then(|m| m.get(&harness));
        let model = normalise(entry.and_then(|e| e.model.as_deref())).or_else(|| {
            tiers
                .instance_default_models
                .and_then(|m| m.get(&harness))
                .map(String::as_str)
                .and_then(|s| normalise(Some(s)))
        });
        let effort = normalise(entry.and_then(|e| e.effort.as_deref()));
        return Resolved {
            combo: ResolvedCombo {
                harness,
                model,
                effort,
            },
            tier,
            via: Via::Legacy,
            warnings,
        };
    }

    // The floor: the reserved Default profile. Guaranteed present by
    // `agent_profile::init`, but a defensive literal `claude` avoids a panic if
    // ever called against an incomplete snapshot (e.g. a unit test that omits
    // it on purpose).
    let combo = profiles
        .get(default_profile_id)
        .cloned()
        .unwrap_or(ResolvedCombo {
            harness: crate::harness_registry::CLAUDE.to_string(),
            model: None,
            effort: None,
        });
    Resolved {
        combo,
        tier: Tier::Default,
        via: Via::Profile {
            profile_id: default_profile_id.to_string(),
        },
        warnings,
    }
}

/// The combination an **infra session** (Pipeline Manager, merge resolver)
/// launches with (#563 AC18, amending ADR-0046's Run-only-harness rule): it has
/// no NodeDef and no Projet of its own, so only `Run → instance → Default`
/// apply — but unlike before #563, a Run's *explicit* choice now supplies model
/// and effort too, not just the harness. Implemented as [`resolve`] with the
/// Node and Projet tiers transparent, which also means a legacy Run/instance
/// harness-only signal still yields NO model/effort here — byte-identical to
/// [`crate::harness_resolver::resolve_infra_harness`] when no `AgentChoice` is
/// ever set.
pub(crate) fn resolve_infra(
    run_choice: Option<&AgentChoice>,
    run_harness: Option<&str>,
    instance_choice: Option<&AgentChoice>,
    instance_default_harness: Option<&str>,
    profiles: &BTreeMap<String, ResolvedCombo>,
    default_profile_id: &str,
) -> Resolved {
    let tiers = Tiers {
        run_choice,
        run_harness,
        instance_choice,
        instance_default_harness,
        ..Default::default()
    };
    resolve(&tiers, profiles, default_profile_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness_registry::{CLAUDE, OPENCODE};

    const DEFAULT_ID: &str = "default";

    fn profiles_with_default() -> BTreeMap<String, ResolvedCombo> {
        let mut m = BTreeMap::new();
        m.insert(
            DEFAULT_ID.to_string(),
            ResolvedCombo {
                harness: CLAUDE.to_string(),
                model: None,
                effort: None,
            },
        );
        m
    }

    fn profile(harness: &str, model: Option<&str>, effort: Option<&str>) -> ResolvedCombo {
        ResolvedCombo {
            harness: harness.to_string(),
            model: model.map(String::from),
            effort: effort.map(String::from),
        }
    }

    #[test]
    fn every_tier_transparent_resolves_to_the_default_profile() {
        let tiers = Tiers::default();
        let r = resolve(&tiers, &profiles_with_default(), DEFAULT_ID);
        assert_eq!(r.combo.harness, CLAUDE);
        assert_eq!(r.combo.model, None);
        assert_eq!(r.combo.effort, None);
        assert_eq!(r.tier, Tier::Default);
        assert!(r.warnings.is_empty());
    }

    #[test]
    fn node_profile_choice_wins_over_everything_coarser() {
        let mut profiles = profiles_with_default();
        profiles.insert(
            "p1".to_string(),
            profile(OPENCODE, Some("m1"), Some("high")),
        );
        let node_choice = AgentChoice::Profile {
            profile_id: "p1".to_string(),
        };
        let run_choice = AgentChoice::Custom {
            harness: CLAUDE.to_string(),
            model: None,
            effort: None,
        };
        let tiers = Tiers {
            node_choice: Some(&node_choice),
            run_choice: Some(&run_choice),
            ..Default::default()
        };
        let r = resolve(&tiers, &profiles, DEFAULT_ID);
        assert_eq!(r.combo, profile(OPENCODE, Some("m1"), Some("high")));
        assert_eq!(r.tier, Tier::Node);
        assert_eq!(
            r.via,
            Via::Profile {
                profile_id: "p1".to_string()
            }
        );
    }

    #[test]
    fn run_custom_wins_over_project_and_instance() {
        let run_choice = AgentChoice::Custom {
            harness: OPENCODE.to_string(),
            model: Some("m2".to_string()),
            effort: None,
        };
        let project_choice = AgentChoice::Custom {
            harness: CLAUDE.to_string(),
            model: None,
            effort: None,
        };
        let tiers = Tiers {
            run_choice: Some(&run_choice),
            project_choice: Some(&project_choice),
            ..Default::default()
        };
        let r = resolve(&tiers, &profiles_with_default(), DEFAULT_ID);
        assert_eq!(r.combo, profile(OPENCODE, Some("m2"), None));
        assert_eq!(r.tier, Tier::Run);
        assert_eq!(r.via, Via::Custom);
    }

    #[test]
    fn project_profile_wins_over_instance() {
        let mut profiles = profiles_with_default();
        profiles.insert("pj".to_string(), profile(CLAUDE, Some("sonnet"), None));
        let project_choice = AgentChoice::Profile {
            profile_id: "pj".to_string(),
        };
        let instance_choice = AgentChoice::Custom {
            harness: OPENCODE.to_string(),
            model: None,
            effort: None,
        };
        let tiers = Tiers {
            project_choice: Some(&project_choice),
            instance_choice: Some(&instance_choice),
            ..Default::default()
        };
        let r = resolve(&tiers, &profiles, DEFAULT_ID);
        assert_eq!(r.combo, profile(CLAUDE, Some("sonnet"), None));
        assert_eq!(r.tier, Tier::Project);
    }

    #[test]
    fn instance_custom_wins_over_the_default_floor() {
        let instance_choice = AgentChoice::Custom {
            harness: OPENCODE.to_string(),
            model: Some("m3".to_string()),
            effort: Some("low".to_string()),
        };
        let tiers = Tiers {
            instance_choice: Some(&instance_choice),
            ..Default::default()
        };
        let r = resolve(&tiers, &profiles_with_default(), DEFAULT_ID);
        assert_eq!(r.combo, profile(OPENCODE, Some("m3"), Some("low")));
        assert_eq!(r.tier, Tier::Instance);
    }

    #[test]
    fn inherit_at_every_finer_tier_falls_through_to_the_winning_coarser_one() {
        let mut profiles = profiles_with_default();
        profiles.insert("pj".to_string(), profile(CLAUDE, None, None));
        let node_choice = AgentChoice::Inherit;
        let run_choice = AgentChoice::Inherit;
        let project_choice = AgentChoice::Profile {
            profile_id: "pj".to_string(),
        };
        let tiers = Tiers {
            node_choice: Some(&node_choice),
            run_choice: Some(&run_choice),
            project_choice: Some(&project_choice),
            ..Default::default()
        };
        let r = resolve(&tiers, &profiles, DEFAULT_ID);
        assert_eq!(r.tier, Tier::Project);
    }

    #[test]
    fn missing_profile_at_node_warns_and_falls_through_to_run() {
        let mut profiles = profiles_with_default();
        profiles.insert("run-p".to_string(), profile(OPENCODE, None, None));
        let node_choice = AgentChoice::Profile {
            profile_id: "gone".to_string(),
        };
        let run_choice = AgentChoice::Profile {
            profile_id: "run-p".to_string(),
        };
        let tiers = Tiers {
            node_choice: Some(&node_choice),
            run_choice: Some(&run_choice),
            ..Default::default()
        };
        let r = resolve(&tiers, &profiles, DEFAULT_ID);
        assert_eq!(r.combo, profile(OPENCODE, None, None));
        assert_eq!(r.tier, Tier::Run);
        assert_eq!(
            r.warnings,
            vec![MissingProfileWarning {
                tier: Tier::Node,
                profile_id: "gone".to_string()
            }]
        );
    }

    #[test]
    fn missing_profile_everywhere_still_resolves_to_the_default_floor_with_warnings() {
        let node_choice = AgentChoice::Profile {
            profile_id: "gone1".to_string(),
        };
        let run_choice = AgentChoice::Profile {
            profile_id: "gone2".to_string(),
        };
        let tiers = Tiers {
            node_choice: Some(&node_choice),
            run_choice: Some(&run_choice),
            ..Default::default()
        };
        let r = resolve(&tiers, &profiles_with_default(), DEFAULT_ID);
        assert_eq!(r.tier, Tier::Default);
        assert_eq!(r.warnings.len(), 2);
    }

    #[test]
    fn a_missing_profile_at_a_tier_that_also_carries_a_legacy_harness_still_uses_the_legacy_signal()
    {
        // A node whose Profile reference is broken but which ALSO still carries a
        // legacy pin: the tier keeps behaving as it did pre-#563 (Inherit), so
        // the pin still wins the harness — the warning does not blank the tier.
        let node_choice = AgentChoice::Profile {
            profile_id: "gone".to_string(),
        };
        let tiers = Tiers {
            node_choice: Some(&node_choice),
            node_pin: Some(CLAUDE),
            ..Default::default()
        };
        let r = resolve(&tiers, &profiles_with_default(), DEFAULT_ID);
        assert_eq!(r.combo.harness, CLAUDE);
        assert_eq!(r.tier, Tier::Node);
        assert_eq!(r.via, Via::Legacy);
        assert_eq!(r.warnings.len(), 1);
    }

    #[test]
    fn no_agent_choice_anywhere_reproduces_the_pre_563_harness_resolver_exactly() {
        let mut entries = BTreeMap::new();
        entries.insert(
            OPENCODE.to_string(),
            HarnessEntry {
                model: Some("openrouter/foo".to_string()),
                effort: None,
            },
        );
        let mut default_models = BTreeMap::new();
        default_models.insert(CLAUDE.to_string(), "sonnet".to_string());

        let tiers = Tiers {
            node_pin: Some(OPENCODE),
            node_harnesses: Some(&entries),
            instance_default_models: Some(&default_models),
            ..Default::default()
        };
        let r = resolve(&tiers, &profiles_with_default(), DEFAULT_ID);
        assert_eq!(r.combo.harness, OPENCODE);
        assert_eq!(r.combo.model.as_deref(), Some("openrouter/foo"));
        assert_eq!(r.combo.effort, None);
        assert_eq!(r.via, Via::Legacy);
        assert!(r.warnings.is_empty());
    }

    #[test]
    fn legacy_run_harness_wins_over_project_and_instance_and_reads_node_map_for_model() {
        let mut entries = BTreeMap::new();
        entries.insert(
            CLAUDE.to_string(),
            HarnessEntry {
                model: Some("opus".to_string()),
                effort: Some("high".to_string()),
            },
        );
        let tiers = Tiers {
            run_harness: Some(CLAUDE),
            project_harness: Some(OPENCODE),
            instance_default_harness: Some(OPENCODE),
            node_harnesses: Some(&entries),
            ..Default::default()
        };
        let r = resolve(&tiers, &profiles_with_default(), DEFAULT_ID);
        assert_eq!(r.combo.harness, CLAUDE);
        assert_eq!(r.combo.model.as_deref(), Some("opus"));
        assert_eq!(r.combo.effort.as_deref(), Some("high"));
        assert_eq!(r.tier, Tier::Run);
        assert_eq!(r.via, Via::Legacy);
    }

    #[test]
    fn empty_string_legacy_harness_never_wins_a_tier() {
        let tiers = Tiers {
            node_pin: Some(""),
            instance_default_harness: Some(""),
            ..Default::default()
        };
        let r = resolve(&tiers, &profiles_with_default(), DEFAULT_ID);
        assert_eq!(r.tier, Tier::Default);
        assert_eq!(r.combo.harness, CLAUDE);
    }

    #[test]
    fn a_custom_node_combo_never_merges_with_the_instance_defaults() {
        let mut default_models = BTreeMap::new();
        default_models.insert(OPENCODE.to_string(), "should-not-leak".to_string());
        let node_choice = AgentChoice::Custom {
            harness: OPENCODE.to_string(),
            model: None,
            effort: None,
        };
        let tiers = Tiers {
            node_choice: Some(&node_choice),
            instance_default_models: Some(&default_models),
            ..Default::default()
        };
        let r = resolve(&tiers, &profiles_with_default(), DEFAULT_ID);
        assert_eq!(r.combo.model, None, "Custom is atomic — no instance leak");
    }

    #[test]
    fn a_custom_with_blank_harness_is_treated_as_inherit() {
        let node_choice = AgentChoice::Custom {
            harness: "   ".to_string(),
            model: Some("x".to_string()),
            effort: None,
        };
        let tiers = Tiers {
            node_choice: Some(&node_choice),
            node_pin: Some(CLAUDE),
            ..Default::default()
        };
        let r = resolve(&tiers, &profiles_with_default(), DEFAULT_ID);
        assert_eq!(r.combo.harness, CLAUDE);
        assert_eq!(r.combo.model, None);
        assert_eq!(r.via, Via::Legacy);
    }

    #[test]
    fn infra_follows_the_runs_explicit_choice_model_and_effort_included() {
        let run_choice = AgentChoice::Custom {
            harness: OPENCODE.to_string(),
            model: Some("infra-model".to_string()),
            effort: Some("high".to_string()),
        };
        let r = resolve_infra(
            Some(&run_choice),
            None,
            None,
            None,
            &profiles_with_default(),
            DEFAULT_ID,
        );
        assert_eq!(
            r.combo,
            profile(OPENCODE, Some("infra-model"), Some("high"))
        );
        assert_eq!(r.tier, Tier::Run);
    }

    #[test]
    fn infra_with_only_legacy_run_harness_gets_no_model_or_effort() {
        // Byte-identical to `harness_resolver::resolve_infra_harness`: a legacy
        // (non-AgentChoice) Run harness carries no model/effort for infra.
        let r = resolve_infra(
            None,
            Some(OPENCODE),
            None,
            Some(CLAUDE),
            &profiles_with_default(),
            DEFAULT_ID,
        );
        assert_eq!(r.combo.harness, OPENCODE);
        assert_eq!(r.combo.model, None);
        assert_eq!(r.combo.effort, None);
    }

    #[test]
    fn infra_falls_through_to_instance_then_default_floor() {
        let r = resolve_infra(None, None, None, None, &profiles_with_default(), DEFAULT_ID);
        assert_eq!(r.combo.harness, CLAUDE);
        assert_eq!(r.tier, Tier::Default);

        let r2 = resolve_infra(
            None,
            None,
            None,
            Some(OPENCODE),
            &profiles_with_default(),
            DEFAULT_ID,
        );
        assert_eq!(r2.combo.harness, OPENCODE);
        assert_eq!(r2.tier, Tier::Instance);
    }

    #[test]
    fn agent_choice_serde_round_trips_all_three_modes() {
        for choice in [
            AgentChoice::Inherit,
            AgentChoice::Profile {
                profile_id: "p1".to_string(),
            },
            AgentChoice::Custom {
                harness: CLAUDE.to_string(),
                model: Some("opus".to_string()),
                effort: None,
            },
        ] {
            let json = serde_json::to_value(&choice).unwrap();
            let back: AgentChoice = serde_json::from_value(json).unwrap();
            assert_eq!(back, choice);
        }
    }

    #[test]
    fn agent_choice_wire_shape_is_mode_tagged() {
        let json = serde_json::to_value(AgentChoice::Profile {
            profile_id: "p1".to_string(),
        })
        .unwrap();
        assert_eq!(json["mode"], "profile");
        assert_eq!(json["profile_id"], "p1");

        let json = serde_json::to_value(AgentChoice::Custom {
            harness: CLAUDE.to_string(),
            model: None,
            effort: None,
        })
        .unwrap();
        assert_eq!(json["mode"], "custom");
        assert_eq!(json["harness"], CLAUDE);
        assert!(json.get("model").is_none());
    }
}
