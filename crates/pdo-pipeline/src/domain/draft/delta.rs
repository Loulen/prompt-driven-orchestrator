//! The delta between two JSON states of a draft, as the replicated structure
//! applies it: field by field, so two people who change two different fields
//! of the same node both keep their change.
//!
//! An element of an array is found by its identity rather than its position:
//! its `id` when it is an object that carries one (nodes, notes, loop
//! regions), its whole JSON otherwise (edges, ports). The delta then still
//! lands on the right element of a draft that someone else changed in the
//! meantime. The frontend applies the same delta to its copy
//! (`frontend/src/pipeline/model/draftDelta.ts`).

use serde_json::{Map, Value};

/// How a value changes.
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    /// Replaced as a whole (a scalar, or a value of another kind).
    Replace(Value),
    /// An object whose fields change.
    Object(Vec<(String, FieldChange)>),
    /// An array whose elements change, in order.
    Array(Vec<ElementChange>),
}

/// How one field of an object changes.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldChange {
    Remove,
    Set(Value),
    Patch(Change),
}

/// How one element of an array changes.
#[derive(Debug, Clone, PartialEq)]
pub enum ElementChange {
    Remove {
        key: ElementKey,
    },
    Patch {
        key: ElementKey,
        change: Change,
    },
    /// Inserted after the element `after` (first when `None`). Inserting an
    /// element whose identity is already there patches it instead.
    Insert {
        after: Option<ElementKey>,
        value: Value,
    },
}

/// The identity of an array element.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ElementKey(String);

impl ElementKey {
    pub fn of(value: &Value) -> Self {
        if let Some(id) = value.get("id").and_then(Value::as_str) {
            return Self(format!("id:{id}"));
        }
        Self(format!("json:{}", canonical(value)))
    }

    /// Whether the identity is an `id`, rather than the whole value.
    pub fn is_id(&self) -> bool {
        self.0.starts_with("id:")
    }
}

/// `value` as JSON with the keys of every object sorted: two equal values
/// always give the same text.
pub fn canonical(value: &Value) -> String {
    match value {
        Value::Object(fields) => {
            let mut keys: Vec<&String> = fields.keys().collect();
            keys.sort();
            let body: Vec<String> = keys
                .into_iter()
                .map(|key| format!("{}:{}", Value::String(key.clone()), canonical(&fields[key])))
                .collect();
            format!("{{{}}}", body.join(","))
        }
        Value::Array(items) => {
            let body: Vec<String> = items.iter().map(canonical).collect();
            format!("[{}]", body.join(","))
        }
        other => other.to_string(),
    }
}

/// The change from `old` to `new`; `None` when they are equal.
pub fn diff(old: &Value, new: &Value) -> Option<Change> {
    if old == new {
        return None;
    }
    match (old, new) {
        (Value::Object(old), Value::Object(new)) => Some(Change::Object(diff_object(old, new))),
        (Value::Array(old), Value::Array(new)) => Some(Change::Array(diff_array(old, new))),
        _ => Some(Change::Replace(new.clone())),
    }
}

fn diff_object(old: &Map<String, Value>, new: &Map<String, Value>) -> Vec<(String, FieldChange)> {
    let mut keys: Vec<&String> = old.keys().chain(new.keys()).collect();
    keys.sort();
    keys.dedup();
    keys.into_iter()
        .filter_map(|key| {
            let change = match (old.get(key), new.get(key)) {
                (Some(_), None) => FieldChange::Remove,
                (None, Some(value)) => FieldChange::Set(value.clone()),
                (Some(before), Some(after)) => match diff(before, after)? {
                    Change::Replace(value) => FieldChange::Set(value),
                    nested => FieldChange::Patch(nested),
                },
                (None, None) => return None,
            };
            Some((key.clone(), change))
        })
        .collect()
}

/// One step of the alignment of two arrays.
enum Step {
    Keep(usize, usize),
    Delete(usize),
    Insert(usize),
}

fn diff_array(old: &[Value], new: &[Value]) -> Vec<ElementChange> {
    let old_keys: Vec<ElementKey> = old.iter().map(ElementKey::of).collect();
    let new_keys: Vec<ElementKey> = new.iter().map(ElementKey::of).collect();
    let steps = align(&old_keys, &new_keys);
    let mut changes = Vec::new();
    let mut i = 0;
    while i < steps.len() {
        match steps[i] {
            Step::Keep(o, n) => {
                if let Some(change) = diff(&old[o], &new[n]) {
                    changes.extend(element_patch(&old_keys[o], change, n, &new_keys));
                }
                i += 1;
            }
            _ => {
                // A run of deletions and insertions between two kept elements.
                let start = i;
                while i < steps.len() && !matches!(steps[i], Step::Keep(..)) {
                    i += 1;
                }
                let deleted: Vec<usize> = steps[start..i]
                    .iter()
                    .filter_map(|s| {
                        if let Step::Delete(o) = s {
                            Some(*o)
                        } else {
                            None
                        }
                    })
                    .collect();
                let inserted: Vec<usize> = steps[start..i]
                    .iter()
                    .filter_map(|s| {
                        if let Step::Insert(n) = s {
                            Some(*n)
                        } else {
                            None
                        }
                    })
                    .collect();
                changes.extend(run_changes(
                    old, new, &old_keys, &new_keys, &deleted, &inserted,
                ));
            }
        }
    }
    changes
}

/// The changes of a run: an element without `id` replaced by another one
/// without `id` at the same place is the same element edited (an edge whose
/// condition changed), so its fields merge; the rest is removed or inserted.
fn run_changes(
    old: &[Value],
    new: &[Value],
    old_keys: &[ElementKey],
    new_keys: &[ElementKey],
    deleted: &[usize],
    inserted: &[usize],
) -> Vec<ElementChange> {
    let mut changes = Vec::new();
    let mut paired = 0;
    while paired < deleted.len().min(inserted.len()) {
        let (o, n) = (deleted[paired], inserted[paired]);
        let both_plain = !old_keys[o].is_id() && !new_keys[n].is_id();
        if !(both_plain && old[o].is_object() && new[n].is_object()) {
            break;
        }
        if let Some(change) = diff(&old[o], &new[n]) {
            changes.extend(element_patch(&old_keys[o], change, n, new_keys));
        }
        paired += 1;
    }
    for &o in &deleted[paired..] {
        changes.push(ElementChange::Remove {
            key: old_keys[o].clone(),
        });
    }
    for &n in &inserted[paired..] {
        changes.push(ElementChange::Insert {
            after: n.checked_sub(1).map(|p| new_keys[p].clone()),
            value: new[n].clone(),
        });
    }
    changes
}

/// A patch of the element `key`, which becomes `new[n]`; a replacement (a
/// value that is no object) removes it and inserts the new one in its place.
fn element_patch(
    key: &ElementKey,
    change: Change,
    n: usize,
    new_keys: &[ElementKey],
) -> Vec<ElementChange> {
    match change {
        Change::Replace(value) => vec![
            ElementChange::Remove { key: key.clone() },
            ElementChange::Insert {
                after: n.checked_sub(1).map(|p| new_keys[p].clone()),
                value,
            },
        ],
        change => vec![ElementChange::Patch {
            key: key.clone(),
            change,
        }],
    }
}

/// The longest common subsequence of the two key lists, as steps.
fn align(old: &[ElementKey], new: &[ElementKey]) -> Vec<Step> {
    let (n, m) = (old.len(), new.len());
    let mut table = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            table[i][j] = if old[i] == new[j] {
                table[i + 1][j + 1] + 1
            } else {
                table[i + 1][j].max(table[i][j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut steps = Vec::with_capacity(n.max(m));
    while i < n && j < m {
        if old[i] == new[j] {
            steps.push(Step::Keep(i, j));
            i += 1;
            j += 1;
        } else if table[i + 1][j] >= table[i][j + 1] {
            steps.push(Step::Delete(i));
            i += 1;
        } else {
            steps.push(Step::Insert(j));
            j += 1;
        }
    }
    steps.extend((i..n).map(Step::Delete));
    steps.extend((j..m).map(Step::Insert));
    steps
}

/// `change` applied to `value`, as the replicated structure applies it: an
/// element found by its identity, a removed one that is already gone left
/// alone, an inserted one that is already there patched.
pub fn apply(value: &mut Value, change: &Change) {
    match (change, &mut *value) {
        (Change::Object(fields), Value::Object(object)) => {
            for (key, field) in fields {
                match field {
                    FieldChange::Remove => {
                        object.remove(key);
                    }
                    FieldChange::Set(new) => {
                        object.insert(key.clone(), new.clone());
                    }
                    FieldChange::Patch(nested) => match object.get_mut(key) {
                        Some(current) => apply(current, nested),
                        None => {
                            let mut fresh = empty_like(nested);
                            apply(&mut fresh, nested);
                            object.insert(key.clone(), fresh);
                        }
                    },
                }
            }
        }
        (Change::Array(elements), Value::Array(array)) => {
            for element in elements {
                apply_element(array, element);
            }
        }
        (Change::Replace(new), _) => *value = new.clone(),
        (nested, _) => {
            *value = empty_like(nested);
            apply(value, nested);
        }
    }
}

fn empty_like(change: &Change) -> Value {
    match change {
        Change::Array(_) => Value::Array(Vec::new()),
        _ => Value::Object(Map::new()),
    }
}

fn apply_element(array: &mut Vec<Value>, change: &ElementChange) {
    let position =
        |array: &[Value], key: &ElementKey| array.iter().position(|e| &ElementKey::of(e) == key);
    match change {
        ElementChange::Remove { key } => {
            if let Some(i) = position(array, key) {
                array.remove(i);
            }
        }
        ElementChange::Patch { key, change } => {
            if let Some(i) = position(array, key) {
                apply(&mut array[i], change);
            }
        }
        ElementChange::Insert { after, value } => {
            if let Some(i) = position(array, &ElementKey::of(value)) {
                array[i] = value.clone();
                return;
            }
            let at = match after {
                None => 0,
                Some(key) => position(array, key).map_or(array.len(), |i| i + 1),
            };
            array.insert(at, value.clone());
        }
    }
}

/// How a prompt changes: the span `[index, index + delete)` (UTF-16 code
/// units, as the text of the replicated structure counts them) replaced by
/// `insert`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptChange {
    pub index: u32,
    pub delete: u32,
    pub insert: String,
}

/// The change from the prompt `old` to `new`: the common prefix and suffix
/// stay, so two people typing in two places of the same prompt both keep
/// their text. `None` when they are equal.
pub fn diff_prompt(old: &str, new: &str) -> Option<PromptChange> {
    if old == new {
        return None;
    }
    let old16: Vec<u16> = old.encode_utf16().collect();
    let new16: Vec<u16> = new.encode_utf16().collect();
    let mut prefix = 0;
    while prefix < old16.len() && prefix < new16.len() && old16[prefix] == new16[prefix] {
        prefix += 1;
    }
    let mut suffix = 0;
    while suffix < old16.len() - prefix
        && suffix < new16.len() - prefix
        && old16[old16.len() - 1 - suffix] == new16[new16.len() - 1 - suffix]
    {
        suffix += 1;
    }
    // Never split a surrogate pair.
    while prefix > 0 && is_low_surrogate(old16.get(prefix).copied()) {
        prefix -= 1;
    }
    while suffix > 0 && is_low_surrogate(old16.get(old16.len() - suffix).copied()) {
        suffix -= 1;
    }
    let insert = String::from_utf16_lossy(&new16[prefix..new16.len() - suffix]);
    Some(PromptChange {
        index: prefix as u32,
        delete: (old16.len() - suffix - prefix) as u32,
        insert,
    })
}

fn is_low_surrogate(unit: Option<u16>) -> bool {
    unit.is_some_and(|u| (0xDC00..=0xDFFF).contains(&u))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn key(value: Value) -> ElementKey {
        ElementKey::of(&value)
    }

    #[test]
    fn equal_values_have_no_change() {
        assert_eq!(diff(&json!({"a": [1, 2]}), &json!({"a": [1, 2]})), None);
    }

    #[test]
    fn an_object_changes_field_by_field() {
        let change = diff(
            &json!({"name": "a", "drop": 1, "keep": true, "nested": {"x": 1, "y": 2}}),
            &json!({"name": "b", "keep": true, "nested": {"x": 1, "y": 3}, "new": [1]}),
        );
        assert_eq!(
            change,
            Some(Change::Object(vec![
                ("drop".into(), FieldChange::Remove),
                ("name".into(), FieldChange::Set(json!("b"))),
                (
                    "nested".into(),
                    FieldChange::Patch(Change::Object(vec![(
                        "y".into(),
                        FieldChange::Set(json!(3))
                    )]))
                ),
                ("new".into(), FieldChange::Set(json!([1]))),
            ]))
        );
    }

    #[test]
    fn a_node_is_patched_by_its_id_wherever_it_sits() {
        let old = json!([{"id": "a", "name": "A"}, {"id": "b", "name": "B"}]);
        let new = json!([{"id": "a", "name": "A"}, {"id": "b", "name": "B2"}]);
        assert_eq!(
            diff(&old, &new),
            Some(Change::Array(vec![ElementChange::Patch {
                key: key(json!({"id": "b"})),
                change: Change::Object(vec![("name".into(), FieldChange::Set(json!("B2")))]),
            }]))
        );
    }

    #[test]
    fn an_added_node_is_inserted_after_its_predecessor() {
        let old = json!([{"id": "a"}]);
        let new = json!([{"id": "a"}, {"id": "b"}]);
        assert_eq!(
            diff(&old, &new),
            Some(Change::Array(vec![ElementChange::Insert {
                after: Some(key(json!({"id": "a"}))),
                value: json!({"id": "b"}),
            }]))
        );
    }

    #[test]
    fn removed_elements_are_named_by_identity() {
        let old = json!([{"source": "a"}, {"id": "n"}, {"source": "b"}]);
        let new = json!([{"id": "n"}]);
        assert_eq!(
            diff(&old, &new),
            Some(Change::Array(vec![
                ElementChange::Remove {
                    key: key(json!({"source": "a"}))
                },
                ElementChange::Remove {
                    key: key(json!({"source": "b"}))
                },
            ]))
        );
    }

    #[test]
    fn an_edited_edge_merges_its_fields() {
        let old = json!([{"source": "a", "target": "b"}]);
        let new = json!([{"source": "a", "target": "b", "else": true}]);
        assert_eq!(
            diff(&old, &new),
            Some(Change::Array(vec![ElementChange::Patch {
                key: key(json!({"source": "a", "target": "b"})),
                change: Change::Object(vec![("else".into(), FieldChange::Set(json!(true)))]),
            }]))
        );
    }

    #[test]
    fn a_scalar_element_is_replaced() {
        assert_eq!(
            diff(&json!(["x", "y"]), &json!(["x", "z"])),
            Some(Change::Array(vec![
                ElementChange::Remove {
                    key: key(json!("y"))
                },
                ElementChange::Insert {
                    after: Some(key(json!("x"))),
                    value: json!("z")
                },
            ]))
        );
    }

    #[test]
    fn applying_a_diff_gives_the_new_value() {
        let old = json!({
            "name": "p",
            "nodes": [{"id": "a", "view": {"x": 1}}, {"id": "b"}, {"id": "c"}],
            "edges": [{"source": "a", "target": "b"}, {"source": "b", "target": "c"}],
            "tags": ["x", "y"],
        });
        let new = json!({
            "name": "q",
            "nodes": [{"id": "c"}, {"id": "a", "view": {"x": 2}}, {"id": "d"}],
            "edges": [{"source": "a", "target": "c", "else": true}],
            "tags": ["y", "z"],
            "loops": [],
        });
        let mut value = old.clone();
        apply(&mut value, &diff(&old, &new).unwrap());
        assert_eq!(canonical(&value), canonical(&new));
    }

    #[test]
    fn a_diff_lands_on_a_draft_someone_else_changed() {
        let base = json!({"nodes": [{"id": "a", "name": "A", "prompt_x": 1}]});
        let mine = json!({"nodes": [{"id": "a", "name": "A2", "prompt_x": 1}]});
        // Meanwhile, someone added a node first and changed another field.
        let mut theirs = json!({"nodes": [{"id": "z"}, {"id": "a", "name": "A", "prompt_x": 9}]});
        apply(&mut theirs, &diff(&base, &mine).unwrap());
        assert_eq!(
            theirs,
            json!({"nodes": [{"id": "z"}, {"id": "a", "name": "A2", "prompt_x": 9}]})
        );
    }

    #[test]
    fn inserting_a_node_already_there_patches_it() {
        let mut draft = json!([{"id": "a", "name": "old"}]);
        apply(
            &mut draft,
            &Change::Array(vec![ElementChange::Insert {
                after: None,
                value: json!({"id": "a", "name": "new"}),
            }]),
        );
        assert_eq!(draft, json!([{"id": "a", "name": "new"}]));
    }

    #[test]
    fn canonical_json_ignores_key_order() {
        assert_eq!(
            canonical(&json!({"b": 1, "a": [{"d": 1, "c": 2}]})),
            canonical(&json!({"a": [{"c": 2, "d": 1}], "b": 1}))
        );
    }

    #[test]
    fn a_prompt_changes_between_its_common_prefix_and_suffix() {
        assert_eq!(diff_prompt("abc", "abc"), None);
        assert_eq!(
            diff_prompt("Keep it short and clear", "Keep it very short and clear"),
            Some(PromptChange {
                index: 8,
                delete: 0,
                insert: "very ".into()
            })
        );
        assert_eq!(
            diff_prompt("hello world", "hello"),
            Some(PromptChange {
                index: 5,
                delete: 6,
                insert: String::new()
            })
        );
    }

    #[test]
    fn a_prompt_counts_in_utf16_units() {
        // "é" is one UTF-16 unit, "😀" two.
        assert_eq!(
            diff_prompt("é😀a", "é😀ba"),
            Some(PromptChange {
                index: 3,
                delete: 0,
                insert: "b".into()
            })
        );
        assert_eq!(
            diff_prompt("😀", "😁"),
            Some(PromptChange {
                index: 0,
                delete: 2,
                insert: "😁".into()
            })
        );
    }
}
