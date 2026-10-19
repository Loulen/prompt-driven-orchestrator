//! How many changes a draft carries since the file it started from: what
//! « draft · N changes » shows, on the tab and in the list. Zero means the
//! draft is the file: nothing left to save.
//!
//! A node counts once however many of its fields (and its prompt) changed; an
//! edge, a note or a loop region once; each other top-level field of the
//! pipeline (name, variables, grid…) once.

use std::collections::{BTreeMap, HashMap};

use serde_json::Value;

use super::delta::canonical;
use super::DraftContent;

/// The arrays whose elements carry an `id`.
const BY_ID: [&str; 3] = ["nodes", "notes", "loops"];
/// The array whose elements are known by their whole value.
const EDGES: &str = "edges";

/// The number of changes from `baseline` to `draft`.
pub fn change_count(baseline: &DraftContent, draft: &DraftContent) -> u32 {
    let empty = serde_json::Map::new();
    let old = baseline.pipeline.as_object().unwrap_or(&empty);
    let new = draft.pipeline.as_object().unwrap_or(&empty);
    let mut count = 0;
    let mut changed_nodes: Vec<String> = Vec::new();
    for key in BY_ID {
        let (n, changed) = changed_by_id(old.get(key), new.get(key));
        count += n;
        if key == "nodes" {
            changed_nodes = changed;
        }
    }
    count += changed_edges(old.get(EDGES), new.get(EDGES));
    let mut other_keys: Vec<&String> = old.keys().chain(new.keys()).collect();
    other_keys.sort();
    other_keys.dedup();
    count += other_keys
        .into_iter()
        .filter(|key| !BY_ID.contains(&key.as_str()) && key.as_str() != EDGES)
        .filter(|key| old.get(*key) != new.get(*key))
        .count() as u32;
    count + changed_prompts(baseline, draft, new.get("nodes"), &changed_nodes)
}

/// The elements of two `id` arrays that were added, removed or changed, and
/// their ids.
fn changed_by_id(old: Option<&Value>, new: Option<&Value>) -> (u32, Vec<String>) {
    let old = by_id(old);
    let new = by_id(new);
    let mut ids: Vec<&String> = old.keys().chain(new.keys()).collect();
    ids.sort();
    ids.dedup();
    let changed: Vec<String> = ids
        .into_iter()
        .filter(|id| old.get(*id) != new.get(*id))
        .cloned()
        .collect();
    (changed.len() as u32, changed)
}

fn by_id(array: Option<&Value>) -> BTreeMap<String, &Value> {
    array
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|element| Some((element.get("id")?.as_str()?.to_string(), element)))
        .collect()
}

/// The edges removed or added: an edge edited in place is one removed and
/// one added, which counts once.
fn changed_edges(old: Option<&Value>, new: Option<&Value>) -> u32 {
    let mut remaining: HashMap<String, u32> = HashMap::new();
    for edge in old.and_then(Value::as_array).into_iter().flatten() {
        *remaining.entry(canonical(edge)).or_default() += 1;
    }
    let mut added = 0;
    for edge in new.and_then(Value::as_array).into_iter().flatten() {
        match remaining.get_mut(&canonical(edge)) {
            Some(left) if *left > 0 => *left -= 1,
            _ => added += 1,
        }
    }
    let removed: u32 = remaining.values().sum();
    removed.max(added)
}

/// The prompts that changed on a node of the draft not already counted.
fn changed_prompts(
    baseline: &DraftContent,
    draft: &DraftContent,
    nodes: Option<&Value>,
    counted: &[String],
) -> u32 {
    by_id(nodes)
        .keys()
        .filter(|id| !counted.contains(id))
        .filter(|id| {
            let before = baseline.prompts.get(*id).map(String::as_str).unwrap_or("");
            let after = draft.prompts.get(*id).map(String::as_str).unwrap_or("");
            before != after
        })
        .count() as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn content(pipeline: Value, prompts: &[(&str, &str)]) -> DraftContent {
        DraftContent::new(
            pipeline,
            prompts.iter().map(|(k, v)| (k.to_string(), v.to_string())),
        )
    }

    fn base() -> Value {
        json!({
            "name": "p",
            "nodes": [{"id": "a", "name": "A"}, {"id": "b", "name": "B"}],
            "edges": [{"source": "a", "target": "b"}],
            "notes": [],
        })
    }

    #[test]
    fn the_file_itself_has_no_change() {
        let file = content(base(), &[("a", "hi")]);
        assert_eq!(change_count(&file, &file.clone()), 0);
    }

    #[test]
    fn a_node_counts_once_however_many_of_its_fields_changed() {
        let mut draft = base();
        draft["nodes"][0] = json!({"id": "a", "name": "A2", "view": {"x": 1, "y": 2}});
        let changed = content(draft, &[("a", "new prompt")]);
        assert_eq!(change_count(&content(base(), &[("a", "hi")]), &changed), 1);
    }

    #[test]
    fn added_removed_nodes_edges_and_fields_each_count() {
        let draft = json!({
            "name": "renamed",
            "nodes": [{"id": "a", "name": "A"}, {"id": "c"}],
            "edges": [{"source": "a", "target": "c"}],
            "notes": [{"id": "n1", "content": ""}],
        });
        // b removed, c added, the edge edited, a note added, the name.
        assert_eq!(change_count(&content(base(), &[]), &content(draft, &[])), 5);
    }

    #[test]
    fn a_prompt_alone_counts() {
        let before = content(base(), &[("b", "x")]);
        let after = content(base(), &[("b", "y")]);
        assert_eq!(change_count(&before, &after), 1);
    }
}
