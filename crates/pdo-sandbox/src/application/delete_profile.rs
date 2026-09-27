//! Delete a staging profile's row.

use pdo_kernel::{Actor, Verb};

use super::profiles::{ProfileError, SandboxProfiles};

impl SandboxProfiles {
    /// Drop the materialised row of `name`.
    ///
    /// **Unconditional** (ADR-0031 §7: a *soft* guard-rail, no referential integrity in
    /// the database). Deleting an edited `full`/`minimal` reverts it to its virtual
    /// default; deleting a user profile makes every future Run referencing it fail loud.
    /// Neither repoints anything — which the referents dialog must say before confirming.
    pub async fn delete(&self, actor: &Actor, name: &str) -> Result<(), ProfileError> {
        self.authorize(actor, Verb::Delete, Some(name))?;
        if self.store.delete(name).await? {
            Ok(())
        } else {
            Err(ProfileError::NotFound(format!(
                "no materialised staging profile named `{name}` \
                 (an unedited built-in default has no row to delete)"
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use pdo_kernel::{Actor, Verb};

    use crate::application::profiles::fixture::{refusing, with_home};
    use crate::application::profiles::ProfileError;
    use crate::domain::profile::ProfileDiff;

    fn row(name: &str) -> ProfileDiff {
        ProfileDiff {
            name: name.into(),
            disabled: Vec::new(),
            extras: Vec::new(),
            env: Default::default(),
            image: None,
            updated_at: "t".into(),
        }
    }

    #[tokio::test]
    async fn deletes_a_row_and_an_unedited_default_is_not_found() {
        let f = with_home(&[]);
        f.store.seed(row("team"));
        f.profiles
            .delete(&Actor::LocalOperator, "team")
            .await
            .unwrap();
        assert!(f.store.names().is_empty());
        let err = f
            .profiles
            .delete(&Actor::LocalOperator, "full")
            .await
            .unwrap_err();
        assert!(matches!(&err, ProfileError::NotFound(m) if m.contains("`full`")));
    }

    #[tokio::test]
    async fn a_refusal_keeps_the_row() {
        let (f, host) = refusing();
        f.store.seed(row("team"));
        let err = f
            .profiles
            .delete(&Actor::LocalOperator, "team")
            .await
            .unwrap_err();
        assert!(matches!(err, ProfileError::Denied(_)));
        assert_eq!(f.store.names(), vec!["team"]);
        let (_, op) = host.asked().pop().unwrap();
        assert_eq!(
            (op.verb, op.target.as_deref()),
            (Verb::Delete, Some("team"))
        );
    }
}
