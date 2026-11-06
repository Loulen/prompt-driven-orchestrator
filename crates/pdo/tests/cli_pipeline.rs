//! Layer 3a — `pdo pipeline` through the real binary, against a real daemon:
//! a pipeline listed and shown, saved into a new version, its history read and
//! an old version restored, exported then imported back without loss (its
//! skills zip included), validated, duplicated, renamed and deleted; the node
//! library and a pasted node; `4` for a pipeline or a version the daemon does
//! not know, `1` for a refusal.

use crate::cli_operator::pdo;
use crate::common::flow::{seed_solo, solo_pipeline_yaml};
use crate::common::TestDaemon;

fn seed(repo: &std::path::Path) -> anyhow::Result<()> {
    seed_solo(repo, &[("solo", false)])
}

fn write(dir: &std::path::Path, name: &str, content: &str) -> String {
    let path = dir.join(name);
    std::fs::write(&path, content).unwrap();
    path.display().to_string()
}

#[tokio::test]
async fn a_pipeline_is_saved_restored_and_listed_by_the_cli() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let files = tempfile::tempdir().unwrap();
    let original = solo_pipeline_yaml("solo", false);

    let list = pdo(&daemon, &["pipeline", "list"]).await;
    assert!(
        list.ok().contains("solo\tsolo\t"),
        "one line per pipeline: {}",
        list.stdout
    );
    let shown = pdo(&daemon, &["pipeline", "show", "solo"]).await;
    assert_eq!(shown.ok(), original, "show prints the file");
    let detail = pdo(&daemon, &["pipeline", "show", "solo", "--json"])
        .await
        .json();
    assert_eq!(detail["prompts"]["worker"], "Do the work.\n");

    // A save from a file freezes the next version, with the prompt given.
    let changed = original.replace("name: Worker", "name: Reviewer");
    let yaml = write(files.path(), "solo.yaml", &changed);
    let prompt = write(files.path(), "worker.md", "Review the work.\n");
    let saved = pdo(
        &daemon,
        &[
            "pipeline",
            "save",
            "solo",
            &yaml,
            "--prompt",
            &format!("worker={prompt}"),
        ],
    )
    .await;
    assert_eq!(saved.ok(), "solo saved as v2\n");
    let detail = daemon.get_json("/pipelines/solo").await;
    assert_eq!(detail["yaml"], changed.as_str());
    assert_eq!(detail["prompts"]["worker"], "Review the work.\n");

    let versions = pdo(&daemon, &["pipeline", "versions", "solo"]).await;
    let lines: Vec<&str> = versions.ok().lines().collect();
    assert_eq!(lines.len(), 2, "{}", versions.stdout);
    assert!(
        lines[0].starts_with("v2\tv2\t") && lines[0].ends_with("\tlocal_operator\t0 runs"),
        "{}",
        lines[0]
    );
    assert!(
        lines[1].starts_with("v1\t") && lines[1].ends_with("\t-\t0 runs"),
        "{}",
        lines[1]
    );
    let first = pdo(&daemon, &["pipeline", "version", "solo", "1"]).await;
    assert_eq!(first.ok(), original);

    // Restored: v1's content and prompts, saved as v3, the file again.
    let restored = pdo(&daemon, &["pipeline", "restore", "solo", "1"]).await;
    assert_eq!(restored.ok(), "v1 restored: solo saved as v3\n");
    let detail = daemon.get_json("/pipelines/solo").await;
    assert_eq!(detail["yaml"], original.as_str());
    assert_eq!(detail["prompts"]["worker"], "Do the work.\n");
    let history = daemon.get_json("/pipelines/solo/versions").await;
    assert_eq!(history["versions"][0]["number"], 3);

    // Validated for no Projet: a test pipeline.
    let validated = pdo(&daemon, &["pipeline", "validate", "solo", "--none"]).await;
    assert_eq!(validated.ok(), "solo validated for none\n");
    let list = pdo(&daemon, &["pipeline", "list"]).await;
    assert!(
        list.ok().contains("solo\tsolo\tv3\t3 nodes\tnone\t\n"),
        "{}",
        list.stdout
    );

    // Unknown: 4. Refused: 1.
    for args in [
        &["pipeline", "show", "nope"][..],
        &["pipeline", "version", "solo", "9"],
        &["pipeline", "restore", "solo", "9"],
        &["pipeline", "delete", "nope"],
    ] {
        let unknown = pdo(&daemon, args).await;
        assert_eq!(unknown.code, Some(4), "{args:?}: {}", unknown.stderr);
    }
    let taken = pdo(&daemon, &["pipeline", "create", "solo"]).await;
    assert_eq!(taken.code, Some(1), "{}", taken.stderr);
    assert!(
        taken.stderr.contains("pipeline already exists"),
        "{}",
        taken.stderr
    );
    let broken = write(files.path(), "broken.yaml", "nodes: [\n");
    let refused = pdo(&daemon, &["pipeline", "save", "solo", &broken]).await;
    assert_eq!(refused.code, Some(1), "{}", refused.stderr);
}

#[tokio::test]
async fn an_exported_pipeline_imports_back_without_loss() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let files = tempfile::tempdir().unwrap();

    // A node carrying a skill of the bank, so the document has a sidecar.
    let skilled = solo_pipeline_yaml("solo", false).replace(
        "    isolated_worktree: false\n",
        "    isolated_worktree: false\n    skills:\n      - id: pdo-interactive\n        name: pdo-interactive\n",
    );
    let yaml = write(files.path(), "solo.yaml", &skilled);
    pdo(&daemon, &["pipeline", "save", "solo", &yaml])
        .await
        .ok();

    let document = files.path().join("solo.pdo.yaml").display().to_string();
    let zip = files.path().join("solo.skills.zip").display().to_string();
    let exported = pdo(
        &daemon,
        &[
            "pipeline", "export", "solo", "--output", &document, "--skills", &zip,
        ],
    )
    .await;
    exported.ok();
    let before = std::fs::read_to_string(&document).unwrap();
    assert!(before.contains("Do the work."), "prompts inlined: {before}");
    assert!(
        std::fs::read(&zip).unwrap().starts_with(b"PK"),
        "the skills sidecar is a zip"
    );
    let to_stdout = pdo(&daemon, &["pipeline", "export", "solo"]).await;
    assert_eq!(to_stdout.ok(), before);

    let deleted = pdo(&daemon, &["pipeline", "delete", "solo"]).await;
    assert_eq!(deleted.ok(), "solo deleted\n");
    let imported = pdo(
        &daemon,
        &["pipeline", "import", &document, "--skills", &zip, "--json"],
    )
    .await
    .json();
    assert_eq!(imported["id"], "solo");
    assert_eq!(imported["warnings"], serde_json::json!([]), "{imported}");
    let after = pdo(&daemon, &["pipeline", "export", "solo"]).await;
    assert_eq!(after.ok(), before, "the round trip loses nothing");

    // A copy under a free name, renamed, then deleted.
    let copy = pdo(&daemon, &["pipeline", "duplicate", "solo", "--json"])
        .await
        .json();
    let copy_id = copy["id"].as_str().unwrap().to_string();
    assert_ne!(copy_id, "solo");
    let renamed = pdo(&daemon, &["pipeline", "rename", &copy_id, "other"]).await;
    assert_eq!(renamed.ok(), "other\tother renamed\n");
    let created = pdo(&daemon, &["pipeline", "create", "fresh"]).await;
    assert_eq!(created.ok(), "fresh created\n");
    let list = pdo(&daemon, &["pipeline", "list"]).await;
    for id in ["solo\t", "other\t", "fresh\t"] {
        assert!(list.ok().contains(id), "{id}: {}", list.stdout);
    }
}

#[tokio::test]
async fn the_node_library_is_kept_by_the_cli() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let files = tempfile::tempdir().unwrap();

    // The node library lives under the real `$HOME`: the entry is named
    // uniquely and deleted by the test itself.
    let name = format!("cli-pipeline-{}", uuid::Uuid::new_v4().simple());
    let entry = write(
        files.path(),
        "entry.json",
        &serde_json::json!({
            "name": name, "type": "agent", "inputs": [{ "name": "task" }],
            "outputs": [{ "name": "out" }], "interactive": false,
            "model": "opus", "prompt": "Review the work.",
        })
        .to_string(),
    );
    let saved = pdo(&daemon, &["pipeline", "library", "save", &entry]).await;
    assert_eq!(saved.ok(), format!("{name} saved\n"));
    let list = pdo(&daemon, &["pipeline", "library", "list"]).await;
    assert!(
        list.ok().contains(&format!("{name}\tagent\topus\t-\n")),
        "{}",
        list.stdout
    );

    let node = pdo(&daemon, &["pipeline", "library", "instantiate", &name]).await;
    let node = node.ok().to_string();
    assert!(node.contains(&format!("name: {name}\n")), "{node}");
    assert!(node.contains("prompt: Review the work.\n"), "{node}");

    // The node it makes, pasted back.
    let pasted = write(files.path(), "node.yaml", &node);
    let parsed = pdo(&daemon, &["pipeline", "parse-node", &pasted, "--json"])
        .await
        .json();
    assert_eq!(parsed["spec"]["name"], name.as_str());
    assert_eq!(parsed["spec"]["type"], "agent");
    let whole = write(files.path(), "whole.yaml", "name: x\nnodes: []\n");
    let refused = pdo(&daemon, &["pipeline", "parse-node", &whole]).await;
    assert_eq!(refused.code, Some(1), "{}", refused.stderr);
    assert!(
        refused.stderr.contains("whole pipeline"),
        "{}",
        refused.stderr
    );

    let deleted = pdo(&daemon, &["pipeline", "library", "delete", &name]).await;
    assert_eq!(deleted.ok(), format!("{name} deleted\n"));
    let gone = pdo(&daemon, &["pipeline", "library", "delete", &name]).await;
    assert_eq!(gone.code, Some(4), "{}", gone.stderr);
    let gone = pdo(&daemon, &["pipeline", "library", "instantiate", &name]).await;
    assert_eq!(gone.code, Some(4), "{}", gone.stderr);
}
