//! **Projets**: a named grouping of member repository paths, and the middle tier
//! of the harness precedence axis (ADR-0046, #552).
//!
//! A Projet is materialised **on demand**: no row exists until a human names a
//! group or attaches a setting (ADR-0046, same posture as the price table of
//! ADR-0034: nothing is ever seeded). Membership is a **verbatim** path
//! comparison: a repo path is never canonicalised (ADR-0033), so two spellings
//! of the same path are two paths and nothing reconciles them silently. A path
//! may belong to **several** Projets (ADR-0087), and each Projet names its
//! **default primary** repository, the one New Run preselects.
//!
//! The Projet of a Run is **chosen at launch and recorded on the Run**
//! (ADR-0087): changing the members of a Projet never moves a Run.

use pdo_kernel::attributes::Attributes;

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

/// What stays of a deleted Projet (CORE-96): its id and its last name. The
/// Runs launched in it keep it (ADR-0087), and the lists still name it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeletedProject {
    pub id: String,
    pub name: String,
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
    /// The member a launch in this Projet preselects as its primary
    /// repository, when one was chosen ([`Project::default_primary`]).
    pub primary_repo: Option<String>,
    /// The entity attributes the extensions stamped on it (CONTEXT.md §
    /// *Extensions*), when the read asked for them.
    pub attributes: Attributes,
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
            primary_repo: None,
            attributes: Attributes::new(),
        }
    }

    /// Whether `path` is a member, compared verbatim (ADR-0033).
    pub fn has_member(&self, path: &str) -> bool {
        self.members.iter().any(|member| member == path)
    }

    /// The repository a launch in this Projet preselects: the chosen default
    /// primary while it is still a member, else the first member attached.
    /// `None` for a Projet without members.
    pub fn default_primary(&self) -> Option<&str> {
        self.primary_repo
            .as_deref()
            .filter(|repo| self.has_member(repo))
            .or_else(|| self.members.first().map(String::as_str))
    }
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

    fn with_members(members: &[&str]) -> Project {
        let mut project = Project::named("a", "Alpha");
        project.members = members.iter().map(|m| m.to_string()).collect();
        project
    }

    #[test]
    fn the_default_primary_is_the_chosen_member_else_the_first_one() {
        let mut project = with_members(&["/repos/web", "/repos/api"]);
        assert_eq!(project.default_primary(), Some("/repos/web"));
        project.primary_repo = Some("/repos/api".into());
        assert_eq!(project.default_primary(), Some("/repos/api"));
    }

    #[test]
    fn a_default_primary_no_longer_member_falls_back_to_the_first_member() {
        let mut project = with_members(&["/repos/web"]);
        project.primary_repo = Some("/repos/gone".into());
        assert_eq!(project.default_primary(), Some("/repos/web"));
        assert_eq!(with_members(&[]).default_primary(), None);
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
