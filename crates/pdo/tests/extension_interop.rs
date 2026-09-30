//! Stage 4 (ADR-0080) — an extension's own mutations through its Host
//! (amendment #34 of ADR-0081), against the real daemon: the interop peers
//! `a`, `b`, `c` and the test extension installed together. An Operation on
//! an extension's resource is refusable by any extension, the emitter
//! included; its event then its `OperationApplied` reach `/ws` and the other
//! extensions; the other extensions' attributes land on the entity, prefixed.

use std::time::{Duration, Instant};

use futures_util::StreamExt;
use pdo::extension_api::{ResourceKind, Verb};
use serde_json::{json, Value};

use crate::common::flow::{post_json, seed_solo};
use crate::common::TestDaemon;
use crate::test_extension::interop::{self, Peer, PeerControl};
use crate::test_extension::{self, OnEvent, Refusal, TestExtension, ACTOR_HEADER};

/// How long a response may take while an extension sleeps 30 s in
/// `on_event`: far below the sleep, far above a response on a loaded machine
/// (the suite runs next to other builds).
const RESPONSE_BUDGET: Duration = Duration::from_secs(15);

fn seed(repo: &std::path::Path) -> anyhow::Result<()> {
    seed_solo(repo, &[("solo", false)])
}

/// A daemon with the test extension, then the peers `a`, `b`, `c`.
async fn daemon() -> (TestDaemon, test_extension::Control, Vec<PeerControl>) {
    let (ext, control) = TestExtension::new(Refusal::Nothing);
    let (peers, peer_controls) = Peer::trio();
    let mut extensions = ext.boxed();
    extensions.extend(peers);
    let daemon = TestDaemon::spawn_with_extensions(seed, extensions)
        .await
        .unwrap();
    (daemon, control, peer_controls)
}

/// `POST /ext/<peer>/items` as `actor`.
async fn create_item(daemon: &TestDaemon, peer: &str, actor: &str, name: &str) -> (u16, Value) {
    let resp = reqwest::Client::new()
        .post(format!("{}/ext/{peer}/items", daemon.url()))
        .header(ACTOR_HEADER, actor)
        .json(&json!({ "name": name }))
        .send()
        .await
        .unwrap();
    let status = resp.status().as_u16();
    (status, resp.json().await.unwrap_or(Value::Null))
}

/// Read `/ws` until `done` holds on what came, 10 s budget.
async fn read_ws_until(
    ws: &mut (impl StreamExt<
        Item = Result<
            tokio_tungstenite::tungstenite::Message,
            tokio_tungstenite::tungstenite::Error,
        >,
    > + Unpin),
    done: impl Fn(&[Value]) -> bool,
) -> Vec<Value> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut seen = Vec::new();
    while !done(&seen) {
        assert!(Instant::now() < deadline, "never came on /ws: {seen:#?}");
        let Ok(Some(Ok(msg))) = tokio::time::timeout(Duration::from_millis(300), ws.next()).await
        else {
            continue;
        };
        if let Ok(Ok(value)) = msg.to_text().map(serde_json::from_str::<Value>) {
            seen.push(value);
        }
    }
    seen
}

/// Wait until `check` holds on `events()` (10 s budget).
async fn wait_until(
    events: impl Fn() -> Vec<Value>,
    check: impl Fn(&[Value]) -> bool,
) -> Vec<Value> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let seen = events();
        if check(&seen) {
            return seen;
        }
        assert!(Instant::now() < deadline, "never came: {seen:#?}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn is_applied_on(event: &Value, resource: &str) -> bool {
    event["type"] == "operation_applied" && event["operation"]["resource"] == resource
}

fn is_item_created(event: &Value, id: &Value) -> bool {
    event["type"] == "extension_event"
        && event["name"] == interop::ITEM_CREATED
        && event["payload"]["id"] == *id
}

#[tokio::test]
async fn an_item_created_through_the_host_publishes_its_event_then_its_operation_applied() {
    let (daemon, control, peers) = daemon().await;
    let mut ws = daemon.connect_ws().await.unwrap();

    let (status, item) = create_item(&daemon, "a", "alice", "first").await;
    assert_eq!(status, 201, "{item}");
    let id = item["id"].clone();
    assert_eq!(
        item["attributes"],
        json!({
            "test/started_by": "alice",
            "b/witness": "alice",
            "c/witness": "alice",
        }),
        "the others' attributes, prefixed, never the emitter's own"
    );
    // Read back by A's own route, the attributes are stored with the item.
    let read = daemon
        .get_json(&format!("/ext/a/items/{}", id.as_str().unwrap()))
        .await;
    assert_eq!(read, item);

    let relayed = read_ws_until(&mut ws, |seen| {
        seen.iter().any(|e| is_applied_on(e, "a/item"))
    })
    .await;
    let event = relayed
        .iter()
        .position(|e| is_item_created(e, &id))
        .expect("the event of A is on /ws");
    let applied = relayed
        .iter()
        .position(|e| is_applied_on(e, "a/item"))
        .unwrap();
    assert!(event < applied, "the event, then its OperationApplied");
    assert_eq!(
        relayed[event],
        json!({
            "type": "extension_event",
            "extension": "a",
            "name": "item_created",
            "payload": item,
        })
    );
    assert_eq!(
        relayed[applied],
        json!({
            "type": "operation_applied",
            "actor": { "kind": "identified", "namespace": "test", "id": "alice" },
            "operation": { "resource": "a/item", "verb": "create", "target": null },
            "entity": { "kind": "a/item", "id": id },
        })
    );

    // Every extension was asked, and the others observe both events.
    assert!(control.seen().iter().any(|(_, op)| {
        op.resource == ResourceKind::extension("a", "item") && op.verb == Verb::Create
    }));
    let both = |events: &[Value]| {
        events.iter().any(|e| is_item_created(e, &id))
            && events.iter().any(|e| is_applied_on(e, "a/item"))
    };
    wait_until(|| control.events(), both).await;
    wait_until(|| peers[1].events(), both).await;
    let own = wait_until(
        || peers[0].events(),
        |events| events.iter().any(|e| is_applied_on(e, "a/item")),
    )
    .await;
    assert!(
        !own.iter().any(|e| e["type"] == "extension_event"),
        "an extension never receives its own events: {own:#?}"
    );
}

#[tokio::test]
async fn any_extension_refuses_an_extension_operation_and_nothing_is_left() {
    let (daemon, control, peers) = daemon().await;

    // B refuses: the request fails with B's reason, nothing is stored.
    peers[1].set_refuses(true);
    let (status, body) = create_item(&daemon, "a", "alice", "refused").await;
    assert_eq!(status, 403, "{body}");
    let expected = interop::reason(
        "b",
        &pdo::extension_api::Operation::new(ResourceKind::extension("a", "item"), Verb::Create),
    );
    assert_eq!(
        body,
        json!({
            "error": "operation_refused",
            "message": format!("operation refused: {expected}"),
            "reason": expected,
        })
    );
    assert_eq!(daemon.get_json("/ext/a/items").await, json!([]));

    // The emitter itself refuses its own Operation.
    peers[1].set_refuses(false);
    peers[0].set_refuses(true);
    let (status, body) = create_item(&daemon, "a", "alice", "refused").await;
    assert_eq!(status, 403, "{body}");
    assert!(
        body["reason"]
            .as_str()
            .unwrap()
            .starts_with("extension `a`"),
        "{body}"
    );

    // An extension naming the resource by its wire name, then one panicking.
    peers[0].set_refuses(false);
    control.set(Refusal::parse("a/item:create").unwrap());
    let (status, body) = create_item(&daemon, "a", "alice", "refused").await;
    assert_eq!(status, 403);
    assert_eq!(
        body["reason"], "the test extension refuses create a/item",
        "{body}"
    );
    control.set(Refusal::Panic);
    let (status, body) = create_item(&daemon, "a", "alice", "refused").await;
    assert_eq!(status, 403);
    assert_eq!(body["reason"], "extension `test` failed");
    assert_eq!(daemon.get_json("/ext/a/items").await, json!([]));

    // No refused creation was applied: once a later mutation is observed, none
    // of the item's is there.
    control.set(Refusal::Nothing);
    let (status, _) = post_json(&daemon, "/projects", json!({ "name": "P" })).await;
    assert_eq!(status, 201);
    let events = wait_until(
        || control.events(),
        |events| events.iter().any(|e| is_applied_on(e, "project")),
    )
    .await;
    assert!(
        !events
            .iter()
            .any(|e| is_applied_on(e, "a/item") || e["type"] == "extension_event"),
        "{events:#?}"
    );
}

#[tokio::test]
async fn a_deletion_names_its_target_and_a_slow_extension_never_delays_the_emitter() {
    let (daemon, control, _) = daemon().await;
    control.on_event(OnEvent::Sleep(Duration::from_secs(30)));
    for name in ["one", "two", "three"] {
        let start = Instant::now();
        let (status, _) = create_item(&daemon, "c", "alice", name).await;
        assert_eq!(status, 201);
        assert!(
            start.elapsed() < RESPONSE_BUDGET,
            "a sleeping on_event delayed the emitter: {:?}",
            start.elapsed()
        );
    }

    let mut ws = daemon.connect_ws().await.unwrap();
    let resp = reqwest::Client::new()
        .delete(format!("{}/ext/c/items/c-1", daemon.url()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 204);
    let relayed = read_ws_until(&mut ws, |seen| {
        seen.iter().any(|e| is_applied_on(e, "c/item"))
    })
    .await;
    let applied = relayed.iter().find(|e| is_applied_on(e, "c/item")).unwrap();
    assert_eq!(
        applied["operation"],
        json!({ "resource": "c/item", "verb": "delete", "target": "c-1" })
    );
    assert_eq!(applied["actor"], json!({ "kind": "local_operator" }));
    let left = daemon.get_json("/ext/c/items").await;
    assert_eq!(left.as_array().unwrap().len(), 2, "{left}");
}

#[tokio::test]
async fn a_core_mutation_is_followed_by_its_operation_applied_on_ws() {
    let (daemon, _, peers) = daemon().await;
    let mut ws = daemon.connect_ws().await.unwrap();
    let (status, project) = post_json(&daemon, "/projects", json!({ "name": "P" })).await;
    assert_eq!(status, 201);
    let relayed = read_ws_until(&mut ws, |seen| {
        seen.iter().any(|e| is_applied_on(e, "project"))
    })
    .await;
    let applied = relayed
        .iter()
        .find(|e| is_applied_on(e, "project"))
        .unwrap();
    assert_eq!(
        applied["entity"],
        json!({ "kind": "project", "id": project["id"] })
    );
    // A peer stamps the core's entities too, like any extension.
    assert_eq!(project["attributes"]["a/witness"], "local_operator");
    wait_until(
        || peers[2].events(),
        |events| events.iter().any(|e| is_applied_on(e, "project")),
    )
    .await;
}
