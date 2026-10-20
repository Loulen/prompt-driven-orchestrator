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

/// What changed from `baseline` to `draft`, in words, in the order of the
/// pipeline: what the notice of a write names (« prompt of reviewer »,
/// « edge reviewer → fixer »). Empty when nothing did.
pub fn changed_fields(baseline: &DraftContent, draft: &DraftContent) -> Vec<String> {
    let empty = serde_json::Map::new();
    let old = baseline.pipeline.as_object().unwrap_or(&empty);
    let new = draft.pipeline.as_object().unwrap_or(&empty);
    let mut fields = Vec::new();
    let mut other_keys: Vec<&String> = old.keys().chain(new.keys()).collect();
    other_keys.sort();
    other_keys.dedup();
    for key in other_keys {
        if !BY_ID.contains(&key.as_str()) && key.as_str() != EDGES && old.get(key) != new.get(key) {
            fields.push(key.clone());
        }
    }
    fields.extend(changed_nodes(
        baseline,
        draft,
        old.get("nodes"),
        new.get("nodes"),
    ));
    fields.extend(changed_edge_words(old.get(EDGES), new.get(EDGES)));
    for (key, word) in [("notes", "note"), ("loops", "loop region")] {
        let (_, changed) = changed_by_id(old.get(key), new.get(key));
        let (before, after) = (by_id(old.get(key)), by_id(new.get(key)));
        for id in changed {
            fields.push(match (before.contains_key(&id), after.contains_key(&id)) {
                (false, _) => format!("new {word}"),
                (_, false) => format!("{word} removed"),
                _ => word.to_string(),
            });
        }
    }
    let mut seen = std::collections::HashSet::new();
    fields.retain(|field| seen.insert(field.clone()));
    fields
}

/// The nodes added, removed, and the fields (and prompt) of the others that
/// changed, by node id.
fn changed_nodes(
    baseline: &DraftContent,
    draft: &DraftContent,
    old: Option<&Value>,
    new: Option<&Value>,
) -> Vec<String> {
    let before = by_id(old);
    let after = by_id(new);
    let mut words = Vec::new();
    for (id, node) in &after {
        let Some(was) = before.get(id) else {
            words.push(format!("new node {id}"));
            continue;
        };
        if was != node {
            let empty = serde_json::Map::new();
            let was = was.as_object().unwrap_or(&empty);
            let node = node.as_object().unwrap_or(&empty);
            let mut keys: Vec<&String> = was.keys().chain(node.keys()).collect();
            keys.sort();
            keys.dedup();
            for key in keys
                .into_iter()
                .filter(|key| was.get(*key) != node.get(*key))
            {
                let field = if key == "view" {
                    "position"
                } else {
                    key.as_str()
                };
                words.push(format!("{field} of {id}"));
            }
        }
        let prompt = |content: &DraftContent| content.prompts.get(id).cloned().unwrap_or_default();
        if prompt(baseline) != prompt(draft) {
            words.push(format!("prompt of {id}"));
        }
    }
    for id in before.keys().filter(|id| !after.contains_key(*id)) {
        words.push(format!("node {id} removed"));
    }
    words
}

/// The edges added and removed, by their ends: an edge edited in place is
/// named once.
fn changed_edge_words(old: Option<&Value>, new: Option<&Value>) -> Vec<String> {
    let mut remaining: Vec<&Value> = old
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .collect();
    let mut added = Vec::new();
    for edge in new.and_then(Value::as_array).into_iter().flatten() {
        match remaining
            .iter()
            .position(|old| canonical(old) == canonical(edge))
        {
            Some(i) => {
                remaining.remove(i);
            }
            None => added.push(edge_words(edge)),
        }
    }
    let removed: Vec<String> = remaining
        .into_iter()
        .map(edge_words)
        .filter(|words| !added.contains(words))
        .map(|words| format!("{words} removed"))
        .collect();
    added.into_iter().chain(removed).collect()
}

/// « edge a → b ».
fn edge_words(edge: &Value) -> String {
    let end = |side: &str| edge[side]["node"].as_str().unwrap_or("?").to_string();
    format!("edge {} \u{2192} {}", end("source"), end("target"))
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

    #[test]
    fn the_changed_fields_are_named() {
        let before = content(base(), &[("b", "x")]);
        let mut pipeline = base();
        pipeline["nodes"][0] = json!({"id": "a", "name": "A2", "view": {"x": 1}});
        pipeline["nodes"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id": "c"}));
        pipeline["edges"] = json!([{"source": {"node": "b"}, "target": {"node": "c"}}]);
        pipeline["name"] = json!("renamed");
        let after = content(pipeline, &[("b", "y")]);
        assert_eq!(
            changed_fields(&before, &after),
            vec![
                "name",
                "name of a",
                "position of a",
                "prompt of b",
                "new node c",
                "edge b \u{2192} c",
                "edge ? \u{2192} ? removed",
            ]
        );
    }

    #[test]
    fn an_edge_edited_in_place_is_named_once_and_nothing_changed_is_empty() {
        let edge =
            |when: Value| json!({"source": {"node": "a"}, "target": {"node": "b"}, "when": when});
        let mut before = base();
        before["edges"] = json!([edge(json!(1))]);
        let mut after = base();
        after["edges"] = json!([edge(json!(2))]);
        let (before, after) = (content(before, &[]), content(after, &[]));
        assert_eq!(changed_fields(&before, &after), vec!["edge a \u{2192} b"]);
        assert!(changed_fields(&before, &before.clone()).is_empty());
    }
}
