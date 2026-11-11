//! Stage 4 (ADR-0080) — adding a repository to a Projet is judged in that
//! Projet: a person who may only write in `p` clones a URL into it and
//! follows the clone to its end, creates a new empty one there, brings in a
//! repository of a Projet where she may read it; she adds no path of the disk
//! and no repository of a Projet she may not read, and nobody else follows
//! her clone.

use std::path::Path;
use std::time::Duration;

use serde_json::{json, Value};

use crate::common::flow;
use crate::common::TestDaemon;
use crate::test_extension::{Refusal, TestExtension, ACTOR_HEADER};

/// Mia may write in `p` only: neither in no Projet (the disk, a repository
/// there) nor in `s`. Bob does not read the disk either.
const MIA_IN_P: &str = "repository:create@~mia,repository:read@~mia,project:update@s~mia,repository:read@s~mia,repository:create@s~mia,repository:read@~bob";

async fn call(
    daemon: &TestDaemon,
    method: reqwest::Method,
    path: &str,
    actor: &str,
    body: Option<Value>,
) -> (u16, Value) {
    let mut request = reqwest::Client::new()
        .request(method, format!("{}{path}", daemon.url()))
        .header(ACTOR_HEADER, actor);
    if let Some(body) = body {
        request = request.json(&body);
    }
    let resp = request.send().await.unwrap();
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    (status, serde_json::from_str(&text).unwrap_or(Value::Null))
}

async fn as_mia(
    daemon: &TestDaemon,
    method: reqwest::Method,
    path: &str,
    body: Option<Value>,
) -> (u16, Value) {
    call(daemon, method, path, "mia", body).await
}

/// A repository with one commit, to clone by its `file://` URL.
fn origin(dir: &Path) -> String {
    flow::git_init(dir).unwrap();
    format!("file://{}", dir.display())
}

async fn project(daemon: &TestDaemon, name: &str, members: &[&str]) -> String {
    let (status, project) = flow::post_json(
        daemon,
        "/projects",
        json!({ "name": name, "members": members }),
    )
    .await;
    assert_eq!(status, 201, "{project}");
    project["id"].as_str().unwrap().to_string()
}

async fn members(daemon: &TestDaemon, id: &str) -> Value {
    daemon.get_json(&format!("/projects/{id}")).await["members"].clone()
}

#[tokio::test]
async fn a_writer_of_a_projet_adds_repositories_to_it_without_the_disk() {
    let (ext, control) = TestExtension::new(Refusal::Nothing);
    let daemon = TestDaemon::spawn_with_extensions(flow::git_init, ext.boxed())
        .await
        .unwrap();
    let clones = tempfile::tempdir().unwrap();
    let (status, body) = call(
        &daemon,
        reqwest::Method::PUT,
        "/settings",
        "admin",
        Some(json!({ "clones_folder": clones.path().to_str().unwrap() })),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let repos = tempfile::tempdir().unwrap();
    let of_r = repos.path().join("risk");
    let of_s = repos.path().join("secret");
    for repo in [&of_r, &of_s] {
        flow::git_init(repo).unwrap();
    }
    let p = project(&daemon, "Payments", &[]).await;
    let r = project(&daemon, "Risk", &[of_r.to_str().unwrap()]).await;
    let s = project(&daemon, "Secret", &[of_s.to_str().unwrap()]).await;
    control.set(Refusal::parse(&MIA_IN_P.replace("@s", &format!("@{s}"))).unwrap());

    // She clones a URL into `p`, and follows the clone to its end.
    let source = tempfile::tempdir().unwrap();
    let url = origin(&source.path().join("ledger"));
    let target = format!("/repos/clone-target?url={url}&project_id={p}");
    let (status, answer) = as_mia(&daemon, reqwest::Method::GET, &target, None).await;
    assert_eq!(status, 200, "{answer}");
    let (status, job) = as_mia(
        &daemon,
        reqwest::Method::POST,
        "/repos/clones",
        Some(json!({ "url": url, "project_id": p })),
    )
    .await;
    assert_eq!(status, 202, "{job}");
    let id = job["id"].as_str().unwrap();
    let mut ended = job.clone();
    for _ in 0..200 {
        let (status, job) = as_mia(
            &daemon,
            reqwest::Method::GET,
            &format!("/repos/clones/{id}"),
            None,
        )
        .await;
        assert_eq!(status, 200, "{job}");
        ended = job;
        if ended["state"] != "running" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(ended["state"], "done", "{ended}");
    let cloned = clones.path().join("ledger");
    assert!(cloned.join(".git").is_dir());
    let (status, listed) = as_mia(&daemon, reqwest::Method::GET, "/repos/clones", None).await;
    assert_eq!(
        (status, listed.as_array().unwrap().len()),
        (200, 1),
        "{listed}"
    );

    // Nobody else follows it: as an unknown clone.
    for method in [reqwest::Method::GET, reqwest::Method::DELETE] {
        let (status, _) = call(&daemon, method, &format!("/repos/clones/{id}"), "bob", None).await;
        assert_eq!(status, 404);
    }
    let (_, listed) = call(&daemon, reqwest::Method::GET, "/repos/clones", "bob", None).await;
    assert_eq!(listed, json!([]));

    // A new empty repository for `p`: in the clones folder.
    let (status, created) = as_mia(
        &daemon,
        reqwest::Method::POST,
        "/repos/create",
        Some(json!({ "name": "books", "project_id": p })),
    )
    .await;
    assert_eq!(status, 200, "{created}");
    let books = clones.path().join("books");
    assert_eq!(created["path"], books.to_str().unwrap());

    // A repository of `r`, where she reads it, joins; one of `s` does not.
    let add = format!("/projects/{p}/members");
    let (status, body) = as_mia(
        &daemon,
        reqwest::Method::POST,
        &add,
        Some(json!({ "path": of_r })),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let (status, body) = as_mia(
        &daemon,
        reqwest::Method::POST,
        &add,
        Some(json!({ "path": of_s })),
    )
    .await;
    assert_eq!(status, 403, "{body}");
    // Neither does a path in no Projet, with the sentence to read.
    let (status, body) = as_mia(
        &daemon,
        reqwest::Method::POST,
        &add,
        Some(json!({ "path": source.path().join("ledger") })),
    )
    .await;
    assert_eq!(status, 403, "{body}");
    assert!(
        body["sentence"].as_str().is_some_and(|s| !s.is_empty()),
        "{body}"
    );

    assert_eq!(
        members(&daemon, &p).await,
        json!([
            cloned.to_str().unwrap(),
            books.to_str().unwrap(),
            of_r.to_str().unwrap()
        ])
    );
    let _ = r;
}
