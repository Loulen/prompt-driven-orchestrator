//! Layer 3 (#672, ADR-0062) — skills **delivered in the worktree, never
//! committed**, end to end through the daemon.
//!
//! A real daemon over a tempdir repo; a skill selected at the instance tier and
//! another on the node; a Run whose repo already versions its own
//! `.agents/skills/x`. The assertions are the Feature Path's: what the frozen
//! events say, what the worktree contains, what `git status` and the Run branch
//! show, and what `info/exclude` holds before and after cleanup.

use std::path::{Path, PathBuf};

use crate::common::TestDaemon;

const PIPELINE_NAME: &str = "skill-delivery-test";

fn pipeline_yaml(node_skills: &str) -> String {
    format!(
        r#"name: {PIPELINE_NAME}
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: aaaaaaaa
    name: worker
    type: agent
    isolated_worktree: false
{node_skills}    outputs:
      - name: out
    view: {{ x: 200, y: 60 }}
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: {{ node: start, port: user_prompt }}
    target: {{ node: aaaaaaaa, port: task }}
  - source: {{ node: aaaaaaaa, port: out }}
    target: {{ node: end, port: result }}
"#
    )
}

fn git(dir: &Path, args: &[&str]) -> String {
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
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// A repo with one commit that already versions `.agents/skills/x/SKILL.md`.
fn seed(repo: &Path) -> anyhow::Result<()> {
    let pipelines_dir = repo.join(".pdo").join("pipelines");
    std::fs::create_dir_all(&pipelines_dir)?;
    std::fs::write(
        pipelines_dir.join(format!("{PIPELINE_NAME}.yaml")),
        pipeline_yaml(""),
    )?;
    let prompts_dir = pipelines_dir.join(format!("{PIPELINE_NAME}.prompts"));
    std::fs::create_dir_all(&prompts_dir)?;
    std::fs::write(prompts_dir.join("aaaaaaaa.md"), "You are the worker.\n")?;
    std::fs::create_dir_all(repo.join(".agents/skills/x"))?;
    std::fs::write(
        repo.join(".agents/skills/x/SKILL.md"),
        "---\nname: x\ndescription: the repo's own\n---\n",
    )?;
    git(repo, &["init", "-q", "-b", "main"]);
    git(repo, &["config", "user.email", "test@example.com"]);
    git(repo, &["config", "user.name", "Test"]);
    git(repo, &["config", "commit.gpgsign", "false"]);
    std::fs::write(repo.join(".gitignore"), ".pdo/runs/\n")?;
    git(repo, &["add", "."]);
    git(repo, &["commit", "-q", "-m", "init"]);
    Ok(())
}

fn skill_md(name: &str, description: &str) -> String {
    format!("---\nname: {name}\ndescription: {description}\n---\n\n# {name}\n")
}

async fn create_skill(daemon: &TestDaemon, name: &str) -> String {
    let resp = reqwest::Client::new()
        .post(format!("{}/settings/skills", daemon.url()))
        .json(&serde_json::json!({ "content": skill_md(name, "a skill for the delivery test") }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        201,
        "POST /settings/skills should return 201"
    );
    let json: serde_json::Value = resp.json().await.unwrap();
    json["id"].as_str().unwrap().to_string()
}

async fn select_at_instance(daemon: &TestDaemon, skills: serde_json::Value) {
    let resp = reqwest::Client::new()
        .put(format!("{}/settings", daemon.url()))
        .json(&serde_json::json!({ "skills": skills }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "PUT /settings should return 200");
}

async fn create_run(daemon: &TestDaemon, body: serde_json::Value) -> String {
    let resp = reqwest::Client::new()
        .post(format!("{}/runs", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "POST /runs should return 201");
    let json: serde_json::Value = resp.json().await.unwrap();
    json["run_id"].as_str().unwrap().to_string()
}

async fn events(daemon: &TestDaemon, run_id: &str) -> Vec<serde_json::Value> {
    let v: serde_json::Value = reqwest::get(format!("{}/runs/{run_id}/events", daemon.url()))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    v.as_array().cloned().unwrap_or_default()
}

fn find<'a>(evs: &'a [serde_json::Value], kind: &str) -> Option<&'a serde_json::Value> {
    evs.iter().rev().find(|e| e["kind"] == kind)
}

async fn wait_for_node_started(daemon: &TestDaemon, run_id: &str) -> serde_json::Value {
    for _ in 0..100 {
        let evs = events(daemon, run_id).await;
        if let Some(e) = evs
            .iter()
            .rev()
            .find(|e| e["kind"] == "node_started" && e["node_id"] == "aaaaaaaa")
        {
            return e.clone();
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("worker node should have started within the timeout");
}

async fn cleanup(daemon: &TestDaemon, run_id: &str) {
    let resp = reqwest::Client::new()
        .post(format!("{}/runs/{run_id}/commands", daemon.url()))
        .json(&serde_json::json!({ "kind": "cleanup_run" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "cleanup_run should return 200");
}

fn worktree_of(daemon: &TestDaemon, run_id: &str) -> PathBuf {
    daemon
        .repo_root()
        .join(".pdo")
        .join("runs")
        .join(run_id)
        .join("worktree")
}

/// `git status --porcelain` minus PDO's own runtime paths (`.pdo/artifacts`,
/// `.pdo/prompts`), which the target repo's `.gitignore` owns (ADR-0060) — the seed
/// here ignores only `.pdo/runs/`.
fn status_outside_pdo(worktree: &Path) -> String {
    git(worktree, &["status", "--porcelain"])
        .lines()
        .filter(|l| !l.contains(".pdo/"))
        .map(|l| format!("{l}\n"))
        .collect()
}

fn exclude_file(repo: &Path) -> PathBuf {
    repo.join(".git").join("info").join("exclude")
}

fn pdo_exclude_lines(repo: &Path) -> Vec<String> {
    std::fs::read_to_string(exclude_file(repo))
        .unwrap_or_default()
        .lines()
        .filter(|l| l.starts_with("# pdo"))
        .map(String::from)
        .collect()
}

/// FP steps 1-5: instance skill + node skill, versioned homonym, frozen events,
/// clean status, no skill on the Run branch, exclusions gone after cleanup.
#[tokio::test]
async fn skills_are_delivered_in_the_worktree_frozen_and_never_committed() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let repo = daemon.repo_root().to_path_buf();
    // A user line the cleanup must leave alone.
    std::fs::create_dir_all(repo.join(".git/info")).unwrap();
    std::fs::write(exclude_file(&repo), "# mine\n*.swp\n").unwrap();

    let tdd = create_skill(&daemon, "tdd").await;
    let grilling = create_skill(&daemon, "grilling").await;
    let homonym = create_skill(&daemon, "x").await;
    select_at_instance(
        &daemon,
        serde_json::json!([{ "id": tdd, "name": "tdd" }, { "id": homonym, "name": "x" }]),
    )
    .await;

    // The node selects `grilling` — its own tier, read at spawn.
    let node_skills = format!("    skills:\n      - id: {grilling}\n        name: grilling\n");
    std::fs::write(
        repo.join(".pdo/pipelines")
            .join(format!("{PIPELINE_NAME}.yaml")),
        pipeline_yaml(&node_skills),
    )
    .unwrap();

    let run_id = create_run(
        &daemon,
        serde_json::json!({
            "pipeline": PIPELINE_NAME,
            "input": "go",
            "target_repo": daemon.target_repo(),
        }),
    )
    .await;
    let worktree = worktree_of(&daemon, &run_id);

    // RunStarted freezes the instance + Projet + Run selection, with the homonym
    // reported as skipped at the Run level (delivered into the Run worktree).
    let evs = events(&daemon, &run_id).await;
    let run_started = find(&evs, "run_started").expect("run_started");
    let frozen = run_started["payload"]["frozen_skills"]
        .as_array()
        .expect("frozen_skills on RunStarted");
    assert_eq!(frozen.len(), 2);
    assert_eq!(frozen[0]["name"], "tdd");
    assert_eq!(frozen[0]["tiers"], serde_json::json!(["instance"]));
    let skipped = run_started["payload"]["skipped_skills"]
        .as_array()
        .expect("skipped_skills on RunStarted");
    assert_eq!(skipped.len(), 1);
    assert_eq!(skipped[0]["name"], "x");

    // The node spawns: NodeStarted freezes the union and the homonym warning.
    let node_started = wait_for_node_started(&daemon, &run_id).await;
    let names: Vec<&str> = node_started["payload"]["skills"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["tdd", "x", "grilling"]);
    let node_skipped = node_started["payload"]["skipped_skills"]
        .as_array()
        .unwrap();
    assert_eq!(node_skipped.len(), 1);
    assert_eq!(node_skipped[0]["name"], "x");
    assert!(node_skipped[0]["reason"]
        .as_str()
        .unwrap()
        .contains("versioned"));

    // FP 3: the skills are on disk, in both locations; the repo's own `x` is intact
    // and tracked; `git status` is clean.
    assert!(worktree.join(".agents/skills/tdd/SKILL.md").is_file());
    assert!(worktree.join(".agents/skills/grilling/SKILL.md").is_file());
    assert!(worktree.join(".claude/skills/tdd/SKILL.md").is_file());
    assert!(worktree.join(".claude/skills/grilling/SKILL.md").is_file());
    assert!(
        std::fs::symlink_metadata(worktree.join(".claude/skills/tdd"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        std::fs::read_to_string(worktree.join(".agents/skills/x/SKILL.md")).unwrap(),
        "---\nname: x\ndescription: the repo's own\n---\n"
    );
    assert!(!worktree.join(".claude/skills/x").exists());
    assert_eq!(
        git(&worktree, &["ls-files", ".agents/skills/x"]).trim(),
        ".agents/skills/x/SKILL.md"
    );
    assert_eq!(status_outside_pdo(&worktree), "");

    // Content frozen at the Run: editing the bank now changes nothing delivered,
    // even to a node spawned later.
    let resp = reqwest::Client::new()
        .put(format!(
            "{}/settings/skills/{tdd}/files/SKILL.md",
            daemon.url()
        ))
        .body(skill_md("tdd", "EDITED after the Run started"))
        .send()
        .await
        .unwrap();
    assert!(
        resp.status().is_success(),
        "PUT SKILL.md: {}",
        resp.status()
    );
    assert!(
        std::fs::read_to_string(worktree.join(".agents/skills/tdd/SKILL.md"))
            .unwrap()
            .contains("a skill for the delivery test")
    );
    assert!(
        std::fs::read_to_string(
            repo.join(".pdo/runs")
                .join(&run_id)
                .join("skills")
                .join(&tdd)
                .join("SKILL.md")
        )
        .unwrap()
        .contains("a skill for the delivery test"),
        "the Run snapshot is the frozen content"
    );

    // FP 4: even `git add -A && git commit` by the agent takes no skill along, and
    // the Run's diff against its fork point shows none.
    std::fs::write(worktree.join("work.txt"), "agent work\n").unwrap();
    git(&worktree, &["add", "-A"]);
    git(&worktree, &["commit", "-q", "-m", "agent commit"]);
    let committed = git(&worktree, &["diff", "--name-only", "main...HEAD"]);
    let committed: Vec<String> = committed
        .lines()
        .filter(|l| !l.starts_with(".pdo/"))
        .map(String::from)
        .collect();
    assert_eq!(committed, vec!["work.txt".to_string()]);
    assert_eq!(status_outside_pdo(&worktree), "");

    // The exclusions are per skill, never a parent folder, marked `# pdo <run-id>`.
    let exclude = std::fs::read_to_string(exclude_file(&repo)).unwrap();
    assert!(exclude.contains("/.agents/skills/tdd/\n"));
    assert!(exclude.contains("/.claude/skills/grilling\n"));
    assert!(!exclude.contains("/.agents/skills/\n"));
    assert!(!exclude.contains("/.agents/skills/x"));
    assert!(pdo_exclude_lines(&repo)
        .iter()
        .all(|l| l == &format!("# pdo {run_id}")));

    // FP 5: cleanup removes every `# pdo` line and nothing else.
    cleanup(&daemon, &run_id).await;
    assert_eq!(
        std::fs::read_to_string(exclude_file(&repo)).unwrap(),
        "# mine\n*.swp\n"
    );
}

/// Changing a node's selection after the Run started reaches a node not yet
/// launched (additive snapshot), while the instance tier stays as frozen.
#[tokio::test]
async fn a_selection_changed_before_spawn_is_snapshotted_additively() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();

    let tdd = create_skill(&daemon, "tdd").await;
    select_at_instance(&daemon, serde_json::json!([{ "id": tdd, "name": "tdd" }])).await;

    let run_id = create_run(
        &daemon,
        serde_json::json!({
            "pipeline": PIPELINE_NAME,
            "input": "go",
            "target_repo": daemon.target_repo(),
        }),
    )
    .await;
    let worktree = worktree_of(&daemon, &run_id);
    assert!(worktree.join(".agents/skills/tdd/SKILL.md").is_file());

    // After the Run started the instance tier is cleared: the Run keeps what it
    // froze (the additive node-tier snapshot is covered by the module's unit test —
    // the spawn here races the edit, so it is not asserted end to end).
    select_at_instance(&daemon, serde_json::json!([])).await;

    let node_started = wait_for_node_started(&daemon, &run_id).await;
    let names: Vec<&str> = node_started["payload"]["skills"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        vec!["tdd"],
        "the frozen instance tier still delivers tdd"
    );
    assert!(worktree.join(".agents/skills/tdd/SKILL.md").is_file());
    assert_eq!(status_outside_pdo(&worktree), "");
}
