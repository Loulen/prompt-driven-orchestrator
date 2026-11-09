//! Sending a batch, replying, resolving and reopening, over the in-memory log.

use pdo_kernel::Actor;
use pdo_review::application::failure::Failure;
use pdo_review::application::list_comments::ListComments;
use pdo_review::application::reply::ReplyCommand;
use pdo_review::application::send_comments::DraftComment;
use pdo_review::domain::comments::decision::Decision;
use pdo_review::domain::comments::MANAGER_NODE_ID;
use pdo_run::domain::event::EventKind;
use serde_json::json;

use crate::fakes::World;

fn draft(line: i64, text: &str) -> DraftComment {
    DraftComment {
        path: "src/a.rs".into(),
        side: "new".into(),
        line,
        text: text.into(),
        ..DraftComment::default()
    }
}

fn agent(node: &str) -> Actor {
    Actor::Agent {
        run_id: "r1".into(),
        node_id: node.into(),
    }
}

/// A Run with `n` comments sent by the operator.
async fn with_sent(n: usize) -> World {
    let w = World::new();
    w.completed_run("r1").await;
    let drafts = (1..=n as i64).map(|l| draft(l, "why?")).collect();
    w.review
        .send_comments(&Actor::LocalOperator, "r1", drafts)
        .await
        .unwrap();
    w
}

#[tokio::test(start_paused = true)]
async fn a_send_records_one_event_per_comment_and_hands_the_manager_one_message() {
    let w = World::new();
    w.completed_run("r1").await;
    w.runs.manager_shows("╭───╮\n│ > │\n╰───╯");
    w.git.files.lock().unwrap().insert(
        ("pdo/run-r1".into(), "src/a.rs".into()),
        "one\ntwo\nthree\n".into(),
    );
    let sent = w
        .review
        .send_comments(
            &Actor::LocalOperator,
            "r1",
            vec![draft(2, "rename"), draft(3, "test it  ")],
        )
        .await
        .unwrap();
    assert_eq!(sent["manager_started"], true);
    let ids: Vec<&str> = sent["sent"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec!["rc-001", "rc-002"]);
    assert_eq!(
        sent["sent"][0]["excerpt"],
        "  1 | one\n> 2 | two\n  3 | three"
    );
    assert_eq!(sent["sent"][1]["text"], "test it");
    assert_eq!(sent["sent"][0]["to_sha"], "a1a1a1a1a1");
    assert_eq!(sent["sent"][0]["batch_id"], sent["batch_id"]);
    let sent_events = w
        .runs
        .kinds("r1")
        .into_iter()
        .filter(|k| *k == EventKind::ReviewCommentSent)
        .count();
    assert_eq!(sent_events, 2);
    // CORE-239: the manager the send starts reads the batch in its launch
    // prompt; nothing is typed into a harness still starting.
    let launched = w.runs.launched_with.lock().unwrap().clone();
    assert_eq!(launched.len(), 1, "one message for the batch");
    assert!(launched[0].contains("[rc-001]") && launched[0].contains("[rc-002]"));
    assert!(
        launched[0].contains("(Fork point → Run tip)"),
        "{}",
        launched[0]
    );
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    assert!(w.runs.pasted.lock().unwrap().is_empty());

    // The next batch continues the sequence, into the running manager.
    let next = w
        .review
        .send_comments(&Actor::LocalOperator, "r1", vec![draft(1, "and this")])
        .await
        .unwrap();
    assert_eq!(next["sent"][0]["id"], "rc-003");
    assert_eq!(next["manager_started"], false);
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    let pasted = w.runs.pasted.lock().unwrap().clone();
    assert_eq!(pasted.len(), 1, "typed into the running manager");
    assert!(pasted[0].1.contains("[rc-003]"), "{}", pasted[0].1);
}

/// CORE-61: the manager a send starts is started by the author of the
/// comments, whose Droits it acts with; a manager already running keeps who
/// started it.
#[tokio::test]
async fn the_manager_a_send_starts_is_started_by_the_author_of_the_comments() {
    let w = World::new();
    w.completed_run("r1").await;
    let bob = Actor::Identified {
        namespace: "test".into(),
        id: "bob".into(),
    };
    w.review
        .send_comments(&bob, "r1", vec![draft(1, "why?")])
        .await
        .unwrap();
    w.review
        .send_comments(&Actor::LocalOperator, "r1", vec![draft(2, "and?")])
        .await
        .unwrap();
    assert_eq!(*w.runs.manager_started_by.lock().unwrap(), vec![bob]);
}

#[tokio::test]
async fn drafts_stay_drafts_when_the_send_is_refused() {
    let w = World::new();
    w.completed_run("r1").await;
    let actor = Actor::LocalOperator;
    let status = |r: Result<serde_json::Value, Failure>| r.unwrap_err().status();

    assert_eq!(
        status(w.review.send_comments(&actor, "r1", vec![]).await),
        400
    );
    let mut bad_side = draft(1, "x");
    bad_side.side = "left".into();
    match w
        .review
        .send_comments(&actor, "r1", vec![draft(1, "ok"), bad_side])
        .await
    {
        Err(Failure::Json(400, body)) => {
            assert_eq!(
                body["error"],
                "comment #2: side must be `old` or `new`, got \"left\""
            )
        }
        other => panic!("{other:?}"),
    }
    let mut unknown_ref = draft(1, "x");
    unknown_ref.from = Some("node:ghost:1:before".into());
    assert_eq!(
        status(
            w.review
                .send_comments(&actor, "r1", vec![unknown_ref])
                .await
        ),
        400
    );
    assert_eq!(
        status(
            w.review
                .send_comments(&actor, "r1", vec![draft(0, "x")])
                .await
        ),
        400
    );

    *w.runs.manager_refusal.lock().unwrap() = Some((409, json!({ "error": "manager_disabled" })));
    match w
        .review
        .send_comments(&actor, "r1", vec![draft(1, "x")])
        .await
    {
        Err(Failure::Json(409, body)) => assert_eq!(body["error"], "manager_disabled"),
        other => panic!("{other:?}"),
    }

    w.git.refs.lock().unwrap().remove("pdo/run-r1");
    match w
        .review
        .send_comments(&actor, "r1", vec![draft(1, "x")])
        .await
    {
        Err(Failure::Json(409, body)) => {
            assert_eq!(body["error"], "run_branch_gone");
            assert_eq!(body["recoverable"], false);
        }
        other => panic!("{other:?}"),
    }
    assert!(!w.runs.kinds("r1").contains(&EventKind::ReviewCommentSent));
    assert!(w.runs.launched_with.lock().unwrap().is_empty());
}

#[tokio::test]
async fn an_agent_reply_proposes_and_the_human_decides() {
    let w = with_sent(1).await;
    let reply = |text: &str, resolved: bool| ReplyCommand {
        text: text.into(),
        resolved,
        author: Some("ignored".into()),
        session_node: None,
    };
    let body = w
        .review
        .reply(&agent("xuTJYLUa"), "r1", "rc-001", reply("Looking.", false))
        .await
        .unwrap();
    assert_eq!(body["outcome"], "replied");
    assert_eq!(body["author"], "xuTJYLUa");

    let body = w
        .review
        .reply(
            &agent(MANAGER_NODE_ID),
            "r1",
            "rc-001",
            reply("Done.", true),
        )
        .await
        .unwrap();
    assert_eq!(body["outcome"], "proposed");
    assert_eq!(body["author"], "manager");
    assert_eq!(body["comment"]["proposal_pending"], true);

    // Reopen declines the proposal; a second reopen changes nothing.
    let actor = Actor::LocalOperator;
    let body = w
        .review
        .decide(&actor, "r1", "rc-001", Decision::Reopen, None, None)
        .await
        .unwrap();
    assert_eq!(body["changed"], true);
    assert_eq!(body["comment"]["reopened_by"], "user");
    let before = w.runs.kinds("r1").len();
    let body = w
        .review
        .decide(&actor, "r1", "rc-001", Decision::Reopen, None, None)
        .await
        .unwrap();
    assert_eq!(body["changed"], false);
    assert_eq!(w.runs.kinds("r1").len(), before, "no event");

    let body = w
        .review
        .decide(
            &actor,
            "r1",
            "rc-001",
            Decision::Resolve,
            Some("lead"),
            None,
        )
        .await
        .unwrap();
    assert_eq!(body["comment"]["status"], "resolved");
    assert_eq!(body["comment"]["resolved_by"], "lead");
}

#[tokio::test]
async fn under_the_setting_an_agent_resolves_directly() {
    let w = with_sent(1).await;
    *w.settings.0.lock().unwrap() = true;
    let body = w
        .review
        .reply(
            &Actor::LocalOperator,
            "r1",
            "rc-001",
            ReplyCommand {
                text: "Fixed.".into(),
                resolved: true,
                author: Some("fixer".into()),
                session_node: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(body["outcome"], "resolved");
    assert_eq!(body["comment"]["resolved_by"], "fixer");
    let kinds = w.runs.kinds("r1");
    assert_eq!(
        kinds[kinds.len() - 2..],
        [
            EventKind::ReviewCommentReplied,
            EventKind::ReviewCommentResolved
        ]
    );
}

#[tokio::test]
async fn a_reply_refuses_an_empty_text_and_an_unknown_comment() {
    let w = with_sent(1).await;
    let actor = Actor::LocalOperator;
    let blank = ReplyCommand {
        text: "  ".into(),
        ..ReplyCommand::default()
    };
    match w.review.reply(&actor, "r1", "rc-001", blank).await {
        Err(Failure::Json(400, body)) => assert_eq!(body["error"], "empty reply text"),
        other => panic!("{other:?}"),
    }
    let text = ReplyCommand {
        text: "x".into(),
        ..ReplyCommand::default()
    };
    match w.review.reply(&actor, "r1", "rc-999", text).await {
        Err(Failure::Json(404, body)) => {
            assert!(body["error"].as_str().unwrap().contains("rc-999"))
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn the_list_filters_by_state_and_maps_onto_a_pair() {
    let w = with_sent(2).await;
    w.review
        .decide(
            &Actor::LocalOperator,
            "r1",
            "rc-002",
            Decision::Resolve,
            None,
            None,
        )
        .await
        .unwrap();
    let list = |state| ListComments {
        state,
        ..ListComments::default()
    };
    let open = w
        .review
        .list_comments("r1", list(Some("open")))
        .await
        .unwrap();
    assert_eq!(open["comments"].as_array().unwrap().len(), 1);
    assert_eq!(open["agent_can_resolve"], false);
    assert!(open.get("from_sha").is_none(), "no pair, no mapping");
    let all = w.review.list_comments("r1", list(None)).await.unwrap();
    assert_eq!(all["comments"].as_array().unwrap().len(), 2);
    assert!(matches!(
        w.review.list_comments("r1", list(Some("nope"))).await,
        Err(Failure::Json(400, _))
    ));

    // Same SHAs as written: reported in place, without asking git.
    let mapped = w
        .review
        .list_comments(
            "r1",
            ListComments {
                state: None,
                from: Some("fork"),
                to: Some("tip"),
            },
        )
        .await
        .unwrap();
    assert_eq!(mapped["to_sha"], "a1a1a1a1a1");
    assert_eq!(mapped["comments"][0]["outdated"], false);
    assert_eq!(mapped["comments"][0]["mapped_line"], 1);
    assert!(!w
        .git
        .calls
        .lock()
        .unwrap()
        .iter()
        .any(|c| c.starts_with("path")));

    // The tip moved: the anchor is followed through the path's diff.
    w.git.with_ref("pdo/run-r1", "b2b2b2b2b2");
    let mapped = w
        .review
        .list_comments(
            "r1",
            ListComments {
                state: None,
                from: None,
                to: Some("tip"),
            },
        )
        .await
        .unwrap();
    assert_eq!(mapped["comments"][1]["mapped_line"], 2);
    assert!(w
        .git
        .calls
        .lock()
        .unwrap()
        .contains(&"path a1a1a1a1a1 b2b2b2b2b2 src/a.rs".to_string()));
}
