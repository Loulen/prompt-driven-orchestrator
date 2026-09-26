use std::process::Command;

use crate::common::TestDaemon;
use reqwest::header::{CONTENT_TYPE, LOCATION};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn mount(daemon: &TestDaemon, name: &str, directory: &std::path::Path) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{}/pages", daemon.url()))
        .json(&json!({ "name": name, "directory": directory }))
        .send()
        .await
        .unwrap()
}

async fn raw_get(daemon: &TestDaemon, path: &str) -> String {
    let mut stream = tokio::net::TcpStream::connect(daemon.addr).await.unwrap();
    stream
        .write_all(
            format!(
                "GET {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
                daemon.addr
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    response
}

async fn run_cli(daemon: &TestDaemon, args: Vec<String>) -> std::process::Output {
    let daemon_url = daemon.url();
    tokio::task::spawn_blocking(move || {
        Command::new(env!("CARGO_BIN_EXE_pdo"))
            .args(args)
            .env("PDO_DAEMON_URL", daemon_url)
            .env_remove("PDO_RUN_ID")
            .env_remove("PDO_NODE_ID")
            .output()
            .unwrap()
    })
    .await
    .unwrap()
}

fn seed_running_pipeline(repo: &std::path::Path) -> anyhow::Result<()> {
    let pipeline_dir = repo.join(".pdo/pipelines");
    std::fs::create_dir_all(pipeline_dir.join("page-owner.prompts"))?;
    std::fs::write(
        pipeline_dir.join("page-owner.yaml"),
        r#"name: page-owner
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs: [{ name: user_prompt }]
  - id: worker
    name: worker
    type: agent
    isolated_worktree: false
    inputs: [{ name: task }]
    outputs: [{ name: result }]
  - id: end
    name: End
    type: end
    inputs: [{ name: result }]
edges:
  - source: { node: start, port: user_prompt }
    target: { node: worker, port: task }
"#,
    )?;
    std::fs::write(
        pipeline_dir.join("page-owner.prompts/worker.md"),
        "Build the page.",
    )?;
    let run = |args: &[&str]| -> anyhow::Result<()> {
        let output = Command::new("git").args(args).current_dir(repo).output()?;
        anyhow::ensure!(
            output.status.success(),
            "git failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
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

async fn create_running_run(daemon: &TestDaemon) -> String {
    let response = reqwest::Client::new()
        .post(format!("{}/runs", daemon.url()))
        .json(&json!({
            "pipeline": "page-owner",
            "input": "make a page",
            "target_repo": daemon.target_repo(),
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201);
    let run_id = response.json::<serde_json::Value>().await.unwrap()["run_id"]
        .as_str()
        .unwrap()
        .to_string();
    for _ in 0..100 {
        let run: serde_json::Value = reqwest::get(format!("{}/runs/{run_id}", daemon.url()))
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        if run["nodes"]["worker"]["status"] == "running" {
            return run_id;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("worker did not start");
}

fn content_type(response: &reqwest::Response) -> &str {
    response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
}

#[tokio::test]
async fn mounted_pages_serve_live_html_files_and_index() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let pages = tempfile::tempdir().unwrap();
    std::fs::write(
        pages.path().join("index.html"),
        "<!doctype html><h1>Home</h1>",
    )
    .unwrap();
    std::fs::write(
        pages.path().join("report.html"),
        "<!doctype html><h1>First</h1>",
    )
    .unwrap();
    std::fs::write(
        pages.path().join("Skills Bank.html"),
        "<!doctype html><h1>Bank</h1>",
    )
    .unwrap();

    let response = mount(&daemon, "skills_bank", pages.path()).await;
    assert_eq!(response.status(), 201);
    let mounted: serde_json::Value = response.json().await.unwrap();
    assert_eq!(
        mounted["directory"],
        pages.path().canonicalize().unwrap().to_str().unwrap()
    );

    let root = reqwest::get(format!("{}/pages/skills_bank/", daemon.url()))
        .await
        .unwrap();
    assert_eq!(root.status(), 200);
    assert!(content_type(&root).starts_with("text/html"));
    assert!(root.text().await.unwrap().contains("Home"));

    let report_url = format!("{}/pages/skills_bank/report.html", daemon.url());
    let report = reqwest::get(&report_url).await.unwrap();
    assert_eq!(report.status(), 200);
    assert!(content_type(&report).starts_with("text/html"));
    assert!(report.text().await.unwrap().contains("First"));

    let spaced = reqwest::get(format!(
        "{}/pages/skills_bank/Skills%20Bank.html",
        daemon.url()
    ))
    .await
    .unwrap();
    assert_eq!(spaced.status(), 200);
    assert!(content_type(&spaced).starts_with("text/html"));

    std::fs::write(
        pages.path().join("report.html"),
        "<!doctype html><h1>Edited</h1>",
    )
    .unwrap();
    let edited = reqwest::get(report_url).await.unwrap();
    assert!(edited.text().await.unwrap().contains("Edited"));
}

#[tokio::test]
async fn mount_root_redirects_to_the_trailing_slash_form() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let pages = tempfile::tempdir().unwrap();
    std::fs::write(pages.path().join("index.html"), "home").unwrap();
    assert_eq!(mount(&daemon, "demo", pages.path()).await.status(), 201);

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let response = client
        .get(format!("{}/pages/demo", daemon.url()))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 308);
    assert_eq!(response.headers().get(LOCATION).unwrap(), "/pages/demo/");
}

#[tokio::test]
async fn missing_and_traversal_requests_are_plain_404s() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let pages = tempfile::tempdir().unwrap();
    std::fs::write(pages.path().join("index.html"), "home").unwrap();
    assert_eq!(mount(&daemon, "demo", pages.path()).await.status(), 201);

    let missing = reqwest::get(format!("{}/pages/demo/missing.html", daemon.url()))
        .await
        .unwrap();
    assert_eq!(missing.status(), 404);
    assert!(!content_type(&missing).starts_with("text/html"));

    let traversal = raw_get(&daemon, "/pages/demo/%2e%2e/%2e%2e/etc/passwd").await;
    assert!(
        traversal.starts_with("HTTP/1.1 404"),
        "traversal must be rejected: {traversal}"
    );
    assert!(!traversal
        .to_ascii_lowercase()
        .contains("content-type: text/html"));
}

#[cfg(unix)]
#[tokio::test]
async fn symlinks_cannot_escape_a_mounted_directory() {
    use std::os::unix::fs::symlink;

    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let pages = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret.html"), "outside").unwrap();
    symlink(outside.path(), pages.path().join("escape")).unwrap();
    assert_eq!(mount(&daemon, "demo", pages.path()).await.status(), 201);

    let response = reqwest::get(format!("{}/pages/demo/escape/secret.html", daemon.url()))
        .await
        .unwrap();
    assert_eq!(response.status(), 404);
    assert!(!content_type(&response).starts_with("text/html"));
}

#[cfg(unix)]
#[tokio::test]
async fn replacing_a_mount_ancestor_with_a_symlink_cannot_escape() {
    use std::os::unix::fs::symlink;

    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let parent = tempfile::tempdir().unwrap();
    let ancestor = parent.path().join("ancestor");
    let mounted = ancestor.join("site");
    std::fs::create_dir_all(&mounted).unwrap();
    std::fs::write(mounted.join("secret.html"), "original").unwrap();
    assert_eq!(mount(&daemon, "demo", &mounted).await.status(), 201);

    std::fs::rename(&ancestor, parent.path().join("original-ancestor")).unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::create_dir(outside.path().join("site")).unwrap();
    std::fs::write(outside.path().join("site/secret.html"), "outside").unwrap();
    symlink(outside.path(), &ancestor).unwrap();

    let response = reqwest::get(format!("{}/pages/demo/secret.html", daemon.url()))
        .await
        .unwrap();
    assert_eq!(response.status(), 404);
}

#[tokio::test]
async fn mount_refusals_have_the_documented_statuses() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let root = tempfile::tempdir().unwrap();
    let valid = root.path().join("valid");
    let file = root.path().join("file.txt");
    let artifacts = root.path().join(".pdo/artifacts/demo");
    std::fs::create_dir(&valid).unwrap();
    std::fs::write(&file, "not a directory").unwrap();
    std::fs::create_dir_all(&artifacts).unwrap();

    assert_eq!(mount(&daemon, "Bad Name", &valid).await.status(), 400);
    assert_eq!(
        mount(&daemon, "missing", &root.path().join("missing"))
            .await
            .status(),
        400
    );
    assert_eq!(mount(&daemon, "file", &file).await.status(), 400);
    assert_eq!(mount(&daemon, "artifacts", &artifacts).await.status(), 400);
    assert_eq!(mount(&daemon, "demo", &valid).await.status(), 201);
    assert_eq!(mount(&daemon, "demo", &valid).await.status(), 409);
}

#[tokio::test]
async fn list_and_unmount_expose_the_mount_lifecycle() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let pages = tempfile::tempdir().unwrap();
    std::fs::write(pages.path().join("index.html"), "home").unwrap();
    assert_eq!(mount(&daemon, "demo", pages.path()).await.status(), 201);

    let listed: serde_json::Value = reqwest::get(format!("{}/pages", daemon.url()))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(listed.as_array().unwrap().len(), 1);
    assert_eq!(listed[0]["name"], "demo");
    assert!(listed[0]["run_id"].is_null());
    assert!(listed[0]["created_at"].is_string());

    let removed = reqwest::Client::new()
        .delete(format!("{}/pages/demo", daemon.url()))
        .send()
        .await
        .unwrap();
    assert_eq!(removed.status(), 200);
    assert_eq!(
        reqwest::get(format!("{}/pages/demo/", daemon.url()))
            .await
            .unwrap()
            .status(),
        404
    );
    let listed: serde_json::Value = reqwest::get(format!("{}/pages", daemon.url()))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(listed.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn operator_mutations_are_written_to_the_audit_log() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let pages = tempfile::tempdir().unwrap();
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/pages", daemon.url()))
        .header("X-PDO-Actor", "cli")
        .json(&json!({ "name": "demo", "directory": pages.path() }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201);
    assert_eq!(
        client
            .delete(format!("{}/pages/demo", daemon.url()))
            .header("X-PDO-Actor", "cli")
            .send()
            .await
            .unwrap()
            .status(),
        200
    );

    let audit: serde_json::Value =
        reqwest::get(format!("{}/audit?target_kind=page_mount", daemon.url()))
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
    assert_eq!(audit.as_array().unwrap().len(), 2);
    assert_eq!(audit[0]["action"], "page_mount.unmounted");
    assert_eq!(audit[0]["actor_hint"], "cli");
    assert_eq!(audit[1]["action"], "page_mount.mounted");
}

#[tokio::test]
async fn archiving_a_run_drops_its_mount_and_keeps_the_run_event() {
    let daemon = TestDaemon::spawn(seed_running_pipeline).await.unwrap();
    let run_id = create_running_run(&daemon).await;
    let pages = tempfile::tempdir().unwrap();
    std::fs::write(pages.path().join("index.html"), "owned").unwrap();

    let mounted = reqwest::Client::new()
        .post(format!("{}/pages", daemon.url()))
        .header("X-PDO-Session-Run-Id", &run_id)
        .header("X-PDO-Session-Node-Id", "worker")
        .json(&json!({ "name": "owned", "directory": pages.path() }))
        .send()
        .await
        .unwrap();
    assert_eq!(mounted.status(), 201);
    let body: serde_json::Value = mounted.json().await.unwrap();
    assert_eq!(body["run_id"], run_id);

    let events: serde_json::Value = reqwest::get(format!("{}/runs/{run_id}/events", daemon.url()))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(events.as_array().unwrap().iter().any(|event| {
        event["kind"] == "command_issued" && event["payload"]["command"] == "page_mount.mounted"
    }));

    let archived = reqwest::Client::new()
        .post(format!("{}/runs/{run_id}/commands", daemon.url()))
        .json(&json!({ "kind": "cleanup_run" }))
        .send()
        .await
        .unwrap();
    assert_eq!(archived.status(), 200);
    assert_eq!(
        reqwest::get(format!("{}/pages/owned/", daemon.url()))
            .await
            .unwrap()
            .status(),
        404
    );

    let recreated = reqwest::Client::new()
        .post(format!("{}/pages", daemon.url()))
        .header("X-PDO-Session-Run-Id", &run_id)
        .header("X-PDO-Session-Node-Id", "worker")
        .json(&json!({ "name": "late", "directory": pages.path() }))
        .send()
        .await
        .unwrap();
    assert_eq!(recreated.status(), 403);
    let mounts: serde_json::Value = reqwest::get(format!("{}/pages", daemon.url()))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(mounts.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn mounts_survive_a_daemon_restart() {
    let root = tempfile::tempdir().unwrap();
    let pages = tempfile::tempdir().unwrap();
    std::fs::write(pages.path().join("index.html"), "persistent").unwrap();

    let first = TestDaemon::spawn_at(root.path()).await.unwrap();
    assert_eq!(mount(&first, "demo", pages.path()).await.status(), 201);
    drop(first);

    let second = TestDaemon::spawn_at(root.path()).await.unwrap();
    let response = reqwest::get(format!("{}/pages/demo/", second.url()))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert!(response.text().await.unwrap().contains("persistent"));
}

#[tokio::test]
async fn page_cli_mounts_lists_unmounts_and_surfaces_refusals() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let pages = tempfile::tempdir().unwrap();
    std::fs::write(pages.path().join("index.html"), "home").unwrap();
    let mounted = run_cli(
        &daemon,
        vec![
            "page".into(),
            "mount".into(),
            "demo".into(),
            pages.path().display().to_string(),
        ],
    )
    .await;
    assert!(mounted.status.success());
    let stdout = String::from_utf8_lossy(&mounted.stdout);
    assert!(stdout.contains("Mounted demo"));
    assert!(stdout.contains(pages.path().canonicalize().unwrap().to_str().unwrap()));

    let listed = run_cli(&daemon, vec!["page".into(), "list".into()]).await;
    assert!(listed.status.success());
    assert!(String::from_utf8_lossy(&listed.stdout).contains("demo"));

    let duplicate = run_cli(
        &daemon,
        vec![
            "page".into(),
            "mount".into(),
            "demo".into(),
            pages.path().display().to_string(),
        ],
    )
    .await;
    assert!(!duplicate.status.success());
    assert!(String::from_utf8_lossy(&duplicate.stderr).contains("already mounted"));

    let removed = run_cli(
        &daemon,
        vec!["page".into(), "unmount".into(), "demo".into()],
    )
    .await;
    assert!(removed.status.success());
    assert!(String::from_utf8_lossy(&removed.stdout).contains("Unmounted demo"));
}
