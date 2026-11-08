//! « Roll up child runs » on Stats › Cost (ADR draft-cout-de-l-arbre §2)
//! against the real daemon, over trees of Runs in its log, each Node session
//! costed from its own transcript:
//!
//! - without the parameter (or with `false`) the response is unchanged: each
//!   Run under its own Pipeline, in its own period;
//! - with it, the cohort is the root Runs of the period, each bringing its
//!   whole tree: a child started after the period enters with its root, its
//!   cost leaves its own Pipeline for its root's, split Parent run / Child
//!   runs › Depth k › child Pipeline;
//! - « completed runs only » judges the root, never the child;
//! - « By model » is not concerned.

use serde_json::{json, Value};

use crate::common::flow::git_init;
use crate::common::TestDaemon;
use pdo_run::domain::event::{Event, EventKind};

/// Priced $1 / M input tokens.
const PRICED: &str = "claude-haiku-4-5";

const ROOT: &str = "20260731-230000-root";
const CHILD: &str = "20260801-003000-child";
const GRAND: &str = "20260801-010000-grand";
const LONE: &str = "20260710-090000-lone";
const OPEN: &str = "20260720-090000-open";
const OPEN_KID: &str = "20260720-100000-openkid";

const JULY: &str = "/stats/cost?from=2026-07-01T00:00:00Z&to=2026-08-01T00:00:00Z&bucket=month";

fn event(run_id: &str, ts: &str, kind: EventKind, node: Option<&str>, payload: Value) -> Event {
    Event {
        id: None,
        run_id: run_id.to_string(),
        ts: ts.to_string(),
        kind,
        node_id: node.map(str::to_string),
        iter: node.map(|_| 1),
        payload: Some(payload),
    }
}

/// A Run of `pipeline` started at `ts` (a child of `parent`, if any), whose
/// `worker` Node session read `input` tokens; `completed` appends its
/// `run_completed`.
async fn seed_run(
    daemon: &TestDaemon,
    run_id: &str,
    pipeline: &str,
    ts: &str,
    parent: Option<&str>,
    input: u64,
    completed: bool,
) {
    let repo = daemon.target_repo();
    let mut started = json!({
        "pipeline_id": pipeline,
        "pipeline_name": pipeline,
        "input": "seeded input",
        "target_repo": repo,
    });
    if let Some(parent) = parent {
        started["parent_run_id"] = json!(parent);
        started["parent_node_id"] = json!("worker");
    }
    daemon
        .seed_event(event(run_id, ts, EventKind::RunStarted, None, started))
        .await;
    let session = format!("s-{run_id}");
    daemon
        .seed_event(event(
            run_id,
            ts,
            EventKind::NodeStarted,
            Some("worker"),
            json!({
                "harness": "claude",
                "session_id": session,
                "node_type": "agent",
                "isolated_worktree": false,
            }),
        ))
        .await;
    if completed {
        daemon
            .seed_event(event(run_id, ts, EventKind::RunCompleted, None, json!({})))
            .await;
    }
    let worktree = std::path::Path::new(&repo)
        .join(".pdo/runs")
        .join(run_id)
        .join("worktree");
    let project = daemon.repo_root().join("home/.claude/projects").join(
        pdo_harness::claude_transcript::encode_working_dir(&worktree),
    );
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join(format!("{session}.jsonl")),
        format!(
            "{{\"type\":\"assistant\",\"requestId\":\"r-{run_id}\",\"message\":{{\"id\":\"m-{run_id}\",\
             \"model\":\"{PRICED}\",\"usage\":{{\"input_tokens\":{input},\"output_tokens\":0}}}}}}\n"
        ),
    )
    .unwrap();
}

/// ROOT (orchestrate, $1.00, completed, 31 July 23:00)
///   └ CHILD (implement, $0.20, running, 1 August) └ GRAND (review, $0.03)
/// OPEN (orchestrate, $0.10, running) └ OPEN_KID (implement, $0.04, completed)
/// LONE (implement, $0.50, completed): nobody orchestrated it.
async fn daemon_with_trees() -> TestDaemon {
    let daemon = TestDaemon::spawn_with_extensions(git_init, Vec::new())
        .await
        .unwrap();
    let at = |day: &str, time: &str| format!("2026-{day}T{time}Z");
    seed_run(
        &daemon,
        ROOT,
        "orchestrate",
        &at("07-31", "23:00:00"),
        None,
        1_000_000,
        true,
    )
    .await;
    seed_run(
        &daemon,
        CHILD,
        "implement",
        &at("08-01", "00:30:00"),
        Some(ROOT),
        200_000,
        false,
    )
    .await;
    seed_run(
        &daemon,
        GRAND,
        "review",
        &at("08-01", "01:00:00"),
        Some(CHILD),
        30_000,
        false,
    )
    .await;
    seed_run(
        &daemon,
        LONE,
        "implement",
        &at("07-10", "09:00:00"),
        None,
        500_000,
        true,
    )
    .await;
    seed_run(
        &daemon,
        OPEN,
        "orchestrate",
        &at("07-20", "09:00:00"),
        None,
        100_000,
        false,
    )
    .await;
    seed_run(
        &daemon,
        OPEN_KID,
        "implement",
        &at("07-20", "10:00:00"),
        Some(OPEN),
        40_000,
        true,
    )
    .await;
    daemon
}

fn usd(row: &Value) -> f64 {
    row["usd"]
        .as_f64()
        .unwrap_or_else(|| panic!("no usd in {row}"))
}

fn approx(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

fn row<'a>(rows: &'a Value, id: &str) -> &'a Value {
    rows.as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == id)
        .unwrap_or_else(|| panic!("no row {id} in {rows}"))
}

fn ids(rows: &Value) -> Vec<&str> {
    rows.as_array()
        .unwrap()
        .iter()
        .map(|row| row["id"].as_str().unwrap())
        .collect()
}

#[tokio::test]
async fn the_roll_up_files_each_tree_under_its_root_pipeline_and_period() {
    let daemon = daemon_with_trees().await;

    // Off: each Run under its own Pipeline; CHILD and GRAND started in August.
    let plain = daemon.get_json(JULY).await;
    let off = daemon
        .get_json(&format!("{JULY}&roll_up_children=false"))
        .await;
    assert_eq!(plain, off, "an explicit false is the same response");
    assert_eq!(ids(&plain["by_pipeline"]), ["orchestrate", "implement"]);
    assert!(approx(usd(row(&plain["by_pipeline"], "orchestrate")), 1.1));
    assert!(approx(usd(row(&plain["by_pipeline"], "implement")), 0.54));
    assert!(
        !plain.to_string().contains("roll_up"),
        "no roll-up anywhere when off"
    );

    // On: the trees follow their roots.
    let on = daemon
        .get_json(&format!("{JULY}&roll_up_children=true"))
        .await;
    assert_eq!(ids(&on["by_pipeline"]), ["orchestrate", "implement"]);
    let root = row(&on["by_pipeline"], "orchestrate");
    assert!(approx(usd(root), 1.37), "{root}");
    assert_eq!(root["runs"], 2, "two root runs");
    let roll_up = &root["roll_up"];
    assert_eq!(roll_up["parent"]["name"], "Parent run");
    assert!(approx(usd(&roll_up["parent"]), 1.1));
    assert_eq!(roll_up["children"]["name"], "Child runs");
    assert_eq!(roll_up["children"]["runs"], 3);
    assert!(approx(usd(&roll_up["children"]), 0.27));
    assert_eq!(roll_up["levels"], 2);
    assert_eq!(roll_up["depths"][0]["name"], "Depth 1");
    assert_eq!(roll_up["depths"][0]["runs"], 2);
    let depth_1_implement = row(&roll_up["depths"][0]["pipelines"], "implement");
    assert!(approx(usd(depth_1_implement), 0.24), "{depth_1_implement}");
    assert_eq!(depth_1_implement["nodes"][0]["id"], "worker");
    assert!(approx(usd(&roll_up["depths"][1]), 0.03));
    assert_eq!(roll_up["depths"][1]["pipelines"][0]["id"], "review");

    // The child Pipeline keeps only the run nobody orchestrated.
    let implement = row(&on["by_pipeline"], "implement");
    assert!(approx(usd(implement), 0.5), "{implement}");
    assert_eq!(implement["runs"], 1);
    assert!(implement.get("roll_up").is_none());

    // The whole tree in its root's period: the total differs from the
    // default view (CHILD and GRAND started in August).
    assert_eq!(on["by_period"].as_array().unwrap().len(), 1);
    assert_eq!(on["by_period"][0]["bucket"], "2026-07");
    assert!(approx(usd(&on["total"]), usd(&plain["total"]) + 0.23));
    // By project: the same tree, under the root's Project.
    let project = &on["by_project"][0];
    assert_eq!(project["pipelines"][0]["roll_up"]["children"]["runs"], 3);
    // « By model » is not concerned.
    assert_eq!(on["by_model"], plain["by_model"]);
}

#[tokio::test]
async fn completed_runs_only_judges_the_root_not_the_child() {
    let daemon = daemon_with_trees().await;

    // Off: the completed Runs started in July — OPEN_KID included.
    let plain = daemon
        .get_json(&format!("{JULY}&completed_only=true"))
        .await;
    assert!(approx(usd(row(&plain["by_pipeline"], "orchestrate")), 1.0));
    assert!(approx(usd(row(&plain["by_pipeline"], "implement")), 0.54));

    // On: ROOT is completed, its running descendants come along; OPEN is not,
    // its completed child leaves with it.
    let on = daemon
        .get_json(&format!("{JULY}&completed_only=true&roll_up_children=true"))
        .await;
    let root = row(&on["by_pipeline"], "orchestrate");
    assert!(approx(usd(root), 1.23), "{root}");
    assert_eq!(root["runs"], 1);
    assert_eq!(root["roll_up"]["children"]["runs"], 2);
    assert!(approx(usd(row(&on["by_pipeline"], "implement")), 0.5));
}
