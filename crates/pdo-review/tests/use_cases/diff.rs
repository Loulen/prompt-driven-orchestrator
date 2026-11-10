//! The raw and the structured diff: which pair, which form, which errors.

use std::path::PathBuf;

use pdo_review::application::failure::Failure;

use crate::fakes::World;

#[tokio::test]
async fn the_raw_diff_is_three_dot_from_the_fork_point_to_the_tip() {
    let w = World::new();
    w.completed_run("r1").await;
    let diff = w.review.run_diff("r1").await.unwrap();
    assert!(diff.starts_with("diff --git"));
    assert_eq!(
        *w.git.calls.lock().unwrap(),
        vec!["raw f0...pdo/run-r1".to_string()]
    );
}

#[tokio::test]
async fn an_unknown_run_is_a_plain_404_on_the_raw_diff_and_a_json_one_on_the_structured() {
    let w = World::new();
    assert!(matches!(
        w.review.run_diff("nope").await,
        Err(Failure::Text(404, body)) if body == "run not found"
    ));
    match w.review.structured_diff("nope", None, None).await {
        Err(Failure::Json(404, body)) => assert_eq!(body["error"], "run not found"),
        other => panic!("{other:?}"),
    }
    *w.runs.unreadable.lock().unwrap() = true;
    match w.review.structured_diff("nope", None, None).await {
        Err(Failure::Json(500, body)) => assert_eq!(body["error"], "error: database is locked"),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn the_default_pair_is_fork_to_tip_three_dot_and_an_explicit_one_two_dot() {
    let w = World::new();
    w.completed_run("r1").await;
    let d = w.review.structured_diff("r1", None, None).await.unwrap();
    assert!(d.three_dot);
    let d = w
        .review
        .structured_diff("r1", Some("fork"), Some("abc123"))
        .await
        .unwrap();
    assert!(!d.three_dot);
    assert_eq!(
        *w.git.calls.lock().unwrap(),
        vec![
            "structured f0 pdo/run-r1 true".to_string(),
            "structured f0 abc123 false".to_string()
        ]
    );
}

#[tokio::test]
async fn the_working_tree_is_the_default_destination_while_it_exists_and_never_a_source() {
    let w = World::new();
    w.completed_run("r1").await;
    *w.runs.worktree.lock().unwrap() = Some(PathBuf::from("/repo/wt"));
    let d = w.review.structured_diff("r1", None, None).await.unwrap();
    assert_eq!(d.to_ref, "worktree");
    assert_eq!(d.to_sha.as_deref(), Some("snap"));
    assert!(matches!(
        w.review.structured_diff("r1", Some("worktree"), None).await,
        Err(Failure::Text(400, _))
    ));
}

#[tokio::test]
async fn a_bad_ref_is_refused_and_a_gone_branch_is_a_404() {
    let w = World::new();
    w.completed_run("r1").await;
    assert!(matches!(
        w.review.structured_diff("r1", Some("--output=/x"), None).await,
        Err(Failure::Text(400, body)) if body.starts_with("invalid ref")
    ));
    match w
        .review
        .structured_diff("r1", Some("node:ghost:1:before"), None)
        .await
    {
        Err(Failure::Json(404, body)) => {
            assert_eq!(body["error"], "unknown run ref: node:ghost:1:before")
        }
        other => panic!("{other:?}"),
    }
    w.git.unknown.lock().unwrap().insert("pdo/run-r1".into());
    assert!(matches!(
        w.review.structured_diff("r1", None, None).await,
        Err(Failure::Text(404, body)) if body == "run branch not found"
    ));
}

/// CORE-231: a raw revision git does not know is the caller's typo, not a gone
/// branch. The 404 names it and the Run ref spellings.
#[tokio::test]
async fn an_unknown_raw_revision_is_named_not_reported_as_a_gone_branch() {
    let w = World::new();
    w.completed_run("r1").await;
    w.git.unknown.lock().unwrap().insert("bogusref".into());
    match w
        .review
        .structured_diff("r1", Some("fork"), Some("bogusref"))
        .await
    {
        Err(Failure::Text(404, body)) => {
            assert!(body.starts_with("unknown ref: bogusref"), "{body}");
            assert!(body.contains("node:<id>:<iter>:before"), "{body}");
        }
        other => panic!("{other:?}"),
    }
}
