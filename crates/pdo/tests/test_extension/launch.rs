//! What the test extension contributes to a Node's launch and to the Triggers
//! (API 0.8): the core's `fixture` for these seams.
//!
//! - **`node_env`**: the variables of [`Launch::set_env`] (`PUT
//!   /ext/test/launch {"env": {"FIXTURE_HELLO": "monde"}}`);
//! - **`node_preamble`**: the text of [`Launch::set_preamble`] (`PUT
//!   /ext/test/launch {"preamble": "Ligne de fixture"}`);
//! - **`trigger_suspension`**: the reason [`Launch::suspend`] gave for a
//!   Trigger (`PUT /ext/test/suspensions {"trigger_id", "reason"}`, `DELETE
//!   /ext/test/suspensions/<id>` lifts it). Every Trigger it is asked about
//!   is recorded with its Projet (0.26, [`Launch::trigger_asks`], `GET
//!   /ext/test/trigger-asks`).
//!
//! - **`session_env`** / **`session_preamble`** (API 0.13), for a session
//!   that is no Node (the Pipeline Manager, the library assistant): the same
//!   variables plus `TEST_STARTED_BY`, the id of who starts the session (else
//!   its kind of Actor, `nobody` when unknown), and the same preamble; a
//!   Session hébergée also gets the `PDO_TOKEN` of who started it
//!   (`sessions.rs`).
//!
//! - **`git_env`**: for a network git operation of the daemon, the variables
//!   and label [`Launch::set_git_env`] gave its Actor (`PUT
//!   /ext/test/git-env {"actor": "alice", "env": {…}, "label": …}`, the id
//!   of an identified Actor, else its kind: `local_operator`, `trigger`; an
//!   empty `env` removes them). Every operation asked about is recorded (`GET
//!   /ext/test/git-env`, with the names of each Actor's variables, never
//!   their values).
//!
//! Every launch it is asked about is recorded ([`Launch::launches`], `GET
//! /ext/test/launch`), with whether it asked for the preamble.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use pdo::extension_api::{
    Actor, GitEnv, GitOperation, NodeEnv, NodeLaunch, SessionKind, SessionLaunch, TriggerRef,
};
use serde_json::{json, Value};

/// The launch side of the test extension's control.
#[derive(Clone, Default)]
pub struct Launch {
    env: Arc<Mutex<NodeEnv>>,
    preamble: Arc<Mutex<Option<String>>>,
    suspensions: Arc<Mutex<BTreeMap<String, String>>>,
    launches: Arc<Mutex<Vec<Value>>>,
    trigger_asks: Arc<Mutex<Vec<TriggerRef>>>,
    git_envs: Arc<Mutex<BTreeMap<String, GitEnv>>>,
    git_asks: Arc<Mutex<Vec<GitOperation>>>,
}

/// How the test extension keys an Actor's git credentials: the id of an
/// identified Actor, else its kind.
fn actor_key(actor: &Actor) -> String {
    match actor {
        Actor::Identified { id, .. } => id.clone(),
        other => other.kind().to_string(),
    }
}

impl Launch {
    /// Give every Node these variables from now on.
    pub fn set_env(&self, env: NodeEnv) {
        *self.env.lock().unwrap() = env;
    }

    /// Put this text at the top of every agent's preamble from now on;
    /// `None` puts none.
    pub fn set_preamble(&self, preamble: Option<String>) {
        *self.preamble.lock().unwrap() = preamble;
    }

    /// Suspend the Trigger `trigger_id`, saying `reason`.
    pub fn suspend(&self, trigger_id: &str, reason: &str) {
        self.suspensions
            .lock()
            .unwrap()
            .insert(trigger_id.to_string(), reason.to_string());
    }

    /// Lift the suspension of `trigger_id`; whether it was suspended.
    pub fn lift(&self, trigger_id: &str) -> bool {
        self.suspensions
            .lock()
            .unwrap()
            .remove(trigger_id)
            .is_some()
    }

    /// Give the network git operations of `actor` (an id, or a kind of
    /// Actor) these variables, named `label` in a failure; an empty `env`
    /// removes them.
    pub fn set_git_env(&self, actor: &str, env: NodeEnv, label: Option<String>) {
        let mut envs = self.git_envs.lock().unwrap();
        if env.is_empty() {
            envs.remove(actor);
            return;
        }
        let given = GitEnv::new(env);
        envs.insert(
            actor.to_string(),
            match label {
                Some(label) => given.labelled(label),
                None => given,
            },
        );
    }

    /// Every git operation `git_env` was asked about, in order.
    pub fn git_asks(&self) -> Vec<GitOperation> {
        self.git_asks.lock().unwrap().clone()
    }

    pub(super) fn git_env(&self, operation: &GitOperation) -> GitEnv {
        self.git_asks.lock().unwrap().push(operation.clone());
        self.git_envs
            .lock()
            .unwrap()
            .get(&actor_key(&operation.actor))
            .cloned()
            .unwrap_or_default()
    }

    /// `GET /ext/test/git-env`: the names of each Actor's variables, its
    /// label, and the operations asked about.
    pub(super) fn git_state(&self) -> Value {
        let envs: BTreeMap<String, Value> = self
            .git_envs
            .lock()
            .unwrap()
            .iter()
            .map(|(actor, given)| {
                (
                    actor.clone(),
                    json!({ "names": given.env.keys().collect::<Vec<_>>(), "label": given.label }),
                )
            })
            .collect();
        json!({ "envs": envs, "operations": self.git_asks() })
    }

    /// `PUT /ext/test/git-env`: `{"actor", "env", "label"?}`.
    pub(super) fn configure_git(&self, body: &Value) {
        let actor = body["actor"].as_str().unwrap_or("local_operator");
        let env: NodeEnv = serde_json::from_value(body["env"].clone()).unwrap_or_default();
        let label = body["label"].as_str().map(str::to_string);
        self.set_git_env(actor, env, label);
    }

    /// Every launch asked about, in order: `{"hook", "launch"}`.
    pub fn launches(&self) -> Vec<Value> {
        self.launches.lock().unwrap().clone()
    }

    /// Every Trigger `trigger_suspension` was asked about, in order.
    pub fn trigger_asks(&self) -> Vec<TriggerRef> {
        self.trigger_asks.lock().unwrap().clone()
    }

    fn record(&self, hook: &str, launch: &NodeLaunch) {
        self.launches
            .lock()
            .unwrap()
            .push(json!({ "hook": hook, "launch": launch }));
    }

    pub(super) fn node_env(&self, launch: &NodeLaunch) -> NodeEnv {
        self.record("node_env", launch);
        self.env.lock().unwrap().clone()
    }

    pub(super) fn node_preamble(&self, launch: &NodeLaunch) -> Option<String> {
        self.record("node_preamble", launch);
        self.preamble.lock().unwrap().clone()
    }

    pub(super) fn session_env(&self, launch: &SessionLaunch) -> NodeEnv {
        self.launches
            .lock()
            .unwrap()
            .push(json!({ "hook": "session_env", "launch": launch }));
        let started_by = match &launch.started_by {
            Some(Actor::Identified { id, .. }) => id.clone(),
            Some(actor) => actor.kind().to_string(),
            None => "nobody".to_string(),
        };
        let mut env = self.env.lock().unwrap().clone();
        env.insert("TEST_STARTED_BY".into(), started_by);
        if launch.session.kind == SessionKind::Hosted {
            let token = super::sessions::token_of(launch.started_by.as_ref());
            env.insert("PDO_TOKEN".into(), token);
        }
        env
    }

    pub(super) fn session_preamble(&self, launch: &SessionLaunch) -> Option<String> {
        self.launches
            .lock()
            .unwrap()
            .push(json!({ "hook": "session_preamble", "launch": launch }));
        self.preamble.lock().unwrap().clone()
    }

    pub(super) fn trigger_suspension(&self, trigger: &TriggerRef) -> Option<String> {
        self.trigger_asks.lock().unwrap().push(trigger.clone());
        self.suspensions.lock().unwrap().get(&trigger.id).cloned()
    }

    /// `GET /ext/test/launch`.
    pub(super) fn state(&self) -> Value {
        json!({
            "env": *self.env.lock().unwrap(),
            "preamble": *self.preamble.lock().unwrap(),
            "launches": self.launches(),
        })
    }

    /// `PUT /ext/test/launch`: `env` and `preamble`, each only when present.
    pub(super) fn configure(&self, body: &Value) {
        if let Some(env) = body.get("env") {
            self.set_env(serde_json::from_value(env.clone()).unwrap_or_default());
        }
        if let Some(preamble) = body.get("preamble") {
            self.set_preamble(preamble.as_str().map(str::to_string));
        }
    }

    /// `GET /ext/test/suspensions`: trigger id → reason.
    pub(super) fn suspensions(&self) -> Value {
        json!(*self.suspensions.lock().unwrap())
    }
}
