//! Layer 3a — `pdo review` through the real binary, against a real daemon: the
//! Run's diff read, a comment written on a line of it and sent (the Review
//! page lists it, the manager is started for it), resolved and reopened; `4`
//! for a Run or a comment the daemon does not know.

use crate::cli_operator::pdo;
use crate::common::TestDaemon;
use crate::review_comments::{create_run, git, seed};

#[tokio::test]
async fn a_comment_is_written_sent_resolved_and_reopened_by_the_cli() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(daemon.url(), daemon.target_repo())
        .await
        .expect("run created");
    let wt_dir = daemon
        .repo_root()
        .join(".pdo/runs")
        .join(&run_id)
        .join("worktree");
    for _ in 0..100 {
        if wt_dir.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    std::fs::write(
        wt_dir.join("lib.rs"),
        "fn a() {}\nfn b() {}\nfn c() {}\nfn d() {}\n",
    )
    .unwrap();
    git(&wt_dir, &["add", "lib.rs"]);
    git(&wt_dir, &["commit", "-q", "-m", "run work"]);

    let stat = pdo(&daemon, &["review", "diff", "--stat", "--run", &run_id]).await;
    assert_eq!(stat.ok(), "modified\t+1\t-0\tlib.rs\n");
    let patch = pdo(&daemon, &["review", "diff", "--run", &run_id]).await;
    assert!(patch.ok().contains("+fn d() {}"), "{}", patch.stdout);

    let sent = pdo(
        &daemon,
        &[
            "review",
            "comment",
            "lib.rs",
            "4",
            "--text",
            "Name this `fn delta`.",
            "--run",
            &run_id,
        ],
    )
    .await;
    assert_eq!(
        sent.ok(),
        format!("rc-001 sent · lib.rs:4 · run {run_id}\n")
    );
    let listed = daemon
        .get_json(&format!("/runs/{run_id}/review/comments?state=all"))
        .await;
    let comment = &listed["comments"][0];
    assert_eq!(comment["id"], "rc-001", "{listed}");
    assert_eq!(comment["text"], "Name this `fn delta`.", "{listed}");

    let empty = pdo(
        &daemon,
        &[
            "review", "comment", "lib.rs", "4", "--text", " ", "--run", &run_id,
        ],
    )
    .await;
    assert_eq!(empty.code, Some(1), "{}", empty.stderr);

    assert_eq!(
        pdo(&daemon, &["review", "resolve", "rc-001", "--run", &run_id])
            .await
            .ok(),
        "rc-001 resolved\n"
    );
    let resolved = daemon
        .get_json(&format!("/runs/{run_id}/review/comments?state=resolved"))
        .await;
    assert_eq!(resolved["comments"][0]["id"], "rc-001", "{resolved}");
    assert_eq!(
        pdo(&daemon, &["review", "reopen", "rc-001", "--run", &run_id])
            .await
            .ok(),
        "rc-001 reopened\n"
    );

    let unknown = pdo(&daemon, &["review", "resolve", "rc-999", "--run", &run_id]).await;
    assert_eq!(unknown.code, Some(4), "{}", unknown.stderr);
    let no_run = pdo(
        &daemon,
        &["review", "diff", "--stat", "--run", "no-such-run"],
    )
    .await;
    assert_eq!(no_run.code, Some(4), "{}", no_run.stderr);
}
