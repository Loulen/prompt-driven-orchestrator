//! The session context's mutations go through the Autorisation d'opération
//! (#920): an extension that refuses them turns each gesture into a `403` with
//! its reason, and the reads stay open.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use pdo::extension_api::{Actor, Authorization, Extension, Manifest, Operation, ResourceKind};
use pdo::DaemonConfig;
use pdo::{serve, Config};

/// Refuses every Operation on a session or a node, and records what it saw.
struct NoSessions(Arc<Mutex<Vec<Operation>>>);

impl Extension for NoSessions {
    fn manifest(&self) -> Manifest {
        Manifest::new("no-sessions", "No sessions", "0.0.0")
    }

    fn authorize(&self, _actor: &Actor, operation: &Operation) -> Authorization {
        self.0.lock().unwrap().push(operation.clone());
        match operation.resource {
            ResourceKind::Session | ResourceKind::Node | ResourceKind::Pipeline => {
                Authorization::deny("sessions are closed today")
            }
            _ => Authorization::Allow,
        }
    }
}

#[tokio::test]
async fn a_refusing_extension_turns_every_session_gesture_into_a_named_403() {
    let repo = tempfile::tempdir().unwrap();
    let seen: Arc<Mutex<Vec<Operation>>> = Arc::default();
    let daemon = serve(
        Config::new(SocketAddr::from(([127, 0, 0, 1], 0)), repo.path()).with_daemon_config(
            DaemonConfig {
                // The extensions' data folders stay out of the real `$HOME`.
                sandbox_home_override: Some(repo.path().join("home")),
                tmux_cmd_override: Some("exec sleep 600".to_string()),
                run_update_check_loop: false,
                run_trigger_scheduler_loop: false,
                price_refresh_at_boot: false,
                nested_daemon: true,
                ..DaemonConfig::default()
            },
        ),
        vec![Box::new(NoSessions(seen.clone()))],
    )
    .await
    .unwrap();
    let base = format!("http://{}", daemon.addr());
    let client = reqwest::Client::new();
    let refused = serde_json::json!({
        "error": "operation_refused",
        "message": "operation refused: sessions are closed today",
        "reason": "sessions are closed today",
    });

    let open = client
        .post(format!("{base}/sessions/libassist"))
        .send()
        .await
        .unwrap();
    assert_eq!(open.status(), 403);
    assert_eq!(open.json::<serde_json::Value>().await.unwrap(), refused);

    let focus = client
        .put(format!("{base}/sessions/libassist/focus"))
        .json(&serde_json::json!({ "pipeline_id": "review" }))
        .send()
        .await
        .unwrap();
    assert_eq!(focus.status(), 403);

    let shell = client
        .post(format!("{base}/sessions/20260101-000000-abcdef0/shell"))
        .send()
        .await
        .unwrap();
    assert_eq!(shell.status(), 403);
    assert_eq!(shell.json::<serde_json::Value>().await.unwrap(), refused);

    let wait = client
        .post(format!(
            "{base}/runs/20260101-000000-abcdef0/nodes/worker/wait-user"
        ))
        .json(&serde_json::json!({ "iter": 1 }))
        .send()
        .await
        .unwrap();
    assert_eq!(wait.status(), 403);

    let close = client
        .delete(format!("{base}/sessions/libassist"))
        .send()
        .await
        .unwrap();
    assert_eq!(close.status(), 403);

    // The reads are not Operations: they stay open.
    let status = client.get(format!("{base}/sessions")).send().await.unwrap();
    assert_eq!(status.status(), 200);
    let read_focus = client
        .get(format!("{base}/sessions/libassist/focus"))
        .send()
        .await
        .unwrap();
    assert_eq!(read_focus.status(), 200);
    assert_eq!(
        read_focus.json::<serde_json::Value>().await.unwrap()["pipeline_id"],
        serde_json::Value::Null,
        "the refused focus was never declared"
    );

    let asked = seen.lock().unwrap().clone();
    assert!(
        asked.iter().any(|o| o.resource == ResourceKind::Session),
        "{asked:?}"
    );
    assert!(
        asked.iter().any(|o| o.resource == ResourceKind::Node),
        "{asked:?}"
    );
}
