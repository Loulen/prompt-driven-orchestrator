//! The reading guide of a mounted page and the instance's Public URL, on a real
//! daemon: a guide checked at mount time and listed with its mount, the links
//! `pdo page mount` prints, the setting stored and refused, and the address a
//! node session receives in its environment and preamble.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde_json::{json, Value};

use crate::common::flow::{create_run, seed_solo, wait_run};
use crate::common::TestDaemon;

async fn put_settings(daemon: &TestDaemon, body: Value) -> reqwest::Response {
    reqwest::Client::new()
        .put(format!("{}/settings", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap()
}

async fn mount(
    daemon: &TestDaemon,
    name: &str,
    directory: &Path,
    guide: Option<&str>,
) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{}/pages", daemon.url()))
        .json(&json!({ "name": name, "directory": directory, "guide": guide }))
        .send()
        .await
        .unwrap()
}

/// A daemon whose `worker` node is running in a Run, and a page directory.
async fn daemon_with_a_run() -> (TestDaemon, String, tempfile::TempDir) {
    let daemon = TestDaemon::spawn(|repo| seed_solo(repo, &[("solo", false)]))
        .await
        .unwrap();
    let run_id = create_run(&daemon, "solo").await;
    wait_run(&daemon, &run_id, "worker running", |run| {
        run["nodes"]["worker"]["status"] == "running"
    })
    .await;
    let pages = tempfile::tempdir().unwrap();
    std::fs::write(pages.path().join("index.html"), "<h2 id='a'>A</h2>").unwrap();
    (daemon, run_id, pages)
}

#[tokio::test]
async fn a_guide_is_checked_mounted_and_listed_with_the_public_url() {
    let (daemon, run_id, pages) = daemon_with_a_run().await;
    assert_eq!(
        put_settings(&daemon, json!({ "public_url": "https://pdo.example.net/" }))
            .await
            .status(),
        200
    );
    let guide = format!(
        "- title: The Run\n  text: Where the page came from.\n  link: '#/runs/{run_id}'\n\
         - title: The code\n  link: /runs/{run_id}/review?file=src%2Fa.ts\n\
         - title: Option A\n  link: '#a'\n"
    );

    let response = mount(&daemon, "guided", pages.path(), Some(&guide)).await;
    assert_eq!(response.status(), 201);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["public_url"], "https://pdo.example.net");
    assert_eq!(body["guide"].as_array().unwrap().len(), 3);
    assert_eq!(body["guide"][0]["title"], "The Run");

    let listed = daemon.get_json("/pages").await;
    assert_eq!(listed[0]["name"], "guided");
    assert_eq!(listed[0]["guide"][2]["link"], "#a");
    // The page itself is served as before.
    let page = reqwest::get(format!("{}/pages/guided/", daemon.url()))
        .await
        .unwrap();
    assert_eq!(page.status(), 200);

    // A guide whose link leads nowhere is refused, naming why; nothing is mounted.
    let refused = mount(
        &daemon,
        "broken",
        pages.path(),
        Some("- title: Gone\n  link: '#/runs/no-such-run'\n"),
    )
    .await;
    assert_eq!(refused.status(), 400);
    let error: Value = refused.json().await.unwrap();
    assert_eq!(error["error"], "guide entry 1: Run `no-such-run` not found");
    let names: Vec<Value> = daemon
        .get_json("/pages")
        .await
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["name"].clone())
        .collect();
    assert_eq!(names, vec![json!("guided")]);
}

#[tokio::test]
async fn a_page_without_a_guide_is_unchanged() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let pages = tempfile::tempdir().unwrap();
    std::fs::write(pages.path().join("index.html"), "plain").unwrap();
    let response = mount(&daemon, "plain", pages.path(), None).await;
    assert_eq!(response.status(), 201);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["guide"], Value::Null);
    assert_eq!(body["public_url"], Value::Null, "no Public URL by default");
    let page = reqwest::get(format!("{}/pages/plain/", daemon.url()))
        .await
        .unwrap();
    assert_eq!(page.text().await.unwrap(), "plain");
}

async fn run_cli(daemon: &TestDaemon, args: Vec<String>) -> std::process::Output {
    let daemon_url = daemon.url();
    tokio::task::spawn_blocking(move || {
        Command::new(env!("CARGO_BIN_EXE_pdo"))
            .args(args)
            .env("PDO_DAEMON_URL", daemon_url)
            .env_remove("PDO_RUN_ID")
            .env_remove("PDO_NODE_ID")
            .env_remove("PDO_PUBLIC_URL")
            .output()
            .unwrap()
    })
    .await
    .unwrap()
}

fn mount_args(name: &str, directory: &Path, guide: Option<&Path>) -> Vec<String> {
    let mut args = vec![
        "page".to_string(),
        "mount".to_string(),
        name.to_string(),
        directory.display().to_string(),
    ];
    if let Some(guide) = guide {
        args.extend(["--guide".to_string(), guide.display().to_string()]);
    }
    args
}

#[tokio::test]
async fn page_mount_prints_full_links_under_the_public_url() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let pages = tempfile::tempdir().unwrap();
    std::fs::write(pages.path().join("index.html"), "home").unwrap();
    let guide = pages.path().join("guide.yaml");
    std::fs::write(&guide, "- title: Start\n  link: index.html\n").unwrap();

    // Without a Public URL: the page alone, under the daemon's address.
    let alone = run_cli(&daemon, mount_args("alone", pages.path(), None)).await;
    assert!(alone.status.success(), "{alone:?}");
    let stdout = String::from_utf8_lossy(&alone.stdout);
    assert!(
        stdout.contains(&format!("Mounted alone at {}/pages/alone/", daemon.url())),
        "{stdout}"
    );

    put_settings(&daemon, json!({ "public_url": "https://pdo.example.net" })).await;
    let guided = run_cli(&daemon, mount_args("guided", pages.path(), Some(&guide))).await;
    assert!(guided.status.success(), "{guided:?}");
    let stdout = String::from_utf8_lossy(&guided.stdout);
    assert!(
        stdout.contains("With its guide: https://pdo.example.net/#/pages/guided\n"),
        "{stdout}"
    );
    assert!(
        stdout.contains("Page alone: https://pdo.example.net/pages/guided/\n"),
        "{stdout}"
    );

    // An invalid guide: refused with the daemon's reason, nothing mounted.
    std::fs::write(&guide, "- title: Out\n  link: ../etc/passwd\n").unwrap();
    let refused = run_cli(&daemon, mount_args("refused", pages.path(), Some(&guide))).await;
    assert!(!refused.status.success());
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(
        stderr.contains("guide entry 1: link `../etc/passwd`"),
        "{stderr}"
    );
    let listed = run_cli(&daemon, vec!["page".into(), "list".into()]).await;
    assert!(!String::from_utf8_lossy(&listed.stdout).contains("refused"));
}

#[tokio::test]
async fn the_public_url_setting_is_stored_normalized_cleared_or_refused() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let field = |settings: &Value| settings["public_url"].clone();
    let initial = daemon.get_json("/settings").await;
    assert_eq!(field(&initial)["effective"], Value::Null);
    assert_eq!(field(&initial)["source"], "default");

    let stored: Value = put_settings(&daemon, json!({ "public_url": " http://10.0.0.4:5272/ " }))
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(field(&stored)["effective"], "http://10.0.0.4:5272");
    assert_eq!(field(&stored)["source"], "stored");

    let refused = put_settings(&daemon, json!({ "public_url": "pdo.example.net" })).await;
    assert_eq!(refused.status(), 400);
    let error: Value = refused.json().await.unwrap();
    assert!(
        error["error"].as_str().unwrap().contains("http://"),
        "{error}"
    );
    assert_eq!(
        field(&daemon.get_json("/settings").await)["effective"],
        "http://10.0.0.4:5272",
        "a refusal stores nothing"
    );

    let cleared: Value = put_settings(&daemon, json!({ "public_url": "" }))
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(field(&cleared)["effective"], Value::Null);
}

/// Poll until `file` holds text.
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

/// The prompt file `worker` read, under the Run's directory.
fn prompt_of(repo: &Path, run_id: &str) -> String {
    fn walk(dir: &Path) -> Option<PathBuf> {
        for entry in std::fs::read_dir(dir).ok()?.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(found) = walk(&path) {
                    return Some(found);
                }
            } else if path.file_name().is_some_and(|f| f == "worker-iter-1.md") {
                return Some(path);
            }
        }
        None
    }
    let file = walk(&repo.join(".pdo/runs").join(run_id)).expect("the prompt is written");
    std::fs::read_to_string(file).unwrap()
}

#[tokio::test]
async fn a_node_launched_after_the_setting_receives_the_public_url() {
    let env_file = std::env::temp_dir().join(format!(
        "pdo-public-url-env-{}-{}.txt",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let daemon = TestDaemon::spawn_with_override(
        |repo| seed_solo(repo, &[("solo", false)]),
        // Written aside then renamed: the file appears whole, never half
        // written to a reader polling for it.
        Some(format!(
            "env > '{0}.part' && mv '{0}.part' '{0}'; exec sleep 600",
            env_file.display()
        )),
    )
    .await
    .unwrap();
    // Set while the daemon runs: no restart, the next launch reads it.
    put_settings(&daemon, json!({ "public_url": "https://pdo.example.net" })).await;

    let run_id = create_run(&daemon, "solo").await;
    let env = read_when_written(&env_file).await;
    let _ = std::fs::remove_file(&env_file);
    assert!(
        env.lines()
            .any(|line| line == "PDO_PUBLIC_URL=https://pdo.example.net"),
        "{env}"
    );
    let prompt = prompt_of(daemon.repo_root(), &run_id);
    assert!(prompt.contains("## Public URL"), "{prompt}");
    assert!(
        prompt.contains(&format!("https://pdo.example.net/#/runs/{run_id}")),
        "{prompt}"
    );
}
