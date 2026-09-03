//! The **skills selected per tier** and their pure resolver into the **skills
//! effectifs** of a NodeRun (#669, spec #667, ADR-0062, CONTEXT.md §*Banque de
//! skills*).
//!
//! Four additive tiers carry the same key `skills`, a list of [`SkillRef`]
//! (`id` + `name`): the Configuration d'instance, the Projet owning the Run's
//! primary repo, the Run (seeded from the Trigger for a fired Run) and the Node of
//! the pipeline document. Since #672 the first three are resolved ONCE at create
//! and frozen on `RunStarted` (`frozen_skills`, see `skill_delivery`); a node spawn
//! adds only its own tier to that base. Unlike the
//! agentic profile (`agent_choice`, ADR-0057) where the finest explicit tier
//! *wins*, skills are a **strict additive union**: no tier removes a skill an
//! outer tier selected (CONTEXT.md: « aucun tier ne retire un skill hérité »).
//!
//! [`resolve`] is a function without side effects, called from the same spawn
//! seam as `agent_choice::resolve` (`node_spawn`), against a snapshot of the
//! bank's `id → name` map taken once per spawn. Identity is the **id**: the
//! `name` a tier stores is a label written beside it for a readable document and
//! is re-read from the bank at resolution (a rename never breaks a selection,
//! #668 AC). An id absent from the bank is a **warning**, never a refusal: the
//! node runs without that skill and the pipeline banner says so (#669 AC).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// One selected skill as a tier stores it: the stable id, and the label the
/// selector showed when the choice was made. `name` is informative only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SkillRef {
    pub id: String,
    #[serde(default)]
    pub name: String,
}

/// Which tier selected a skill. Ordered coarsest → finest, the order the
/// selectors list inherited skills in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SkillTier {
    Instance,
    Project,
    Run,
    Node,
}

/// Every tier's selection side by side. `None` reads as empty.
#[derive(Debug, Default, Clone)]
pub(crate) struct SkillTiers<'a> {
    pub instance: Option<&'a [SkillRef]>,
    pub project: Option<&'a [SkillRef]>,
    pub run: Option<&'a [SkillRef]>,
    pub node: Option<&'a [SkillRef]>,
}

/// One effective skill: the id, its **current** bank name, and every tier that
/// selected it (a skill checked at two tiers is delivered once, attributed to
/// both).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct EffectiveSkill {
    pub id: String,
    pub name: String,
    pub tiers: Vec<SkillTier>,
}

/// A selected id the bank no longer knows. The label is the stored one, so the
/// warning can still name what was meant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MissingSkill {
    pub id: String,
    pub name: String,
    pub tiers: Vec<SkillTier>,
}

/// The outcome of [`resolve`]: what the NodeRun receives, and what it was
/// promised but cannot get.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ResolvedSkills {
    pub skills: Vec<EffectiveSkill>,
    pub missing: Vec<MissingSkill>,
}

/// Union the four tiers, coarsest first, de-duplicated by id. `bank` is the
/// `id → name` snapshot: a known id takes the bank's name (labels are not
/// identity), an unknown id lands in `missing` with the stored label.
pub(crate) fn resolve(tiers: &SkillTiers<'_>, bank: &BTreeMap<String, String>) -> ResolvedSkills {
    // Insertion-ordered accumulation: first tier that names an id decides its
    // position, later tiers only add themselves to `tiers`.
    let mut order: Vec<String> = Vec::new();
    let mut seen: BTreeMap<String, (String, Vec<SkillTier>)> = BTreeMap::new();

    let slots: [(SkillTier, Option<&[SkillRef]>); 4] = [
        (SkillTier::Instance, tiers.instance),
        (SkillTier::Project, tiers.project),
        (SkillTier::Run, tiers.run),
        (SkillTier::Node, tiers.node),
    ];
    for (tier, refs) in slots {
        for skill in refs.unwrap_or(&[]) {
            let id = skill.id.trim();
            if id.is_empty() {
                continue;
            }
            match seen.get_mut(id) {
                Some((_, tiers)) => {
                    if !tiers.contains(&tier) {
                        tiers.push(tier);
                    }
                }
                None => {
                    order.push(id.to_string());
                    seen.insert(id.to_string(), (skill.name.trim().to_string(), vec![tier]));
                }
            }
        }
    }

    let mut out = ResolvedSkills::default();
    for id in order {
        let (label, tiers) = seen.remove(&id).expect("every ordered id was inserted");
        match bank.get(&id) {
            Some(name) => out.skills.push(EffectiveSkill {
                id,
                name: name.clone(),
                tiers,
            }),
            None => out.missing.push(MissingSkill {
                id,
                name: label,
                tiers,
            }),
        }
    }
    out
}

/// Deserialize a tier's stored JSON list (a nullable TEXT column). `NULL`,
/// empty or unparseable ⇒ an empty selection — the tier is transparent, never an
/// error (the same degrade-to-empty as `default_harness_model`).
pub(crate) fn from_stored_json(stored: Option<String>) -> Vec<SkillRef> {
    stored
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .and_then(|s| serde_json::from_str::<Vec<SkillRef>>(s).ok())
        .unwrap_or_default()
}

/// Serialize a selection for a nullable TEXT column: an empty list stores
/// `NULL`, so "nothing selected" and "never set" read the same.
pub(crate) fn to_stored_json(skills: &[SkillRef]) -> Option<String> {
    if skills.is_empty() {
        None
    } else {
        serde_json::to_string(skills).ok()
    }
}

/// Normalise a selection as received on the wire: trim ids, drop blanks, keep
/// the first occurrence of each id.
pub(crate) fn normalise(skills: Vec<SkillRef>) -> Vec<SkillRef> {
    let mut seen = std::collections::BTreeSet::new();
    skills
        .into_iter()
        .filter_map(|s| {
            let id = s.id.trim().to_string();
            if id.is_empty() || !seen.insert(id.clone()) {
                return None;
            }
            Some(SkillRef {
                id,
                name: s.name.trim().to_string(),
            })
        })
        .collect()
}

/// Does `skills` select `id`?
pub(crate) fn selects(skills: &[SkillRef], id: &str) -> bool {
    skills.iter().any(|s| s.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(id: &str, name: &str) -> SkillRef {
        SkillRef {
            id: id.into(),
            name: name.into(),
        }
    }

    fn bank(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
        entries
            .iter()
            .map(|(id, name)| (id.to_string(), name.to_string()))
            .collect()
    }

    #[test]
    fn union_is_additive_and_attributes_each_skill_to_its_tiers() {
        let instance = [r("a", "tdd")];
        let project = [r("b", "grilling")];
        let node = [r("c", "code-review")];
        let bank = bank(&[("a", "tdd"), ("b", "grilling"), ("c", "code-review")]);
        let resolved = resolve(
            &SkillTiers {
                instance: Some(&instance),
                project: Some(&project),
                run: None,
                node: Some(&node),
            },
            &bank,
        );
        assert_eq!(
            resolved.skills,
            vec![
                EffectiveSkill {
                    id: "a".into(),
                    name: "tdd".into(),
                    tiers: vec![SkillTier::Instance]
                },
                EffectiveSkill {
                    id: "b".into(),
                    name: "grilling".into(),
                    tiers: vec![SkillTier::Project]
                },
                EffectiveSkill {
                    id: "c".into(),
                    name: "code-review".into(),
                    tiers: vec![SkillTier::Node]
                },
            ]
        );
        assert!(resolved.missing.is_empty());
    }

    #[test]
    fn a_skill_selected_at_two_tiers_is_delivered_once_with_both_origins() {
        let instance = [r("a", "tdd")];
        let run = [r("a", "tdd"), r("d", "docs")];
        let bank = bank(&[("a", "tdd"), ("d", "docs")]);
        let resolved = resolve(
            &SkillTiers {
                instance: Some(&instance),
                run: Some(&run),
                ..Default::default()
            },
            &bank,
        );
        assert_eq!(resolved.skills.len(), 2);
        assert_eq!(resolved.skills[0].id, "a");
        assert_eq!(
            resolved.skills[0].tiers,
            vec![SkillTier::Instance, SkillTier::Run]
        );
        assert_eq!(resolved.skills[1].tiers, vec![SkillTier::Run]);
    }

    #[test]
    fn the_bank_name_wins_over_the_stored_label() {
        let node = [r("a", "old-label")];
        let bank = bank(&[("a", "renamed")]);
        let resolved = resolve(
            &SkillTiers {
                node: Some(&node),
                ..Default::default()
            },
            &bank,
        );
        assert_eq!(resolved.skills[0].name, "renamed");
    }

    #[test]
    fn an_unknown_id_is_a_warning_not_a_refusal() {
        let project = [r("gone", "deleted-skill")];
        let node = [r("a", "tdd")];
        let bank = bank(&[("a", "tdd")]);
        let resolved = resolve(
            &SkillTiers {
                project: Some(&project),
                node: Some(&node),
                ..Default::default()
            },
            &bank,
        );
        assert_eq!(resolved.skills.len(), 1);
        assert_eq!(
            resolved.missing,
            vec![MissingSkill {
                id: "gone".into(),
                name: "deleted-skill".into(),
                tiers: vec![SkillTier::Project]
            }]
        );
    }

    #[test]
    fn empty_tiers_and_blank_ids_are_transparent() {
        let node = [r("  ", "blank"), r("a", "tdd")];
        let bank = bank(&[("a", "tdd")]);
        let resolved = resolve(
            &SkillTiers {
                node: Some(&node),
                ..Default::default()
            },
            &bank,
        );
        assert_eq!(resolved.skills.len(), 1);
        assert!(resolve(&SkillTiers::default(), &bank).skills.is_empty());
    }

    #[test]
    fn stored_json_round_trips_and_degrades_to_empty() {
        let skills = vec![r("a", "tdd"), r("b", "grilling")];
        let stored = to_stored_json(&skills).unwrap();
        assert_eq!(from_stored_json(Some(stored)), skills);
        assert_eq!(to_stored_json(&[]), None);
        assert!(from_stored_json(None).is_empty());
        assert!(from_stored_json(Some("not json".into())).is_empty());
        assert!(from_stored_json(Some("   ".into())).is_empty());
    }

    #[test]
    fn normalise_trims_dedupes_and_drops_blanks() {
        let out = normalise(vec![
            r(" a ", " tdd "),
            r("a", "dup"),
            r("", "blank"),
            r("b", "x"),
        ]);
        assert_eq!(out, vec![r("a", "tdd"), r("b", "x")]);
        assert!(selects(&out, "b"));
        assert!(!selects(&out, "z"));
    }

    #[test]
    fn tier_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&SkillTier::Instance).unwrap(),
            "\"instance\""
        );
        let e: EffectiveSkill =
            serde_json::from_str(r#"{"id":"a","name":"tdd","tiers":["project","node"]}"#).unwrap();
        assert_eq!(e.tiers, vec![SkillTier::Project, SkillTier::Node]);
    }
}
