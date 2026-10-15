//! PDO-17 — an output has no accumulation flag. A legacy pipeline whose ports
//! still declare `repeated` loads without it: the loader and `pdo migrate` share
//! the conversion that drops every port `repeated`. Accumulation stays on the
//! edge (`repeated: true` on the edge is untouched).

use crate::common::TestDaemon;

const LEGACY: &str = r#"name: legacy-repeated
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - { name: user_prompt }
  - id: aBcD1234
    name: reviewer
    type: agent
    isolated_worktree: false
    outputs:
      - { name: review, repeated: true }
  - id: eFgH5678
    name: implementer
    type: agent
    isolated_worktree: false
    outputs:
      - { name: code }
  - id: end
    name: End
    type: end
    inputs:
      - { name: result }
edges:
  - source: { node: start, port: user_prompt }
    target: { node: aBcD1234, port: in }
  - source: { node: aBcD1234, port: review }
    target: { node: eFgH5678, port: reviews }
    repeated: true
  - source: { node: eFgH5678, port: code }
    target: { node: end, port: result }
"#;

fn assert_no_port_repeated(nodes: &[serde_json::Value]) {
    for node in nodes {
        for key in ["inputs", "outputs"] {
            for port in node[key].as_array().into_iter().flatten() {
                assert!(
                    port.get("repeated").is_none(),
                    "port still has a repeated flag: {port}"
                );
            }
        }
    }
}

#[tokio::test]
async fn loading_a_legacy_pipeline_drops_the_port_repeated_and_keeps_the_edge_one() {
    let daemon = TestDaemon::spawn(|repo| {
        let dir = repo.join(".pdo").join("pipelines");
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join("legacy-repeated.yaml"), LEGACY)?;
        Ok(())
    })
    .await
    .unwrap();

    let resp = reqwest::get(format!("{}/pipelines/legacy-repeated", daemon.url()))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let detail: serde_json::Value = resp.json().await.unwrap();
    let pipeline = &detail["pipeline"];

    assert_no_port_repeated(pipeline["nodes"].as_array().unwrap());
    let edges = pipeline["edges"].as_array().unwrap();
    assert_eq!(
        edges[1]["repeated"], true,
        "the edge keeps its accumulation"
    );
    assert_ne!(edges[0]["repeated"], true);
}

#[test]
fn pdo_migrate_drops_the_port_repeated_once_and_keeps_a_backup() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("legacy-repeated.yaml");
    std::fs::write(&path, LEGACY).unwrap();

    pdo_daemon::run_migrate(Some(tmp.path().to_path_buf()), false).unwrap();

    let backup = tmp.path().join("legacy-repeated.yaml.bak");
    assert_eq!(std::fs::read_to_string(&backup).unwrap(), LEGACY);

    let migrated = std::fs::read_to_string(&path).unwrap();
    let doc: serde_json::Value =
        serde_json::to_value(serde_yaml::from_str::<serde_yaml::Value>(&migrated).unwrap())
            .unwrap();
    assert_no_port_repeated(doc["nodes"].as_array().unwrap());
    assert_eq!(doc["edges"][1]["repeated"], true);

    // A second migration changes nothing and writes no new backup.
    std::fs::remove_file(&backup).unwrap();
    pdo_daemon::run_migrate(Some(tmp.path().to_path_buf()), false).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), migrated);
    assert!(
        !backup.exists(),
        "an up-to-date pipeline is not migrated again"
    );
}
