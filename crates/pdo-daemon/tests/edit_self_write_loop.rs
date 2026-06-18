//! Layer 3a — proves Bug E (#17) is fixed: the daemon's `pipeline_watcher` no
//! longer broadcasts `pipeline_changed` for writes the daemon performed itself
//! (PUT /pipelines/:id), but still broadcasts for external writes.

mod common;

use std::time::Duration;

use common::{ws_text, TestDaemon};
use futures_util::{SinkExt, StreamExt};
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::protocol::Message;

const PIPELINE_NAME: &str = "editable";
const INITIAL_YAML: &str = r#"name: editable
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: worker
    name: worker
    type: doc-only
    inputs:
      - name: task
    outputs:
      - name: result
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: worker, port: task }
"#;

const UPDATED_YAML: &str = r#"name: editable
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: worker
    name: worker
    type: doc-only
    inputs:
      - name: task
      - name: extra
    outputs:
      - name: result
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: worker, port: task }
"#;

fn seed_pipeline(repo: &std::path::Path) -> anyhow::Result<()> {
    let dir = repo.join(".pdo").join("pipelines");
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join(format!("{PIPELINE_NAME}.yaml")), INITIAL_YAML)?;
    Ok(())
}

fn pipeline_path(repo: &std::path::Path) -> std::path::PathBuf {
    repo.join(".pdo")
        .join("pipelines")
        .join(format!("{PIPELINE_NAME}.yaml"))
}

/// Read pipeline_changed events for `pipeline_id` until the deadline elapses or
/// the WS closes. Discards `ready` / `heartbeat`. Returns the first match found,
/// or `None` if the deadline hit silence.
async fn next_pipeline_changed_for(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    pipeline_id: &str,
    deadline: Duration,
) -> Option<serde_json::Value> {
    let result = timeout(deadline, async {
        loop {
            let next = ws.next().await?;
            let msg = next.ok()?;
            let Some(text) = ws_text(&msg) else {
                continue;
            };
            let parsed: serde_json::Value = serde_json::from_str(text).ok()?;
            if parsed["type"] == "pipeline_changed" && parsed["pipeline_id"] == pipeline_id {
                return Some(parsed);
            }
        }
    })
    .await;
    result.ok().flatten()
}

#[tokio::test]
async fn put_pipeline_does_not_trigger_self_broadcast() {
    let daemon = TestDaemon::spawn(seed_pipeline).await.unwrap();

    let mut ws = daemon.connect_ws().await.unwrap();
    // drain the initial `ready`
    let _ = ws.next().await.unwrap().unwrap();

    let body = serde_json::json!({ "yaml": UPDATED_YAML, "prompts": {} });
    let resp = reqwest::Client::new()
        .put(format!("{}/pipelines/{}", daemon.url(), PIPELINE_NAME))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "PUT should succeed");

    // Watcher debounce is 1s + self-write TTL is 2s. 3s of silence proves the
    // suppression worked end-to-end.
    let evt = next_pipeline_changed_for(&mut ws, PIPELINE_NAME, Duration::from_secs(3)).await;
    assert!(
        evt.is_none(),
        "expected no pipeline_changed broadcast for self-write, got {evt:?}"
    );
}

#[tokio::test]
async fn external_write_still_triggers_broadcast() {
    let daemon = TestDaemon::spawn(seed_pipeline).await.unwrap();

    let mut ws = daemon.connect_ws().await.unwrap();
    let _ = ws.next().await.unwrap().unwrap();

    // Write directly to disk — bypassing the API. This is what vim / git checkout
    // / a future Pipeline Manager looks like to the watcher. The fix must NOT
    // break detection of these.
    std::fs::write(pipeline_path(daemon.repo_root()), UPDATED_YAML).unwrap();

    let evt = next_pipeline_changed_for(&mut ws, PIPELINE_NAME, Duration::from_secs(4))
        .await
        .expect("external write should still emit pipeline_changed within 4s");

    assert_eq!(evt["type"], "pipeline_changed");
    assert_eq!(evt["pipeline_id"], PIPELINE_NAME);
}

#[tokio::test]
async fn read_only_get_does_not_trigger_broadcast() {
    // Bug F: notify-debouncer-mini reports events for plain reads on this
    // platform, which previously fed into a self-perpetuating broadcast loop
    // (broadcast → frontend GETs → watcher fires → broadcast). The mtime
    // dedup filter must catch these.
    let daemon = TestDaemon::spawn(seed_pipeline).await.unwrap();

    let mut ws = daemon.connect_ws().await.unwrap();
    let _ = ws.next().await.unwrap().unwrap();

    // GET the pipeline a few times. Each call invokes std::fs::read_to_string
    // inside the daemon, which the watcher used to amplify into a broadcast.
    for _ in 0..3 {
        reqwest::get(format!("{}/pipelines/{}", daemon.url(), PIPELINE_NAME))
            .await
            .unwrap()
            .error_for_status()
            .unwrap();
    }

    // Generous window: the mini debouncer aggregates over 1s, give it a couple
    // of cycles to be sure nothing arrives.
    let evt = next_pipeline_changed_for(&mut ws, PIPELINE_NAME, Duration::from_secs(3)).await;
    assert!(
        evt.is_none(),
        "GET should not produce pipeline_changed, got {evt:?}"
    );
}

#[tokio::test]
async fn external_write_after_self_write_ttl_expiry_still_broadcasts() {
    // Regression check: the TTL is *time-bounded*, not "once and forever".
    // After 2s + safety margin, an external write to the same path must trigger
    // the broadcast again. This proves we don't accidentally permanently mute
    // a path after the daemon writes it.
    let daemon = TestDaemon::spawn(seed_pipeline).await.unwrap();

    let mut ws = daemon.connect_ws().await.unwrap();
    let _ = ws.next().await.unwrap().unwrap();

    let body = serde_json::json!({ "yaml": UPDATED_YAML, "prompts": {} });
    reqwest::Client::new()
        .put(format!("{}/pipelines/{}", daemon.url(), PIPELINE_NAME))
        .json(&body)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();

    // Wait past the 2s self-write TTL. The first PUT's debounced event will
    // fire ~1s after the write and be suppressed; we sleep enough to clear the
    // window before doing the external write.
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Drain anything that snuck in (heartbeat etc.) so the next assertion
    // reflects events caused by *our* external write.
    let _ = timeout(Duration::from_millis(50), ws.next()).await;

    std::fs::write(pipeline_path(daemon.repo_root()), INITIAL_YAML).unwrap();
    let evt = next_pipeline_changed_for(&mut ws, PIPELINE_NAME, Duration::from_secs(4))
        .await
        .expect("post-TTL external write should broadcast");
    assert_eq!(evt["pipeline_id"], PIPELINE_NAME);

    // Force-close the socket so the daemon's WS handler exits cleanly.
    let _ = ws.send(Message::Close(None)).await;
}
