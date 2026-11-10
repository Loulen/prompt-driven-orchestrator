//! CORE-239 — a batch on its way to the Pipeline Manager: nothing typed into a
//! prompt only a person can answer, a manager that ends before answering said
//! on the comments, and « Send again » without rewriting them. Time is paused:
//! the delivery's minutes pass in a blink.

use std::time::Duration;

use pdo_kernel::Actor;
use pdo_review::application::failure::Failure;
use pdo_review::application::ports::ReviewRuns;
use pdo_review::application::reply::ReplyCommand;
use pdo_review::application::send_comments::DraftComment;
use pdo_review::domain::comments::delivery::{EXITED_AT_TRUST_PROMPT_REASON, TRUST_PROMPT_REASON};
use pdo_review::domain::comments::{ReviewComment, MANAGER_NODE_ID};
use pdo_run::domain::event::EventKind;
use pdo_run::domain::review_comment::ReviewDeliveryState;

use crate::fakes::World;

const TRUST_PROMPT: &str = "Quick safety check: Is this a project you created or one you trust?\n\
    ❯ 1. No, exit\n  2. Yes, I trust this folder";
const READY: &str = "╭───╮\n│ > │\n╰───╯\n  ? for shortcuts";

fn draft(line: i64) -> DraftComment {
    DraftComment {
        path: "src/a.rs".into(),
        side: "new".into(),
        line,
        text: "why?".into(),
        ..DraftComment::default()
    }
}

async fn send(w: &World, lines: &[i64]) {
    w.review
        .send_comments(
            &Actor::LocalOperator,
            "r1",
            lines.iter().map(|l| draft(*l)).collect(),
        )
        .await
        .unwrap();
}

/// A moment for the delivery to take its next looks.
async fn moments(secs: u64) {
    tokio::time::sleep(Duration::from_secs(secs)).await;
}

async fn comments(w: &World) -> Vec<ReviewComment> {
    let run = w.runs.load("r1").await.unwrap().unwrap();
    run.state.review_comments
}

async fn delivery_of(w: &World, id: &str) -> Option<(ReviewDeliveryState, Option<String>)> {
    comments(w)
        .await
        .into_iter()
        .find(|c| c.id == id)
        .and_then(|c| c.delivery)
        .map(|d| (d.state, d.reason))
}

#[tokio::test(start_paused = true)]
async fn a_manager_that_exits_before_answering_is_said_on_its_comments() {
    let w = World::new();
    w.completed_run("r1").await;
    w.runs.manager_shows(READY);
    send(&w, &[1, 2]).await;
    moments(3).await;
    assert_eq!(delivery_of(&w, "rc-001").await, None, "on its way");

    w.runs.manager_exits();
    moments(10).await;
    for id in ["rc-001", "rc-002"] {
        assert_eq!(
            delivery_of(&w, id).await,
            Some((ReviewDeliveryState::Exited, None)),
            "{id}"
        );
    }
    let events = w.runs.payloads("r1", EventKind::ReviewCommentDelivery);
    assert_eq!(events.len(), 1, "one event for the batch: {events:?}");
    assert_eq!(events[0]["ids"], serde_json::json!(["rc-001", "rc-002"]));
}

#[tokio::test(start_paused = true)]
async fn an_answered_comment_is_not_said_lost_when_the_manager_ends() {
    let w = World::new();
    w.completed_run("r1").await;
    w.runs.manager_shows(READY);
    send(&w, &[1, 2]).await;
    let manager = Actor::Agent {
        run_id: "r1".into(),
        node_id: MANAGER_NODE_ID.into(),
    };
    let reply = ReplyCommand {
        text: "done".into(),
        resolved: false,
        author: None,
        session_node: Some(MANAGER_NODE_ID.into()),
    };
    w.review
        .reply(&manager, "r1", "rc-001", reply)
        .await
        .unwrap();
    w.runs.manager_exits();
    moments(10).await;
    assert_eq!(delivery_of(&w, "rc-001").await, None, "answered");
    assert_eq!(
        delivery_of(&w, "rc-002").await.map(|d| d.0),
        Some(ReviewDeliveryState::Exited)
    );
}

#[tokio::test(start_paused = true)]
async fn nothing_is_typed_into_a_trust_prompt_and_the_thread_says_how_to_unblock() {
    let w = World::new();
    w.completed_run("r1").await;
    // A manager already running, parked on its agent's folder-trust prompt.
    w.runs.manager_shows(TRUST_PROMPT);
    *w.runs.manager_running.lock().unwrap() = true;
    send(&w, &[1]).await;
    moments(5).await;
    assert!(
        w.runs.pasted.lock().unwrap().is_empty(),
        "nothing typed into it"
    );
    assert_eq!(
        delivery_of(&w, "rc-001").await,
        Some((
            ReviewDeliveryState::Blocked,
            Some(TRUST_PROMPT_REASON.to_string())
        ))
    );

    // The person answers it in the Manager tab: the batch goes out.
    w.runs.manager_shows(READY);
    moments(3).await;
    assert_eq!(delivery_of(&w, "rc-001").await, None);
    let pasted = w.runs.pasted.lock().unwrap().clone();
    assert_eq!(pasted.len(), 1);
    assert!(pasted[0].1.contains("[rc-001]"));
}

#[tokio::test(start_paused = true)]
async fn a_manager_that_exits_at_the_trust_prompt_says_so_and_send_again_restarts_it() {
    let w = World::new();
    w.completed_run("r1").await;
    w.runs.manager_shows(TRUST_PROMPT);
    send(&w, &[1, 2]).await;
    moments(3).await;
    assert_eq!(
        delivery_of(&w, "rc-002").await.map(|d| d.0),
        Some(ReviewDeliveryState::Blocked)
    );
    // « No, exit »: the session ends.
    w.runs.manager_exits();
    moments(3).await;
    assert_eq!(
        delivery_of(&w, "rc-001").await,
        Some((
            ReviewDeliveryState::Exited,
            Some(EXITED_AT_TRUST_PROMPT_REASON.to_string())
        ))
    );

    // « Send again »: a new manager reads both comments in its launch prompt.
    w.runs.manager_shows(READY);
    let body = w
        .review
        .resend_comments(
            &Actor::LocalOperator,
            "r1",
            vec!["rc-001".into(), "rc-002".into(), "rc-001".into()],
        )
        .await
        .unwrap();
    assert_eq!(body["manager_started"], true);
    assert_eq!(body["resent"], serde_json::json!(["rc-001", "rc-002"]));
    let launched = w.runs.launched_with.lock().unwrap().clone();
    assert_eq!(launched.len(), 2, "started again");
    assert!(launched[1].contains("[rc-001]") && launched[1].contains("[rc-002]"));
    assert_eq!(delivery_of(&w, "rc-001").await, None, "on its way again");
    let resent = w.runs.payloads("r1", EventKind::ReviewCommentDelivery);
    let last = resent.last().unwrap();
    assert_eq!(last["state"], "awaiting");
    assert_eq!(last["actor"]["kind"], "local_operator");
    moments(3).await;
    assert!(w.runs.pasted.lock().unwrap().is_empty());
}

#[tokio::test(start_paused = true)]
async fn send_again_into_a_running_manager_types_the_comments_as_written() {
    let w = World::new();
    w.completed_run("r1").await;
    w.runs.manager_shows(READY);
    send(&w, &[1]).await;
    w.runs.manager_exits();
    moments(5).await;
    *w.runs.manager_running.lock().unwrap() = true;
    let body = w
        .review
        .resend_comments(&Actor::LocalOperator, "r1", vec!["rc-001".into()])
        .await
        .unwrap();
    assert_eq!(body["manager_started"], false);
    moments(2).await;
    let pasted = w.runs.pasted.lock().unwrap().clone();
    assert_eq!(pasted.len(), 1);
    assert!(pasted[0].1.contains("[rc-001]") && pasted[0].1.contains("why?"));
}

#[tokio::test(start_paused = true)]
async fn send_again_refuses_nothing_to_send_an_unknown_and_an_answered_comment() {
    let w = World::new();
    w.completed_run("r1").await;
    w.runs.manager_shows(READY);
    send(&w, &[1, 2]).await;
    let actor = Actor::LocalOperator;
    let status = |r: Result<serde_json::Value, Failure>| match r {
        Err(Failure::Text(code, _)) | Err(Failure::Json(code, _)) => code,
        other => panic!("{other:?}"),
    };
    assert_eq!(
        status(w.review.resend_comments(&actor, "r1", vec![]).await),
        400
    );
    assert_eq!(
        status(
            w.review
                .resend_comments(&actor, "r1", vec!["rc-404".into()])
                .await
        ),
        404
    );
    let reply = ReplyCommand {
        text: "done".into(),
        resolved: false,
        author: None,
        session_node: Some(MANAGER_NODE_ID.into()),
    };
    w.review.reply(&actor, "r1", "rc-001", reply).await.unwrap();
    assert_eq!(
        status(
            w.review
                .resend_comments(&actor, "r1", vec!["rc-001".into()])
                .await
        ),
        409
    );
    // rc-002 is still on its way to the running manager: it would reach it twice.
    assert_eq!(
        status(
            w.review
                .resend_comments(&actor, "r1", vec!["rc-002".into()])
                .await
        ),
        409
    );
}
