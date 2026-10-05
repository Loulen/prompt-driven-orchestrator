//! What the scheduler reads off the blackboard: a Run's variables and its
//! producers' frontmatter; the switch pass-through; a collection's items.

use std::collections::HashMap;

use crate::application::node::spawn::SpawnContext;
use crate::domain::blackboard;
use crate::domain::{event, state};
use pdo_pipeline::domain::pipeline;

/// Deposits one `_item.md` per lap under the collection entry's artifact dir
/// (ADR-0011): `<artifacts>/<entry>/iter-N/_item.md` with `item / iter / total`
/// frontmatter, so each spawned lap reads its own item.
pub fn deposit_collection_items(
    artifacts_dir: &std::path::Path,
    entry_node_id: &str,
    items: &[serde_yaml::Value],
) {
    for (i, item) in items.iter().enumerate() {
        let iter_num = (i + 1) as i64;
        let item_dir = artifacts_dir
            .join(entry_node_id)
            .join(format!("iter-{iter_num}"));
        let _ = std::fs::create_dir_all(&item_dir);
        let item_str = serde_yaml::to_string(item).unwrap_or_else(|_| format!("{item:?}"));
        let content = format!(
            "---\nitem: {}\niter: {}\ntotal: {}\n---\n\n{}",
            item_str.trim(),
            iter_num,
            items.len(),
            item_str.trim()
        );
        let _ = std::fs::write(item_dir.join("_item.md"), content);
    }
}

pub fn resolve_run_variables(
    pipeline: &pipeline::PipelineDef,
    events: &[event::Event],
) -> HashMap<String, serde_yaml::Value> {
    let mut resolved_vars = pipeline.variable_defaults();
    if let Some(payload) = events.first().and_then(|e| e.payload.as_ref()) {
        if let Some(vars) = payload.get("variables") {
            if let Ok(overrides) =
                serde_json::from_value::<HashMap<String, serde_yaml::Value>>(vars.clone())
            {
                for (k, v) in overrides {
                    resolved_vars.insert(k, v);
                }
            }
        }
    }
    resolved_vars
}

pub fn passthrough_switch_artifact(
    ctx: &SpawnContext<'_>,
    switch_node_id: &str,
    chosen_branch: &str,
    source_iter: i64,
) {
    let in_edge = ctx
        .pipeline
        .edges
        .iter()
        .find(|e| e.target.node == switch_node_id && e.target.port == "in");
    let Some(in_edge) = in_edge else { return };

    let src_path = blackboard::artifact_path(
        ctx.artifacts_dir,
        &in_edge.source.node,
        source_iter,
        in_edge.source.port(),
    );
    if !src_path.exists() {
        return;
    }

    let dst_path = blackboard::artifact_path(ctx.artifacts_dir, switch_node_id, 1, chosen_branch);
    if let Some(parent) = dst_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::copy(&src_path, &dst_path);
}

/// The evaluation context a completed node's outgoing `when:` clauses read.
///
/// Every declared output port contributes its frontmatter TWICE:
///
///  - under the bare field name (`verdict`), the merged, last-port-wins view the
///    predicate grammar has always read — what an unqualified clause means;
///  - under the port-qualified name (`review.verdict`), so a multi-port edge can
///    say WHICH carried output a condition reads (ADR-0073 §3 / #843). The
///    frontend writes the qualified spelling as soon as an edge carries two
///    ports, and the bare one while it carries one — which is why a single-port
///    pipeline keeps evaluating exactly as before.
///
/// A port whose name already contains a `.` would produce an ambiguous qualified
/// key; that is harmless here (the bare key is still authoritative) and the panel
/// never offers such a name.
pub fn resolve_source_frontmatter(
    pipeline: &pipeline::PipelineDef,
    completed_node_id: &str,
    iter: i64,
    artifacts_dir: &std::path::Path,
) -> HashMap<String, serde_yaml::Value> {
    let node = match pipeline.nodes.iter().find(|n| n.id == completed_node_id) {
        Some(n) => n,
        None => return HashMap::new(),
    };

    let mut fields = HashMap::new();
    for port in &node.outputs {
        let artifact_path =
            blackboard::artifact_path(artifacts_dir, completed_node_id, iter, &port.name);
        if let Ok(port_fields) =
            pdo_pipeline::adapters::frontmatter_file::parse_frontmatter_from_file(&artifact_path)
        {
            for (k, v) in port_fields {
                fields.insert(format!("{}.{k}", port.name), v.clone());
                fields.insert(k, v);
            }
        }
    }
    fields
}

/// Output frontmatter of every settled node in `run_state`, keyed by node id.
/// Feeds convergence suppression (ADR-0011) so a convergence node does not stall on a branch
/// an upstream conditional/`else` edge permanently suppressed.
pub fn resolve_completed_frontmatter(
    pipeline: &pipeline::PipelineDef,
    run_state: &state::RunState,
    artifacts_dir: &std::path::Path,
) -> HashMap<String, HashMap<String, serde_yaml::Value>> {
    let mut by_node = HashMap::new();
    for (node_id, node_state) in &run_state.nodes {
        // A `Skipped` node counts as a settled producer: it wrote an empty artifact,
        // so it resolves to empty frontmatter.
        if !node_state.status.is_settled_complete() {
            continue;
        }
        let fm = resolve_source_frontmatter(pipeline, node_id, node_state.iter, artifacts_dir);
        by_node.insert(node_id.clone(), fm);
    }
    by_node
}

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;
    use crate::domain::scheduler;
    use crate::domain::scheduler::dispatcher as scheduler_dispatcher;
    use pdo_pipeline::domain::pipeline;
    use pretty_assertions::assert_eq;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    /// A Run's events, its state and their projection, as these tests read them.
    mod event_log {
        pub use crate::domain::event::*;
        pub use crate::domain::projection::*;
        pub use crate::domain::state::*;
    }

    /// #843 / ADR-0073 §3 — a `when:` on a multi-port edge names WHICH carried
    /// output it reads by qualifying the field (`spec.ready`). That only works if
    /// the producer's frontmatter map carries the qualified spelling beside the
    /// merged one every pre-#843 clause reads.
    #[test]
    fn source_frontmatter_carries_both_the_merged_and_the_port_qualified_view() {
        let dir = tempfile::tempdir().unwrap();
        let artifacts = dir.path();

        let write = |port: &str, body: &str| {
            let path = blackboard::artifact_path(artifacts, "design", 1, port);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, body).unwrap();
        };
        write("out", "---\nready: false\nverdict: PASS\n---\n\nbody\n");
        write("spec", "---\nready: true\n---\n\nbody\n");

        let yaml = "name: multi\nnodes:\n  - id: start\n    name: Start\n    type: start\n    outputs:\n      - name: user_prompt\n  - id: design\n    name: design\n    type: agent\n    isolated_worktree: false\n    outputs:\n      - name: out\n      - name: spec\n  - id: end\n    name: End\n    type: end\n    inputs:\n      - name: result\nedges:\n  - source: {node: start, port: user_prompt}\n    target: {node: design, port: brief}\n  - source: {node: design, ports: [out, spec]}\n    target: {node: end, port: result}\n";
        let pipeline = pipeline::parse_pipeline(yaml).unwrap().pipeline;

        let fields = resolve_source_frontmatter(&pipeline, "design", 1, artifacts);

        // Port-qualified: each output's own value, whatever the other says.
        assert_eq!(
            fields.get("out.ready"),
            Some(&serde_yaml::Value::Bool(false))
        );
        assert_eq!(
            fields.get("spec.ready"),
            Some(&serde_yaml::Value::Bool(true))
        );
        // Merged: the bare name an unqualified (pre-#843) clause reads, still
        // present and still last-port-wins.
        assert_eq!(fields.get("ready"), Some(&serde_yaml::Value::Bool(true)));
        assert_eq!(
            fields.get("verdict"),
            Some(&serde_yaml::Value::String("PASS".into()))
        );
        assert_eq!(
            fields.get("out.verdict"),
            Some(&serde_yaml::Value::String("PASS".into()))
        );
    }

    #[test]
    fn passthrough_switch_artifact_copies_input_to_matched_branch() {
        let tmp = tempfile::tempdir().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();

        let upstream_artifact = blackboard::artifact_path(&artifacts_dir, "reviewer", 1, "review");
        std::fs::create_dir_all(upstream_artifact.parent().unwrap()).unwrap();
        std::fs::write(&upstream_artifact, "---\nverdict: PASS\n---\nLooks good.\n").unwrap();

        let pipeline = pipeline::PipelineDef {
            name: "passthrough-test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![],
            edges: vec![pipeline::EdgeDef {
                source: pipeline::EdgeSource::single("reviewer", "review"),
                target: pipeline::EdgeEndpoint {
                    node: "sw".into(),
                    port: "in".into(),
                },
                reason: None,
                when: None,
                is_else: false,
                repeated: false,
                ..Default::default()
            }],
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: true,
            grid_size: None,
        };

        let ctx = SpawnContext {
            pipeline: &pipeline,
            run_id: "test-run",
            pipeline_path: tmp.path(),
            worktree_dir: tmp.path(),
            artifacts_dir: &artifacts_dir,
            resolved_vars: &HashMap::new(),
            repo_root: tmp.path(),
        };

        passthrough_switch_artifact(&ctx, "sw", "pass", 1);

        let dst = blackboard::artifact_path(&artifacts_dir, "sw", 1, "pass");
        assert!(dst.exists(), "passthrough artifact should exist at {dst:?}");
        let content = std::fs::read_to_string(&dst).unwrap();
        assert!(
            content.contains("verdict: PASS"),
            "passthrough should copy content verbatim"
        );
    }

    #[test]
    fn passthrough_switch_artifact_noop_when_no_source() {
        let tmp = tempfile::tempdir().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();

        let pipeline = pipeline::PipelineDef {
            name: "passthrough-noop".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![],
            edges: vec![pipeline::EdgeDef {
                source: pipeline::EdgeSource::single("reviewer", "review"),
                target: pipeline::EdgeEndpoint {
                    node: "sw".into(),
                    port: "in".into(),
                },
                reason: None,
                when: None,
                is_else: false,
                repeated: false,
                ..Default::default()
            }],
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: true,
            grid_size: None,
        };

        let ctx = SpawnContext {
            pipeline: &pipeline,
            run_id: "test-run",
            pipeline_path: tmp.path(),
            worktree_dir: tmp.path(),
            artifacts_dir: &artifacts_dir,
            resolved_vars: &HashMap::new(),
            repo_root: tmp.path(),
        };

        passthrough_switch_artifact(&ctx, "sw", "pass", 1);

        let dst = blackboard::artifact_path(&artifacts_dir, "sw", 1, "pass");
        assert!(
            !dst.exists(),
            "no artifact should be created when source is missing"
        );
    }
}
