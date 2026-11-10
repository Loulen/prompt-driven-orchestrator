//! The Coût de l'arbre (ADR-0101) against the real daemon,
//! over a real tree of Runs in its log, each costed from its own transcript:
//!
//! - `GET /runs/{id}` carries an additive `tree_cost`: the total, `parent`
//!   (the Run's own `cost`, untouched), `children` and `descendants`, and each
//!   depth. An archived descendant counts; a forgotten one is out of the tree.
//!   A Run without descendant carries none.
//! - A descendant without a cost refuses the sums it belongs to, the known
//!   slices kept; a lower-bound descendant makes the tree a lower bound.
//! - `GET /runs/{id}/children` gives each child its own tree cost.
//! - An extension reads the same figures through `Host::run_cost`.

use serde_json::Value;

use crate::common::flow::git_init;
use crate::common::TestDaemon;
use crate::test_extension::{Refusal, TestExtension};

const ROOT: &str = "20261107-100000-root";
const CHILD: &str = "20261107-100100-child";
const GRANDCHILD: &str = "20261107-100200-grandchild";
const ARCHIVED: &str = "20261107-100300-archived";
const FORGOTTEN: &str = "20261107-100400-forgotten";

/// `input` tokens of a model priced $1 / M input tokens: `input / 1e6` dollars.
const PRICED: &str = "claude-haiku-4-5";

/// Plant a Claude Code transcript of `model` for `run_id`'s worktree, under
/// the daemon's home: `input` tokens, one message.
fn plant_transcript(daemon: &TestDaemon, run_id: &str, model: &str, input: u64) {
    let worktree = std::path::Path::new(&daemon.target_repo())
        .join(".pdo/runs")
        .join(run_id)
        .join("worktree");
    let encoded = pdo_harness::claude_transcript::encode_working_dir(&worktree);
    let project = daemon
        .repo_root()
        .join("home/.claude/projects")
        .join(encoded);
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join("s.jsonl"),
        format!(
            "{{\"type\":\"assistant\",\"requestId\":\"r-{run_id}\",\"message\":{{\"id\":\"m-{run_id}\",\
             \"model\":\"{model}\",\"usage\":{{\"input_tokens\":{input},\"output_tokens\":0}}}}}}\n"
        ),
    )
    .unwrap();
}

async fn get(daemon: &TestDaemon, path: &str) -> Value {
    let resp = reqwest::get(format!("{}{path}", daemon.url()))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "GET {path}");
    resp.json().await.unwrap()
}

fn usd(cost: &Value) -> f64 {
    cost["usd"]
        .as_f64()
        .unwrap_or_else(|| panic!("no usd in {cost}"))
}

fn approx(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

/// ROOT ($0.10) ─ CHILD ($0.20) ─ GRANDCHILD ($0.30)
///              ├ ARCHIVED ($0.40, archived)
///              └ FORGOTTEN (archived then forgotten: out of the tree)
async fn daemon_with_a_tree() -> TestDaemon {
    let (ext, _control) = TestExtension::new(Refusal::Nothing);
    let daemon = TestDaemon::spawn_with_extensions(git_init, ext.boxed())
        .await
        .unwrap();
    let repo = daemon.target_repo();
    daemon.seed_run_in(ROOT, "orchestrate", &repo).await;
    daemon
        .seed_child_run_in(CHILD, "implement", &repo, ROOT, "worker")
        .await;
    daemon
        .seed_child_run_in(GRANDCHILD, "review", &repo, CHILD, "worker")
        .await;
    daemon
        .seed_child_run_in(ARCHIVED, "implement", &repo, ROOT, "worker")
        .await;
    daemon.seed_run_archived(ARCHIVED).await;
    daemon
        .seed_child_run_in(FORGOTTEN, "implement", &repo, ROOT, "worker")
        .await;
    daemon.seed_run_archived(FORGOTTEN).await;
    let resp = reqwest::Client::new()
        .delete(format!("{}/runs/{FORGOTTEN}", daemon.url()))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "forget: {}", resp.status());
    plant_transcript(&daemon, ROOT, PRICED, 100_000);
    plant_transcript(&daemon, CHILD, PRICED, 200_000);
    plant_transcript(&daemon, GRANDCHILD, PRICED, 300_000);
    plant_transcript(&daemon, ARCHIVED, PRICED, 400_000);
    daemon
}

#[tokio::test]
async fn a_run_reads_its_tree_cost_and_its_own_cost_unchanged() {
    let daemon = daemon_with_a_tree().await;

    let run = get(&daemon, &format!("/runs/{ROOT}")).await;
    assert!(
        approx(usd(&run["cost"]), 0.1),
        "cost stays the parent run's: {}",
        run["cost"]
    );
    let tree = &run["tree_cost"];
    assert!(approx(usd(&tree["total"]), 1.0), "{tree}");
    assert!(approx(usd(&tree["parent"]), 0.1), "{tree}");
    assert!(approx(usd(&tree["children"]), 0.9), "{tree}");
    assert_eq!(tree["descendants"], 3, "the forgotten child is out: {tree}");
    assert_eq!(tree["unavailable_runs"], 0);
    assert_eq!(tree["by_depth"][0]["depth"], 1);
    assert_eq!(tree["by_depth"][0]["runs"], 2);
    assert!(approx(usd(&tree["by_depth"][0]["cost"]), 0.6), "{tree}");
    assert_eq!(tree["by_depth"][1]["depth"], 2);
    assert!(approx(usd(&tree["by_depth"][1]["cost"]), 0.3), "{tree}");

    // A Run without descendant carries no tree cost.
    let leaf = get(&daemon, &format!("/runs/{GRANDCHILD}")).await;
    assert!(leaf.get("tree_cost").is_none(), "{}", leaf["tree_cost"]);
    assert!(approx(usd(&leaf["cost"]), 0.3));

    // Each child of the Orchestration tab carries its own tree.
    let children = get(&daemon, &format!("/runs/{ROOT}/children")).await;
    let entries = children["nodes"][0]["children"].as_array().unwrap();
    let child = entries.iter().find(|c| c["run_id"] == CHILD).unwrap();
    assert!(approx(usd(&child["cost"]), 0.2));
    assert!(approx(usd(&child["tree_cost"]["total"]), 0.5), "{child}");
    assert_eq!(child["tree_cost"]["descendants"], 1);
    let archived = entries.iter().find(|c| c["run_id"] == ARCHIVED).unwrap();
    assert!(archived.get("tree_cost").is_none(), "{archived}");

    // The extension reads the same figures.
    let view = get(&daemon, &format!("/ext/test/runs/{ROOT}/view")).await;
    let cost = &view["cost"];
    assert!(approx(usd(cost), 0.1), "{cost}");
    assert!(approx(usd(&cost["tree"]["total"]), 1.0), "{cost}");
    assert!(approx(usd(&cost["tree"]["parent"]), 0.1));
    assert!(approx(usd(&cost["tree"]["children"]), 0.9));
    assert_eq!(cost["tree"]["descendants"], 3);
    assert_eq!(cost["tree"]["by_depth"][1]["runs"], 1);
    let leaf_view = get(&daemon, &format!("/ext/test/runs/{GRANDCHILD}/view")).await;
    assert!(leaf_view["cost"].get("tree").is_none(), "{leaf_view}");
}

#[tokio::test]
async fn an_unknown_descendant_refuses_the_sum_and_a_lower_bound_spreads() {
    let daemon = daemon_with_a_tree().await;
    // A grandchild of an unpriced model: the tree is a lower bound.
    plant_transcript(&daemon, GRANDCHILD, "claude-mystery-9", 300_000);
    let tree = get(&daemon, &format!("/runs/{ROOT}")).await["tree_cost"].clone();
    assert_eq!(tree["total"]["partial"], true, "{tree}");
    assert_eq!(tree["total"]["unpriced_models"][0], "claude-mystery-9");
    assert_eq!(
        tree["parent"]["partial"], false,
        "the parent's own cost is untouched"
    );

    // A child without any session cost: the sums it belongs to read « — »,
    // the known slices stay.
    let repo = daemon.target_repo();
    daemon
        .seed_child_run_in("20261107-100500-mute", "implement", &repo, ROOT, "worker")
        .await;
    let tree = get(&daemon, &format!("/runs/{ROOT}")).await["tree_cost"].clone();
    assert_eq!(tree["total"], Value::Null, "{tree}");
    assert_eq!(tree["children"], Value::Null);
    assert_eq!(tree["unavailable_runs"], 1);
    assert_eq!(tree["descendants"], 4);
    assert!(approx(usd(&tree["parent"]), 0.1), "{tree}");
    assert_eq!(tree["by_depth"][0]["cost"], Value::Null);
    assert!(
        tree["by_depth"][1]["cost"].is_object(),
        "depth 2 stays: {tree}"
    );
    let view = get(&daemon, &format!("/ext/test/runs/{ROOT}/view")).await;
    assert_eq!(
        view["cost"]["tree"]["total"]["usd"],
        Value::Null,
        "« — », never $0"
    );
    assert!(approx(usd(&view["cost"]["tree"]["parent"]), 0.1));
}
