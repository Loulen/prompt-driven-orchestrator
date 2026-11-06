//! Layer 3a — `pdo review` through the real binary, against a real daemon: the
//! Run's diff read, a comment written on a line of it and sent (the Review
//! page lists it, the manager is started for it), resolved and reopened; `4`
//! for a Run or a comment the daemon does not know.

use crate::cli_operator::pdo;
use crate::common::TestDaemon;
use crate::review_comments::{create_run, git, seed};

/// The review fixture, plus a file long enough that a change at its end
/// leaves its first lines out of the hunk.
fn seed_long(repo: &std::path::Path) -> anyhow::Result<()> {
    seed(repo)?;
    let lines: String = (1..=12).map(|n| format!("line {n}\n")).collect();
    std::fs::write(repo.join("long.md"), lines)?;
    git(repo, &["add", "long.md"]);
    git(repo, &["commit", "-q", "-m", "long"]);
    Ok(())
}

#[tokio::test]
async fn a_comment_is_written_sent_resolved_and_reopened_by_the_cli() {
    let daemon = TestDaemon::spawn(seed_long).await.unwrap();
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

    // Work not committed yet is in the patch as in `--stat`, as the Review
    // page shows it.
    std::fs::write(wt_dir.join("notes.md"), "draft\n").unwrap();
    let stat = pdo(&daemon, &["review", "diff", "--stat", "--run", &run_id]).await;
    assert!(
        stat.ok().contains("\t+1\t-0\tnotes.md\n"),
        "{}",
        stat.stdout
    );
    let patch = pdo(&daemon, &["review", "diff", "--run", &run_id]).await;
    assert!(patch.ok().contains("+draft\n"), "{}", patch.stdout);
    // Git's own patch holds the committed work only.
    let committed = pdo(&daemon, &["review", "diff", "--git", "--run", &run_id]).await;
    let committed = committed.ok();
    assert!(committed.contains("+fn d() {}"), "{committed}");
    assert!(!committed.contains("draft"), "{committed}");

    // The page comments only a line its diff shows: a line past the file, or
    // a file outside the diff, is refused and nothing is sent.
    for (path, line) in [("lib.rs", "99"), ("nope.md", "1")] {
        let off = pdo(
            &daemon,
            &[
                "review", "comment", path, line, "--text", "Here?", "--run", &run_id,
            ],
        )
        .await;
        assert_eq!(off.code, Some(1), "{}", off.stderr);
        assert!(
            off.stderr.contains("is not a line of the Run's diff"),
            "{}",
            off.stderr
        );
    }
    let none = daemon
        .get_json(&format!("/runs/{run_id}/review/comments?state=all"))
        .await;
    assert_eq!(none["comments"], serde_json::json!([]), "{none}");

    // A line the page's context expanders reveal is one it lets one comment:
    // line 1 of a file changed at its end, read in the working tree.
    let mut long: String = (1..=11).map(|n| format!("line {n}\n")).collect();
    long.push_str("line twelve\n");
    std::fs::write(wt_dir.join("long.md"), long).unwrap();
    let revealed = pdo(
        &daemon,
        &[
            "review", "comment", "long.md", "1", "--text", "Title?", "--run", &run_id,
        ],
    )
    .await;
    assert_eq!(
        revealed.ok(),
        format!("rc-001 sent · long.md:1 · run {run_id}\n")
    );
    let past = pdo(
        &daemon,
        &[
            "review", "comment", "long.md", "13", "--text", "Here?", "--run", &run_id,
        ],
    )
    .await;
    assert_eq!(past.code, Some(1), "{}", past.stderr);

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
        format!("rc-002 sent · lib.rs:4 · run {run_id}\n")
    );
    let listed = daemon
        .get_json(&format!("/runs/{run_id}/review/comments?state=all"))
        .await;
    let comment = listed["comments"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "rc-002")
        .unwrap_or_else(|| panic!("rc-002 is listed: {listed}"));
    assert_eq!(comment["path"], "lib.rs", "{listed}");
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
