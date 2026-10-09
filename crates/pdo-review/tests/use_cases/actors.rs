//! CORE-63: every gesture on a review comment records the Actor of the request
//! that made it, on the comment and on the event, next to the author's role.

use pdo_kernel::Actor;
use pdo_review::application::reply::ReplyCommand;
use pdo_review::application::send_comments::DraftComment;
use pdo_review::domain::comments::decision::Decision;
use pdo_review::domain::comments::MANAGER_NODE_ID;
use pdo_run::domain::event::EventKind;
use serde_json::json;

use crate::fakes::World;

fn identified(id: &str) -> Actor {
    Actor::Identified {
        namespace: "test".into(),
        id: id.into(),
    }
}

fn draft(text: &str) -> DraftComment {
    DraftComment {
        path: "src/a.rs".into(),
        side: "new".into(),
        line: 1,
        text: text.into(),
        ..DraftComment::default()
    }
}

async fn sent_by(actor: &Actor) -> World {
    let w = World::new();
    w.completed_run("r1").await;
    w.review
        .send_comments(actor, "r1", vec![draft("Who wrote this?")])
        .await
        .unwrap();
    w
}

#[tokio::test]
async fn a_sent_comment_and_its_event_carry_the_actor_and_the_text() {
    let w = sent_by(&identified("alice")).await;
    let events = w.runs.payloads("r1", EventKind::ReviewCommentSent);
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0]["actor"],
        json!({ "kind": "identified", "namespace": "test", "id": "alice" })
    );
    assert_eq!(events[0]["text"], "Who wrote this?");
    assert_eq!(events[0]["author"], "user", "the role stays");
    let listed = w
        .review
        .list_comments("r1", Default::default())
        .await
        .unwrap();
    assert_eq!(listed["comments"][0]["actor"]["id"], "alice");
}

#[tokio::test]
async fn the_local_operator_is_recorded_too() {
    let w = sent_by(&Actor::LocalOperator).await;
    let events = w.runs.payloads("r1", EventKind::ReviewCommentSent);
    assert_eq!(events[0]["actor"], json!({ "kind": "local_operator" }));
}

#[tokio::test]
async fn a_reply_carries_the_actor_of_the_session_and_keeps_its_role() {
    let w = sent_by(&identified("alice")).await;
    // An extension identified the agent's session as Bob: the role still
    // comes from the session the call was made from.
    let body = w
        .review
        .reply(
            &identified("bob"),
            "r1",
            "rc-001",
            ReplyCommand {
                text: "Looking.".into(),
                session_node: Some(MANAGER_NODE_ID.into()),
                ..ReplyCommand::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(body["author"], "manager");
    assert_eq!(body["comment"]["replies"][0]["actor"]["id"], "bob");
    let events = w.runs.payloads("r1", EventKind::ReviewCommentReplied);
    assert_eq!(events[0]["actor"]["id"], "bob");
    assert_eq!(events[0]["text"], "Looking.");
    assert_eq!(events[0]["author"], "manager");
}

#[tokio::test]
async fn resolve_and_reopen_carry_the_actor_who_decides() {
    let w = sent_by(&identified("alice")).await;
    let body = w
        .review
        .decide(
            &identified("bob"),
            "r1",
            "rc-001",
            Decision::Resolve,
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(body["comment"]["resolved_by"], "user");
    assert_eq!(body["comment"]["resolved_by_actor"]["id"], "bob");
    w.review
        .decide(
            &identified("alice"),
            "r1",
            "rc-001",
            Decision::Reopen,
            None,
            None,
        )
        .await
        .unwrap();
    let resolved = w.runs.payloads("r1", EventKind::ReviewCommentResolved);
    assert_eq!(resolved[0]["actor"]["id"], "bob");
    let reopened = w.runs.payloads("r1", EventKind::ReviewCommentReopened);
    assert_eq!(reopened[0]["actor"]["id"], "alice");
    let listed = w
        .review
        .list_comments("r1", Default::default())
        .await
        .unwrap();
    assert_eq!(listed["comments"][0]["reopened_by_actor"]["id"], "alice");
    assert!(listed["comments"][0].get("resolved_by_actor").is_none());
}

#[tokio::test]
async fn a_decision_from_an_identified_session_is_signed_by_its_role() {
    let w = sent_by(&identified("alice")).await;
    let body = w
        .review
        .decide(
            &identified("bob"),
            "r1",
            "rc-001",
            Decision::Resolve,
            None,
            Some("xuTJYLUa"),
        )
        .await
        .unwrap();
    assert_eq!(body["comment"]["resolved_by"], "xuTJYLUa");
    assert_eq!(body["comment"]["resolved_by_actor"]["id"], "bob");
}
