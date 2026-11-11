//! A Run in the Projet of its repository, against the real daemon and the
//! real `pdo` binary, with the test extension:
//!
//! - `POST /runs` naming no Projet launches the Run in the one Projet holding
//!   its repository, and asks `run` × `create` there;
//! - a repository of several Projets is refused, naming them; `pdo run
//!   create` adds the `--project` to pass;
//! - a repository of no Projet also asks `repository` × `read` in no Projet:
//!   refused, no Run and no branch;
//! - from a node session of a Run in P, a repository of another Projet is
//!   refused, one of P launches in P.

use std::process::Command;

use pdo::extension_api::{ResourceKind, Verb};
use serde_json::{json, Value};

use crate::common::flow::{git_init, seed_solo};
use crate::common::TestDaemon;
use crate::test_extension::{Control, Refusal, TestExtension};

async fn daemon() -> (TestDaemon, Control) {
    let (ext, control) = TestExtension::new(Refusal::Nothing);
    let daemon =
        TestDaemon::spawn_with_extensions(|repo| seed_solo(repo, &[("solo", false)]), ext.boxed())
            .await
            .unwrap();
    (daemon, control)
}

async fn post(daemon: &TestDaemon, path: &str, body: Value) -> (u16, Value) {
    let resp = reqwest::Client::new()
        .post(format!("{}{path}", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    (status, serde_json::from_str(&text).unwrap_or(Value::Null))
}

async fn get(daemon: &TestDaemon, path: &str) -> Value {
    reqwest::get(format!("{}{path}", daemon.url()))
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

/// The Projet `name` holding `repos`: its id.
async fn projet(daemon: &TestDaemon, name: &str, repos: &[&str]) -> String {
    let (status, project) = post(
        daemon,
        "/projects",
        json!({ "name": name, "members": repos }),
    )
    .await;
    assert_eq!(status, 201, "{project}");
    project["id"].as_str().unwrap().to_string()
}

fn launch(repo: &str) -> Value {
    json!({ "pipeline": "solo", "input": "go", "target_repo": repo })
}

/// A git repository with one commit, kept alive by the returned guard.
fn repository() -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().unwrap();
    git_init(dir.path()).unwrap();
    let path = dir.path().to_string_lossy().into_owned();
    (dir, path)
}

/// `pdo run create` against `daemon`, out of any session, or from the node
/// session `session`.
async fn pdo_run_create(
    daemon: &TestDaemon,
    args: &[&str],
    session: Option<(&str, &str)>,
) -> (Option<i32>, String, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_pdo"));
    cmd.args(["run", "create"]).args(args);
    cmd.env("PDO_DAEMON_URL", daemon.url());
    cmd.env_remove("PDO_RUN_ID");
    cmd.env_remove("PDO_NODE_ID");
    if let Some((run_id, node_id)) = session {
        cmd.env("PDO_RUN_ID", run_id);
        cmd.env("PDO_NODE_ID", node_id);
    }
    let output = tokio::task::spawn_blocking(move || cmd.output().unwrap())
        .await
        .unwrap();
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn run_id_of(stdout: &str) -> String {
    stdout
        .trim()
        .strip_prefix("Run ")
        .and_then(|rest| rest.split_whitespace().next())
        .unwrap_or_else(|| panic!("no run id in {stdout}"))
        .to_string()
}

async fn wait_node_running(daemon: &TestDaemon, run_id: &str) {
    for _ in 0..100 {
        if get(daemon, &format!("/runs/{run_id}")).await["nodes"]["worker"]["status"] == "running" {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("the worker of {run_id} never ran");
}

#[tokio::test]
async fn a_run_naming_no_projet_is_in_the_one_holding_its_repository() {
    let (daemon, control) = daemon().await;
    let (_q_repo, q_repo) = repository();
    let p = projet(&daemon, "Payments", &[&daemon.target_repo()]).await;
    let q = projet(&daemon, "Risk", &[&q_repo]).await;

    let (code, stdout, stderr) = pdo_run_create(
        &daemon,
        &[
            "solo",
            "--input",
            "go",
            "--target-repo",
            &daemon.target_repo(),
        ],
        None,
    )
    .await;
    assert_eq!(code, Some(0), "{stderr}");
    let run_id = run_id_of(&stdout);
    assert_eq!(
        get(&daemon, &format!("/runs/{run_id}")).await["project_id"],
        json!(p)
    );
    let creations: Vec<Vec<String>> = control
        .asked()
        .into_iter()
        .filter(|(_, op)| op.resource == ResourceKind::Run && op.verb == Verb::Create)
        .map(|(_, op)| op.projects)
        .collect();
    assert_eq!(creations, vec![vec![p.clone()]]);

    // The same repository in Risk too: the launch names its Projet.
    let (status, _) = post(
        &daemon,
        &format!("/projects/{q}/members"),
        json!({ "path": daemon.target_repo() }),
    )
    .await;
    assert_eq!(status, 200);
    let (status, refusal) = post(&daemon, "/runs", launch(&daemon.target_repo())).await;
    assert_eq!(status, 400, "{refusal}");
    assert_eq!(
        refusal["error"],
        "This repository is in Payments and Risk: pick its Project."
    );
    let (code, _, stderr) = pdo_run_create(
        &daemon,
        &[
            "solo",
            "--input",
            "go",
            "--target-repo",
            &daemon.target_repo(),
        ],
        None,
    )
    .await;
    assert_ne!(code, Some(0));
    assert!(
        stderr.contains("This repository is in Payments and Risk: pick its Project.")
            && stderr.contains(&format!("Pass --project <id>: Payments ({p}), Risk ({q}).")),
        "{stderr}"
    );
    let (code, stdout, stderr) = pdo_run_create(
        &daemon,
        &[
            "solo",
            "--input",
            "go",
            "--target-repo",
            &daemon.target_repo(),
            "--project",
            &q,
        ],
        None,
    )
    .await;
    assert_eq!(code, Some(0), "{stderr}");
    assert_eq!(
        get(&daemon, &format!("/runs/{}", run_id_of(&stdout))).await["project_id"],
        json!(q)
    );
}

#[tokio::test]
async fn a_repository_of_no_projet_is_read_from_the_disk() {
    let (daemon, control) = daemon().await;
    let (_loose, loose) = repository();
    let runs_before = get(&daemon, "/runs").await.as_array().unwrap().len();

    control.set(Refusal::parse("repository:read").unwrap());
    let (status, refusal) = post(&daemon, "/runs", launch(&loose)).await;
    assert_eq!(status, 403, "{refusal}");
    assert_eq!(
        refusal["reason"],
        "the test extension refuses read repository"
    );
    assert_eq!(
        get(&daemon, "/runs").await.as_array().unwrap().len(),
        runs_before,
        "no Run"
    );
    let branches = Command::new("git")
        .args(["branch", "--list", "pdo/run-*"])
        .current_dir(&loose)
        .output()
        .unwrap();
    assert!(branches.stdout.is_empty(), "no branch");

    control.set(Refusal::Nothing);
    let (status, created) = post(&daemon, "/runs", launch(&loose)).await;
    assert_eq!(status, 201, "{created}");
    let run_id = created["run_id"].as_str().unwrap();
    assert_eq!(
        get(&daemon, &format!("/runs/{run_id}")).await["project_id"],
        Value::Null
    );
}

#[tokio::test]
async fn from_a_node_session_a_run_works_in_its_parents_projet() {
    let (daemon, _control) = daemon().await;
    let (_q_repo, q_repo) = repository();
    let p = projet(&daemon, "Payments", &[&daemon.target_repo()]).await;
    projet(&daemon, "Risk", &[&q_repo]).await;
    let (status, parent) = post(&daemon, "/runs", launch(&daemon.target_repo())).await;
    assert_eq!(status, 201, "{parent}");
    let parent = parent["run_id"].as_str().unwrap().to_string();
    wait_node_running(&daemon, &parent).await;
    let runs_before = get(&daemon, "/runs").await.as_array().unwrap().len();

    let session = Some((parent.as_str(), "worker"));
    let (code, _, stderr) = pdo_run_create(
        &daemon,
        &["solo", "--input", "go", "--target-repo", &q_repo],
        session,
    )
    .await;
    assert_ne!(code, Some(0));
    assert!(
        stderr.contains("is not a member of project `Payments`"),
        "{stderr}"
    );
    assert_eq!(
        get(&daemon, "/runs").await.as_array().unwrap().len(),
        runs_before,
        "no Run"
    );

    let (code, stdout, stderr) = pdo_run_create(
        &daemon,
        &[
            "solo",
            "--input",
            "go",
            "--target-repo",
            &daemon.target_repo(),
        ],
        session,
    )
    .await;
    assert_eq!(code, Some(0), "{stderr}");
    assert_eq!(
        get(&daemon, &format!("/runs/{}", run_id_of(&stdout))).await["project_id"],
        json!(p)
    );
}
