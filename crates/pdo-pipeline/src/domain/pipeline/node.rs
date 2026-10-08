//! A pipeline node: its type, its ports, where it sits and how it runs.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

fn deserialize_optional_non_blank<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<String>::deserialize(deserializer)?;
    Ok(value.filter(|text| !text.trim().is_empty()))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
#[serde(rename_all = "kebab-case")]
pub enum NodeType {
    /// The one agentic node type (#653 / ADR-0060). It replaces the retired pair
    /// of types, which named an *effect* while the runtime only ever read them as
    /// a working-directory decision. Where the NodeRun works is now the node's
    /// own `isolated_worktree` line, not a guess from its type.
    Agent,
    Start,
    End,
    Switch,
    Loop,
    /// A node that runs author-written bash deterministically instead of
    /// launching Claude (#248 / ADR-0017). Runs in a tmux session (attachable
    /// like any NodeRun) whose tail is `timeout N bash <body>`: exit 0 ⇒ node
    /// completes, non-zero/timeout ⇒ node fails. Shares the Run worktree by
    /// default and opts into isolation like any `agent` (#653).
    Script,
}

impl NodeType {
    /// A *regular* node (agent / script) declares no inputs: its inputs are
    /// emergent, derived from incoming edges and named after the edge target
    /// port (#149 / ADR-0011). Structural nodes (start/end/switch/loop)
    /// keep their required, declared input ports. A `script` node consumes whole
    /// artifacts by edge just like a work node, so its inputs are emergent too
    /// (#248).
    pub fn has_emergent_inputs(&self) -> bool {
        matches!(self, NodeType::Agent | NodeType::Script)
    }

    /// The isolation this type carries when the document says nothing (#653 /
    /// ADR-0060): an `agent` is isolated, a `script` shares the Run worktree,
    /// and every other type carries no isolation setting at all — structural
    /// nodes never run in a worktree of their own. `None` is what keeps
    /// `isolated_worktree` off a Start/End document instead of writing a line
    /// nobody may edit.
    pub fn default_isolation(&self) -> Option<bool> {
        match self {
            NodeType::Agent => Some(true),
            NodeType::Script => Some(false),
            _ => None,
        }
    }

    /// The wire/YAML spelling of this type — the single place the strings live.
    pub fn as_str(&self) -> &'static str {
        match self {
            NodeType::Agent => "agent",
            NodeType::Start => "start",
            NodeType::End => "end",
            NodeType::Switch => "switch",
            NodeType::Loop => "loop",
            NodeType::Script => "script",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export, optional_fields = nullable)]
pub struct FrontmatterFieldDecl {
    #[serde(rename = "type")]
    pub field_type: String,
    #[serde(default)]
    pub allowed: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum PortType {
    #[default]
    Markdown,
    Image,
    ImageList,
    /// A rendered, static HTML artifact (`output.html`): a leaf review surface
    /// shown in a sandboxed iframe (no JS), never consumed downstream (ADR-0028).
    Html,
}

pub const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "gif"];

pub fn is_image_file(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| IMAGE_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "lowercase")]
pub enum PortSide {
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export, optional_fields = nullable, rename = "PortDef")]
pub struct Port {
    pub name: String,
    #[serde(default)]
    pub repeated: bool,
    #[serde(default)]
    pub side: Option<PortSide>,
    #[serde(default)]
    // Always sent, but a port the editor creates omits it (markdown by default).
    #[ts(optional = nullable)]
    pub port_type: PortType,
    #[serde(default)]
    pub frontmatter: Option<HashMap<String, FrontmatterFieldDecl>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional = nullable, type = "Record<string, unknown> | null")]
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
    #[ts(optional = nullable)]
    pub required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct ViewPosition {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, optional_fields = nullable, rename = "ParsedNodeDef")]
pub struct NodeDef {
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
    #[ts(optional = nullable, type = "number | string | null")]
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
    #[ts(optional = nullable)]
    pub harnesses: BTreeMap<String, pdo_harness::resolver::HarnessEntry>,
    /// The node's tier of the agentic-profile union (ADR-0057). `Some(Profile |
    /// Custom)` supplies harness, model and effort atomically, and
    /// [`pin_harness`]/[`harnesses`] are then NOT consulted (ADR-0057 ¶1: Custom
    /// "ne réactive pas l'ancien résolveur"). Absent or `Some(Inherit)` preserves
    /// the legacy behaviour exactly. Semantic, not layout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_choice: Option<pdo_harness::agent_choice::AgentChoice>,
    /// The Node tier of the **skills** selection (#669, ADR-0062): the skills this
    /// node's NodeRun receives on top of the instance, Projet and Run tiers
    /// (strict additive union, `skill_selection::resolve`). Referenced by **id**;
    /// the `name` beside it is a readable label the bank re-reads at spawn.
    /// Semantic, not layout — in the diff and the library content hash. Refused
    /// on a `script` node at parse ([`normalize_node_value`]): a script launches
    /// no agent, so a skill there would be a setting that silently does nothing.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional = nullable)]
    pub skills: Vec<pdo_skill::domain::selection::SkillRef>,
    /// The **finest** tier of [`crate::auto_fail::resolve_auto_fail`] (`node → Run
    /// → Projet → instance`): `Some(_)` overrides every coarser tier for this
    /// node's `pdo fail`. Semantic, not layout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_fail: Option<bool>,
    /// Where this node's NodeRun works (#653, ADR-0060). `true` ⇒ a sub-worktree
    /// of its own; `false` ⇒ the Run's shared worktree. Carried only by `agent`
    /// and `script`, where [`normalize_node_value`] fills the type's default
    /// (`agent` isolated, `script` shared) so the parsed document ALWAYS states
    /// the choice — including when it equals the default. `None` on structural
    /// nodes (no worktree of their own). Semantic, not layout: moving a node between worktrees changes what
    /// the pipeline does, so it enters the diff and the library content hash.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isolated_worktree: Option<bool>,
    /// The « Orchestrator » toggle (#723, ADR-0064). `true` ⇒ the node's NodeRun
    /// is held by the strong orchestrator↔children binding (#724): the completion
    /// gate refuses `pdo complete` while the child runs spawned from this node's
    /// sessions (same `parent_run_id` + `parent_node_id`) are non-terminal. It is
    /// NOT a node type — the spawn capacity stays universal; the toggle only adds
    /// the binding contract (plus the #723 prompt amendment and skill reference).
    /// Semantic, not layout — in the diff and the library content hash.
    #[serde(default, skip_serializing_if = "is_false")]
    #[ts(optional = nullable)]
    pub orchestrator: bool,
    /// The « on demand » option (ADR-0086): the runtime never launches this
    /// node (not as a root, not through an incoming edge) and never skips it;
    /// only the user's ▶ does. Its edges hold no downstream node until it is
    /// launched. An option of an `agent` or a `script`, refused on a structural
    /// node at parse ([`super::normalize::normalize_node_value`]). Semantic,
    /// not layout — in the diff and the library content hash.
    #[serde(default, skip_serializing_if = "is_false")]
    #[ts(optional = nullable)]
    pub on_demand: bool,
}

impl NodeDef {
    /// Whether this node's NodeRun gets a sub-worktree of its own (#653,
    /// ADR-0060). `agent`/`script` read their explicit line, falling back to the type's
    /// default for a `NodeDef` built in code rather than parsed; every other
    /// type runs in the Run worktree.
    ///
    /// This is the *document's* answer. A live NodeRun reads its FROZEN answer
    /// off `NodeStarted` instead — editing the graph never moves a running
    /// iteration between worktrees.
    pub fn is_isolated(&self) -> bool {
        match self.node_type {
            NodeType::Agent | NodeType::Script => self
                .isolated_worktree
                .or_else(|| self.node_type.default_isolation())
                .unwrap_or(false),
            _ => false,
        }
    }
}

/// The TypeScript shape of a pipeline node over HTTP: the parsed [`NodeDef`] plus
/// the node's `provisioning` rules, which the YAML carries and `GET
/// /pipelines/{id}` copies into the JSON (`pipeline_json_with_node_provisioning`).
#[derive(TS)]
#[ts(export, rename = "NodeDef")]
#[allow(dead_code)]
pub(crate) struct NodeDefWire {
    #[ts(flatten)]
    def: NodeDef,
    #[ts(optional)]
    provisioning: Option<pdo_workspace::domain::provisioning::ProvisioningRules>,
}

pub(crate) fn is_false(b: &bool) -> bool {
    !*b
}
