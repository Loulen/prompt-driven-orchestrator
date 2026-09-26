//! Layer 3 (#735, ADR-0065) — the Stats › Cost « By model » axis, end to end
//! through a real daemon.
//!
//! Two `claude` Runs execute the same worker Node with two different pinned
//! models (the Feature Path): Run A pins the full id `claude-sonnet-5`, Run B
//! pins the alias `opus` and plants **no transcript**, so its source is mute
//! and the requested id shows up as its own `requested` row. Run A's session
//! carries **two** models (the main one plus a subagent on another model), so
//! the cost ventilates per message and the execution counts in each bucket.
//!
//! What is proven here rather than in unit tests (ADR-0004's règle d'or):
//!   1. `GET /stats/cost` answers with a `by_model` tree: Model → Effort →
//!      Pipeline → Node, ids verbatim, ranked by cost;
//!   2. observed vs requested provenance rides the wire, and the alias pin and
//!      the observed full id are two rows (ADR-0065 §2 — never merged);
//!   3. a two-model session costs — and counts — in each bucket, the average
//!      per execution staying coherent with the bucket's cost (ADR-0065 §3);
//!   4. the Node leaves of `by_pipeline` carry the model × effort pairs, with
//!      the effort marked requested.

use crate::common::TestDaemon;
use std::process::Command;

fn tmux_available() -> bool {
    Command::new("tmux")
        .arg("-V")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn worker_yaml(name: &str, model: &str) -> String {
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
    pin_harness: claude
    harnesses:
      claude:
        model: {model}
        effort: high
    inputs:
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
    for (name, model) in [("bymodel-a", "claude-sonnet-5"), ("bymodel-b", "opus")] {
        std::fs::write(
            pipelines.join(format!("{name}.yaml")),
            worker_yaml(name, model),
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

async fn create_run(daemon: &TestDaemon, pipeline: &str) -> String {
    let resp = reqwest::Client::new()
        .post(format!("{}/runs", daemon.url()))
        .json(&serde_json::json!({
            "pipeline": pipeline,
            "input": "go",
            "target_repo": daemon.target_repo(),
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "POST /runs should create the run");
    resp.json::<serde_json::Value>().await.unwrap()["run_id"]
        .as_str()
        .unwrap()
        .to_string()
}

/// Wait until `node_id`'s latest `node_started` carries the pinned model — the
/// freeze the cost fold reads.
async fn wait_for_started(daemon: &TestDaemon, run_id: &str, node_id: &str) {
    for _ in 0..100 {
        let evs: serde_json::Value = reqwest::get(format!("{}/runs/{run_id}/events", daemon.url()))
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let started = evs
            .as_array()
            .unwrap()
            .iter()
            .rev()
            .find(|e| e["kind"] == "node_started" && e["node_id"] == node_id);
        if started
            .and_then(|e| e["payload"]["model"].as_str())
            .is_some()
        {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let evs: serde_json::Value = reqwest::get(format!("{}/runs/{run_id}/events", daemon.url()))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    panic!("{node_id} should have started with a frozen model within the timeout; events: {evs}");
}

/// Plant a transcript for the run's shared worktree cwd, attributed to the
/// node's pinned session — `lines` is the raw JSONL body.
fn plant_session(daemon: &TestDaemon, run_id: &str, session_id: &str, lines: &[String]) {
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

fn assistant(id: &str, model: &str, input: u64) -> String {
    format!(
        "{{\"type\":\"assistant\",\"requestId\":\"r-{id}\",\"message\":{{\"id\":\"{id}\",\
         \"model\":\"{model}\",\"usage\":{{\"input_tokens\":{input},\"output_tokens\":0}}}}}}"
    )
}

async fn stats_cost(daemon: &TestDaemon) -> serde_json::Value {
    reqwest::get(format!(
        "{}/stats/cost?from=1970-01-01T00:00:00Z&to=2100-01-01T00:00:00Z&bucket=day",
        daemon.url()
    ))
    .await
    .unwrap()
    .json()
    .await
    .unwrap()
}

/// The pi session-folder name for a working directory — pi encodes the cwd
/// into the FOLDER (`--…--`), the session id into the FILE name
/// (`crate::pi_session::session_dir_name`, measured 0.85.1).
fn pi_session_dir_name(working_dir: &std::path::Path) -> String {
    let raw = working_dir.to_string_lossy();
    let stripped = raw
        .strip_prefix('/')
        .or_else(|| raw.strip_prefix('\\'))
        .unwrap_or(&raw);
    let encoded: String = stripped
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':') {
                '-'
            } else {
                c
            }
        })
        .collect();
    format!("--{encoded}--")
}

/// Plant a `pi` session for the run's shared worktree: one assistant message
/// carrying the model verbatim, the provider and a reported cost, at the
/// thinking level a `thinking_level_change` event set.
fn plant_pi_session(daemon: &TestDaemon, run_id: &str, session_id: &str, model: &str) {
    let worktree = daemon
        .repo_root()
        .join(".pdo/runs")
        .join(run_id)
        .join("worktree");
    let dir = daemon
        .repo_root()
        .join(".pi/agent/sessions")
        .join(pi_session_dir_name(&worktree));
    std::fs::create_dir_all(&dir).unwrap();
    let level_change = r#"{"type":"thinking_level_change","id":"t1","thinkingLevel":"low"}"#;
    let message = format!(
        r#"{{"type":"message","id":"m1","message":{{"role":"assistant","model":"{model}","provider":"openrouter","usage":{{"input":1000,"output":10,"totalTokens":1010,"cost":{{"total":0.5}}}},"stopReason":"stop"}}}}"#
    );
    std::fs::write(
        dir.join(format!("2026-09-07T10-00-00-000Z_{session_id}.jsonl")),
        format!("{level_change}\n{message}\n"),
    )
    .unwrap();
}

/// Plant a `copilot` journal for the run: the session opens on the model and
/// effort (measured 1.0.83 shapes), one usage point names the model again.
fn plant_copilot_journal(daemon: &TestDaemon, session_id: &str, model: &str) {
    let dir = daemon
        .repo_root()
        .join(".copilot/session-state")
        .join(session_id);
    std::fs::create_dir_all(&dir).unwrap();
    let start = format!(
        r#"{{"type":"session.start","data":{{"selectedModel":"{model}","reasoningEffort":"medium"}}}}"#
    );
    let checkpoint = format!(
        r#"{{"type":"session.usage_checkpoint","data":{{"totalNanoAiu":25000000000,"modelCacheState":[{{"modelId":"{model}"}}]}}}}"#
    );
    std::fs::write(dir.join("events.jsonl"), format!("{start}\n{checkpoint}\n")).unwrap();
}

/// Wait until `node_id` has started at all — an opencode node pins no model.
async fn wait_for_node(daemon: &TestDaemon, run_id: &str, node_id: &str) {
    for _ in 0..100 {
        let evs: serde_json::Value = reqwest::get(format!("{}/runs/{run_id}/events", daemon.url()))
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        if evs
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["kind"] == "node_started" && e["node_id"] == node_id)
        {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("{node_id} should have started within the timeout");
}

fn stats_fixtures(repo: &std::path::Path) -> anyhow::Result<()> {
    let pipelines = repo.join(".pdo").join("pipelines");
    std::fs::create_dir_all(&pipelines)?;
    for (name, harness, model, effort) in [
        ("bymodel-a", "claude", Some("claude-sonnet-5"), Some("high")),
        // The pi run pins a DIFFERENT model/effort than its source reports —
        // the observed values win and say so (ADR-0065 §1).
        ("bymodel-pi", "pi", Some("claude-sonnet-5"), Some("medium")),
        (
            "bymodel-copilot",
            "copilot",
            Some("gpt-5.6-sol"),
            Some("low"),
        ),
        ("bymodel-opencode", "opencode", None, None),
    ] {
        let harness_block = match (model, effort) {
            (Some(m), Some(e)) => format!(
                "    harnesses:\n      {harness}:\n        model: {m}\n        effort: {e}\n"
            ),
            _ => String::new(),
        };
        std::fs::write(
            pipelines.join(format!("{name}.yaml")),
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
            ),
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

#[tokio::test]
async fn by_model_merges_pi_and_claude_per_id_and_keeps_opencode_off_the_axis() {
    if !tmux_available() {
        return; // the node must actually spawn to freeze model/effort/session
    }
    let daemon =
        TestDaemon::spawn_with_home_override(stats_fixtures, Some("exec sleep 600".to_string()))
            .await
            .unwrap();

    // Run A (claude): observed per message, effort requested. Run B (pi): the
    // SAME model id, observed from its session with the provider; its pinned
    // effort (medium) is overridden by the observed level (low). Run C
    // (copilot): observed model + reasoning effort at the usage point. Run D
    // (opencode): no cost source — no model row, "—" elsewhere.
    let run_a = create_run(&daemon, "bymodel-a").await;
    wait_for_started(&daemon, &run_a, "worker").await;
    let sid_a = daemon.pinned_session_id(&run_a, "worker").await;
    // sonnet-5 3/15 · 1_000_000 in → $3.00.
    plant_session(
        &daemon,
        &run_a,
        &sid_a,
        &[assistant("m1", "claude-sonnet-5", 1_000_000)],
    );

    let run_b = create_run(&daemon, "bymodel-pi").await;
    wait_for_started(&daemon, &run_b, "worker").await;
    let sid_b = daemon.pinned_session_id(&run_b, "worker").await;
    plant_pi_session(&daemon, &run_b, &sid_b, "claude-sonnet-5");

    let run_c = create_run(&daemon, "bymodel-copilot").await;
    wait_for_started(&daemon, &run_c, "worker").await;
    let sid_c = daemon.pinned_session_id(&run_c, "worker").await;
    plant_copilot_journal(&daemon, &sid_c, "gpt-5.6-sol");

    let run_d = create_run(&daemon, "bymodel-opencode").await;
    wait_for_node(&daemon, &run_d, "worker").await;

    let cost = stats_cost(&daemon).await;

    // One id via claude AND pi is ONE row carrying TWO harness columns.
    let models = cost["by_model"].as_array().expect("by_model on the wire");
    let ids: Vec<&str> = models.iter().filter_map(|m| m["id"].as_str()).collect();
    assert_eq!(
        ids,
        vec!["claude-sonnet-5", "gpt-5.6-sol"],
        "ranked by cost; opencode has no row at all"
    );
    let sonnet = &models[0];
    assert_eq!(sonnet["provenance"], "observed");
    assert_eq!(sonnet["usd"], 3.5, "claude $3 + pi $0.5, totals summed");
    assert_eq!(sonnet["executions"], 2);
    let harnesses = sonnet["harnesses"].as_array().unwrap();
    let claude_entry = harnesses
        .iter()
        .find(|h| h["harness"] == "claude")
        .expect("the claude column");
    assert_eq!(claude_entry["usd"], 3.0);
    assert_eq!(claude_entry["provenance"], "observed");
    assert!(claude_entry.get("provider").is_none());
    let pi_entry = harnesses
        .iter()
        .find(|h| h["harness"] == "pi")
        .expect("the pi column");
    assert_eq!(pi_entry["usd"], 0.5);
    assert_eq!(pi_entry["provenance"], "observed");
    assert_eq!(
        pi_entry["provider"], "openrouter",
        "the provider rides the harness entry — tooltip only, never the identity"
    );

    // Two efforts under the one id: high (claude, requested) and low (pi,
    // observed — the pinned medium is NOT shown).
    let efforts = sonnet["efforts"].as_array().unwrap();
    let effort_ids: Vec<&str> = efforts.iter().filter_map(|e| e["id"].as_str()).collect();
    assert_eq!(effort_ids, vec!["high", "low"], "ranked by cost");
    let high = &efforts[0];
    assert_eq!(high["provenance"], "requested");
    let low = &efforts[1];
    assert_eq!(low["provenance"], "observed");
    let low_pi = low["harnesses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["harness"] == "pi")
        .unwrap();
    assert_eq!(low_pi["effort_provenance"], "observed");

    // copilot: observed model, observed effort, reported (not ~estimated).
    let gpt = &models[1];
    assert_eq!(gpt["provenance"], "observed");
    assert_eq!(gpt["usd"], 0.25);
    assert_eq!(gpt["estimated"], false);
    let gpt_effort = &gpt["efforts"].as_array().unwrap()[0];
    assert_eq!(gpt_effort["id"], "medium");
    assert_eq!(gpt_effort["provenance"], "observed");

    // opencode stays "—" under « By pipeline », said by name.
    let pipeline = cost["by_pipeline"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "bymodel-opencode")
        .expect("the opencode run's pipeline");
    let worker = pipeline["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["name"] == "Worker")
        .expect("the opencode worker node");
    assert!(worker["usd"].is_null(), "no reading, never an invented $0");
    assert_eq!(
        worker["harnesses"][0]["missing_reasons"][0],
        "harness has no cost source"
    );
    // ...and the total still says it, by name.
    assert!(cost["total"]["missing_reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r == "harness has no cost source"));
    assert!(
        cost["harnesses"]
            .as_array()
            .unwrap()
            .iter()
            .any(|h| h == "opencode"),
        "opencode stays an active harness column — just never on the model axis"
    );
}

#[tokio::test]
async fn by_model_ventilates_observed_models_requested_fallback_and_node_pairs() {
    if !tmux_available() {
        return; // the node must actually spawn to freeze model/effort/session
    }
    // The home override puts `~/.claude` under the test's own tempdir —
    // hermetic, no real `$HOME` touched (the seam the transcript plants read).
    let daemon = TestDaemon::spawn_with_home_override(seed, Some("exec sleep 600".to_string()))
        .await
        .unwrap();

    // Run A: pinned `claude-sonnet-5`, observed per message — plus a subagent
    // turn on opus-4-8 in the SAME session (ADR-0065 §3: one execution, two
    // buckets, each coherent with its own cost).
    let run_a = create_run(&daemon, "bymodel-a").await;
    wait_for_started(&daemon, &run_a, "worker").await;
    let sid_a = daemon.pinned_session_id(&run_a, "worker").await;
    // opus-4-8 5/25 · 1_000_000 in → $5.00 ; sonnet-5 3/15 · 1_000_000 → $3.00.
    plant_session(
        &daemon,
        &run_a,
        &sid_a,
        &[
            assistant("m1", "claude-sonnet-5", 1_000_000),
            assistant("m2", "claude-opus-4-8", 1_000_000),
        ],
    );

    // Run B: pinned the ALIAS `opus`, no transcript — the source is mute, the
    // requested id falls back and is marked `requested` on the wire.
    let run_b = create_run(&daemon, "bymodel-b").await;
    wait_for_started(&daemon, &run_b, "worker").await;

    let cost = stats_cost(&daemon).await;
    let models = cost["by_model"].as_array().expect("by_model on the wire");
    let ids: Vec<&str> = models.iter().filter_map(|m| m["id"].as_str()).collect();
    assert_eq!(
        ids,
        vec!["claude-opus-4-8", "claude-sonnet-5", "opus"],
        "ranked by cost; the alias pin and the observed full id are two rows"
    );

    let opus = &models[0];
    assert_eq!(opus["provenance"], "observed");
    assert_eq!(opus["usd"], 5.0);
    assert_eq!(opus["executions"], 1);
    assert_eq!(opus["average_usd"], 5.0);
    // Model → Effort → Pipeline → Node, ids verbatim at every level.
    let efforts = opus["efforts"].as_array().unwrap();
    assert_eq!(efforts.len(), 1);
    assert_eq!(efforts[0]["id"], "high");
    assert_eq!(
        efforts[0]["provenance"], "requested",
        "claude's source never writes the effort"
    );
    let pipelines = efforts[0]["pipelines"].as_array().unwrap();
    assert_eq!(pipelines[0]["id"], "bymodel-a");
    assert!(pipelines[0]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|n| n["name"] == "Worker"));

    let sonnet = &models[1];
    assert_eq!(sonnet["provenance"], "observed");
    assert_eq!(sonnet["usd"], 3.0);

    let alias = &models[2];
    assert_eq!(alias["id"], "opus");
    assert_eq!(
        alias["provenance"], "requested",
        "a mute source marks the fallback"
    );
    assert!(alias["usd"].is_null(), "no reading, never an invented $0");
    assert_eq!(alias["unknown"], 1);

    // The Node leaves of by_pipeline end the drill with model × effort pairs.
    let pipeline = cost["by_pipeline"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "bymodel-a")
        .expect("run A's pipeline");
    let worker = pipeline["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["name"] == "Worker")
        .expect("the worker Node");
    let pairs = worker["models"]
        .as_array()
        .expect("Node leaves carry pairs");
    let pair_ids: Vec<&str> = pairs.iter().filter_map(|p| p["model"].as_str()).collect();
    assert_eq!(
        pair_ids,
        vec!["claude-opus-4-8", "claude-sonnet-5"],
        "the two-model session ventilates into two pairs"
    );
    for pair in pairs {
        assert_eq!(pair["effort"], "high");
        assert_eq!(pair["effort_provenance"], "requested");
        assert_eq!(pair["model_provenance"], "observed");
        assert_eq!(pair["executions"], 1, "the execution counts in each bucket");
    }
    let opus_pair = pairs
        .iter()
        .find(|p| p["model"] == "claude-opus-4-8")
        .unwrap();
    assert_eq!(opus_pair["usd"], 5.0);

    // The pipeline axes stay untouched in shape: the totals still sum both runs.
    let total = &cost["total"];
    assert_eq!(total["usd"], 8.0);
    // Two honest gaps: run B's unreadable node, and the infrastructure slice
    // neither run can attribute (each node's transcript owns its lines).
    assert_eq!(total["unknown"], 2);
}
