//! Layer 3 — the release channel of a distribution (#24).
//!
//! A distribution is a binary that calls `pdo::serve` with its own extensions
//! (ADR-0081). It declares where it checks for updates, the version it compares,
//! and how it updates, with `Config::with_release_channel`. Each daemon here goes
//! through `serve`, as such a binary would; its home is a tempdir (never the real
//! `~/.pdo` nor `~/.config`) and its update executor is a fixture script, so no
//! test ever runs a real upgrade.
//!
//! What is proven here:
//!   1. the channel's version is compared with ITS source, and `GET /update` says
//!      so (`installed_version`, `latest_version`, `channel_name`, `core_version`,
//!      `manual_command`); Update runs the declared command, not a vanilla one;
//!   2. without an upgrade command, Update is refused, even with a cargo-dist
//!      receipt that makes the same machine's vanilla daemon offer it;
//!   3. without a source, nothing is fetched and the read says why, whatever an
//!      earlier check left in the shared cache;
//!   4. the operator's `PDO_UPDATE_SOURCE_URL` still wins over the channel's source.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use pdo::DaemonConfig;
use pdo::{serve, Config, Daemon, ReleaseChannel};
use pdo_platform::domain::install::Supervision;

/// A `releases/latest` endpoint publishing `v0.4.0`, counting its hits.
struct Fixture {
    addr: SocketAddr,
    hits: Arc<AtomicUsize>,
    _task: tokio::task::JoinHandle<()>,
}

impl Fixture {
    async fn spawn() -> Self {
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = hits.clone();
        let app = axum::Router::new().route(
            "/releases/latest",
            axum::routing::get(move || {
                let counter = counter.clone();
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    (
                        [("content-type", "application/json")],
                        r#"{"tag_name":"v0.4.0","prerelease":false}"#,
                    )
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Self {
            addr,
            hits,
            _task: task,
        }
    }

    fn url(&self) -> String {
        format!("http://{}/releases/latest", self.addr)
    }

    fn hits(&self) -> usize {
        self.hits.load(Ordering::SeqCst)
    }
}

/// A daemon that fires nothing on its own, whose home is `home`, and whose
/// update executor is a fixture that echoes the command it receives, then exits 0.
fn quiet(home: &Path) -> DaemonConfig {
    let executor = home.join("executor.sh");
    std::fs::create_dir_all(home).unwrap();
    std::fs::write(
        &executor,
        "#!/bin/sh\necho \"cmd=$PDO_UPDATE_COMMAND\"\necho \"method=$PDO_UPDATE_METHOD\"\nexit 0\n",
    )
    .unwrap();
    DaemonConfig {
        sandbox_home_override: Some(home.to_path_buf()),
        tmux_cmd_override: Some("exec sleep 600".to_string()),
        update_executor_override: Some(format!("sh {}", executor.display())),
        supervision_override: Some(Supervision::None),
        run_update_check_loop: false,
        run_trigger_scheduler_loop: false,
        price_refresh_at_boot: false,
        nested_daemon: true,
        ..DaemonConfig::default()
    }
}

fn acme(source_url: Option<String>, upgrade_command: Option<&str>) -> ReleaseChannel {
    ReleaseChannel {
        name: "acme".into(),
        installed_version: "0.3.0".into(),
        source_url,
        upgrade_command: upgrade_command.map(str::to_string),
    }
}

/// Start a daemon through `serve`, under `root/repo` with its home in
/// `root/home`, with the release channel `channel` (none: the vanilla one).
async fn start(
    root: &Path,
    daemon: DaemonConfig,
    channel: Option<ReleaseChannel>,
) -> (Daemon, String) {
    // The `env` tier of the update check wins over every other: a daemon that
    // runs this suite from its own node session exports `PDO_UPDATE_CHECK=0`,
    // which would turn every check here into a `409`. Nothing in this binary
    // sets it (nextest runs one process per test).
    std::env::remove_var("PDO_UPDATE_CHECK");
    let repo = root.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let mut config =
        Config::new(SocketAddr::from(([127, 0, 0, 1], 0)), &repo).with_daemon_config(daemon);
    if let Some(channel) = channel {
        config = config.with_release_channel(channel);
    }
    let daemon = serve(config, vec![]).await.unwrap();
    let base = format!("http://{}", daemon.addr());
    (daemon, base)
}

fn home(root: &Path) -> PathBuf {
    root.join("home")
}

async fn update_json(base: &str) -> serde_json::Value {
    reqwest::get(format!("{base}/update"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

async fn post(base: &str, path: &str) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{base}{path}"))
        .send()
        .await
        .unwrap()
}

#[tokio::test]
async fn a_distribution_compares_its_own_version_and_updates_with_its_own_command() {
    let _env = crate::common::lock_update_check_unset();
    let fixture = Fixture::spawn().await;
    let root = tempfile::tempdir().unwrap();
    let (_daemon, base) = start(
        root.path(),
        quiet(&home(root.path())),
        Some(acme(Some(fixture.url()), Some("acme-upgrade"))),
    )
    .await;

    let resp = post(&base, "/update/check").await;
    assert_eq!(resp.status(), 200, "{:?}", resp.text().await);
    let body = update_json(&base).await;
    assert_eq!(body["installed_version"], "0.3.0");
    assert_eq!(body["latest_version"], "0.4.0");
    assert_eq!(body["newer_available"], true);
    assert_eq!(body["source_url"], fixture.url());
    assert_eq!(body["manual_command"], "acme-upgrade");
    assert_eq!(body["channel_name"], "acme");
    assert_eq!(body["core_version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(body["install_method"], "declared");
    assert_eq!(body["can_apply"], true);
    assert_eq!(fixture.hits(), 1);

    // Update runs the declared command (through the fixture executor).
    let resp = post(&base, "/update/apply").await;
    assert_eq!(resp.status(), 202);
    let started: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(started["command"], "acme-upgrade");
    assert_eq!(started["method"], "declared");
    assert_eq!(started["from_version"], "0.3.0");
    let id = started["attempt_id"].as_str().unwrap().to_string();
    let log_path = home(root.path()).join(format!(".pdo/update/{id}.log"));
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let log = std::fs::read_to_string(&log_path).unwrap_or_default();
        if log.contains("cmd=acme-upgrade") {
            assert!(log.contains("method=declared"), "{log}");
            break;
        }
        assert!(Instant::now() < deadline, "the executor never ran: {log}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn a_distribution_without_an_upgrade_command_never_applies_even_with_a_receipt() {
    let _env = crate::common::lock_update_check_unset();
    let fixture = Fixture::spawn().await;
    let root = tempfile::tempdir().unwrap();
    // The receipt a cargo-dist install (of the vanilla v1, say) leaves behind, in
    // the config home of the test (`<home>/.config`).
    let receipt = home(root.path()).join(".config/pdo-daemon/pdo-daemon-receipt.json");
    std::fs::create_dir_all(receipt.parent().unwrap()).unwrap();
    std::fs::write(&receipt, "{}").unwrap();

    // Control: the vanilla daemon of the same home sees it, and would offer Update.
    let vanilla_root = root.path().join("vanilla");
    let (_vanilla, vanilla_base) = start(&vanilla_root, quiet(&home(root.path())), None).await;
    let vanilla = update_json(&vanilla_base).await;
    assert_eq!(vanilla["channel_name"], "PDO");
    assert_eq!(vanilla["install_method"], "script", "{vanilla}");
    assert_eq!(vanilla["can_apply"], true);

    let (_daemon, base) = start(
        root.path(),
        quiet(&home(root.path())),
        Some(acme(Some(fixture.url()), None)),
    )
    .await;
    let body = update_json(&base).await;
    assert_eq!(body["can_apply"], false);
    assert!(
        body["apply_blocked_reason"]
            .as_str()
            .is_some_and(|r| !r.is_empty()),
        "{body}"
    );
    assert_eq!(body["install_method"], "declared");
    assert_eq!(body["manual_command"], serde_json::Value::Null);

    let resp = post(&base, "/update/apply").await;
    assert_eq!(resp.status(), 409);
    let refused: serde_json::Value = resp.json().await.unwrap();
    assert!(refused["error"]
        .as_str()
        .is_some_and(|e| e.starts_with("acme declares no update command")));
    assert!(
        !home(root.path())
            .join(".pdo/update/last-attempt.json")
            .exists(),
        "no attempt was journaled"
    );
}

#[tokio::test]
async fn a_distribution_without_a_source_asks_nothing_and_says_why() {
    let _env = crate::common::lock_update_check_unset();
    let fixture = Fixture::spawn().await;
    let root = tempfile::tempdir().unwrap();
    // What an earlier check of the fixture left in the cache the homes share.
    let cache = home(root.path()).join(".pdo/update/check.json");
    std::fs::create_dir_all(cache.parent().unwrap()).unwrap();
    std::fs::write(
        &cache,
        serde_json::json!({
            "schema": "update-check-v1",
            "source": fixture.url(),
            "checked_at": chrono::Utc::now().to_rfc3339(),
            "latest_version": "0.4.0",
            "error": null,
        })
        .to_string(),
    )
    .unwrap();
    let mut daemon = quiet(&home(root.path()));
    // The boot and periodic checks run too: they must not fetch anything either.
    daemon.run_update_check_loop = true;
    let (_daemon, base) = start(root.path(), daemon, Some(acme(None, Some("acme-upgrade")))).await;

    let resp = post(&base, "/update/check").await;
    assert_eq!(resp.status(), 409);
    let body = update_json(&base).await;
    assert_eq!(body["latest_version"], serde_json::Value::Null);
    assert_eq!(body["newer_available"], false);
    assert!(
        body["reason"].as_str().is_some_and(|r| !r.is_empty()),
        "{body}"
    );
    assert_eq!(body["source_url"], serde_json::Value::Null);
    let changelog: serde_json::Value = reqwest::get(format!("{base}/update/changelog"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(changelog["fallback_reason"].as_str().is_some());
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(fixture.hits(), 0, "no request reaches the fixture");
}

#[tokio::test]
async fn the_operators_source_still_wins_over_the_channels() {
    let _env = crate::common::lock_update_check_unset();
    let channel_source = Fixture::spawn().await;
    let operator_source = Fixture::spawn().await;
    let root = tempfile::tempdir().unwrap();
    let mut daemon = quiet(&home(root.path()));
    // What `PDO_UPDATE_SOURCE_URL` sets.
    daemon.update_source_url = Some(operator_source.url());
    let (_daemon, base) = start(
        root.path(),
        daemon,
        Some(acme(Some(channel_source.url()), Some("acme-upgrade"))),
    )
    .await;

    assert_eq!(post(&base, "/update/check").await.status(), 200);
    let body = update_json(&base).await;
    assert_eq!(body["source_url"], operator_source.url());
    assert_eq!(body["latest_version"], "0.4.0");
    assert_eq!(operator_source.hits(), 1);
    assert_eq!(channel_source.hits(), 0);
}

#[tokio::test]
async fn the_vanilla_binary_is_pdo_at_the_cores_version_checking_the_public_repository() {
    let _env = crate::common::lock_update_check_unset();
    let root = tempfile::tempdir().unwrap();
    // No check runs (no loop, no `POST /update/check`): nothing leaves the machine.
    let (_daemon, base) = start(root.path(), quiet(&home(root.path())), None).await;
    let body = update_json(&base).await;
    assert_eq!(body["channel_name"], "PDO");
    assert_eq!(body["installed_version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(body["core_version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(
        body["source_url"],
        "https://api.github.com/repos/Loulen/prompt-driven-orchestrator/releases/latest"
    );
    assert_eq!(body["source"], "GitHub Releases");
    assert_eq!(body["latest_version"], serde_json::Value::Null);
}
