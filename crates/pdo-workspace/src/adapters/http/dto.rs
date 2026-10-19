//! The Projet as it travels over HTTP, and the bodies the project routes read.

use pdo_kernel::attributes::Attributes;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::application::ProjectUpdate;
use crate::domain::project::{Project, StoredSetting};

use super::{deserialize_double_option, ProjectSettings};

/// A persisted Projet: a named group of member paths and the optional settings it
/// carries. Each optional setting is absent when the Projet states none — the
/// tier is then transparent to resolution.
///
/// `A` is the agentic-profile union and `S` a selected skill, as the contexts
/// that own them define them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, rename = "Project")]
pub struct ProjectDto<A, S> {
    pub id: String,
    pub name: String,
    /// The harness this Projet carries (ADR-0046). An empty string is treated as
    /// "unset" by the resolver seam (the `Some("")` trap of #347).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub harness: Option<String>,
    /// The **project** tier of the auto-fail preference (ADR-0049).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub auto_fail: Option<bool>,
    /// The Projet tier of the agentic-profile union (#563, ADR-0057). When set it
    /// wins outright at this tier over `harness` (never merges).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub agent_choice: Option<A>,
    /// The Projet tier of the **skills** selection (#669, ADR-0062): unioned with
    /// the instance, Run and Node tiers for every Run whose primary repo this
    /// Projet owns. Empty ⇒ transparent.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<S>,
    /// Member repository paths, compared **verbatim** (ADR-0033). Ordered by
    /// attach time. A path may be a member of several Projets (ADR-0087).
    #[serde(default)]
    pub members: Vec<String>,
    /// The member a launch in this Projet preselects as its primary
    /// repository (ADR-0087): the one chosen, else the first member. Absent
    /// for a Projet without members.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub primary_repo: Option<String>,
    /// The entity attributes the extensions stamped on it (CONTEXT.md §
    /// *Extensions*); absent when none.
    #[serde(default, skip_serializing_if = "Attributes::is_empty")]
    #[ts(as = "Option<Attributes>", optional)]
    pub attributes: Attributes,
}

impl<A, S> ProjectDto<A, S> {
    /// The wire form of `project`, its settings decoded by the contexts that own
    /// them.
    pub fn from_project<P>(project: Project) -> Self
    where
        P: ProjectSettings<AgentChoice = A, Skill = S>,
    {
        let primary_repo = project.default_primary().map(str::to_string);
        Self {
            agent_choice: project
                .agent_choice
                .as_ref()
                .and_then(|stored| P::decode_agent_choice(stored.as_str())),
            skills: P::decode_skills(project.skills.as_ref().map(StoredSetting::as_str)),
            id: project.id,
            name: project.name,
            harness: project.harness,
            auto_fail: project.auto_fail,
            primary_repo,
            members: project.members,
            attributes: project.attributes,
        }
    }
}

/// Body of `POST /projects` — the only path that creates a `projects` row from a
/// name. `members` (optional) are the repo paths the named group starts with, so
/// naming a group is one `Create` Opération; the harness and the other settings
/// are attached afterwards.
#[derive(Deserialize)]
pub(crate) struct CreateProjectRequest {
    pub name: String,
    #[serde(default)]
    pub members: Vec<String>,
}

/// Body of `PATCH /projects/{id}` — rename the Projet and/or (re)set the settings
/// it carries. Every setting is a double-`Option`: **absent** leaves it untouched,
/// `null` clears it, a value sets it. An empty harness clears it too — don't let
/// `Some("")` through as a harness name.
#[derive(Deserialize)]
#[serde(bound(deserialize = "A: Deserialize<'de>, S: Deserialize<'de>"))]
pub(crate) struct PatchProjectRequest<A, S> {
    #[serde(default)]
    name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_double_option")]
    harness: Option<Option<String>>,
    /// The Projet's `AgentChoice` (ADR-0057). `null` clears it, falling through to
    /// the legacy `harness` field and then to the instance tier.
    #[serde(default, deserialize_with = "deserialize_double_option")]
    agent_choice: Option<Option<A>>,
    /// The Projet's skills selection (#669), replaced wholesale: absent leaves it,
    /// a list sets it, an empty list clears it (flat — the empty list IS the clear).
    #[serde(default)]
    skills: Option<Vec<S>>,
    /// The Projet's `auto_fail` (ADR-0049); `null` states no preference.
    #[serde(default, deserialize_with = "deserialize_double_option")]
    auto_fail: Option<Option<bool>>,
    /// The default primary repository (ADR-0087), a member of the Projet;
    /// `null` or blank goes back to the first member.
    #[serde(default, deserialize_with = "deserialize_double_option")]
    primary_repo: Option<Option<String>>,
}

impl<A, S> PatchProjectRequest<A, S> {
    /// The update to apply, each setting encoded the way it is stored.
    pub(crate) fn into_update<P>(self) -> ProjectUpdate
    where
        P: ProjectSettings<AgentChoice = A, Skill = S>,
    {
        ProjectUpdate {
            name: self.name,
            harness: self.harness,
            agent_choice: self
                .agent_choice
                .map(|choice| choice.map(|c| StoredSetting(P::encode_agent_choice(&c)))),
            skills: self
                .skills
                .map(|skills| P::encode_skills(skills).map(StoredSetting)),
            auto_fail: self.auto_fail,
            primary_repo: self.primary_repo,
        }
    }
}

/// Body of `POST|DELETE /projects/{id}/members`. The path is compared
/// **verbatim** (ADR-0033). A path may be a member of several Projets
/// (ADR-0087): the attach and the detach touch the named Projet only.
#[derive(Deserialize)]
pub(crate) struct ProjectMemberRequest {
    pub path: String,
}
