//! `/ws`: the live stream every client subscribes to. It relays the kernel's
//! bus as is (D20), after a `ready` greeting, with a heartbeat every
//! [`HEARTBEAT_INTERVAL`]. A connection that lagged behind the bus (messages
//! lost) receives [`RESYNC`] (ADR-0083 §6).
//!
//! Filtered per connection (CORE-5): a message about a Run reaches a
//! connection only if its Actor (the one `identify` set on the upgrade
//! request) may read that Run. Each decision is kept
//! [`READ_DECISION_TTL`], so a busy Run asks once per few seconds, not once
//! per event; a change of rights shows within that delay. A `trigger_fired`
//! still reaches the connection, without the `run_id` of a Run it may not
//! read. A message guarded by a read ([`READ_GUARD`]: what a Trigger
//! announces, in its Projets; an extension's event its extension guarded)
//! and an `operation_applied` on a Trigger or on an extension's Ressource
//! reach a connection only if its Actor may run that read (the read of the
//! entity, in the Operation's Projets); the guard never leaves the daemon.
//! The other messages (pipelines, Projets, the extensions' unguarded
//! events…) pass as before.
//!
//! The client's Close frame is answered before the connection ends (ADR-0083
//! §3): a closing without it counts as a failure for the browser. Every
//! opening, closing and refused upgrade has its line in the journal
//! ([`ws_log`]).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Extension;
use futures_util::SinkExt;
use pdo_kernel::authorization::authorize_read;
use pdo_kernel::event_bus::READ_GUARD;
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use pdo_session::adapters::http::origin::check_origin;
use pdo_session::adapters::http::ws_log::{self, Closing, NO_STATUS};
use serde_json::Value;
use tokio::sync::broadcast;
use tokio::time;
use tracing::warn;

use crate::Runs;

pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);

/// What a client that lagged behind the bus receives: re-read the whole state.
pub const RESYNC: &str = r#"{"type":"resync"}"#;

/// How long a connection keeps the answer to « may I read this Run? ».
pub const READ_DECISION_TTL: Duration = Duration::from_secs(5);

/// How long a decision is kept at all: a `run_forgotten` (whose Run, gone,
/// has no Projet any more) is judged by the last decision about its Run.
const FORGET_DECISIONS_AFTER: Duration = Duration::from_secs(600);

/// The path of the stream, as the journal names it.
const PATH: &str = "/ws";

pub(super) async fn upgrade(
    State(runs): State<Arc<Runs>>,
    Extension(actor): Extension<Actor>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    // `/ws` streams every event of every Run to any subscriber, so without this guard
    // any web page open in the operator's browser could subscribe and exfiltrate
    // repos/prompts/verdicts. `headers` MUST precede `ws`: the upgrade extractor
    // consumes the request, so it stays last (axum 0.8).
    let requested = Instant::now();
    let options = runs.options();
    if !check_origin(&headers, options.daemon_port, &options.allowed_ws_origins) {
        ws_log::refused(PATH, StatusCode::FORBIDDEN, "Origin not allowed", &headers);
        return (StatusCode::FORBIDDEN, "Origin not allowed").into_response();
    }
    ws.on_upgrade(move |socket| async move {
        ws_log::opened(PATH, requested.elapsed());
        let opened = Instant::now();
        let closing = relay(socket, runs, actor).await;
        ws_log::closed(PATH, closing, opened.elapsed());
    })
}

/// Relay the bus to `socket` until either side ends it; how it ended.
async fn relay(mut socket: WebSocket, runs: Arc<Runs>, actor: Actor) -> Closing {
    let ready = serde_json::json!({ "type": "ready" });
    if socket
        .send(Message::Text(ready.to_string().into()))
        .await
        .is_err()
    {
        return Closing::Dropped;
    }

    let mut bus = runs.bus().subscribe();
    let mut heartbeat = time::interval(HEARTBEAT_INTERVAL);
    let mut visible = Visibility::new(&runs, actor);

    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                visible.forget_old();
                let msg = serde_json::json!({
                    "type": "heartbeat",
                    "ts": unix_timestamp_secs(),
                });
                if socket.send(Message::Text(msg.to_string().into())).await.is_err() {
                    return Closing::Dropped;
                }
            }
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Close(frame))) => {
                        // The answer is queued by the protocol; flushing sends
                        // it before the connection is dropped.
                        let _ = socket.flush().await;
                        return Closing::ByClient(frame.map_or(NO_STATUS, |f| f.code));
                    }
                    // The client has nothing to say on this stream; pings are
                    // answered by the protocol itself.
                    Some(Ok(_)) => {}
                    None | Some(Err(_)) => return Closing::Dropped,
                }
            }
            message = bus.recv() => {
                match message {
                    Ok(msg) => {
                        let Some(msg) = visible.filter(msg).await else {
                            continue;
                        };
                        if socket.send(Message::Text(msg.to_string().into())).await.is_err() {
                            return Closing::Dropped;
                        }
                    }
                    // ADR-0083 §6: the bus dropped messages this client never
                    // got, so its picture may miss a transition. Nothing is
                    // replayed: the client is told to re-read everything.
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!("WebSocket client lagged by {n} events, asking it to resync");
                        if socket.send(Message::Text(RESYNC.into())).await.is_err() {
                            return Closing::Dropped;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        let _ = socket.close().await;
                        return Closing::ByDaemon;
                    }
                }
            }
        }
    }
}

/// What one connection may see of the Runs, with the decisions it got.
struct Visibility<'a> {
    runs: &'a Runs,
    actor: Actor,
    decisions: HashMap<String, (bool, Instant)>,
    /// The decisions on the guarded reads, by the read's JSON.
    guards: HashMap<String, (bool, Instant)>,
}

impl<'a> Visibility<'a> {
    fn new(runs: &'a Runs, actor: Actor) -> Self {
        Self {
            runs,
            actor,
            decisions: HashMap::new(),
            guards: HashMap::new(),
        }
    }

    /// What of `message` reaches the connection: all of it, the message
    /// without the Run it may not read, or nothing. Never its guard.
    async fn filter(&mut self, mut message: Value) -> Option<Value> {
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
    fn forget_old(&mut self) {
        self.decisions
            .retain(|_, (_, at)| at.elapsed() < FORGET_DECISIONS_AFTER);
        self.guards
            .retain(|_, (_, at)| at.elapsed() < READ_DECISION_TTL);
    }
}

/// The Run a message of the bus is about, and whether that Run is gone: a
/// Run's event (`{"type": "event", "event": {"run_id"…}}`), its
/// `run_forgotten`, an event of one of its sessions (`session_started`,
/// `session_taken_over`, `session_ended`), an `operation_applied` on a Run or
/// one of its nodes (the creation of a Run has no target: the Run is the
/// entity it produced).
/// `None` for everything else.
fn run_of_message(message: &Value) -> Option<(&str, bool)> {
    match message["type"].as_str()? {
        "event" => Some((message["event"]["run_id"].as_str()?, false)),
        "run_forgotten" => Some((message["run_id"].as_str()?, true)),
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

/// The read guarding an `operation_applied` on a Trigger or on an
/// extension's Ressource: the read of the entity it is about (its target, or
/// the entity it created), on the same Ressource, in the Projets of its
/// Operation. `None` for the other messages, and for a creation whose entity
/// nobody knows.
fn entity_read_of(message: &Value) -> Option<Value> {
    if message["type"] != "operation_applied" {
        return None;
    }
    let operation: Operation = serde_json::from_value(message["operation"].clone()).ok()?;
    if !matches!(
        operation.resource,
        ResourceKind::Trigger | ResourceKind::Extension { .. }
    ) {
        return None;
    }
    let target = match operation.target {
        Some(target) => target,
        None => message["entity"]["id"].as_str()?.to_string(),
    };
    let read = Operation::new(operation.resource, Verb::Read)
        .on(target)
        .in_projects(operation.projects);
    serde_json::to_value(read).ok()
}

/// The Run a message about something else names in passing: the Run a
/// Trigger's fire created.
fn run_mentioned_by(message: &Value) -> Option<&str> {
    match message["type"].as_str()? {
        "trigger_fired" => message["run_id"].as_str(),
        _ => None,
    }
}

fn unix_timestamp_secs() -> String {
    use std::time::SystemTime;
    let d = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = d.as_secs();
    let millis = d.subsec_millis();
    format!("{secs}.{millis:03}")
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
