//! Layer 3a (pdo-issues#3, #1032, ADR-0081) — the **Coût de l'arbre** of a Run,
//! end to end through a real daemon.
//!
//! Runs are created FROM a live node session (the `X-PDO-Session-*` headers a
//! node is wrapped with, prior art `run_provenance.rs`), so the daemon freezes
//! the mechanical `parent_run_id` link; each Run's worker gets a planted
//! transcript (prior art `stats_cost_by_model.rs`), so its own cost is known.
//!
//! What is proven here:
//!   1. `GET /runs/{id}` carries an additive `tree_cost` — total, `parent`,
//!      `children`, `descendants`, `by_depth` — while `cost` stays the Coût du
//!      Run parent; a Run without descendant carries no `tree_cost`;
//!   2. a two-level tree ventilates by depth, and an archived descendant still
//!      counts;
//!   3. `GET /runs/{id}/children` exposes each child's tree cost,
//!      grandchildren included;
//!   4. a descendant with no cost source makes the total unavailable (slices
//!      readable), a lower-bound descendant makes the tree a lower bound;
//!   5. (#1033) `GET /stats/cost?roll_up_children=true` files the whole tree
//!      under its root's Pipeline — Parent run / Child runs › Depth N — with
//!      the Child runs figure Run info shows, and leaves the default response
//!      and the « By model » axis untouched.

use crate::common::TestDaemon;
use std::process::Command;

fn tmux_available() -> bool {
    Command::new("tmux")
        .arg("-V")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn worker_yaml(name: &str, harness: &str, model: Option<&str>) -> String {
    let harness_block = model
        .map(|m| format!("    harnesses:\n      {harness}:\n        model: {m}\n"))
        .unwrap_or_default();
    format!(
        r#"name: {name}
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: worker
    name: Worker
    type: agent
    isolated_worktree: false
    pin_harness: {harness}
{harness_block}    inputs:
      - {{ name: task, side: top }}
    outputs:
      - {{ name: out, side: bottom }}
  - id: end
    name: End
    type: end
    inputs:
      - {{ name: result, side: top }}
edges:
  - source: {{ node: start, port: user_prompt }}
    target: {{ node: worker, port: task }}
  - source: {{ node: worker, port: out }}
    target: {{ node: end, port: result }}
"#
    )
}

fn seed(repo: &std::path::Path) -> anyhow::Result<()> {
    let pipelines = repo.join(".pdo").join("pipelines");
    std::fs::create_dir_all(&pipelines)?;
    for (name, harness, model) in [
        ("tree-claude", "claude", Some("claude-sonnet-5")),
        ("tree-child", "claude", Some("claude-sonnet-5")),
        ("tree-opencode", "opencode", None),
    ] {
        std::fs::write(
            pipelines.join(format!("{name}.yaml")),
            worker_yaml(name, harness, model),
        )?;
        let prompts = pipelines.join(format!("{name}.prompts"));
        std::fs::create_dir_all(&prompts)?;
        std::fs::write(prompts.join("worker.md"), "Work.\n")?;
    }
    let run = |args: &[&str]| -> anyhow::Result<()> {
        let out = Command::new("git").args(args).current_dir(repo).output()?;
        anyhow::ensure!(out.status.success(), "git {args:?} failed");
        Ok(())
    };
    run(&["init", "-q", "-b", "main"])?;
    run(&["config", "user.email", "t@example.com"])?;
    run(&["config", "user.name", "Test"])?;
    run(&["config", "commit.gpgsign", "false"])?;
    std::fs::write(repo.join(".gitignore"), ".pdo/runs/\n")?;
    run(&["add", "."])?;
    run(&["commit", "-q", "-m", "init"])?;
    Ok(())
}

/// Create a Run of `pipeline`; from `session` (`(run, node)`) it is that
/// node's child, otherwise a root.
async fn create_run(daemon: &TestDaemon, pipeline: &str, session: Option<(&str, &str)>) -> String {
    let mut req = reqwest::Client::new()
        .post(format!("{}/runs", daemon.url()))
        .json(&serde_json::json!({
            "pipeline": pipeline,
            "input": "go",
            "target_repo": daemon.target_repo(),
        }));
    if let Some((run_id, node_id)) = session {
        req = req
            .header("X-PDO-Session-Run-Id", run_id)
            .header("X-PDO-Session-Node-Id", node_id);
    }
    let resp = req.send().await.unwrap();
    assert_eq!(resp.status(), 201, "POST /runs should create the run");
    resp.json::<serde_json::Value>().await.unwrap()["run_id"]
        .as_str()
        .unwrap()
        .to_string()
}

/// Wait until the run's worker holds a live session (so it can create children
/// and its pinned session id is frozen).
async fn wait_worker_running(daemon: &TestDaemon, run_id: &str) {
    for _ in 0..150 {
        let run = get_json(daemon, &format!("/runs/{run_id}")).await;
        if run["nodes"]["worker"]["status"] == "running" {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("the worker of {run_id} never reached `running`");
}

async fn get_json(daemon: &TestDaemon, path: &str) -> serde_json::Value {
    let resp = reqwest::get(format!("{}{path}", daemon.url()))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "GET {path}");
    resp.json().await.unwrap()
}

fn assistant(id: &str, model: &str, input: u64) -> String {
    format!(
        "{{\"type\":\"assistant\",\"requestId\":\"r-{id}\",\"message\":{{\"id\":\"{id}\",\
         \"model\":\"{model}\",\"usage\":{{\"input_tokens\":{input},\"output_tokens\":0}}}}}}"
    )
}

/// Plant the worker's transcript of `run_id` (its shared worktree cwd, its
/// pinned session id).
async fn plant_worker(daemon: &TestDaemon, run_id: &str, lines: &[String]) {
    let session_id = daemon.pinned_session_id(run_id, "worker").await;
    let worktree = daemon
        .repo_root()
        .join(".pdo/runs")
        .join(run_id)
        .join("worktree");
    let enc = pdo_daemon::stale_detector::encode_working_dir(&worktree);
    let proj = daemon.repo_root().join(".claude/projects").join(enc);
    std::fs::create_dir_all(&proj).unwrap();
    std::fs::write(proj.join(format!("{session_id}.jsonl")), lines.join("\n")).unwrap();
}

/// A claude-sonnet-5 worker run (3 $/M input) costing `input` tokens.
async fn priced_run(daemon: &TestDaemon, parent: Option<(&str, &str)>, input: u64) -> String {
    priced_run_of(daemon, "tree-claude", parent, input).await
}

/// The same, of Pipeline `pipeline`.
async fn priced_run_of(
    daemon: &TestDaemon,
    pipeline: &str,
    parent: Option<(&str, &str)>,
    input: u64,
) -> String {
    let run_id = create_run(daemon, pipeline, parent).await;
    wait_worker_running(daemon, &run_id).await;
    plant_worker(
        daemon,
        &run_id,
        &[assistant("m1", "claude-sonnet-5", input)],
    )
    .await;
    run_id
}

fn usd(v: &serde_json::Value) -> f64 {
    v["usd"]
        .as_f64()
        .unwrap_or_else(|| panic!("a usd figure in {v}"))
}

fn approx(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

#[tokio::test]
async fn tree_cost_sums_descendants_by_depth_and_leaves_cost_unchanged() {
    if !tmux_available() {
        return; // the workers must actually spawn to create children
    }
    let daemon = TestDaemon::spawn_with_home_override(seed, Some("exec sleep 600".to_string()))
        .await
        .unwrap();

    // Parent $3.00 → child $0.30.
    let parent = priced_run(&daemon, None, 1_000_000).await;
    let child = priced_run(&daemon, Some((&parent, "worker")), 100_000).await;

    // One level: total = parent + child, `cost` untouched.
    let run = get_json(&daemon, &format!("/runs/{parent}")).await;
    assert!(
        approx(usd(&run["cost"]), 3.0),
        "cost stays the Coût du Run parent: {}",
        run["cost"]
    );
    let tree = &run["tree_cost"];
    assert!(approx(usd(&tree["total"]), 3.3), "tree total: {tree}");
    assert!(approx(usd(&tree["parent"]), 3.0));
    assert!(approx(usd(&tree["children"]), 0.3));
    assert_eq!(tree["descendants"], 1);
    assert_eq!(tree["by_depth"].as_array().unwrap().len(), 1);
    assert_eq!(tree["total"]["by_harness"][0]["harness"], "claude");
    assert!(
        approx(usd(&tree["total"]["by_harness"][0]), 3.3),
        "by_harness cumulated"
    );

    // A Run without descendant carries no tree_cost.
    let leaf = get_json(&daemon, &format!("/runs/{child}")).await;
    assert!(
        leaf.get("tree_cost").is_none(),
        "no tree_cost without child: {}",
        leaf["tree_cost"]
    );
    assert!(approx(usd(&leaf["cost"]), 0.3));

    // Grandchild $0.03 → a two-level tree.
    let grandchild = priced_run(&daemon, Some((&child, "worker")), 10_000).await;
    let run = get_json(&daemon, &format!("/runs/{parent}")).await;
    let tree = &run["tree_cost"];
    assert!(approx(usd(&tree["total"]), 3.33), "tree total: {tree}");
    assert!(approx(usd(&tree["children"]), 0.33));
    assert_eq!(tree["descendants"], 2);
    let depths: Vec<(u64, u64, f64)> = tree["by_depth"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| {
            (
                d["depth"].as_u64().unwrap(),
                d["runs"].as_u64().unwrap(),
                usd(&d["cost"]),
            )
        })
        .collect();
    assert_eq!(depths.len(), 2);
    assert_eq!((depths[0].0, depths[0].1), (1, 1));
    assert!(approx(depths[0].2, 0.3));
    assert_eq!((depths[1].0, depths[1].1), (2, 1));
    assert!(approx(depths[1].2, 0.03));
    assert!(approx(usd(&run["cost"]), 3.0), "cost still unchanged");

    // The children endpoint: the child's own cost AND its tree cost.
    let children = get_json(&daemon, &format!("/runs/{parent}/children")).await;
    let entry = &children["nodes"][0]["children"][0];
    assert_eq!(entry["run_id"], child.as_str());
    assert!(approx(usd(&entry["cost"]), 0.3));
    assert!(
        approx(usd(&entry["tree_cost"]["total"]), 0.33),
        "grandchild included: {entry}"
    );
    assert_eq!(entry["tree_cost"]["descendants"], 1);
    // A child without descendant has no tree_cost on the children endpoint.
    let grand_entries = get_json(&daemon, &format!("/runs/{child}/children")).await;
    let g = &grand_entries["nodes"][0]["children"][0];
    assert_eq!(g["run_id"], grandchild.as_str());
    assert!(g.get("tree_cost").is_none());

    // An archived descendant is still counted.
    let resp = reqwest::Client::new()
        .post(format!("{}/runs/{grandchild}/commands", daemon.url()))
        .json(&serde_json::json!({ "kind": "cleanup_run" }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "archive the grandchild");
    let archived = get_json(&daemon, &format!("/runs/{grandchild}")).await;
    assert_eq!(archived["status"], "archived");
    let run = get_json(&daemon, &format!("/runs/{parent}")).await;
    assert_eq!(
        run["tree_cost"]["descendants"], 2,
        "archived child still in the tree"
    );
    assert!(
        approx(usd(&run["tree_cost"]["total"]), 3.33),
        "{}",
        run["tree_cost"]
    );
}

#[tokio::test]
async fn an_uncosted_descendant_withholds_the_total_and_a_partial_one_marks_a_lower_bound() {
    if !tmux_available() {
        return;
    }
    let daemon = TestDaemon::spawn_with_home_override(seed, Some("exec sleep 600".to_string()))
        .await
        .unwrap();

    // A child on a harness with no cost source: the total is "—" with the
    // reason, the parent slice and the claude by-harness slice stay readable.
    let parent = priced_run(&daemon, None, 1_000_000).await;
    let child = create_run(&daemon, "tree-opencode", Some((&parent, "worker"))).await;
    wait_worker_running(&daemon, &child).await;
    let tree = get_json(&daemon, &format!("/runs/{parent}")).await["tree_cost"].clone();
    assert_eq!(
        tree["total"]["uncosted_harnesses"],
        serde_json::json!(["opencode"]),
        "{tree}"
    );
    assert_eq!(tree["total"]["usd"], 0.0, "never half a sum");
    assert!(
        approx(usd(&tree["total"]["by_harness"][0]), 3.0),
        "the claude slice survives"
    );
    assert!(approx(usd(&tree["parent"]), 3.0));
    assert_eq!(
        tree["children"]["uncosted_harnesses"],
        serde_json::json!(["opencode"])
    );

    // A child whose transcript names an unpriced model: the tree is a lower bound.
    let parent = priced_run(&daemon, None, 1_000_000).await;
    let child = create_run(&daemon, "tree-claude", Some((&parent, "worker"))).await;
    wait_worker_running(&daemon, &child).await;
    plant_worker(
        &daemon,
        &child,
        &[
            assistant("m1", "claude-sonnet-5", 100_000),
            assistant("m2", "mystery-model-9", 100_000),
        ],
    )
    .await;
    let tree = get_json(&daemon, &format!("/runs/{parent}")).await["tree_cost"].clone();
    assert_eq!(tree["total"]["partial"], true, "{tree}");
    assert!(approx(usd(&tree["total"]), 3.3));
    assert_eq!(tree["parent"]["partial"], false);
    assert_eq!(tree["children"]["partial"], true);
    assert!(
        tree["total"]["unpriced_models"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m.as_str().is_some_and(|m| m.contains("mystery"))),
        "the offender is named: {tree}"
    );
}

#[tokio::test]
async fn stats_cost_roll_up_files_the_tree_under_its_root_pipeline() {
    if !tmux_available() {
        return;
    }
    let daemon = TestDaemon::spawn_with_home_override(seed, Some("exec sleep 600".to_string()))
        .await
        .unwrap();

    // Orchestrator (tree-claude, $3.00) → child (tree-child, $0.30) →
    // grandchild (tree-child, $0.03).
    let parent = priced_run(&daemon, None, 1_000_000).await;
    let child = priced_run_of(&daemon, "tree-child", Some((&parent, "worker")), 100_000).await;
    let _grandchild = priced_run_of(&daemon, "tree-child", Some((&child, "worker")), 10_000).await;

    let base = "/stats/cost?from=1970-01-01T00:00:00Z&to=2100-01-01T00:00:00Z&bucket=month";
    let find = |rows: &serde_json::Value, id: &str| -> serde_json::Value {
        rows.as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == id)
            .cloned()
            .unwrap_or_else(|| panic!("no row {id} in {rows}"))
    };

    // Off: each Run under its own Pipeline; the explicit `false` is the same
    // response.
    let plain = get_json(&daemon, base).await;
    let off = get_json(&daemon, &format!("{base}&roll_up_children=false")).await;
    assert_eq!(plain, off);
    assert!(approx(
        usd(&find(&plain["by_pipeline"], "tree-claude")),
        3.0
    ));
    assert!(approx(
        usd(&find(&plain["by_pipeline"], "tree-child")),
        0.33
    ));

    // On: the child Pipeline's runs were all orchestrated — it leaves the list.
    let on = get_json(&daemon, &format!("{base}&roll_up_children=true")).await;
    let rows = on["by_pipeline"].as_array().unwrap();
    assert_eq!(rows.len(), 1, "{rows:?}");
    let root = &rows[0];
    assert_eq!(root["id"], "tree-claude");
    assert!(approx(usd(root), 3.33), "{root}");
    let roll_up = &root["roll_up"];
    assert!(approx(usd(&roll_up["parent"]), 3.0));
    assert_eq!(roll_up["children"]["runs"], 2);
    assert_eq!(roll_up["levels"], 2);
    assert_eq!(roll_up["depths"][0]["pipelines"][0]["id"], "tree-child");
    assert!(approx(usd(&roll_up["depths"][1]), 0.03));
    assert!(approx(usd(&on["total"]), usd(&plain["total"])));

    // The Child runs figure is the one Run info shows for the orchestrator.
    let run = get_json(&daemon, &format!("/runs/{parent}")).await;
    assert!(approx(
        usd(&roll_up["children"]),
        usd(&run["tree_cost"]["children"])
    ));

    // « By model » is not concerned.
    assert_eq!(on["by_model"], plain["by_model"]);
}
