//! API 0.8 — **the launch of a Node** (CORE-4), on a real daemon with the test
//! extension: who created the Run, the Trigger's last editor, the variables and
//! the preamble an extension adds to a Node, the transcripts under its
//! `CLAUDE_CONFIG_DIR`, and a Trigger an extension suspends.

use std::path::{Path, PathBuf};
use std::time::Duration;

use pdo::extension_api::Actor;
use serde_json::{json, Value};

use crate::common::flow::{create_run, git_init, post_json, seed_solo, wait_run};
use crate::common::{ensure_pdo_on_path, TestDaemon};
use crate::test_extension::{Control, Refusal, TestExtension, ACTOR_HEADER};

/// `start → notify (script) → end`; the script writes what it sees.
const SCRIPT_YAML: &str = r#"name: env-script
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: notify
    name: notify
    type: script
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: notify, port: in }
  - source: { node: start, port: user_prompt }
    target: { node: end, port: result }
"#;

fn env(pairs: &[(&str, &str)]) -> pdo::extension_api::NodeEnv {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn alice() -> Actor {
    Actor::Identified {
        namespace: "test".into(),
        id: "alice".into(),
    }
}

/// A daemon with the test extension, its agents running `tail` instead of a
/// harness.
async fn daemon_with<F>(setup: F, tail: Option<String>) -> (TestDaemon, Control)
where
    F: FnOnce(&Path) -> anyhow::Result<()>,
{
    let (ext, control) = TestExtension::new(Refusal::Nothing);
    let daemon = TestDaemon::spawn_configured(setup, move |root, config| {
        if let Some(tail) = tail {
            config.tmux_cmd_override = Some(tail);
        }
        config.sandbox_home_override = Some(root.join("home"));
        pdo::compose(config, ext.boxed(), None).unwrap();
    })
    .await
    .unwrap();
    (daemon, control)
}

/// Poll until `file` exists; its text.
async fn read_when_written(file: &Path) -> String {
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    loop {
        if let Ok(text) = std::fs::read_to_string(file) {
            if !text.is_empty() {
                return text;
            }
        }
        assert!(
            std::time::Instant::now() < deadline,
            "{} never written",
            file.display()
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// The prompt file a node's session read, under the Run's directory.
fn prompt_file(repo: &Path, run_id: &str, node: &str) -> Option<PathBuf> {
    fn walk(dir: &Path, name: &str) -> Option<PathBuf> {
        for entry in std::fs::read_dir(dir).ok()?.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(found) = walk(&path, name) {
                    return Some(found);
                }
            } else if path.file_name().is_some_and(|f| f == name) {
                return Some(path);
            }
        }
        None
    }
    walk(
        &repo.join(".pdo/runs").join(run_id),
        &format!("{node}-iter-1.md"),
    )
}

async fn node_started(daemon: &TestDaemon, run_id: &str, node: &str) -> Value {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let events = daemon.get_json(&format!("/runs/{run_id}/events")).await;
        if let Some(event) = events.as_array().and_then(|events| {
            events
                .iter()
                .find(|e| e["kind"] == "node_started" && e["node_id"] == node)
                .cloned()
        }) {
            return event;
        }
        assert!(std::time::Instant::now() < deadline, "{node} never started");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test]
async fn a_script_node_gets_the_extension_variables_but_never_over_the_core_ones() {
    ensure_pdo_on_path();
    let out = std::sync::Arc::new(std::sync::Mutex::new(PathBuf::new()));
    let seen = out.clone();
    let (daemon, control) = daemon_with(
        move |repo| {
            let file = repo.join("script-env.txt");
            *seen.lock().unwrap() = file.clone();
            let dir = repo.join(".pdo/pipelines");
            std::fs::create_dir_all(dir.join("env-script.prompts"))?;
            std::fs::write(dir.join("env-script.yaml"), SCRIPT_YAML)?;
            std::fs::write(
                dir.join("env-script.prompts/notify.md"),
                format!(
                    "#!/usr/bin/env bash\nprintf 'FIXTURE_HELLO=%s\\nRUN=%s\\n' \
                     \"$FIXTURE_HELLO\" \"$PDO_RUN_ID\" > '{}'\n",
                    file.display()
                ),
            )?;
            git_init(repo)
        },
        None,
    )
    .await;
    control
        .launch
        .set_env(env(&[("FIXTURE_HELLO", "monde"), ("PDO_RUN_ID", "hijack")]));
    control.launch.set_preamble(Some("Ligne de fixture".into()));

    let run_id = create_run(&daemon, "env-script").await;
    let file = out.lock().unwrap().clone();
    let text = read_when_written(&file).await;

    assert!(text.contains("FIXTURE_HELLO=monde"), "{text}");
    assert!(
        text.contains(&format!("RUN={run_id}")),
        "the core's variable is never overwritten: {text}"
    );
    let launches = control.launch.launches();
    let asked: Vec<&str> = launches.iter().filter_map(|l| l["hook"].as_str()).collect();
    assert_eq!(
        asked,
        vec!["node_env"],
        "a script reads no preamble: never asked for one"
    );
    let launch = &launches[0]["launch"];
    assert_eq!(launch["run_id"], run_id.as_str());
    assert_eq!(launch["node_id"], "notify");
    assert_eq!(launch["kind"], "script");
    assert_eq!(launch["created_by"], json!({ "kind": "local_operator" }));
    assert_eq!(launch["repository"], daemon.target_repo().as_str());
}

#[tokio::test]
async fn an_agent_reads_the_extension_preamble_first_and_carries_its_variables() {
    let marker = std::sync::Arc::new(std::sync::Mutex::new(PathBuf::new()));
    let seen = marker.clone();
    let tail = {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let file =
            std::env::temp_dir().join(format!("pdo-agent-env-{}-{nanos}.txt", std::process::id()));
        *seen.lock().unwrap() = file.clone();
        format!("env > '{}'; exec sleep 600", file.display())
    };
    let (daemon, control) =
        daemon_with(|repo| seed_solo(repo, &[("solo", false)]), Some(tail)).await;
    let config_dir = daemon.repo_root().join("home/iam/alice");
    control.launch.set_env(env(&[
        ("FIXTURE_HELLO", "monde"),
        ("CLAUDE_CONFIG_DIR", &config_dir.to_string_lossy()),
    ]));
    control.launch.set_preamble(Some("Ligne de fixture".into()));

    let run_id = create_run(&daemon, "solo").await;
    let file = marker.lock().unwrap().clone();
    let env_text = read_when_written(&file).await;
    assert!(env_text.contains("FIXTURE_HELLO=monde"), "{env_text}");
    assert!(
        env_text.contains(&format!("PDO_RUN_ID={run_id}")),
        "{env_text}"
    );

    let prompt = prompt_file(daemon.repo_root(), &run_id, "worker").expect("the prompt is written");
    let prompt = std::fs::read_to_string(prompt).unwrap();
    assert!(
        prompt.starts_with("Ligne de fixture\n\n# PDO Runtime Preamble"),
        "{prompt}"
    );

    // The node's `CLAUDE_CONFIG_DIR` is frozen on its start, where its cost
    // and its liveness are read.
    let started = node_started(&daemon, &run_id, "worker").await;
    assert_eq!(
        started["payload"]["claude_config_dir"],
        config_dir.to_string_lossy().as_ref(),
        "{started}"
    );
    let session_id = started["payload"]["session_id"]
        .as_str()
        .unwrap()
        .to_string();
    let worktree = daemon
        .repo_root()
        .join(".pdo/runs")
        .join(&run_id)
        .join("worktree");
    let project =
        config_dir
            .join("projects")
            .join(pdo_harness::claude_transcript::encode_working_dir(
                &worktree,
            ));
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join(format!("{session_id}.jsonl")),
        "{\"type\":\"assistant\",\"requestId\":\"r1\",\"message\":{\"id\":\"m1\",\
         \"model\":\"claude-opus-4-8\",\"usage\":{\"input_tokens\":1000000,\"output_tokens\":0}}}\n",
    )
    .unwrap();
    let run = wait_run(
        &daemon,
        &run_id,
        "the cost under CLAUDE_CONFIG_DIR",
        |run| run["cost"]["usd"].as_f64().is_some_and(|usd| usd > 0.0),
    )
    .await;
    assert!(
        (run["cost"]["usd"].as_f64().unwrap() - 5.0).abs() < 1e-9,
        "{run}"
    );
}

#[tokio::test]
async fn a_run_records_who_created_it() {
    let (daemon, control) = daemon_with(|repo| seed_solo(repo, &[("solo", false)]), None).await;
    let resp = reqwest::Client::new()
        .post(format!("{}/runs", daemon.url()))
        .header(ACTOR_HEADER, "alice")
        .json(&json!({ "pipeline": "solo", "input": "go", "target_repo": daemon.target_repo() }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    let run_id = resp.json::<Value>().await.unwrap()["run_id"]
        .as_str()
        .unwrap()
        .to_string();

    let run = daemon.get_json(&format!("/runs/{run_id}")).await;
    assert_eq!(run["created_by"], json!(alice()), "{run}");
    assert!(run.get("trigger_editor").is_none(), "{run}");
    let runs = daemon.get_json("/runs").await;
    let listed = runs
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["run_id"] == run_id.as_str())
        .unwrap();
    assert_eq!(listed["created_by"], json!(alice()));

    node_started(&daemon, &run_id, "worker").await;
    let launch = control
        .launch
        .launches()
        .into_iter()
        .find(|l| l["hook"] == "node_preamble")
        .expect("an agent's preamble is asked for");
    assert_eq!(launch["launch"]["created_by"], json!(alice()));
    assert_eq!(launch["launch"]["kind"], "agent");
}

async fn create_trigger_as(daemon: &TestDaemon, who: &str) -> Value {
    let resp = reqwest::Client::new()
        .post(format!("{}/triggers", daemon.url()))
        .header(ACTOR_HEADER, who)
        .json(&json!({
            "name": "Weekly dependency bump",
            "pipeline_id": "solo",
            "cron": "0 7 * * 1",
            "input_template": "go",
            "target_repo": daemon.target_repo(),
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    resp.json().await.unwrap()
}

#[tokio::test]
async fn a_fired_run_is_created_by_its_trigger_and_carries_its_last_editor() {
    let (daemon, control) = daemon_with(|repo| seed_solo(repo, &[("solo", false)]), None).await;
    let trigger = create_trigger_as(&daemon, "alice").await;
    assert_eq!(trigger["last_editor"], json!(alice()), "{trigger}");
    let id = trigger["id"].as_str().unwrap().to_string();

    let (status, body) = post_json(&daemon, &format!("/triggers/{id}/fire"), json!({})).await;
    assert_eq!(status, 200, "{body}");
    let run_id = body["run_id"].as_str().unwrap().to_string();
    let run = daemon.get_json(&format!("/runs/{run_id}")).await;
    assert_eq!(
        run["created_by"],
        json!({ "kind": "trigger", "trigger_id": id })
    );
    assert_eq!(run["trigger_editor"], json!(alice()));

    node_started(&daemon, &run_id, "worker").await;
    let launch = control.launch.launches().pop().unwrap();
    assert_eq!(
        launch["launch"]["trigger"],
        json!({ "id": id, "editor": alice() })
    );
}

#[tokio::test]
async fn a_suspended_trigger_shows_why_and_fires_neither_by_cron_nor_by_run_now() {
    let (daemon, control) = daemon_with(|repo| seed_solo(repo, &[("solo", false)]), None).await;
    let trigger = create_trigger_as(&daemon, "emma").await;
    let id = trigger["id"].as_str().unwrap().to_string();
    control.launch.suspend(&id, "Emma Leroy was disabled");

    let listed = daemon.get_json("/triggers").await;
    assert_eq!(
        listed[0]["suspension"],
        json!({ "extension": "test", "reason": "Emma Leroy was disabled" }),
        "{listed}"
    );
    let asked = control.launch.trigger_asks();
    assert!(
        asked.iter().any(|t| t.id == id
            && t.last_editor
                == Some(Actor::Identified {
                    namespace: "test".into(),
                    id: "emma".into()
                })),
        "the extension is asked with the last editor"
    );

    let (status, body) = post_json(&daemon, &format!("/triggers/{id}/fire"), json!({})).await;
    assert_eq!(status, 409, "{body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("Emma Leroy was disabled"),
        "{body}"
    );

    daemon.force_trigger_due(&id).await;
    daemon.run_trigger_tick().await;
    let fires = daemon.get_json(&format!("/triggers/{id}/fires")).await;
    assert_eq!(fires[0]["outcome"], "skipped-suspended", "{fires}");
    assert_eq!(fires[0]["reason"], "Emma Leroy was disabled");
    let runs = daemon.get_json("/runs").await;
    assert!(
        runs.as_array()
            .unwrap()
            .iter()
            .all(|r| r["triggered_by"] != id.as_str()),
        "no Run fired: {runs}"
    );

    // Lifted: the next read shows it running again, and it fires.
    assert!(control.launch.lift(&id));
    let trigger = daemon.get_json(&format!("/triggers/{id}")).await;
    assert!(trigger.get("suspension").is_none(), "{trigger}");
    let (status, _) = post_json(&daemon, &format!("/triggers/{id}/fire"), json!({})).await;
    assert_eq!(status, 200);
}
