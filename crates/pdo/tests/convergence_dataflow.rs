//! The dataflow of HP-02 on a real daemon running real `script` nodes
//! (CORE-240): a bounded review loop that passes on its 2nd lap, a collection
//! of two items and a branch not taken converge on `summarizer`. ADR-0082: the
//! convergence starts once every incoming edge is resolved — after the loop's
//! PASS and the collection's barrier — and reads every lap of the collection.

use crate::common::flow::{git_init, post_json, wait_run, worktree};
use crate::common::{ensure_pdo_on_path, TestDaemon};

const HP02_DATAFLOW: &str = r#"name: hp02-dataflow
version: "1.0"
nodes:
  - { id: start, name: Start, type: start, outputs: [{ name: user_prompt }] }
  - { id: planner, name: planner, type: script, outputs: [{ name: plan }] }
  - { id: coder, name: coder, type: script, outputs: [{ name: code }] }
  - { id: reviewer, name: reviewer, type: script, outputs: [{ name: review }] }
  - { id: itemizer, name: itemizer, type: script, outputs: [{ name: item_note }] }
  - { id: fallback, name: fallback, type: script, outputs: [{ name: note }] }
  - { id: summarizer, name: summarizer, type: script, outputs: [{ name: summary }] }
  - { id: end, name: End, type: end, inputs: [{ name: result }] }
edges:
  - source: { node: start, port: user_prompt }
    target: { node: planner, port: task }
  - source: { node: planner, port: plan }
    target: { node: coder, port: plan }
    when: { route: { eq: go } }
  - source: { node: planner, port: plan }
    target: { node: itemizer, port: plan }
    when: { route: { eq: go } }
  - source: { node: planner, port: plan }
    target: { node: fallback, port: plan }
    else: true
  - source: { node: coder, port: code }
    target: { node: reviewer, port: code }
  - source: { node: reviewer, port: review }
    target: { node: coder, port: review }
    when: { verdict: { eq: FAIL } }
  - source: { node: reviewer, port: review }
    target: { node: summarizer, port: review }
    when: { verdict: { eq: PASS } }
  - source: { node: itemizer, port: item_note }
    target: { node: summarizer, port: notes }
  - source: { node: fallback, port: note }
    target: { node: summarizer, port: notes }
  - source: { node: summarizer, port: summary }
    target: { node: end, port: result }
loops:
  - { id: review_loop, kind: bounded, members: [coder, reviewer], max_iter: 3 }
  - { id: topics_fanout, kind: collection, over: topics, members: [itemizer] }
"#;

const PLANNER: &str = "#!/usr/bin/env bash\nset -euo pipefail\n\
    printf -- '---\\nroute: go\\ntopics: [alpha, beta]\\n---\\nplan\\n' > \"$PDO_OUTPUT_PLAN\"\n";
const CODER: &str = "#!/usr/bin/env bash\nset -euo pipefail\n\
    printf 'code %s\\n' \"$PDO_NODE_ITER\" > \"$PDO_OUTPUT_CODE\"\n";
/// FAIL on lap 1, PASS on lap 2; slow enough that the collection's barrier
/// fires while the loop is still turning — the order F2-1 was seen in.
const REVIEWER: &str = "#!/usr/bin/env bash\nset -euo pipefail\nsleep 2\n\
    if [ \"$PDO_NODE_ITER\" -ge 2 ]; then v=PASS; else v=FAIL; fi\n\
    printf -- '---\\nverdict: %s\\n---\\nreview %s\\n' \"$v\" \"$PDO_NODE_ITER\" > \"$PDO_OUTPUT_REVIEW\"\n";
const ITEMIZER: &str = "#!/usr/bin/env bash\nset -euo pipefail\n\
    printf 'note %s\\n' \"$PDO_ITEM\" > \"$PDO_OUTPUT_ITEM_NOTE\"\n";
const FALLBACK: &str = "#!/usr/bin/env bash\nset -euo pipefail\n\
    printf 'fallback\\n' > \"$PDO_OUTPUT_NOTE\"\n";
/// `summarizer` writes down the input paths it was handed.
const SUMMARIZER: &str = "#!/usr/bin/env bash\nset -euo pipefail\n\
    printf 'review=[%s]\\nnotes=[%s]\\n' \"${PDO_INPUT_REVIEW:-}\" \"${PDO_INPUT_NOTES:-}\" > \"$PDO_OUTPUT_SUMMARY\"\n";

fn seed(repo: &std::path::Path) -> anyhow::Result<()> {
    let pipelines = repo.join(".pdo").join("pipelines");
    let prompts = pipelines.join("hp02-dataflow.prompts");
    std::fs::create_dir_all(&prompts)?;
    std::fs::write(pipelines.join("hp02-dataflow.yaml"), HP02_DATAFLOW)?;
    for (node, body) in [
        ("planner", PLANNER),
        ("coder", CODER),
        ("reviewer", REVIEWER),
        ("itemizer", ITEMIZER),
        ("fallback", FALLBACK),
        ("summarizer", SUMMARIZER),
    ] {
        std::fs::write(prompts.join(format!("{node}.md")), body)?;
    }
    git_init(repo)
}

/// The position of the first event matching `kind`, `node` and `iter`.
fn position(
    events: &[serde_json::Value],
    kind: &str,
    node: Option<&str>,
    iter: Option<i64>,
) -> usize {
    events
        .iter()
        .position(|e| {
            e["kind"] == kind
                && node.is_none_or(|n| e["node_id"] == n)
                && iter.is_none_or(|i| e["iter"] == i)
        })
        .unwrap_or_else(|| panic!("no {kind} {node:?} {iter:?} in {events:#?}"))
}

#[tokio::test]
async fn the_convergence_waits_for_the_loop_exit_and_reads_every_lap_of_the_collection() {
    ensure_pdo_on_path();
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let (status, body) = post_json(
        &daemon,
        "/runs",
        serde_json::json!({
            "pipeline": "hp02-dataflow",
            "input": "go",
            "target_repo": daemon.target_repo(),
        }),
    )
    .await;
    assert_eq!(status, 201, "POST /runs: {body}");
    let run_id = body["run_id"].as_str().unwrap().to_string();

    let run = wait_run(&daemon, &run_id, "the Run ends", |run| {
        ["completed", "failed", "awaiting_user", "halted"]
            .contains(&run["status"].as_str().unwrap_or(""))
    })
    .await;
    assert_eq!(run["status"], "completed", "{run}");
    assert_eq!(run["nodes"]["fallback"]["pruned"], true, "{run}");

    // F2-1: summarizer starts after the reviewer's PASS (lap 2) and after the
    // collection's barrier — never on the FAIL of lap 1.
    let events = daemon.get_json(&format!("/runs/{run_id}/events")).await;
    let events = events.as_array().unwrap();
    let started = position(events, "node_started", Some("summarizer"), None);
    let passed = position(events, "node_completed", Some("reviewer"), Some(2));
    let barrier = position(events, "collection_done", None, None);
    assert!(
        started > passed && started > barrier,
        "summarizer started at #{started}, reviewer lap 2 done at #{passed}, barrier at #{barrier}"
    );

    // F2-2 + the branch not taken: `review` is lap 2's, `notes` holds both laps
    // of the collection in item order, and nothing of `fallback`.
    let handed = std::fs::read_to_string(
        worktree(&daemon, &run_id).join(".pdo/artifacts/summarizer/iter-1/summary/output.md"),
    )
    .unwrap();
    assert!(
        handed.contains("reviewer/iter-2/review/output.md"),
        "{handed}"
    );
    let lap1 = handed
        .find("itemizer/iter-1/item_note/output.md")
        .unwrap_or_else(|| panic!("lap 1 lost: {handed}"));
    let lap2 = handed
        .find("itemizer/iter-2/item_note/output.md")
        .unwrap_or_else(|| panic!("lap 2 lost: {handed}"));
    assert!(lap1 < lap2, "laps out of item order: {handed}");
    assert!(!handed.contains("fallback/"), "{handed}");

    let io = daemon
        .get_json(&format!("/runs/{run_id}/nodes/summarizer/io?iter=1"))
        .await;
    let inputs = io["inputs"].as_array().unwrap();
    let ports: Vec<&str> = inputs.iter().map(|p| p["port"].as_str().unwrap()).collect();
    assert_eq!(ports, vec!["review", "notes"], "{io}");
    let notes: Vec<&str> = inputs[1]["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["path"].as_str().unwrap())
        .collect();
    assert_eq!(
        notes,
        vec![
            "itemizer/iter-1/item_note/output.md",
            "itemizer/iter-2/item_note/output.md",
        ],
        "{io}"
    );
}
