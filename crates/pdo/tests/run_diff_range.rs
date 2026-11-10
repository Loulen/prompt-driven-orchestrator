//! Layer 3a — the aggregate Run diff endpoint over a real daemon (#376).
//!
//! Closes the AC for the backend slice (ADR-0004 golden rule: no AC closed
//! without a test at layer ≥ 3). Proves, end-to-end against a booted daemon,
//! that `GET /runs/<id>/diff`:
//!   - shows the run branch's own change,
//!   - is a **three-dot** range (main's advance *after* the fork does not
//!     surface as a phantom deletion), and
//!   - excludes the `.pdo/` blackboard.
//!
//! The entry node runs under the harness `exec sleep 600` override, so it never
//! calls `pdo complete` → the run branch is never merged and stays a clean fork
//! of the seed commit for the duration.

use crate::common::TestDaemon;

const PIPELINE_NAME: &str = "diff-solo";
const PIPELINE_YAML: &str = r#"name: diff-solo
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: solo
    name: solo
    type: agent
    isolated_worktree: false
    inputs:
      - name: in
    outputs:
      - name: out
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: solo, port: in }
"#;

fn seed(repo: &std::path::Path) -> anyhow::Result<()> {
    let pipelines_dir = repo.join(".pdo").join("pipelines");
    std::fs::create_dir_all(&pipelines_dir)?;
    std::fs::write(
        pipelines_dir.join(format!("{PIPELINE_NAME}.yaml")),
        PIPELINE_YAML,
    )?;
    git_init_with_commit(repo)?;
    Ok(())
}

fn git_init_with_commit(repo: &std::path::Path) -> anyhow::Result<()> {
    let run = |args: &[&str]| -> anyhow::Result<()> {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()?;
        if !out.status.success() {
            anyhow::bail!(
                "git {:?} failed: {}",
                args,
                String::from_utf8_lossy(&out.stderr)
            );
        }
        Ok(())
    };
    run(&["init", "-q", "-b", "main"])?;
    run(&["config", "user.email", "test@example.com"])?;
    run(&["config", "user.name", "Test"])?;
    run(&["config", "commit.gpgsign", "false"])?;
    std::fs::write(repo.join(".gitignore"), ".pdo/runs/\n")?;
    run(&["add", "."])?;
    run(&["commit", "-q", "-m", "init"])?;
    Ok(())
}

async fn create_run(daemon_url: String, target_repo: String) -> Option<String> {
    create_run_on_branch(daemon_url, target_repo, None).await
}

async fn create_run_on_branch(
    daemon_url: String,
    target_repo: String,
    source_branch: Option<&str>,
) -> Option<String> {
    // #470: the target repo is required at the create boundary (ADR-0033).
    let mut body = serde_json::json!({
        "pipeline": PIPELINE_NAME,
        "input": "go",
        "target_repo": target_repo,
    });
    // #417: naming the source branch makes the daemon freeze `fork_sha` from that exact
    // local ref at creation — the fork point the diff base must anchor on.
    if let Some(branch) = source_branch {
        body["source_branch"] = serde_json::json!(branch);
    }
    let resp = reqwest::Client::new()
        .post(format!("{daemon_url}/runs"))
        .json(&body)
        .send()
        .await
        .ok()?;
    let json: serde_json::Value = resp.json().await.ok()?;
    json["run_id"].as_str().map(String::from)
}

#[tokio::test]
async fn run_diff_uses_three_dot_and_excludes_pdo_over_real_daemon() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let repo = daemon.repo_root().to_path_buf();
    let run_id = create_run(daemon.url(), daemon.target_repo())
        .await
        .expect("run created");

    // Wait for the daemon to create the run branch + pipeline worktree
    // (<repo>/.pdo/runs/<run-id>/worktree/, see CONTEXT.md § worktree).
    let wt_dir = repo.join(".pdo/runs").join(&run_id).join("worktree");
    for _ in 0..100 {
        if wt_dir.join(".git").exists() || wt_dir.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(
        wt_dir.exists(),
        "pipeline worktree should exist for {run_id}"
    );

    let git = |dir: &std::path::Path, args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap()
    };
    // Real change on the run branch (via the daemon-created pipeline worktree).
    std::fs::write(wt_dir.join("feature.rs"), "fn feature() {}\n").unwrap();
    std::fs::create_dir_all(wt_dir.join(".pdo")).unwrap();
    std::fs::write(wt_dir.join(".pdo/artifact.txt"), "blackboard\n").unwrap();
    git(&wt_dir, &["add", "feature.rs"]);
    git(&wt_dir, &["add", "-f", ".pdo/artifact.txt"]);
    git(&wt_dir, &["commit", "-m", "run work + artifact"]);

    // Advance main after the fork.
    std::fs::write(repo.join("unrelated.rs"), "fn unrelated() {}\n").unwrap();
    git(&repo, &["add", "unrelated.rs"]);
    git(&repo, &["commit", "-m", "advance main"]);

    let body = reqwest::Client::new()
        .get(format!("{}/runs/{run_id}/diff", daemon.url()))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    assert!(
        body.contains("feature.rs") && body.contains("fn feature()"),
        "real run change present: {body}"
    );
    assert!(
        !body.contains("unrelated.rs"),
        "no phantom deletion of main's advance: {body}"
    );
    assert!(
        !body.contains("artifact.txt"),
        "blackboard excluded: {body}"
    );
}

#[tokio::test]
async fn run_diff_ignores_parked_checkout_over_real_daemon() {
    // #417 (layer-3a): the whole bug, end-to-end. The shared checkout is parked on a
    // branch that diverges BEFORE the Run's fork point, so the pre-fix `HEAD...` range
    // collapses the merge-base onto the common ancestor and sweeps in every commit `main`
    // gained since. With `fork_sha` frozen at creation, `GET /runs/<id>/diff` anchors on
    // the fork point and shows ONLY the Run's own change. RED under the old code.
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let repo = daemon.repo_root().to_path_buf();

    let git = |dir: &std::path::Path, args: &[&str]| {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        out
    };

    // c0 (the seed "init" commit) is the common ancestor. Branch the operator's parked
    // head off it BEFORE advancing main, so its merge-base with the Run is c0.
    let c0 = String::from_utf8_lossy(&git(&repo, &["rev-parse", "HEAD"]).stdout)
        .trim()
        .to_string();
    git(&repo, &["branch", "wip/parked", &c0]);

    // Advance main two commits PAST c0 — the phantom payload the buggy merge-base sweeps in.
    std::fs::write(repo.join("file_a.rs"), "fn a() {}\n").unwrap();
    git(&repo, &["add", "file_a.rs"]);
    git(&repo, &["commit", "-m", "main: +file_a"]);
    std::fs::write(repo.join("file_b.rs"), "fn b() {}\n").unwrap();
    git(&repo, &["add", "file_b.rs"]);
    git(&repo, &["commit", "-m", "main: +file_b"]); // main tip = the Run's fork point

    // Create the Run forked from `main` (freezes fork_sha = current main tip).
    let run_id = create_run_on_branch(daemon.url(), daemon.target_repo(), Some("main"))
        .await
        .expect("run created");

    let wt_dir = repo.join(".pdo/runs").join(&run_id).join("worktree");
    for _ in 0..100 {
        if wt_dir.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(
        wt_dir.exists(),
        "pipeline worktree should exist for {run_id}"
    );

    // The Run's only real change, on the run branch (via the daemon-created worktree).
    std::fs::write(wt_dir.join("run_file.rs"), "fn run_work() {}\n").unwrap();
    git(&wt_dir, &["add", "run_file.rs"]);
    git(&wt_dir, &["commit", "-m", "run work"]);

    // PARK the shared checkout on the divergent branch — the exact #417 / #451 condition.
    git(&repo, &["checkout", "wip/parked"]);

    let body = reqwest::Client::new()
        .get(format!("{}/runs/{run_id}/diff", daemon.url()))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    assert!(
        body.contains("run_file.rs") && body.contains("fn run_work()"),
        "diff must contain the Run's own change: {body}"
    );
    // The phantom under `HEAD...`: main's commits since c0, swept in by the parked HEAD.
    assert!(
        !body.contains("file_a.rs") && !body.contains("file_b.rs"),
        "parked divergent HEAD must not sweep in main's pre-fork files: {body}"
    );
}

/// #748 — the whole bug, end-to-end: a Run whose `target_repo` is NOT the daemon's
/// working directory. The daemon root has no such run branch, so the pre-#748
/// handlers (`git diff` in `state.repo_root`) answered 404 for every such Run —
/// which is every Run of an operator who points PDO at another repository. Both
/// the raw endpoint and the structured one must read the Run's effective repo.
#[tokio::test]
async fn run_diff_and_structured_diff_use_the_effective_repo_over_real_daemon() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();

    // A second, unrelated git repository: the Run's target.
    let target_tmp = tempfile::tempdir().unwrap();
    let target = target_tmp.path().to_path_buf();
    git_init_with_commit(&target).unwrap();
    let run_id = create_run(daemon.url(), target.to_string_lossy().into_owned())
        .await
        .expect("run created");

    // The pipeline worktree is cut in the TARGET repo, not under the daemon root.
    let wt_dir = target.join(".pdo/runs").join(&run_id).join("worktree");
    for _ in 0..100 {
        if wt_dir.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(
        wt_dir.exists(),
        "pipeline worktree should exist in the target repo"
    );
    assert!(
        !daemon
            .repo_root()
            .join(".pdo/runs")
            .join(&run_id)
            .join("worktree")
            .exists(),
        "nothing of this Run is cut under the daemon root"
    );

    let git = |dir: &std::path::Path, args: &[&str]| {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        out
    };
    std::fs::write(wt_dir.join("feature.rs"), "fn feature() {}\nfn more() {}\n").unwrap();
    std::fs::write(wt_dir.join(".gitignore"), ".pdo/runs/\n# tweak\n").unwrap();
    git(&wt_dir, &["add", "-A"]);
    git(
        &wt_dir,
        &["commit", "-q", "-m", "run work in the target repo"],
    );

    // Raw patch.
    let body = reqwest::Client::new()
        .get(format!("{}/runs/{run_id}/diff", daemon.url()))
        .send()
        .await
        .unwrap();
    assert_eq!(
        body.status(),
        200,
        "raw diff must resolve in the target repo"
    );
    let body = body.text().await.unwrap();
    assert!(
        body.contains("feature.rs") && body.contains("fn feature()"),
        "raw diff computed in the target repo: {body}"
    );

    // Structured diff: files + hunks, totals identical to the LOC stat.
    let json: serde_json::Value = reqwest::Client::new()
        .get(format!("{}/runs/{run_id}/diff/structured", daemon.url()))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(json["three_dot"], serde_json::json!(true), "{json}");
    assert_eq!(json["files_changed"], serde_json::json!(2), "{json}");
    let paths: Vec<&str> = json["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["new_path"].as_str().unwrap())
        .collect();
    assert_eq!(paths, vec![".gitignore", "feature.rs"]);
    let feature = &json["files"][1];
    assert_eq!(feature["status"], serde_json::json!("added"));
    assert_eq!(feature["additions"], serde_json::json!(2));
    assert_eq!(
        feature["hunks"][0]["lines"][0]["kind"],
        serde_json::json!("add")
    );
    assert_eq!(
        feature["hunks"][0]["lines"][0]["content"],
        serde_json::json!("fn feature() {}")
    );

    let run: serde_json::Value = reqwest::Client::new()
        .get(format!("{}/runs/{run_id}", daemon.url()))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        run["loc"]["files_changed"], json["files_changed"],
        "counted = shown"
    );
    assert_eq!(run["loc"]["insertions"], json["additions"]);
    assert_eq!(run["loc"]["deletions"], json["deletions"]);

    // File content at the Run tip (default ref).
    let content = reqwest::Client::new()
        .get(format!(
            "{}/runs/{run_id}/file?path=feature.rs",
            daemon.url()
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(content.status(), 200);
    assert_eq!(
        content.text().await.unwrap(),
        "fn feature() {}\nfn more() {}\n"
    );
}
