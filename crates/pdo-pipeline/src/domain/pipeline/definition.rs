//! The pipeline document: its variables, nodes, edges, loop regions and notes.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::edge::EdgeDef;
use super::node::{NodeDef, NodeDefWire, ViewPosition};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)]
pub enum VariableType {
    Int,
    Float,
    String,
    Bool,
    List,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct VariableDef {
    #[serde(rename = "type")]
    pub var_type: VariableType,
    #[ts(type = "unknown")]
    pub default: serde_yaml::Value,
}

/// The kind of a named loop region (ADR-0011 / #148, #151). `Bounded` loops
/// carry an iteration counter and a `max_iter`; they are born by auto-detection
/// of a cycle so no cycle is ever accidentally unbounded. `Collection` loops
/// (ex-ForEach) carry an `over: <field>` driver naming a list in the entering
/// artifact's frontmatter; they fan the member(s) out in parallel (one lap per
/// item) and their outgoing edges fire once on the barrier (all items finished).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum LoopKind {
    Bounded,
    Collection,
}

/// A named loop region (ADR-0011 / #148, #151). Replaces the `Loop` and
/// `ForEach` nodes: the loop is identified by `id`, its body is the explicit
/// `members` list (>= 1 node; a single self-looping member is valid). A
/// `bounded` region (`max_iter`) has a region-wide iteration counter keyed by
/// `id`; an `iter >= max` exit edge routes the exhaustion, otherwise it blocks
/// "exhausted — unrouted" (never a silent stall). A `collection` region (`over`)
/// fans the member(s) out in parallel, one lap per item of the named list, and
/// barriers — its outgoing edges fire once when all items finish.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct LoopRegion {
    pub id: String,
    pub kind: LoopKind,
    pub members: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional = nullable, type = "number | string | null")]
    pub max_iter: Option<serde_yaml::Value>,
    /// The **Plafond de coût** of a `bounded` region (ADR draft
    /// plafond-de-cout-de-boucle): a dollar cap on its **Coût de boucle**,
    /// checked at each re-entry. A positive number or a `$var` reference;
    /// absent = no cap (no default). Refused on a `collection` region (no
    /// re-entry to check it at). Serialized only when set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional = nullable, type = "number | string | null")]
    pub max_cost_usd: Option<serde_yaml::Value>,
    /// The frontmatter field naming the list a `collection` region fans out over
    /// (ADR-0011 / #151). `None` for a `bounded` region.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub over: Option<String>,
}

/// An inert canvas note (ADR-0018): no port, no edge, never spawned, outside the
/// DAG and the runtime. Layout, not semantics — excluded from the semantic
/// pipeline-diff.
///
/// The Rust field is MANDATORY: the frontend rehydrates from the daemon-parsed
/// `PipelineDef`, never the raw YAML, so without it serde drops the note silently
/// and it vanishes on reload.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, optional_fields = nullable, rename = "NoteDef")]
pub struct Note {
    pub id: String,
    pub content: String,
    /// Canvas position. Absent ⇒ not yet placed; reuses `ViewPosition` like
    /// `NodeDef.view`. Omitted from YAML when `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<ViewPosition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct PipelineDef {
    pub name: String,
    pub version: Option<String>,
    #[serde(default, deserialize_with = "deserialize_variables")]
    pub variables: HashMap<String, VariableDef>,
    #[serde(default)]
    #[ts(as = "Vec<NodeDefWire>")]
    pub nodes: Vec<NodeDef>,
    #[serde(default)]
    pub edges: Vec<EdgeDef>,
    /// Named bounded loop regions (ADR-0011 / #148). Absent on pipelines with no
    /// loops; round-trips when present.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional = nullable)]
    pub loops: Vec<LoopRegion>,
    /// Inert canvas notes (#307 / ADR-0018). Absent on pipelines with no notes;
    /// round-trips when present. Layout, not semantics — excluded from the
    /// pipeline-diff on the frontend but persisted verbatim here.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional = nullable)]
    pub notes: Vec<Note>,
    /// Whether a manual Run must be launched with a non-empty user prompt (#158).
    /// Defaults to `true` (the prompt is mandatory) and is omitted from YAML in
    /// that case, so prompt-required pipelines stay clean. When `false`, a Run
    /// may start with empty input and a provided prompt is treated as additional
    /// info rather than the sole source of work.
    #[serde(default = "default_prompt_required", skip_serializing_if = "is_true")]
    #[ts(optional = nullable)]
    pub prompt_required: bool,
    /// The pipeline's own wiring-grid size (#877 / ADR-0076). Absent ⇒ the
    /// pipeline follows the reader's global default, and it is omitted from YAML
    /// in that case so an untouched pipeline round-trips byte for byte. Layout,
    /// not semantics — excluded from the semantic pipeline-diff.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grid_size: Option<GridSize>,
    /// « Attente des nœuds lancés » (ADR-0086). `true` ⇒ a Run that reaches
    /// `End` completes only once no node is `running`, `waiting` or
    /// `awaiting_user`; until then it stays `Running`. `false` (the default)
    /// keeps the Run ending at `End`, and is omitted from YAML so an untouched
    /// pipeline round-trips byte for byte. Semantic, not layout.
    #[serde(default, skip_serializing_if = "is_false")]
    #[ts(optional = nullable)]
    pub wait_for_started_nodes: bool,
}

/// The three wiring-grid sizes (#877 / ADR-0076): `S` = 20px, `M` = 30px,
/// `L` = 40px. The daemon never uses the step itself — only the canvas does —
/// but it must parse the field, or serde drops it and the choice vanishes on
/// reload (the frontend rehydrates from the daemon-parsed `PipelineDef`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum GridSize {
    S,
    M,
    L,
}

fn default_prompt_required() -> bool {
    true
}

fn is_true(b: &bool) -> bool {
    *b
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn infer_variable_type(val: &serde_yaml::Value) -> VariableType {
    match val {
        serde_yaml::Value::Bool(_) => VariableType::Bool,
        serde_yaml::Value::Number(n) => {
            if n.is_f64() && !n.is_i64() && !n.is_u64() {
                VariableType::Float
            } else {
                VariableType::Int
            }
        }
        serde_yaml::Value::Sequence(_) => VariableType::List,
        _ => VariableType::String,
    }
}

fn deserialize_variables<'de, D>(deserializer: D) -> Result<HashMap<String, VariableDef>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw: HashMap<String, serde_yaml::Value> = HashMap::deserialize(deserializer)?;
    let mut result = HashMap::new();

    for (name, value) in raw {
        let is_explicit = value.as_mapping().is_some_and(|m| {
            m.contains_key(serde_yaml::Value::String("type".into()))
                && m.contains_key(serde_yaml::Value::String("default".into()))
        });
        let var_def = if is_explicit {
            serde_yaml::from_value::<VariableDef>(value).map_err(serde::de::Error::custom)?
        } else {
            VariableDef {
                var_type: infer_variable_type(&value),
                default: value,
            }
        };
        result.insert(name, var_def);
    }

    Ok(result)
}

impl PipelineDef {
    pub fn variable_defaults(&self) -> HashMap<String, serde_yaml::Value> {
        self.variables
            .iter()
            .map(|(k, v)| (k.clone(), v.default.clone()))
            .collect()
    }
}
