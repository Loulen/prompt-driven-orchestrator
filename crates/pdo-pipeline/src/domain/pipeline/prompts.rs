//! A pipeline's prompts: one `.md` per node in the `<stem>.prompts/` sidecar.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use super::definition::PipelineDef;

/// Split a prompt map into the entries whose key is a live node and the ids of
/// those that are not.
///
/// The sidecar prompt dir is keyed by node id and outlives the nodes it was
/// written for: delete a node and its `.md` stays on disk. Every consumer that
/// validates a pipeline against its prompts — the portable-document importer
/// above all — requires *keys ⊆ nodes*, so apply this wherever prompts cross a
/// boundary and that invariant holds by construction rather than by luck.
pub fn split_live_prompts(
    pipeline: &PipelineDef,
    prompts: &HashMap<String, String>,
) -> (HashMap<String, String>, Vec<String>) {
    let live = pipeline
        .nodes
        .iter()
        .map(|node| node.id.as_str())
        .collect::<HashSet<_>>();
    let mut kept = HashMap::with_capacity(prompts.len());
    let mut orphans = Vec::new();
    for (node_id, content) in prompts {
        if live.contains(node_id.as_str()) {
            kept.insert(node_id.clone(), content.clone());
        } else {
            orphans.push(node_id.clone());
        }
    }
    orphans.sort();
    (kept, orphans)
}

pub fn canonical_prompt_path(pipeline_path: &Path, node_id: &str) -> std::path::PathBuf {
    let dir = pipeline_path.parent().unwrap_or(std::path::Path::new("."));
    let stem = pipeline_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("pipeline");
    dir.join(format!("{stem}.prompts"))
        .join(format!("{node_id}.md"))
}
