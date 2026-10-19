//! The replicated document of a draft over `yrs`, the Rust implementation of
//! the Yjs CRDT: its updates (format v1) are the ones the `yjs` package of the
//! frontend reads and writes.
//!
//! Two roots: `pipeline`, the pipeline as `GET /pipelines/{id}` serves it
//! (an object is a map, an array an array, the rest a value), and `prompts`,
//! one text per node. Text offsets count UTF-16 code units, as Yjs does.

use std::sync::Arc;

use serde_json::{Map as JsonMap, Value};
use yrs::types::ToJson;
use yrs::updates::decoder::Decode;
use yrs::updates::encoder::Encode;
use yrs::{
    Any, Array, ArrayPrelim, ArrayRef, Doc, GetString, In, Map, MapPrelim, MapRef, Number,
    OffsetKind, Options, Out, ReadTxn, StateVector, Text, TextPrelim, Transact, TransactionMut,
    Update,
};

use crate::application::ports::{DraftDocument, DraftDocuments};
use crate::domain::draft::delta::{
    apply, diff, diff_prompt, Change, ElementChange, ElementKey, FieldChange, PromptChange,
};
use crate::domain::draft::{draft_number, DraftContent};

const PIPELINE: &str = "pipeline";
const PROMPTS: &str = "prompts";

/// Makes [`YDraft`]s.
pub struct YDraftDocuments;

impl DraftDocuments for YDraftDocuments {
    fn create(&self, content: &DraftContent) -> Box<dyn DraftDocument> {
        let draft = YDraft::empty();
        draft.change(&DraftContent::default(), content);
        Box::new(draft)
    }

    fn load(&self, state: &[u8]) -> Result<Box<dyn DraftDocument>, String> {
        let draft = YDraft::empty();
        draft.apply_update(state)?;
        Ok(Box::new(draft))
    }
}

/// One draft's document.
pub struct YDraft {
    doc: Doc,
    pipeline: MapRef,
    prompts: MapRef,
}

impl YDraft {
    fn empty() -> Self {
        let doc = Doc::with_options(Options {
            offset_kind: OffsetKind::Utf16,
            ..Options::default()
        });
        let pipeline = doc.get_or_insert_map(PIPELINE);
        let prompts = doc.get_or_insert_map(PROMPTS);
        Self {
            doc,
            pipeline,
            prompts,
        }
    }
}

impl DraftDocument for YDraft {
    fn content(&self) -> DraftContent {
        let txn = self.doc.transact();
        let pipeline = any_to_json(self.pipeline.to_json(&txn));
        let prompts = self
            .prompts
            .iter(&txn)
            .filter_map(|(node, value)| Some((node.to_string(), text_of(&txn, value)?)))
            .collect();
        DraftContent { pipeline, prompts }
    }

    fn apply_update(&self, update: &[u8]) -> Result<(), String> {
        let update = Update::decode_v1(update).map_err(|e| e.to_string())?;
        self.doc
            .transact_mut()
            .apply_update(update)
            .map_err(|e| e.to_string())
    }

    fn change(&self, from: &DraftContent, to: &DraftContent) -> Vec<u8> {
        let before = self.doc.transact().state_vector();
        {
            let mut txn = self.doc.transact_mut();
            match diff(&from.pipeline, &to.pipeline) {
                Some(Change::Object(fields)) => apply_object(&mut txn, &self.pipeline, &fields),
                Some(_) => reset_map(&mut txn, &self.pipeline, &to.pipeline),
                None => {}
            }
            change_prompts(&mut txn, &self.prompts, from, to);
        }
        self.doc.transact().encode_state_as_update_v1(&before)
    }

    fn encode(&self) -> Vec<u8> {
        self.doc
            .transact()
            .encode_state_as_update_v1(&StateVector::default())
    }

    fn state_vector(&self) -> Vec<u8> {
        self.doc.transact().state_vector().encode_v1()
    }

    fn missing_for(&self, state_vector: &[u8]) -> Result<Vec<u8>, String> {
        let state_vector = StateVector::decode_v1(state_vector).map_err(|e| e.to_string())?;
        Ok(self.doc.transact().encode_state_as_update_v1(&state_vector))
    }
}

fn text_of<T: ReadTxn>(txn: &T, value: Out) -> Option<String> {
    match value {
        Out::YText(text) => Some(text.get_string(txn)),
        Out::Any(Any::String(s)) => Some(s.to_string()),
        _ => None,
    }
}

fn change_prompts(
    txn: &mut TransactionMut,
    prompts: &MapRef,
    from: &DraftContent,
    to: &DraftContent,
) {
    for node in from.prompts.keys().filter(|n| !to.prompts.contains_key(*n)) {
        prompts.remove(txn, node);
    }
    for (node, new) in &to.prompts {
        let old = from.prompts.get(node).map(String::as_str).unwrap_or("");
        let text = match prompts.get(txn, node) {
            Some(Out::YText(text)) => text,
            _ => {
                prompts.insert(txn, node.as_str(), TextPrelim::new(new.as_str()));
                continue;
            }
        };
        if let Some(PromptChange {
            index,
            delete,
            insert,
        }) = diff_prompt(old, new)
        {
            let len = text.len(txn);
            let index = index.min(len);
            text.remove_range(txn, index, delete.min(len - index));
            text.insert(txn, index, &insert);
        }
    }
}

fn reset_map(txn: &mut TransactionMut, map: &MapRef, value: &Value) {
    let keys: Vec<String> = map.keys(txn).map(str::to_string).collect();
    for key in keys {
        map.remove(txn, &key);
    }
    for (key, field) in value.as_object().into_iter().flatten() {
        map.insert(txn, key.as_str(), to_in(field));
    }
}

fn apply_object(txn: &mut TransactionMut, map: &MapRef, fields: &[(String, FieldChange)]) {
    for (key, field) in fields {
        match field {
            FieldChange::Remove => {
                map.remove(txn, key);
            }
            FieldChange::Set(value) => {
                map.insert(txn, key.as_str(), to_in(value));
            }
            FieldChange::Patch(change) => match (map.get(txn, key), change) {
                (Some(Out::YMap(nested)), Change::Object(fields)) => {
                    apply_object(txn, &nested, fields)
                }
                (Some(Out::YArray(nested)), Change::Array(elements)) => {
                    apply_array(txn, &nested, elements)
                }
                (current, change) => {
                    // The structure differs from what the change expects:
                    // rewrite the field from its value.
                    let mut value =
                        current.map_or(Value::Null, |out| any_to_json(out.to_json(txn)));
                    apply(&mut value, change);
                    map.insert(txn, key.as_str(), to_in(&value));
                }
            },
        }
    }
}

fn keys_of(txn: &TransactionMut, array: &ArrayRef) -> Vec<ElementKey> {
    array
        .iter(txn)
        .map(|out| ElementKey::of(&any_to_json(out.to_json(txn))))
        .collect()
}

fn apply_array(txn: &mut TransactionMut, array: &ArrayRef, elements: &[ElementChange]) {
    for element in elements {
        let keys = keys_of(txn, array);
        let position = |key: &ElementKey| keys.iter().position(|k| k == key).map(|i| i as u32);
        match element {
            ElementChange::Remove { key } => {
                if let Some(i) = position(key) {
                    array.remove(txn, i);
                }
            }
            ElementChange::Patch { key, change } => {
                if let Some(i) = position(key) {
                    patch_element(txn, array, i, change);
                }
            }
            ElementChange::Insert { after, value } => {
                let key = ElementKey::of(value);
                if let Some(i) = position(&key) {
                    let current = array
                        .get(txn, i)
                        .map_or(Value::Null, |o| any_to_json(o.to_json(txn)));
                    if let Some(change) = diff(&current, value) {
                        patch_element(txn, array, i, &change);
                    }
                    continue;
                }
                let at = match after {
                    None => 0,
                    Some(after) => position(after).map_or(array.len(txn), |i| i + 1),
                };
                array.insert(txn, at, to_in(value));
            }
        }
    }
}

fn patch_element(txn: &mut TransactionMut, array: &ArrayRef, index: u32, change: &Change) {
    match (array.get(txn, index), change) {
        (Some(Out::YMap(map)), Change::Object(fields)) => apply_object(txn, &map, fields),
        (Some(Out::YArray(nested)), Change::Array(elements)) => apply_array(txn, &nested, elements),
        (current, change) => {
            let mut value = current.map_or(Value::Null, |out| any_to_json(out.to_json(txn)));
            apply(&mut value, change);
            array.remove(txn, index);
            array.insert(txn, index, to_in(&value));
        }
    }
}

/// A JSON value as the document holds it.
fn to_in(value: &Value) -> In {
    match value {
        Value::Null => In::Any(Any::Null),
        Value::Bool(b) => In::Any(Any::Bool(*b)),
        Value::Number(n) => In::Any(match n.as_i64() {
            Some(i) => Any::from(i),
            None => Any::from(n.as_f64().unwrap_or(0.0)),
        }),
        Value::String(s) => In::Any(Any::String(Arc::from(s.as_str()))),
        Value::Array(items) => In::Array(ArrayPrelim::from_iter(items.iter().map(to_in))),
        Value::Object(fields) => In::Map(MapPrelim::from_iter(
            fields.iter().map(|(k, v)| (k.as_str(), to_in(v))),
        )),
    }
}

/// A value of the document as JSON. A whole float is an integer again
/// ([`draft_number`]: the frontend's numbers have no integer type);
/// `undefined` is no value.
fn any_to_json(any: Any) -> Value {
    match any {
        Any::Null | Any::Undefined | Any::Buffer(_) => Value::Null,
        Any::Bool(b) => Value::Bool(b),
        Any::Number(Number::Int(i)) => Value::from(i),
        Any::Number(Number::Float(f)) => draft_number(f),
        Any::String(s) => Value::String(s.to_string()),
        Any::Array(items) => Value::Array(items.iter().cloned().map(any_to_json).collect()),
        Any::Map(fields) => Value::Object(
            fields
                .iter()
                .filter(|(_, v)| !matches!(v, Any::Undefined))
                .map(|(k, v)| (k.clone(), any_to_json(v.clone())))
                .collect::<JsonMap<String, Value>>(),
        ),
    }
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

    fn base() -> DraftContent {
        content(
            json!({
                "name": "p",
                "grid_size": null,
                "nodes": [
                    {"id": "a", "name": "A", "view": {"x": 100, "y": 40}, "outputs": [{"name": "spec"}]},
                    {"id": "b", "name": "B", "view": {"x": 100, "y": 200}},
                ],
                "edges": [{"source": {"node": "a", "port": "spec"}, "target": {"node": "b", "port": "spec"}}],
            }),
            &[("a", "Read the task."), ("b", "Review it.")],
        )
    }

    /// Two participants holding the same draft.
    fn pair() -> (Box<dyn DraftDocument>, Box<dyn DraftDocument>) {
        let first = YDraftDocuments.create(&base());
        let second = YDraftDocuments.load(&first.encode()).unwrap();
        (first, second)
    }

    #[test]
    fn a_draft_holds_its_content_through_its_encoding() {
        let draft = YDraftDocuments.create(&base());
        assert_eq!(draft.content(), base());
        assert_eq!(
            YDraftDocuments.load(&draft.encode()).unwrap().content(),
            base()
        );
    }

    #[test]
    fn two_fields_changed_at_once_both_stay() {
        let (first, second) = pair();
        let mut renamed = base();
        renamed.pipeline["nodes"][0]["name"] = json!("Designer");
        let mut moved = base();
        moved.pipeline["nodes"][0]["view"] = json!({"x": 300, "y": 40});
        let one = first.change(&base(), &renamed);
        let two = second.change(&base(), &moved);
        first.apply_update(&two).unwrap();
        second.apply_update(&one).unwrap();
        let mut both = base();
        both.pipeline["nodes"][0]["name"] = json!("Designer");
        both.pipeline["nodes"][0]["view"] = json!({"x": 300, "y": 40});
        assert_eq!(first.content(), both);
        assert_eq!(second.content(), both);
    }

    #[test]
    fn one_field_changed_at_once_converges_for_all() {
        let (first, second) = pair();
        let mut mine = base();
        mine.pipeline["name"] = json!("mine");
        let mut theirs = base();
        theirs.pipeline["name"] = json!("theirs");
        let one = first.change(&base(), &mine);
        let two = second.change(&base(), &theirs);
        first.apply_update(&two).unwrap();
        second.apply_update(&one).unwrap();
        assert_eq!(first.content(), second.content());
    }

    #[test]
    fn nodes_added_at_once_both_land() {
        let (first, second) = pair();
        let mut mine = base();
        mine.pipeline["nodes"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id": "c"}));
        let mut theirs = base();
        theirs.pipeline["nodes"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id": "d"}));
        let one = first.change(&base(), &mine);
        second
            .apply_update(&second.change(&base(), &theirs))
            .unwrap();
        first.apply_update(&second.encode()).unwrap();
        second.apply_update(&one).unwrap();
        let ids = |d: &dyn DraftDocument| {
            let mut ids: Vec<String> = d.content().pipeline["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|n| n["id"].as_str().unwrap().to_string())
                .collect();
            ids.sort();
            ids
        };
        assert_eq!(ids(first.as_ref()), vec!["a", "b", "c", "d"]);
        assert_eq!(first.content(), second.content());
    }

    #[test]
    fn two_people_typing_in_one_prompt_keep_both_texts() {
        let (first, second) = pair();
        let mut mine = base();
        mine.prompts
            .insert("a".into(), "Please read the task.".into());
        let mut theirs = base();
        theirs
            .prompts
            .insert("a".into(), "Read the task. Be short.".into());
        let one = first.change(&base(), &mine);
        let two = second.change(&base(), &theirs);
        first.apply_update(&two).unwrap();
        second.apply_update(&one).unwrap();
        assert_eq!(
            first.content().prompts["a"],
            "Please read the task. Be short."
        );
        assert_eq!(first.content(), second.content());
    }

    #[test]
    fn a_peer_gets_only_what_it_misses() {
        let (first, second) = pair();
        let mut changed = base();
        changed.prompts.remove("b");
        changed.pipeline["nodes"].as_array_mut().unwrap().remove(1);
        first.change(&base(), &changed);
        let missing = first.missing_for(&second.state_vector()).unwrap();
        second.apply_update(&missing).unwrap();
        assert_eq!(second.content(), changed);
    }

    #[test]
    fn a_change_on_a_changed_draft_lands_by_identity() {
        let draft = YDraftDocuments.create(&base());
        // Someone else added a node in front.
        let mut theirs = base();
        theirs.pipeline["nodes"]
            .as_array_mut()
            .unwrap()
            .insert(0, json!({"id": "z"}));
        draft.change(&base(), &theirs);
        // A change computed against the old file still finds node `b`.
        let mut file = base();
        file.pipeline["nodes"][1]["name"] = json!("B2");
        draft.change(&base(), &file);
        let nodes = draft.content().pipeline["nodes"].clone();
        assert_eq!(nodes[0]["id"], "z");
        assert_eq!(nodes[2]["name"], "B2");
    }

    #[test]
    fn a_garbled_update_is_refused() {
        assert!(YDraftDocuments
            .create(&base())
            .apply_update(&[1, 2, 3])
            .is_err());
    }
}
