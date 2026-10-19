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
            pipeline,
            prompts: prompts.into_iter().collect(),
        }
    }
}

/// A draft as the pipeline list shows it: its changes and who has it open.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, ts_rs::TS)]
#[ts(export)]
pub struct DraftSummary {
    pub changes: u32,
    pub participants: Vec<presence::DraftParticipant>,
}
