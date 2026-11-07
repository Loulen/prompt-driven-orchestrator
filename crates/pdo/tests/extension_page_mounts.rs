//! A page mounted from a node session is judged in the Projet of its Run: on a
//! real daemon with the test extension, which grants the page mounts only in
//! the Projet `P`. A node of a Run of `P` mounts and unmounts a page without
//! any instance-wide grant; a node of a Run of another Projet `Q` is refused;
//! a mount outside any Run names no Projet. The question of authorization
//! answers the same.

use serde_json::{json, Value};

use crate::common::flow::{seed_solo, wait_node_status};
use crate::common::TestDaemon;
use crate::test_extension::{Control, Refusal, TestExtension};
use pdo::extension_api::{Actor, Operation, ResourceKind, Verb};

async fn daemon() -> (TestDaemon, Control) {
    let (ext, control) = TestExtension::new(Refusal::Nothing);
    let daemon =
        TestDaemon::spawn_with_extensions(|repo| seed_solo(repo, &[("solo", false)]), ext.boxed())
            .await
            .unwrap();
    (daemon, control)
}

/// `method path` from `session` (`(run, node)`), or from no session.
async fn call(
    daemon: &TestDaemon,
    method: reqwest::Method,
    path: &str,
    session: Option<(&str, &str)>,
    body: Option<Value>,
) -> (u16, Value) {
    let mut request = reqwest::Client::new().request(method, format!("{}{path}", daemon.url()));
    if let Some((run_id, node_id)) = session {
        request = request
            .header("X-PDO-Session-Run-Id", run_id)
            .header("X-PDO-Session-Node-Id", node_id);
    }
    if let Some(body) = body {
        request = request.json(&body);
    }
    let resp = request.send().await.unwrap();
    let status = resp.status().as_u16();
    (status, resp.json().await.unwrap_or(Value::Null))
}

async fn projet(daemon: &TestDaemon, name: &str) -> String {
    let (status, body) = call(
        daemon,
        reqwest::Method::POST,
        "/projects",
        None,
        Some(json!({ "name": name, "members": [daemon.target_repo()] })),
    )
    .await;
    assert_eq!(status, 201, "{body}");
    body["id"].as_str().unwrap().to_string()
}

/// A Run of `solo` launched in `project`, once its `worker` runs.
async fn running_run_in(daemon: &TestDaemon, project: &str) -> String {
    let (status, body) = call(
        daemon,
        reqwest::Method::POST,
        "/runs",
        None,
        Some(json!({
            "pipeline": "solo",
            "input": "go",
            "target_repo": daemon.target_repo(),
            "project_id": project,
        })),
    )
    .await;
    assert_eq!(status, 201, "{body}");
    let run_id = body["run_id"].as_str().unwrap().to_string();
    wait_node_status(daemon, &run_id, "worker", "running").await;
    run_id
}

/// The last page-mount Operation the extension was asked about with `verb`.
fn last_asked(control: &Control, verb: Verb) -> (Actor, Operation) {
    control
        .seen()
        .into_iter()
        .rev()
        .find(|(_, operation)| {
            operation.resource == ResourceKind::PageMount && operation.verb == verb
        })
        .unwrap_or_else(|| panic!("no page mount {verb:?} asked: {:#?}", control.seen()))
}

async fn served(daemon: &TestDaemon, name: &str) -> u16 {
    reqwest::get(format!("{}/pages/{name}/", daemon.url()))
        .await
        .unwrap()
        .status()
        .as_u16()
}

async fn listed(daemon: &TestDaemon) -> Vec<String> {
    let (_, mounts) = call(daemon, reqwest::Method::GET, "/pages", None, None).await;
    mounts
        .as_array()
        .unwrap()
        .iter()
        .map(|mount| mount["name"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn a_node_mounts_and_unmounts_in_the_projet_of_its_run_and_nowhere_else() {
    let (daemon, control) = daemon().await;
    let p = projet(&daemon, "P").await;
    let q = projet(&daemon, "Q").await;
    let in_p = running_run_in(&daemon, &p).await;
    let in_q = running_run_in(&daemon, &q).await;
    // Page mounts granted in P only: no instance-wide grant, nothing in Q.
    control.set(
        Refusal::parse(&format!(
            "page_mount:create@,page_mount:create@{q},page_mount:delete@,page_mount:delete@{q}"
        ))
        .unwrap(),
    );
    let pages = tempfile::tempdir().unwrap();
    std::fs::write(pages.path().join("index.html"), "<h1>Mock-up</h1>").unwrap();
    let mount = json!({ "name": "mockup", "directory": pages.path() });

    // The node of the Run of P mounts: judged in P, served and listed.
    let (status, body) = call(
        &daemon,
        reqwest::Method::POST,
        "/pages",
        Some((&in_p, "worker")),
        Some(mount.clone()),
    )
    .await;
    assert_eq!(status, 201, "{body}");
    assert_eq!(body["run_id"], in_p.as_str());
    let (actor, operation) = last_asked(&control, Verb::Create);
    assert!(matches!(actor, Actor::Agent { .. }), "{actor:?}");
    assert_eq!(operation.projects, [p.clone()]);
    assert_eq!(served(&daemon, "mockup").await, 200);
    assert_eq!(listed(&daemon).await, ["mockup"]);

    // It unmounts: judged in P, the page is gone.
    let (status, body) = call(
        &daemon,
        reqwest::Method::DELETE,
        "/pages/mockup",
        Some((&in_p, "worker")),
        None,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let (_, operation) = last_asked(&control, Verb::Delete);
    assert_eq!(operation.target.as_deref(), Some("mockup"));
    assert_eq!(operation.projects, [p.clone()]);
    assert_eq!(served(&daemon, "mockup").await, 404);
    assert!(listed(&daemon).await.is_empty());

    // The node of the Run of Q is refused, nothing is mounted.
    let (status, body) = call(
        &daemon,
        reqwest::Method::POST,
        "/pages",
        Some((&in_q, "worker")),
        Some(mount.clone()),
    )
    .await;
    assert_eq!(status, 403, "{body}");
    assert_eq!(last_asked(&control, Verb::Create).1.projects, [q.clone()]);
    assert!(listed(&daemon).await.is_empty());

    // Outside any Run, the mount names no Projet, as before.
    let (status, body) = call(
        &daemon,
        reqwest::Method::POST,
        "/pages",
        None,
        Some(mount.clone()),
    )
    .await;
    assert_eq!(status, 403, "{body}");
    let (actor, operation) = last_asked(&control, Verb::Create);
    assert_eq!(actor, Actor::LocalOperator);
    assert!(operation.is_outside_projects(), "{operation:?}");
}

#[tokio::test]
async fn the_unmount_of_a_runs_mount_is_judged_in_the_projet_of_that_run() {
    let (daemon, control) = daemon().await;
    let p = projet(&daemon, "P").await;
    let in_p = running_run_in(&daemon, &p).await;
    let pages = tempfile::tempdir().unwrap();
    std::fs::write(pages.path().join("index.html"), "page").unwrap();
    let (status, body) = call(
        &daemon,
        reqwest::Method::POST,
        "/pages",
        Some((&in_p, "worker")),
        Some(json!({ "name": "owned", "directory": pages.path() })),
    )
    .await;
    assert_eq!(status, 201, "{body}");

    // The operator unmounts the Run's page: still judged in the Run's Projet.
    let (status, body) = call(&daemon, reqwest::Method::DELETE, "/pages/owned", None, None).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(last_asked(&control, Verb::Delete).1.projects, [p]);
}

#[tokio::test]
async fn the_question_of_authorization_places_a_mount_in_the_projet_of_the_run() {
    let (daemon, control) = daemon().await;
    let p = projet(&daemon, "P").await;
    let q = projet(&daemon, "Q").await;
    let in_p = running_run_in(&daemon, &p).await;
    let in_q = running_run_in(&daemon, &q).await;
    let pages = tempfile::tempdir().unwrap();
    std::fs::write(pages.path().join("index.html"), "page").unwrap();
    let (status, body) = call(
        &daemon,
        reqwest::Method::POST,
        "/pages",
        Some((&in_p, "worker")),
        Some(json!({ "name": "owned", "directory": pages.path() })),
    )
    .await;
    assert_eq!(status, 201, "{body}");
    control.set(
        Refusal::parse(&format!(
            "page_mount:create@,page_mount:create@{q},page_mount:delete@"
        ))
        .unwrap(),
    );

    let question = json!({ "operations": [
        { "resource": "page_mount", "verb": "create" },
        { "resource": "page_mount", "verb": "delete", "target": "owned" },
    ] });
    let decisions = |body: &Value| -> Vec<String> {
        body["decisions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d["decision"].as_str().unwrap().to_string())
            .collect()
    };
    for (session, expected) in [
        (Some((in_p.as_str(), "worker")), ["allow", "allow"]),
        (Some((in_q.as_str(), "worker")), ["deny", "allow"]),
        (None, ["deny", "allow"]),
    ] {
        let (status, body) = call(
            &daemon,
            reqwest::Method::POST,
            "/authorize",
            session,
            Some(question.clone()),
        )
        .await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(decisions(&body), expected, "from {session:?}: {body}");
    }
    let mounts = control
        .asked()
        .into_iter()
        .filter(|(_, o)| o.resource == ResourceKind::PageMount && o.verb == Verb::Create)
        .map(|(_, o)| o.projects)
        .collect::<Vec<_>>();
    assert_eq!(&mounts[mounts.len() - 3..], [vec![p], vec![q], vec![]]);
}
