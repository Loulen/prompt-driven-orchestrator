//! The credentials of a network git operation the daemon runs for an Actor:
//! what the extensions provide for it, or the daemon's own.

use std::collections::btree_map::Entry;
use std::collections::BTreeMap;
use std::fmt;
use std::panic::{catch_unwind, AssertUnwindSafe};

use pdo_extension_api::GitOperation;

use super::launch::is_env_name;
use super::Extensions;
use crate::Actor;

/// The variables the core sets on every network git operation: no extension
/// overwrites them (no prompt, the operator's `GIT_SSH_COMMAND` in batch
/// mode, git's messages in English for the core to read).
pub const CORE_GIT_VARIABLES: &[&str] = &[
    "GIT_TERMINAL_PROMPT",
    "GIT_ASKPASS",
    "SSH_ASKPASS_REQUIRE",
    "GIT_SSH_COMMAND",
    "LC_ALL",
];

/// Who provided the credentials of an operation, for its failure message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvidedFor {
    /// The Actor, as a person reads it (its display name when an extension
    /// gives one).
    pub actor: String,
    /// What the extensions say they provided, if they say.
    pub label: Option<String>,
}

/// The credentials a network git operation runs with: the environment the
/// extensions provide for its Actor, or none (the daemon's own credentials).
///
/// Its `Debug` names the variables, never their values: they may be secrets.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct GitCredentials {
    /// The variables to add to git's environment; never one of
    /// [`CORE_GIT_VARIABLES`], every name a valid shell identifier.
    pub env: BTreeMap<String, String>,
    /// `None`: no extension provided anything, the daemon's credentials apply.
    pub provided: Option<ProvidedFor>,
}

impl GitCredentials {
    /// The daemon's own credentials: nothing added to git's environment.
    pub fn daemon() -> Self {
        Self::default()
    }

    /// `env`, provided for `actor`.
    pub fn provided(
        env: BTreeMap<String, String>,
        actor: impl Into<String>,
        label: Option<String>,
    ) -> Self {
        Self {
            env,
            provided: Some(ProvidedFor {
                actor: actor.into(),
                label,
            }),
        }
    }

    /// A digest of the environment, so two operations can tell whether they
    /// run with the same credentials without keeping any value: the daemon's
    /// credentials all share one.
    pub fn fingerprint(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.env.hash(&mut hasher);
        hasher.finish()
    }

    /// The sentence saying these credentials were refused by `host`.
    pub fn refused_by(&self, host: &str) -> String {
        match &self.provided {
            Some(ProvidedFor {
                actor,
                label: Some(label),
            }) => format!("The credentials provided for {actor} ({label}) were refused for {host}"),
            Some(ProvidedFor { actor, label: None }) => {
                format!("The credentials provided for {actor} were refused for {host}")
            }
            None => format!("The daemon's git credentials were refused for {host}"),
        }
    }
}

impl fmt::Debug for GitCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GitCredentials")
            .field("env", &self.env.keys().collect::<Vec<_>>())
            .field("provided", &self.provided)
            .finish()
    }
}

/// How `actor` reads in a sentence when no extension names it.
pub fn actor_in_sentence(actor: &Actor) -> String {
    match actor {
        Actor::LocalOperator => "the local operator".to_string(),
        Actor::Agent { run_id, node_id } => format!("the agent {node_id} of Run {run_id}"),
        Actor::Trigger { trigger_id } => format!("the Trigger {trigger_id}"),
        Actor::Identified { id, .. } => id.clone(),
        other => other.kind().to_string(),
    }
}

/// Every extension's variables for `operation` (the first registered wins a
/// name, a core variable is dropped), and the first label of those that gave
/// any. A panicking extension adds nothing.
pub(super) fn git_credentials(extensions: &Extensions, operation: &GitOperation) -> GitCredentials {
    let mut env = BTreeMap::new();
    let mut label = None;
    for r in &extensions.registry.registered {
        let id = &r.manifest.id;
        let given = match catch_unwind(AssertUnwindSafe(|| r.extension.git_env(operation))) {
            Ok(given) => given,
            Err(_) => {
                tracing::warn!("extension `{id}` panicked in git_env; it adds nothing");
                continue;
            }
        };
        let mut added = false;
        for (name, value) in given.env {
            if !is_env_name(&name) {
                tracing::warn!("extension `{id}`: variable `{name}` is no valid name");
                continue;
            }
            if CORE_GIT_VARIABLES.contains(&name.as_str()) {
                tracing::warn!("extension `{id}`: variable `{name}` is the core's");
                continue;
            }
            match env.entry(name) {
                Entry::Vacant(slot) => {
                    slot.insert(value);
                    added = true;
                }
                Entry::Occupied(taken) => {
                    tracing::warn!("extension `{id}`: variable `{}` already set", taken.key())
                }
            }
        }
        if added && label.is_none() {
            label = given
                .label
                .map(|text| text.trim().to_string())
                .filter(|text| !text.is_empty());
        }
    }
    if env.is_empty() {
        return GitCredentials::daemon();
    }
    let actor = extensions
        .display(&operation.actor)
        .map(|display| display.name)
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| actor_in_sentence(&operation.actor));
    GitCredentials::provided(env, actor, label)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extension_host::{ExtensionHost, NoExtensions};
    use pdo_extension_api::{ActorDisplay, Extension, GitEnv, GitOperationKind, Manifest, NodeEnv};

    /// Gives `vars` (labelled `label`) to every operation of `alice`; panics
    /// when `panics`.
    struct Gives {
        id: &'static str,
        vars: &'static [(&'static str, &'static str)],
        label: Option<&'static str>,
        panics: bool,
    }

    impl Gives {
        fn new(id: &'static str, vars: &'static [(&'static str, &'static str)]) -> Self {
            Self {
                id,
                vars,
                label: None,
                panics: false,
            }
        }
    }

    impl Extension for Gives {
        fn manifest(&self) -> Manifest {
            Manifest::new(self.id, self.id, "0.0.0")
        }

        fn display_name(&self, actor: &Actor) -> Option<ActorDisplay> {
            matches!(actor, Actor::Identified { id, .. } if id == "alice")
                .then(|| ActorDisplay::new("Alice"))
        }

        fn git_env(&self, operation: &GitOperation) -> GitEnv {
            if self.panics {
                panic!("boom");
            }
            if !matches!(&operation.actor, Actor::Identified { id, .. } if id == "alice") {
                return GitEnv::default();
            }
            let env: NodeEnv = self
                .vars
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            match self.label {
                Some(label) => GitEnv::new(env).labelled(label),
                None => GitEnv::new(env),
            }
        }
    }

    fn alice() -> Actor {
        Actor::Identified {
            namespace: "a".into(),
            id: "alice".into(),
        }
    }

    fn clone_for(actor: Actor) -> GitOperation {
        GitOperation::new(
            actor,
            "https://git.example.com/acme/app.git",
            GitOperationKind::Clone,
        )
    }

    fn host(extensions: Vec<Box<dyn Extension>>) -> Extensions {
        Extensions::register(extensions).unwrap()
    }

    #[test]
    fn without_extension_the_daemon_credentials_apply() {
        assert_eq!(
            NoExtensions.git_credentials(&clone_for(alice())),
            GitCredentials::daemon()
        );
        let none = host(vec![Box::new(Gives::new("a", &[("X", "1")]))]);
        assert_eq!(
            none.git_credentials(&clone_for(Actor::LocalOperator)),
            GitCredentials::daemon(),
            "an extension that gives nothing for this Actor"
        );
    }

    #[test]
    fn the_first_extension_wins_a_name_and_the_core_keeps_its_own() {
        let mut labelled = Gives::new(
            "a",
            &[
                ("GIT_CONFIG_COUNT", "1"),
                ("GIT_TERMINAL_PROMPT", "1"),
                ("GIT_SSH_COMMAND", "ssh -i key"),
                ("bad name", "x"),
            ],
        );
        labelled.label = Some("a token");
        let mut second = Gives::new("b", &[("GIT_CONFIG_COUNT", "2"), ("TOKEN_B", "t")]);
        second.label = Some("another");
        let mut panics = Gives::new("c", &[]);
        panics.panics = true;
        let got = host(vec![Box::new(labelled), Box::new(panics), Box::new(second)])
            .git_credentials(&clone_for(alice()));
        assert_eq!(
            got.env,
            BTreeMap::from([
                ("GIT_CONFIG_COUNT".to_string(), "1".to_string()),
                ("TOKEN_B".to_string(), "t".to_string()),
            ])
        );
        assert_eq!(
            got.provided,
            Some(ProvidedFor {
                actor: "Alice".into(),
                label: Some("a token".into())
            })
        );
    }

    #[test]
    fn only_core_variables_given_is_the_daemon_credentials() {
        let got = host(vec![Box::new(Gives::new("a", &[("LC_ALL", "fr_FR")]))])
            .git_credentials(&clone_for(alice()));
        assert_eq!(got, GitCredentials::daemon());
    }

    #[test]
    fn the_refusal_names_whose_credentials() {
        assert_eq!(
            GitCredentials::daemon().refused_by("git.example.com"),
            "The daemon's git credentials were refused for git.example.com"
        );
        let provided = GitCredentials::provided(BTreeMap::new(), "Alice", None);
        assert_eq!(
            provided.refused_by("h"),
            "The credentials provided for Alice were refused for h"
        );
        let labelled = GitCredentials::provided(BTreeMap::new(), "Alice", Some("a token".into()));
        assert_eq!(
            labelled.refused_by("h"),
            "The credentials provided for Alice (a token) were refused for h"
        );
    }

    #[test]
    fn a_fingerprint_tells_two_environments_apart() {
        let one = |v: &str| {
            GitCredentials::provided(
                BTreeMap::from([("T".to_string(), v.to_string())]),
                "A",
                None,
            )
        };
        assert_eq!(one("a").fingerprint(), one("a").fingerprint());
        assert_ne!(one("a").fingerprint(), one("b").fingerprint());
        assert_eq!(
            GitCredentials::daemon().fingerprint(),
            GitCredentials::default().fingerprint()
        );
    }

    #[test]
    fn the_debug_never_shows_a_value() {
        let got = GitCredentials::provided(
            BTreeMap::from([("SECRET".to_string(), "s3cr3t".to_string())]),
            "Alice",
            None,
        );
        let shown = format!("{got:?}");
        assert!(
            shown.contains("SECRET") && !shown.contains("s3cr3t"),
            "{shown}"
        );
    }

    #[test]
    fn an_actor_reads_in_a_sentence() {
        assert_eq!(
            actor_in_sentence(&Actor::LocalOperator),
            "the local operator"
        );
        assert_eq!(actor_in_sentence(&alice()), "alice");
        assert_eq!(
            actor_in_sentence(&Actor::Trigger {
                trigger_id: "t1".into()
            }),
            "the Trigger t1"
        );
    }
}
