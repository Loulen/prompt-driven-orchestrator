//! Every gesture on a comment asks the Autorisation d'opération first, and a
//! refusal writes nothing — before the Run is even looked up.

use pdo_kernel::Actor;
use pdo_review::application::failure::Failure;
use pdo_review::application::reply::ReplyCommand;
use pdo_review::application::send_comments::DraftComment;
use pdo_review::domain::comments::decision::Decision;

use crate::fakes::World;

fn draft() -> DraftComment {
    DraftComment {
        path: "src/a.rs".into(),
        side: "new".into(),
        line: 1,
        text: "why?".into(),
        ..DraftComment::default()
    }
}

#[tokio::test]
async fn each_gesture_names_its_operation_in_the_projets_of_its_run() {
    let w = World::new();
    w.completed_run("r1").await;
    w.runs
        .projects
        .lock()
        .unwrap()
        .insert("r1".into(), vec!["p1".into()]);
    let actor = Actor::LocalOperator;
    let _ = w.review.send_comments(&actor, "r1", vec![draft()]).await;
    let _ = w
        .review
        .reply(&actor, "r1", "rc-001", ReplyCommand::default())
        .await;
    let _ = w
        .review
        .decide(&actor, "r1", "rc-001", Decision::Resolve, None)
        .await;
    let _ = w
        .review
        .decide(&actor, "r1", "rc-001", Decision::Reopen, None)
        .await;
    assert_eq!(
        w.asked(),
        vec![
            r#"Operation { resource: ReviewComment, verb: Create, target: Some("r1"), projects: ["p1"] }"#,
            r#"Operation { resource: ReviewComment, verb: Action("reply"), target: Some("r1/rc-001"), projects: ["p1"] }"#,
            r#"Operation { resource: ReviewComment, verb: Action("resolve"), target: Some("r1/rc-001"), projects: ["p1"] }"#,
            r#"Operation { resource: ReviewComment, verb: Action("reopen"), target: Some("r1/rc-001"), projects: ["p1"] }"#,
        ]
    );
}

#[tokio::test]
async fn a_refusal_answers_403_and_writes_nothing() {
    let w = World::new();
    w.completed_run("r1").await;
    w.refuse();
    let actor = Actor::Agent {
        run_id: "r1".into(),
        node_id: "worker".into(),
    };
    let before = w.runs.kinds("r1");
    let refused = |r: Result<serde_json::Value, Failure>| matches!(r, Err(Failure::Refused(_)));
    assert!(refused(
        w.review.send_comments(&actor, "r1", vec![draft()]).await
    ));
    let reply = ReplyCommand {
        text: "done".into(),
        resolved: true,
        author: None,
    };
    assert!(refused(w.review.reply(&actor, "r1", "rc-001", reply).await));
    assert!(refused(
        w.review
            .decide(&actor, "r1", "rc-001", Decision::Resolve, None)
            .await
    ));
    // Refused before the Run is looked up: an unknown Run is refused too.
    assert!(refused(
        w.review
            .send_comments(&actor, "no-such-run", vec![draft()])
            .await
    ));
    assert_eq!(w.runs.kinds("r1"), before);
    assert!(w.runs.handed.lock().unwrap().is_empty());
}

#[tokio::test]
async fn the_reads_never_ask() {
    let w = World::new();
    w.completed_run("r1").await;
    w.refuse();
    assert!(w.review.run_diff("r1").await.is_ok());
    assert!(w.review.structured_diff("r1", None, None).await.is_ok());
    assert!(w
        .review
        .list_comments("r1", Default::default())
        .await
        .is_ok());
    assert!(w.asked().is_empty());
}
