//! #614 (correctif 10): a default harness that does not resolve is **refused at
//! registration**, exactly as the default sandbox already is — never accepted and
//! then breaking every spawn that falls through to it.
//!
//! Layer-3, through `PUT /settings`. An embedded name (`claude`, `copilot`)
//! resolves and is accepted; an unknown name is a `400` that names the knobs.

use crate::common::TestDaemon;

/// A minimal repo so the daemon boots; no run is created here.
fn seed() -> impl FnOnce(&std::path::Path) -> anyhow::Result<()> {
    |dir: &std::path::Path| {
        let run = |args: &[&str]| -> anyhow::Result<()> {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(dir)
                .output()?;
            anyhow::ensure!(out.status.success(), "git {args:?} failed");
            Ok(())
        };
        run(&["init", "-q"])?;
        run(&["config", "user.email", "t@e.st"])?;
        run(&["config", "user.name", "t"])?;
        std::fs::write(dir.join("README.md"), "x\n")?;
        std::fs::write(dir.join(".gitignore"), ".pdo/runs/\n")?;
        run(&["add", "."])?;
        run(&["commit", "-q", "-m", "init"])?;
        Ok(())
    }
}

async fn put_default_harness(daemon: &TestDaemon, name: &str) -> reqwest::Response {
    reqwest::Client::new()
        .put(format!("{}/settings", daemon.url()))
        .json(&serde_json::json!({ "default_harness": name }))
        .send()
        .await
        .unwrap()
}

#[tokio::test]
async fn an_embedded_default_harness_is_accepted() {
    let daemon = TestDaemon::spawn(seed()).await.unwrap();
    for name in ["claude", "opencode", "copilot"] {
        let resp = put_default_harness(&daemon, name).await;
        assert_eq!(
            resp.status(),
            200,
            "an embedded harness `{name}` must be an accepted default"
        );
    }
}

#[tokio::test]
async fn a_default_harness_that_does_not_resolve_is_refused() {
    let daemon = TestDaemon::spawn(seed()).await.unwrap();
    let resp = put_default_harness(&daemon, "totally-unknown-harness-xyz").await;
    assert_eq!(
        resp.status(),
        400,
        "an unresolvable default harness must be refused at registration (correctif 10)"
    );
    let body: serde_json::Value = resp.json().await.unwrap();
    let err = body["error"].as_str().unwrap_or_default();
    assert!(
        err.contains("totally-unknown-harness-xyz") && err.contains("default harness"),
        "the refusal must name the bad harness: {err}"
    );
}

#[tokio::test]
async fn clearing_the_default_harness_is_accepted() {
    // "" is the clear sentinel — accepted, so a user can reset to the floor.
    let daemon = TestDaemon::spawn(seed()).await.unwrap();
    let resp = put_default_harness(&daemon, "").await;
    assert_eq!(resp.status(), 200, "the empty clear sentinel is accepted");
}
