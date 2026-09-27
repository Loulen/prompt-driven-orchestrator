//! The migrator (ADR-0011, ADR-0046, ADR-0060): rewrites a pipeline document
//! from a retired shape into the current one, as YAML, before any reader
//! parses it. Each retired shape has its own module; this one decides whether
//! a document needs migrating and runs the passes in order.

mod detect;
mod foreach;
mod lint;
mod loops;
mod structure;
mod switches;

use std::collections::HashMap;
use std::path::Path;

use crate::domain::pipeline;

pub(crate) use detect::deterministic_id;
use detect::looks_like_nanoid;
pub use detect::needs_migration;
use foreach::{dissolve_foreaches, drop_declared_inputs};
pub use lint::lint_missing_merge;
use loops::dissolve_loops;
use structure::{
    backfill_port_sides, inject_start_end_nodes, rewrite_edge_endpoint, rewrite_halt_edge,
    rewrite_loop_members,
};
use switches::dissolve_switches;

#[derive(Debug)]
pub struct MigrateResult {
    pub migrated: bool,
    pub yaml_text: String,
    pub prompt_moves: Vec<(String, String)>,
}

/// The `review-loop` seed pipeline **as it shipped pre-ADR-0011**: a `type: loop`
/// node (bound 3) wrapping implementer → reviewer, exited through a `type: switch`.
/// Kept verbatim because the seed on disk is now migrated — asserting dissolution
/// against it would be green and blind. Read by the tests of every reader.
pub const LEGACY_REVIEW_LOOP_YAML: &str = r#"
name: review-loop
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - { name: user_prompt, side: right }
  - id: qdtXejYS
    name: loop
    type: loop
    max_iter: 3
    inputs:
      - { name: in, side: left }
      - { name: break, side: left }
    outputs:
      - { name: body, side: right }
      - { name: done, side: right }
    view: { x: 100, y: 160 }
  - id: XBG5Cxkn
    name: implementer
    type: agent
    isolated_worktree: true
    inputs:
      - { name: task, side: left }
    outputs:
      - { name: code, side: right }
    view: { x: 335, y: 249 }
  - id: Qws9KzRZ
    name: reviewer
    type: agent
    isolated_worktree: false
    inputs:
      - { name: code, side: left }
    outputs:
      - name: review
        side: right
        frontmatter:
          verdict:
            type: enum
            allowed: [PASS, APPROVED, FAIL, NEEDS_WORK]
    view: { x: 500, y: 100 }
  - id: tmkRzihO
    name: switch
    type: switch
    inputs:
      - { name: in, side: left }
    outputs:
      - name: pass
        side: right
        when:
          verdict:
                      in: [PASS, APPROVED]
      - { name: default, side: right }
    view: { x: 700, y: 100 }
  - id: end
    name: End
    type: end
    inputs:
      - { name: result, side: left }
edges:
  - source: { node: start, port: user_prompt }
    target: { node: qdtXejYS, port: in }
  - source: { node: qdtXejYS, port: body }
    target: { node: XBG5Cxkn, port: task }
  - source: { node: XBG5Cxkn, port: code }
    target: { node: Qws9KzRZ, port: code }
  - source: { node: Qws9KzRZ, port: review }
    target: { node: tmkRzihO, port: in }
  - source: { node: tmkRzihO, port: pass }
    target: { node: qdtXejYS, port: break }
  - source: { node: qdtXejYS, port: done }
    target: { node: end, port: result }
"#;

pub fn migrate_pipeline_yaml(
    yaml_text: &str,
    pipeline_path: &Path,
) -> Result<MigrateResult, String> {
    let mut doc: serde_yaml::Value =
        serde_yaml::from_str(yaml_text).map_err(|e| format!("YAML parse error: {e}"))?;

    if !needs_migration(&doc) {
        return Ok(MigrateResult {
            migrated: false,
            yaml_text: yaml_text.to_string(),
            prompt_moves: vec![],
        });
    }

    let pipeline_dir = pipeline_path.parent().unwrap_or(Path::new("."));

    // Dissolve Switch nodes into guarded edges (ADR-0011) before id-rewriting,
    // so the rewritten edges still reference the original node ids.
    dissolve_switches(&mut doc)?;

    // Dissolve Loop nodes into a `loops:` region + rewired body edges (#148),
    // also before id-rewriting so members/edges reference the original ids.
    dissolve_loops(&mut doc)?;

    // Dissolve ForEach nodes into a `loops:` collection region + rewired body
    // edges (#151), also before id-rewriting.
    dissolve_foreaches(&mut doc)?;

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
        if matches!(node_type, "start" | "end" | "switch" | "loop" | "merge") {
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

    // The dissolve passes run *before* id-rewriting, so their `loops:` regions
    // reference original node ids. Remap `members` like the edges: a stale member
    // id orphans the region, which then silently no-ops at runtime.
    rewrite_loop_members(&mut doc, &id_map);

    // Strip declared inputs from regular nodes (they are emergent), moving any
    // `repeated: true` onto the matching incoming edge so accumulation survives.
    // Before side backfill, so dropped inputs are never backfilled.
    drop_declared_inputs(&mut doc);

    let nodes_for_side = doc.get_mut("nodes").and_then(|n| n.as_sequence_mut());
    if let Some(nodes_for_side) = nodes_for_side {
        for node in nodes_for_side.iter_mut() {
            backfill_port_sides(node, "inputs", "left");
            backfill_port_sides(node, "outputs", "right");
        }
    }

    inject_start_end_nodes(&mut doc);

    // Reuse the exact fold `pipeline::normalize_node_value` runs, so a persisted
    // rewrite and an in-memory parse agree; idempotent.
    if let Some(nodes) = doc.get_mut("nodes").and_then(|n| n.as_sequence_mut()) {
        for node in nodes.iter_mut() {
            if let Some(mapping) = node.as_mapping_mut() {
                pipeline::fold_flat_model_effort_into_harnesses(mapping);
            }
        }
    }

    let yaml_text =
        serde_yaml::to_string(&doc).map_err(|e| format!("YAML serialize error: {e}"))?;

    Ok(MigrateResult {
        migrated: true,
        yaml_text,
        prompt_moves,
    })
}

#[cfg(test)]
mod tests {
    use super::detect::{NANOID_ALPHABET, NANOID_LEN};
    use super::*;
    use crate::domain::pipeline::{NodeType, PipelineDef};

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
    type: agent
    isolated_worktree: true
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
        // A canonical pipeline (nanoid ids, no declared inputs on regular nodes) is
        // already migrated: a no-op.
        let result = migrate_pipeline_yaml(yaml, Path::new("/tmp/test.yaml")).unwrap();
        assert!(!result.migrated);
    }

    #[test]
    fn migrates_flat_model_effort_into_harnesses_claude() {
        // A pipeline whose only legacy trait is a flat model/effort folds them under
        // `harnesses.claude` and drops the flat keys; a second run is a no-op.
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
    type: agent
    isolated_worktree: true
    model: opus
    effort: low
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
        assert!(
            result.migrated,
            "a flat model/effort must trigger migration"
        );
        let parsed: serde_yaml::Value = serde_yaml::from_str(&result.yaml_text).unwrap();
        let node = parsed["nodes"]
            .as_sequence()
            .unwrap()
            .iter()
            .find(|n| n["name"].as_str() == Some("implementer"))
            .unwrap();
        assert!(node.get("model").is_none(), "flat model removed");
        assert!(node.get("effort").is_none(), "flat effort removed");
        assert_eq!(node["harnesses"]["claude"]["model"].as_str(), Some("opus"));
        assert_eq!(node["harnesses"]["claude"]["effort"].as_str(), Some("low"));

        let again = migrate_pipeline_yaml(&result.yaml_text, Path::new("/tmp/test.yaml")).unwrap();
        assert!(
            !again.migrated,
            "an already-folded pipeline must not migrate again"
        );
    }

    #[test]
    fn migrates_flat_model_effort_on_a_merge_node() {
        // A `merge` node spawns an agent, so its flat model/effort migrate too: the
        // `needs_migration` clause runs before the structural-node `continue`.
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
    name: merger
    type: merge
    model: sonnet
    outputs:
      - name: merged
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
        assert!(result.migrated, "a merge node's flat model must migrate");
        let parsed: serde_yaml::Value = serde_yaml::from_str(&result.yaml_text).unwrap();
        let node = parsed["nodes"]
            .as_sequence()
            .unwrap()
            .iter()
            .find(|n| n["name"].as_str() == Some("merger"))
            .unwrap();
        assert!(node.get("model").is_none());
        assert_eq!(
            node["harnesses"]["claude"]["model"].as_str(),
            Some("sonnet")
        );
    }

    #[test]
    fn drops_declared_inputs_on_regular_nodes() {
        // #149: inputs are emergent (derived from edges). The migrator strips the
        // now-redundant declared `inputs` from agent / script nodes.
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
    name: planner
    type: agent
    isolated_worktree: false
    inputs:
      - name: task
        side: left
    outputs:
      - name: plan
        side: right
edges:
  - source: { node: start, port: user_prompt }
    target: { node: aBcD1234, port: task }
"#;
        let result = migrate_pipeline_yaml(yaml, Path::new("/tmp/test.yaml")).unwrap();
        assert!(result.migrated);

        let parsed: serde_yaml::Value = serde_yaml::from_str(&result.yaml_text).unwrap();
        let nodes = parsed["nodes"].as_sequence().unwrap();
        let planner = nodes
            .iter()
            .find(|n| n["name"].as_str() == Some("planner"))
            .unwrap();
        assert!(
            planner.get("inputs").is_none()
                || planner["inputs"]
                    .as_sequence()
                    .is_some_and(|s| s.is_empty()),
            "regular node should have no declared inputs after migration"
        );
        assert!(planner["outputs"].as_sequence().is_some());
        // The End node keeps its structural `result` input.
        let end = nodes
            .iter()
            .find(|n| n["type"].as_str() == Some("end"))
            .unwrap();
        assert!(end["inputs"].as_sequence().is_some_and(|s| !s.is_empty()));
    }

    #[test]
    fn migrates_repeated_input_flag_onto_edge() {
        // A `repeated: true` declared input becomes `repeated: true` on the matching
        // incoming edge, so accumulation survives.
        let yaml = r#"
name: cycle
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
    name: reviewer
    type: agent
    isolated_worktree: false
    outputs:
      - name: review
        side: right
  - id: eFgH5678
    name: implementer
    type: agent
    isolated_worktree: true
    inputs:
      - name: reviews
        side: left
        repeated: true
    outputs:
      - name: code
        side: right
edges:
  - source: { node: aBcD1234, port: review }
    target: { node: eFgH5678, port: reviews }
"#;
        let result = migrate_pipeline_yaml(yaml, Path::new("/tmp/test.yaml")).unwrap();
        assert!(result.migrated);

        let parsed: serde_yaml::Value = serde_yaml::from_str(&result.yaml_text).unwrap();
        let edges = parsed["edges"].as_sequence().unwrap();
        let edge = edges
            .iter()
            .find(|e| e["target"]["port"].as_str() == Some("reviews"))
            .unwrap();
        assert_eq!(
            edge["repeated"].as_bool(),
            Some(true),
            "repeated should migrate from the input port onto the edge"
        );

        let nodes = parsed["nodes"].as_sequence().unwrap();
        let implementer = nodes
            .iter()
            .find(|n| n["name"].as_str() == Some("implementer"))
            .unwrap();
        assert!(
            implementer.get("inputs").is_none()
                || implementer["inputs"]
                    .as_sequence()
                    .is_some_and(|s| s.is_empty()),
            "implementer should have no declared inputs after migration"
        );
    }

    #[test]
    fn migrates_old_format_nodes() {
        let yaml = r#"
name: review-loop
version: "1.0"
nodes:
  - id: implementer
    type: agent
    isolated_worktree: true
    prompt_file: .pdo/prompts/implementer.md
    inputs:
      - name: review
    outputs:
      - name: code
    view: { x: 100, y: 160 }
  - id: reviewer
    type: agent
    isolated_worktree: false
    prompt_file: .pdo/prompts/reviewer.md
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
    type: agent
    isolated_worktree: false
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
    type: agent
    isolated_worktree: false
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
    fn backfills_output_port_side_defaults() {
        // Output ports get a default `side: right`; declared inputs are dropped, so
        // there are no input sides to backfill.
        let yaml = r#"
name: test
version: "1.0"
nodes:
  - id: aBcD1234
    name: worker
    type: agent
    isolated_worktree: false
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
        let outputs = worker["outputs"].as_sequence().unwrap();

        assert!(
            worker.get("inputs").is_none()
                || worker["inputs"].as_sequence().is_some_and(|s| s.is_empty()),
            "declared inputs are dropped (emergent)"
        );
        assert_eq!(outputs[0]["side"].as_str().unwrap(), "right");
        assert_eq!(outputs[1]["side"].as_str().unwrap(), "right");
    }

    #[test]
    fn preserves_existing_output_port_side() {
        // An explicit output `side` is never overwritten; declared inputs are
        // dropped regardless.
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
    type: agent
    isolated_worktree: false
    outputs:
      - name: plan
        side: top
edges: []
"#;
        let result = migrate_pipeline_yaml(yaml, Path::new("/tmp/test.yaml")).unwrap();
        assert!(!result.migrated);
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
        // The Loop alone needs no migration, but the Switch dissolves into guarded
        // edges.
        assert!(result.migrated);
        let migrated: PipelineDef = serde_yaml::from_str(&result.yaml_text).unwrap();
        assert!(
            !migrated
                .nodes
                .iter()
                .any(|n| n.node_type == NodeType::Switch),
            "Switch node must be removed by the migrator"
        );
    }

    #[test]
    fn accepts_when_on_edges() {
        // ADR-0011 supersedes #45: edge-level `when:` clauses are supported.
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
    type: agent
    isolated_worktree: false
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
        assert!(result.is_ok(), "edge when: must be accepted: {result:?}");
    }

    #[test]
    fn migrates_switch_node_to_guarded_edges() {
        // A producer → Switch → {pass, default} dissolves into two edges leaving the
        // producer's port directly. Nanoid ids + sided ports so the ONLY migration
        // under test is switch dissolution.
        let yaml = r#"
name: switch-migrate
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
        side: right
  - id: aBcD1234
    name: reviewer
    type: agent
    isolated_worktree: false
    inputs:
      - name: code
        side: left
    outputs:
      - name: review
        side: right
  - id: eFgH5678
    name: implementer
    type: agent
    isolated_worktree: true
    inputs:
      - name: review
        side: left
    outputs:
      - name: code
        side: right
  - id: sW1tcH00
    name: verdict-switch
    type: switch
    inputs:
      - name: in
        side: left
    outputs:
      - name: pass
        side: right
        when:
          verdict: { in: [PASS, APPROVED] }
      - name: default
        side: right
  - id: end
    name: End
    type: end
    inputs:
      - name: result
        side: left
edges:
  - source: { node: start, port: user_prompt }
    target: { node: eFgH5678, port: review }
  - source: { node: eFgH5678, port: code }
    target: { node: aBcD1234, port: code }
  - source: { node: aBcD1234, port: review }
    target: { node: sW1tcH00, port: in }
  - source: { node: sW1tcH00, port: pass }
    target: { node: end, port: result }
  - source: { node: sW1tcH00, port: default }
    target: { node: eFgH5678, port: review }
"#;
        let result = migrate_pipeline_yaml(yaml, Path::new("/tmp/test.yaml")).unwrap();
        assert!(result.migrated);

        let migrated: PipelineDef = serde_yaml::from_str(&result.yaml_text).unwrap();

        assert!(
            !migrated
                .nodes
                .iter()
                .any(|n| n.node_type == NodeType::Switch),
            "Switch node must be removed"
        );

        // No edge references the switch node any more.
        assert!(
            !migrated
                .edges
                .iter()
                .any(|e| e.source.node == "sW1tcH00" || e.target.node == "sW1tcH00"),
            "no edge may reference the dissolved switch"
        );

        // The guarded `pass` branch becomes a when-edge from reviewer:review → end.
        let pass_edge = migrated
            .edges
            .iter()
            .find(|e| {
                e.source.node == "aBcD1234" && e.source.port() == "review" && e.target.node == "end"
            })
            .expect("guarded pass edge from reviewer to end");
        assert!(pass_edge.when.is_some(), "pass edge keeps its when: clause");
        assert!(!pass_edge.is_else);

        // The `default` branch becomes an else-edge from reviewer:review → implementer.
        let else_edge = migrated
            .edges
            .iter()
            .find(|e| {
                e.source.node == "aBcD1234"
                    && e.source.port() == "review"
                    && e.target.node == "eFgH5678"
            })
            .expect("else edge from reviewer to implementer");
        assert!(else_edge.is_else, "default branch becomes else: true");
        assert!(else_edge.when.is_none());
    }

    #[test]
    fn migrate_switch_is_idempotent() {
        let yaml = r#"
name: switch-migrate
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
        side: right
  - id: reviewer1
    name: reviewer
    type: agent
    isolated_worktree: false
    inputs:
      - name: code
        side: left
    outputs:
      - name: review
        side: right
  - id: sw1
    name: verdict-switch
    type: switch
    inputs:
      - name: in
        side: left
    outputs:
      - name: pass
        side: right
        when:
          verdict: { eq: PASS }
      - name: default
        side: right
  - id: end
    name: End
    type: end
    inputs:
      - name: result
        side: left
edges:
  - source: { node: start, port: user_prompt }
    target: { node: reviewer1, port: code }
  - source: { node: reviewer1, port: review }
    target: { node: sw1, port: in }
  - source: { node: sw1, port: pass }
    target: { node: end, port: result }
  - source: { node: sw1, port: default }
    target: { node: end, port: result }
"#;
        let first = migrate_pipeline_yaml(yaml, Path::new("/tmp/test.yaml")).unwrap();
        assert!(first.migrated);
        let second = migrate_pipeline_yaml(&first.yaml_text, Path::new("/tmp/test.yaml")).unwrap();
        assert!(
            !second.migrated,
            "a migrated switch pipeline must not migrate again"
        );
    }

    use crate::domain::pipeline::{
        EdgeDef, EdgeEndpoint, EdgeSource, NodeDef, Port, PortSide, PortType,
    };

    fn make_isolated_node(id: &str) -> NodeDef {
        NodeDef {
            skills: Vec::new(),
            isolated_worktree: Some(true),
            id: id.into(),
            name: id.into(),
            node_type: NodeType::Agent,
            inputs: vec![Port {
                name: "in".into(),
                repeated: false,
                side: Some(PortSide::Left),
                port_type: PortType::Markdown,
                frontmatter: None,
                when: None,
                description: None,
                instructions: None,
                required: false,
            }],
            outputs: vec![Port {
                name: "out".into(),
                repeated: false,
                side: Some(PortSide::Right),
                port_type: PortType::Markdown,
                frontmatter: None,
                when: None,
                description: None,
                instructions: None,
                required: false,
            }],
            interactive: false,
            view: None,
            max_iter: None,
            over: None,
            pin_harness: None,
            harnesses: Default::default(),
            agent_choice: None,
            auto_fail: None,
            orchestrator: false,
        }
    }

    fn make_merge_node(id: &str) -> NodeDef {
        NodeDef {
            skills: Vec::new(),
            isolated_worktree: None,
            id: id.into(),
            name: id.into(),
            node_type: NodeType::Merge,
            inputs: vec![Port {
                name: "branches".into(),
                repeated: true,
                side: Some(PortSide::Left),
                port_type: PortType::Markdown,
                frontmatter: None,
                when: None,
                description: None,
                instructions: None,
                required: false,
            }],
            outputs: vec![Port {
                name: "merged".into(),
                repeated: false,
                side: Some(PortSide::Right),
                port_type: PortType::Markdown,
                frontmatter: None,
                when: None,
                description: None,
                instructions: None,
                required: false,
            }],
            interactive: false,
            view: None,
            max_iter: None,
            over: None,
            pin_harness: None,
            harnesses: Default::default(),
            agent_choice: None,
            auto_fail: None,
            orchestrator: false,
        }
    }

    fn make_shared_node(id: &str) -> NodeDef {
        NodeDef {
            skills: Vec::new(),
            isolated_worktree: Some(false),
            id: id.into(),
            name: id.into(),
            node_type: NodeType::Agent,
            inputs: vec![Port {
                name: "in".into(),
                repeated: false,
                side: Some(PortSide::Left),
                port_type: PortType::Markdown,
                frontmatter: None,
                when: None,
                description: None,
                instructions: None,
                required: false,
            }],
            outputs: vec![Port {
                name: "out".into(),
                repeated: false,
                side: Some(PortSide::Right),
                port_type: PortType::Markdown,
                frontmatter: None,
                when: None,
                description: None,
                instructions: None,
                required: false,
            }],
            interactive: false,
            view: None,
            max_iter: None,
            over: None,
            pin_harness: None,
            harnesses: Default::default(),
            agent_choice: None,
            auto_fail: None,
            orchestrator: false,
        }
    }

    fn make_edge(src: &str, src_port: &str, tgt: &str, tgt_port: &str) -> EdgeDef {
        EdgeDef {
            source: EdgeSource::single(src, src_port),
            target: EdgeEndpoint {
                node: tgt.into(),
                port: tgt_port.into(),
            },
            reason: None,
            when: None,
            is_else: false,
            repeated: false,
            ..Default::default()
        }
    }

    #[test]
    fn lint_flags_fan_out_cm_without_merge() {
        let pipeline = PipelineDef {
            name: "fan-out-no-merge".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_isolated_node("impl-a"),
                make_isolated_node("impl-b"),
                make_shared_node("reviewer"),
            ],
            edges: vec![
                make_edge("impl-a", "out", "reviewer", "in"),
                make_edge("impl-b", "out", "reviewer", "in"),
            ],
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: true,
            grid_size: None,
        };
        let diags = lint_missing_merge(&pipeline);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("reviewer"));
        assert!(diags[0].message.contains("impl-a"));
        assert!(diags[0].message.contains("impl-b"));
    }

    #[test]
    fn lint_no_warning_when_merge_present() {
        let pipeline = PipelineDef {
            name: "fan-out-with-merge".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_isolated_node("impl-a"),
                make_isolated_node("impl-b"),
                make_merge_node("merger"),
                make_shared_node("downstream"),
            ],
            edges: vec![
                make_edge("impl-a", "out", "merger", "branches"),
                make_edge("impl-b", "out", "merger", "branches"),
                make_edge("merger", "merged", "downstream", "in"),
            ],
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: true,
            grid_size: None,
        };
        let diags = lint_missing_merge(&pipeline);
        assert!(
            diags.is_empty(),
            "Merge downstream should suppress lint, got: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn lint_no_warning_for_single_cm_source() {
        let pipeline = PipelineDef {
            name: "single-cm".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![make_isolated_node("impl-a"), make_shared_node("reviewer")],
            edges: vec![make_edge("impl-a", "out", "reviewer", "in")],
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: true,
            grid_size: None,
        };
        let diags = lint_missing_merge(&pipeline);
        assert!(diags.is_empty());
    }

    #[test]
    fn lint_no_warning_for_shared_worktree_fan_out() {
        let pipeline = PipelineDef {
            name: "shared-worktree-fan-out".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_shared_node("plan-a"),
                make_shared_node("plan-b"),
                make_shared_node("summary"),
            ],
            edges: vec![
                make_edge("plan-a", "out", "summary", "in"),
                make_edge("plan-b", "out", "summary", "in"),
            ],
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: true,
            grid_size: None,
        };
        let diags = lint_missing_merge(&pipeline);
        assert!(diags.is_empty());
    }

    #[test]
    fn migrates_foreach_without_over_defaults_to_items() {
        // A ForEach without an explicit `over` dissolves into a collection region
        // whose `over` defaults to `items`, the legacy ForEach default.
        let yaml = r#"
name: test
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
        side: right
  - id: aBcD1234
    name: lister
    type: agent
    isolated_worktree: false
    outputs:
      - name: plan
        side: right
  - id: feNODE01
    name: per-issue
    type: for-each
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
  - id: wRkR5678
    name: worker
    type: agent
    isolated_worktree: true
    outputs:
      - name: out
        side: right
  - id: end
    name: End
    type: end
    inputs:
      - name: result
        side: left
edges:
  - source: { node: aBcD1234, port: plan }
    target: { node: feNODE01, port: in }
  - source: { node: feNODE01, port: body }
    target: { node: wRkR5678, port: in }
  - source: { node: feNODE01, port: done }
    target: { node: end, port: result }
"#;
        let parsed = migrate_str_and_parse(yaml);
        // `parse_pipeline` hard-refuses `type: for-each`, so `migrate_str_and_parse`
        // succeeding already proves dissolution.
        assert!(
            !parsed.nodes.iter().any(|n| n.id == "feNODE01"),
            "ForEach node must be dissolved"
        );
        assert_eq!(parsed.loops.len(), 1);
        assert_eq!(parsed.loops[0].kind, pipeline::LoopKind::Collection);
        assert_eq!(parsed.loops[0].over.as_deref(), Some("items"));
    }

    #[test]
    fn migrates_foreach_node_to_collection_region() {
        // A ForEach dissolves into a `loops:` collection entry + rewired body edges:
        // lister -> FE(over: issues) -> worker, FE.done -> end must yield a
        // single-member collection region, ForEach node and ports gone.
        let yaml = r#"
name: foreach-migrate
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
        side: right
  - id: aBcD1234
    name: lister
    type: agent
    isolated_worktree: false
    outputs:
      - name: plan
        side: right
  - id: feNODE01
    name: per-issue
    type: for-each
    over: issues
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
  - id: wRkR5678
    name: worker
    type: agent
    isolated_worktree: true
    outputs:
      - name: out
        side: right
  - id: end
    name: End
    type: end
    inputs:
      - name: result
        side: left
edges:
  - source: { node: start, port: user_prompt }
    target: { node: aBcD1234, port: task }
  - source: { node: aBcD1234, port: plan }
    target: { node: feNODE01, port: in }
  - source: { node: feNODE01, port: body }
    target: { node: wRkR5678, port: in }
  - source: { node: feNODE01, port: done }
    target: { node: end, port: result }
"#;
        let parsed = migrate_str_and_parse(yaml);

        assert!(
            !parsed.nodes.iter().any(|n| n.id == "feNODE01"),
            "no ForEach node may remain after migration"
        );
        assert!(
            !parsed
                .edges
                .iter()
                .any(|e| e.source.node == "feNODE01" || e.target.node == "feNODE01"),
            "no edge may reference the dissolved ForEach node"
        );

        // Exactly one collection region, single member = the body node `worker`,
        // over = issues, no max_iter.
        assert_eq!(parsed.loops.len(), 1, "one collection region expected");
        let region = &parsed.loops[0];
        assert_eq!(region.kind, pipeline::LoopKind::Collection);
        assert_eq!(region.over.as_deref(), Some("issues"));
        assert_eq!(region.members, vec!["wRkR5678".to_string()]);
        assert_eq!(region.max_iter, None);

        // The entering edge enters the member directly: lister:plan -> worker.
        assert!(
            parsed
                .edges
                .iter()
                .any(|e| { e.source.node == "aBcD1234" && e.target.node == "wRkR5678" }),
            "entering edge should land on the member directly"
        );
        // The barrier edge leaves the member to the done target: worker -> end.
        assert!(
            parsed
                .edges
                .iter()
                .any(|e| { e.source.node == "wRkR5678" && e.target.node == "end" }),
            "barrier edge should leave the member to the done target"
        );

        // Idempotent: migrating the result again is a no-op.
        let first = migrate_pipeline_yaml(yaml, Path::new("/tmp/fixture.yaml")).unwrap();
        let second =
            migrate_pipeline_yaml(&first.yaml_text, Path::new("/tmp/fixture.yaml")).unwrap();
        assert!(!second.migrated, "foreach migration must be idempotent");
    }

    #[test]
    fn foreach_dissolution_dedups_colliding_region_ids_in_document_order() {
        // The region id defaults to the ForEach node's NAME, user-chosen free text.
        // Two ForEaches sharing a name must not emit colliding region ids (and so
        // colliding loop counters); the second gets a deterministic `-2` suffix.
        let yaml = r#"
name: foreach-dup
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
        side: right
  - id: liStErAA
    name: lister
    type: agent
    isolated_worktree: false
    outputs:
      - name: plan
        side: right
  - id: feNODE01
    name: per-item
    type: for-each
    over: issues
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
  - id: wRkRAAAA
    name: worker-a
    type: agent
    isolated_worktree: true
    outputs:
      - name: out
        side: right
  - id: feNODE02
    name: per-item
    type: for-each
    over: bugs
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
  - id: wRkRBBBB
    name: worker-b
    type: agent
    isolated_worktree: true
    outputs:
      - name: out
        side: right
  - id: end
    name: End
    type: end
    inputs:
      - name: result
        side: left
edges:
  - source: { node: start, port: user_prompt }
    target: { node: liStErAA, port: task }
  - source: { node: liStErAA, port: plan }
    target: { node: feNODE01, port: in }
  - source: { node: feNODE01, port: body }
    target: { node: wRkRAAAA, port: in }
  - source: { node: feNODE01, port: done }
    target: { node: feNODE02, port: in }
  - source: { node: feNODE02, port: body }
    target: { node: wRkRBBBB, port: in }
  - source: { node: feNODE02, port: done }
    target: { node: end, port: result }
"#;
        let parsed = migrate_str_and_parse(yaml);

        assert_eq!(parsed.loops.len(), 2, "two collection regions expected");
        let ids: Vec<&str> = parsed.loops.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["per-item", "per-item-2"],
            "colliding names dedup with a suffix, in document order"
        );
        // Each region keeps its own driver.
        assert_eq!(parsed.loops[0].over.as_deref(), Some("issues"));
        assert_eq!(parsed.loops[1].over.as_deref(), Some("bugs"));
    }

    #[test]
    fn migrate_str_and_parse_rejects_foreach() {
        let yaml = r#"
name: foreach-empty-over
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
        side: right
  - id: aBcD1234
    name: lister
    type: agent
    isolated_worktree: false
    outputs:
      - name: plan
        side: right
  - id: feNODE01
    name: per-issue
    type: for-each
    over: tasks
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
  - id: wRkR5678
    name: worker
    type: agent
    isolated_worktree: true
    outputs:
      - name: out
        side: right
  - id: end
    name: End
    type: end
    inputs:
      - name: result
        side: left
edges:
  - source: { node: aBcD1234, port: plan }
    target: { node: feNODE01, port: in }
  - source: { node: feNODE01, port: body }
    target: { node: wRkR5678, port: in }
  - source: { node: feNODE01, port: done }
    target: { node: end, port: result }
"#;
        let parsed = migrate_str_and_parse(yaml);
        let region = &parsed.loops[0];
        assert_eq!(region.over.as_deref(), Some("tasks"));
    }

    #[test]
    fn foreach_dissolution_remaps_members_when_body_has_human_id() {
        // The dissolve passes run before id-rewriting, so the emitted region
        // references the body's *original* id. When that id is re-idified to a
        // nanoid, `loops[].members` must follow like the edges, or the region is
        // orphaned and silently no-ops at runtime.
        let yaml = r#"
name: foreach-human-id
version: "1.0"
nodes:
  - id: lister
    type: agent
    isolated_worktree: false
    outputs:
      - name: plan
        side: right
  - id: feNODE01
    name: per-item
    type: for-each
    over: items
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
  - id: worker
    type: agent
    isolated_worktree: true
    outputs:
      - name: out
        side: right
edges:
  - source: { node: lister, port: plan }
    target: { node: feNODE01, port: in }
  - source: { node: feNODE01, port: body }
    target: { node: worker, port: in }
"#;
        let parsed = migrate_str_and_parse(yaml);

        assert_eq!(parsed.loops.len(), 1);
        let region = &parsed.loops[0];
        let new_worker_id = deterministic_id("worker");
        assert_eq!(
            region.members,
            vec![new_worker_id.clone()],
            "members must be remapped to the re-idified node"
        );
        // Cross-check: every member names an existing node (the launch-time
        // dangling-reference gate would otherwise refuse this pipeline).
        assert!(
            pipeline::dangling_edge_references(&parsed).is_empty(),
            "migrated pipeline must carry no dangling references"
        );
    }

    #[test]
    fn loop_dissolution_remaps_members_when_body_has_human_id() {
        // Same bug class for `bounded` regions from dissolve_loops: a re-idified
        // body node's members must follow.
        let yaml = r#"
name: loop-human-id
version: "1.0"
nodes:
  - id: feeder
    type: agent
    isolated_worktree: false
    outputs:
      - name: plan
        side: right
  - id: lpNODE01
    name: retry
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
  - id: fixer
    type: agent
    isolated_worktree: true
    outputs:
      - name: out
        side: right
edges:
  - source: { node: feeder, port: plan }
    target: { node: lpNODE01, port: in }
  - source: { node: lpNODE01, port: body }
    target: { node: fixer, port: in }
  - source: { node: fixer, port: out }
    target: { node: lpNODE01, port: break }
    when:
      verdict: { eq: PASS }
  - source: { node: lpNODE01, port: done }
    target: { node: end, port: result }
"#;
        let parsed = migrate_str_and_parse(yaml);

        assert_eq!(parsed.loops.len(), 1);
        let region = &parsed.loops[0];
        assert_eq!(region.kind, pipeline::LoopKind::Bounded);
        assert_eq!(
            region.members,
            vec![deterministic_id("fixer")],
            "bounded-region members must be remapped to the re-idified node"
        );
        assert!(
            pipeline::dangling_edge_references(&parsed).is_empty(),
            "migrated pipeline must carry no dangling references"
        );
    }

    fn migrate_str_and_parse(yaml: &str) -> PipelineDef {
        let result = migrate_pipeline_yaml(yaml, Path::new("/tmp/fixture.yaml")).unwrap();
        // After migration there must be no Switch left, and the result re-parses.
        let parsed = pipeline::parse_pipeline(&result.yaml_text)
            .expect("migrated fixture must parse")
            .pipeline;
        assert!(
            !parsed.nodes.iter().any(|n| n.node_type == NodeType::Switch),
            "no Switch node may remain after migration"
        );
        assert!(
            !parsed.edges.iter().any(|e| {
                parsed.nodes.iter().any(|n| {
                    n.node_type == NodeType::Switch
                        && (n.id == e.source.node || n.id == e.target.node)
                })
            }),
            "no edge may reference a Switch node"
        );
        parsed
    }

    #[test]
    fn migrates_loop_node_to_bounded_region() {
        // A Loop node dissolves into a `loops:` entry + rewired body edges: the
        // legacy review-loop shape must yield a bounded region over the body nodes.
        let yaml = LEGACY_REVIEW_LOOP_YAML;
        let parsed = migrate_str_and_parse(yaml);

        // No Loop node may remain, nor any edge referencing it.
        assert!(
            !parsed.nodes.iter().any(|n| n.node_type == NodeType::Loop),
            "no Loop node may remain after migration"
        );

        // Exactly one bounded region, members = the two body nodes, max_iter 3.
        assert_eq!(parsed.loops.len(), 1, "one bounded region expected");
        let region = &parsed.loops[0];
        assert_eq!(region.kind, pipeline::LoopKind::Bounded);
        let mut members = region.members.clone();
        members.sort();
        assert_eq!(
            members,
            vec!["Qws9KzRZ".to_string(), "XBG5Cxkn".to_string()]
        );
        assert_eq!(
            region.max_iter,
            Some(serde_yaml::Value::Number(serde_yaml::Number::from(3)))
        );

        // No edge may reference the dissolved loop's ports.
        assert!(
            !parsed
                .edges
                .iter()
                .any(|e| e.source.node == "qdtXejYS" || e.target.node == "qdtXejYS"),
            "no edge may reference the dissolved Loop node"
        );

        // The region matches what cycle auto-detection would find on the rewired
        // graph (the back-edge makes the loop a real cycle).
        let cycles = crate::domain::cycles::detect_cycles(&parsed);
        assert_eq!(cycles.len(), 1, "rewired graph has exactly one cycle");
        let mut cyc = cycles[0].clone();
        cyc.sort();
        assert_eq!(cyc, members);

        // Idempotent: migrating the result again is a no-op.
        let first = migrate_pipeline_yaml(yaml, Path::new("/tmp/fixture.yaml")).unwrap();
        let second =
            migrate_pipeline_yaml(&first.yaml_text, Path::new("/tmp/fixture.yaml")).unwrap();
        assert!(!second.migrated, "loop migration must be idempotent");
    }

    #[test]
    fn migrates_review_loop_fixture_switch_to_guarded_edges() {
        let yaml = LEGACY_REVIEW_LOOP_YAML;
        let parsed = migrate_str_and_parse(yaml);
        // The dissolved switch leaves at least one guarded (when:) or else edge.
        assert!(
            parsed.edges.iter().any(|e| e.when.is_some() || e.is_else),
            "review-loop should have conditional edges after migration"
        );
        // Idempotent: migrating the result again is a no-op.
        let first = migrate_pipeline_yaml(yaml, Path::new("/tmp/fixture.yaml")).unwrap();
        let second =
            migrate_pipeline_yaml(&first.yaml_text, Path::new("/tmp/fixture.yaml")).unwrap();
        assert!(!second.migrated, "migration must be idempotent");
    }

    #[test]
    fn loop_entry_guard_survives_dissolution() {
        // `U.p -> L.in [when W]`: the guard decides whether the loop is entered at
        // all, so it MUST ride along when the edge is rewired onto the body entry.
        // Dropping it made every verdict enter the fix loop, with the `else` route
        // to End dead code.
        let yaml = r#"
name: guarded-entry
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - { name: user_prompt, side: right }
  - id: aaaa0001
    name: triage
    type: agent
    isolated_worktree: false
    outputs:
      - name: out
        side: right
        frontmatter:
          Verdict: { type: enum, allowed: [Bug, Feature] }
  - id: lpNODE01
    name: retry
    type: loop
    max_iter: 4
    inputs:
      - { name: in, side: left }
      - { name: break, side: left }
    outputs:
      - { name: body, side: right }
      - { name: done, side: right }
  - id: aaaa0002
    name: fixer
    type: agent
    isolated_worktree: true
    outputs:
      - { name: out, side: right }
  - id: end
    name: End
    type: end
    inputs:
      - { name: result, side: left }
edges:
  - source: { node: start, port: user_prompt }
    target: { node: aaaa0001, port: in }
  - source: { node: aaaa0001, port: out }
    target: { node: lpNODE01, port: in }
    when:
      Verdict: { eq: Bug }
  - source: { node: aaaa0001, port: out }
    target: { node: end, port: result }
    else: true
  - source: { node: lpNODE01, port: body }
    target: { node: aaaa0002, port: in }
  - source: { node: aaaa0002, port: out }
    target: { node: lpNODE01, port: break }
    when:
      Verdict: { eq: Pass }
  - source: { node: lpNODE01, port: done }
    target: { node: end, port: result }
"#;
        let parsed = migrate_str_and_parse(yaml);
        let entry_edge = parsed
            .edges
            .iter()
            .find(|e| e.source.node == "aaaa0001" && e.target.node == "aaaa0002")
            .expect("the loop-entry edge must be rewired onto the body entry");
        let when = entry_edge
            .when
            .as_ref()
            .expect("the entry guard must survive the rewire");
        assert_eq!(
            when.get("Verdict")
                .and_then(|c| c.get("eq"))
                .and_then(|v| v.as_str()),
            Some("Bug"),
            "the rewired entry edge must keep its original when-clause: {when:?}"
        );
    }

    #[test]
    fn shipped_loop_fixtures_need_no_migration() {
        // `review-loop` and `simple-bugfix` ship migrated: the editor has no
        // representation for `type: loop` / `type: switch`, so the legacy YAML drew
        // no loop region and left `max_iter` unreachable.
        for (name, yaml, expect_members, expect_max) in [
            (
                "review-loop",
                include_str!("../../../../../.pdo/pipelines/review-loop.yaml"),
                vec!["Qws9KzRZ", "XBG5Cxkn"],
                3i64,
            ),
            (
                "simple-bugfix",
                include_str!("../../../../../.pdo/pipelines/simple-bugfix.yaml"),
                vec!["9NOnrpKY", "KHFCO0US"],
                5i64,
            ),
        ] {
            let result = migrate_pipeline_yaml(yaml, Path::new("/tmp/fixture.yaml")).unwrap();
            assert!(
                !result.migrated,
                "{name} must ship already migrated — run `pdo migrate --dir .pdo/pipelines`"
            );

            let parsed = pipeline::parse_pipeline(yaml).unwrap();
            assert!(
                !parsed
                    .pipeline
                    .nodes
                    .iter()
                    .any(|n| matches!(n.node_type, NodeType::Loop | NodeType::Switch)),
                "{name} must carry no legacy control node"
            );
            assert!(
                parsed.diagnostics.is_empty(),
                "{name} must parse diagnostic-clean: {:?}",
                parsed.diagnostics
            );

            let pipeline = parsed.pipeline;
            assert_eq!(pipeline.loops.len(), 1, "{name}: one bounded region");
            let region = &pipeline.loops[0];
            assert_eq!(region.kind, pipeline::LoopKind::Bounded);
            let mut members = region.members.clone();
            members.sort();
            assert_eq!(members, expect_members, "{name}: region members");
            // The bound comes off the file, NOT DEFAULT_MAX_ITER: a region
            // materialized over the legacy shape advertised 5 while the engine ran
            // the node's own bound.
            assert_eq!(
                crate::domain::loops::resolve_region_max_iter(region, &HashMap::new()),
                expect_max,
                "{name}: the persisted bound must be the real one"
            );
            // The region is a genuine cycle in the graph, so the scheduler's
            // re-entry path (not the generic forward spawn) governs its laps.
            let cycles = crate::domain::cycles::detect_cycles(&pipeline);
            assert_eq!(cycles.len(), 1, "{name}: exactly one cycle");
            let mut cycle = cycles[0].clone();
            cycle.sort();
            assert_eq!(cycle, members, "{name}: the cycle IS the region");
        }
    }

    #[test]
    fn migrates_planner_fixture_no_switch_unchanged_topology() {
        // planner has no Switch — migration must not introduce conditional edges.
        let yaml = include_str!("../../../../../.pdo/pipelines/planner.yaml");
        let result = migrate_pipeline_yaml(yaml, Path::new("/tmp/fixture.yaml")).unwrap();
        let parsed = pipeline::parse_pipeline(&result.yaml_text)
            .unwrap()
            .pipeline;
        assert!(
            !parsed.edges.iter().any(|e| e.when.is_some() || e.is_else),
            "planner has no switch, so no conditional edges should appear"
        );
    }
}
