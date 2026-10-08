//! The git identity a commit is written under: the four variables through
//! which git takes an author and a committer over `user.name` / `user.email`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The variables through which git takes an identity, author then committer.
pub const GIT_IDENTITY_VARS: [&str; 4] = [
    "GIT_AUTHOR_NAME",
    "GIT_AUTHOR_EMAIL",
    "GIT_COMMITTER_NAME",
    "GIT_COMMITTER_EMAIL",
];

/// The identity PDO signs a delivery with, by variable name.
///
/// Holds only [`GIT_IDENTITY_VARS`]: it is built from a Node's environment,
/// which may carry secrets, and frozen in the event log. Empty means "the
/// daemon's own identity": git then reads its environment and configuration
/// as it always did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CommitIdentity(BTreeMap<String, String>);

impl CommitIdentity {
    /// The identity variables among `env`, every other variable dropped.
    pub fn from_env<'a>(env: impl IntoIterator<Item = (&'a String, &'a String)>) -> Self {
        Self(
            env.into_iter()
                .filter(|(name, _)| GIT_IDENTITY_VARS.contains(&name.as_str()))
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect(),
        )
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The variables to set on a git command, in name order.
    pub fn vars(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn keeps_the_four_identity_variables_and_nothing_else() {
        let env = env(&[
            ("GIT_AUTHOR_NAME", "Alice"),
            ("GIT_AUTHOR_EMAIL", "alice@example.com"),
            ("GIT_COMMITTER_NAME", "Alice"),
            ("GIT_COMMITTER_EMAIL", "alice@example.com"),
            ("PDO_TOKEN", "secret"),
            ("GIT_SSH_COMMAND", "ssh -i key"),
        ]);
        let identity = CommitIdentity::from_env(&env);
        let vars: Vec<_> = identity.vars().map(|(name, _)| name).collect();
        assert_eq!(
            vars,
            vec![
                "GIT_AUTHOR_EMAIL",
                "GIT_AUTHOR_NAME",
                "GIT_COMMITTER_EMAIL",
                "GIT_COMMITTER_NAME"
            ]
        );
    }

    #[test]
    fn an_environment_without_identity_gives_the_empty_one() {
        let identity = CommitIdentity::from_env(&env(&[("PDO_TOKEN", "secret")]));
        assert!(identity.is_empty());
        assert_eq!(identity, CommitIdentity::default());
    }

    #[test]
    fn serializes_as_a_plain_map() {
        let identity = CommitIdentity::from_env(&env(&[("GIT_AUTHOR_NAME", "Alice")]));
        let json = serde_json::to_value(&identity).unwrap();
        assert_eq!(json, serde_json::json!({ "GIT_AUTHOR_NAME": "Alice" }));
        let back: CommitIdentity = serde_json::from_value(json).unwrap();
        assert_eq!(back, identity);
    }
}
