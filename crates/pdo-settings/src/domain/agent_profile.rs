//! A **profil agentique** — an instance-scoped, named, reusable combination of
//! a required harness with an optional model and effort (#563, ADR-0057,
//! CONTEXT.md §*Profil agentique*).
//!
//! A profile's identity is its `id`, stable and distinct from its `name`: a
//! rename never breaks a referent, which is why every reference PDO stores
//! (on a node, a Run, a Projet, the instance) is an `id`, never a `name`
//! (ADR-0057 ¶2).
//!
//! Every instance carries a **reserved** profile, [`DEFAULT_PROFILE_ID`]: named
//! `Default`, harness `claude`, no model, no effort. It is the floor of the
//! whole precedence chain — modifiable and renamable like any other profile,
//! but never deletable (an operator must always have a plancher to fall back
//! to, ADR-0057 ¶3).
//!
//! Names are unique **case-insensitively** (ADR-0057 ¶5, #563 AC25) so a picker
//! never shows two entries a human cannot tell apart; the refusal names the
//! clashing profile, which a bare constraint violation could not.

use std::fmt;

use pdo_harness::agent_choice::ResolvedCombo;
use pdo_kernel::attributes::Attributes;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The reserved id of the instance's floor profile (ADR-0057 ¶3). Distinct from
/// its `name` (`Default`), which is editable — only the id is fixed forever, so
/// a rename can never orphan the plancher every resolution falls back to.
pub const DEFAULT_PROFILE_ID: &str = pdo_harness::agent_choice::DEFAULT_PROFILE_ID;
/// The seeded name of the reserved profile. Editable afterwards like any other
/// profile's name (still subject to the case-insensitive uniqueness check).
pub const DEFAULT_PROFILE_NAME: &str = "Default";

/// A persisted agent profile: a stable `id`, a case-insensitively unique `name`,
/// a required `harness`, and an optional `model` / `effort` — the same shape a
/// node's `Custom` choice carries inline (ADR-0057 ¶1: "Custom porte la même
/// forme qu'un profil").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AgentProfile {
    pub id: String,
    pub name: String,
    pub harness: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub effort: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    /// The entity attributes the extensions stamped on it (CONTEXT.md §
    /// *Extensions*); absent when none.
    #[serde(default, skip_serializing_if = "Attributes::is_empty")]
    #[ts(as = "Option<Attributes>", optional)]
    pub attributes: Attributes,
}

impl AgentProfile {
    /// This profile's combination, the shape the snapshot of every profile indexes by id.
    pub fn combo(&self) -> ResolvedCombo {
        ResolvedCombo {
            harness: self.harness.clone(),
            model: self.model.clone(),
            effort: self.effort.clone(),
        }
    }
}

/// Why a create/update/delete was refused — named so the HTTP layer can turn
/// each variant into the right status code and an actionable message, the same
/// split the sandbox profile validators uses (message strings) but as a proper
/// enum here because the caller (delete) also needs to branch on *which* refusal
/// happened, not just render it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentProfileError {
    EmptyName,
    /// The harness is blank once trimmed — required (#563 AC2).
    EmptyHarness,
    /// Another profile already carries this name, case-insensitively.
    DuplicateName {
        existing_id: String,
        name: String,
    },
    NotFound,
    /// [`DEFAULT_PROFILE_ID`] can never be deleted (ADR-0057 ¶3 / #563 AC10).
    DefaultUndeletable,
    Storage(String),
}

impl fmt::Display for AgentProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyName => write!(f, "a profile name cannot be blank"),
            Self::EmptyHarness => write!(f, "a profile's harness cannot be blank"),
            Self::DuplicateName { name, .. } => write!(
                f,
                "a profile named `{name}` already exists (names are unique \
                 case-insensitively)"
            ),
            Self::NotFound => write!(f, "no such agent profile"),
            Self::DefaultUndeletable => write!(
                f,
                "the `{DEFAULT_PROFILE_NAME}` profile is the instance's reserved \
                 floor and cannot be deleted — rename or edit it instead"
            ),
            Self::Storage(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for AgentProfileError {}

/// A profile as a create or an edit writes it: `name` and `harness` trimmed and
/// required, `model` and `effort` trimmed, an empty one stored as unset — the
/// same `""`-is-unset discipline every free-text setting follows (#347).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileDraft {
    pub name: String,
    pub harness: String,
    pub model: Option<String>,
    pub effort: Option<String>,
}

impl ProfileDraft {
    /// Normalise a request, refusing a blank name or harness (#563 AC2).
    pub fn new(
        name: &str,
        harness: &str,
        model: Option<&str>,
        effort: Option<&str>,
    ) -> Result<Self, AgentProfileError> {
        let name = name.trim().to_string();
        if name.is_empty() {
            return Err(AgentProfileError::EmptyName);
        }
        let harness = harness.trim().to_string();
        if harness.is_empty() {
            return Err(AgentProfileError::EmptyHarness);
        }
        let unset_if_blank = |value: Option<&str>| {
            value
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
        };
        Ok(Self {
            name,
            harness,
            model: unset_if_blank(model),
            effort: unset_if_blank(effort),
        })
    }
}

/// Whether `existing`, the profile carrying the draft's name, clashes with it:
/// always, unless it is the very profile being edited (`editing`).
pub fn name_clash(existing: &AgentProfile, editing: Option<&str>) -> Option<AgentProfileError> {
    (Some(existing.id.as_str()) != editing).then(|| AgentProfileError::DuplicateName {
        existing_id: existing.id.clone(),
        name: existing.name.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(id: &str, name: &str) -> AgentProfile {
        AgentProfile {
            id: id.into(),
            name: name.into(),
            harness: "claude".into(),
            model: None,
            effort: None,
            created_at: String::new(),
            updated_at: String::new(),
            attributes: Default::default(),
        }
    }

    #[test]
    fn empty_name_or_harness_is_refused() {
        assert_eq!(
            ProfileDraft::new("  ", "claude", None, None).unwrap_err(),
            AgentProfileError::EmptyName
        );
        assert_eq!(
            ProfileDraft::new("X", "  ", None, None).unwrap_err(),
            AgentProfileError::EmptyHarness
        );
    }

    #[test]
    fn empty_model_and_effort_normalise_to_none() {
        let draft = ProfileDraft::new(" P ", " claude ", Some(""), Some("  ")).unwrap();
        assert_eq!(draft.name, "P");
        assert_eq!(draft.harness, "claude");
        assert_eq!((draft.model, draft.effort), (None, None));
        let draft = ProfileDraft::new("P", "claude", Some(" opus "), Some("high")).unwrap();
        assert_eq!(draft.model.as_deref(), Some("opus"));
        assert_eq!(draft.effort.as_deref(), Some("high"));
    }

    #[test]
    fn a_name_clashes_with_another_profile_but_not_with_itself() {
        let alpha = profile("agp-a", "Alpha");
        assert_eq!(
            name_clash(&alpha, None),
            Some(AgentProfileError::DuplicateName {
                existing_id: "agp-a".into(),
                name: "Alpha".into()
            })
        );
        assert!(name_clash(&alpha, Some("agp-b")).is_some());
        assert_eq!(name_clash(&alpha, Some("agp-a")), None);
    }

    #[test]
    fn the_refusals_say_what_to_do() {
        let dup = AgentProfileError::DuplicateName {
            existing_id: "x".into(),
            name: "Reviewer".into(),
        };
        assert!(dup.to_string().contains("`Reviewer` already exists"));
        assert!(AgentProfileError::DefaultUndeletable
            .to_string()
            .contains("`Default` profile"));
    }
}
