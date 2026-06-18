//! Layer 3a — issue #72: external edits to a pipeline YAML while the canvas may
//! have unsaved changes. The daemon must still emit `pipeline_changed` on the
//! WebSocket so the frontend can decide whether to show the conflict modal.
//!
//! This test proves: writing to a pipeline YAML on disk (bypassing the API)
//! produces a `pipeline_changed` event on the WebSocket bus with the correct
//! `pipeline_id`, regardless of how many times the file is modified.

mod common;

use std::time::Duration;

use common::{ws_text, TestDaemon};
use futures_util::StreamExt;
use tokio::time::timeout;

const PIPELINE_NAME: &str = "hot-reload-target";
const INITIAL_YAML: &str = r#"name: hot-reload-target
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

const EXTERNALLY_MODIFIED_YAML: &str = r#"name: hot-reload-target
version: "1.1"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: worker
    name: worker-renamed
    type: doc-only
    inputs:
      - name: task
      - name: context
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
async fn external_edit_emits_pipeline_changed_for_conflict_detection() {
    let daemon = TestDaemon::spawn(seed_pipeline).await.unwrap();

    let mut ws = daemon.connect_ws().await.unwrap();
    // drain the initial `ready`
    let _ = ws.next().await.unwrap().unwrap();

    // Simulate an external edit (vim, VS Code, git checkout, etc.)
    std::fs::write(pipeline_path(daemon.repo_root()), EXTERNALLY_MODIFIED_YAML).unwrap();

    let evt = next_pipeline_changed_for(&mut ws, PIPELINE_NAME, Duration::from_secs(4))
        .await
        .expect("external edit must emit pipeline_changed within 4s");

    assert_eq!(evt["type"], "pipeline_changed");
    assert_eq!(evt["pipeline_id"], PIPELINE_NAME);
    assert!(
        evt["path"].as_str().unwrap().contains(PIPELINE_NAME),
        "path should reference the pipeline file"
    );
}

#[tokio::test]
async fn successive_external_edits_each_emit_pipeline_changed() {
    let daemon = TestDaemon::spawn(seed_pipeline).await.unwrap();

    let mut ws = daemon.connect_ws().await.unwrap();
    let _ = ws.next().await.unwrap().unwrap();

    // First external edit
    std::fs::write(pipeline_path(daemon.repo_root()), EXTERNALLY_MODIFIED_YAML).unwrap();
    let evt1 = next_pipeline_changed_for(&mut ws, PIPELINE_NAME, Duration::from_secs(4))
        .await
        .expect("first external edit must emit pipeline_changed");
    assert_eq!(evt1["pipeline_id"], PIPELINE_NAME);

    // Wait for debounce to settle before second edit
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Second external edit (revert to original)
    std::fs::write(pipeline_path(daemon.repo_root()), INITIAL_YAML).unwrap();
    let evt2 = next_pipeline_changed_for(&mut ws, PIPELINE_NAME, Duration::from_secs(4))
        .await
        .expect("second external edit must also emit pipeline_changed");
    assert_eq!(evt2["pipeline_id"], PIPELINE_NAME);
}
