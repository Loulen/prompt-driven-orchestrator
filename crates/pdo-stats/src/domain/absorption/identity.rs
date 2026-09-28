//! The Run identity read, prefactored out of Sessions, Cost and Performance:
//! the `pipeline_id` → `pipeline_name` → `(unknown)` fallback and the frozen
//! `node_defs`, read in ONE place, where the resolver substitutes an absorbed
//! key with its absorbent's.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use serde_json::Value;

use super::resolver::{CoupleScope, GlobalAbsorptions, ScopeAbsorptions};
use super::{couple_key, couple_scope, split_couple_key, UNKNOWN_PIPELINE};

/// A model × effort couple of a Node as its row counts it (#906): the four
/// resolution steps applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoupleIdentity {
    /// The couple the row is keyed by, after step 4.
    pub model: String,
    pub effort: Option<String>,
    pub key: String,
    /// The couple after the global steps (1–3) — the local members' tallies.
    pub global_key: String,
    /// The couple the event log wrote — the global members' tallies.
    pub raw_key: String,
    /// Whether a global absorption (models, efforts) moved it.
    pub global: bool,
    /// The raw `(Pipeline, Node)` scope its couple absorptions live in; empty
    /// on a row that is no Node (Infrastructure, Unassigned).
    pub scope: String,
}

// --- The Run identity read (prefactored out of Sessions, Cost, Performance) ----

/// The Pipeline key and name of a Run as the event log froze them: the
/// `pipeline_id` going forward (#377), else the always-present `pipeline_name`,
/// else `(unknown)`.
pub fn pipeline_identity(payload: &Value) -> (String, String) {
    let key = payload
        .get("pipeline_id")
        .and_then(|v| v.as_str())
        .or_else(|| payload.get("pipeline_name").and_then(|v| v.as_str()))
        .unwrap_or(UNKNOWN_PIPELINE)
        .to_string();
    let name = payload
        .get("pipeline_name")
        .and_then(|v| v.as_str())
        .unwrap_or(&key)
        .to_string();
    (key, name)
}

/// One node of the Run's frozen pipeline snapshot, as Sessions and Cost read it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NodeDef {
    pub name: String,
    pub node_type: String,
}

pub fn node_defs(payload: &Value) -> BTreeMap<String, NodeDef> {
    payload
        .get("node_defs")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|def| {
            let id = def.get("id")?.as_str()?;
            Some((
                id.to_string(),
                NodeDef {
                    name: def
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or(id)
                        .to_string(),
                    node_type: def
                        .get("node_type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                },
            ))
        })
        .collect()
}

/// A Run's identity for the Stats folds, absorptions applied.
#[derive(Debug, Clone)]
pub struct RunIdentity {
    /// The key the Run is counted under — its absorbent's when it is absorbed.
    pub pipeline_key: String,
    /// The name to show for that key: the Run's own name, or, for an absorbed
    /// Run, the absorbent's stored name.
    pub pipeline_name: String,
    /// Whether the name is the Run's own. A row's name is its most recent own
    /// Run's name; an absorbed Run only names the row when the absorbent has no
    /// Run of its own in the period.
    pub own_name: bool,
    /// The key the event log wrote, before substitution — the member tallies.
    pub raw_pipeline_key: String,
    pub node_defs: BTreeMap<String, NodeDef>,
    /// The Node absorptions of the Pipeline row this Run is counted under.
    pub nodes: Arc<ScopeAbsorptions>,
    /// The Model and effort absorptions (steps 1–2 of a couple).
    pub global: GlobalAbsorptions,
    /// The couple absorptions, by raw `(Pipeline, Node)` scope.
    pub couples: Arc<HashMap<String, CoupleScope>>,
    /// The Run itself, for the tallies of a fold that only sees the identity
    /// (Performance's « By model » axis) — empty unless [`Self::for_run`] set it.
    pub run_id: String,
    pub started_at: String,
}

/// A Node of a Run as a Stats row counts it, Node absorptions applied.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NodeIdentity {
    /// The Node id the row is keyed by — its absorbent's when it is absorbed.
    pub key: String,
    /// The Node id the event log wrote — the member tallies.
    pub raw_key: String,
    pub name: String,
    /// Same naming rule as a Pipeline row: an absorbed Node only names the row
    /// when the absorbent Node has no execution of its own.
    pub own_name: bool,
}

impl NodeIdentity {
    /// Apply the naming rule to a Node row's name slot (latest own wins).
    pub fn adopt_name(&self, slot: &mut String) {
        if self.own_name || slot.is_empty() {
            *slot = self.name.clone();
        }
    }
}

impl RunIdentity {
    pub fn for_run(mut self, run_id: &str, started_at: &str) -> Self {
        self.run_id = run_id.to_string();
        self.started_at = started_at.to_string();
        self
    }

    /// Apply the naming rule to a row's name slot (folds walk Runs oldest first,
    /// so « latest own Run wins » is a plain overwrite).
    pub fn adopt_name(&self, slot: &mut String) {
        if self.own_name || slot.is_empty() {
            *slot = self.pipeline_name.clone();
        }
    }

    pub fn node_name(&self, node_id: &str) -> String {
        self.node_defs
            .get(node_id)
            .map(|def| def.name.clone())
            .unwrap_or_else(|| node_id.to_string())
    }

    /// Node `raw` of this Run, as the Pipeline row counts it.
    pub fn node(&self, raw: &str) -> NodeIdentity {
        let key = self.nodes.resolve(raw).to_string();
        let own_name = key == raw;
        let name = if own_name {
            self.node_name(raw)
        } else {
            self.nodes
                .stored_name(&key)
                .map(str::to_string)
                .unwrap_or_else(|| self.node_name(&key))
        };
        NodeIdentity {
            key,
            raw_key: raw.to_string(),
            name,
            own_name,
        }
    }

    /// The model id model `raw` is counted under (step 2).
    pub fn model(&self, raw: &str) -> String {
        self.global.models.resolve(raw).to_string()
    }

    /// The effort the « By model » axis counts `raw_effort` of raw model
    /// `raw_model` under (step 1, on the raw model — ADR-0078).
    pub fn effort(&self, raw_model: &str, raw_effort: Option<&str>) -> Option<String> {
        self.global.effort(raw_model, raw_effort)
    }

    /// The couple `raw_model` × `raw_effort` of Node `raw_node` of this Run is
    /// counted as (steps 1–4). `raw_node` is `None` on a row that is no Node:
    /// only the global steps apply there.
    pub fn couple(
        &self,
        raw_node: Option<&str>,
        raw_model: &str,
        raw_effort: Option<&str>,
    ) -> CoupleIdentity {
        let raw_key = couple_key(raw_model, raw_effort);
        let (model, effort) = self.global.couple(raw_model, raw_effort);
        let global_key = couple_key(&model, effort.as_deref());
        let scope = raw_node
            .map(|node| couple_scope(&self.raw_pipeline_key, node))
            .unwrap_or_default();
        let local = self
            .couples
            .get(&scope)
            .and_then(|couples| couples.to.get(&global_key))
            .and_then(|key| split_couple_key(key).map(|split| (key.clone(), split)));
        let (key, model, effort) = match local {
            Some((key, (model, effort))) => (key, model, effort),
            None => (global_key.clone(), model, effort),
        };
        CoupleIdentity {
            model,
            effort,
            key,
            global: global_key != raw_key,
            global_key,
            raw_key,
            scope,
        }
    }
}

// --- What a row says about its members -------------------------------------------
