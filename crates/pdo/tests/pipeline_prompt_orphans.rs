//! Layer 3a — the sidecar prompt dir must never make a pipeline un-portable.
//!
//! Prompts live one file per node in `<stem>.prompts/<node-id>.md`, and the
//! portable document carries them as a map keyed by node id. Deleting a node
//! used to leave its `.md` behind: the export then emitted a key naming no
//! node, and PDO's own importer rejected the whole document — on the *other*
//! machine, the one that cannot fix the source.
//!
//! The unit tests in `portable_pipeline` build the prompt map in memory, so it
//! can never hold an orphan. These go through the real filesystem sidecar and
//! the real HTTP handlers, which is exactly where the bug lived.

use crate::common::TestDaemon;

const TWO_NODE_YAML: &str = r#"name: portable-prompts
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: worker
    name: Worker
    type: doc-only
    inputs:
      - name: in
    outputs:
      - name: out
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: worker, port: in }
  - source: { node: worker, port: out }
    target: { node: end, port: result }
"#;

/// The same pipeline with `worker` removed — what the editor sends after a
/// right-click → Delete → Save.
const ONE_NODE_YAML: &str = r#"name: portable-prompts
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: end, port: result }
"#;

fn seed(repo: &std::path::Path) -> anyhow::Result<()> {
    let dir = repo.join(".pdo").join("pipelines");
    std::fs::create_dir_all(dir.join("portable-prompts.prompts"))?;
    std::fs::write(dir.join("portable-prompts.yaml"), TWO_NODE_YAML)?;
    std::fs::write(
        dir.join("portable-prompts.prompts").join("worker.md"),
        "Review carefully.",
    )?;
    Ok(())
}

/// The source of the poison: a save that drops a node must drop its prompt too.
#[tokio::test]
async fn saving_without_a_node_removes_its_prompt_file() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let prompts_dir = daemon
        .repo_root()
        .join(".pdo")
        .join("pipelines")
        .join("portable-prompts.prompts");
    assert!(prompts_dir.join("worker.md").exists());

    let resp = reqwest::Client::new()
        .put(format!("{}/pipelines/portable-prompts", daemon.url()))
        .json(&serde_json::json!({ "yaml": ONE_NODE_YAML, "prompts": {} }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    assert!(
        !prompts_dir.join("worker.md").exists(),
        "the prompt of the deleted node survived the save"
    );
}

/// A save that keeps a node must keep its prompt, even when the client sends no
/// prompt map at all — pruning is driven by the saved YAML, not by the request.
#[tokio::test]
async fn saving_without_sending_prompts_keeps_the_prompts_of_live_nodes() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let prompts_dir = daemon
        .repo_root()
        .join(".pdo")
        .join("pipelines")
        .join("portable-prompts.prompts");

    let resp = reqwest::Client::new()
        .put(format!("{}/pipelines/portable-prompts", daemon.url()))
        .json(&serde_json::json!({ "yaml": TWO_NODE_YAML }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    assert_eq!(
        std::fs::read_to_string(prompts_dir.join("worker.md")).unwrap(),
        "Review carefully."
    );
}

/// The reported journey, minus the second machine: a pipeline already poisoned
/// on disk (by a PDO that predates the fix) must still export a document its own
/// importer accepts.
#[tokio::test]
async fn a_pipeline_with_an_orphan_prompt_on_disk_still_round_trips() {
    let daemon = TestDaemon::spawn(|repo| {
        seed(repo)?;
        // The leftover of a node deleted before the fix landed.
        std::fs::write(
            repo.join(".pdo")
                .join("pipelines")
                .join("portable-prompts.prompts")
                .join("FBKE6BhH.md"),
            "Prompt of a node that no longer exists.",
        )?;
        Ok(())
    })
    .await
    .unwrap();
    let client = reqwest::Client::new();

    let document = client
        .get(format!(
            "{}/pipelines/portable-prompts/document",
            daemon.url()
        ))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        !document.contains("FBKE6BhH"),
        "the export carried the dead key:\n{document}"
    );

    let resp = client
        .post(format!("{}/pipelines/import", daemon.url()))
        .json(&serde_json::json!({ "document": document }))
        .send()
        .await
        .unwrap();
    let status = resp.status();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(status, 201, "{body}");

    let imported = daemon
        .repo_root()
        .join(".pdo")
        .join("pipelines")
        .join(format!("{}.prompts", body["id"].as_str().unwrap()));
    assert!(imported.join("worker.md").exists());
    assert!(!imported.join("FBKE6BhH.md").exists());
    assert_eq!(body["warnings"].as_array().unwrap().len(), 0);
}

/// A document written by an older PDO still holds the dead key. It is a
/// leftover, not a corruption: import it, drop it, and say so.
#[tokio::test]
async fn a_document_carrying_a_dead_prompt_key_imports_with_a_warning() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let client = reqwest::Client::new();

    let document = client
        .get(format!(
            "{}/pipelines/portable-prompts/document",
            daemon.url()
        ))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    // Re-poison the document the way pre-fix PDO emitted it.
    let document = format!("{document}  FBKE6BhH: Prompt of a deleted node.\n");
    assert!(document.contains("FBKE6BhH"));

    let resp = client
        .post(format!("{}/pipelines/import", daemon.url()))
        .json(&serde_json::json!({ "document": document }))
        .send()
        .await
        .unwrap();
    let status = resp.status();
    let body: serde_json::Value = resp.json().await.unwrap();

    assert_eq!(status, 201, "{body}");
    let warnings = body["warnings"].as_array().unwrap();
    assert_eq!(warnings.len(), 1, "{body}");
    assert!(
        warnings[0].as_str().unwrap().contains("prompts.FBKE6BhH"),
        "{body}"
    );
    let imported = daemon
        .repo_root()
        .join(".pdo")
        .join("pipelines")
        .join(format!("{}.prompts", body["id"].as_str().unwrap()));
    assert!(!imported.join("FBKE6BhH.md").exists());
}

/// The one prompt key that stays fatal: it names a path, not a node.
#[tokio::test]
async fn a_prompt_key_that_escapes_the_sidecar_dir_is_still_refused() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let client = reqwest::Client::new();

    let document = client
        .get(format!(
            "{}/pipelines/portable-prompts/document",
            daemon.url()
        ))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let document = format!("{document}  ../../outside: injected\n");

    let resp = client
        .post(format!("{}/pipelines/import", daemon.url()))
        .json(&serde_json::json!({ "document": document }))
        .send()
        .await
        .unwrap();
    let status = resp.status();
    let body: serde_json::Value = resp.json().await.unwrap();

    assert_eq!(status, 400, "{body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("cannot be used as a prompt filename"),
        "{body}"
    );
}

/// Duplicating a poisoned pipeline must not carry the leftover into the copy.
#[tokio::test]
async fn duplicating_a_pipeline_leaves_its_orphan_prompts_behind() {
    let daemon = TestDaemon::spawn(|repo| {
        seed(repo)?;
        std::fs::write(
            repo.join(".pdo")
                .join("pipelines")
                .join("portable-prompts.prompts")
                .join("FBKE6BhH.md"),
            "Prompt of a node that no longer exists.",
        )?;
        Ok(())
    })
    .await
    .unwrap();

    let resp = reqwest::Client::new()
        .post(format!(
            "{}/pipelines/portable-prompts/duplicate",
            daemon.url()
        ))
        .send()
        .await
        .unwrap();
    let status = resp.status();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(status, 201, "{body}");

    let copy = daemon
        .repo_root()
        .join(".pdo")
        .join("pipelines")
        .join(format!("{}.prompts", body["id"].as_str().unwrap()));
    assert!(copy.join("worker.md").exists());
    assert!(!copy.join("FBKE6BhH.md").exists());
}
