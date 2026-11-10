//! What the extensions contribute to the launch of a session (its environment,
//! its preamble), and the suspension of a Trigger.

use std::collections::btree_map::Entry;
use std::collections::BTreeMap;
use std::panic::{catch_unwind, AssertUnwindSafe};

use pdo_extension_api::{NodeKind, SessionKind, SessionLaunch, TriggerRef};
use serde::{Deserialize, Serialize};

use super::Extensions;

/// What the extensions add to the launch of a session.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NodeContribution {
    /// The variables to add to the session's environment, by name. Every
    /// name is a valid shell identifier; the core still drops those it sets
    /// itself.
    pub env: BTreeMap<String, String>,
    /// The text to place at the top of an agent's prompt; `None` for a
    /// script, or when no extension gives one.
    pub preamble: Option<String>,
}

impl NodeContribution {
    /// `prompt`, under the extensions' text at its very top (followed by a
    /// blank line), if any.
    pub fn above(&self, prompt: String) -> String {
        match &self.preamble {
            Some(text) => format!("{text}\n\n{prompt}"),
            None => prompt,
        }
    }
}

/// A Trigger an extension suspends, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TriggerSuspension {
    /// The id of the extension that suspends it.
    pub extension: String,
    pub reason: String,
}

/// Whether `name` can be exported as is by a shell: `[A-Za-z_][A-Za-z0-9_]*`.
/// A name is spliced unquoted into `export <name>=…`, so anything else is
/// refused rather than escaped.
pub fn is_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Whether the session `launch` starts runs an agent, which reads a prompt:
/// every session but a script Node and a shell.
fn runs_an_agent(launch: &SessionLaunch) -> bool {
    match launch.session.kind {
        SessionKind::Node => launch
            .node
            .as_ref()
            .is_none_or(|node| node.kind == NodeKind::Agent),
        SessionKind::Shell => false,
        _ => true,
    }
}

/// Every extension's variables (the first registered wins a name) and
/// preambles (in registration order). A panicking extension adds nothing.
pub(super) fn session_launch(extensions: &Extensions, launch: &SessionLaunch) -> NodeContribution {
    let mut env = BTreeMap::new();
    let mut preambles = Vec::new();
    let agent = runs_an_agent(launch);
    for r in &extensions.registry.registered {
        let id = &r.manifest.id;
        match catch_unwind(AssertUnwindSafe(|| r.extension.session_env(launch))) {
            Ok(vars) => {
                for (name, value) in vars {
                    if !is_env_name(&name) {
                        tracing::warn!("extension `{id}`: variable `{name}` is no valid name");
                        continue;
                    }
                    match env.entry(name) {
                        Entry::Vacant(slot) => {
                            slot.insert(value);
                        }
                        Entry::Occupied(taken) => tracing::warn!(
                            "extension `{id}`: variable `{}` already set",
                            taken.key()
                        ),
                    }
                }
            }
            Err(_) => tracing::warn!("extension `{id}` panicked in session_env; it adds nothing"),
        }
        if !agent {
            continue;
        }
        match catch_unwind(AssertUnwindSafe(|| r.extension.session_preamble(launch))) {
            Ok(Some(text)) if !text.trim().is_empty() => preambles.push(text.trim().to_string()),
            Ok(_) => {}
            Err(_) => {
                tracing::warn!("extension `{id}` panicked in session_preamble; it adds nothing")
            }
        }
    }
    NodeContribution {
        env,
        preamble: (!preambles.is_empty()).then(|| preambles.join("\n\n")),
    }
}

/// The first extension that suspends `trigger`. A panic suspends nothing.
pub(super) fn trigger_suspension(
    extensions: &Extensions,
    trigger: &TriggerRef,
) -> Option<TriggerSuspension> {
    extensions.registry.registered.iter().find_map(|r| {
        match catch_unwind(AssertUnwindSafe(|| r.extension.trigger_suspension(trigger))) {
            Ok(Some(reason)) => Some(TriggerSuspension {
                extension: r.manifest.id.clone(),
                reason,
            }),
            Ok(None) => None,
            Err(_) => {
                tracing::warn!(
                    "extension `{}` panicked in trigger_suspension; ignored",
                    r.manifest.id
                );
                None
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extension_host::{ExtensionHost, NoExtensions};
    use pdo_extension_api::{Actor, Extension, Manifest, NodeEnv, NodeLaunch, SessionRef};

    /// Gives `vars` and `preamble` to every launch, suspends every Trigger
    /// when `suspends`; panics everywhere when `panics`.
    struct Contributes {
        id: &'static str,
        vars: &'static [(&'static str, &'static str)],
        preamble: Option<&'static str>,
        suspends: bool,
        panics: bool,
    }

    impl Contributes {
        fn new(id: &'static str) -> Self {
            Self {
                id,
                vars: &[],
                preamble: None,
                suspends: false,
                panics: false,
            }
        }
    }

    impl Extension for Contributes {
        fn manifest(&self) -> Manifest {
            Manifest::new(self.id, self.id, "0.0.0")
        }
        fn node_env(&self, _launch: &NodeLaunch) -> NodeEnv {
            assert!(!self.panics, "boom");
            self.vars
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        }
        fn node_preamble(&self, _launch: &NodeLaunch) -> Option<String> {
            assert!(!self.panics, "boom");
            self.preamble.map(str::to_string)
        }
        fn trigger_suspension(&self, trigger: &TriggerRef) -> Option<String> {
            assert!(!self.panics, "boom");
            self.suspends
                .then(|| format!("{} suspends {}", self.id, trigger.name))
        }
    }

    fn launch(kind: NodeKind) -> SessionLaunch {
        SessionLaunch::new(
            SessionRef::new("pdo-r1-n1-iter-1", SessionKind::Node)
                .in_run("r1")
                .of_node("n1", 1),
        )
        .of_node(NodeLaunch::new("r1", "n1", 1, kind, "/repo"))
    }

    fn manager() -> SessionLaunch {
        SessionLaunch::new(SessionRef::new("pdo-mgr-r1", SessionKind::Manager).in_run("r1"))
            .started_by(Actor::LocalOperator)
    }

    fn host(extensions: Vec<Contributes>) -> Extensions {
        Extensions::register(
            extensions
                .into_iter()
                .map(|e| Box::new(e) as Box<dyn Extension>)
                .collect(),
        )
        .unwrap()
    }

    #[test]
    fn without_extension_nothing_is_contributed() {
        let contribution = NoExtensions.session_launch(&launch(NodeKind::Agent));
        assert_eq!(contribution, NodeContribution::default());
        assert_eq!(
            NoExtensions.trigger_suspension(&TriggerRef::new("t1", "T", None)),
            None
        );
    }

    #[test]
    fn the_first_extension_wins_a_variable_and_preambles_follow_each_other() {
        let mut a = Contributes::new("a");
        a.vars = &[("FIXTURE_HELLO", "monde"), ("SHARED", "a")];
        a.preamble = Some("Ligne de a\n");
        let mut b = Contributes::new("b");
        b.vars = &[("SHARED", "b"), ("ONLY_B", "1")];
        b.preamble = Some("Ligne de b");
        let host = host(vec![a, b]);

        let agent = host.session_launch(&launch(NodeKind::Agent));
        assert_eq!(agent.env["FIXTURE_HELLO"], "monde");
        assert_eq!(agent.env["SHARED"], "a", "the first registered wins");
        assert_eq!(agent.env["ONLY_B"], "1");
        assert_eq!(agent.preamble.as_deref(), Some("Ligne de a\n\nLigne de b"));

        let script = host.session_launch(&launch(NodeKind::Script));
        assert_eq!(script.env, agent.env, "a script gets the variables");
        assert_eq!(script.preamble, None, "a script reads no preamble");
    }

    #[test]
    fn a_name_no_shell_can_export_is_dropped() {
        let mut a = Contributes::new("a");
        a.vars = &[
            ("A B; rm -rf /", "x"),
            ("9LIVES", "x"),
            ("", "x"),
            ("_OK", "y"),
        ];
        let env = host(vec![a]).session_launch(&launch(NodeKind::Script)).env;
        assert_eq!(env.keys().collect::<Vec<_>>(), vec!["_OK"]);
    }

    #[test]
    fn a_panicking_extension_adds_nothing_and_suspends_nothing() {
        let mut boom = Contributes::new("boom");
        boom.panics = true;
        let mut a = Contributes::new("a");
        a.vars = &[("X", "1")];
        a.suspends = true;
        let host = host(vec![boom, a]);
        let contribution = host.session_launch(&launch(NodeKind::Agent));
        assert_eq!(contribution.env.len(), 1);
        assert_eq!(
            host.trigger_suspension(&TriggerRef::new("t1", "Nightly", None)),
            Some(TriggerSuspension {
                extension: "a".into(),
                reason: "a suspends Nightly".into(),
            })
        );
    }

    #[test]
    fn the_preamble_goes_above_the_prompt() {
        let none = NodeContribution::default();
        assert_eq!(none.above("P".into()), "P");
        let some = NodeContribution {
            preamble: Some("E".into()),
            ..NodeContribution::default()
        };
        assert_eq!(some.above("P".into()), "E\n\nP");
    }

    #[test]
    fn a_blank_preamble_is_no_preamble() {
        let mut a = Contributes::new("a");
        a.preamble = Some("  \n");
        let contribution = host(vec![a]).session_launch(&launch(NodeKind::Agent));
        assert_eq!(contribution.preamble, None);
    }

    /// Answers every session, naming its kind and who starts it.
    struct EverySession;

    impl Extension for EverySession {
        fn manifest(&self) -> Manifest {
            Manifest::new("every", "every", "0.0.0")
        }
        fn session_env(&self, launch: &SessionLaunch) -> NodeEnv {
            let kind = serde_json::to_value(launch.session.kind).unwrap();
            let by = match &launch.started_by {
                Some(Actor::Identified { id, .. }) => id.clone(),
                Some(actor) => actor.kind().to_string(),
                None => "nobody".into(),
            };
            NodeEnv::from([
                ("SESSION_KIND".into(), kind.as_str().unwrap().to_string()),
                ("STARTED_BY".into(), by),
            ])
        }
        fn session_preamble(&self, launch: &SessionLaunch) -> Option<String> {
            Some(format!("Session {}", launch.session.id))
        }
    }

    #[test]
    fn an_extension_that_knows_only_nodes_adds_nothing_to_another_session() {
        let mut a = Contributes::new("a");
        a.vars = &[("X", "1")];
        a.preamble = Some("Ligne de a");
        let contribution = host(vec![a]).session_launch(&manager());
        assert_eq!(contribution, NodeContribution::default());
    }

    #[test]
    fn every_agent_session_is_asked_with_who_starts_it() {
        let host = Extensions::register(vec![Box::new(EverySession)]).unwrap();
        let manager = host.session_launch(&manager());
        assert_eq!(manager.env["SESSION_KIND"], "manager");
        assert_eq!(manager.env["STARTED_BY"], "local_operator");
        assert_eq!(manager.preamble.as_deref(), Some("Session pdo-mgr-r1"));

        let assistant = host.session_launch(
            &SessionLaunch::new(SessionRef::new(
                "pdo-libassist-x",
                SessionKind::LibraryAssistant,
            ))
            .started_by(Actor::Identified {
                namespace: "test".into(),
                id: "bob".into(),
            }),
        );
        assert_eq!(assistant.env["SESSION_KIND"], "library_assistant");
        assert_eq!(assistant.env["STARTED_BY"], "bob");
        assert!(assistant.preamble.is_some());

        let script = host.session_launch(&launch(NodeKind::Script));
        assert_eq!(script.env["SESSION_KIND"], "node");
        assert_eq!(script.env["STARTED_BY"], "nobody");
        assert_eq!(script.preamble, None, "a script reads no preamble");

        let shell = host.session_launch(&SessionLaunch::new(
            SessionRef::new("pdo-shell-r1", SessionKind::Shell).in_run("r1"),
        ));
        assert_eq!(shell.preamble, None, "a shell runs no agent");
    }
}
