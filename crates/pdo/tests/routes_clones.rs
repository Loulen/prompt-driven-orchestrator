//! Layer 4 (ADR-0080) — a repository added by its URL (CORE-97): the clones
//! folder setting, the clone of a `file://` repository into it, the Projet it
//! joins, the reuse of a URL already cloned, a cancel and a failure.

use std::path::Path;
use std::time::Duration;

use crate::common::flow;
use crate::common::TestDaemon;

fn seed(repo: &Path) -> anyhow::Result<()> {
    flow::git_init(repo)
}

/// A repository with two commits, to clone by its `file://` URL.
fn origin(dir: &Path) -> String {
    flow::git_init(dir).unwrap();
    std::fs::write(dir.join("second.txt"), "2\n").unwrap();
    flow::git(dir, &["add", "."]);
    flow::git(dir, &["commit", "-m", "second"]);
    format!("file://{}", dir.display())
}

async fn set_clones_folder(daemon: &TestDaemon, folder: &str) {
    let resp = reqwest::Client::new()
        .put(format!("{}/settings", daemon.url()))
        .json(&serde_json::json!({ "clones_folder": folder }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        200,
        "{}",
        resp.text().await.unwrap_or_default()
    );
}

/// The clone `id`, once it no longer runs.
async fn settled(daemon: &TestDaemon, id: &str) -> serde_json::Value {
    for _ in 0..200 {
        let job = daemon.get_json(&format!("/repos/clones/{id}")).await;
        if job["state"] != "running" {
            return job;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the clone {id} never ended");
}

#[tokio::test]
async fn a_url_is_cloned_in_full_into_the_clones_folder_joins_the_projet_and_is_reused() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let source = tempfile::tempdir().unwrap();
    let url = origin(&source.path().join("analytics"));
    let clones = tempfile::tempdir().unwrap();
    let folder = clones.path().join("repos");
    set_clones_folder(&daemon, &format!("{}/", folder.display())).await;
    let settings = daemon.get_json("/settings").await;
    assert_eq!(
        settings["clones_folder"]["effective"],
        folder.display().to_string(),
        "{settings}"
    );

    let target = daemon
        .get_json(&format!("/repos/clone-target?url={}", urlencode(&url)))
        .await;
    let expected = folder.join("analytics").display().to_string();
    assert_eq!(target["name"], "analytics", "{target}");
    assert_eq!(target["path"], expected);
    assert_eq!(target["cloned"], false);

    let (status, project) = flow::post_json(
        &daemon,
        "/projects",
        serde_json::json!({ "name": "Checkout" }),
    )
    .await;
    assert_eq!(status, 201, "{project}");
    let project_id = project["id"].as_str().unwrap().to_string();

    let (status, job) = flow::post_json(
        &daemon,
        "/repos/clones",
        serde_json::json!({ "url": url, "project_id": project_id }),
    )
    .await;
    assert!(status == 202 || status == 200, "{status} {job}");
    let id = job["id"].as_str().unwrap().to_string();
    let done = settled(&daemon, &id).await;
    assert_eq!(done["state"], "done", "{done}");
    assert_eq!(done["reused"], false);

    // A full clone: both commits, origin the URL.
    let clone = folder.join("analytics");
    assert_eq!(
        flow::git(&clone, &["rev-list", "--count", "HEAD"]).trim(),
        "2"
    );
    assert_eq!(
        flow::git(&clone, &["remote", "get-url", "origin"]).trim(),
        url
    );
    let project = daemon.get_json(&format!("/projects/{project_id}")).await;
    assert_eq!(
        project["members"],
        serde_json::json!([expected]),
        "{project}"
    );
    let listed = daemon.get_json("/repos/clones").await;
    assert_eq!(listed[0]["id"], id.as_str(), "{listed}");

    // The same URL again: reused, no second clone.
    let target = daemon
        .get_json(&format!("/repos/clone-target?url={}", urlencode(&url)))
        .await;
    assert_eq!(target["cloned"], true, "{target}");
    let marker = clone.join("untracked-marker");
    std::fs::write(&marker, "kept").unwrap();
    let (status, again) =
        flow::post_json(&daemon, "/repos/clones", serde_json::json!({ "url": url })).await;
    assert_eq!(status, 200, "{again}");
    assert_eq!(
        (again["state"].as_str(), again["reused"].as_bool()),
        (Some("done"), Some(true))
    );
    assert!(marker.exists(), "the clone was not made again");
}

#[tokio::test]
async fn a_failed_or_refused_clone_leaves_nothing_and_says_why() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let clones = tempfile::tempdir().unwrap();
    set_clones_folder(&daemon, clones.path().to_str().unwrap()).await;

    let (status, job) = flow::post_json(
        &daemon,
        "/repos/clones",
        serde_json::json!({ "url": "file:///definitely/not/a/repo.git" }),
    )
    .await;
    assert_eq!(status, 202, "{job}");
    let failed = settled(&daemon, job["id"].as_str().unwrap()).await;
    assert_eq!(failed["state"], "failed", "{failed}");
    let error = failed["error"].as_str().unwrap();
    assert!(
        error.starts_with("No repository answers at file:///definitely/not/a/repo.git"),
        "{error}"
    );
    assert!(!clones.path().join("repo").exists());

    // A failure, once read, is dismissed: it leaves the list for good.
    let id = failed["id"].as_str().unwrap();
    let resp = reqwest::Client::new()
        .delete(format!("{}/repos/clones/{id}", daemon.url()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let listed = daemon.get_json("/repos/clones").await;
    assert!(
        listed.as_array().unwrap().iter().all(|j| j["id"] != id),
        "{listed}"
    );

    // A folder holding something else is refused by name.
    let source = tempfile::tempdir().unwrap();
    let url = origin(&source.path().join("busy"));
    std::fs::create_dir_all(clones.path().join("busy")).unwrap();
    std::fs::write(clones.path().join("busy/file"), "x").unwrap();
    let (status, body) =
        flow::post_json(&daemon, "/repos/clones", serde_json::json!({ "url": url })).await;
    assert_eq!(status, 400, "{body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("already exists and is not a clone of"),
        "{body}"
    );

    // A path is no URL; an unknown clone is a 404.
    let (status, _) = flow::post_json(
        &daemon,
        "/repos/clones",
        serde_json::json!({ "url": "/tmp/x" }),
    )
    .await;
    assert_eq!(status, 400);
    let resp = reqwest::Client::new()
        .delete(format!("{}/repos/clones/nope", daemon.url()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn a_cancelled_clone_leaves_nothing() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let clones = tempfile::tempdir().unwrap();
    set_clones_folder(&daemon, clones.path().to_str().unwrap()).await;
    let source = tempfile::tempdir().unwrap();
    let url = origin(&source.path().join("app"));
    let (_, job) =
        flow::post_json(&daemon, "/repos/clones", serde_json::json!({ "url": url })).await;
    let id = job["id"].as_str().unwrap();
    let resp = reqwest::Client::new()
        .delete(format!("{}/repos/clones/{id}", daemon.url()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let answered: serde_json::Value = resp.json().await.unwrap();
    // A clone this small may finish before the cancel lands (the cancel then
    // dismisses it); either way nothing half-done stays.
    let ended = if answered["state"] == "running" {
        settled(&daemon, id).await
    } else {
        answered
    };
    match ended["state"].as_str() {
        Some("cancelled") => assert!(!clones.path().join("app").exists()),
        Some("done") => assert!(clones.path().join("app/.git").is_dir()),
        other => panic!("{other:?}: {ended}"),
    }
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}
