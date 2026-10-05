//! Layer 4 (ADR-0080) — the identity seams of the extension API 0.8 (CORE-2),
//! at the daemon's HTTP seam, with the internal test extension requiring a
//! token (`test_extension/gate.rs`):
//!
//! - **admission**: a request nobody identified is refused (`401`, the core's
//!   error body) or redirected to the extension's sign-in page, on REST, `/ws`,
//!   the PTY, the extensions' routes and `/ext-cli`; never the SPA's files;
//!   without a token required, nothing changes;
//! - **display name**: `GET /actors/me`, `POST /actors/display`, the audit
//!   journal's rows;
//! - **CLI**: `PDO_TOKEN` reaches the daemon as `Authorization: Bearer`.

use std::process::Command;

use pdo::extension_api::Extension;
use reqwest::StatusCode;
use serde_json::{json, Value};

use crate::common::{flow, TestDaemon};
use crate::test_extension::gate::{Gate, SIGN_IN};
use crate::test_extension::{Control, Refusal, TestExtension};

const TOKEN: &str = "s3cret";

async fn daemon(gate: Gate) -> (TestDaemon, Control) {
    let (ext, control) = TestExtension::new(Refusal::Nothing);
    control.set_gate(gate);
    let extensions: Vec<Box<dyn Extension>> = vec![Box::new(ext)];
    let daemon = TestDaemon::spawn_with_extensions(
        |repo| flow::seed_solo(repo, &[("solo", false)]),
        extensions,
    )
    .await
    .unwrap();
    (daemon, control)
}

fn client() -> reqwest::Client {
    reqwest::Client::new()
}

async fn body(resp: reqwest::Response) -> (StatusCode, Value) {
    let status = resp.status();
    let body = resp.json().await.unwrap_or(Value::Null);
    (status, body)
}

/// The status of a WebSocket upgrade of `path`, with `token` as bearer if any.
async fn upgrade(daemon: &TestDaemon, path: &str, token: Option<&str>) -> u16 {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tokio_tungstenite::tungstenite::Error;
    let mut request = format!("ws://{}{path}", daemon.addr)
        .into_client_request()
        .unwrap();
    if let Some(token) = token {
        request
            .headers_mut()
            .insert("authorization", format!("Bearer {token}").parse().unwrap());
    }
    match tokio_tungstenite::connect_async(request).await {
        Ok((_ws, response)) => response.status().as_u16(),
        Err(Error::Http(response)) => response.status().as_u16(),
        Err(other) => panic!("upgrade of {path}: {other}"),
    }
}

#[tokio::test]
async fn a_request_nobody_identified_is_refused_with_the_core_s_401() {
    let (daemon, _control) = daemon(Gate::token(TOKEN)).await;
    let url = daemon.url();

    let (status, refused) = body(client().get(format!("{url}/runs")).send().await.unwrap()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(refused["error"], "unauthenticated");
    assert_eq!(refused["reason"], "the test extension requires a token");
    assert!(refused.get("redirect").is_none(), "{refused}");

    // A mutation, an extension's own route, the CLI relay: refused alike.
    let resp = client()
        .post(format!("{url}/triggers"))
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let resp = client()
        .get(format!("{url}/ext/test/events"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let resp = client()
        .post(format!("{url}/ext-cli/test"))
        .json(&json!({ "args": ["echo"], "stdin": "" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // What no route claims under `/ext`: a path the extension does not serve,
    // an extension that is not loaded. Fallbacks, which the admission guards too.
    for path in [
        "/ext/test/does-not-exist",
        "/ext/nobody",
        "/ext/test/nested/x",
    ] {
        let resp = client().get(format!("{url}{path}")).send().await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "{path}");
    }

    // `/ws` and a terminal's PTY: the upgrade itself is refused.
    assert_eq!(upgrade(&daemon, "/ws", None).await, 401);
    assert_eq!(upgrade(&daemon, "/sessions/pdo-none/pty", None).await, 401);

    // The SPA's own files always load; the extension's sign-in page is admitted.
    let resp = client().get(format!("{url}/")).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let resp = client()
        .get(format!("{url}{SIGN_IN}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn a_redirection_names_the_sign_in_page() {
    let (daemon, _control) = daemon(Gate::token(TOKEN).redirecting()).await;
    let resp = client()
        .get(format!("{}/ext", daemon.url()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.headers()["www-authenticate"], "Bearer");
    let (status, refused) = body(resp).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(refused["error"], "unauthenticated");
    assert_eq!(refused["redirect"], SIGN_IN);
}

#[tokio::test]
async fn the_token_holder_is_served_everywhere() {
    let (daemon, _control) = daemon(Gate::token(TOKEN).redirecting()).await;
    let url = daemon.url();
    let resp = client()
        .get(format!("{url}/runs"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    // The browser's way: the cookie its sign-in page sets.
    let resp = client()
        .get(format!("{url}/runs"))
        .header("cookie", format!("pdo-test-token={TOKEN}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(upgrade(&daemon, "/ws", Some(TOKEN)).await, 101);
    let resp = client()
        .get(format!("{url}/actors/me"))
        .bearer_auth("wrong")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn without_a_token_required_nothing_changes() {
    let (daemon, _control) = daemon(Gate::default()).await;
    let url = daemon.url();
    let resp = client().get(format!("{url}/runs")).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(upgrade(&daemon, "/ws", None).await, 101);
    let (status, me) = body(
        client()
            .get(format!("{url}/actors/me"))
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        me,
        json!({ "actor": { "kind": "local_operator" }, "display": null })
    );
}

#[tokio::test]
async fn the_gate_changes_while_the_daemon_runs() {
    let (daemon, control) = daemon(Gate::default()).await;
    let url = daemon.url();
    let resp = client()
        .put(format!("{url}/ext/test/gate"))
        .json(&json!({ "token": TOKEN, "redirect": true }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(control.gate(), Gate::token(TOKEN).redirecting());
    let resp = client()
        .get(format!("{url}/ext/test/gate"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_display_name_of_the_caller_and_of_any_actor() {
    let (daemon, _control) = daemon(Gate::token(TOKEN)).await;
    let url = daemon.url();
    let (status, me) = body(
        client()
            .get(format!("{url}/actors/me"))
            .bearer_auth(TOKEN)
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        me,
        json!({
            "actor": { "kind": "identified", "namespace": "test", "id": "alice" },
            "display": { "name": "Alice", "avatar_url": "/ext/test/avatar.svg" },
        })
    );
    let (status, displays) = body(
        client()
            .post(format!("{url}/actors/display"))
            .bearer_auth(TOKEN)
            .json(&json!({ "actors": [
                { "kind": "identified", "namespace": "test", "id": "bob" },
                { "kind": "local_operator" },
            ]}))
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(displays["displays"][0]["name"], "Bob");
    assert_eq!(displays["displays"][1], Value::Null);
}

#[tokio::test]
async fn the_audit_journal_shows_who_made_a_mutation() {
    let (daemon, _control) = daemon(Gate::token(TOKEN)).await;
    let url = daemon.url();
    let resp = client()
        .post(format!("{url}/triggers"))
        .bearer_auth(TOKEN)
        .json(&json!({
            "name": "nightly",
            "pipeline_id": "solo",
            "cron": "0 3 * * *",
            "input_template": "go",
            "target_repo": daemon.target_repo(),
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let (status, rows) = body(
        client()
            .get(format!("{url}/audit"))
            .bearer_auth(TOKEN)
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let row = &rows[0];
    assert_eq!(row["action"], "trigger.created", "{rows}");
    assert_eq!(row["actor"]["id"], "alice");
    assert_eq!(row["actor_display"]["name"], "Alice");
}

/// `pdo <args>` against `daemon`, with `PDO_TOKEN` set to `token` if any.
async fn pdo(daemon: &TestDaemon, args: &[&str], token: Option<&str>) -> (i32, String, String) {
    pdo_in(daemon, args, token, None).await
}

/// [`pdo`], from the node session `(run, node)` if any.
async fn pdo_in(
    daemon: &TestDaemon,
    args: &[&str],
    token: Option<&str>,
    session: Option<(&str, &str)>,
) -> (i32, String, String) {
    let url = daemon.url();
    let session = session.map(|(r, n)| (r.to_string(), n.to_string()));
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    let token = token.map(str::to_string);
    tokio::task::spawn_blocking(move || {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_pdo"));
        cmd.args(&args);
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("PDO_") {
                cmd.env_remove(key);
            }
        }
        cmd.env("PDO_DAEMON_URL", &url);
        if let Some(token) = token {
            cmd.env("PDO_TOKEN", token);
        }
        if let Some((run, node)) = &session {
            cmd.env("PDO_RUN_ID", run).env("PDO_NODE_ID", node);
        }
        let out = cmd.output().unwrap();
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn the_cli_sends_pdo_token() {
    let (daemon, control) = daemon(Gate::token(TOKEN)).await;

    let (code, _, stderr) = pdo(&daemon, &["run", "list"], None).await;
    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains("401"), "{stderr}");
    assert!(stderr.contains("requires a token"), "{stderr}");

    let (code, stdout, stderr) = pdo(&daemon, &["run", "list", "--json"], Some(TOKEN)).await;
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout.trim(), "[]");

    // `pdo ext`: the extension sees the token's holder.
    let (code, stdout, stderr) = pdo(&daemon, &["ext", "test", "echo", "salut"], Some(TOKEN)).await;
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout.trim(), "salut");
    assert!(stderr.contains("identified:test/alice"), "{stderr}");
    let (code, _, _) = pdo(&daemon, &["ext", "test", "echo", "salut"], None).await;
    assert_ne!(code, 0);

    // `pdo run create`: the created Run's attribute names the token's holder.
    let target = daemon.target_repo();
    let (code, stdout, stderr) = pdo(
        &daemon,
        &[
            "run",
            "create",
            "solo",
            "--input",
            "go",
            "--target-repo",
            &target,
        ],
        Some(TOKEN),
    )
    .await;
    assert_eq!(code, 0, "{stdout}{stderr}");
    let (code, stdout, _) = pdo(&daemon, &["run", "list"], Some(TOKEN)).await;
    assert_eq!(code, 0);
    assert_eq!(stdout.lines().count(), 1, "{stdout}");
    assert!(control.seen().iter().any(|(actor, op)| op.resource
        == pdo::extension_api::ResourceKind::Run
        && matches!(actor, pdo::extension_api::Actor::Identified { id, .. } if id == "alice")));
}

#[tokio::test]
async fn the_agent_cli_sends_pdo_token_too() {
    let (daemon, _control) = daemon(Gate::token(TOKEN)).await;
    let session = Some(("no-such-run", "n1"));
    for args in [
        &["fail", "--reason", "x"][..],
        &["skip", "--reason", "x"][..],
        &["complete"][..],
        &["wait-user", "--message", "x"][..],
    ] {
        let unauthenticated =
            |stderr: &str| stderr.contains("401") || stderr.contains("unauthenticated");
        let (_, _, refused) = pdo_in(&daemon, args, None, session).await;
        assert!(
            unauthenticated(&refused),
            "{args:?} without token: {refused}"
        );
        let (_, _, answered) = pdo_in(&daemon, args, Some(TOKEN), session).await;
        assert!(
            !unauthenticated(&answered),
            "{args:?} with the token: {answered}"
        );
    }
}
