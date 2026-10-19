//! The shared draft of a pipeline (ADR-0088, CONTEXT.md « Brouillon
//! partagé »): what it holds, the delta between two of its states, how many
//! changes it carries since the file, and the presence of its participants.
//!
//! The draft holds the pipeline as `GET /pipelines/{id}` serves it (JSON) and
//! the prompts of its nodes. The replicated structure that merges concurrent
//! edits field by field is an adapter (`adapters/draft_doc.rs`); the rules
//! here only read and compare JSON.

pub mod changes;
pub mod delta;
pub mod presence;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A state of the draft: the pipeline document and the prompts, by node id.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DraftContent {
    pub pipeline: Value,
    #[serde(default)]
    pub prompts: BTreeMap<String, String>,
}

impl DraftContent {
    pub fn new(pipeline: Value, prompts: impl IntoIterator<Item = (String, String)>) -> Self {
        Self {
            pipeline: whole_numbers(pipeline),
            prompts: prompts.into_iter().collect(),
        }
    }

    /// The same content, its whole floats written as integers: how the
    /// replicated document gives them back, so that a file and the draft that
    /// holds it compare equal.
    pub fn normalized(self) -> Self {
        Self {
            pipeline: whole_numbers(self.pipeline),
            prompts: self.prompts,
        }
    }

    /// How many nodes it holds.
    pub fn node_count(&self) -> u32 {
        self.pipeline["nodes"]
            .as_array()
            .map_or(0, |nodes| nodes.len() as u32)
    }
}

/// A number as the draft holds it: a whole float exactly held by a float
/// (`470.0`) is an integer (`470`), what the editor and the replicated
/// document hold; any other float stays one, a non-finite one is no value.
pub fn draft_number(f: f64) -> Value {
    const EXACT: f64 = 9_007_199_254_740_992.0; // 2^53
    if f.fract() == 0.0 && f.abs() < EXACT {
        Value::from(f as i64)
    } else {
        serde_json::Number::from_f64(f).map_or(Value::Null, Value::Number)
    }
}

/// `value` with its numbers as the draft holds them ([`draft_number`]); an
/// integer beyond `i64` is a float there.
pub fn whole_numbers(value: Value) -> Value {
    match value {
        Value::Number(n) if !n.is_i64() => n.as_f64().map_or(Value::Number(n), draft_number),
        Value::Array(items) => Value::Array(items.into_iter().map(whole_numbers).collect()),
        Value::Object(fields) => Value::Object(
            fields
                .into_iter()
                .map(|(key, field)| (key, whole_numbers(field)))
                .collect(),
        ),
        other => other,
    }
}

/// A draft as the pipeline list shows it: its changes, its nodes and who has
/// it open.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, ts_rs::TS)]
#[ts(export)]
pub struct DraftSummary {
    pub changes: u32,
    /// The nodes the draft holds (the file's count until it changes).
    pub nodes: u32,
    pub participants: Vec<presence::DraftParticipant>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn whole_floats_are_integers_the_others_stay() {
        let value = json!({
            "view": {"x": 470.0, "y": -3.0},
            "list": [0.5, -0.0, 12],
            "big": 9_007_199_254_740_992.0_f64,
            "huge": u64::MAX,
        });
        let normalized = whole_numbers(value);
        assert_eq!(normalized["view"], json!({"x": 470, "y": -3}));
        assert!(normalized["view"]["x"].is_i64());
        assert_eq!(normalized["list"], json!([0.5, 0, 12]));
        assert!(normalized["big"].is_f64());
        assert_eq!(normalized["huge"], json!(u64::MAX as f64));
    }
}
