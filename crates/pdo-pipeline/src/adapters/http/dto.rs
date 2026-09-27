//! The bodies of the pipeline routes, as the daemon has always spoken them.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::application::PipelineSummary;
use crate::domain::library::LibraryEntry;
use crate::domain::pipeline::{self, VariableDef};
use crate::domain::yaml_json::yaml_value_to_json;

#[derive(Serialize, TS)]
#[ts(export)]
pub(super) struct PipelineVariableInfo {
    var_type: pipeline::VariableType,
    #[ts(type = "unknown")]
    default: serde_json::Value,
}

impl From<&VariableDef> for PipelineVariableInfo {
    fn from(variable: &VariableDef) -> Self {
        Self {
            var_type: variable.var_type.clone(),
            default: yaml_value_to_json(&variable.default),
        }
    }
}

#[derive(Serialize, TS)]
#[ts(export)]
pub(super) struct PipelineListEntry {
    id: String,
    name: String,
    // Always "instance": the list only ever serves instance pipelines.
    #[ts(type = "\"instance\"")]
    scope: String,
    path: String,
    node_count: usize,
    modified: Option<String>,
    variables: HashMap<String, PipelineVariableInfo>,
    /// Whether a manual Run must supply a non-empty prompt. Surfaced so the New Run
    /// modal can make the prompt field optional.
    prompt_required: bool,
}

impl From<PipelineSummary> for PipelineListEntry {
    fn from(summary: PipelineSummary) -> Self {
        Self {
            id: summary.id,
            name: summary.name,
            scope: "instance".to_string(),
            path: summary.path.to_string_lossy().to_string(),
            node_count: summary.node_count,
            modified: summary.modified,
            variables: summary
                .variables
                .iter()
                .map(|(k, v)| (k.clone(), v.into()))
                .collect(),
            prompt_required: summary.prompt_required,
        }
    }
}

#[derive(Deserialize)]
pub(super) struct CreatePipelineRequest {
    pub(super) name: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub(super) scope: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct SavePipelineRequest {
    pub(super) yaml: String,
    #[serde(default)]
    pub(super) prompts: HashMap<String, String>,
}

#[derive(Deserialize)]
pub(super) struct RenamePipelineRequest {
    pub(super) name: String,
}

#[derive(Deserialize)]
pub(super) struct ImportPipelineDocumentRequest {
    pub(super) document: String,
    /// #673: the skills sidecar as PDO exported it (or as the user re-zipped the
    /// `<pipeline>.skills/` folder), base64. Absent ⇒ unknown ids import with the
    /// "skill absent" warning, never a failure.
    #[serde(default)]
    pub(super) skills_sidecar: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct SaveToLibraryRequest {
    name: String,
    #[serde(rename = "type")]
    node_type: pipeline::NodeType,
    #[serde(default)]
    inputs: Vec<pipeline::Port>,
    #[serde(default)]
    outputs: Vec<pipeline::Port>,
    #[serde(default)]
    interactive: bool,
    /// Per-node model override. Optional so a client that omits it stars the node
    /// as model-less rather than 400-ing.
    #[serde(default)]
    model: Option<String>,
    /// Per-node reasoning-effort override. Optional for the same reason.
    #[serde(default)]
    effort: Option<String>,
    #[serde(default)]
    prompt: String,
    /// Where an instance of this entry works (#655/ADR-0060). Optional so a
    /// client that omits it stars the node on its type's default (stamped by
    /// the save) instead of 400-ing — same posture as `model`.
    #[serde(default)]
    isolated_worktree: Option<bool>,
}

impl From<SaveToLibraryRequest> for LibraryEntry {
    fn from(req: SaveToLibraryRequest) -> Self {
        LibraryEntry {
            name: req.name,
            node_type: req.node_type,
            inputs: req.inputs,
            outputs: req.outputs,
            interactive: req.interactive,
            model: req.model,
            effort: req.effort,
            max_iter: None,
            branches: None,
            prompt: req.prompt,
            isolated_worktree: req.isolated_worktree,
        }
    }
}

/// `POST /nodes/parse`: the raw YAML text of a single node.
#[derive(Deserialize)]
pub(super) struct ParseNodeRequest {
    pub(super) yaml: String,
}

/// `POST /library/import`: the raw `.js` source plus the original filename
/// (used only for the fallback name and error messages). The content arrives
/// over the wire, so the daemon never reads `~/.claude/workflows` off disk —
/// it binds `0.0.0.0` and must stay host-agnostic.
#[derive(Deserialize)]
pub(super) struct ImportWorkflowRequest {
    pub(super) filename: String,
    pub(super) content: String,
}
