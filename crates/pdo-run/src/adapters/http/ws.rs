//! `/ws`: the live stream every client subscribes to. It relays the kernel's
//! bus as is (D20), after a `ready` greeting, with a heartbeat every
//! [`HEARTBEAT_INTERVAL`].
//!
//! Filtered per connection (CORE-5): a message about a Run reaches a
//! connection only if its Actor (the one `identify` set on the upgrade
//! request) may read that Run. Each decision is kept
//! [`READ_DECISION_TTL`], so a busy Run asks once per few seconds, not once
//! per event; a change of rights shows within that delay. A `trigger_fired`
//! still reaches the connection, without the `run_id` of a Run it may not
//! read. The other messages (Triggers, pipelines, Projets, the extensions'
//! events…) pass as before.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Extension;
use pdo_kernel::Actor;
use pdo_session::adapters::http::origin::{check_origin, origin_for_log};
use serde_json::Value;
use tokio::sync::broadcast;
use tokio::time;
use tracing::{info, warn};

use crate::Runs;

pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);

/// How long a connection keeps the answer to « may I read this Run? ».
pub const READ_DECISION_TTL: Duration = Duration::from_secs(5);

/// How long a decision is kept at all: a `run_forgotten` (whose Run, gone,
/// has no Projet any more) is judged by the last decision about its Run.
const FORGET_DECISIONS_AFTER: Duration = Duration::from_secs(600);

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
    let options = runs.options();
    if !check_origin(&headers, options.daemon_port, &options.allowed_ws_origins) {
        warn!(
            "Rejected /ws WebSocket upgrade: Origin not allowed ({})",
            origin_for_log(&headers)
        );
        return (StatusCode::FORBIDDEN, "Origin not allowed").into_response();
    }
    ws.on_upgrade(move |socket| relay(socket, runs, actor))
}

async fn relay(mut socket: WebSocket, runs: Arc<Runs>, actor: Actor) {
    info!("WebSocket client connected");
    let ready = serde_json::json!({ "type": "ready" });
    if socket
        .send(Message::Text(ready.to_string().into()))
        .await
        .is_err()
    {
        return;
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
                    break;
                }
            }
            message = bus.recv() => {
                match message {
                    Ok(msg) => {
                        let Some(msg) = visible.filter(msg).await else {
                            continue;
                        };
                        if socket.send(Message::Text(msg.to_string().into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!("WebSocket client lagged by {n} events");
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }

    info!("WebSocket client disconnected");
}

/// What one connection may see of the Runs, with the decisions it got.
struct Visibility<'a> {
    runs: &'a Runs,
    actor: Actor,
    decisions: HashMap<String, (bool, Instant)>,
}

impl<'a> Visibility<'a> {
    fn new(runs: &'a Runs, actor: Actor) -> Self {
        Self {
            runs,
            actor,
            decisions: HashMap::new(),
        }
    }

    /// What of `message` reaches the connection: all of it, the message
    /// without the Run it may not read, or nothing.
    async fn filter(&mut self, mut message: Value) -> Option<Value> {
        if self.runs.extensions().allows_everything() {
            return Some(message);
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

    /// Drop the decisions nobody needs any more.
    fn forget_old(&mut self) {
        self.decisions
            .retain(|_, (_, at)| at.elapsed() < FORGET_DECISIONS_AFTER);
    }
}

/// The Run a message of the bus is about, and whether that Run is gone: a
/// Run's event (`{"type": "event", "event": {"run_id"…}}`), its
/// `run_forgotten`, an `operation_applied` on a Run or one of its nodes (the
/// creation of a Run has no target: the Run is the entity it produced).
/// `None` for everything else.
fn run_of_message(message: &Value) -> Option<(&str, bool)> {
    match message["type"].as_str()? {
        "event" => Some((message["event"]["run_id"].as_str()?, false)),
        "run_forgotten" => Some((message["run_id"].as_str()?, true)),
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
    fn the_other_messages_are_about_no_run() {
        for message in [
            json!({ "type": "pipeline_changed" }),
            json!({ "type": "trigger_fired", "trigger_id": "t1", "run_id": "r1" }),
            json!({ "type": "project_changed" }),
            json!({
                "type": "operation_applied",
                "operation": { "resource": "trigger", "verb": "update", "target": "t1" },
            }),
            json!({ "type": "extension_event", "extension": "pm", "name": "x", "payload": {} }),
        ] {
            assert_eq!(run_of_message(&message), None, "{message}");
        }
    }
}
