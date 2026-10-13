//! PDO-17 — a port has no side. A legacy pipeline whose ports still declare one
//! renders unchanged: the loader and `pdo migrate` share one conversion that
//! moves an output's side onto its unanchored edges (`source_anchor` without
//! offset, the side's middle) and an End `result`'s side onto the `target_side`
//! of the edges landing there, then drops every port `side`.

use crate::common::TestDaemon;

const LEGACY: &str = r#"name: legacy-sides
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - { name: user_prompt, side: right }
  - id: aBcD1234
    name: worker
    type: agent
    isolated_worktree: false
    outputs:
      - { name: plan, side: bottom }
    view: { x: 200, y: 100 }
  - id: end
    name: End
    type: end
    inputs:
      - { name: result, side: top }
edges:
  - source: { node: start, port: user_prompt }
    target: { node: aBcD1234, port: in }
  - source: { node: aBcD1234, port: plan }
    target: { node: end, port: result }
  - source: { node: aBcD1234, port: plan }
    target: { node: end, port: result }
    source_anchor: { side: left, offset: 20 }
    target_anchor: { side: right, offset: 10 }
"#;

fn assert_no_port_side(nodes: &[serde_json::Value]) {
    for node in nodes {
        for key in ["inputs", "outputs"] {
            for port in node[key].as_array().into_iter().flatten() {
                assert!(port.get("side").is_none(), "port still has a side: {port}");
            }
        }
    }
}

#[tokio::test]
async fn loading_a_legacy_pipeline_converts_port_sides_into_edge_layout() {
    let daemon = TestDaemon::spawn(|repo| {
        let dir = repo.join(".pdo").join("pipelines");
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join("legacy-sides.yaml"), LEGACY)?;
        Ok(())
    })
    .await
    .unwrap();

    let resp = reqwest::get(format!("{}/pipelines/legacy-sides", daemon.url()))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let detail: serde_json::Value = resp.json().await.unwrap();
    let pipeline = &detail["pipeline"];

    assert_no_port_side(pipeline["nodes"].as_array().unwrap());

    let edges = pipeline["edges"].as_array().unwrap();
    // `side: right` is the default departure: nothing written.
    assert!(edges[0].get("source_anchor").is_none());
    assert!(edges[0].get("target_side").is_none());
    // The unanchored edge leaves by the bottom middle and lands on the End top.
    assert_eq!(
        edges[1]["source_anchor"],
        serde_json::json!({ "side": "bottom" }),
        "an anchor without offset is the side's middle"
    );
    assert_eq!(edges[1]["target_side"], "top");
    assert!(edges[1].get("target_anchor").is_none());
    // An edge that is already anchored keeps its anchors.
    assert_eq!(
        edges[2]["source_anchor"],
        serde_json::json!({ "side": "left", "offset": 20.0 })
    );
    assert_eq!(
        edges[2]["target_anchor"],
        serde_json::json!({ "side": "right", "offset": 10.0 })
    );
    assert!(edges[2].get("target_side").is_none());
}

#[test]
fn pdo_migrate_converts_port_sides_once_and_keeps_a_backup() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("legacy-sides.yaml");
    std::fs::write(&path, LEGACY).unwrap();

    pdo_daemon::run_migrate(Some(tmp.path().to_path_buf()), false).unwrap();

    let backup = tmp.path().join("legacy-sides.yaml.bak");
    assert_eq!(std::fs::read_to_string(&backup).unwrap(), LEGACY);

    let migrated = std::fs::read_to_string(&path).unwrap();
    let doc: serde_json::Value =
        serde_json::to_value(serde_yaml::from_str::<serde_yaml::Value>(&migrated).unwrap())
            .unwrap();
    assert_no_port_side(doc["nodes"].as_array().unwrap());
    let edges = doc["edges"].as_array().unwrap();
    assert_eq!(
        edges[1]["source_anchor"],
        serde_json::json!({ "side": "bottom" })
    );
    assert_eq!(edges[1]["target_side"], "top");
    assert_eq!(edges[2]["source_anchor"]["offset"], 20);

    // A second migration changes nothing and writes no new backup.
    std::fs::remove_file(&backup).unwrap();
    pdo_daemon::run_migrate(Some(tmp.path().to_path_buf()), false).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), migrated);
    assert!(
        !backup.exists(),
        "an up-to-date pipeline is not migrated again"
    );
}
