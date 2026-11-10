//! The composition root (#915): `pdo::serve(config, extensions)` starts the
//! daemon, and the `identify` middleware puts an Acteur on every request before
//! any handler sees it.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::extract::Request;
use axum::middleware::{self, Next};
use pdo::extension_api::{Extension, Manifest, RequestParts};
use pdo::DaemonConfig;
use pdo::{serve, Config, Daemon};
use pdo_kernel::actor::{SESSION_NODE_HEADER, SESSION_RUN_HEADER};
use pdo_kernel::Actor;

/// A daemon that spawns no real agent and runs no background loop.
/// A daemon that fires nothing on its own, whose home (hence the extensions'
/// data folders) is under `repo`, never the real `$HOME`.
fn quiet(repo: &std::path::Path) -> DaemonConfig {
    DaemonConfig {
        sandbox_home_override: Some(repo.join("home")),
        tmux_cmd_override: Some("exec sleep 600".to_string()),
        run_update_check_loop: false,
        run_trigger_scheduler_loop: false,
        price_refresh_at_boot: false,
        nested_daemon: true,
        ..DaemonConfig::default()
    }
}

type Seen = Arc<Mutex<Vec<(String, Option<Actor>)>>>;

/// Start a daemon through `serve`, recording the actor each request carries
/// once it is past `identify`.
async fn daemon_recording(
    repo: &std::path::Path,
    extensions: Vec<Box<dyn Extension>>,
) -> (Daemon, Seen) {
    let seen: Seen = Arc::default();
    let log = seen.clone();
    let config = Config::new(SocketAddr::from(([127, 0, 0, 1], 0)), repo)
        .with_daemon_config(quiet(repo))
        .with_inner_layer(move |router| {
            let log = log.clone();
            router.layer(middleware::from_fn(move |req: Request, next: Next| {
                let log = log.clone();
                async move {
                    let actor = req.extensions().get::<Actor>().cloned();
                    log.lock()
                        .unwrap()
                        .push((req.uri().path().to_string(), actor));
                    next.run(req).await
                }
            }))
        });
    (serve(config, extensions).await.unwrap(), seen)
}

#[tokio::test]
async fn serve_with_no_extension_starts_the_daemon() {
    let repo = tempfile::tempdir().unwrap();
    let daemon = serve(
        Config::new(SocketAddr::from(([127, 0, 0, 1], 0)), repo.path())
            .with_daemon_config(quiet(repo.path())),
        vec![],
    )
    .await
    .unwrap();
    let base = format!("http://{}", daemon.addr());

    let runs = reqwest::get(format!("{base}/runs")).await.unwrap();
    assert_eq!(runs.status(), 200);
    assert_eq!(
        runs.json::<serde_json::Value>().await.unwrap(),
        serde_json::json!([])
    );

    let sessions: serde_json::Value = reqwest::get(format!("{base}/sessions"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(sessions["version"], env!("CARGO_PKG_VERSION"));
}

#[tokio::test]
async fn identify_puts_the_local_operator_on_every_request_by_default() {
    let repo = tempfile::tempdir().unwrap();
    let (daemon, seen) = daemon_recording(repo.path(), vec![]).await;
    let base = format!("http://{}", daemon.addr());
    let client = reqwest::Client::new();

    client.get(format!("{base}/runs")).send().await.unwrap();
    client
        .get(format!("{base}/pipelines"))
        .send()
        .await
        .unwrap();
    client.get(format!("{base}/settings")).send().await.unwrap();
    client
        .post(format!("{base}/runs/no-such-run/nodes/no-such-node/done"))
        .json(&serde_json::json!({ "iter": 1 }))
        .send()
        .await
        .unwrap();
    // The SPA fallback goes through `identify` too.
    client.get(format!("{base}/")).send().await.unwrap();

    let seen = seen.lock().unwrap();
    let paths: Vec<&str> = seen.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(
        paths,
        vec![
            "/runs",
            "/pipelines",
            "/settings",
            "/runs/no-such-run/nodes/no-such-node/done",
            "/"
        ]
    );
    for (path, actor) in seen.iter() {
        assert_eq!(actor.as_ref(), Some(&Actor::LocalOperator), "{path}");
    }
}

#[tokio::test]
async fn a_call_from_a_node_session_is_the_agent_of_that_node() {
    let repo = tempfile::tempdir().unwrap();
    let (daemon, seen) = daemon_recording(repo.path(), vec![]).await;

    // The headers the agent CLI (`pdo complete`, `pdo fail`…) sends.
    reqwest::Client::new()
        .post(format!("http://{}/runs/r1/nodes/n1/done", daemon.addr()))
        .header(SESSION_RUN_HEADER, "r1")
        .header(SESSION_NODE_HEADER, "n1")
        .json(&serde_json::json!({ "iter": 1 }))
        .send()
        .await
        .unwrap();

    let seen = seen.lock().unwrap();
    assert_eq!(
        seen[0].1,
        Some(Actor::Agent {
            run_id: "r1".into(),
            node_id: "n1".into()
        })
    );
}

/// An extension that knows who the caller is from a header.
struct HeaderIdentity;

impl Extension for HeaderIdentity {
    fn manifest(&self) -> Manifest {
        Manifest::new("header-identity", "Header identity", "0.0.0")
    }

    fn identify(&self, request: &RequestParts) -> Option<Actor> {
        let user = request.headers.get("x-test-user")?.to_str().ok()?;
        Some(Actor::Identified {
            namespace: "header-identity".into(),
            id: user.into(),
        })
    }
}

#[tokio::test]
async fn an_extension_identifies_the_caller_and_the_core_falls_back_otherwise() {
    let repo = tempfile::tempdir().unwrap();
    let (daemon, seen) = daemon_recording(repo.path(), vec![Box::new(HeaderIdentity)]).await;
    let base = format!("http://{}", daemon.addr());
    let client = reqwest::Client::new();

    client
        .get(format!("{base}/runs"))
        .header("x-test-user", "alice")
        .send()
        .await
        .unwrap();
    client.get(format!("{base}/runs")).send().await.unwrap();

    let seen = seen.lock().unwrap();
    assert_eq!(
        seen[0].1,
        Some(Actor::Identified {
            namespace: "header-identity".into(),
            id: "alice".into()
        })
    );
    assert_eq!(seen[1].1, Some(Actor::LocalOperator));
}
