use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Severity {
    Warning,
    #[allow(dead_code)]
    Error,
}

fn deserialize_optional_non_blank<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<String>::deserialize(deserializer)?;
    Ok(value.filter(|text| !text.trim().is_empty()))
}

#[derive(Debug, Clone)]
pub(crate) struct Diagnostic {
    #[allow(dead_code)]
    pub severity: Severity,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum NodeType {
    DocOnly,
    CodeMutating,
    Start,
    End,
    Switch,
    Loop,
    Merge,
    /// Runs author-written bash deterministically instead of launching an agent
    /// (ADR-0017): a tmux session whose tail is `timeout N bash <body>` — exit 0 ⇒
    /// completes, non-zero/timeout ⇒ fails. Doc-only-effect: no sub-worktree, it
    /// runs in the run's shared worktree.
    Script,
}

impl NodeType {
    /// A *regular* node declares no inputs: they are emergent, derived from
    /// incoming edges and named after the edge target port (ADR-0011). Structural
    /// nodes (start/end/switch/loop/merge) keep their declared input ports.
    pub(crate) fn has_emergent_inputs(&self) -> bool {
        matches!(
            self,
            NodeType::DocOnly | NodeType::CodeMutating | NodeType::Script
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct FrontmatterFieldDecl {
    #[serde(rename = "type")]
    pub field_type: String,
    #[serde(default)]
    pub allowed: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PortType {
    #[default]
    Markdown,
    Image,
    ImageList,
    /// A rendered, static HTML artifact (`output.html`): a leaf review surface
    /// shown in a sandboxed iframe (no JS), never consumed downstream (ADR-0028).
    Html,
}

pub(crate) const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "gif"];

pub(crate) fn is_image_file(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| IMAGE_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum PortSide {
    Left,
    Right,
    Top,
    Bottom,
}

impl std::fmt::Display for PortSide {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PortSide::Left => write!(f, "left"),
            PortSide::Right => write!(f, "right"),
            PortSide::Top => write!(f, "top"),
            PortSide::Bottom => write!(f, "bottom"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct Port {
    pub name: String,
    #[serde(default)]
    pub repeated: bool,
    #[serde(default)]
    pub side: Option<PortSide>,
    #[serde(default)]
    pub port_type: PortType,
    #[serde(default)]
    pub frontmatter: Option<HashMap<String, FrontmatterFieldDecl>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<serde_yaml::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_non_blank",
        skip_serializing_if = "Option::is_none"
    )]
    pub instructions: Option<String>,
    /// Marks an **input** port whose artifact must arrive for the node to run
    /// (ADR-0011). When every edge feeding it is structurally dead, the
    /// reachability sweep auto-skips the node rather than hanging on an input that
    /// can never come. Absent leaves the node subject only to the whole-node
    /// deadness rule. Semantic, not layout.
    #[serde(default, skip_serializing_if = "is_false")]
    pub required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ViewPosition {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct NodeDef {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub node_type: NodeType,
    #[serde(default)]
    pub inputs: Vec<Port>,
    #[serde(default)]
    pub outputs: Vec<Port>,
    #[serde(default)]
    pub interactive: bool,
    pub view: Option<ViewPosition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_iter: Option<serde_yaml::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub over: Option<String>,
    /// The harness this node **requires** (ADR-0046). A pin both selects the
    /// harness and shields it from every coarser tier (Run / Projet / instance);
    /// absent ⇒ follow the tier above. Free-text pass-through, no closed enum
    /// (ADR-0001). Semantic, not layout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pin_harness: Option<String>,
    /// Per-harness settings (ADR-0046): `harnesses.<name> = {model, effort}`. Model
    /// and effort are NOT precedence axes of their own — they are read from the
    /// *winning* harness's entry, so the node stays executable on every harness
    /// instead of being launched with a slug the other one rejects. Replaces the
    /// legacy flat `model:`/`effort:`, which the migrator folds into
    /// `harnesses.claude.*`.
    ///
    /// `BTreeMap` for deterministic serialization. Semantic, not layout — included
    /// in the pipeline diff and the library content hash.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub harnesses: BTreeMap<String, crate::harness_resolver::HarnessEntry>,
    /// The node's tier of the agentic-profile union (ADR-0057). `Some(Profile |
    /// Custom)` supplies harness, model and effort atomically, and
    /// [`pin_harness`]/[`harnesses`] are then NOT consulted (ADR-0057 ¶1: Custom
    /// "ne réactive pas l'ancien résolveur"). Absent or `Some(Inherit)` preserves
    /// the legacy behaviour exactly. Semantic, not layout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_choice: Option<crate::agent_choice::AgentChoice>,
    /// The **finest** tier of [`crate::auto_fail::resolve_auto_fail`] (`node → Run
    /// → Projet → instance`): `Some(_)` overrides every coarser tier for this
    /// node's `pdo fail`. Semantic, not layout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_fail: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct EdgeEndpoint {
    pub node: String,
    pub port: String,
}

/// Edge routing mode (#154). `Auto` edges store no waypoints — their
/// right-angle path is recomputed deterministically and re-routes on node move.
/// `Manual` edges pin the route to persisted `waypoints`. Routing is *layout*,
/// not semantics: it persists in the pipeline file (so a shared workflow keeps
/// its arrows) but is excluded from the semantic pipeline-diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EdgeRouteMode {
    Auto,
    Manual,
}

/// A pinned waypoint on a manually-routed edge — absolute canvas coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub(crate) struct EdgeWaypoint {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct EdgeDef {
    pub source: EdgeEndpoint,
    pub target: EdgeEndpoint,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Optional `when:` clause: a mechanical predicate (ADR-0002 grammar)
    /// evaluated against the source node's frontmatter, `iter`, and pipeline
    /// variables. When present, the edge fires only if the clause is satisfied.
    /// Conditional routing lives on the edge (ADR-0011).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<serde_yaml::Value>,
    /// `else: true` marks a fallback edge that fires iff no sibling edge on the
    /// same source port matched (ADR-0011). Mutually exclusive with `when:`.
    #[serde(default, rename = "else", skip_serializing_if = "is_false")]
    pub is_else: bool,
    /// `repeated: true` marks an edge whose source artifact accumulates across
    /// iterations: the resolver globs `iter-*` and pools every match into the
    /// emergent input. Loop accumulation ("read all laps") lives on the edge,
    /// not on a declared input port (ADR-0011 / #149).
    #[serde(default, skip_serializing_if = "is_false")]
    pub repeated: bool,
    /// Routing mode (#154). Absent ⇒ auto (recomputed, never persisted).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<EdgeRouteMode>,
    /// Pinned absolute waypoints (#154). Only meaningful when `mode == Manual`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waypoints: Option<Vec<EdgeWaypoint>>,
    /// The target card side this incoming edge anchors on (#168). Like
    /// `mode`/`waypoints` this is *layout*, not semantics: it persists so a
    /// shared workflow keeps its arrow arrival sides, but is excluded from the
    /// semantic pipeline-diff. Absent ⇒ left (legacy anchoring), never written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_side: Option<PortSide>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)]
pub(crate) enum VariableType {
    Int,
    Float,
    String,
    Bool,
    List,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct VariableDef {
    #[serde(rename = "type")]
    pub var_type: VariableType,
    pub default: serde_yaml::Value,
}

/// The kind of a named loop region (ADR-0011 / #148, #151). `Bounded` loops
/// carry an iteration counter and a `max_iter`; they are born by auto-detection
/// of a cycle so no cycle is ever accidentally unbounded. `Collection` loops
/// (ex-ForEach) carry an `over: <field>` driver naming a list in the entering
/// artifact's frontmatter; they fan the member(s) out in parallel (one lap per
/// item) and their outgoing edges fire once on the barrier (all items finished).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LoopKind {
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct LoopRegion {
    pub id: String,
    pub kind: LoopKind,
    pub members: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_iter: Option<serde_yaml::Value>,
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Note {
    pub id: String,
    pub content: String,
    /// Canvas position. Absent ⇒ not yet placed; reuses `ViewPosition` like
    /// `NodeDef.view`. Omitted from YAML when `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<ViewPosition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PipelineDef {
    pub name: String,
    pub version: Option<String>,
    #[serde(default, deserialize_with = "deserialize_variables")]
    pub variables: HashMap<String, VariableDef>,
    #[serde(default)]
    pub nodes: Vec<NodeDef>,
    #[serde(default)]
    pub edges: Vec<EdgeDef>,
    /// Named bounded loop regions (ADR-0011 / #148). Absent on pipelines with no
    /// loops; round-trips when present.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub loops: Vec<LoopRegion>,
    /// Inert canvas notes (#307 / ADR-0018). Absent on pipelines with no notes;
    /// round-trips when present. Layout, not semantics — excluded from the
    /// pipeline-diff on the frontend but persisted verbatim here.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<Note>,
    /// Whether a manual Run must be launched with a non-empty user prompt (#158).
    /// Defaults to `true` (the prompt is mandatory) and is omitted from YAML in
    /// that case, so prompt-required pipelines stay clean. When `false`, a Run
    /// may start with empty input and a provided prompt is treated as additional
    /// info rather than the sole source of work.
    #[serde(default = "default_prompt_required", skip_serializing_if = "is_true")]
    pub prompt_required: bool,
}

fn default_prompt_required() -> bool {
    true
}

fn is_true(b: &bool) -> bool {
    *b
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
    pub(crate) fn variable_defaults(&self) -> HashMap<String, serde_yaml::Value> {
        self.variables
            .iter()
            .map(|(k, v)| (k.clone(), v.default.clone()))
            .collect()
    }
}

#[derive(Debug)]
pub(crate) struct ParseResult {
    pub pipeline: PipelineDef,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ParseError {
    #[error("invalid YAML: {0}")]
    InvalidYaml(#[from] serde_yaml::Error),
    #[error("missing required field: {0}")]
    MissingField(String),
    #[error("switch node '{node_id}' output '{port}': when-clause field '{field}' not found in upstream schema")]
    UndeclaredWhenField {
        node_id: String,
        port: String,
        field: String,
    },
}

/// Top-level YAML keys accepted by the unknown-field lint.
/// MUST list every serializable field of PipelineDef — see `known_keys_cover_serialized_pipeline`.
const KNOWN_TOP_LEVEL_KEYS: &[&str] = &[
    "name",
    "version",
    "variables",
    "nodes",
    "edges",
    "loops",
    "notes",
    "prompt_required",
];

/// The node `type` strings the parser accepts verbatim. Anything else is coerced
/// to `doc-only` with a warning (see `normalize_node_value`); `for-each`/`foreach`
/// are refused outright (retired in ADR-0011).
pub(crate) const VALID_NODE_TYPES: &[&str] = &[
    "doc-only",
    "code-mutating",
    "start",
    "end",
    "switch",
    "loop",
    "merge",
    "script",
];

/// Normalize one node's raw YAML `type` field in place, returning any warnings.
/// This is the per-node pass `parse_pipeline` runs over each element of the
/// `nodes:` sequence, factored out so the single-node `POST /nodes/parse`
/// endpoint (#345) shares exactly the same coercion rules — one source of truth
/// for node-type handling:
///
/// - `for-each` / `foreach` → hard `Err` (the type was removed in ADR-0011;
///   coercing a fan-out to `doc-only` would run each item's work zero times).
/// - missing `type` → default to `doc-only` (noisy warning).
/// - unknown `type` → coerce to `doc-only` (noisy warning).
/// - a known type (incl. legacy `switch`/`loop`) → left untouched, no warning.
///
/// A non-mapping value (e.g. a stray scalar in the `nodes:` list) is a no-op.
pub(crate) fn normalize_node_value(
    node_val: &mut serde_yaml::Value,
) -> Result<Vec<Diagnostic>, ParseError> {
    let mut diagnostics = Vec::new();
    let Some(node_map) = node_val.as_mapping_mut() else {
        return Ok(diagnostics);
    };
    let type_key = serde_yaml::Value::String("type".into());
    let node_id = node_map
        .get(serde_yaml::Value::String("id".into()))
        .and_then(|v| v.as_str())
        .unwrap_or("<unknown>")
        .to_string();

    match node_map.get(&type_key).and_then(|v| v.as_str()) {
        // ForEach is RETIRED (ADR-0011): a hard refusal, not the warn+coerce
        // below — rewriting a fan-out node to doc-only would run each item's work
        // zero times.
        Some(t @ ("for-each" | "foreach")) => {
            return Err(ParseError::MissingField(format!(
                "node '{node_id}': node type '{t}' was removed (ADR-0011) — \
                 run `pdo migrate` to convert it to a collection loop region"
            )));
        }
        None => {
            diagnostics.push(Diagnostic {
                severity: Severity::Warning,
                message: format!("node '{node_id}': missing 'type', defaulting to 'doc-only'"),
            });
            node_map.insert(type_key, serde_yaml::Value::String("doc-only".into()));
        }
        Some(t) if !VALID_NODE_TYPES.contains(&t) => {
            diagnostics.push(Diagnostic {
                severity: Severity::Warning,
                message: format!(
                    "node '{node_id}': unknown node type '{t}', defaulting to 'doc-only'"
                ),
            });
            node_map.insert(type_key, serde_yaml::Value::String("doc-only".into()));
        }
        _ => {}
    }

    // Fold a legacy flat `model:` / `effort:` into `harnesses.claude.*` so a parse
    // is lossless even before the on-disk migrator has rewritten the file, keeping
    // `harnesses` the single source the resolver reads.
    fold_flat_model_effort_into_harnesses(node_map);

    Ok(diagnostics)
}

/// Fold a node's legacy flat `model:` / `effort:` keys into
/// `harnesses.claude.{model, effort}` (ADR-0046), removing the flat keys.
///
/// Shared by [`normalize_node_value`] and the pipeline migrator so both fold
/// identically. Returns `true` iff the raw YAML changed — the migrator's
/// `needs_migration` guard keys off it.
///
/// An explicit `harnesses.claude` entry is authoritative: a flat value fills a slot
/// only when the map has none, so a second run is a no-op.
pub(crate) fn fold_flat_model_effort_into_harnesses(node_map: &mut serde_yaml::Mapping) -> bool {
    let removed_model = node_map.remove(serde_yaml::Value::String("model".into()));
    let removed_effort = node_map.remove(serde_yaml::Value::String("effort".into()));
    let changed = removed_model.is_some() || removed_effort.is_some();

    let flat_model = removed_model
        .as_ref()
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let flat_effort = removed_effort
        .as_ref()
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    if flat_model.is_none() && flat_effort.is_none() {
        return changed; // the flat keys were absent or empty — nothing to carry
    }

    let harnesses_key = serde_yaml::Value::String("harnesses".into());
    if !node_map
        .get(&harnesses_key)
        .map(serde_yaml::Value::is_mapping)
        .unwrap_or(false)
    {
        node_map.insert(
            harnesses_key.clone(),
            serde_yaml::Value::Mapping(serde_yaml::Mapping::new()),
        );
    }
    let harnesses = node_map
        .get_mut(&harnesses_key)
        .and_then(serde_yaml::Value::as_mapping_mut)
        .expect("harnesses just ensured to be a mapping");

    let claude_key = serde_yaml::Value::String(crate::harness_registry::CLAUDE.into());
    if !harnesses
        .get(&claude_key)
        .map(serde_yaml::Value::is_mapping)
        .unwrap_or(false)
    {
        harnesses.insert(
            claude_key.clone(),
            serde_yaml::Value::Mapping(serde_yaml::Mapping::new()),
        );
    }
    let claude = harnesses
        .get_mut(&claude_key)
        .and_then(serde_yaml::Value::as_mapping_mut)
        .expect("harnesses.claude just ensured to be a mapping");

    if let Some(m) = flat_model {
        claude
            .entry(serde_yaml::Value::String("model".into()))
            .or_insert(serde_yaml::Value::String(m));
    }
    if let Some(e) = flat_effort {
        claude
            .entry(serde_yaml::Value::String("effort".into()))
            .or_insert(serde_yaml::Value::String(e));
    }
    changed
}

pub(crate) fn parse_pipeline(yaml: &str) -> Result<ParseResult, ParseError> {
    let mut raw: serde_yaml::Value = serde_yaml::from_str(yaml)?;

    if raw
        .as_mapping()
        .ok_or_else(|| ParseError::MissingField("root must be a mapping".into()))?
        .get(serde_yaml::Value::String("name".into()))
        .is_none()
    {
        return Err(ParseError::MissingField("name".into()));
    }

    let mut diagnostics = Vec::new();

    if let Some(nodes) = raw
        .as_mapping_mut()
        .and_then(|m| m.get_mut(serde_yaml::Value::String("nodes".into())))
        .and_then(|v| v.as_sequence_mut())
    {
        for node_val in nodes.iter_mut() {
            diagnostics.extend(normalize_node_value(node_val)?);
        }
    }

    let mut pipeline: PipelineDef = serde_yaml::from_value(raw.clone())?;

    for node in &mut pipeline.nodes {
        for port in &mut node.inputs {
            if port.side.is_none() {
                port.side = Some(PortSide::Left);
            }
        }
        for port in &mut node.outputs {
            if port.side.is_none() {
                port.side = Some(PortSide::Right);
            }
        }
    }

    for node in &mut pipeline.nodes {
        match node.node_type {
            NodeType::Switch => {
                if node.inputs.len() != 1 || node.inputs[0].name != "in" {
                    return Err(ParseError::MissingField(format!(
                        "switch node '{}' must have exactly one input named 'in'",
                        node.id
                    )));
                }
                let has_default = node.outputs.iter().any(|p| p.name == "default");
                if !has_default {
                    node.outputs.push(Port {
                        name: "default".into(),
                        repeated: false,
                        side: Some(PortSide::Right),
                        port_type: PortType::Markdown,
                        frontmatter: None,
                        when: None,
                        description: None,
                        instructions: None,
                        required: false,
                    });
                }
            }
            NodeType::Merge => {
                if node.inputs.len() != 1
                    || node.inputs[0].name != "branches"
                    || !node.inputs[0].repeated
                {
                    return Err(ParseError::MissingField(format!(
                        "merge node '{}' must have exactly one input named 'branches' with repeated: true",
                        node.id
                    )));
                }
                if node.outputs.len() != 1 || node.outputs[0].name != "merged" {
                    return Err(ParseError::MissingField(format!(
                        "merge node '{}' must have exactly one output named 'merged'",
                        node.id
                    )));
                }
            }
            NodeType::Loop => {
                if node.max_iter.is_none() {
                    return Err(ParseError::MissingField(format!(
                        "loop node '{}' must declare 'max_iter'",
                        node.id
                    )));
                }
                let expected_inputs = ["in", "break"];
                let expected_outputs = ["body", "done"];
                let input_names: Vec<&str> = node.inputs.iter().map(|p| p.name.as_str()).collect();
                let output_names: Vec<&str> =
                    node.outputs.iter().map(|p| p.name.as_str()).collect();
                for name in &expected_inputs {
                    if !input_names.contains(name) {
                        return Err(ParseError::MissingField(format!(
                            "loop node '{}' must have input '{}'",
                            node.id, name
                        )));
                    }
                }
                for name in &expected_outputs {
                    if !output_names.contains(name) {
                        return Err(ParseError::MissingField(format!(
                            "loop node '{}' must have output '{}'",
                            node.id, name
                        )));
                    }
                }
            }
            _ => {}
        }
    }

    for node in &pipeline.nodes {
        if !matches!(node.node_type, NodeType::DocOnly | NodeType::CodeMutating) {
            for port in node
                .outputs
                .iter()
                .filter(|port| port.instructions.is_some())
            {
                diagnostics.push(Diagnostic {
                    severity: Severity::Warning,
                    message: format!(
                        "deterministic node '{}' preserves instructions on output '{}' but does not inject them because no agent produces that output",
                        node.id, port.name
                    ),
                });
            }
        }
    }

    if let Some(mapping) = raw.as_mapping() {
        for key in mapping.keys() {
            if let Some(k) = key.as_str() {
                if !KNOWN_TOP_LEVEL_KEYS.contains(&k) {
                    diagnostics.push(Diagnostic {
                        severity: Severity::Warning,
                        message: format!("unknown field '{k}' (ignored)"),
                    });
                }
            }
        }
    }

    // Warnings here, refusals at run launch: see `dangling_edge_references`.
    for message in dangling_edge_references(&pipeline) {
        diagnostics.push(Diagnostic {
            severity: Severity::Warning,
            message,
        });
    }

    let start_nodes: Vec<&NodeDef> = pipeline
        .nodes
        .iter()
        .filter(|n| n.node_type == NodeType::Start)
        .collect();
    let end_nodes: Vec<&NodeDef> = pipeline
        .nodes
        .iter()
        .filter(|n| n.node_type == NodeType::End)
        .collect();

    if start_nodes.len() != 1 {
        return Err(ParseError::MissingField(format!(
            "pipeline must have exactly one start node, found {}",
            start_nodes.len()
        )));
    }
    if end_nodes.len() != 1 {
        return Err(ParseError::MissingField(format!(
            "pipeline must have exactly one end node, found {}",
            end_nodes.len()
        )));
    }

    let start = start_nodes[0];
    if !start.inputs.is_empty() {
        return Err(ParseError::MissingField(
            "start node must have zero inputs".into(),
        ));
    }
    if start.outputs.len() != 1 || start.outputs[0].name != "user_prompt" {
        return Err(ParseError::MissingField(
            "start node must have exactly one output port named 'user_prompt'".into(),
        ));
    }

    let end = end_nodes[0];
    if !end.outputs.is_empty() {
        return Err(ParseError::MissingField(
            "end node must have zero outputs".into(),
        ));
    }
    if end.inputs.len() != 1 || end.inputs[0].name != "result" {
        return Err(ParseError::MissingField(
            "end node must have exactly one input port named 'result'".into(),
        ));
    }

    validate_switch_when_clauses(&pipeline)?;

    // Auto-materialize bounded regions here, at the model boundary rather than in
    // the editor's `addEdge` (ADR-0011 clause (b)): every reader funnels through
    // this function, so a cycle that arrived any other way (hand-written file,
    // imported workflow, a mere reopen) still gets a lap counter and an `Exhausted`
    // halt instead of running unbounded through the generic forward-spawn path.
    //
    // Derived, never written: the file on disk stays untouched, and the id is a
    // pure function of the member set, so repeated parses agree and the region's
    // lap counter keeps its identity.
    pipeline
        .loops
        .extend(crate::loop_region::materialize_missing_regions(&pipeline));

    // Legacy control nodes are the carve-out above (a `type: loop` node governs its
    // own laps, so no region is materialized over it). Warn rather than leave the
    // canvas silently unable to render the loop.
    let legacy: Vec<String> = pipeline
        .nodes
        .iter()
        .filter(|n| matches!(n.node_type, NodeType::Loop | NodeType::Switch))
        .map(|n| {
            let kind = if n.node_type == NodeType::Loop {
                "loop"
            } else {
                "switch"
            };
            format!("{kind} '{}'", n.id)
        })
        .collect();
    if !legacy.is_empty() {
        // Only mention `max_iter` when a loop node exists: a `switch`-only pipeline
        // has no bound to edit, and naming a control the user cannot find misleads.
        let has_loop = pipeline.nodes.iter().any(|n| n.node_type == NodeType::Loop);
        diagnostics.push(Diagnostic {
            severity: Severity::Warning,
            message: format!(
                "legacy control node(s) {} were removed from the model (ADR-0011) and \
                 the canvas cannot represent them — run `pdo migrate` to convert them \
                 into conditional edges and a `loops:` region{}",
                legacy.join(", "),
                if has_loop {
                    "; until then this loop has no region on the canvas and its `max_iter` is not editable"
                } else {
                    ""
                }
            ),
        });
    }

    Ok(ParseResult {
        pipeline,
        diagnostics,
    })
}

/// Dangling references: an edge endpoint naming a missing node, a port not
/// declared on its node, or a loop-region member naming a missing node. Emergent
/// inputs are exempt — an edge target port on a *regular* node names the emergent
/// input and is valid by construction (ADR-0011).
///
/// Warnings at edit time (ADR-0001 sharp tool: the editor never blocks), refusals
/// at run launch — a run over a dangling reference is guaranteed to stall silently
/// mid-run, so rejecting it is a runtime-coherence invariant, not prescriptive
/// validation.
pub(crate) fn dangling_edge_references(pipeline: &PipelineDef) -> Vec<String> {
    let node_ids: HashSet<&str> = pipeline.nodes.iter().map(|n| n.id.as_str()).collect();

    let check = |edge: &EdgeDef,
                 endpoint: &EdgeEndpoint,
                 role: &str,
                 get_ports: fn(&NodeDef) -> &[Port]|
     -> Option<String> {
        let edge_label = format!(
            "edge '{}.{} -> {}.{}'",
            edge.source.node, edge.source.port, edge.target.node, edge.target.port
        );
        if !node_ids.contains(endpoint.node.as_str()) {
            return Some(format!(
                "{edge_label}: {role} references non-existent node '{}'",
                endpoint.node
            ));
        }
        let node = pipeline
            .nodes
            .iter()
            .find(|n| n.id == endpoint.node)
            .unwrap();
        if role == "target" && node.node_type.has_emergent_inputs() {
            return None;
        }
        if !get_ports(node).iter().any(|p| p.name == endpoint.port) {
            return Some(format!(
                "{edge_label}: {role} port '{}' not found on node '{}'",
                endpoint.port, endpoint.node
            ));
        }
        None
    };

    let mut errors = Vec::new();
    for edge in &pipeline.edges {
        if let Some(e) = check(edge, &edge.source, "source", |n| &n.outputs) {
            errors.push(e);
        }
        if let Some(e) = check(edge, &edge.target, "target", |n| &n.inputs) {
            errors.push(e);
        }
    }

    // A region member naming a missing node is the same class of dangling
    // reference: the region silently no-ops at runtime, its gates matching no node.
    for region in &pipeline.loops {
        for member in &region.members {
            if !node_ids.contains(member.as_str()) {
                errors.push(format!(
                    "loop region '{}': member references non-existent node '{}'",
                    region.id, member
                ));
            }
        }
    }
    errors
}

fn validate_switch_when_clauses(pipeline: &PipelineDef) -> Result<(), ParseError> {
    let variable_names: HashSet<&str> = pipeline.variables.keys().map(|k| k.as_str()).collect();

    for node in &pipeline.nodes {
        if node.node_type != NodeType::Switch {
            continue;
        }
        let upstream_schema = resolve_switch_upstream_schema(pipeline, &node.id);
        for port in &node.outputs {
            if port.name == "default" {
                continue;
            }
            let when = match &port.when {
                Some(w) => w,
                None => continue,
            };
            let mapping = match when.as_mapping() {
                Some(m) => m,
                None => continue,
            };
            for (key, _) in mapping {
                let field_name = match key.as_str() {
                    Some(s) => s,
                    None => continue,
                };
                if let Some(var_name) = field_name.strip_prefix('$') {
                    if !variable_names.contains(var_name) {
                        return Err(ParseError::UndeclaredWhenField {
                            node_id: node.id.clone(),
                            port: port.name.clone(),
                            field: field_name.to_string(),
                        });
                    }
                    continue;
                }
                match &upstream_schema {
                    Some(schema) if schema.contains_key(field_name) => {}
                    _ => {
                        return Err(ParseError::UndeclaredWhenField {
                            node_id: node.id.clone(),
                            port: port.name.clone(),
                            field: field_name.to_string(),
                        });
                    }
                }
            }
        }
    }
    Ok(())
}

/// The frontmatter schema declared on the upstream output port feeding a switch's
/// `in` port. `None` when the node isn't a switch, nothing connects to `in`, or the
/// upstream port declares no schema.
pub(crate) fn resolve_switch_upstream_schema(
    pipeline: &PipelineDef,
    switch_node_id: &str,
) -> Option<HashMap<String, FrontmatterFieldDecl>> {
    let node = pipeline.nodes.iter().find(|n| n.id == switch_node_id)?;
    if node.node_type != NodeType::Switch {
        return None;
    }
    let edge = pipeline
        .edges
        .iter()
        .find(|e| e.target.node == switch_node_id && e.target.port == "in")?;
    let source_node = pipeline.nodes.iter().find(|n| n.id == edge.source.node)?;
    let source_port = source_node
        .outputs
        .iter()
        .find(|p| p.name == edge.source.port)?;
    source_port.frontmatter.clone()
}

/// Split a prompt map into the entries whose key is a live node and the ids of
/// those that are not.
///
/// The sidecar prompt dir is keyed by node id and outlives the nodes it was
/// written for: delete a node and its `.md` stays on disk. Every consumer that
/// validates a pipeline against its prompts — the portable-document importer
/// above all — requires *keys ⊆ nodes*, so apply this wherever prompts cross a
/// boundary and that invariant holds by construction rather than by luck.
pub(crate) fn split_live_prompts(
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

pub(crate) fn canonical_prompt_path(pipeline_path: &Path, node_id: &str) -> std::path::PathBuf {
    let dir = pipeline_path.parent().unwrap_or(std::path::Path::new("."));
    let stem = pipeline_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("pipeline");
    dir.join(format!("{stem}.prompts"))
        .join(format!("{node_id}.md"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    const START_END_NODES: &str = r#"
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: end
    name: End
    type: end
    inputs:
      - name: result"#;

    fn with_start_end(yaml: &str) -> String {
        if yaml.contains("type: start") && yaml.contains("type: end") {
            return yaml.to_string();
        }
        let replacement = format!("nodes:{START_END_NODES}");
        if yaml.contains("nodes: []") {
            yaml.replacen("nodes: []", &replacement, 1)
        } else {
            yaml.replacen("nodes:", &replacement, 1)
        }
    }

    const VALID_MINIMAL: &str = r#"
name: test-pipeline
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: end
    name: End
    type: end
    inputs:
      - name: result
  - id: ab12cd34
    name: planner
    type: doc-only
    inputs:
      - name: task
    outputs:
      - name: plan
"#;

    #[test]
    fn parses_valid_minimal_pipeline() {
        let result = parse_pipeline(VALID_MINIMAL).unwrap();
        assert_eq!(result.pipeline.name, "test-pipeline");
        assert_eq!(result.pipeline.version.as_deref(), Some("1.0"));
        assert_eq!(result.pipeline.nodes.len(), 3);

        let node = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab12cd34")
            .unwrap();
        assert_eq!(node.name, "planner");
        assert_eq!(node.node_type, NodeType::DocOnly);
        assert_eq!(node.inputs.len(), 1);
        assert_eq!(node.inputs[0].name, "task");
        assert_eq!(node.outputs.len(), 1);
        assert_eq!(node.outputs[0].name, "plan");

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn fails_on_missing_node_name() {
        let yaml = with_start_end(
            r#"
name: no-name
nodes:
  - id: ab12cd34
    type: doc-only
    inputs:
      - name: in
    outputs:
      - name: out
"#,
        );
        let err = parse_pipeline(&yaml).unwrap_err();
        assert!(matches!(err, ParseError::InvalidYaml(_)));
    }

    #[test]
    fn ignores_deprecated_prompt_file_field() {
        let yaml = with_start_end(
            r#"
name: old-style
nodes:
  - id: ab12cd34
    name: worker
    type: doc-only
    prompt_file: prompts/worker.md
    inputs:
      - name: in
    outputs:
      - name: out
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn errors_on_invalid_yaml() {
        let yaml = "{{not: valid: yaml:::";
        let err = parse_pipeline(yaml).unwrap_err();
        assert!(matches!(err, ParseError::InvalidYaml(_)));
    }

    #[test]
    fn errors_on_missing_pipeline_name() {
        let yaml = r#"
version: "1.0"
nodes: []
"#;
        let err = parse_pipeline(yaml).unwrap_err();
        assert!(matches!(err, ParseError::MissingField(_)));
    }

    #[test]
    fn rejects_pipeline_without_start_node() {
        let yaml = r#"
name: no-start
nodes:
  - id: end
    name: End
    type: end
    inputs:
      - name: result
  - id: ab12cd34
    name: worker
    type: doc-only
    inputs: []
    outputs: []
"#;
        let err = parse_pipeline(yaml).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("start"), "error should mention start: {msg}");
    }

    #[test]
    fn rejects_pipeline_without_end_node() {
        let yaml = r#"
name: no-end
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: ab12cd34
    name: worker
    type: doc-only
    inputs: []
    outputs: []
"#;
        let err = parse_pipeline(yaml).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("end"), "error should mention end: {msg}");
    }

    #[test]
    fn rejects_start_node_with_inputs() {
        let yaml = r#"
name: bad-start
nodes:
  - id: start
    name: Start
    type: start
    inputs:
      - name: oops
    outputs:
      - name: user_prompt
  - id: end
    name: End
    type: end
    inputs:
      - name: result
"#;
        let err = parse_pipeline(yaml).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("zero inputs"),
            "error should mention zero inputs: {msg}"
        );
    }

    #[test]
    fn rejects_start_node_with_wrong_output_port() {
        let yaml = r#"
name: bad-start-port
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: wrong_name
  - id: end
    name: End
    type: end
    inputs:
      - name: result
"#;
        let err = parse_pipeline(yaml).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("user_prompt"),
            "error should mention user_prompt: {msg}"
        );
    }

    #[test]
    fn rejects_end_node_with_outputs() {
        let yaml = r#"
name: bad-end
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: end
    name: End
    type: end
    inputs:
      - name: result
    outputs:
      - name: oops
"#;
        let err = parse_pipeline(yaml).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("zero outputs"),
            "error should mention zero outputs: {msg}"
        );
    }

    #[test]
    fn rejects_end_node_with_wrong_input_port() {
        let yaml = r#"
name: bad-end-port
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: end
    name: End
    type: end
    inputs:
      - name: wrong_name
"#;
        let err = parse_pipeline(yaml).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("result"), "error should mention result: {msg}");
    }

    #[test]
    fn rejects_legacy_halt_target_syntax() {
        let yaml = with_start_end(
            r#"
name: with-halt
nodes:
  - id: ab12cd34
    name: reviewer
    type: doc-only
    outputs:
      - name: review
edges:
  - source: { node: ab12cd34, port: review }
    target: { halt: { message: "Blocked" } }
"#,
        );
        let err = parse_pipeline(&yaml).unwrap_err();
        assert!(matches!(err, ParseError::InvalidYaml(_)));
    }

    #[test]
    fn parses_edge_with_reason() {
        let yaml = with_start_end(
            r#"
name: with-reason
nodes:
  - id: ab12cd34
    name: reviewer
    type: doc-only
    outputs:
      - name: review
edges:
  - source: { node: ab12cd34, port: review }
    target: { node: end, port: result }
    reason: "Blocked after {iter} iterations"
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let edge = &result.pipeline.edges[0];
        assert_eq!(
            edge.reason.as_deref(),
            Some("Blocked after {iter} iterations")
        );
        assert_eq!(edge.target.node, "end");
        assert_eq!(edge.target.port, "result");
    }

    #[test]
    fn parses_edge_with_when_clause_and_else_marker() {
        let yaml = with_start_end(
            r#"
name: conditional-edges
nodes:
  - id: ab000001
    name: reviewer
    type: doc-only
    outputs:
      - name: review
  - id: ab000002
    name: implementer
    type: code-mutating
    inputs:
      - name: review
    outputs:
      - name: code
  - id: ab000003
    name: archiver
    type: doc-only
    inputs:
      - name: review
    outputs:
      - name: note
edges:
  - source: { node: ab000001, port: review }
    target: { node: ab000002, port: review }
    when:
      verdict: { eq: FAIL }
  - source: { node: ab000001, port: review }
    target: { node: ab000003, port: review }
    else: true
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let guarded = &result.pipeline.edges[0];
        assert!(guarded.when.is_some(), "first edge should carry when:");
        assert!(!guarded.is_else, "first edge is not an else edge");

        let fallback = &result.pipeline.edges[1];
        assert!(fallback.when.is_none(), "else edge carries no when:");
        assert!(fallback.is_else, "second edge should be an else edge");

        let serialized = serde_yaml::to_string(&result.pipeline).unwrap();
        let reparsed = parse_pipeline(&serialized).unwrap();
        assert!(reparsed.pipeline.edges[0].when.is_some());
        assert!(reparsed.pipeline.edges[1].is_else);
    }

    #[test]
    fn parses_interactive_node() {
        let yaml = with_start_end(
            r#"
name: interactive-pipe
nodes:
  - id: ab000001
    name: griller
    type: doc-only
    interactive: true
    inputs:
      - name: task
    outputs:
      - name: brief
  - id: ab000002
    name: worker
    type: code-mutating
    inputs:
      - name: brief
    outputs:
      - name: summary
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let griller = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab000001")
            .unwrap();
        assert!(griller.interactive);
        let worker = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab000002")
            .unwrap();
        assert!(!worker.interactive);
    }

    #[test]
    fn interactive_defaults_to_false() {
        let result = parse_pipeline(VALID_MINIMAL).unwrap();
        let planner = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab12cd34")
            .unwrap();
        assert!(!planner.interactive);
    }

    #[test]
    fn parses_typed_variables_explicit_form() {
        let yaml = with_start_end(
            r#"
name: typed-vars
variables:
  max_iter:
    type: int
    default: 5
  mode:
    type: string
    default: strict
  verbose:
    type: bool
    default: true
nodes: []
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let vars = &result.pipeline.variables;
        assert_eq!(vars.len(), 3);
        assert_eq!(vars["max_iter"].var_type, VariableType::Int);
        assert_eq!(vars["mode"].var_type, VariableType::String);
        assert_eq!(vars["verbose"].var_type, VariableType::Bool);
    }

    #[test]
    fn parses_variables_inferred_type_from_value() {
        let yaml = with_start_end(
            r#"
name: inferred-vars
variables:
  max_iter: 5
  threshold: 0.8
  mode: strict
  verbose: true
  tags: [a, b, c]
nodes: []
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let vars = &result.pipeline.variables;
        assert_eq!(vars["max_iter"].var_type, VariableType::Int);
        assert_eq!(vars["threshold"].var_type, VariableType::Float);
        assert_eq!(vars["mode"].var_type, VariableType::String);
        assert_eq!(vars["verbose"].var_type, VariableType::Bool);
        assert_eq!(vars["tags"].var_type, VariableType::List);
    }

    #[test]
    fn variable_defaults_extracts_values() {
        let yaml = with_start_end(
            r#"
name: defaults-test
variables:
  max_iter: 5
  threshold: 0.8
nodes: []
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let defaults = result.pipeline.variable_defaults();
        assert_eq!(
            defaults["max_iter"],
            serde_yaml::Value::Number(serde_yaml::Number::from(5))
        );
    }

    #[test]
    fn parses_pipeline_with_edges_and_variables() {
        let yaml = with_start_end(
            r#"
name: full-pipeline
version: "2.0"
variables:
  max_iter: 5
  threshold: 0.8
nodes:
  - id: ab000001
    name: planner
    type: doc-only
    inputs:
      - name: task
    outputs:
      - name: plan
  - id: ab000002
    name: implementer
    type: code-mutating
    inputs:
      - name: plan
    outputs:
      - name: summary
edges:
  - source: { node: ab000001, port: plan }
    target: { node: ab000002, port: plan }
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        assert_eq!(result.pipeline.edges.len(), 1);
        assert_eq!(result.pipeline.variables.len(), 2);
        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn dangling_edge_references_names_the_edge_and_the_port() {
        // A source-port typo is a dangling reference; the message names both the
        // edge and the missing port so a launch refusal is actionable.
        let yaml = with_start_end(
            r#"
name: dangling
nodes:
  - id: ab000001
    name: planner
    type: doc-only
    outputs:
      - name: plan
  - id: ab000002
    name: implementer
    type: doc-only
edges:
  - source: { node: ab000001, port: plaan }
    target: { node: ab000002, port: spec }
"#,
        );
        let pipeline = parse_pipeline(&yaml).unwrap().pipeline;
        let errors = dangling_edge_references(&pipeline);
        assert_eq!(
            errors.len(),
            1,
            "exactly one dangling reference: {errors:?}"
        );
        assert!(
            errors[0].contains("ab000001") && errors[0].contains("'plaan'"),
            "error must name the edge endpoint and the missing port; got: {}",
            errors[0]
        );
        assert!(
            errors[0].contains("ab000002") && errors[0].contains("spec"),
            "error must identify the edge (both endpoints); got: {}",
            errors[0]
        );
    }

    #[test]
    fn dangling_edge_references_flags_nonexistent_nodes_but_not_emergent_inputs() {
        let yaml = with_start_end(
            r#"
name: dangling-node
nodes:
  - id: ab000001
    name: planner
    type: doc-only
    outputs:
      - name: plan
  - id: ab000002
    name: implementer
    type: doc-only
edges:
  - source: { node: ab000001, port: plan }
    target: { node: ghost, port: plan }
  - source: { node: ab000001, port: plan }
    target: { node: ab000002, port: anything }
"#,
        );
        let pipeline = parse_pipeline(&yaml).unwrap().pipeline;
        let errors = dangling_edge_references(&pipeline);
        assert_eq!(
            errors.len(),
            1,
            "only the ghost-node edge is dangling — an emergent input on a \
             regular node is valid by construction (#149); got: {errors:?}"
        );
        assert!(errors[0].contains("non-existent node 'ghost'"));
    }

    #[test]
    fn dangling_edge_references_empty_on_well_formed_pipeline() {
        let yaml = r#"
name: ok
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: ab000001
    name: planner
    type: doc-only
    outputs:
      - name: plan
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: ab000001, port: task }
  - source: { node: ab000001, port: plan }
    target: { node: end, port: result }
"#;
        let pipeline = parse_pipeline(yaml).unwrap().pipeline;
        assert!(dangling_edge_references(&pipeline).is_empty());
    }

    #[test]
    fn dangling_references_flag_unknown_loop_member() {
        // A region member naming a missing node makes the region silently no-op at
        // runtime — same class as a dangling edge.
        let yaml = r#"
name: orphan-region
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: ab000001
    name: worker
    type: doc-only
    outputs:
      - name: out
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: ab000001, port: task }
  - source: { node: ab000001, port: out }
    target: { node: end, port: result }
loops:
  - id: per-item
    kind: collection
    over: items
    members: [worker]
"#;
        let result = parse_pipeline(yaml).unwrap();
        let errors = dangling_edge_references(&result.pipeline);
        assert_eq!(errors.len(), 1);
        assert!(
            errors[0].contains("loop region 'per-item'")
                && errors[0].contains("non-existent node 'worker'"),
            "unexpected message: {}",
            errors[0]
        );
        assert!(result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("loop region 'per-item'")));
    }

    #[test]
    fn warns_on_edge_to_nonexistent_node() {
        let yaml = with_start_end(
            r#"
name: bad-edge
nodes:
  - id: ab000001
    name: planner
    type: doc-only
    outputs:
      - name: plan
edges:
  - source: { node: ab000001, port: plan }
    target: { node: ghost, port: plan }
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let warnings: Vec<&str> = result
            .diagnostics
            .iter()
            .map(|d| d.message.as_str())
            .collect();
        assert!(warnings
            .iter()
            .any(|w| w.contains("non-existent node 'ghost'")));
    }

    #[test]
    fn warns_on_source_port_typo_but_not_emergent_target() {
        // Outputs stay declared, so a source-port typo still warns. Inputs on a
        // regular node are emergent — any target port is valid by construction, so
        // there must be no false-positive "target port not found".
        let yaml = with_start_end(
            r#"
name: bad-port
nodes:
  - id: ab000001
    name: planner
    type: doc-only
    outputs:
      - name: plan
  - id: ab000002
    name: implementer
    type: doc-only
edges:
  - source: { node: ab000001, port: plaan }
    target: { node: ab000002, port: anything }
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let warnings: Vec<&str> = result
            .diagnostics
            .iter()
            .map(|d| d.message.as_str())
            .collect();
        assert!(warnings
            .iter()
            .any(|w| w.contains("source port 'plaan' not found")));
        assert!(
            !warnings.iter().any(|w| w.contains("target port")),
            "emergent input on a regular node must not warn on target port; got: {warnings:?}"
        );
    }

    #[test]
    fn warns_on_structural_target_port_typo() {
        // Structural nodes (here: the End node's `result` input) keep their
        // declared, required ports — a target-port typo on them is still a warning.
        let yaml = r#"
name: bad-structural-target
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: ab000001
    name: only
    type: doc-only
    outputs:
      - name: out
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: ab000001, port: user_prompt }
  - source: { node: ab000001, port: out }
    target: { node: end, port: resullt }
"#;
        let result = parse_pipeline(yaml).unwrap();
        let warnings: Vec<&str> = result
            .diagnostics
            .iter()
            .map(|d| d.message.as_str())
            .collect();
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("target port 'resullt' not found")),
            "structural End-node target typo should warn; got: {warnings:?}"
        );
        // The canonical emergent Start->only edge must NOT warn (the #149 regression).
        assert!(
            !warnings.iter().any(|w| w.contains("user_prompt")),
            "emergent Start->only edge must not warn; got: {warnings:?}"
        );
    }

    #[test]
    fn no_warning_on_cycle_in_topology() {
        let yaml = with_start_end(
            r#"
name: cycle
nodes:
  - id: ab000001
    name: implementer
    type: doc-only
    inputs:
      - name: review
    outputs:
      - name: code
  - id: ab000002
    name: reviewer
    type: doc-only
    inputs:
      - name: code
    outputs:
      - name: review
edges:
  - source: { node: ab000001, port: code }
    target: { node: ab000002, port: code }
  - source: { node: ab000002, port: review }
    target: { node: ab000001, port: review }
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        assert!(
            result.diagnostics.is_empty(),
            "cycle should not produce warnings, got: {:?}",
            result
                .diagnostics
                .iter()
                .map(|d| &d.message)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn parses_bounded_loop_region_block() {
        // A loop is a named entry of the `loops:` block, no longer a node.
        let yaml = with_start_end(
            r#"
name: with-region
nodes:
  - id: ab000001
    name: implementer
    type: code-mutating
    outputs:
      - name: code
  - id: ab000002
    name: reviewer
    type: doc-only
    outputs:
      - name: review
loops:
  - id: review_loop
    kind: bounded
    members: [ab000001, ab000002]
    max_iter: 3
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        assert_eq!(result.pipeline.loops.len(), 1);
        let region = &result.pipeline.loops[0];
        assert_eq!(region.id, "review_loop");
        assert_eq!(region.kind, LoopKind::Bounded);
        assert_eq!(region.members, vec!["ab000001", "ab000002"]);
        assert_eq!(
            region.max_iter,
            Some(serde_yaml::Value::Number(serde_yaml::Number::from(3)))
        );
    }

    #[test]
    fn parses_collection_loop_region_block_and_round_trips() {
        // A collection region carries no `max_iter` — the lap count is the
        // collection size. The `over` driver and `kind` must round-trip.
        let yaml = with_start_end(
            r#"
name: with-collection
nodes:
  - id: ab000001
    name: triage
    type: doc-only
    outputs:
      - name: plan
        frontmatter:
          issues:
            type: list
  - id: ab000002
    name: fixer
    type: code-mutating
    outputs:
      - name: fix
loops:
  - id: per-issue
    kind: collection
    over: issues
    members: [ab000002]
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        assert_eq!(result.pipeline.loops.len(), 1);
        let region = &result.pipeline.loops[0];
        assert_eq!(region.id, "per-issue");
        assert_eq!(region.kind, LoopKind::Collection);
        assert_eq!(region.over.as_deref(), Some("issues"));
        assert_eq!(region.members, vec!["ab000002"]);
        assert_eq!(region.max_iter, None);

        let serialized = serde_yaml::to_string(&result.pipeline).unwrap();
        let reparsed = parse_pipeline(&serialized).unwrap();
        let r = &reparsed.pipeline.loops[0];
        assert_eq!(r.kind, LoopKind::Collection);
        assert_eq!(r.over.as_deref(), Some("issues"));
    }

    #[test]
    fn parses_nodes_with_view_positions() {
        let yaml = with_start_end(
            r#"
name: with-view
nodes:
  - id: ab12cd34
    name: planner
    type: doc-only
    view: { x: 100, y: 200 }
    outputs:
      - name: plan
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let node = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab12cd34")
            .unwrap();
        let view = node.view.as_ref().unwrap();
        assert_eq!(view.x, 100.0);
        assert_eq!(view.y, 200.0);
    }

    #[test]
    fn accepts_edge_with_when_clause() {
        // ADR-0011 supersedes #45: conditional routing lives on the edge again.
        let yaml = with_start_end(
            r#"
name: conditional
nodes:
  - id: ab000001
    name: reviewer
    type: doc-only
    outputs:
      - name: review
  - id: ab000002
    name: implementer
    type: code-mutating
    inputs:
      - name: review
    outputs:
      - name: code
edges:
  - source: { node: ab000001, port: review }
    target: { node: ab000002, port: review }
    when:
      iter: { lt: 5 }
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        assert!(result.pipeline.edges[0].when.is_some());
    }

    #[test]
    fn parses_repeated_flag_on_edge() {
        // `repeated` is an edge property, not a declared input port: it marks an
        // edge whose source artifact accumulates across iterations.
        let yaml = with_start_end(
            r#"
name: repeated-edge
nodes:
  - id: ab000001
    name: reviewer
    type: doc-only
    outputs:
      - name: review
  - id: ab000002
    name: implementer
    type: code-mutating
    outputs:
      - name: code
edges:
  - source: { node: ab000001, port: review }
    target: { node: ab000002, port: reviews }
    repeated: true
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        assert!(result.pipeline.edges[0].repeated);
        let serialized = serde_yaml::to_string(&result.pipeline).unwrap();
        let reparsed: PipelineDef = serde_yaml::from_str(&serialized).unwrap();
        assert!(reparsed.edges[0].repeated);
    }

    #[test]
    fn parses_manual_edge_routing_and_round_trips() {
        // Manual routing persists in the pipeline file so a shared workflow carries
        // its arrows; the daemon must re-serialize it without drift.
        let yaml = with_start_end(
            r#"
name: routed-edge
nodes:
  - id: ab000001
    name: reviewer
    type: doc-only
    outputs:
      - name: review
  - id: ab000002
    name: implementer
    type: code-mutating
    outputs:
      - name: code
edges:
  - source: { node: ab000001, port: review }
    target: { node: ab000002, port: review }
    mode: manual
    waypoints:
      - { x: 120, y: 40 }
      - { x: 120, y: 220 }
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let edge = &result.pipeline.edges[0];
        assert_eq!(edge.mode, Some(EdgeRouteMode::Manual));
        let wp = edge.waypoints.as_ref().expect("waypoints parsed");
        assert_eq!(wp.len(), 2);
        assert_eq!(wp[0].x, 120.0);
        assert_eq!(wp[0].y, 40.0);
        assert_eq!(wp[1].y, 220.0);

        let serialized = serde_yaml::to_string(&result.pipeline).unwrap();
        let reparsed: PipelineDef = serde_yaml::from_str(&serialized).unwrap();
        let redge = &reparsed.edges[0];
        assert_eq!(redge.mode, Some(EdgeRouteMode::Manual));
        assert_eq!(redge.waypoints.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn parses_edge_target_side_and_round_trips() {
        // The incoming-edge anchor side is layout, but persisted so a shared
        // workflow keeps its arrow arrival sides.
        let yaml = with_start_end(
            r#"
name: anchored-edge
nodes:
  - id: ab000001
    name: planner
    type: doc-only
    outputs:
      - name: plan
  - id: ab000002
    name: implementer
    type: code-mutating
    outputs:
      - name: code
edges:
  - source: { node: ab000001, port: plan }
    target: { node: ab000002, port: plan }
    target_side: top
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        assert_eq!(result.pipeline.edges[0].target_side, Some(PortSide::Top));

        let serialized = serde_yaml::to_string(&result.pipeline).unwrap();
        let reparsed: PipelineDef = serde_yaml::from_str(&serialized).unwrap();
        assert_eq!(reparsed.edges[0].target_side, Some(PortSide::Top));
    }

    #[test]
    fn edge_target_side_defaults_to_none() {
        // An edge without an explicit anchor side parses with `target_side: None`
        // (legacy left anchoring is the canvas default, never written to file).
        let yaml = with_start_end(
            r#"
name: unanchored-edge
nodes:
  - id: ab000001
    name: planner
    type: doc-only
    outputs:
      - name: plan
  - id: ab000002
    name: implementer
    type: code-mutating
    outputs:
      - name: code
edges:
  - source: { node: ab000001, port: plan }
    target: { node: ab000002, port: plan }
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        assert_eq!(result.pipeline.edges[0].target_side, None);
        // Absent ⇒ never serialized (clean file, round-trips by absence).
        let serialized = serde_yaml::to_string(&result.pipeline).unwrap();
        assert!(!serialized.contains("target_side"));
    }

    #[test]
    fn edge_routing_defaults_to_none() {
        // An edge without explicit routing parses with no mode and no waypoints
        // (auto routing is recomputed deterministically, never persisted).
        let yaml = with_start_end(
            r#"
name: auto-edge
nodes:
  - id: ab000001
    name: planner
    type: doc-only
    outputs:
      - name: plan
  - id: ab000002
    name: implementer
    type: code-mutating
    outputs:
      - name: code
edges:
  - source: { node: ab000001, port: plan }
    target: { node: ab000002, port: plan }
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        assert_eq!(result.pipeline.edges[0].mode, None);
        assert!(result.pipeline.edges[0].waypoints.is_none());
    }

    #[test]
    fn edge_repeated_defaults_to_false() {
        let yaml = with_start_end(
            r#"
name: plain-edge
nodes:
  - id: ab000001
    name: planner
    type: doc-only
    outputs:
      - name: plan
  - id: ab000002
    name: implementer
    type: code-mutating
    outputs:
      - name: code
edges:
  - source: { node: ab000001, port: plan }
    target: { node: ab000002, port: plan }
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        assert!(!result.pipeline.edges[0].repeated);
    }

    #[test]
    fn parses_multiple_nodes_with_multiple_ports() {
        let yaml = with_start_end(
            r#"
name: multi-port
nodes:
  - id: ab000001
    name: planner
    type: doc-only
    inputs:
      - name: task
    outputs:
      - name: plan
      - name: task_list
  - id: ab000002
    name: implementer
    type: code-mutating
    inputs:
      - name: plan
      - name: task_list
    outputs:
      - name: summary
  - id: ab000003
    name: reviewer
    type: doc-only
    inputs:
      - name: summary
    outputs:
      - name: review
edges:
  - source: { node: ab000001, port: plan }
    target: { node: ab000002, port: plan }
  - source: { node: ab000001, port: task_list }
    target: { node: ab000002, port: task_list }
  - source: { node: ab000002, port: summary }
    target: { node: ab000003, port: summary }
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        assert_eq!(result.pipeline.edges.len(), 3);
        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn parses_output_port_with_frontmatter_schema() {
        let yaml = with_start_end(
            r#"
name: with-schema
nodes:
  - id: ab12cd34
    name: reviewer
    type: doc-only
    inputs:
      - name: code
    outputs:
      - name: review
        frontmatter:
          verdict:
            type: enum
            allowed: [PASS, FAIL]
          score:
            type: int
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let node = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab12cd34")
            .unwrap();
        let port = &node.outputs[0];
        assert_eq!(port.name, "review");
        let schema = port.frontmatter.as_ref().unwrap();
        assert_eq!(schema.len(), 2);
        assert_eq!(schema["verdict"].field_type, "enum");
        assert_eq!(
            schema["verdict"].allowed,
            Some(vec!["PASS".into(), "FAIL".into()])
        );
        assert_eq!(schema["score"].field_type, "int");
        assert!(schema["score"].allowed.is_none());
    }

    #[test]
    fn port_side_defaults_left_for_inputs_right_for_outputs() {
        let result = parse_pipeline(VALID_MINIMAL).unwrap();
        let node = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab12cd34")
            .unwrap();
        assert_eq!(node.inputs[0].side, Some(PortSide::Left));
        assert_eq!(node.outputs[0].side, Some(PortSide::Right));
    }

    #[test]
    fn parses_explicit_port_side_all_four_values() {
        let yaml = with_start_end(
            r#"
name: sides-test
nodes:
  - id: ab12cd34
    name: worker
    type: doc-only
    inputs:
      - name: left-in
        side: left
      - name: right-in
        side: right
      - name: top-in
        side: top
      - name: bottom-in
        side: bottom
    outputs:
      - name: left-out
        side: left
      - name: right-out
        side: right
      - name: top-out
        side: top
      - name: bottom-out
        side: bottom
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let node = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab12cd34")
            .unwrap();

        assert_eq!(node.inputs[0].side, Some(PortSide::Left));
        assert_eq!(node.inputs[1].side, Some(PortSide::Right));
        assert_eq!(node.inputs[2].side, Some(PortSide::Top));
        assert_eq!(node.inputs[3].side, Some(PortSide::Bottom));

        assert_eq!(node.outputs[0].side, Some(PortSide::Left));
        assert_eq!(node.outputs[1].side, Some(PortSide::Right));
        assert_eq!(node.outputs[2].side, Some(PortSide::Top));
        assert_eq!(node.outputs[3].side, Some(PortSide::Bottom));
    }

    #[test]
    fn port_side_omitted_gets_contextual_default() {
        let yaml = with_start_end(
            r#"
name: defaults-test
nodes:
  - id: ab12cd34
    name: worker
    type: doc-only
    inputs:
      - name: a
      - name: b
        side: top
    outputs:
      - name: x
      - name: y
        side: bottom
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let node = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab12cd34")
            .unwrap();

        assert_eq!(node.inputs[0].side, Some(PortSide::Left));
        assert_eq!(node.inputs[1].side, Some(PortSide::Top));
        assert_eq!(node.outputs[0].side, Some(PortSide::Right));
        assert_eq!(node.outputs[1].side, Some(PortSide::Bottom));
    }

    #[test]
    fn output_port_without_frontmatter_has_none() {
        let result = parse_pipeline(VALID_MINIMAL).unwrap();
        let planner = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab12cd34")
            .unwrap();
        let port = &planner.outputs[0];
        assert!(port.frontmatter.is_none());
    }

    #[test]
    fn canonical_prompt_path_for_template() {
        let pp = std::path::Path::new("/pipelines/review-loop.yaml");
        let path = canonical_prompt_path(pp, "ab12cd34");
        assert_eq!(
            path.to_str().unwrap(),
            "/pipelines/review-loop.prompts/ab12cd34.md"
        );
    }

    #[test]
    fn canonical_prompt_path_for_run() {
        let pp = std::path::Path::new("/runs/run-1/pipeline.yaml");
        let path = canonical_prompt_path(pp, "ab12cd34");
        assert_eq!(
            path.to_str().unwrap(),
            "/runs/run-1/pipeline.prompts/ab12cd34.md"
        );
    }

    #[test]
    fn parses_merge_node() {
        let yaml = with_start_end(
            r#"
name: merge-test
nodes:
  - id: ab000001
    name: merge-point
    type: merge
    inputs:
      - name: branches
        repeated: true
    outputs:
      - name: merged
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let mg = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab000001")
            .unwrap();
        assert_eq!(mg.node_type, NodeType::Merge);
        assert_eq!(mg.inputs.len(), 1);
        assert_eq!(mg.inputs[0].name, "branches");
        assert!(mg.inputs[0].repeated);
        assert_eq!(mg.outputs.len(), 1);
        assert_eq!(mg.outputs[0].name, "merged");
    }

    /// The shipped `disk-janitor` pipeline must parse cleanly and keep `reap` a
    /// `script` node — a silent degrade to `doc-only` would spend an LLM turn and
    /// lose determinism. `serializer_round_trip` only asserts *if* a pipeline is
    /// valid; this pins that it is.
    #[test]
    fn shipped_disk_janitor_pipeline_is_valid() {
        let yaml = include_str!("../../../.pdo/pipelines/disk-janitor.yaml");
        let result = parse_pipeline(yaml).expect("disk-janitor.yaml must parse");

        let reap = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "reap")
            .expect("disk-janitor.yaml must have a `reap` node");
        assert_eq!(
            reap.node_type,
            NodeType::Script,
            "the janitor's reap node must stay a `script` node (deterministic, no LLM turn)"
        );

        assert!(
            result
                .pipeline
                .nodes
                .iter()
                .any(|n| n.node_type == NodeType::Start),
            "must have a start node"
        );
        assert!(
            result
                .pipeline
                .nodes
                .iter()
                .any(|n| n.node_type == NodeType::End),
            "must have an end node"
        );
        assert!(
            !result
                .diagnostics
                .iter()
                .any(|d| d.severity == Severity::Error),
            "shipped pipeline must have no error-severity diagnostics: {:?}",
            result.diagnostics
        );
    }

    #[test]
    fn parses_script_node() {
        let yaml = with_start_end(
            r#"
name: script-test
nodes:
  - id: ab000001
    name: notify
    type: script
    outputs:
      - name: out
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let node = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab000001")
            .unwrap();
        assert_eq!(node.node_type, NodeType::Script);
        assert_eq!(node.outputs.len(), 1);
        assert_eq!(node.outputs[0].name, "out");
    }

    #[test]
    fn script_node_is_not_rewritten_to_doc_only() {
        // Silent-failure guard: `script` must be in `valid_types`, else
        // `parse_pipeline` degrades the node to `doc-only` with a diagnostic.
        let yaml = with_start_end(
            r#"
name: script-not-rewritten
nodes:
  - id: ab000001
    name: notify
    type: script
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let node = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab000001")
            .unwrap();
        assert_eq!(
            node.node_type,
            NodeType::Script,
            "must not degrade to doc-only"
        );
        assert!(
            !result
                .diagnostics
                .iter()
                .any(|d| d.message.contains("ab000001")),
            "no rewrite diagnostic for a valid script node: {:?}",
            result.diagnostics
        );
    }

    #[test]
    fn script_node_has_emergent_inputs() {
        assert!(NodeType::Script.has_emergent_inputs());
    }

    #[test]
    fn script_node_roundtrips_via_serde() {
        let node = NodeDef {
            id: "s1".into(),
            name: "notify".into(),
            node_type: NodeType::Script,
            inputs: vec![],
            outputs: vec![Port {
                name: "out".into(),
                repeated: false,
                side: None,
                port_type: PortType::Markdown,
                frontmatter: None,
                when: None,
                description: None,
                instructions: None,
                required: false,
            }],
            interactive: false,
            view: None,
            max_iter: None,
            over: None,
            pin_harness: None,
            harnesses: Default::default(),
            agent_choice: None,
            auto_fail: None,
        };
        let yaml = serde_yaml::to_string(&node).unwrap();
        assert!(yaml.contains("type: script"), "serializes to kebab: {yaml}");
        let back: NodeDef = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(back.node_type, NodeType::Script);
    }

    /// A node with no `agent_choice:` key parses to `None` and re-serializes
    /// without the key — the byte-identical no-migration contract (ADR-0057).
    #[test]
    fn node_without_agent_choice_key_parses_to_none_and_omits_it_on_reserialize() {
        let yaml = with_start_end(
            r#"
name: legacy-pipeline
nodes:
  - id: ab000001
    name: planner
    type: doc-only
    inputs:
      - name: task
    outputs:
      - name: plan
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let node = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab000001")
            .unwrap();
        assert_eq!(node.agent_choice, None);
        let round = serde_yaml::to_string(node).unwrap();
        assert!(
            !round.contains("agent_choice"),
            "absent agent_choice must not appear on reserialize: {round}"
        );
    }

    /// `agent_choice: {mode: profile, ...}` parses into
    /// [`crate::agent_choice::AgentChoice::Profile`] and round-trips unchanged.
    #[test]
    fn node_agent_choice_profile_reference_round_trips() {
        let yaml = with_start_end(
            r#"
name: profile-pipeline
nodes:
  - id: ab000001
    name: planner
    type: doc-only
    inputs:
      - name: task
    outputs:
      - name: plan
    agent_choice:
      mode: profile
      profile_id: reviewer
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let node = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab000001")
            .unwrap();
        assert_eq!(
            node.agent_choice,
            Some(crate::agent_choice::AgentChoice::Profile {
                profile_id: "reviewer".into()
            })
        );
        let round = serde_yaml::to_string(node).unwrap();
        let back: NodeDef = serde_yaml::from_str(&round).unwrap();
        assert_eq!(back.agent_choice, node.agent_choice);
    }

    /// `agent_choice: {mode: custom, ...}` parses atomically — no field is read
    /// from `harnesses`/`pin_harness` even when both are present (ADR-0057 ¶1).
    #[test]
    fn node_agent_choice_custom_round_trips_and_coexists_with_legacy_fields() {
        let yaml = with_start_end(
            r#"
name: custom-pipeline
nodes:
  - id: ab000001
    name: planner
    type: doc-only
    inputs:
      - name: task
    outputs:
      - name: plan
    pin_harness: claude
    harnesses:
      claude:
        model: sonnet
    agent_choice:
      mode: custom
      harness: opencode
      model: gpt-5
      effort: high
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let node = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab000001")
            .unwrap();
        assert_eq!(
            node.agent_choice,
            Some(crate::agent_choice::AgentChoice::Custom {
                harness: "opencode".into(),
                model: Some("gpt-5".into()),
                effort: Some("high".into()),
            })
        );
        // Legacy fields are still parsed and preserved verbatim — Custom wins
        // at resolution time (agent_choice.rs), it does not erase them.
        assert_eq!(node.pin_harness.as_deref(), Some("claude"));
        assert!(node.harnesses.contains_key("claude"));
    }

    #[test]
    fn merge_node_rejects_wrong_input_name() {
        let yaml = with_start_end(
            r#"
name: bad-merge
nodes:
  - id: ab000001
    name: bad-merge
    type: merge
    inputs:
      - name: in
        repeated: true
    outputs:
      - name: merged
"#,
        );
        let err = parse_pipeline(&yaml).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("branches"),
            "error should mention 'branches': {msg}"
        );
    }

    #[test]
    fn merge_node_rejects_non_repeated_input() {
        let yaml = with_start_end(
            r#"
name: bad-merge
nodes:
  - id: ab000001
    name: bad-merge
    type: merge
    inputs:
      - name: branches
    outputs:
      - name: merged
"#,
        );
        let err = parse_pipeline(&yaml).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("repeated"),
            "error should mention 'repeated': {msg}"
        );
    }

    #[test]
    fn merge_node_rejects_wrong_output_name() {
        let yaml = with_start_end(
            r#"
name: bad-merge
nodes:
  - id: ab000001
    name: bad-merge
    type: merge
    inputs:
      - name: branches
        repeated: true
    outputs:
      - name: out
"#,
        );
        let err = parse_pipeline(&yaml).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("merged"),
            "error should mention 'merged': {msg}"
        );
    }

    #[test]
    fn legacy_auto_merge_resolver_field_ignored() {
        let yaml = with_start_end(
            r#"
name: with-old-field
auto_merge_resolver: true
nodes: []
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        assert_eq!(result.pipeline.name, "with-old-field");
    }

    #[test]
    fn parses_switch_node_with_when_on_outputs() {
        let yaml = with_start_end(
            r#"
name: switch-test
nodes:
  - id: reviewer
    name: Reviewer
    type: doc-only
    outputs:
      - name: review
        frontmatter:
          verdict:
            type: enum
            allowed: [PASS, APPROVED, FAIL]
  - id: ab000001
    name: verdict-switch
    type: switch
    inputs:
      - name: in
    outputs:
      - name: pass
        when:
          verdict: { in: [PASS, APPROVED] }
      - name: default
edges:
  - source: { node: reviewer, port: review }
    target: { node: ab000001, port: in }
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let sw = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab000001")
            .unwrap();
        assert_eq!(sw.node_type, NodeType::Switch);
        assert_eq!(sw.inputs.len(), 1);
        assert_eq!(sw.inputs[0].name, "in");
        assert_eq!(sw.outputs.len(), 2);
        assert_eq!(sw.outputs[0].name, "pass");
        assert!(sw.outputs[0].when.is_some());
        assert_eq!(sw.outputs[1].name, "default");
        assert!(sw.outputs[1].when.is_none());
    }

    #[test]
    fn switch_node_auto_appends_default_if_missing() {
        let yaml = with_start_end(
            r#"
name: switch-no-default
nodes:
  - id: reviewer
    name: Reviewer
    type: doc-only
    outputs:
      - name: review
        frontmatter:
          verdict:
            type: enum
            allowed: [PASS, FAIL]
  - id: ab000001
    name: verdict-switch
    type: switch
    inputs:
      - name: in
    outputs:
      - name: pass
        when:
          verdict: { in: [PASS] }
edges:
  - source: { node: reviewer, port: review }
    target: { node: ab000001, port: in }
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let sw = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab000001")
            .unwrap();
        assert_eq!(sw.outputs.len(), 2);
        assert_eq!(sw.outputs[1].name, "default");
        assert!(sw.outputs[1].when.is_none());
        assert_eq!(sw.outputs[1].side, Some(PortSide::Right));
    }

    #[test]
    fn switch_node_explicit_default_not_duplicated() {
        let yaml = with_start_end(
            r#"
name: switch-explicit-default
nodes:
  - id: reviewer
    name: Reviewer
    type: doc-only
    outputs:
      - name: review
        frontmatter:
          verdict:
            type: enum
            allowed: [PASS, FAIL]
  - id: ab000001
    name: verdict-switch
    type: switch
    inputs:
      - name: in
    outputs:
      - name: pass
        when:
          verdict: { in: [PASS] }
      - name: default
        side: bottom
edges:
  - source: { node: reviewer, port: review }
    target: { node: ab000001, port: in }
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let sw = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab000001")
            .unwrap();
        assert_eq!(sw.outputs.len(), 2);
        let default_port = sw.outputs.iter().find(|p| p.name == "default").unwrap();
        assert_eq!(default_port.side, Some(PortSide::Bottom));
    }

    #[test]
    fn switch_node_rejects_wrong_input_name() {
        let yaml = with_start_end(
            r#"
name: bad-switch
nodes:
  - id: ab000001
    name: bad-switch
    type: switch
    inputs:
      - name: data
    outputs:
      - name: default
"#,
        );
        let err = parse_pipeline(&yaml).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("exactly one input named 'in'"),
            "error should mention input 'in': {msg}"
        );
    }

    #[test]
    fn switch_node_rejects_multiple_inputs() {
        let yaml = with_start_end(
            r#"
name: bad-switch
nodes:
  - id: ab000001
    name: bad-switch
    type: switch
    inputs:
      - name: in
      - name: extra
    outputs:
      - name: default
"#,
        );
        let err = parse_pipeline(&yaml).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("exactly one input named 'in'"),
            "error should mention input constraint: {msg}"
        );
    }

    #[test]
    fn parses_loop_node_with_max_iter() {
        let yaml = with_start_end(
            r#"
name: loop-test
nodes:
  - id: ab000001
    name: review-loop
    type: loop
    max_iter: 5
    inputs:
      - name: in
      - name: break
    outputs:
      - name: body
      - name: done
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let lp = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab000001")
            .unwrap();
        assert_eq!(lp.node_type, NodeType::Loop);
        assert_eq!(lp.inputs.len(), 2);
        assert_eq!(lp.outputs.len(), 2);
        let max_iter = lp.max_iter.as_ref().unwrap();
        assert_eq!(max_iter.as_u64(), Some(5));
    }

    #[test]
    fn parses_loop_node_with_variable_max_iter() {
        let yaml = with_start_end(
            r#"
name: loop-var-test
variables:
  max_iter_review: 3
nodes:
  - id: ab000001
    name: review-loop
    type: loop
    max_iter: "$max_iter_review"
    inputs:
      - name: in
      - name: break
    outputs:
      - name: body
      - name: done
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let lp = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab000001")
            .unwrap();
        let max_iter = lp.max_iter.as_ref().unwrap();
        assert_eq!(max_iter.as_str(), Some("$max_iter_review"));
    }

    #[test]
    fn loop_node_rejects_missing_max_iter() {
        let yaml = with_start_end(
            r#"
name: bad-loop
nodes:
  - id: ab000001
    name: bad-loop
    type: loop
    inputs:
      - name: in
      - name: break
    outputs:
      - name: body
      - name: done
"#,
        );
        let err = parse_pipeline(&yaml).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("must declare 'max_iter'"),
            "error should mention max_iter: {msg}"
        );
    }

    #[test]
    fn loop_node_rejects_missing_break_input() {
        let yaml = with_start_end(
            r#"
name: bad-loop
nodes:
  - id: ab000001
    name: bad-loop
    type: loop
    max_iter: 3
    inputs:
      - name: in
    outputs:
      - name: body
      - name: done
"#,
        );
        let err = parse_pipeline(&yaml).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("must have input 'break'"),
            "error should mention break: {msg}"
        );
    }

    #[test]
    fn loop_node_rejects_missing_body_output() {
        let yaml = with_start_end(
            r#"
name: bad-loop
nodes:
  - id: ab000001
    name: bad-loop
    type: loop
    max_iter: 3
    inputs:
      - name: in
      - name: break
    outputs:
      - name: done
"#,
        );
        let err = parse_pipeline(&yaml).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("must have output 'body'"),
            "error should mention body: {msg}"
        );
    }

    #[test]
    fn round_trip_loop_switch_pipeline() {
        let yaml = r#"
name: review-loop
version: "1.0"
variables:
  max_iter_review: 3
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: ab000001
    name: review-loop
    type: loop
    max_iter: "$max_iter_review"
    inputs:
      - name: in
      - name: break
    outputs:
      - name: body
      - name: done
  - id: ab000002
    name: implementer
    type: code-mutating
    inputs:
      - name: task
    outputs:
      - name: code
  - id: ab000003
    name: reviewer
    type: doc-only
    inputs:
      - name: code
    outputs:
      - name: review
        frontmatter:
          verdict:
            type: enum
            allowed: [PASS, FAIL, APPROVED]
  - id: ab000004
    name: verdict-switch
    type: switch
    inputs:
      - name: in
    outputs:
      - name: pass
        when:
          verdict: { in: [PASS, APPROVED] }
      - name: default
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: ab000001, port: in }
  - source: { node: ab000001, port: body }
    target: { node: ab000002, port: task }
  - source: { node: ab000002, port: code }
    target: { node: ab000003, port: code }
  - source: { node: ab000003, port: review }
    target: { node: ab000004, port: in }
  - source: { node: ab000004, port: pass }
    target: { node: ab000001, port: break }
  - source: { node: ab000001, port: done }
    target: { node: end, port: result }
"#;
        let result = parse_pipeline(yaml).unwrap();
        assert_eq!(result.pipeline.name, "review-loop");
        assert_eq!(result.pipeline.nodes.len(), 6);
        assert_eq!(result.pipeline.edges.len(), 6);

        let loop_node = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.node_type == NodeType::Loop)
            .unwrap();
        assert_eq!(
            loop_node.max_iter.as_ref().unwrap().as_str(),
            Some("$max_iter_review")
        );

        let switch_node = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.node_type == NodeType::Switch)
            .unwrap();
        assert_eq!(switch_node.outputs.len(), 2);

        let serialized = serde_yaml::to_string(&result.pipeline).unwrap();
        let result2 = parse_pipeline(&serialized).unwrap();
        assert_eq!(result2.pipeline.name, result.pipeline.name);
        assert_eq!(result2.pipeline.nodes.len(), result.pipeline.nodes.len());
        assert_eq!(result2.pipeline.edges.len(), result.pipeline.edges.len());
    }

    #[test]
    fn parse_materializes_a_region_for_an_undeclared_cycle() {
        // ADR-0011 (b): a cycle is never accidentally unbounded, whatever route it
        // arrived by — hand-written file, imported workflow, or a mere reopen.
        let yaml = with_start_end(
            r#"
name: cyclic
nodes:
  - id: ab000001
    name: implementer
    type: code-mutating
    outputs:
      - name: code
  - id: ab000002
    name: reviewer
    type: doc-only
    outputs:
      - name: review
edges:
  - source: { node: ab000001, port: code }
    target: { node: ab000002, port: code }
  - source: { node: ab000002, port: review }
    target: { node: ab000001, port: task }
    else: true
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        assert_eq!(result.pipeline.loops.len(), 1);
        let region = &result.pipeline.loops[0];
        assert_eq!(region.kind, LoopKind::Bounded);
        let mut members = region.members.clone();
        members.sort();
        assert_eq!(members, vec!["ab000001", "ab000002"]);
        assert_eq!(
            region.id,
            crate::loop_region::generated_region_id(&region.members)
        );
        // Derived, never invented: re-parsing the same bytes yields the same id, so
        // the region a live run counts laps against keeps its identity.
        assert_eq!(
            parse_pipeline(&yaml).unwrap().pipeline.loops[0].id,
            region.id
        );
        assert!(
            result.diagnostics.is_empty(),
            "a plain cycle must parse clean: {:?}",
            result.diagnostics
        );
    }

    #[test]
    fn parse_keeps_a_declared_region_verbatim() {
        // No-regression half of #396: a pipeline that already carries its `loops:`
        // block keeps that entry — its own id, its own bound — and gains no
        // duplicate region beside it.
        let yaml = with_start_end(
            r#"
name: cyclic-declared
nodes:
  - id: ab000001
    name: implementer
    type: code-mutating
    outputs:
      - name: code
  - id: ab000002
    name: reviewer
    type: doc-only
    outputs:
      - name: review
edges:
  - source: { node: ab000001, port: code }
    target: { node: ab000002, port: code }
  - source: { node: ab000002, port: review }
    target: { node: ab000001, port: task }
    else: true
loops:
  - id: my-loop
    kind: bounded
    members: [ab000001, ab000002]
    max_iter: 2
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        assert_eq!(result.pipeline.loops.len(), 1);
        assert_eq!(result.pipeline.loops[0].id, "my-loop");
        assert_eq!(
            crate::loop_region::resolve_region_max_iter(&result.pipeline.loops[0], &HashMap::new()),
            2
        );
    }

    #[test]
    fn parse_diagnoses_legacy_control_nodes_instead_of_wrapping_them() {
        // A `type: loop` node keeps its own bound and scheduler path, so no region
        // is derived over its cycle — that would run one loop on two counters and
        // show DEFAULT_MAX_ITER where the engine uses 3.
        let result = parse_pipeline(crate::pipeline_migrator::LEGACY_REVIEW_LOOP_YAML).unwrap();
        assert!(
            result.pipeline.loops.is_empty(),
            "no region over a legacy loop node: {:?}",
            result.pipeline.loops
        );
        let messages: Vec<&str> = result
            .diagnostics
            .iter()
            .map(|d| d.message.as_str())
            .collect();
        assert!(
            messages.iter().any(|m| m.contains("pdo migrate")
                && m.contains("loop 'qdtXejYS'")
                && m.contains("switch 'tmkRzihO'")),
            "the diagnostic must name every legacy node and the remedy: {messages:?}"
        );
    }

    #[test]
    fn parse_fixture_review_loop_yaml() {
        // The seed fixture is post-ADR-0011: conditional edges plus a named bounded
        // region, no legacy control node.
        let yaml = include_str!("../../../.pdo/pipelines/review-loop.yaml");
        let result = parse_pipeline(yaml).unwrap();
        assert_eq!(result.pipeline.name, "review-loop");
        assert!(!result
            .pipeline
            .nodes
            .iter()
            .any(|n| matches!(n.node_type, NodeType::Loop | NodeType::Switch)));
        assert!(result
            .pipeline
            .edges
            .iter()
            .any(|e| e.when.is_some() || e.is_else));
        assert_eq!(result.pipeline.loops.len(), 1);
        assert_eq!(result.pipeline.loops[0].kind, LoopKind::Bounded);
    }

    #[test]
    fn existing_node_types_still_parse() {
        let result = parse_pipeline(VALID_MINIMAL).unwrap();
        let types: Vec<&NodeType> = result.pipeline.nodes.iter().map(|n| &n.node_type).collect();
        assert!(types.contains(&&NodeType::Start));
        assert!(types.contains(&&NodeType::End));
        assert!(types.contains(&&NodeType::DocOnly));
    }

    #[test]
    fn resolve_switch_upstream_schema_returns_schema_when_connected() {
        let yaml = with_start_end(
            r#"
name: typed-switch
nodes:
  - id: reviewer
    name: Reviewer
    type: doc-only
    inputs:
      - name: code
    outputs:
      - name: review
        frontmatter:
          verdict:
            type: enum
            allowed: [PASS, FAIL]
          score:
            type: int
  - id: gate
    name: Gate
    type: switch
    inputs:
      - name: in
    outputs:
      - name: pass
        when:
          verdict: { eq: PASS }
      - name: default
edges:
  - source: { node: reviewer, port: review }
    target: { node: gate, port: in }
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let schema = resolve_switch_upstream_schema(&result.pipeline, "gate");
        assert!(schema.is_some());
        let schema = schema.unwrap();
        assert_eq!(schema.len(), 2);
        assert_eq!(schema["verdict"].field_type, "enum");
        assert_eq!(
            schema["verdict"].allowed,
            Some(vec!["PASS".into(), "FAIL".into()])
        );
        assert_eq!(schema["score"].field_type, "int");
    }

    #[test]
    fn resolve_switch_upstream_schema_returns_none_when_no_edge() {
        let yaml = with_start_end(
            r#"
name: unconnected-switch
nodes:
  - id: gate
    name: Gate
    type: switch
    inputs:
      - name: in
    outputs:
      - name: default
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let schema = resolve_switch_upstream_schema(&result.pipeline, "gate");
        assert!(schema.is_none());
    }

    #[test]
    fn resolve_switch_upstream_schema_returns_none_when_upstream_has_no_schema() {
        let yaml = with_start_end(
            r#"
name: untyped-switch
nodes:
  - id: reviewer
    name: Reviewer
    type: doc-only
    inputs:
      - name: code
    outputs:
      - name: review
  - id: gate
    name: Gate
    type: switch
    inputs:
      - name: in
    outputs:
      - name: default
edges:
  - source: { node: reviewer, port: review }
    target: { node: gate, port: in }
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let schema = resolve_switch_upstream_schema(&result.pipeline, "gate");
        assert!(schema.is_none());
    }

    #[test]
    fn rejects_switch_when_field_not_in_upstream_schema() {
        let yaml = with_start_end(
            r#"
name: bad-switch-when
nodes:
  - id: reviewer
    name: Reviewer
    type: doc-only
    inputs:
      - name: code
    outputs:
      - name: review
        frontmatter:
          verdict:
            type: enum
            allowed: [PASS, FAIL]
  - id: gate
    name: Gate
    type: switch
    inputs:
      - name: in
    outputs:
      - name: pass
        when:
          nonexistent_field: { eq: PASS }
      - name: default
edges:
  - source: { node: reviewer, port: review }
    target: { node: gate, port: in }
"#,
        );
        let err = parse_pipeline(&yaml).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("nonexistent_field") && msg.contains("not found in upstream schema"),
            "error should mention undeclared field: {msg}"
        );
    }

    #[test]
    fn rejects_switch_when_field_with_no_upstream_schema() {
        let yaml = with_start_end(
            r#"
name: untyped-upstream
nodes:
  - id: reviewer
    name: Reviewer
    type: doc-only
    inputs:
      - name: code
    outputs:
      - name: review
  - id: gate
    name: Gate
    type: switch
    inputs:
      - name: in
    outputs:
      - name: pass
        when:
          verdict: { eq: PASS }
      - name: default
edges:
  - source: { node: reviewer, port: review }
    target: { node: gate, port: in }
"#,
        );
        let err = parse_pipeline(&yaml).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("verdict") && msg.contains("not found in upstream schema"),
            "error should mention missing schema: {msg}"
        );
    }

    #[test]
    fn rejects_switch_when_field_with_no_upstream_edge() {
        let yaml = with_start_end(
            r#"
name: no-edge
nodes:
  - id: gate
    name: Gate
    type: switch
    inputs:
      - name: in
    outputs:
      - name: pass
        when:
          verdict: { eq: PASS }
      - name: default
"#,
        );
        let err = parse_pipeline(&yaml).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("verdict") && msg.contains("not found in upstream schema"),
            "error should mention missing upstream: {msg}"
        );
    }

    #[test]
    fn accepts_switch_when_field_matching_upstream_schema() {
        let yaml = with_start_end(
            r#"
name: valid-switch
nodes:
  - id: reviewer
    name: Reviewer
    type: doc-only
    inputs:
      - name: code
    outputs:
      - name: review
        frontmatter:
          verdict:
            type: enum
            allowed: [PASS, FAIL]
          score:
            type: int
  - id: gate
    name: Gate
    type: switch
    inputs:
      - name: in
    outputs:
      - name: pass
        when:
          verdict: { eq: PASS }
          score: { gte: 7 }
      - name: default
edges:
  - source: { node: reviewer, port: review }
    target: { node: gate, port: in }
"#,
        );
        let result = parse_pipeline(&yaml);
        assert!(result.is_ok(), "should accept valid schema fields");
    }

    #[test]
    fn accepts_switch_when_with_variable_ref() {
        let yaml = with_start_end(
            r#"
name: var-switch
variables:
  threshold: 7
nodes:
  - id: gate
    name: Gate
    type: switch
    inputs:
      - name: in
    outputs:
      - name: pass
        when:
          $threshold: { gte: 5 }
      - name: default
"#,
        );
        let result = parse_pipeline(&yaml);
        assert!(result.is_ok(), "should accept $variable references");
    }

    #[test]
    fn rejects_switch_when_with_undeclared_variable_ref() {
        let yaml = with_start_end(
            r#"
name: bad-var-switch
nodes:
  - id: gate
    name: Gate
    type: switch
    inputs:
      - name: in
    outputs:
      - name: pass
        when:
          $nonexistent: { eq: 5 }
      - name: default
"#,
        );
        let err = parse_pipeline(&yaml).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("$nonexistent"),
            "error should mention undeclared variable: {msg}"
        );
    }

    #[test]
    fn accepts_switch_with_no_when_clauses() {
        let yaml = with_start_end(
            r#"
name: empty-switch
nodes:
  - id: gate
    name: Gate
    type: switch
    inputs:
      - name: in
    outputs:
      - name: default
"#,
        );
        let result = parse_pipeline(&yaml);
        assert!(result.is_ok(), "switch with only default should be valid");
    }

    #[test]
    fn resolve_switch_upstream_schema_returns_none_for_non_switch_node() {
        let yaml = with_start_end(
            r#"
name: not-a-switch
nodes:
  - id: planner
    name: Planner
    type: doc-only
    inputs:
      - name: task
    outputs:
      - name: plan
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let schema = resolve_switch_upstream_schema(&result.pipeline, "planner");
        assert!(schema.is_none());
    }

    #[test]
    fn foreach_node_type_is_refused_at_parse() {
        // Not warn+coerce: silently rewriting a fan-out node to doc-only would
        // run each item's work zero times. The error names the migration path.
        for t in ["for-each", "foreach"] {
            let yaml = format!(
                r#"
name: test
nodes:
  - id: fe1
    name: per-item
    type: {t}
    inputs:
      - name: in
      - name: break
    outputs:
      - name: body
      - name: done
edges: []
"#
            );
            let err = parse_pipeline(&yaml).unwrap_err();
            let msg = err.to_string();
            assert!(
                msg.contains("removed") && msg.contains("pdo migrate"),
                "refusal must name `pdo migrate`, got: {msg}"
            );
        }
    }

    fn node_value(yaml: &str) -> serde_yaml::Value {
        serde_yaml::from_str(yaml).unwrap()
    }

    fn type_str(v: &serde_yaml::Value) -> Option<String> {
        v.as_mapping()
            .and_then(|m| m.get(serde_yaml::Value::String("type".into())))
            .and_then(|t| t.as_str())
            .map(str::to_string)
    }

    #[test]
    fn normalize_node_value_defaults_missing_type() {
        let mut v = node_value("id: n1\nname: X\n");
        let diags = normalize_node_value(&mut v).unwrap();
        assert_eq!(diags.len(), 1);
        assert!(
            diags[0].message.contains("missing 'type'"),
            "{}",
            diags[0].message
        );
        assert_eq!(type_str(&v).as_deref(), Some("doc-only"));
    }

    #[test]
    fn normalize_node_value_coerces_unknown_type() {
        let mut v = node_value("id: n1\nname: X\ntype: bogus\n");
        let diags = normalize_node_value(&mut v).unwrap();
        assert_eq!(diags.len(), 1);
        assert!(
            diags[0].message.contains("unknown node type 'bogus'"),
            "{}",
            diags[0].message
        );
        assert_eq!(type_str(&v).as_deref(), Some("doc-only"));
    }

    #[test]
    fn normalize_node_value_leaves_known_types_untouched() {
        for t in VALID_NODE_TYPES {
            let mut v = node_value(&format!("id: n1\nname: X\ntype: {t}\n"));
            let diags = normalize_node_value(&mut v).unwrap();
            assert!(diags.is_empty(), "type {t} should not warn");
            assert_eq!(type_str(&v).as_deref(), Some(*t));
        }
    }

    #[test]
    fn normalize_node_value_refuses_foreach() {
        for t in ["for-each", "foreach"] {
            let mut v = node_value(&format!("id: n1\nname: X\ntype: {t}\n"));
            let err = normalize_node_value(&mut v).unwrap_err().to_string();
            assert!(
                err.contains("removed") && err.contains("pdo migrate"),
                "{err}"
            );
        }
    }

    #[test]
    fn normalize_node_value_ignores_non_mapping() {
        // A stray non-mapping element (e.g. a scalar in the nodes list) is a
        // no-op: no diagnostics, no mutation.
        let mut v = node_value("- a scalar");
        let before = v.clone();
        let diags = normalize_node_value(&mut v).unwrap();
        assert!(diags.is_empty());
        assert_eq!(v, before);
    }

    #[test]
    fn normalize_node_value_matches_parse_pipeline_diagnostic() {
        // The per-node pass must emit exactly the diagnostic the full pipeline
        // parse emits for the same node — one source of truth.
        let mut node = node_value("id: n1\nname: X\ntype: bogus\n");
        let node_diags = normalize_node_value(&mut node).unwrap();
        let yaml = with_start_end(
            r#"
name: t
nodes:
  - id: n1
    name: X
    type: bogus
edges: []
"#,
        );
        let msgs: Vec<String> = parse_pipeline(&yaml)
            .unwrap()
            .diagnostics
            .iter()
            .map(|d| d.message.clone())
            .collect();
        assert!(
            msgs.iter().any(|m| *m == node_diags[0].message),
            "parse_pipeline diagnostics {msgs:?} must include {:?}",
            node_diags[0].message
        );
    }

    #[test]
    fn parses_node_model_and_round_trips() {
        let yaml = with_start_end(
            r#"
name: per-node-model
nodes:
  - id: ab000001
    name: implementer
    type: code-mutating
    model: opus
    outputs:
      - name: code
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let node = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab000001")
            .unwrap();
        // A flat `model:` is folded into `harnesses.claude.model` on parse.
        assert_eq!(
            node.harnesses
                .get("claude")
                .and_then(|e| e.model.as_deref()),
            Some("opus")
        );

        let serialized = serde_yaml::to_string(&result.pipeline).unwrap();
        let result2 = parse_pipeline(&serialized).unwrap();
        let node2 = result2
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab000001")
            .unwrap();
        assert_eq!(
            node2
                .harnesses
                .get("claude")
                .and_then(|e| e.model.as_deref()),
            Some("opus")
        );
    }

    #[test]
    fn node_model_defaults_to_none() {
        // A node with no `model:` parses to `None` and is never serialized — the
        // absence gate that keeps an unset node byte-identical to a library twin
        // with no model (the "diverged forever" guard).
        let yaml = with_start_end(
            r#"
name: no-model
nodes:
  - id: ab000001
    name: implementer
    type: code-mutating
    outputs:
      - name: code
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let node = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab000001")
            .unwrap();
        assert!(node.harnesses.is_empty());
        // Absent ⇒ never serialized (clean file, round-trips by absence).
        let serialized = serde_yaml::to_string(&result.pipeline).unwrap();
        assert!(!serialized.contains("model:"));
        assert!(!serialized.contains("harnesses:"));
    }

    #[test]
    fn parses_node_effort_and_round_trips() {
        // `effort:` is pass-through, deliberately not an enum: a value the wire
        // does not know must reach the harness verbatim rather than fail the whole
        // pipeline parse.
        let yaml = with_start_end(
            r#"
name: per-node-effort
nodes:
  - id: ab000001
    name: implementer
    type: code-mutating
    model: opus
    effort: low
    outputs:
      - name: code
  - id: ab000002
    name: exotic
    type: doc-only
    effort: turbo
    outputs:
      - name: notes
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        // A flat `effort:` is folded into `harnesses.claude.effort` on parse.
        let find = |p: &PipelineDef, id: &str| {
            p.nodes
                .iter()
                .find(|n| n.id == id)
                .and_then(|n| n.harnesses.get("claude").and_then(|e| e.effort.clone()))
        };
        assert_eq!(find(&result.pipeline, "ab000001").as_deref(), Some("low"));
        assert_eq!(
            result
                .pipeline
                .nodes
                .iter()
                .find(|n| n.id == "ab000001")
                .unwrap()
                .harnesses
                .get("claude")
                .and_then(|e| e.model.as_deref()),
            Some("opus")
        );
        // An unknown level parses fine (pass-through, no closed enum).
        assert_eq!(find(&result.pipeline, "ab000002").as_deref(), Some("turbo"));

        let serialized = serde_yaml::to_string(&result.pipeline).unwrap();
        let result2 = parse_pipeline(&serialized).unwrap();
        assert_eq!(find(&result2.pipeline, "ab000001").as_deref(), Some("low"));
        assert_eq!(
            find(&result2.pipeline, "ab000002").as_deref(),
            Some("turbo")
        );
    }

    #[test]
    fn node_effort_defaults_to_none() {
        // A node with no `effort:` parses to `None` and is never serialized — the
        // absence gate that keeps an unset node byte-identical to a library twin
        // with no effort, and the launch command identical to the legacy path.
        let yaml = with_start_end(
            r#"
name: no-effort
nodes:
  - id: ab000001
    name: implementer
    type: code-mutating
    outputs:
      - name: code
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let node = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab000001")
            .unwrap();
        assert!(node.harnesses.is_empty());
        // Absent ⇒ never serialized (clean file, round-trips by absence).
        let serialized = serde_yaml::to_string(&result.pipeline).unwrap();
        assert!(!serialized.contains("effort:"));
    }

    #[test]
    fn round_trip_frontmatter_output_port() {
        let yaml = with_start_end(
            r#"
name: frontmatter-rt
nodes:
  - id: reviewer
    name: reviewer
    type: doc-only
    inputs:
      - name: code
    outputs:
      - name: review
        frontmatter:
          verdict:
            type: enum
            allowed: [PASS, FAIL]
          score:
            type: int
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let serialized = serde_yaml::to_string(&result.pipeline).unwrap();
        let result2 = parse_pipeline(&serialized).unwrap();
        let reviewer = result2
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "reviewer")
            .unwrap();
        let out = &reviewer.outputs[0];
        let fm = out.frontmatter.as_ref().unwrap();
        assert_eq!(fm["verdict"].field_type, "enum");
        assert_eq!(
            fm["verdict"].allowed.as_deref(),
            Some(&["PASS".into(), "FAIL".into()][..])
        );
        assert_eq!(fm["score"].field_type, "int");
    }

    #[test]
    fn round_trip_switch_with_when_clauses() {
        let yaml = with_start_end(
            r#"
name: switch-rt
nodes:
  - id: reviewer
    name: reviewer
    type: doc-only
    inputs:
      - name: code
    outputs:
      - name: review
        frontmatter:
          verdict:
            type: enum
            allowed: [PASS, APPROVED, FAIL]
  - id: gate
    name: gate
    type: switch
    inputs:
      - name: in
    outputs:
      - name: pass
        when:
          verdict:
            in: [PASS, APPROVED]
      - name: rework
        when:
          verdict:
            eq: FAIL
edges:
  - source: { node: start, port: user_prompt }
    target: { node: reviewer, port: code }
  - source: { node: reviewer, port: review }
    target: { node: gate, port: in }
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let serialized = serde_yaml::to_string(&result.pipeline).unwrap();
        let result2 = parse_pipeline(&serialized).unwrap();
        let gate = result2
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "gate")
            .unwrap();
        let pass_port = gate.outputs.iter().find(|p| p.name == "pass").unwrap();
        assert!(pass_port.when.is_some());
        let rework_port = gate.outputs.iter().find(|p| p.name == "rework").unwrap();
        assert!(rework_port.when.is_some());
    }

    #[test]
    fn round_trip_multi_field_when_clause() {
        let yaml = with_start_end(
            r#"
name: multi-when-rt
nodes:
  - id: reviewer
    name: reviewer
    type: doc-only
    inputs:
      - name: code
    outputs:
      - name: review
        frontmatter:
          verdict:
            type: enum
            allowed: [PASS, FAIL]
          complexity_score:
            type: int
  - id: gate
    name: gate
    type: switch
    inputs:
      - name: in
    outputs:
      - name: pass
        when:
          verdict:
            eq: PASS
          complexity_score:
            lt: 3
edges:
  - source: { node: start, port: user_prompt }
    target: { node: reviewer, port: code }
  - source: { node: reviewer, port: review }
    target: { node: gate, port: in }
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let serialized = serde_yaml::to_string(&result.pipeline).unwrap();
        let result2 = parse_pipeline(&serialized).unwrap();
        let gate = result2
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "gate")
            .unwrap();
        let pass_port = gate.outputs.iter().find(|p| p.name == "pass").unwrap();
        let when = pass_port.when.as_ref().unwrap();
        assert!(when.as_mapping().unwrap().len() >= 2);
    }

    #[test]
    fn port_type_defaults_to_markdown() {
        let yaml = r#"
name: test
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: worker
    name: Worker
    type: doc-only
    inputs:
      - name: task
    outputs:
      - name: out
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: worker, port: task }
  - source: { node: worker, port: out }
    target: { node: end, port: result }
"#;
        let result = parse_pipeline(yaml).unwrap();
        let worker = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "worker")
            .unwrap();
        assert_eq!(worker.outputs[0].port_type, PortType::Markdown);
    }

    #[test]
    fn port_type_image_deserializes() {
        let yaml = r#"
name: test
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: designer
    name: Designer
    type: doc-only
    inputs:
      - name: task
    outputs:
      - name: screenshot
        port_type: image
      - name: gallery
        port_type: image_list
      - name: report
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: designer, port: task }
  - source: { node: designer, port: report }
    target: { node: end, port: result }
"#;
        let result = parse_pipeline(yaml).unwrap();
        let designer = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "designer")
            .unwrap();
        assert_eq!(designer.outputs[0].name, "screenshot");
        assert_eq!(designer.outputs[0].port_type, PortType::Image);
        assert_eq!(designer.outputs[1].name, "gallery");
        assert_eq!(designer.outputs[1].port_type, PortType::ImageList);
        assert_eq!(designer.outputs[2].name, "report");
        assert_eq!(designer.outputs[2].port_type, PortType::Markdown);
    }

    #[test]
    fn port_type_html_deserializes_and_round_trips() {
        // A `port_type: html` output survives load → save: mapped to PortType::Html
        // (not the markdown default) and emitted back as the bare scalar `html`.
        let yaml = r#"
name: test
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: designer
    name: Designer
    type: doc-only
    inputs:
      - name: task
    outputs:
      - name: report
        port_type: html
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: designer, port: task }
  - source: { node: designer, port: report }
    target: { node: end, port: result }
"#;
        let result = parse_pipeline(yaml).unwrap();
        let designer = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "designer")
            .unwrap();
        assert_eq!(designer.outputs[0].name, "report");
        assert_eq!(designer.outputs[0].port_type, PortType::Html);

        // Enum-level serde round-trip: html <-> "html".
        let rendered = serde_yaml::to_string(&PortType::Html).unwrap();
        assert_eq!(rendered.trim(), "html");
        let back: PortType = serde_yaml::from_str("html").unwrap();
        assert_eq!(back, PortType::Html);
    }

    #[test]
    fn port_type_image_list_deserializes_on_input_port() {
        // The frontend emits `port_type: image_list` on INPUT ports too; the daemon
        // read path must round-trip it to PortType::ImageList, not the markdown
        // default.
        let yaml = r#"
name: test
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: tester
    name: Tester
    type: doc-only
    inputs:
      - name: screens
        port_type: image_list
      - name: notes
    outputs:
      - name: screens-fixed
        port_type: image_list
      - name: result
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: tester, port: notes }
  - source: { node: tester, port: result }
    target: { node: end, port: result }
"#;
        let result = parse_pipeline(yaml).unwrap();
        let tester = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "tester")
            .unwrap();
        assert_eq!(tester.inputs[0].name, "screens");
        assert_eq!(tester.inputs[0].port_type, PortType::ImageList);
        assert_eq!(tester.inputs[1].name, "notes");
        assert_eq!(tester.inputs[1].port_type, PortType::Markdown);
        assert_eq!(tester.outputs[0].name, "screens-fixed");
        assert_eq!(tester.outputs[0].port_type, PortType::ImageList);
        assert_eq!(tester.outputs[1].name, "result");
        assert_eq!(tester.outputs[1].port_type, PortType::Markdown);
    }

    #[test]
    fn prompt_required_defaults_to_true_when_absent() {
        let yaml = with_start_end(
            r#"
name: no-flag
nodes: []
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        assert!(result.pipeline.prompt_required);
    }

    #[test]
    fn prompt_required_false_round_trips() {
        let yaml = with_start_end(
            r#"
name: optional-prompt
prompt_required: false
nodes: []
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        assert!(!result.pipeline.prompt_required);

        let serialized = serde_yaml::to_string(&result.pipeline).unwrap();
        let result2 = parse_pipeline(&serialized).unwrap();
        assert!(!result2.pipeline.prompt_required);
    }

    #[test]
    fn prompt_required_true_is_not_serialized() {
        // The default round-trips by absence, keeping prompt-required pipelines
        // (the common case) clean in YAML — same convention as `loops`.
        let yaml = with_start_end(
            r#"
name: default-flag
nodes: []
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let serialized = serde_yaml::to_string(&result.pipeline).unwrap();
        assert!(!serialized.contains("prompt_required"));
    }

    #[test]
    fn prompt_required_false_emits_no_unknown_field_diagnostic() {
        // prompt_required is a legitimate field; the unknown-field lint must not
        // warn about it.
        let yaml = with_start_end(
            r#"
name: optional-prompt
prompt_required: false
nodes: []
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        assert!(
            result.diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            result.diagnostics
        );
        assert!(!result.pipeline.prompt_required);
    }

    #[test]
    fn deterministic_output_instructions_are_preserved_with_a_non_blocking_diagnostic() {
        let yaml = VALID_MINIMAL.replace(
            "      - name: user_prompt",
            "      - name: user_prompt\n        instructions: Keep this guidance.",
        );
        let result = parse_pipeline(&yaml).unwrap();

        assert_eq!(
            result.pipeline.nodes[0].outputs[0].instructions.as_deref(),
            Some("Keep this guidance.")
        );
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.severity == Severity::Warning
                && diagnostic.message.contains("deterministic node 'start'")
                && diagnostic.message.contains("output 'user_prompt'")
        }));
    }

    #[test]
    fn truly_unknown_field_still_warns() {
        let yaml = with_start_end(
            r#"
name: with-bogus
bogus_field: 1
nodes: []
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        assert_eq!(result.diagnostics.len(), 1);
        assert_eq!(
            result.diagnostics[0].message,
            "unknown field 'bogus_field' (ignored)"
        );
    }

    #[test]
    fn known_keys_cover_serialized_pipeline() {
        // Drift guard: every top-level key PipelineDef can serialize must be in
        // KNOWN_TOP_LEVEL_KEYS, or the lint false-positives on the next added field.
        // The fixture sets every optional field to a non-default value so all
        // skip_serializing_if fields are actually emitted.
        let yaml = r#"
name: full-fixture
version: "1.0"
variables:
  max_iter_review: 3
prompt_required: false
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: ab000001
    name: implementer
    type: code-mutating
    inputs:
      - name: task
    outputs:
      - name: code
  - id: ab000002
    name: reviewer
    type: doc-only
    inputs:
      - name: code
    outputs:
      - name: review
        frontmatter:
          verdict:
            type: enum
            allowed: [PASS, FAIL]
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: ab000001, port: task }
  - source: { node: ab000001, port: code }
    target: { node: ab000002, port: code }
  # Conditional routing on the EDGE (ADR-0011) — a legacy `switch` node here
  # would trip the "run `pdo migrate`" diagnostic (#396) and make this fixture
  # lint-dirty for a reason unrelated to what it guards.
  - source: { node: ab000002, port: review }
    target: { node: end, port: result }
    when:
      verdict: { in: [PASS] }
loops:
  - id: review_loop
    kind: bounded
    members: [ab000001, ab000002]
    max_iter: "$max_iter_review"
notes:
  - id: note-1
    content: "remember to bound this loop"
    view:
      x: 120.0
      y: 240.0
"#;
        let result = parse_pipeline(yaml).unwrap();
        assert!(
            result.diagnostics.is_empty(),
            "fixture must lint clean: {:?}",
            result.diagnostics
        );

        let serialized = serde_yaml::to_string(&result.pipeline).unwrap();
        let value: serde_yaml::Value = serde_yaml::from_str(&serialized).unwrap();
        let mapping = value.as_mapping().unwrap();
        for key in mapping.keys() {
            let k = key.as_str().unwrap();
            assert!(
                KNOWN_TOP_LEVEL_KEYS.contains(&k),
                "field '{k}' serialized but missing from KNOWN_TOP_LEVEL_KEYS — add it or the lint will false-positive (cf. #183)"
            );
        }
    }
}
