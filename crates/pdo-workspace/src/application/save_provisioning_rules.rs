//! Store the provisioning rules of the instance or of a Projet.

use pdo_kernel::{Actor, Operation, ResourceKind, Verb};

use crate::domain::provisioning::{ProvisioningRules, ProvisioningScope};

use super::error::{WorkspaceError, WorkspaceResult};
use super::ports::StorageError;
use super::workspace::Workspace;

/// The scope id the instance's rules are stored under.
pub const INSTANCE_SCOPE_ID: &str = "instance";

/// Whose rules are being saved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RulesOwner {
    Instance,
    Project(String),
}

impl Workspace {
    pub async fn save_provisioning_rules(
        &self,
        actor: &Actor,
        owner: RulesOwner,
        rules: ProvisioningRules,
    ) -> WorkspaceResult<ProvisioningRules> {
        let (scope, scope_id, operation) = match &owner {
            RulesOwner::Instance => (
                ProvisioningScope::Instance,
                INSTANCE_SCOPE_ID,
                Operation::new(ResourceKind::InstanceSettings, Verb::Update),
            ),
            RulesOwner::Project(id) => {
                let exists = self
                    .projects
                    .get(id)
                    .await
                    .map_err(|e| StorageError(format!("failed to load project {id}: {e}")))?;
                if exists.is_none() {
                    return Err(WorkspaceError::NotFound);
                }
                (
                    ProvisioningScope::Project,
                    id.as_str(),
                    Operation::new(ResourceKind::Project, Verb::Update).on(id),
                )
            }
        };
        self.authorize(actor, operation)?;
        self.provisioning
            .save(scope, scope_id, &rules)
            .await
            .map_err(|e| {
                StorageError(format!(
                    "failed to save {} provisioning rules: {e}",
                    if scope_id == INSTANCE_SCOPE_ID {
                        "instance"
                    } else {
                        "project"
                    }
                ))
            })?;
        Ok(rules)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::testing::{denying, fixture};
    use crate::domain::project::Project;

    fn rules(copy: &str) -> ProvisioningRules {
        ProvisioningRules {
            copy: vec![copy.into()],
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn saves_instance_and_project_rules_under_their_scope() {
        let f = fixture();
        f.projects.seed(Project::named("p1", "Front"));
        let actor = Actor::LocalOperator;
        f.workspace
            .save_provisioning_rules(&actor, RulesOwner::Instance, rules(".env"))
            .await
            .unwrap();
        let saved = f
            .workspace
            .save_provisioning_rules(&actor, RulesOwner::Project("p1".into()), rules("x"))
            .await
            .unwrap();
        assert_eq!(saved, rules("x"));
        assert_eq!(
            f.workspace
                .provisioning_rules(ProvisioningScope::Instance, INSTANCE_SCOPE_ID)
                .await
                .unwrap(),
            rules(".env")
        );
        assert_eq!(
            f.workspace
                .provisioning_rules(ProvisioningScope::Project, "p1")
                .await
                .unwrap(),
            rules("x")
        );
    }

    #[tokio::test]
    async fn an_unknown_projet_is_not_found_and_stores_nothing() {
        let f = fixture();
        let err = f
            .workspace
            .save_provisioning_rules(
                &Actor::LocalOperator,
                RulesOwner::Project("nope".into()),
                rules("x"),
            )
            .await
            .unwrap_err();
        assert_eq!(err, WorkspaceError::NotFound);
        assert!(f
            .workspace
            .provisioning_rules(ProvisioningScope::Project, "nope")
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn the_instance_rules_are_an_instance_settings_operation() {
        let f = denying("no");
        let err = f
            .workspace
            .save_provisioning_rules(&Actor::LocalOperator, RulesOwner::Instance, rules("x"))
            .await
            .unwrap_err();
        assert!(matches!(err, WorkspaceError::Denied(_)));
        assert_eq!(f.host.asked()[0].resource, ResourceKind::InstanceSettings);
    }
}
