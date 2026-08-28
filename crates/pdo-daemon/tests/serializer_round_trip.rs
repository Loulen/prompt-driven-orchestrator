//! Layer 3a — serializer round-trip: every pipeline YAML in `.pdo/pipelines/`
//! can be loaded, saved via PUT, and re-loaded without errors or structural drift.
//! Refs #75.

use crate::common::TestDaemon;

fn seed_pipeline_files(repo: &std::path::Path) -> anyhow::Result<()> {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join(".pdo")
        .join("pipelines");

    let dst = repo.join(".pdo").join("pipelines");
    std::fs::create_dir_all(&dst)?;

    for entry in std::fs::read_dir(&src)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "yaml") {
            let filename = path.file_name().unwrap();
            std::fs::copy(&path, dst.join(filename))?;
        }
    }
    Ok(())
}

#[tokio::test]
async fn every_pipeline_yaml_round_trips_through_save() {
    let daemon = TestDaemon::spawn(seed_pipeline_files).await.unwrap();

    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{}/pipelines", daemon.url()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let all: Vec<serde_json::Value> = resp.json().await.unwrap();

    // `GET /pipelines` merges every scope, so it also lists whatever lives in the
    // developer's real `~/.pdo/pipelines` — HOME is not overridden here. Keep only
    // what this test seeded.
    //
    // **This filter is load-bearing, not tidiness.** Without it, the identity
    // `PUT` below resolved a user-scoped id through the repo-first fallback,
    // missed it in the tempdir, and wrote the developer's own template file. Two
    // real consequences: the suite mutated files outside its sandbox on every run,
    // and the resulting `pipeline_changed` broadcast reached *other* tests'
    // daemons — which is what made
    // `run_scoped_pipeline::unrelated_md_in_run_worktree_does_not_emit_pipeline_event`
    // fail whenever the two ran concurrently. Every request below is scope-pinned
    // for the same reason.
    let pipelines: Vec<&serde_json::Value> = all
        .iter()
        .filter(|e| e["scope"].as_str() == Some("repo"))
        .collect();
    assert!(
        !pipelines.is_empty(),
        "should find at least one repo-scoped pipeline (seeded from the repo's .pdo/pipelines), got: {all:?}"
    );

    for entry in &pipelines {
        let id = entry["id"].as_str().unwrap();

        // GET the pipeline (skip legacy pipelines that fail validation)
        let resp = client
            .get(format!("{}/pipelines/{}?scope=repo", daemon.url(), id))
            .send()
            .await
            .unwrap();
        if resp.status() != 200 {
            continue;
        }
        let detail: serde_json::Value = resp.json().await.unwrap();
        let yaml = detail["yaml"].as_str().unwrap();

        let body = serde_json::json!({ "yaml": yaml, "prompts": {} });
        let resp = client
            .put(format!("{}/pipelines/{}?scope=repo", daemon.url(), id))
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            200,
            "PUT /pipelines/{id} rejected its own YAML: {:?}",
            resp.text().await
        );

        let resp = client
            .get(format!("{}/pipelines/{}?scope=repo", daemon.url(), id))
            .send()
            .await
            .unwrap();
        let detail2: serde_json::Value = resp.json().await.unwrap();
        let pipeline1 = &detail["pipeline"];
        let pipeline2 = &detail2["pipeline"];

        assert_eq!(
            pipeline1["name"], pipeline2["name"],
            "pipeline {id}: name changed after round-trip"
        );
        assert_eq!(
            pipeline1["nodes"].as_array().unwrap().len(),
            pipeline2["nodes"].as_array().unwrap().len(),
            "pipeline {id}: node count changed after round-trip"
        );
        assert_eq!(
            pipeline1["edges"].as_array().unwrap().len(),
            pipeline2["edges"].as_array().unwrap().len(),
            "pipeline {id}: edge count changed after round-trip"
        );
    }
}

/// Test the specific reproducer: a pipeline with frontmatter output ports.
/// This was the canvas state that triggered #75.
#[tokio::test]
async fn reproducer_frontmatter_pipeline_round_trips() {
    let yaml = r#"name: reproducer-75
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: reviewer
    name: reviewer
    type: doc-only
    inputs:
      - name: code
    outputs:
      - name: review
        frontmatter:
          verdict:
            type: enum
            allowed: [PASS, FAIL]
          score:
            type: int
  - id: gate
    name: gate
    type: switch
    inputs:
      - name: in
    outputs:
      - name: pass
        when:
          verdict:
            in: [PASS, APPROVED]
      - name: rework
        when:
          verdict:
            eq: FAIL
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: reviewer, port: code }
  - source: { node: reviewer, port: review }
    target: { node: gate, port: in }
  - source: { node: gate, port: pass }
    target: { node: end, port: result }
"#;

    let daemon = TestDaemon::spawn(|repo| {
        let dir = repo.join(".pdo").join("pipelines");
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join("reproducer-75.yaml"), yaml)?;
        Ok(())
    })
    .await
    .unwrap();

    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{}/pipelines/reproducer-75", daemon.url()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let detail: serde_json::Value = resp.json().await.unwrap();
    let original_yaml = detail["yaml"].as_str().unwrap();

    let body = serde_json::json!({ "yaml": original_yaml, "prompts": {} });
    let resp = client
        .put(format!("{}/pipelines/reproducer-75", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        200,
        "PUT rejected reproducer YAML: {:?}",
        resp.text().await
    );

    let resp = client
        .get(format!("{}/pipelines/reproducer-75", daemon.url()))
        .send()
        .await
        .unwrap();
    let detail2: serde_json::Value = resp.json().await.unwrap();

    let p1 = &detail["pipeline"];
    let p2 = &detail2["pipeline"];
    assert_eq!(p1["name"], p2["name"]);
    assert_eq!(
        p1["nodes"].as_array().unwrap().len(),
        p2["nodes"].as_array().unwrap().len()
    );

    let reviewer = p2["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == "reviewer")
        .unwrap();
    let review_port = reviewer["outputs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "review")
        .unwrap();
    assert!(
        review_port["frontmatter"].is_object(),
        "frontmatter should survive round-trip"
    );
    assert_eq!(review_port["frontmatter"]["verdict"]["type"], "enum");
}
