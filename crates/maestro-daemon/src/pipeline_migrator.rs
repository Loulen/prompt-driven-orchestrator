use std::collections::HashMap;
use std::path::Path;

use tracing::{info, warn};

use crate::pipeline;

const NANOID_ALPHABET: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
const NANOID_LEN: usize = 8;

fn deterministic_id(old_id: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in old_id.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    let mut out = String::with_capacity(NANOID_LEN);
    for i in 0..NANOID_LEN {
        let idx = ((hash >> (i * 8)) & 0xFF) as usize % NANOID_ALPHABET.len();
        out.push(NANOID_ALPHABET[idx] as char);
    }
    out
}

fn looks_like_nanoid(id: &str) -> bool {
    id.len() == NANOID_LEN && id.bytes().all(|b| NANOID_ALPHABET.contains(&b))
}

fn port_missing_side(ports: &[serde_yaml::Value]) -> bool {
    ports.iter().any(|p| {
        p.as_mapping()
            .is_some_and(|m| !m.contains_key(serde_yaml::Value::String("side".into())))
    })
}

fn nodes_contain_type(nodes: &[serde_yaml::Value], node_type: &str) -> bool {
    nodes.iter().any(|n| {
        n.get("type")
            .and_then(|v| v.as_str())
            .is_some_and(|t| t == node_type)
    })
}

fn has_start_end_nodes(yaml_value: &serde_yaml::Value) -> bool {
    let nodes = match yaml_value.get("nodes").and_then(|n| n.as_sequence()) {
        Some(seq) => seq,
        None => return false,
    };
    nodes_contain_type(nodes, "start") && nodes_contain_type(nodes, "end")
}

fn has_when_on_edges(yaml_value: &serde_yaml::Value) -> bool {
    let edges = match yaml_value.get("edges").and_then(|e| e.as_sequence()) {
        Some(seq) => seq,
        None => return false,
    };
    edges.iter().any(|e| {
        e.as_mapping()
            .is_some_and(|m| m.contains_key(serde_yaml::Value::String("when".into())))
    })
}

fn has_halt_edges(yaml_value: &serde_yaml::Value) -> bool {
    let edges = match yaml_value.get("edges").and_then(|e| e.as_sequence()) {
        Some(seq) => seq,
        None => return false,
    };
    edges.iter().any(|e| {
        e.get("target")
            .and_then(|t| t.as_mapping())
            .is_some_and(|m| m.contains_key(serde_yaml::Value::String("halt".into())))
    })
}

fn needs_migration(yaml_value: &serde_yaml::Value) -> bool {
    if !has_start_end_nodes(yaml_value) {
        return true;
    }
    if has_halt_edges(yaml_value) {
        return true;
    }
    let nodes = match yaml_value.get("nodes").and_then(|n| n.as_sequence()) {
        Some(seq) => seq,
        None => return false,
    };
    for node in nodes {
        let node_type = node.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if matches!(node_type, "start" | "end" | "switch" | "loop") {
            continue;
        }
        if node.get("prompt_file").and_then(|v| v.as_str()).is_some() {
            return true;
        }
        if let Some(id) = node.get("id").and_then(|v| v.as_str()) {
            if !looks_like_nanoid(id) {
                return true;
            }
        }
        if let Some(inputs) = node.get("inputs").and_then(|v| v.as_sequence()) {
            if port_missing_side(inputs) {
                return true;
            }
        }
        if let Some(outputs) = node.get("outputs").and_then(|v| v.as_sequence()) {
            if port_missing_side(outputs) {
                return true;
            }
        }
    }
    false
}

#[derive(Debug)]
pub struct MigrateResult {
    pub migrated: bool,
    pub yaml_text: String,
    pub prompt_moves: Vec<(String, String)>,
}

pub fn migrate_pipeline_yaml(
    yaml_text: &str,
    pipeline_path: &Path,
) -> Result<MigrateResult, String> {
    let mut doc: serde_yaml::Value =
        serde_yaml::from_str(yaml_text).map_err(|e| format!("YAML parse error: {e}"))?;

    if has_when_on_edges(&doc) {
        return Err("edge-level `when:` clauses are no longer supported; \
             use a Switch node instead (see issue #45)"
            .into());
    }

    if !needs_migration(&doc) {
        return Ok(MigrateResult {
            migrated: false,
            yaml_text: yaml_text.to_string(),
            prompt_moves: vec![],
        });
    }

    let pipeline_dir = pipeline_path.parent().unwrap_or(Path::new("."));

    let nodes = doc
        .get_mut("nodes")
        .and_then(|n| n.as_sequence_mut())
        .ok_or("missing 'nodes' sequence")?;

    let mut id_map: HashMap<String, String> = HashMap::new();
    let mut prompt_moves: Vec<(String, String)> = Vec::new();

    for node in nodes.iter_mut() {
        let mapping = node.as_mapping_mut().ok_or("node is not a mapping")?;

        let node_type = mapping
            .get(serde_yaml::Value::String("type".into()))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if matches!(node_type, "start" | "end" | "switch" | "loop") {
            continue;
        }

        let old_id = mapping
            .get(serde_yaml::Value::String("id".into()))
            .and_then(|v| v.as_str())
            .ok_or("node missing 'id'")?
            .to_string();

        if looks_like_nanoid(&old_id)
            && mapping
                .get(serde_yaml::Value::String("name".into()))
                .is_some()
        {
            continue;
        }

        let new_id = deterministic_id(&old_id);
        id_map.insert(old_id.clone(), new_id.clone());

        mapping.insert(
            serde_yaml::Value::String("id".into()),
            serde_yaml::Value::String(new_id.clone()),
        );

        if mapping
            .get(serde_yaml::Value::String("name".into()))
            .is_none()
        {
            mapping.insert(
                serde_yaml::Value::String("name".into()),
                serde_yaml::Value::String(old_id.clone()),
            );
        }

        if let Some(pf) = mapping
            .remove(serde_yaml::Value::String("prompt_file".into()))
            .and_then(|v| v.as_str().map(String::from))
        {
            let old_path = pipeline_dir.join(&pf);
            let new_path = pipeline::canonical_prompt_path(pipeline_path, &new_id);
            if old_path.to_string_lossy() != new_path.to_string_lossy() {
                prompt_moves.push((
                    old_path.to_string_lossy().into_owned(),
                    new_path.to_string_lossy().into_owned(),
                ));
            }
        }
    }

    let edges = doc.get_mut("edges").and_then(|e| e.as_sequence_mut());

    if let Some(edges) = edges {
        for edge in edges.iter_mut() {
            rewrite_edge_endpoint(edge, "source", &id_map);
            rewrite_halt_edge(edge);
            rewrite_edge_endpoint(edge, "target", &id_map);
        }
    }

    let nodes_for_side = doc.get_mut("nodes").and_then(|n| n.as_sequence_mut());
    if let Some(nodes_for_side) = nodes_for_side {
        for node in nodes_for_side.iter_mut() {
            backfill_port_sides(node, "inputs", "left");
            backfill_port_sides(node, "outputs", "right");
        }
    }

    inject_start_end_nodes(&mut doc);

    let yaml_text =
        serde_yaml::to_string(&doc).map_err(|e| format!("YAML serialize error: {e}"))?;

    Ok(MigrateResult {
        migrated: true,
        yaml_text,
        prompt_moves,
    })
}

fn backfill_port_sides(node: &mut serde_yaml::Value, key: &str, default_side: &str) {
    let ports = match node.get_mut(key).and_then(|v| v.as_sequence_mut()) {
        Some(seq) => seq,
        None => return,
    };
    let side_key = serde_yaml::Value::String("side".into());
    for port in ports.iter_mut() {
        if let Some(m) = port.as_mapping_mut() {
            if !m.contains_key(&side_key) {
                m.insert(
                    side_key.clone(),
                    serde_yaml::Value::String(default_side.into()),
                );
            }
        }
    }
}

fn rewrite_halt_edge(edge: &mut serde_yaml::Value) {
    let target = match edge.get("target") {
        Some(t) => t.clone(),
        None => return,
    };
    let halt = match target
        .as_mapping()
        .and_then(|m| m.get(serde_yaml::Value::String("halt".into())))
    {
        Some(h) => h.clone(),
        None => return,
    };

    let reason = halt
        .as_mapping()
        .and_then(|m| m.get(serde_yaml::Value::String("message".into())))
        .and_then(|v| v.as_str())
        .map(String::from);

    let mut new_target = serde_yaml::Mapping::new();
    new_target.insert(
        serde_yaml::Value::String("node".into()),
        serde_yaml::Value::String("end".into()),
    );
    new_target.insert(
        serde_yaml::Value::String("port".into()),
        serde_yaml::Value::String("result".into()),
    );

    if let Some(e) = edge.as_mapping_mut() {
        e.insert(
            serde_yaml::Value::String("target".into()),
            serde_yaml::Value::Mapping(new_target),
        );
        if let Some(reason) = reason {
            e.insert(
                serde_yaml::Value::String("reason".into()),
                serde_yaml::Value::String(reason),
            );
        }
    }
}

fn inject_start_end_nodes(doc: &mut serde_yaml::Value) {
    if has_start_end_nodes(doc) {
        return;
    }

    let nodes = match doc.get_mut("nodes").and_then(|n| n.as_sequence_mut()) {
        Some(seq) => seq,
        None => return,
    };

    let has_start = nodes_contain_type(nodes, "start");
    let has_end = nodes_contain_type(nodes, "end");

    if !has_start {
        let mut start = serde_yaml::Mapping::new();
        start.insert(
            serde_yaml::Value::String("id".into()),
            serde_yaml::Value::String("start".into()),
        );
        start.insert(
            serde_yaml::Value::String("name".into()),
            serde_yaml::Value::String("Start".into()),
        );
        start.insert(
            serde_yaml::Value::String("type".into()),
            serde_yaml::Value::String("start".into()),
        );
        let mut output_port = serde_yaml::Mapping::new();
        output_port.insert(
            serde_yaml::Value::String("name".into()),
            serde_yaml::Value::String("user_prompt".into()),
        );
        start.insert(
            serde_yaml::Value::String("outputs".into()),
            serde_yaml::Value::Sequence(vec![serde_yaml::Value::Mapping(output_port)]),
        );
        nodes.insert(0, serde_yaml::Value::Mapping(start));
    }

    if !has_end {
        let mut end = serde_yaml::Mapping::new();
        end.insert(
            serde_yaml::Value::String("id".into()),
            serde_yaml::Value::String("end".into()),
        );
        end.insert(
            serde_yaml::Value::String("name".into()),
            serde_yaml::Value::String("End".into()),
        );
        end.insert(
            serde_yaml::Value::String("type".into()),
            serde_yaml::Value::String("end".into()),
        );
        let mut input_port = serde_yaml::Mapping::new();
        input_port.insert(
            serde_yaml::Value::String("name".into()),
            serde_yaml::Value::String("result".into()),
        );
        end.insert(
            serde_yaml::Value::String("inputs".into()),
            serde_yaml::Value::Sequence(vec![serde_yaml::Value::Mapping(input_port)]),
        );
        nodes.push(serde_yaml::Value::Mapping(end));
    }
}

fn rewrite_edge_endpoint(
    edge: &mut serde_yaml::Value,
    key: &str,
    id_map: &HashMap<String, String>,
) {
    if let Some(ep) = edge.get_mut(key).and_then(|v| v.as_mapping_mut()) {
        let node_key = serde_yaml::Value::String("node".into());
        if let Some(old) = ep.get(&node_key).and_then(|v| v.as_str()).map(String::from) {
            if let Some(new_id) = id_map.get(&old) {
                ep.insert(node_key, serde_yaml::Value::String(new_id.clone()));
            }
        }
    }
}

pub fn migrate_pipeline_file(pipeline_path: &Path) -> Result<bool, String> {
    let yaml_text = std::fs::read_to_string(pipeline_path)
        .map_err(|e| format!("read {}: {e}", pipeline_path.display()))?;

    let result = migrate_pipeline_yaml(&yaml_text, pipeline_path)?;
    if !result.migrated {
        return Ok(false);
    }

    for (src, dst) in &result.prompt_moves {
        let dst_path = Path::new(dst);
        if let Some(parent) = dst_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
        }
        let src_path = Path::new(src);
        if src_path.exists() {
            std::fs::rename(src_path, dst_path)
                .map_err(|e| format!("rename {} -> {}: {e}", src, dst))?;
            info!(from = %src, to = %dst, "moved prompt file");
        }
    }

    std::fs::write(pipeline_path, &result.yaml_text)
        .map_err(|e| format!("write {}: {e}", pipeline_path.display()))?;

    info!(path = %pipeline_path.display(), "migrated pipeline YAML");
    Ok(true)
}

pub fn migrate_all(pipelines_dir: &Path) -> Result<usize, String> {
    let mut count = 0;
    let entries = std::fs::read_dir(pipelines_dir)
        .map_err(|e| format!("read dir {}: {e}", pipelines_dir.display()))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("dir entry: {e}"))?;
        let path = entry.path();
        let is_yaml = path
            .extension()
            .is_some_and(|ext| ext == "yaml" || ext == "yml");
        if !is_yaml {
            continue;
        }
        match migrate_pipeline_file(&path) {
            Ok(true) => count += 1,
            Ok(false) => {}
            Err(e) => warn!(path = %path.display(), error = %e, "skipped pipeline migration"),
        }
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_id_is_stable() {
        let a = deterministic_id("implementer");
        let b = deterministic_id("implementer");
        assert_eq!(a, b);
        assert_eq!(a.len(), NANOID_LEN);
        assert!(a.bytes().all(|b| NANOID_ALPHABET.contains(&b)));
    }

    #[test]
    fn deterministic_id_differs_for_different_inputs() {
        let a = deterministic_id("implementer");
        let b = deterministic_id("reviewer");
        assert_ne!(a, b);
    }

    #[test]
    fn looks_like_nanoid_accepts_valid() {
        assert!(looks_like_nanoid("aBcD1234"));
        assert!(looks_like_nanoid("00000000"));
    }

    #[test]
    fn looks_like_nanoid_rejects_invalid() {
        assert!(!looks_like_nanoid("implementer"));
        assert!(!looks_like_nanoid("ab-cd_12"));
        assert!(!looks_like_nanoid("short"));
        assert!(!looks_like_nanoid(""));
    }

    #[test]
    fn idempotent_on_already_migrated() {
        let yaml = r#"
name: test
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: aBcD1234
    name: implementer
    type: code-mutating
    inputs:
      - name: review
        side: left
    outputs:
      - name: code
        side: right
    view: { x: 100, y: 160 }
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges: []
"#;
        let result = migrate_pipeline_yaml(yaml, Path::new("/tmp/test.yaml")).unwrap();
        assert!(!result.migrated);
    }

    #[test]
    fn migrates_old_format_nodes() {
        let yaml = r#"
name: review-loop
version: "1.0"
nodes:
  - id: implementer
    type: code-mutating
    prompt_file: .maestro/prompts/implementer.md
    inputs:
      - name: review
    outputs:
      - name: code
    view: { x: 100, y: 160 }
  - id: reviewer
    type: doc-only
    prompt_file: .maestro/prompts/reviewer.md
    inputs:
      - name: code
    outputs:
      - name: review
    view: { x: 500, y: 160 }
edges:
  - source: { node: implementer, port: code }
    target: { node: reviewer, port: code }
  - source: { node: reviewer, port: review }
    target: { node: implementer, port: review }
"#;
        let result = migrate_pipeline_yaml(yaml, Path::new("/pipelines/review-loop.yaml")).unwrap();
        assert!(result.migrated);

        let new_impl_id = deterministic_id("implementer");
        let new_rev_id = deterministic_id("reviewer");

        let parsed: serde_yaml::Value = serde_yaml::from_str(&result.yaml_text).unwrap();

        let nodes = parsed["nodes"].as_sequence().unwrap();
        assert_eq!(nodes[0]["type"].as_str().unwrap(), "start");
        assert_eq!(nodes[1]["id"].as_str().unwrap(), new_impl_id);
        assert_eq!(nodes[1]["name"].as_str().unwrap(), "implementer");
        assert!(nodes[1].get("prompt_file").is_none());
        assert_eq!(nodes[2]["id"].as_str().unwrap(), new_rev_id);
        assert_eq!(nodes[2]["name"].as_str().unwrap(), "reviewer");
        assert_eq!(nodes[3]["type"].as_str().unwrap(), "end");

        let edges = parsed["edges"].as_sequence().unwrap();
        assert_eq!(edges[0]["source"]["node"].as_str().unwrap(), new_impl_id);
        assert_eq!(edges[0]["target"]["node"].as_str().unwrap(), new_rev_id);
        assert_eq!(edges[1]["source"]["node"].as_str().unwrap(), new_rev_id);
        assert_eq!(edges[1]["target"]["node"].as_str().unwrap(), new_impl_id);

        assert_eq!(result.prompt_moves.len(), 2);
    }

    #[test]
    fn migrates_edges_with_halt_target() {
        let yaml = r#"
name: test
version: "1.0"
nodes:
  - id: worker
    type: doc-only
    inputs: []
    outputs:
      - name: out
edges:
  - source: { node: worker, port: out }
    target:
      halt: { message: "done" }
"#;
        let result = migrate_pipeline_yaml(yaml, Path::new("/pipelines/test.yaml")).unwrap();
        assert!(result.migrated);

        let parsed: serde_yaml::Value = serde_yaml::from_str(&result.yaml_text).unwrap();

        let new_id = deterministic_id("worker");
        let edges = parsed["edges"].as_sequence().unwrap();
        assert_eq!(edges[0]["source"]["node"].as_str().unwrap(), new_id);
        assert_eq!(edges[0]["target"]["node"].as_str().unwrap(), "end");
        assert_eq!(edges[0]["target"]["port"].as_str().unwrap(), "result");
        assert_eq!(edges[0]["reason"].as_str().unwrap(), "done");

        let nodes = parsed["nodes"].as_sequence().unwrap();
        assert!(
            nodes.iter().any(|n| n["type"].as_str() == Some("start")),
            "start node should be injected"
        );
        assert!(
            nodes.iter().any(|n| n["type"].as_str() == Some("end")),
            "end node should be injected"
        );
    }

    #[test]
    fn prompt_moves_use_canonical_path() {
        let yaml = r#"
name: demo
version: "1.0"
nodes:
  - id: agent
    type: doc-only
    prompt_file: old/path/agent.md
    inputs: []
    outputs: []
edges: []
"#;
        let result = migrate_pipeline_yaml(yaml, Path::new("/pipelines/demo.yaml")).unwrap();
        assert!(result.migrated);
        assert_eq!(result.prompt_moves.len(), 1);

        let new_id = deterministic_id("agent");
        let expected_dst = format!("/pipelines/demo.prompts/{new_id}.md");
        assert_eq!(result.prompt_moves[0].0, "/pipelines/old/path/agent.md");
        assert_eq!(result.prompt_moves[0].1, expected_dst);
    }

    #[test]
    fn backfills_port_side_defaults() {
        let yaml = r#"
name: test
version: "1.0"
nodes:
  - id: aBcD1234
    name: worker
    type: doc-only
    inputs:
      - name: task
      - name: context
    outputs:
      - name: plan
      - name: summary
edges: []
"#;
        let result = migrate_pipeline_yaml(yaml, Path::new("/tmp/test.yaml")).unwrap();
        assert!(result.migrated);

        let parsed: serde_yaml::Value = serde_yaml::from_str(&result.yaml_text).unwrap();
        let nodes = parsed["nodes"].as_sequence().unwrap();
        let worker = nodes
            .iter()
            .find(|n| n["name"].as_str() == Some("worker"))
            .unwrap();
        let inputs = worker["inputs"].as_sequence().unwrap();
        let outputs = worker["outputs"].as_sequence().unwrap();

        assert_eq!(inputs[0]["side"].as_str().unwrap(), "left");
        assert_eq!(inputs[1]["side"].as_str().unwrap(), "left");
        assert_eq!(outputs[0]["side"].as_str().unwrap(), "right");
        assert_eq!(outputs[1]["side"].as_str().unwrap(), "right");
    }

    #[test]
    fn preserves_existing_port_side() {
        let yaml = r#"
name: test
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
  - id: aBcD1234
    name: worker
    type: doc-only
    inputs:
      - name: task
        side: bottom
    outputs:
      - name: plan
        side: top
edges: []
"#;
        let result = migrate_pipeline_yaml(yaml, Path::new("/tmp/test.yaml")).unwrap();
        assert!(!result.migrated);
    }

    #[test]
    fn migrate_file_on_disk() {
        let tmp = tempfile::tempdir().unwrap();
        let yaml_path = tmp.path().join("test.yaml");
        let old_prompt_dir = tmp.path().join("old_prompts");
        std::fs::create_dir_all(&old_prompt_dir).unwrap();
        std::fs::write(old_prompt_dir.join("mynode.md"), "hello prompt").unwrap();

        let yaml = "name: test\nversion: '1.0'\nnodes:\n  - id: mynode\n    type: doc-only\n    prompt_file: old_prompts/mynode.md\n    inputs: []\n    outputs: []\nedges: []\n".to_string();
        std::fs::write(&yaml_path, &yaml).unwrap();

        let migrated = migrate_pipeline_file(&yaml_path).unwrap();
        assert!(migrated);

        let new_yaml = std::fs::read_to_string(&yaml_path).unwrap();
        assert!(!new_yaml.contains("prompt_file"));
        assert!(new_yaml.contains("mynode")); // as name

        let new_id = deterministic_id("mynode");
        let canonical = tmp.path().join(format!("test.prompts/{new_id}.md"));
        assert!(canonical.exists());
        assert_eq!(std::fs::read_to_string(canonical).unwrap(), "hello prompt");

        // idempotent: second run does nothing
        let not_migrated = migrate_pipeline_file(&yaml_path).unwrap();
        assert!(!not_migrated);
    }

    #[test]
    fn migrate_all_skips_non_yaml() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("readme.md"), "# hi").unwrap();
        std::fs::write(
            tmp.path().join("pipe.yaml"),
            "name: p\nversion: '1.0'\nnodes:\n  - id: n1\n    type: doc-only\n    inputs: []\n    outputs: []\nedges: []\n",
        )
        .unwrap();

        let count = migrate_all(tmp.path()).unwrap();
        assert_eq!(count, 1);

        // second run: already migrated
        let count2 = migrate_all(tmp.path()).unwrap();
        assert_eq!(count2, 0);
    }

    #[test]
    fn switch_loop_nodes_pass_through_unchanged() {
        let yaml = r#"
name: test
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: aBcD1234
    name: my-loop
    type: loop
    max_iter: 3
    inputs:
      - name: in
        side: left
      - name: break
        side: left
    outputs:
      - name: body
        side: right
      - name: done
        side: right
  - id: xYzW5678
    name: my-switch
    type: switch
    inputs:
      - name: in
        side: left
    outputs:
      - name: pass
        side: right
        when:
          verdict: { in: [PASS] }
      - name: default
        side: right
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges: []
"#;
        let result = migrate_pipeline_yaml(yaml, Path::new("/tmp/test.yaml")).unwrap();
        assert!(!result.migrated);
    }

    #[test]
    fn rejects_when_on_edges() {
        let yaml = r#"
name: test
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: aBcD1234
    name: worker
    type: doc-only
    inputs:
      - name: task
        side: left
    outputs:
      - name: result
        side: right
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: aBcD1234, port: result }
    target: { node: end, port: result }
    when:
      iter: { lt: 3 }
"#;
        let result = migrate_pipeline_yaml(yaml, Path::new("/tmp/test.yaml"));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("when:"), "error should mention when:");
        assert!(
            err.contains("Switch node"),
            "error should mention Switch node"
        );
        assert!(
            err.contains("issue #45"),
            "error should reference issue #45"
        );
    }
}
