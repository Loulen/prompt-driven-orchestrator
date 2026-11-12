//! **The environment of a network git operation of the daemon**, on a real
//! daemon with the test extension and a remote that asks for credentials: an
//! Actor's clone, fetch, Source de skills and Trigger fetch run with what the
//! extension provides for them (a credential helper through `GIT_CONFIG_*`),
//! the secret never sits in an argv, and a refusal names whose credentials were
//! refused.

use std::path::Path;
use std::time::Duration;

use pdo::extension_api::{Actor, GitOperationKind};
use serde_json::{json, Value};

use crate::common::flow::seed_solo;
use crate::common::TestDaemon;
use crate::test_extension::{Control, Refusal, TestExtension, ACTOR_HEADER};

#[path = "../../pdo-workspace/tests/adapters/git_http.rs"]
mod git_http;

use git_http::{helper_log, provided_env, write_helper, AuthGitServer};

fn seed(repo: &Path) -> anyhow::Result<()> {
    seed_solo(repo, &[("solo", false)])
}

fn alice() -> Actor {
    Actor::Identified {
        namespace: "test".into(),
        id: "alice".into(),
    }
}

/// `method path` as the test Actor `who`: the status and the JSON answer.
async fn call(
    daemon: &TestDaemon,
    who: &str,
    method: reqwest::Method,
    path: &str,
    body: Option<Value>,
) -> (u16, Value) {
    let mut request = reqwest::Client::new()
        .request(method, format!("{}{path}", daemon.url()))
        .header(ACTOR_HEADER, who);
    if let Some(body) = body {
        request = request.json(&body);
    }
    let resp = request.send().await.unwrap();
    let status = resp.status().as_u16();
    (status, resp.json().await.unwrap_or(Value::Null))
}

/// Clone `url` into `path` as `who`, and wait for the clone to end.
async fn clone_as(daemon: &TestDaemon, who: &str, url: &str, path: &Path) -> Value {
    let (status, job) = call(
        daemon,
        who,
        reqwest::Method::POST,
        "/repos/clones",
        Some(json!({ "url": url, "path": path })),
    )
    .await;
    assert!(status == 200 || status == 202, "{status} {job}");
    let id = job["id"].as_str().unwrap().to_string();
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    loop {
        let (_, job) = call(
            daemon,
            who,
            reqwest::Method::GET,
            &format!("/repos/clones/{id}"),
            None,
        )
        .await;
        if job["state"] != "running" {
            return job;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the clone never ended"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// A daemon with the test extension providing `alice`'s credentials for the
/// served remote, and the helper's log.
async fn daemon_for(
    server: &AuthGitServer,
    scratch: &Path,
) -> (TestDaemon, Control, std::path::PathBuf) {
    let (ext, control) = TestExtension::new(Refusal::Nothing);
    let daemon = TestDaemon::spawn_with_extensions(seed, ext.boxed())
        .await
        .unwrap();
    let (helper, log) = write_helper(scratch);
    control.launch.set_git_env(
        "alice",
        provided_env(&helper, &log, &server.secret),
        Some("the test token".into()),
    );
    (daemon, control, log)
}

#[tokio::test]
async fn a_clone_and_a_fetch_take_the_credentials_provided_for_their_actor() {
    let server = AuthGitServer::start().await;
    let scratch = tempfile::tempdir().unwrap();
    let (daemon, control, log) = daemon_for(&server, scratch.path()).await;
    let host = server.url.split('/').nth(2).unwrap().to_string();

    // Alice adds the repository by its URL: cloned with her helper.
    let mine = scratch.path().join("clones/app");
    let done = clone_as(&daemon, "alice", &server.url, &mine).await;
    assert_eq!(done["state"], "done", "{done}");
    assert!(mine.join("README.md").is_file());
    let said = helper_log(&log);
    assert!(said.contains("called"), "{said}");
    assert!(!said.contains("leaked"), "{said}");
    let asked = control.launch.git_asks();
    assert_eq!(
        (&asked[0].actor, asked[0].kind, asked[0].url.as_str()),
        (&alice(), GitOperationKind::Clone, server.url.as_str())
    );

    // Bob has nothing provided: the daemon's credentials, refused by name,
    // for a new clone and for the reuse of Alice's.
    let theirs = scratch.path().join("clones/other");
    let refused = clone_as(&daemon, "bob", &server.url, &theirs).await;
    assert_eq!(refused["state"], "failed", "{refused}");
    let error = refused["error"].as_str().unwrap();
    assert!(
        error.starts_with(&format!(
            "The daemon's git credentials were refused for {host}"
        )),
        "{error}"
    );
    let reused = clone_as(&daemon, "bob", &server.url, &mine).await;
    assert_eq!(reused["state"], "failed", "{reused}");
    assert!(
        reused["error"]
            .as_str()
            .unwrap()
            .starts_with("The daemon's git credentials were refused"),
        "{reused}"
    );

    // New Run's fetch: Alice's passes, Bob's names the daemon's credentials.
    let path = urlencode(&mine.display().to_string());
    let (status, list) = call(
        &daemon,
        "alice",
        reqwest::Method::POST,
        &format!("/repos/fetch?path={path}"),
        None,
    )
    .await;
    assert_eq!(status, 200, "{list}");
    assert!(list["fetch_error"].is_null(), "{list}");
    let (_, list) = call(
        &daemon,
        "bob",
        reqwest::Method::POST,
        &format!("/repos/fetch?path={path}"),
        None,
    )
    .await;
    assert_eq!(list["fetch_error"]["kind"], "auth", "{list}");
    assert!(
        list["fetch_error"]["message"]
            .as_str()
            .unwrap()
            .starts_with("The daemon's git credentials were refused"),
        "{list}"
    );

    // A refusal of what was provided names the Actor and the label.
    control.launch.set_git_env(
        "alice",
        provided_env(Path::new("/bin/true"), &log, "wrong"),
        Some("the test token".into()),
    );
    let (_, list) = call(
        &daemon,
        "alice",
        reqwest::Method::POST,
        &format!("/repos/fetch?path={path}"),
        None,
    )
    .await;
    let message = list["fetch_error"]["message"].as_str().unwrap_or_default();
    assert!(
        message.starts_with(&format!(
            "The credentials provided for Alice (the test token) were refused for {host}"
        )),
        "{list}"
    );
    assert!(!helper_log(&log).contains("leaked"));
}

#[tokio::test]
async fn a_source_of_skills_is_scanned_with_the_credentials_of_its_actor() {
    let server = AuthGitServer::start().await;
    let scratch = tempfile::tempdir().unwrap();
    let (daemon, control, log) = daemon_for(&server, scratch.path()).await;

    let (status, scan) = call(
        &daemon,
        "alice",
        reqwest::Method::POST,
        "/settings/skills/scan",
        Some(json!({ "scan_id": "scan-alice", "source": server.url })),
    )
    .await;
    assert_eq!(status, 200, "{scan}");
    assert!(helper_log(&log).contains("called"));
    assert!(!helper_log(&log).contains("leaked"));
    assert!(control
        .launch
        .git_asks()
        .iter()
        .any(|op| op.kind == GitOperationKind::Skills && op.actor == alice()));

    let (status, refused) = call(
        &daemon,
        "bob",
        reqwest::Method::POST,
        "/settings/skills/scan",
        Some(json!({ "scan_id": "scan-bob", "source": server.url })),
    )
    .await;
    assert_eq!(status, 502, "{refused}");
    assert!(
        refused
            .to_string()
            .contains("The daemon's git credentials were refused"),
        "{refused}"
    );
}

#[tokio::test]
async fn the_fetch_before_a_fire_takes_the_credentials_of_the_triggers_last_editor() {
    let server = AuthGitServer::start().await;
    let scratch = tempfile::tempdir().unwrap();
    let (daemon, control, log) = daemon_for(&server, scratch.path()).await;
    let repo = daemon.target_repo();
    let out = std::process::Command::new("git")
        .args(["remote", "add", "origin", &server.url])
        .current_dir(&repo)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let (status, trigger) = call(
        &daemon,
        "alice",
        reqwest::Method::POST,
        "/triggers",
        Some(json!({
            "name": "nightly",
            "pipeline_id": "solo",
            "cron": "0 3 * * *",
            "input_template": "go",
            "target_repo": repo,
        })),
    )
    .await;
    assert_eq!(status, 201, "{trigger}");
    let id = trigger["id"].as_str().unwrap().to_string();

    daemon.force_trigger_due(&id).await;
    daemon.run_trigger_tick().await;

    let fetch = control
        .launch
        .git_asks()
        .into_iter()
        .find(|op| op.kind == GitOperationKind::TriggerFetch)
        .expect("the fire fetched through the hook");
    assert_eq!(fetch.actor, alice(), "its last editor, not the Trigger");
    assert!(helper_log(&log).contains("called"));
    assert!(!helper_log(&log).contains("leaked"));
    let fires = daemon.get_json(&format!("/triggers/{id}/fires")).await;
    assert_eq!(fires[0]["outcome"], "fired", "{fires}");
    let run_id = fires[0]["run_id"].as_str().expect("the fire started a Run");
    let detail = daemon.get_json(&format!("/runs/{run_id}")).await;
    assert!(
        detail["source_fetch_error"].is_null(),
        "the fetch passed: {detail}"
    );
}

fn urlencode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}
