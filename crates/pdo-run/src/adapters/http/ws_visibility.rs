//! What one connection of `/ws` may see (CORE-5): a message about a Run
//! reaches it only if its Actor may read that Run. Each decision is kept
//! [`READ_DECISION_TTL`], so a busy Run asks once per few seconds, not once
//! per event; a change of rights shows within that delay. A `trigger_fired`
//! still reaches the connection, without the `run_id` of a Run it may not
//! read. A message guarded by a read ([`READ_GUARD`]: what a Trigger
//! announces, in its Projets; an extension's event its extension guarded)
//! and an `operation_applied` on a Trigger, a session or an extension's
//! Ressource reach a connection only if its Actor may run that read (the read
//! of the entity, in the Operation's Projets); the guard never leaves the
//! daemon.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use pdo_extension_api::{PRESENCE_ARRIVED, PRESENCE_LEFT};
use pdo_kernel::authorization::authorize_read;
use pdo_kernel::event_bus::READ_GUARD;
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use serde_json::Value;

use crate::Runs;

/// How long a connection keeps the answer to « may I read this Run? ».
pub const READ_DECISION_TTL: Duration = Duration::from_secs(5);

/// How long a decision is kept at all: a `run_forgotten` (whose Run, gone,
/// has no Projet any more) is judged by the last decision about its Run.
const FORGET_DECISIONS_AFTER: Duration = Duration::from_secs(600);

/// What one connection may see of the Runs, with the decisions it got.
pub(super) struct Visibility<'a> {
    runs: &'a Runs,
    actor: Actor,
    decisions: HashMap<String, (bool, Instant)>,
    /// The decisions on the guarded reads, by the read's JSON.
    guards: HashMap<String, (bool, Instant)>,
}

impl<'a> Visibility<'a> {
    pub(super) fn new(runs: &'a Runs, actor: Actor) -> Self {
        Self {
            runs,
            actor,
            decisions: HashMap::new(),
            guards: HashMap::new(),
        }
    }

    /// What of `message` reaches the connection: all of it, the message
    /// without the Run it may not read, or nothing. Never its guard.
    pub(super) async fn filter(&mut self, mut message: Value) -> Option<Value> {
        let guard = message
            .as_object_mut()
            .and_then(|fields| fields.remove(READ_GUARD));
        if self.runs.extensions().allows_everything() {
            return Some(message);
        }
        if let Some(read) = guard.or_else(|| entity_read_of(&message)) {
            if !self.may_run(read) {
                return None;
            }
        }
        if let Some((run_id, gone)) = run_of_message(&message) {
            let run_id = run_id.to_string();
            return self.may_read(&run_id, gone).await.then_some(message);
        }
        if let Some(run_id) = run_mentioned_by(&message).map(str::to_string) {
            if !self.may_read(&run_id, false).await {
                if let Some(fields) = message.as_object_mut() {
                    fields.remove("run_id");
                }
            }
        }
        Some(message)
    }

    /// Whether the connection may read `run_id`, `gone` or not.
    async fn may_read(&mut self, run_id: &str, gone: bool) -> bool {
        if let Some((readable, at)) = self.decisions.get(run_id) {
            if gone || at.elapsed() < READ_DECISION_TTL {
                return *readable;
            }
        }
        let readable = self.runs.authorize_read(&self.actor, run_id).await.is_ok();
        self.decisions
            .insert(run_id.to_string(), (readable, Instant::now()));
        readable
    }

    /// Whether the connection may run the guarded `read` (an Operation's
    /// JSON). A guard that is no read Operation refuses.
    fn may_run(&mut self, read: Value) -> bool {
        let key = read.to_string();
        if let Some((allowed, at)) = self.guards.get(&key) {
            if at.elapsed() < READ_DECISION_TTL {
                return *allowed;
            }
        }
        let allowed = match serde_json::from_value::<Operation>(read) {
            Ok(read) if read.is_read() => {
                authorize_read(self.runs.extensions(), &self.actor, &read).is_ok()
            }
            _ => false,
        };
        self.guards.insert(key, (allowed, Instant::now()));
        allowed
    }

    /// Drop the decisions nobody needs any more.
    pub(super) fn forget_old(&mut self) {
        self.decisions
            .retain(|_, (_, at)| at.elapsed() < FORGET_DECISIONS_AFTER);
        self.guards
            .retain(|_, (_, at)| at.elapsed() < READ_DECISION_TTL);
    }
}

/// The Run a message of the bus is about, and whether that Run is gone: a
/// Run's event (`{"type": "event", "event": {"run_id"…}}`), its
/// `run_forgotten`, an arrival or a departure on it, an event of one of its
/// sessions (`session_started`, `session_taken_over`, `session_ended`), an
/// `operation_applied` on a Run or one of its nodes (the creation of a Run has
/// no target: the Run is the entity it produced).
/// `None` for everything else.
fn run_of_message(message: &Value) -> Option<(&str, bool)> {
    match message["type"].as_str()? {
        "event" => Some((message["event"]["run_id"].as_str()?, false)),
        "run_forgotten" => Some((message["run_id"].as_str()?, true)),
        PRESENCE_ARRIVED | PRESENCE_LEFT => Some((message["run_id"].as_str()?, false)),
        // CORE-61: a session of a Run (the library assistant's has none).
        "session_started" | "session_taken_over" | "session_ended" => {
            Some((message["session"]["run_id"].as_str()?, false))
        }
        "operation_applied" => {
            let operation = &message["operation"];
            match operation["resource"].as_str()? {
                "run" => match operation["target"].as_str() {
                    Some(target) => Some((target, false)),
                    None => Some((message["entity"]["id"].as_str()?, false)),
                },
                "node" => Some((operation["target"].as_str()?.split('/').next()?, false)),
                _ => None,
            }
        }
        _ => None,
    }
}

/// The read guarding an `operation_applied` on a Trigger, a session or an
/// extension's Ressource: the read of the entity it is about (its target, or
/// the entity it created), on the same Ressource, in the Projets of its
/// Operation. A session's Operation without a target (the opening of a
/// Session hébergée) is guarded by the read of the sessions of its Projets,
/// as its `hosted_session_changed` is reached by those who may read it.
/// `None` for the other messages, and for a creation whose entity nobody
/// knows. One on the shared configuration (a pipeline, the library, an
/// agentic profile, a skill, the instance settings) is guarded by the read of
/// that configuration, untargeted, in no Projet, as its `GET` is.
fn entity_read_of(message: &Value) -> Option<Value> {
    if message["type"] != "operation_applied" {
        return None;
    }
    let operation: Operation = serde_json::from_value(message["operation"].clone()).ok()?;
    if matches!(
        operation.resource,
        ResourceKind::Pipeline
            | ResourceKind::LibraryEntry
            | ResourceKind::AgentProfile
            | ResourceKind::Skill
            | ResourceKind::SkillFolder
            | ResourceKind::InstanceSettings
    ) {
        return serde_json::to_value(Operation::new(operation.resource, Verb::Read)).ok();
    }
    if !matches!(
        operation.resource,
        ResourceKind::Trigger | ResourceKind::Session | ResourceKind::Extension { .. }
    ) {
        return None;
    }
    let target = operation
        .target
        .or_else(|| message["entity"]["id"].as_str().map(str::to_string));
    let read = Operation::new(operation.resource.clone(), Verb::Read);
    let read = match target {
        Some(target) => read.on(target),
        None if operation.resource == ResourceKind::Session => read,
        None => return None,
    };
    serde_json::to_value(read.in_projects(operation.projects)).ok()
}

/// The Run a message about something else names in passing: the Run a
/// Trigger's fire created.
fn run_mentioned_by(message: &Value) -> Option<&str> {
    match message["type"].as_str()? {
        "trigger_fired" => message["run_id"].as_str(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_message_names_the_run_it_is_about() {
        assert_eq!(
            run_of_message(
                &json!({ "type": "event", "event": { "run_id": "r1", "kind": "node_started" } })
            ),
            Some(("r1", false))
        );
        assert_eq!(
            run_of_message(&json!({ "type": "run_forgotten", "run_id": "r2" })),
            Some(("r2", true))
        );
        assert_eq!(
            run_of_message(&json!({
                "type": "session_taken_over",
                "actor": { "kind": "local_operator" },
                "session": { "id": "pdo-mgr-r3", "kind": "manager", "run_id": "r3" },
            })),
            Some(("r3", false))
        );
        assert_eq!(
            run_of_message(&json!({
                "type": "session_ended",
                "actor": null,
                "session": { "id": "pdo-libassist-shared", "kind": "library_assistant", "run_id": null },
            })),
            None
        );
        assert_eq!(
            run_of_message(&json!({
                "type": "operation_applied",
                "operation": { "resource": "node", "verb": { "action": "retry" }, "target": "r3/n1" },
            })),
            Some(("r3", false))
        );
        assert_eq!(
            run_of_message(&json!({
                "type": "operation_applied",
                "operation": { "resource": "run", "verb": "delete", "target": "r4" },
            })),
            Some(("r4", false))
        );
        assert_eq!(
            run_of_message(&json!({
                "type": "operation_applied",
                "operation": { "resource": "run", "verb": "create", "target": null },
                "entity": { "kind": "run", "id": "r5" },
            })),
            Some(("r5", false)),
            "a creation is about the Run it produced"
        );
    }

    #[test]
    fn a_presence_is_about_its_run() {
        for kind in [PRESENCE_ARRIVED, PRESENCE_LEFT] {
            assert_eq!(
                run_of_message(
                    &json!({ "type": kind, "run_id": "r1", "actor": {}, "presences": [] })
                ),
                Some(("r1", false))
            );
        }
    }

    #[test]
    fn a_triggers_fire_names_the_run_it_created() {
        assert_eq!(
            run_mentioned_by(
                &json!({ "type": "trigger_fired", "trigger_id": "t1", "run_id": "r1" })
            ),
            Some("r1")
        );
        assert_eq!(
            run_mentioned_by(
                &json!({ "type": "trigger_fired", "trigger_id": "t1", "outcome": "skipped" })
            ),
            None
        );
        assert_eq!(
            run_mentioned_by(&json!({ "type": "event", "event": { "run_id": "r1" } })),
            None
        );
    }

    #[test]
    fn an_operation_on_a_trigger_is_guarded_by_its_read_in_its_projets() {
        let applied = |target: Value| {
            json!({
                "type": "operation_applied",
                "operation": {
                    "resource": "trigger", "verb": "update", "target": target, "projects": ["p2"],
                },
                "entity": { "kind": "trigger", "id": "t9" },
            })
        };
        let read = |id: &str| json!({ "resource": "trigger", "verb": "read", "target": id, "projects": ["p2"] });
        assert_eq!(entity_read_of(&applied(json!("t1"))), Some(read("t1")));
        assert_eq!(
            entity_read_of(&applied(Value::Null)),
            Some(read("t9")),
            "a creation is about the Trigger it produced"
        );
        assert_eq!(
            entity_read_of(&json!({
                "type": "operation_applied",
                "operation": { "resource": "run", "verb": "delete", "target": "r1" },
            })),
            None
        );
        assert_eq!(
            entity_read_of(&json!({ "type": "trigger_updated", "trigger_id": "t1" })),
            None
        );
    }

    #[test]
    fn an_operation_on_the_shared_configuration_is_guarded_by_its_read() {
        for resource in [
            "pipeline",
            "library_entry",
            "agent_profile",
            "skill",
            "skill_folder",
            "instance_settings",
        ] {
            let applied = json!({
                "type": "operation_applied",
                "operation": { "resource": resource, "verb": "update", "target": "x1" },
            });
            assert_eq!(
                entity_read_of(&applied),
                Some(
                    json!({ "resource": resource, "verb": "read", "target": null, "projects": [] })
                ),
                "{resource}"
            );
        }
    }

    #[test]
    fn an_operation_on_a_session_is_guarded_by_the_read_of_the_session_in_its_projets() {
        let applied = |verb: &str, target: Value| {
            json!({
                "type": "operation_applied",
                "operation": {
                    "resource": "session", "verb": { "action": verb }, "target": target,
                    "projects": ["p2"],
                },
            })
        };
        assert_eq!(
            entity_read_of(&applied("terminate", json!("pdo-hosted-1"))),
            Some(json!({
                "resource": "session", "verb": "read", "target": "pdo-hosted-1",
                "projects": ["p2"],
            }))
        );
        let opened = entity_read_of(&applied("open", Value::Null)).unwrap();
        assert_eq!(opened["resource"], "session");
        assert_eq!(opened["verb"], "read");
        assert!(opened["target"].is_null(), "{opened}");
        assert_eq!(opened["projects"], json!(["p2"]));
    }

    #[test]
    fn an_operation_on_an_extensions_ressource_is_guarded_by_the_read_of_its_entity() {
        let applied = |target: Value| {
            json!({
                "type": "operation_applied",
                "operation": {
                    "resource": "test/note", "verb": { "action": "pin" }, "target": target,
                    "projects": ["p2"],
                },
                "entity": { "kind": "test/note", "id": "n9" },
            })
        };
        let read = |id: &str| json!({ "resource": "test/note", "verb": "read", "target": id, "projects": ["p2"] });
        assert_eq!(entity_read_of(&applied(json!("n1"))), Some(read("n1")));
        assert_eq!(
            entity_read_of(&applied(Value::Null)),
            Some(read("n9")),
            "a creation is about the entity it produced"
        );
        assert_eq!(
            entity_read_of(&json!({
                "type": "operation_applied",
                "operation": { "resource": "test/note", "verb": "create", "target": null },
                "entity": null,
            })),
            None,
            "nothing to read: the message passes as before"
        );
        assert_eq!(
            entity_read_of(&json!({
                "type": "operation_applied",
                "operation": { "resource": "project", "verb": "update", "target": "p1" },
            })),
            None
        );
    }

    #[test]
    fn the_other_messages_are_about_no_run() {
        for message in [
            json!({ "type": "pipeline_changed" }),
            json!({ "type": "trigger_fired", "trigger_id": "t1", "run_id": "r1" }),
            json!({ "type": "project_changed" }),
            json!({
                "type": "operation_applied",
                "operation": { "resource": "trigger", "verb": "update", "target": "t1" },
            }),
            json!({ "type": "extension_event", "extension": "test", "name": "x", "payload": {} }),
        ] {
            assert_eq!(run_of_message(&message), None, "{message}");
        }
    }
}
