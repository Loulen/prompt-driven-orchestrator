//! **Projets**: a named grouping of member repository paths, and the middle tier
//! of the harness precedence axis (ADR-0046, #552).
//!
//! A Projet is materialised **on demand**: no row exists until a human names a
//! group or attaches a setting (ADR-0046, same posture as the price table of
//! ADR-0034: nothing is ever seeded). Membership is a **verbatim** path
//! comparison: a repo path is never canonicalised (ADR-0033), so two spellings
//! of the same path are two paths and nothing reconciles them silently. A path
//! belongs to **at most one** Projet; a second attach is a **refusal that names
//! the owning Projet**, decided before any write.
//!
//! The Projet of a Run is the one that owns its **primary** repo path; a
//! secondary repo (ADR-0042) is never consulted, so adding or removing one can
//! change neither the Projet nor the resolved harness.

/// The stored JSON text of a setting a Projet carries **for another context**
/// (the agentic profile of `harness`, the skills selection of `skill`).
///
/// The workspace keeps it and hands it back, and never reads it: the context
/// that owns the setting decides its shape. That is what lets the Projet carry
/// those tiers without the workspace depending on the contexts downstream of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredSetting(pub String);

impl StoredSetting {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A persisted Projet: a named group of member paths and the optional settings
/// it carries. Each optional setting is `None` when the Projet states none: the
/// tier is then transparent to resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub id: String,
    pub name: String,
    /// The harness this Projet carries (ADR-0046). Never `Some("")`: a blank is
    /// stored as "none" (the `Some("")` trap of #347).
    pub harness: Option<String>,
    /// The **project** tier of the auto-fail preference (ADR-0049).
    pub auto_fail: Option<bool>,
    /// The Projet tier of the agentic-profile union (#563, ADR-0057).
    pub agent_choice: Option<StoredSetting>,
    /// The Projet tier of the skills selection (#669, ADR-0062). `None` when
    /// nothing is selected.
    pub skills: Option<StoredSetting>,
    /// Member repository paths, compared **verbatim** (ADR-0033), ordered by
    /// attach time.
    pub members: Vec<String>,
}

impl Project {
    /// A freshly named Projet: no member, no setting.
    pub fn named(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            harness: None,
            auto_fail: None,
            agent_choice: None,
            skills: None,
            members: Vec::new(),
        }
    }
}

/// The outcome of attaching a path to a Projet: the "at most one Projet per
/// path" invariant expressed as data, so the API can render the named refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddMember {
    Added,
    /// Already a member of **this** Projet: an idempotent no-op.
    AlreadyMember,
    /// Already a member of **another** Projet. No write happened; the owning
    /// Projet is named so the caller can say which one.
    Refused {
        owner_id: String,
        owner_name: String,
    },
}

/// Whether `path` may join `project_id`, given the Projet that owns it now.
/// `None` means the path is free and the attach may proceed.
pub fn membership_verdict(owner: Option<&Project>, project_id: &str) -> Option<AddMember> {
    let owner = owner?;
    Some(if owner.id == project_id {
        AddMember::AlreadyMember
    } else {
        AddMember::Refused {
            owner_id: owner.id.clone(),
            owner_name: owner.name.clone(),
        }
    })
}

/// A name a Projet may carry: trimmed, never blank.
pub fn project_name(raw: &str) -> Option<&str> {
    let name = raw.trim();
    (!name.is_empty()).then_some(name)
}

/// A harness a Projet may carry: a blank clears it (the `Some("")` trap of #347).
pub fn project_harness(raw: Option<&str>) -> Option<&str> {
    raw.filter(|s| !s.is_empty())
}

/// A new Projet id: `prj-<UTC timestamp>-<7 random hex chars>`.
pub fn project_id(timestamp: &str, random: &str) -> String {
    let short: String = random.chars().take(7).collect();
    format!("prj-{timestamp}-{short}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_free_path_may_join() {
        assert_eq!(membership_verdict(None, "p1"), None);
    }

    #[test]
    fn the_same_owner_is_an_idempotent_noop_and_another_one_is_named() {
        let alpha = Project::named("a", "Alpha");
        assert_eq!(
            membership_verdict(Some(&alpha), "a"),
            Some(AddMember::AlreadyMember)
        );
        assert_eq!(
            membership_verdict(Some(&alpha), "b"),
            Some(AddMember::Refused {
                owner_id: "a".into(),
                owner_name: "Alpha".into()
            })
        );
    }

    #[test]
    fn names_are_trimmed_and_never_blank() {
        assert_eq!(project_name("  Front  "), Some("Front"));
        assert_eq!(project_name("   "), None);
    }

    #[test]
    fn a_blank_harness_is_no_harness() {
        assert_eq!(project_harness(Some("")), None);
        assert_eq!(project_harness(Some("codex")), Some("codex"));
        assert_eq!(project_harness(None), None);
    }

    #[test]
    fn ids_carry_the_timestamp_and_seven_random_chars() {
        assert_eq!(
            project_id("20260926-120000", "abcdef0123"),
            "prj-20260926-120000-abcdef0"
        );
    }
}
