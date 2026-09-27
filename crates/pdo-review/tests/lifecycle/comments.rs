//! The review comments' routes over the run context's real log: the send's
//! refusals, the list, the replies, the resolutions and the reopenings (moved
//! from the legacy crate's in-crate tests, #926).

use axum::body::Body;
use axum::http::{Request, StatusCode};
use pdo_review::domain::comments::MANAGER_NODE_ID;
use tower::ServiceExt;

use crate::harness::{build_router, event_log, load_events, test_state};
use crate::helpers::{review_get, review_post, seed_completed_run, seed_sent_comment};

#[tokio::test]
async fn send_review_comments_refuses_when_the_run_branch_is_gone() {
    // #750: the seeded run never had a `pdo/run-<id>` branch in this
    // checkout (nor an archived Run has one) — sending answers 409
    // `run_branch_gone` with the reason the UI prints on its disabled
    // buttons, and NO event is appended: drafts stay drafts.
    let state = test_state().await;
    let run_id = "review-send-branch-gone";
    seed_completed_run(&state, run_id).await;

    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{run_id}/review/comments/send"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "comments": [{ "path": "src/a.rs", "side": "new", "line": 3, "text": "why?" }]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(body["error"], "run_branch_gone");
    assert!(
        body["message"]
            .as_str()
            .unwrap()
            .contains(&format!("pdo/run-{run_id} no longer exists")),
        "got {body}"
    );
    let events = load_events(&state.db, run_id).await.unwrap();
    assert!(
        !events
            .iter()
            .any(|e| e.kind == event_log::EventKind::ReviewCommentSent),
        "no comment recorded on a refused send"
    );
}

#[tokio::test]
async fn send_review_comments_validates_the_batch_and_lists_nothing_by_default() {
    // #750: an empty batch and an unknown run are refused before any git or
    // tmux work; the list endpoint of a never-reviewed Run is `[]`.
    let state = test_state().await;
    let run_id = "review-send-validation";
    seed_completed_run(&state, run_id).await;

    let app = build_router(state.clone());
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{run_id}/review/comments/send"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "comments": [] }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/runs/no-such-run/review/comments/send")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "comments": [{ "path": "a", "side": "new", "line": 1, "text": "t" }] })
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/runs/{run_id}/review/comments"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(body["comments"], serde_json::json!([]));
}

#[tokio::test]
async fn review_reply_from_the_manager_session_is_a_proposal_the_human_resolves_or_reopens() {
    // #751, ADR-0067 §4 — setting off (default): `resolved: true` PROPOSES.
    // Author from the session headers (`__manager__` ⇒ `manager`); every
    // step is an event; resolve / reopen are the human's verbs; a reopen on a
    // proposal declines it and keeps the comment open.
    let state = test_state().await;
    let run_id = "review-reply-proposal";
    seed_completed_run(&state, run_id).await;
    seed_sent_comment(&state, run_id, "rc-001").await;
    let app = build_router(state.clone());
    let base = format!("/runs/{run_id}/review/comments");

    // Plain reply from a node session: author = node id, still open.
    let (status, body) = review_post(
        &app,
        &format!("{base}/rc-001/reply"),
        serde_json::json!({ "text": "Looking into it." }),
        Some((run_id, "xuTJYLUa")),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["outcome"], "replied");
    assert_eq!(body["author"], "xuTJYLUa");
    assert_eq!(body["comment"]["replies"][0]["author"], "xuTJYLUa");
    assert!(body["comment"].get("proposal_pending").is_none());

    // `--resolved` from the manager session: a proposal.
    let (status, body) = review_post(
        &app,
        &format!("{base}/rc-001/reply"),
        serde_json::json!({ "text": "Renamed in 7f2b0d1.", "resolved": true }),
        Some((run_id, MANAGER_NODE_ID)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["outcome"], "proposed");
    assert_eq!(body["author"], "manager");
    assert_eq!(body["comment"]["status"], "sent");
    assert_eq!(body["comment"]["proposal_pending"], true);
    assert_eq!(body["comment"]["replies"][1]["proposes_resolution"], true);

    // The list filters by state and says what `--resolved` does.
    let (status, list) = review_get(&app, &format!("{base}?state=open")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["comments"].as_array().unwrap().len(), 1);
    assert_eq!(list["agent_can_resolve"], false);
    let (_, none) = review_get(&app, &format!("{base}?state=resolved")).await;
    assert_eq!(none["comments"], serde_json::json!([]));
    let (status, _) = review_get(&app, &format!("{base}?state=nope")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Reopen on the proposal = declined, still open, recorded.
    let (status, body) = review_post(
        &app,
        &format!("{base}/rc-001/reopen"),
        serde_json::json!({}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["changed"], true);
    assert_eq!(body["comment"]["status"], "sent");
    assert!(body["comment"].get("proposal_pending").is_none());
    assert_eq!(body["comment"]["proposal_declined"], true);
    assert_eq!(body["comment"]["reopened_by"], "user");
    // Nothing pending any more: a second reopen changes nothing, no event.
    let (status, body) = review_post(
        &app,
        &format!("{base}/rc-001/reopen"),
        serde_json::json!({}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["changed"], false);

    // Resolve by the human, then reopen: back to sent, resolver cleared.
    let (status, body) = review_post(
        &app,
        &format!("{base}/rc-001/resolve"),
        serde_json::json!({}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["comment"]["status"], "resolved");
    assert_eq!(body["comment"]["resolved_by"], "user");
    let (_, again) = review_post(
        &app,
        &format!("{base}/rc-001/resolve"),
        serde_json::json!({}),
        None,
    )
    .await;
    assert_eq!(again["changed"], false, "already resolved is not an error");
    let (_, list) = review_get(&app, &format!("{base}?state=resolved")).await;
    assert_eq!(list["comments"].as_array().unwrap().len(), 1);
    let (status, body) = review_post(
        &app,
        &format!("{base}/rc-001/reopen"),
        serde_json::json!({}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["comment"]["status"], "sent");
    assert!(body["comment"].get("resolved_by").is_none());
    assert!(body["comment"].get("proposal_declined").is_none());

    // Refusals: unknown id, empty text.
    let (status, body) = review_post(
        &app,
        &format!("{base}/rc-999/reply"),
        serde_json::json!({ "text": "x" }),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body["error"].as_str().unwrap().contains("rc-999"));
    let (status, _) = review_post(
        &app,
        &format!("{base}/rc-001/reply"),
        serde_json::json!({ "text": "   " }),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Everything above is in the log, immutable and timestamped.
    let events = load_events(&state.db, run_id).await.unwrap();
    let kinds: Vec<String> = events
        .iter()
        .filter(|e| {
            matches!(
                e.kind,
                event_log::EventKind::ReviewCommentReplied
                    | event_log::EventKind::ReviewCommentResolved
                    | event_log::EventKind::ReviewCommentReopened
            )
        })
        .map(|e| format!("{:?}", e.kind))
        .collect();
    assert_eq!(
        kinds,
        vec![
            "ReviewCommentReplied",
            "ReviewCommentReplied",
            "ReviewCommentReopened",
            "ReviewCommentResolved",
            "ReviewCommentReopened"
        ]
    );
}

#[tokio::test]
async fn review_reply_resolves_directly_when_the_instance_setting_is_on() {
    // #751 AC: setting on ⇒ `--resolved` resolves on the spot, by the agent; the
    // human keeps Reopen. Without session headers the declared author is taken.
    let state = test_state().await;
    state.settings.set(true);
    let run_id = "review-reply-direct";
    seed_completed_run(&state, run_id).await;
    seed_sent_comment(&state, run_id, "rc-001").await;
    let app = build_router(state.clone());
    let base = format!("/runs/{run_id}/review/comments");

    let (_, list) = review_get(&app, &base).await;
    assert_eq!(list["agent_can_resolve"], true);

    let (status, body) = review_post(
        &app,
        &format!("{base}/rc-001/reply"),
        serde_json::json!({ "text": "Fixed.", "resolved": true, "author": "fixer" }),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["outcome"], "resolved");
    assert_eq!(body["author"], "fixer");
    assert_eq!(body["comment"]["status"], "resolved");
    assert_eq!(body["comment"]["resolved_by"], "fixer");
    assert!(body["comment"].get("proposal_pending").is_none());

    // A reply on a resolved comment is just a reply, `resolved` or not.
    let (status, body) = review_post(
        &app,
        &format!("{base}/rc-001/reply"),
        serde_json::json!({ "text": "Also added a test.", "resolved": true }),
        Some((run_id, MANAGER_NODE_ID)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["outcome"], "replied");
    assert_eq!(body["comment"]["replies"].as_array().unwrap().len(), 2);
    assert!(body["comment"]["replies"][1]
        .get("proposes_resolution")
        .is_none());

    // The human keeps the last word.
    let (status, body) = review_post(
        &app,
        &format!("{base}/rc-001/reopen"),
        serde_json::json!({}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["comment"]["status"], "sent");
    assert_eq!(body["comment"]["reopened_by"], "user");
}
