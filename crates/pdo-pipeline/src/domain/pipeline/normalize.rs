//! The per-node normalization every reader shares: the node type, the
//! isolation default, the skills refusal on a script, the legacy flat
//! `model`/`effort` fold.

use super::diagnostic::{Diagnostic, ParseError, Severity};

/// The node `type` strings the parser accepts verbatim. Anything else is coerced
/// to `agent` with a warning (see `normalize_node_value`); `for-each`/`foreach`
/// are refused outright (retired in ADR-0011).
///
/// #653 / ADR-0060: `doc-only` and `code-mutating` are GONE, with no alias and no
/// migrator. They now take the same generic path as any other invalid value —
/// coerced to `agent` with the unknown-type warning — because a bespoke
/// diagnostic for a retired type is a compatibility branch by another name.
///
/// ADR-0082: `merge` is GONE the same way. A legacy `type: merge` becomes an
/// isolated `agent` with the ordinary warning — its `branches` / `merged` ports
/// survive untouched, and convergence is the generic rule of every node.
pub const VALID_NODE_TYPES: &[&str] = &[
    "agent", "start", "end", "switch", "loop",
    // #248: without this, a `type: script` node is silently rewritten to
    // `agent` with only a diagnostic (see the layer-1 test).
    "script",
];

/// Normalize one node's raw YAML `type` field in place, returning any warnings.
/// This is the per-node pass `parse_pipeline` runs over each element of the
/// `nodes:` sequence, factored out so the single-node `POST /nodes/parse`
/// endpoint (#345) shares exactly the same coercion rules — one source of truth
/// for node-type handling:
///
/// - `for-each` / `foreach` → hard `Err` (the type was removed in ADR-0011;
///   coercing a fan-out to `agent` would run each item's work zero times).
/// - missing `type` → default to `agent` (noisy warning).
/// - unknown `type` → coerce to `agent` (noisy warning). This is the arm the
///   retired `doc-only` / `code-mutating` (#653) and `merge` (ADR-0082) land in.
/// - a known type (incl. legacy `switch`/`loop`) → left untouched, no warning.
///
/// It then stamps the node's isolation default (#653): an `agent`/`script` with
/// no `isolated_worktree` key gets its type's default written in, so a parsed
/// document always states where the node works.
///
/// A non-mapping value (e.g. a stray scalar in the `nodes:` list) is a no-op.
pub fn normalize_node_value(
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
        // ForEach is RETIRED (ADR-0011 / #269): a hard refusal, not the
        // warn+coerce below — silently rewriting a fan-out node to a work node
        // would run each item's work zero times.
        Some(t @ ("for-each" | "foreach")) => {
            return Err(ParseError::MissingField(format!(
                "node '{node_id}': node type '{t}' was removed (ADR-0011) — \
                 run `pdo migrate` to convert it to a collection loop region"
            )));
        }
        None => {
            diagnostics.push(Diagnostic {
                severity: Severity::Warning,
                message: format!("node '{node_id}': missing 'type', defaulting to 'agent'"),
            });
            node_map.insert(type_key, serde_yaml::Value::String("agent".into()));
        }
        Some(t) if !VALID_NODE_TYPES.contains(&t) => {
            diagnostics.push(Diagnostic {
                severity: Severity::Warning,
                message: format!(
                    "node '{node_id}': unknown node type '{t}', defaulting to 'agent'"
                ),
            });
            node_map.insert(type_key, serde_yaml::Value::String("agent".into()));
        }
        _ => {}
    }

    stamp_isolation_default(node_map);

    // #669 / ADR-0062: a `script` node runs deterministic bash and launches no
    // agent, so a `skills` selection on it can never take effect. Refused rather
    // than dropped: a setting that silently does nothing is the failure mode the
    // spec names (« un réglage sans signification ne peut exister en silence »).
    if node_map
        .get(serde_yaml::Value::String("type".into()))
        .and_then(|v| v.as_str())
        .is_some_and(|t| t == "script")
    {
        if let Some(skills) = node_map.get(serde_yaml::Value::String(SKILLS_KEY.into())) {
            let is_empty_list = skills.as_sequence().is_some_and(|s| s.is_empty());
            if !skills.is_null() && !is_empty_list {
                return Err(ParseError::InvalidField(format!(
                    "node '{node_id}': a `script` node cannot carry `skills` — a script \
                     launches no agent, so no skill would ever be read. Remove the \
                     `skills` key, or make the node an `agent`"
                )));
            }
        }
    }

    // ADR-0086: « on demand » is an option of a node that runs something. On a
    // structural node (Start, End, Loop, Switch) nothing could ever be launched
    // by the ▶, so the option is refused rather than carried in silence.
    let on_demand = node_map
        .get(serde_yaml::Value::String(ON_DEMAND_KEY.into()))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if on_demand {
        let node_type = node_map
            .get(serde_yaml::Value::String("type".into()))
            .and_then(|v| v.as_str())
            .unwrap_or("agent");
        if !matches!(node_type, "agent" | "script") {
            return Err(ParseError::InvalidField(format!(
                "node '{node_id}': a `{node_type}` node cannot be `on_demand` — only an \
                 `agent` or a `script` node can wait for the user's ▶. Remove the \
                 `on_demand` key"
            )));
        }
    }

    // ADR-0089: a legacy output marked `intermediate` is a live artifact. Read
    // as one here, so every reader sees the same node; the file is rewritten
    // only when it is saved.
    diagnostics.extend(migrate_intermediate_outputs(node_map, &node_id));

    // ADR-0089: live artifacts belong to a node that runs something. On a
    // structural node nothing would ever publish them: refused, like
    // `on_demand`, rather than carried in silence.
    let node_type = node_map
        .get(serde_yaml::Value::String("type".into()))
        .and_then(|v| v.as_str())
        .unwrap_or("agent")
        .to_string();
    if !matches!(node_type.as_str(), "agent" | "script") {
        let declares = node_map
            .get(serde_yaml::Value::String(LIVE_ARTIFACTS_KEY.into()))
            .and_then(|v| v.as_sequence())
            .is_some_and(|s| !s.is_empty());
        let free = node_map
            .get(serde_yaml::Value::String(FREE_LIVE_ARTIFACTS_KEY.into()))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if declares || free {
            return Err(ParseError::InvalidField(format!(
                "node '{node_id}': a `{node_type}` node cannot publish live artifacts — only \
                 an `agent` or a `script` node works while the Run goes. Remove the \
                 `{LIVE_ARTIFACTS_KEY}` and `{FREE_LIVE_ARTIFACTS_KEY}` keys"
            )));
        }
    }

    // #550: fold a legacy flat `model:` / `effort:` into `harnesses.claude.*` so
    // that a parse is lossless even before the on-disk migrator has rewritten the
    // file. The migrator persists the same fold; this is the in-memory safety net
    // that keeps `harnesses` the single source the resolver reads.
    fold_flat_model_effort_into_harnesses(node_map);

    Ok(diagnostics)
}

/// The node key that says where a NodeRun works (#653, ADR-0060).
pub const ISOLATED_WORKTREE_KEY: &str = "isolated_worktree";

/// The node key of the « on demand » option (ADR-0086).
pub const ON_DEMAND_KEY: &str = "on_demand";

/// The node key of its declared live artifacts (ADR-0089).
pub const LIVE_ARTIFACTS_KEY: &str = "live_artifacts";

/// The node key of the « Free live artifacts » option (ADR-0089).
pub const FREE_LIVE_ARTIFACTS_KEY: &str = "free_live_artifacts";

/// The key a legacy output carried to be an *Output intermédiaire*, read as a
/// live artifact since ADR-0089.
const INTERMEDIATE_KEY: &str = "intermediate";

/// Move every output marked `intermediate: true` of an `agent`/`script` node
/// into its `live_artifacts` (ADR-0089), keeping its name, type, expected
/// content and frontmatter schema. An `intermediate` key anywhere else (an
/// input, a structural node's output) meant nothing: dropped with a warning; a
/// `false` is dropped silently.
pub fn migrate_intermediate_outputs(
    node_map: &mut serde_yaml::Mapping,
    node_id: &str,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let key = serde_yaml::Value::String(INTERMEDIATE_KEY.into());
    let runs_something = matches!(
        node_map
            .get(serde_yaml::Value::String("type".into()))
            .and_then(|v| v.as_str()),
        Some("agent" | "script")
    );

    let mut moved: Vec<serde_yaml::Value> = Vec::new();
    for (role, ports_key) in [("input", "inputs"), ("output", "outputs")] {
        let Some(ports) = node_map
            .get_mut(serde_yaml::Value::String(ports_key.into()))
            .and_then(|v| v.as_sequence_mut())
        else {
            continue;
        };
        let mut kept = Vec::with_capacity(ports.len());
        for mut port in std::mem::take(ports) {
            let flagged = port
                .as_mapping_mut()
                .and_then(|m| m.remove(&key))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if !flagged {
                kept.push(port);
            } else if role == "output" && runs_something {
                moved.push(live_artifact_from_port(port));
            } else {
                let name = port
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("<unnamed>");
                diagnostics.push(Diagnostic {
                    severity: Severity::Warning,
                    message: format!(
                        "{role} '{name}' of node '{node_id}' is marked intermediate, which only \
                         applies to an output of an agent or script node (ignored)"
                    ),
                });
                kept.push(port);
            }
        }
        *ports = kept;
    }

    if !moved.is_empty() {
        let live_key = serde_yaml::Value::String(LIVE_ARTIFACTS_KEY.into());
        if !node_map.get(&live_key).is_some_and(|v| v.is_sequence()) {
            node_map.insert(live_key.clone(), serde_yaml::Value::Sequence(Vec::new()));
        }
        if let Some(live) = node_map
            .get_mut(&live_key)
            .and_then(|v| v.as_sequence_mut())
        {
            live.extend(moved);
        }
    }
    diagnostics
}

/// A legacy intermediate output's mapping as a live artifact: what a live
/// artifact keeps of a port (name, type, expected content, frontmatter schema).
fn live_artifact_from_port(port: serde_yaml::Value) -> serde_yaml::Value {
    let serde_yaml::Value::Mapping(port) = port else {
        return port;
    };
    let mut live = serde_yaml::Mapping::new();
    for field in ["name", "port_type", "instructions", "frontmatter"] {
        if let Some(value) = port.get(field) {
            live.insert(serde_yaml::Value::String(field.into()), value.clone());
        }
    }
    serde_yaml::Value::Mapping(live)
}

/// The node key carrying its skills selection (#669, ADR-0062).
pub const SKILLS_KEY: &str = "skills";

/// Write a node's isolation default into the raw YAML when the document is
/// silent (#653, ADR-0060).
///
/// This is what makes "the Document always states the choice" true without
/// asking every author to type the line: an `agent` with no `isolated_worktree`
/// parses as isolated and *re-serializes* saying so. A type that carries no
/// isolation (`start`, `end`, `switch`, `loop`) has any stray key dropped rather
/// than round-tripped — a line nobody may edit would only invite someone to
/// edit it.
///
/// Runs after the type normalization above, so it reads the coerced type.
fn stamp_isolation_default(node_map: &mut serde_yaml::Mapping) {
    let key = serde_yaml::Value::String(ISOLATED_WORKTREE_KEY.into());
    let default = match node_map
        .get(serde_yaml::Value::String("type".into()))
        .and_then(|v| v.as_str())
    {
        Some("agent") => Some(true),
        Some("script") => Some(false),
        _ => None,
    };
    match default {
        Some(value) => {
            if !node_map.get(&key).map(|v| v.is_bool()).unwrap_or(false) {
                node_map.insert(key, serde_yaml::Value::Bool(value));
            }
        }
        None => {
            node_map.remove(&key);
        }
    }
}

/// Fold a node's legacy flat `model:` / `effort:` keys (#296/#424) into
/// `harnesses.claude.{model, effort}` (#550, ADR-0046), removing the flat keys.
///
/// Shared by [`normalize_node_value`] and the pipeline migrator so both fold
/// identically. Returns `true` iff the raw YAML changed — the migrator's
/// `needs_migration` guard keys off it.
///
/// An explicit `harnesses.claude` entry is authoritative: a flat value fills a slot
/// only when the map has none, so a second run is a no-op.
pub fn fold_flat_model_effort_into_harnesses(node_map: &mut serde_yaml::Mapping) -> bool {
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

    let claude_key = serde_yaml::Value::String(pdo_harness::registry::CLAUDE.into());
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
