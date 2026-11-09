//! A node pruned because its branch was not taken is dead for routing
//! (ADR-0082), on a real daemon running real `script` nodes: an `End` reachable
//! only through it leaves the Run parked « unrouted » until a `force_route`,
//! and a convergence receives only the branches that fired.

use std::time::Duration;

use crate::common::flow::{git_init, post_json, wait_run, worktree};
use crate::common::{ensure_pdo_on_path, TestDaemon};

/// `gen → okn (when verdict = pass) → End`.
const UNROUTED_LAB: &str = r#"name: unrouted-lab
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: gen
    name: gen
    type: script
    outputs:
      - name: out
  - id: okn
    name: okn
    type: script
    outputs:
      - name: out
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: gen, port: task }
  - source: { node: gen, port: out }
    target: { node: okn, port: in }
    when:
      verdict:
        eq: pass
  - source: { node: okn, port: out }
    target: { node: end, port: result }
"#;

/// `gen → okn (pass) → join` and `gen → elsen (else) → e2 → join → End`.
const DEAD2_LAB: &str = r#"name: dead2-lab
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: gen
    name: gen
    type: script
    outputs:
      - name: out
  - id: okn
    name: okn
    type: script
    outputs:
      - name: out
  - id: elsen
    name: elsen
    type: script
    outputs:
      - name: out
  - id: e2
    name: e2
    type: script
    outputs:
      - name: out
  - id: join
    name: join
    type: script
    outputs:
      - name: out
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: gen, port: task }
  - source: { node: gen, port: out }
    target: { node: okn, port: in }
    when:
      verdict:
        eq: pass
  - source: { node: gen, port: out }
    target: { node: elsen, port: in }
    else: true
  - source: { node: okn, port: out }
    target: { node: join, port: a }
  - source: { node: elsen, port: out }
    target: { node: e2, port: in }
  - source: { node: e2, port: out }
    target: { node: join, port: c }
  - source: { node: join, port: out }
    target: { node: end, port: result }
"#;

/// `gen` turns the Run's prompt into its verdict.
const GEN: &str = "#!/usr/bin/env bash\nset -euo pipefail\n\
    v=$(tr -d '[:space:]' < \"$PDO_INPUT_TASK\")\n\
    printf -- '---\\nverdict: %s\\n---\\ngen\\n' \"$v\" > \"$PDO_OUTPUT_OUT\"\n";
const PLAIN: &str = "#!/usr/bin/env bash\nset -euo pipefail\n\
    printf 'done\\n' > \"$PDO_OUTPUT_OUT\"\n";
/// `join` writes down the input paths it was handed.
const JOIN: &str = "#!/usr/bin/env bash\nset -euo pipefail\n\
    printf 'a=[%s] c=[%s]\\n' \"${PDO_INPUT_A:-}\" \"${PDO_INPUT_C:-}\" > \"$PDO_OUTPUT_OUT\"\n";

fn seed(repo: &std::path::Path) -> anyhow::Result<()> {
    let pipelines = repo.join(".pdo").join("pipelines");
    for (name, yaml, nodes) in [
        ("unrouted-lab", UNROUTED_LAB, &["gen", "okn"][..]),
        (
            "dead2-lab",
            DEAD2_LAB,
            &["gen", "okn", "elsen", "e2", "join"][..],
        ),
    ] {
        let prompts = pipelines.join(format!("{name}.prompts"));
        std::fs::create_dir_all(&prompts)?;
        std::fs::write(pipelines.join(format!("{name}.yaml")), yaml)?;
        for node in nodes {
            let body = match *node {
                "gen" => GEN,
                "join" => JOIN,
                _ => PLAIN,
            };
            std::fs::write(prompts.join(format!("{node}.md")), body)?;
        }
    }
    git_init(repo)
}

async fn launch(daemon: &TestDaemon, pipeline: &str, prompt: &str) -> String {
    let (status, body) = post_json(
        daemon,
        "/runs",
        serde_json::json!({
            "pipeline": pipeline,
            "input": prompt,
            "target_repo": daemon.target_repo(),
        }),
    )
    .await;
    assert_eq!(status, 201, "POST /runs: {body}");
    body["run_id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn an_unrouted_run_stays_parked_through_its_pruned_node_until_force_route() {
    ensure_pdo_on_path();
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = launch(&daemon, "unrouted-lab", "other").await;

    let run = wait_run(&daemon, &run_id, "okn pruned", |run| {
        run["nodes"]["okn"]["status"] == "skipped"
    })
    .await;
    assert_eq!(run["nodes"]["okn"]["pruned"], true, "{run}");
    // Give a wrong completion every chance to land before reading the Run.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let run = daemon.get_json(&format!("/runs/{run_id}")).await;
    assert_eq!(run["status"], "awaiting_user", "{run}");
    assert_eq!(run["awaiting_reason_code"], "unrouted", "{run}");

    let (status, body) = post_json(
        &daemon,
        &format!("/runs/{run_id}/commands"),
        serde_json::json!({ "kind": "force_route", "from": "gen", "target": "end" }),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    wait_run(&daemon, &run_id, "the forced route ends the Run", |run| {
        run["status"] == "completed"
    })
    .await;
}

#[tokio::test]
async fn a_convergence_receives_only_the_branch_that_fired() {
    ensure_pdo_on_path();
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = launch(&daemon, "dead2-lab", "pass").await;

    let run = wait_run(&daemon, &run_id, "the Run completes", |run| {
        run["status"] == "completed" || run["status"] == "failed"
    })
    .await;
    assert_eq!(run["status"], "completed", "{run}");
    for pruned in ["elsen", "e2"] {
        assert_eq!(run["nodes"][pruned]["status"], "skipped", "{run}");
    }

    let handed = std::fs::read_to_string(
        worktree(&daemon, &run_id).join(".pdo/artifacts/join/iter-1/out/output.md"),
    )
    .unwrap();
    assert!(handed.contains("okn/iter-1/out/output.md"), "{handed}");
    assert!(
        handed.contains("c=[]"),
        "join was handed the untaken branch: {handed}"
    );

    let io = daemon
        .get_json(&format!("/runs/{run_id}/nodes/join/io?iter=1"))
        .await;
    let ports: Vec<&str> = io["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["port"].as_str().unwrap())
        .collect();
    assert_eq!(ports, vec!["a"], "{io}");
}
