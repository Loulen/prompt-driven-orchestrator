//! Rename a Projet and (re)set the settings it carries. A PATCH never
//! materialises a Projet: only naming one does.

use pdo_kernel::{Actor, Operation, ResourceKind, Verb};

use crate::domain::project::{project_harness, project_name, Project, StoredSetting};

use super::error::{WorkspaceError, WorkspaceResult};
use super::ports::StorageError;
use super::workspace::Workspace;

/// What a PATCH changes. Each field is a double `Option`: **absent** (`None`)
/// leaves it untouched, `Some(None)` clears it, `Some(Some(v))` sets it.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ProjectUpdate {
    /// A rename. Never a clear: a Projet always has a name.
    pub name: Option<String>,
    /// A blank harness clears it, like `null`.
    pub harness: Option<Option<String>>,
    pub agent_choice: Option<Option<StoredSetting>>,
    /// The whole selection, replaced: `Some(None)` is the empty selection.
    pub skills: Option<Option<StoredSetting>>,
    pub auto_fail: Option<Option<bool>>,
    /// The default primary repository (ADR-0087): a member of the Projet;
    /// blank or `null` clears it.
    pub primary_repo: Option<Option<String>>,
}

impl Workspace {
    pub async fn update_project(
        &self,
        actor: &Actor,
        id: &str,
        update: ProjectUpdate,
    ) -> WorkspaceResult<Project> {
        let failed = |what: &str| {
            let what = what.to_string();
            move |e: StorageError| StorageError(format!("failed to {what} project {id}: {e}"))
        };
        let Some(current) = self.projects.get(id).await.map_err(failed("load"))? else {
            return Err(WorkspaceError::NotFound);
        };
        let primary_repo = update.primary_repo.as_ref().map(|repo| {
            repo.as_deref()
                .map(str::trim)
                .filter(|repo| !repo.is_empty())
        });
        if let Some(Some(repo)) = primary_repo {
            if !current.has_member(repo) {
                return Err(WorkspaceError::Invalid(format!(
                    "the default primary repository must be a member of the project: {repo}"
                )));
            }
        }
        let name = match update.name.as_deref() {
            Some(raw) => match project_name(raw) {
                Some(name) => Some(name),
                None => {
                    return Err(WorkspaceError::Invalid(
                        "project name cannot be blank".into(),
                    ))
                }
            },
            None => None,
        };
        self.authorize(
            actor,
            Operation::new(ResourceKind::Project, Verb::Update).on(id),
        )?;

        if let Some(name) = name {
            self.projects
                .rename(id, name)
                .await
                .map_err(failed("rename"))?;
        }
        if let Some(harness) = &update.harness {
            self.projects
                .set_harness(id, project_harness(harness.as_deref()))
                .await
                .map_err(failed("set harness on"))?;
        }
        if let Some(choice) = update.agent_choice {
            self.projects
                .set_agent_choice(id, choice)
                .await
                .map_err(failed("set agent_choice on"))?;
        }
        if let Some(skills) = update.skills {
            self.projects
                .set_skills(id, skills)
                .await
                .map_err(failed("set skills on"))?;
        }
        if let Some(auto_fail) = update.auto_fail {
            self.projects
                .set_auto_fail(id, auto_fail)
                .await
                .map_err(failed("set auto_fail on"))?;
        }
        if let Some(repo) = primary_repo {
            self.projects
                .set_primary_repo(id, repo)
                .await
                .map_err(failed("set primary_repo on"))?;
        }
        match self.projects.get(id).await.map_err(failed("reload"))? {
            Some(project) => {
                self.events.project_changed(id).await;
                Ok(project)
            }
            None => Err(WorkspaceError::NotFound),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::testing::{denying, fixture, Fixture};

    async fn seeded() -> (Fixture, String) {
        let f = fixture();
        let project = f
            .workspace
            .create_project(&Actor::LocalOperator, "Front", &[])
            .await
            .unwrap();
        let id = project.id;
        (f, id)
    }

    #[tokio::test]
    async fn renames_and_sets_every_setting_then_announces_once() {
        let (f, id) = seeded().await;
        let project = f
            .workspace
            .update_project(
                &Actor::LocalOperator,
                &id,
                ProjectUpdate {
                    name: Some(" Back ".into()),
                    harness: Some(Some("codex".into())),
                    agent_choice: Some(Some(StoredSetting("{\"mode\":\"inherit\"}".into()))),
                    skills: Some(Some(StoredSetting("[{\"id\":\"s\"}]".into()))),
                    auto_fail: Some(Some(true)),
                    primary_repo: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(project.name, "Back");
        assert_eq!(project.harness.as_deref(), Some("codex"));
        assert_eq!(project.auto_fail, Some(true));
        assert!(project.agent_choice.is_some() && project.skills.is_some());
        assert_eq!(f.events.changed(), vec![id.clone(), id]);
    }

    #[tokio::test]
    async fn null_and_blank_clear_and_absent_leaves_alone() {
        let (f, id) = seeded().await;
        let actor = Actor::LocalOperator;
        let set = ProjectUpdate {
            harness: Some(Some("codex".into())),
            auto_fail: Some(Some(false)),
            ..Default::default()
        };
        f.workspace.update_project(&actor, &id, set).await.unwrap();
        let blank = ProjectUpdate {
            harness: Some(Some(String::new())),
            ..Default::default()
        };
        let project = f
            .workspace
            .update_project(&actor, &id, blank)
            .await
            .unwrap();
        assert_eq!(project.harness, None);
        assert_eq!(project.auto_fail, Some(false), "absent leaves it alone");
        let clear = ProjectUpdate {
            auto_fail: Some(None),
            ..Default::default()
        };
        let project = f
            .workspace
            .update_project(&actor, &id, clear)
            .await
            .unwrap();
        assert_eq!(project.auto_fail, None);
    }

    #[tokio::test]
    async fn an_unknown_projet_is_not_found_and_never_created() {
        let f = fixture();
        let err = f
            .workspace
            .update_project(&Actor::LocalOperator, "nope", ProjectUpdate::default())
            .await
            .unwrap_err();
        assert_eq!(err, WorkspaceError::NotFound);
        assert!(f.projects.all().is_empty());
    }

    #[tokio::test]
    async fn a_blank_rename_is_refused_before_any_write() {
        let (f, id) = seeded().await;
        let update = ProjectUpdate {
            name: Some("  ".into()),
            harness: Some(Some("codex".into())),
            ..Default::default()
        };
        let err = f
            .workspace
            .update_project(&Actor::LocalOperator, &id, update)
            .await
            .unwrap_err();
        assert_eq!(
            err,
            WorkspaceError::Invalid("project name cannot be blank".into())
        );
        assert_eq!(f.projects.all()[0].harness, None);
    }

    #[tokio::test]
    async fn the_default_primary_is_a_member_and_a_blank_clears_it() {
        let f = fixture();
        let mut front = Project::named("p1", "Front");
        front.members = vec!["/repos/web".into(), "/repos/api".into()];
        f.projects.seed(front);
        let actor = Actor::LocalOperator;
        let set = |repo: &str| ProjectUpdate {
            primary_repo: Some(Some(repo.into())),
            ..Default::default()
        };
        let project = f
            .workspace
            .update_project(&actor, "p1", set(" /repos/api "))
            .await
            .unwrap();
        assert_eq!(project.primary_repo.as_deref(), Some("/repos/api"));
        let err = f
            .workspace
            .update_project(&actor, "p1", set("/repos/other"))
            .await
            .unwrap_err();
        assert!(matches!(err, WorkspaceError::Invalid(_)), "{err:?}");
        let project = f
            .workspace
            .update_project(&actor, "p1", set(""))
            .await
            .unwrap();
        assert_eq!(project.primary_repo, None);
    }

    #[tokio::test]
    async fn the_operation_targets_the_projet_and_a_refusal_writes_nothing() {
        let f = denying("read only");
        f.projects.seed(Project::named("p1", "Front"));
        let update = ProjectUpdate {
            name: Some("Back".into()),
            ..Default::default()
        };
        let err = f
            .workspace
            .update_project(&Actor::LocalOperator, "p1", update)
            .await
            .unwrap_err();
        assert!(matches!(err, WorkspaceError::Denied(_)));
        assert_eq!(f.projects.all()[0].name, "Front");
        let asked = f.host.asked();
        assert_eq!(asked[0].target.as_deref(), Some("p1"));
        assert_eq!(asked[0].verb, Verb::Update);
    }
}
