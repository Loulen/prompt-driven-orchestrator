//! The Agent de support over HTTP, on a real daemon with the test extension
//! and a real tmux: one per Actor, opened on a choice of agent with the rights
//! of its Actor (what the test extension gives the launch of who starts it),
//! asked while it lives, refused on a harness that is not installed, read and
//! closed. The agent is the daemon's harmless tail, which writes its
//! environment then sleeps.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::common::TestDaemon;
use crate::test_extension::{Control, Refusal, TestExtension, ACTOR_HEADER};

/// Two harnesses declared on disk: `fake`, whose binary (`sh`) is installed
/// everywhere, and `ghost`, whose binary is installed nowhere.
const DESCRIPTORS: &str = r#"harnesses:
  fake:
    binary: sh
    launch: ["exec", "sh", "{prompt}"]
  ghost:
    binary: pdo-no-such-harness-binary
    launch: ["exec", "pdo-no-such-harness-binary", "{prompt}"]
"#;

/// A daemon with the test extension, its home holding [`DESCRIPTORS`], whose
/// every session writes its environment to `<envs>/<session name>.env`.
async fn daemon(envs: &Path) -> (TestDaemon, Control) {
    let tail = format!(
        "env > '{}'/\"$(tmux display-message -p '#S')\".env; exec sleep 600",
        envs.display()
    );
    let (ext, control) = TestExtension::new(Refusal::Nothing);
    let daemon = TestDaemon::spawn_configured(
        |root| {
            let file = root.join("home/.pdo/harnesses/descriptors.yaml");
            std::fs::create_dir_all(file.parent().unwrap())?;
            std::fs::write(file, DESCRIPTORS)?;
            Ok(())
        },
        move |root, config| {
            config.tmux_cmd_override = Some(tail);
            config.sandbox_home_override = Some(root.join("home"));
            // No network: the core's code is `support_agent_sources.rs`'s.
            config.core_source_url = Some("file:///nonexistent/pdo-core".into());
            pdo::compose(config, ext.boxed(), None).unwrap();
        },
    )
    .await
    .unwrap();
    (daemon, control)
}

async fn call_as(
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
    (status, resp.json().await.unwrap_or(Value::Null))
}

fn fake() -> Value {
    json!({ "mode": "custom", "harness": "fake", "model": "m1" })
}

async fn env_of(envs: &Path, name: &str) -> String {
    let file: PathBuf = envs.join(format!("{name}.env"));
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Ok(text) = std::fs::read_to_string(&file) {
            if text.contains("PATH=") {
                return text;
            }
        }
        assert!(
            Instant::now() < deadline,
            "{} never written",
            file.display()
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test]
async fn an_actor_opens_asks_and_closes_its_own_support_agent() {
    let envs = tempfile::tempdir().unwrap();
    let (daemon, control) = daemon(envs.path()).await;

    let (status, before) = call_as(
        &daemon,
        reqwest::Method::GET,
        "/sessions/support",
        "alice",
        None,
    )
    .await;
    assert_eq!(status, 200, "{before}");
    assert_eq!(before["alive"], false);
    assert_eq!(before["agent"], Value::Null);

    let (status, opened) = call_as(
        &daemon,
        reqwest::Method::POST,
        "/sessions/support",
        "alice",
        Some(json!({ "choice": fake(), "question": "which Runs failed?" })),
    )
    .await;
    assert_eq!(status, 201, "{opened}");
    assert_eq!(opened["alive"], true);
    assert_eq!(opened["agent"]["harness"], "fake");
    assert_eq!(opened["agent"]["model"], "m1");
    assert_eq!(opened["agent"]["label"], "Custom");
    assert_eq!(opened["last_choice"], fake());
    let session = opened["session"].as_str().unwrap().to_string();
    assert!(session.starts_with("pdo-support-"), "{session}");

    // The session carries what the extension gives its Actor.
    let env = env_of(envs.path(), &session).await;
    assert!(env.contains("TEST_STARTED_BY=alice"), "{env}");
    assert!(env.contains(&format!("PDO_SESSION_ID={session}")), "{env}");
    assert!(!env.contains("PDO_RUN_ID="), "{env}");
    let launch = control
        .launch
        .launches()
        .into_iter()
        .find(|l| l["hook"] == "session_env" && l["launch"]["session"]["id"] == session)
        .unwrap();
    assert_eq!(launch["launch"]["session"]["kind"], "support_agent");
    // Its folder holds the embedded prompt, as its instructions, and the
    // first question.
    let dir = daemon.repo_root().join("home/.pdo/support").join(&session);
    let instructions = std::fs::read_to_string(dir.join("AGENTS.md")).unwrap();
    assert!(instructions.contains("pdo help"), "{instructions}");
    let question = std::fs::read_to_string(dir.join("question.md")).unwrap();
    assert_eq!(question.trim_end(), "which Runs failed?");

    // One per Actor: a second open is refused while it lives.
    let (status, again) = call_as(
        &daemon,
        reqwest::Method::POST,
        "/sessions/support",
        "alice",
        Some(json!({ "choice": fake() })),
    )
    .await;
    assert_eq!(status, 409, "{again}");
    assert_eq!(again["error"], "support_agent_open");

    // The live agent is asked; Bob has none to ask.
    let (status, asked) = call_as(
        &daemon,
        reqwest::Method::POST,
        "/sessions/support/ask",
        "alice",
        Some(json!({ "question": "how many Pipelines?" })),
    )
    .await;
    assert_eq!(status, 200, "{asked}");
    assert_eq!(asked["session"], session.as_str());
    let (status, none) = call_as(
        &daemon,
        reqwest::Method::POST,
        "/sessions/support/ask",
        "bob",
        Some(json!({ "question": "hi" })),
    )
    .await;
    assert_eq!(status, 404, "{none}");
    assert_eq!(none["error"], "no_support_agent");

    // The page reloads: the agent is still there.
    let (_, read) = call_as(
        &daemon,
        reqwest::Method::GET,
        "/sessions/support",
        "alice",
        None,
    )
    .await;
    assert_eq!(read["alive"], true);
    assert!(crate::common::tmux(&daemon.tmux_socket()).session_exists(&session));

    let (status, closed) = call_as(
        &daemon,
        reqwest::Method::DELETE,
        "/sessions/support",
        "alice",
        None,
    )
    .await;
    assert_eq!(status, 200, "{closed}");
    assert_eq!(closed["alive"], false);
    assert_eq!(
        closed["last_choice"],
        fake(),
        "the last choice is remembered"
    );
    assert!(!crate::common::tmux(&daemon.tmux_socket()).session_exists(&session));
}

#[tokio::test]
async fn a_harness_that_is_not_installed_or_no_agent_at_all_is_refused_by_name() {
    let envs = tempfile::tempdir().unwrap();
    let (daemon, _control) = daemon(envs.path()).await;
    let (status, opened) = call_as(
        &daemon,
        reqwest::Method::POST,
        "/sessions/support",
        "alice",
        Some(json!({ "choice": fake() })),
    )
    .await;
    assert_eq!(status, 201, "{opened}");

    let (status, ghost) = call_as(
        &daemon,
        reqwest::Method::POST,
        "/sessions/support",
        "alice",
        Some(json!({ "choice": { "mode": "custom", "harness": "ghost" }, "replace": true })),
    )
    .await;
    assert_eq!(status, 409, "{ghost}");
    assert_eq!(ghost["error"], "harness_not_installed");
    assert!(
        ghost["message"].as_str().unwrap().contains("not installed"),
        "{ghost}"
    );
    let (_, still) = call_as(
        &daemon,
        reqwest::Method::GET,
        "/sessions/support",
        "alice",
        None,
    )
    .await;
    assert_eq!(still["alive"], true, "a refused replacement closes nothing");

    for (choice, error) in [
        (json!({ "mode": "inherit" }), "agent_required"),
        (
            json!({ "mode": "custom", "harness": "nope" }),
            "unknown_harness",
        ),
        (
            json!({ "mode": "profile", "profile_id": "gone" }),
            "unknown_profile",
        ),
    ] {
        let (status, body) = call_as(
            &daemon,
            reqwest::Method::POST,
            "/sessions/support",
            "bob",
            Some(json!({ "choice": choice })),
        )
        .await;
        assert_eq!(status, 400, "{body}");
        assert_eq!(body["error"], error);
    }

    // `replace` closes the open one and opens the other.
    let (status, replaced) = call_as(
        &daemon,
        reqwest::Method::POST,
        "/sessions/support",
        "alice",
        Some(json!({
            "choice": { "mode": "custom", "harness": "fake", "model": "m2" },
            "replace": true,
        })),
    )
    .await;
    assert_eq!(status, 201, "{replaced}");
    assert_eq!(replaced["agent"]["model"], "m2");
}

/// `pdo support` through the real binary: open on a harness, read, ask,
/// close; `4` when asking with none open; `1`, with the reason, on a harness
/// that is not installed.
#[tokio::test]
async fn the_cli_opens_reads_asks_and_closes_the_support_agent() {
    use crate::cli_operator::pdo;
    let envs = tempfile::tempdir().unwrap();
    let (daemon, _control) = daemon(envs.path()).await;

    let closed = pdo(&daemon, &["support", "status"]).await;
    assert_eq!(closed.ok(), "pdo-support-operator\tclosed\n");
    let none = pdo(&daemon, &["support", "ask", "hi"]).await;
    assert_eq!(none.code, Some(4), "{}", none.stderr);

    let ghost = pdo(&daemon, &["support", "open", "--harness", "ghost"]).await;
    assert_eq!(ghost.code, Some(1), "{}", ghost.stderr);
    assert!(ghost.stderr.contains("not installed"), "{}", ghost.stderr);

    let opened = pdo(
        &daemon,
        &[
            "support",
            "open",
            "which Runs failed?",
            "--harness",
            "fake",
            "--model",
            "m1",
        ],
    )
    .await;
    assert_eq!(
        opened.ok(),
        "opened: pdo-support-operator\topen\tCustom\tfake · m1 · —\n"
    );
    let again = pdo(&daemon, &["support", "open", "--harness", "fake"]).await;
    assert_eq!(again.code, Some(1), "{}", again.stderr);
    assert!(
        again.stderr.contains("a support agent is open"),
        "{}",
        again.stderr
    );

    let asked = pdo(
        &daemon,
        &["support", "ask", "how many Pipelines?", "--json"],
    )
    .await;
    assert_eq!(asked.json()["alive"], true);
    let kept = pdo(&daemon, &["support", "keep", "--json"]).await;
    assert_eq!(kept.json()["still_there"], Value::Null);
    assert_eq!(
        pdo(&daemon, &["support", "close"]).await.ok(),
        "closed: pdo-support-operator\tclosed\n"
    );
    assert_eq!(
        pdo(&daemon, &["support", "status", "--json"]).await.json()["last_choice"],
        json!({ "mode": "custom", "harness": "fake", "model": "m1" })
    );
}

/// The `support_agent_changed` messages of `/ws` until one says `change`.
async fn change_until<S>(ws: &mut S, change: &str, within: Duration) -> Value
where
    S: futures_util::Stream<
            Item = Result<
                tokio_tungstenite::tungstenite::protocol::Message,
                tokio_tungstenite::tungstenite::Error,
            >,
        > + Unpin,
{
    use futures_util::StreamExt;
    let deadline = Instant::now() + within;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        assert!(!left.is_zero(), "no `{change}` on /ws");
        let Ok(Some(Ok(message))) = tokio::time::timeout(left, ws.next()).await else {
            continue;
        };
        let Ok(text) = message.into_text() else {
            continue;
        };
        let Ok(message) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        if message["type"] == "support_agent_changed" && message["support"]["change"] == change {
            return message;
        }
    }
}

#[tokio::test]
async fn an_idle_agent_asks_still_there_on_ws_and_keep_answers_it() {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let envs = tempfile::tempdir().unwrap();
    let (daemon, _control) = daemon(envs.path()).await;
    let (status, settings) = call_as(
        &daemon,
        reqwest::Method::PUT,
        "/settings",
        "alice",
        Some(json!({ "support_idle_timeout_secs": 1 })),
    )
    .await;
    assert_eq!(status, 200, "{settings}");
    let mut request = format!("ws://{}/ws", daemon.addr)
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert(ACTOR_HEADER, "alice".parse().unwrap());
    let (mut ws, _) = tokio_tungstenite::connect_async(request).await.unwrap();

    let (status, opened) = call_as(
        &daemon,
        reqwest::Method::POST,
        "/sessions/support",
        "alice",
        Some(json!({ "choice": fake() })),
    )
    .await;
    assert_eq!(status, 201, "{opened}");
    let session = opened["session"].as_str().unwrap().to_string();
    change_until(&mut ws, "opened", Duration::from_secs(5)).await;

    let idle = change_until(&mut ws, "idle", Duration::from_secs(15)).await;
    assert_eq!(idle["support_session"], session.as_str());
    assert_eq!(idle["support"]["idle_timeout_secs"], 1);
    assert!(idle.get("read_guard").is_none(), "the guard stays home");
    let (_, asked) = call_as(
        &daemon,
        reqwest::Method::GET,
        "/sessions/support",
        "alice",
        None,
    )
    .await;
    assert_eq!(
        asked["still_there"]["closes_at"],
        idle["support"]["closes_at"]
    );

    let (status, kept) = call_as(
        &daemon,
        reqwest::Method::POST,
        "/sessions/support/keep",
        "alice",
        None,
    )
    .await;
    assert_eq!(status, 200, "{kept}");
    assert_eq!(kept["alive"], true);
    assert_eq!(kept["still_there"], Value::Null);
    change_until(&mut ws, "active", Duration::from_secs(5)).await;

    // With no agent, Keep has nothing to keep.
    call_as(
        &daemon,
        reqwest::Method::DELETE,
        "/sessions/support",
        "alice",
        None,
    )
    .await;
    change_until(&mut ws, "closed", Duration::from_secs(5)).await;
    let (status, none) = call_as(
        &daemon,
        reqwest::Method::POST,
        "/sessions/support/keep",
        "alice",
        None,
    )
    .await;
    assert_eq!(status, 404, "{none}");
    assert_eq!(none["error"], "no_support_agent");
}
