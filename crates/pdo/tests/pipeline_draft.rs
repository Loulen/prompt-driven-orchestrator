//! Layer 4 (ADR-0080) — the shared draft of a pipeline (ADR-0088) over its
//! WebSocket, `WS /pipelines/{id}/draft`, with two clients: a change of one
//! reaches the other, concurrent changes of two fields both stay, presence
//! comes and goes, a client that may not update the pipeline reads, and the draft survives a
//! restart of the daemon.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use pdo::extension_api::{ResourceKind, Verb};
use pdo_pipeline::adapters::draft_doc::YDraftDocuments;
use pdo_pipeline::application::ports::{DraftDocument, DraftDocuments};
use pdo_pipeline::domain::draft::DraftContent;
use serde_json::{json, Value};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use crate::common::{flow, TestDaemon};
use crate::test_extension::{Refusal, TestExtension, ACTOR_HEADER};

const SYNC_REQUEST: u8 = 0;
const SYNC_REPLY: u8 = 1;
const UPDATE: u8 = 2;

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// A browser's side of the draft: its socket and its copy.
struct Client {
    socket: Socket,
    doc: Box<dyn DraftDocument>,
    welcome: Value,
}

impl Client {
    async fn join(daemon: &TestDaemon, pipeline: &str, actor: Option<&str>) -> Self {
        let mut request = format!("ws://{}/pipelines/{pipeline}/draft", daemon.addr)
            .into_client_request()
            .unwrap();
        if let Some(actor) = actor {
            request
                .headers_mut()
                .insert(ACTOR_HEADER, actor.parse().unwrap());
        }
        let (socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
        let mut client = Self {
            socket,
            doc: YDraftDocuments.create(&DraftContent::default()),
            welcome: Value::Null,
        };
        client.welcome = client.text_of("welcome").await;
        // Ask for the draft, with an empty copy: the whole draft comes back.
        let empty = YDraftDocuments
            .create(&DraftContent::default())
            .state_vector();
        client.send(SYNC_REQUEST, &empty).await;
        let reply = client.binary_of(SYNC_REPLY).await;
        client.doc = YDraftDocuments.load(&reply).unwrap();
        client
    }

    async fn send(&mut self, tag: u8, payload: &[u8]) {
        let mut frame = vec![tag];
        frame.extend_from_slice(payload);
        self.socket
            .send(Message::Binary(frame.into()))
            .await
            .unwrap();
    }

    /// Change the copy and send the change.
    async fn edit(&mut self, edit: impl FnOnce(&mut DraftContent)) {
        let before = self.doc.content();
        let mut after = before.clone();
        edit(&mut after);
        let update = self.doc.change(&before, &after);
        self.send(UPDATE, &update).await;
    }

    async fn next(&mut self) -> Message {
        timeout(Duration::from_secs(5), self.socket.next())
            .await
            .expect("a message within 5 s")
            .expect("the socket is open")
            .unwrap()
    }

    /// The next text message of type `kind`, skipping the others.
    async fn text_of(&mut self, kind: &str) -> Value {
        loop {
            if let Message::Text(body) = self.next().await {
                let message: Value = serde_json::from_str(&body).unwrap();
                if message["type"] == kind {
                    return message;
                }
            }
        }
    }

    /// The payload of the next binary frame tagged `tag`.
    async fn binary_of(&mut self, tag: u8) -> Vec<u8> {
        loop {
            if let Message::Binary(frame) = self.next().await {
                if frame.first() == Some(&tag) {
                    return frame[1..].to_vec();
                }
            }
        }
    }

    /// Apply the next update from another participant.
    async fn hear_update(&mut self) {
        let update = self.binary_of(UPDATE).await;
        self.doc.apply_update(&update).unwrap();
    }

    /// The participants, once their count is `count`.
    async fn participants(&mut self, count: usize) -> Vec<Value> {
        loop {
            let message = self.text_of("participants").await;
            let participants = message["participants"].as_array().unwrap().clone();
            if participants.len() == count {
                return participants;
            }
        }
    }
}

fn seed(repo: &std::path::Path) -> anyhow::Result<()> {
    flow::write_pipeline(
        repo,
        "draft-target",
        &flow::solo_pipeline_yaml("draft-target", false),
    )
}

fn worker(content: &DraftContent) -> &Value {
    content.pipeline["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["type"] == "agent")
        .unwrap()
}

fn worker_index(content: &DraftContent) -> usize {
    content.pipeline["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .position(|n| n["type"] == "agent")
        .unwrap()
}

#[tokio::test]
async fn two_clients_share_one_draft_and_both_concurrent_fields_stay() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let mut alice = Client::join(&daemon, "draft-target", None).await;
    let mut bob = Client::join(&daemon, "draft-target", None).await;
    assert_eq!(alice.welcome["can_write"], true);
    assert_eq!(alice.welcome["generation"], bob.welcome["generation"]);
    assert_ne!(alice.welcome["color"], bob.welcome["color"]);
    assert_eq!(alice.participants(2).await.len(), 2);

    // A change of one reaches the other.
    let i = worker_index(&alice.doc.content());
    alice
        .edit(|c| c.pipeline["nodes"][i]["name"] = json!("designer"))
        .await;
    bob.hear_update().await;
    assert_eq!(worker(&bob.doc.content())["name"], "designer");
    assert_eq!(bob.text_of("status").await["changes"], 1);

    // Both change a different field of the same node at once.
    alice
        .edit(|c| c.pipeline["nodes"][i]["view"] = json!({"x": 640, "y": 80}))
        .await;
    bob.edit(|c| c.pipeline["nodes"][i]["interactive"] = json!(true))
        .await;
    alice.hear_update().await;
    bob.hear_update().await;
    assert_eq!(alice.doc.content(), bob.doc.content());
    let node = worker(&alice.doc.content()).clone();
    assert_eq!(node["view"], json!({"x": 640, "y": 80}));
    assert_eq!(node["interactive"], true);

    // Presence: a selection shows, a departure too.
    bob.socket
        .send(Message::Text(
            json!({"type": "presence", "presence": {"selection": {"kind": "node", "id": "worker"}}})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    loop {
        let seen = alice.participants(2).await;
        if seen.iter().any(|p| p["selection"]["id"] == "worker") {
            break;
        }
    }
    bob.socket.close(None).await.unwrap();
    assert_eq!(alice.participants(1).await.len(), 1);

    // The list shows the draft.
    let list = daemon.get_json("/pipelines").await;
    let entry = list
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == "draft-target")
        .unwrap();
    assert_eq!(entry["draft"]["changes"], 1);
    assert_eq!(entry["draft"]["participants"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn the_draft_survives_a_restart_of_the_daemon() {
    let repo = tempfile::tempdir().unwrap();
    seed(repo.path()).unwrap();
    let generation = {
        let daemon = TestDaemon::spawn_at(repo.path()).await.unwrap();
        let mut alice = Client::join(&daemon, "draft-target", None).await;
        let i = worker_index(&alice.doc.content());
        alice
            .edit(|c| c.pipeline["nodes"][i]["view"] = json!({"x": 1, "y": 2}))
            .await;
        // Kept once the daemon counted it.
        assert_eq!(alice.text_of("status").await["changes"], 1);
        alice.welcome["generation"].clone()
    };
    let daemon = TestDaemon::spawn_at(repo.path()).await.unwrap();
    let again = Client::join(&daemon, "draft-target", None).await;
    assert_eq!(again.welcome["generation"], generation);
    assert_eq!(
        worker(&again.doc.content())["view"],
        json!({"x": 1, "y": 2})
    );
}

#[tokio::test]
async fn a_client_who_may_not_update_reads_and_its_changes_are_dropped() {
    let (extension, _) = TestExtension::new(Refusal::Only(ResourceKind::Pipeline, Verb::Update));
    let daemon = TestDaemon::spawn_with_extensions(seed, extension.boxed())
        .await
        .unwrap();
    let mut reader = Client::join(&daemon, "draft-target", Some("reader")).await;
    assert_eq!(reader.welcome["can_write"], false);
    let i = worker_index(&reader.doc.content());
    reader
        .edit(|c| c.pipeline["nodes"][i]["name"] = json!("vandal"))
        .await;
    let fresh = Client::join(&daemon, "draft-target", Some("other")).await;
    assert_ne!(worker(&fresh.doc.content())["name"], "vandal");
}

#[tokio::test]
async fn an_unknown_pipeline_has_no_draft() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let url = format!("ws://{}/pipelines/nope/draft", daemon.addr);
    assert!(tokio_tungstenite::connect_async(url).await.is_err());
}
