//! The fan-out lint: isolated nodes converging with no Merge between them.

use std::collections::{HashMap, HashSet};

use crate::domain::pipeline::{Diagnostic, NodeType, PipelineDef, Severity};

/// Detects fan-outs where 2+ **isolated** nodes (#653) share a common downstream
/// target but no Merge node sits between them and the target.
///
/// Isolation, not the node's type, is what makes a fan-out risky: two nodes that
/// each fork a sub-worktree produce two branches nothing reconciles, while two
/// nodes sharing the Run worktree already write to one tree.
///
/// Returns info-only diagnostics (ADR-0001: non-blocking).
#[allow(dead_code)]
pub fn lint_missing_merge(pipeline: &PipelineDef) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    let isolated_ids: HashSet<&str> = pipeline
        .nodes
        .iter()
        .filter(|n| n.node_type != NodeType::Merge && n.is_isolated())
        .map(|n| n.id.as_str())
        .collect();

    let merge_ids: HashSet<&str> = pipeline
        .nodes
        .iter()
        .filter(|n| n.node_type == NodeType::Merge)
        .map(|n| n.id.as_str())
        .collect();

    let mut target_isolated_sources: HashMap<&str, Vec<&str>> = HashMap::new();

    for edge in &pipeline.edges {
        let src = edge.source.node.as_str();
        let tgt = edge.target.node.as_str();
        if isolated_ids.contains(src) && !merge_ids.contains(tgt) {
            target_isolated_sources.entry(tgt).or_default().push(src);
        }
    }

    for (target_id, sources) in &target_isolated_sources {
        if sources.len() >= 2 {
            diagnostics.push(Diagnostic {
                severity: Severity::Warning,
                message: format!(
                    "node '{}' receives edges from {} isolated nodes ({}) without a Merge node — \
                     parallel code changes may conflict at merge time",
                    target_id,
                    sources.len(),
                    sources.join(", "),
                ),
            });
        }
    }

    diagnostics
}
