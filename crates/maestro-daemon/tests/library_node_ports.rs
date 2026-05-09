//! Layer 3a — POST a library node with fully-typed ports (frontmatter schemas,
//! when clauses, repeated flag, side) → GET /library → assert every field
//! round-trips through the daemon and on-disk YAML.  Regression test for #71.

mod common;

use common::TestDaemon;

#[tokio::test]
async fn library_node_preserves_all_port_fields() {
    let daemon = TestDaemon::spawn(|_repo_root| Ok(())).await.unwrap();
    let client = reqwest::Client::new();

    let payload = serde_json::json!({
        "name": "Typed Reviewer",
        "type": "doc-only",
        "inputs": [
            {
                "name": "code",
                "repeated": false,
                "side": "left"
            },
            {
                "name": "reviews",
                "repeated": true,
                "side": "top"
            }
        ],
        "outputs": [
            {
                "name": "review",
                "repeated": false,
                "side": "right",
                "frontmatter": {
                    "verdict": {
                        "type": "enum",
                        "allowed": ["PASS", "FAIL"]
                    },
                    "score": {
                        "type": "int"
                    }
                },
                "when": {
                    "verdict": { "eq": "PASS" }
                }
            }
        ],
        "interactive": true,
        "prompt": "You are a code reviewer."
    });

    // POST to library
    let resp = client
        .post(format!("{}/library", daemon.url()))
        .json(&payload)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "POST /library should return 201");

    let created: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(created["name"], "Typed Reviewer");

    // GET /library and find our entry
    let resp = client
        .get(format!("{}/library", daemon.url()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let entries: Vec<serde_json::Value> = resp.json().await.unwrap();
    let entry = entries
        .iter()
        .find(|e| e["name"] == "Typed Reviewer")
        .expect("entry should exist in GET /library");

    // Assert inputs
    let inputs = entry["inputs"].as_array().unwrap();
    assert_eq!(inputs.len(), 2);
    assert_eq!(inputs[0]["name"], "code");
    assert_eq!(inputs[0]["repeated"], false);
    assert_eq!(inputs[0]["side"], "left");
    assert_eq!(inputs[1]["name"], "reviews");
    assert_eq!(inputs[1]["repeated"], true);
    assert_eq!(inputs[1]["side"], "top");

    // Assert output with frontmatter + when
    let outputs = entry["outputs"].as_array().unwrap();
    assert_eq!(outputs.len(), 1);
    let output = &outputs[0];
    assert_eq!(output["name"], "review");
    assert_eq!(output["side"], "right");

    let frontmatter = &output["frontmatter"];
    assert!(!frontmatter.is_null(), "frontmatter should be present");
    assert_eq!(frontmatter["verdict"]["type"], "enum");
    let allowed = frontmatter["verdict"]["allowed"].as_array().unwrap();
    assert_eq!(allowed.len(), 2);
    assert!(allowed.contains(&serde_json::json!("PASS")));
    assert!(allowed.contains(&serde_json::json!("FAIL")));
    assert_eq!(frontmatter["score"]["type"], "int");

    let when = &output["when"];
    assert!(!when.is_null(), "when clause should be present");
    assert_eq!(when["verdict"]["eq"], "PASS");

    // Assert metadata
    assert_eq!(entry["interactive"], true);
    assert_eq!(entry["prompt"], "You are a code reviewer.");

    // POST /library/{name}/instantiate and verify round-trip
    let resp = client
        .post(format!(
            "{}/library/{}/instantiate",
            daemon.url(),
            "Typed Reviewer"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let instantiated: serde_json::Value = resp.json().await.unwrap();

    let spec = &instantiated["spec"];
    let inst_outputs = spec["outputs"].as_array().unwrap();
    let inst_output = &inst_outputs[0];
    assert!(
        !inst_output["frontmatter"].is_null(),
        "instantiated output should have frontmatter"
    );
    assert_eq!(inst_output["frontmatter"]["verdict"]["type"], "enum");
    assert!(
        !inst_output["when"].is_null(),
        "instantiated output should have when"
    );
    assert_eq!(inst_output["when"]["verdict"]["eq"], "PASS");
    assert_eq!(instantiated["prompt"], "You are a code reviewer.");
}
