//! The layout/semantics partition, Rust side (#395).
//!
//! `frontend/src/lib/layoutFields.ts` is the **single owner** of which serialized
//! fields carry a pipeline's meaning (SEMANTIC) and which only describe its canvas
//! presentation (LAYOUT) — see #355 and CONTEXT.md. That partition landed on the
//! frontend only, so `library_store::pipelines::content_hash` went on hashing raw
//! YAML bytes: moving a node flipped the library badge to "out of sync" while the
//! canvas star, on the very same edit, still said "synced". Two contradictory
//! verdicts on one file.
//!
//! This module mirrors the LAYOUT half of that partition and projects a *parsed*
//! `PipelineDef` onto a canonical string: layout removed, field order fixed by the
//! projection structs, map keys sorted, and every parser normalization (port sides,
//! the switch `default` output, …) already baked in by `parse_pipeline`. Hashing
//! that string instead of the file's bytes is what makes the daemon agree with the
//! canvas — and it also absorbs the formatting churn a canvas save produces
//! (flow vs block style, quoting, key order), which a textual strip of `view:`
//! would not.
//!
//! Two tripwires keep the mirror honest:
//!
//!  - every projection is built by **destructuring** its source struct, so adding a
//!    field to `pipeline::{PipelineDef, NodeDef, Port, EdgeDef, EdgeEndpoint,
//!    LoopRegion, VariableDef}` fails to compile until it is classified here;
//!  - `layout_fields_match_frontend_owner` reads `layoutFields.ts` and asserts the
//!    LAYOUT sets are identical, scope by scope, name for name.
//!
//! The SEMANTIC half is deliberately *not* mirrored. The frontend's list enumerates
//! what its serializer emits, whereas the parse surface here carries a few extra
//! fields (`EdgeDef::reason`, `EdgeDef::repeated`, `Port::description`,
//! `NodeDef::over`). Those are content, not canvas presentation, so they count: a
//! Rust-side semantic *superset* can only make drift detection stricter, never
//! resurrect a false positive.
//!
//! Sequence order (`nodes`, `edges`, `loops`, `members`, `allowed`, `waypoints`) is
//! preserved as authored. The frontend compares those arrays with `deepEqual`,
//! which is order-sensitive; reordering here would re-open the very disagreement
//! this module closes.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::pipeline::{
    EdgeDef, EdgeEndpoint, FrontmatterFieldDecl, LoopKind, LoopRegion, NodeDef, NodeType,
    PipelineDef, Port, PortSide, PortType, VariableDef, VariableType,
};

/// Fields excluded from the projection, per serializer scope, spelled exactly as
/// in `frontend/src/lib/layoutFields.ts`. Scope names use the frontend's
/// `SerializerScope` spelling so the cross-language guard can match them.
///
/// GUARD-ONLY, like `SEMANTIC_FIELDS` on the frontend: the projection excludes
/// layout by *not destructuring it into a field*, never by name lookup. This table
/// exists so `layout_fields_match_frontend_owner` can compare the two partitions,
/// and so a reader can see the classification without tracing every `let _layout`.
#[allow(dead_code)]
pub(crate) const LAYOUT_FIELDS: &[(&str, &[&str])] = &[
    ("pipeline", &["notes"]),
    ("node", &["view"]),
    ("inputPort", &[]),
    ("outputPort", &[]),
    ("edge", &["mode", "waypoints", "target_side"]),
    ("loopRegion", &[]),
    // GUARD-ONLY, mirroring the frontend: the whole `notes` block is dropped at
    // pipeline scope (ADR-0018 R1), so the projection never descends into a note.
    ("note", &["id", "content", "view"]),
];

/// Canonical, deterministic rendering of `pipeline`'s semantics.
///
/// `Err` is reachable only in theory (`canon_yaml` leaves nothing serde_json can
/// reject); callers treat it like an unprojectable pipeline rather than papering
/// over it with a constant, which would make two different failures hash equal.
pub(crate) fn canonical_form(pipeline: &PipelineDef) -> Result<String, serde_json::Error> {
    serde_json::to_string(&PipelineProjection::of(pipeline))
}

#[derive(Serialize)]
struct PipelineProjection<'a> {
    name: &'a str,
    version: Option<&'a str>,
    prompt_required: bool,
    variables: BTreeMap<&'a str, VariableProjection<'a>>,
    nodes: Vec<NodeProjection<'a>>,
    edges: Vec<EdgeProjection<'a>>,
    loops: Vec<LoopProjection<'a>>,
}

impl<'a> PipelineProjection<'a> {
    fn of(pipeline: &'a PipelineDef) -> Self {
        let PipelineDef {
            name,
            version,
            variables,
            nodes,
            edges,
            loops,
            notes,
            prompt_required,
        } = pipeline;
        let _layout = notes; // LAYOUT_FIELDS["pipeline"] — whole block dropped
        Self {
            name,
            version: version.as_deref(),
            prompt_required: *prompt_required,
            variables: variables
                .iter()
                .map(|(k, v)| (k.as_str(), VariableProjection::of(v)))
                .collect(),
            nodes: nodes.iter().map(NodeProjection::of).collect(),
            edges: edges.iter().map(EdgeProjection::of).collect(),
            loops: loops.iter().map(LoopProjection::of).collect(),
        }
    }
}

#[derive(Serialize)]
struct VariableProjection<'a> {
    var_type: &'a VariableType,
    default: serde_json::Value,
}

impl<'a> VariableProjection<'a> {
    fn of(variable: &'a VariableDef) -> Self {
        let VariableDef { var_type, default } = variable;
        Self {
            var_type,
            default: canon_yaml(default),
        }
    }
}

#[derive(Serialize)]
struct NodeProjection<'a> {
    id: &'a str,
    name: &'a str,
    node_type: &'a NodeType,
    interactive: bool,
    model: Option<&'a str>,
    /// Per-node reasoning-effort override (#424). Semantic for the same reason as
    /// `model`: it changes how the agent behaves, so a pipeline that differs only
    /// by an effort level is *not* the same pipeline — the library drift badge and
    /// the pipeline diff must both see it.
    effort: Option<&'a str>,
    max_iter: Option<serde_json::Value>,
    /// Legacy per-node collection driver. The frontend serializer no longer emits
    /// it, so it is absent from `SEMANTIC_FIELDS.node`; it is still a behavioural
    /// field on the parse surface, hence semantic here.
    over: Option<&'a str>,
    inputs: Vec<PortProjection<'a>>,
    outputs: Vec<PortProjection<'a>>,
}

impl<'a> NodeProjection<'a> {
    fn of(node: &'a NodeDef) -> Self {
        let NodeDef {
            id,
            name,
            node_type,
            inputs,
            outputs,
            interactive,
            view,
            max_iter,
            over,
            model,
            effort,
        } = node;
        let _layout = view; // LAYOUT_FIELDS["node"]
        Self {
            id,
            name,
            node_type,
            interactive: *interactive,
            model: model.as_deref(),
            effort: effort.as_deref(),
            max_iter: max_iter.as_ref().map(canon_yaml),
            over: over.as_deref(),
            inputs: inputs.iter().map(PortProjection::of).collect(),
            outputs: outputs.iter().map(PortProjection::of).collect(),
        }
    }
}

#[derive(Serialize)]
struct PortProjection<'a> {
    name: &'a str,
    repeated: bool,
    /// Semantic, like on the frontend (#355 D5): the node-library star already
    /// treats port side as identity, so the pipeline diff must agree.
    side: Option<&'a PortSide>,
    port_type: &'a PortType,
    frontmatter: Option<BTreeMap<&'a str, &'a FrontmatterFieldDecl>>,
    when: Option<serde_json::Value>,
    /// Not emitted by the frontend serializer, but authored text — semantic here.
    description: Option<&'a str>,
}

impl<'a> PortProjection<'a> {
    fn of(port: &'a Port) -> Self {
        let Port {
            name,
            repeated,
            side,
            port_type,
            frontmatter,
            when,
            description,
        } = port;
        Self {
            name,
            repeated: *repeated,
            side: side.as_ref(),
            port_type,
            frontmatter: frontmatter
                .as_ref()
                .map(|m| m.iter().map(|(k, v)| (k.as_str(), v)).collect()),
            when: when.as_ref().map(canon_yaml),
            description: description.as_deref(),
        }
    }
}

#[derive(Serialize)]
struct EdgeProjection<'a> {
    source: EndpointProjection<'a>,
    target: EndpointProjection<'a>,
    /// Not emitted by the frontend serializer, but authored text — semantic here.
    reason: Option<&'a str>,
    when: Option<serde_json::Value>,
    is_else: bool,
    /// Loop accumulation ("read all laps") — behavioural, so semantic, even though
    /// the frontend serializer currently drops it.
    repeated: bool,
}

impl<'a> EdgeProjection<'a> {
    fn of(edge: &'a EdgeDef) -> Self {
        let EdgeDef {
            source,
            target,
            reason,
            when,
            is_else,
            repeated,
            mode,
            waypoints,
            target_side,
        } = edge;
        // LAYOUT_FIELDS["edge"] — routing is presentation (#154 / #168).
        let (_layout_mode, _layout_waypoints, _layout_target_side) = (mode, waypoints, target_side);
        Self {
            source: EndpointProjection::of(source),
            target: EndpointProjection::of(target),
            reason: reason.as_deref(),
            when: when.as_ref().map(canon_yaml),
            is_else: *is_else,
            repeated: *repeated,
        }
    }
}

#[derive(Serialize)]
struct EndpointProjection<'a> {
    node: &'a str,
    port: &'a str,
}

impl<'a> EndpointProjection<'a> {
    fn of(endpoint: &'a EdgeEndpoint) -> Self {
        let EdgeEndpoint { node, port } = endpoint;
        Self { node, port }
    }
}

#[derive(Serialize)]
struct LoopProjection<'a> {
    id: &'a str,
    kind: &'a LoopKind,
    members: &'a [String],
    max_iter: Option<serde_json::Value>,
    over: Option<&'a str>,
}

impl<'a> LoopProjection<'a> {
    fn of(region: &'a LoopRegion) -> Self {
        let LoopRegion {
            id,
            kind,
            members,
            max_iter,
            over,
        } = region;
        Self {
            id,
            kind,
            members,
            max_iter: max_iter.as_ref().map(canon_yaml),
            over: over.as_deref(),
        }
    }
}

/// Render a free-form YAML value (`when:` clauses, `max_iter:`, variable defaults)
/// into JSON with **sorted** mapping keys. `serde_yaml::Mapping` keeps document
/// order, so two files differing only in the order of a `when:` clause's keys must
/// still land on one canonical form.
fn canon_yaml(value: &serde_yaml::Value) -> serde_json::Value {
    use serde_json::Value as J;
    match value {
        serde_yaml::Value::Null => J::Null,
        serde_yaml::Value::Bool(b) => J::Bool(*b),
        serde_yaml::Value::Number(n) => canon_number(n),
        serde_yaml::Value::String(s) => J::String(s.clone()),
        serde_yaml::Value::Sequence(seq) => J::Array(seq.iter().map(canon_yaml).collect()),
        serde_yaml::Value::Mapping(map) => {
            let sorted: BTreeMap<String, J> = map
                .iter()
                .map(|(k, v)| (canon_key(k), canon_yaml(v)))
                .collect();
            J::Object(sorted.into_iter().collect())
        }
        // A `!tag`ged scalar has no JSON shape; keep the tag next to its value so
        // two different tags never collapse onto one form.
        serde_yaml::Value::Tagged(tagged) => J::Array(vec![
            J::String(format!("!{}", tagged.tag)),
            canon_yaml(&tagged.value),
        ]),
    }
}

/// YAML allows non-string mapping keys. Render every key through JSON so a string
/// key `"1"` (`"\"1\""`) and an integer key `1` (`"1"`) stay distinguishable.
fn canon_key(key: &serde_yaml::Value) -> String {
    let canonical = canon_yaml(key);
    serde_json::to_string(&canonical).unwrap_or_else(|_| format!("{canonical:?}"))
}

fn canon_number(number: &serde_yaml::Number) -> serde_json::Value {
    if let Some(i) = number.as_i64() {
        return serde_json::Value::Number(i.into());
    }
    if let Some(u) = number.as_u64() {
        return serde_json::Value::Number(u.into());
    }
    if let Some(f) = number.as_f64() {
        return match serde_json::Number::from_f64(f) {
            Some(n) => serde_json::Value::Number(n),
            // NaN / ±inf have no JSON number. Keep them apart as strings rather
            // than collapsing to null, which would equate `nan` with `inf`.
            None => serde_json::Value::String(format!("__nonfinite__:{f}")),
        };
    }
    serde_json::Value::Null
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::parse_pipeline;

    fn canonical(yaml: &str) -> String {
        let parsed = parse_pipeline(yaml).expect("fixture must parse");
        canonical_form(&parsed.pipeline).expect("projection must serialize")
    }

    /// start → doer → end, with the layout bits threaded through as parameters so
    /// a test can move a node or pin a route without touching anything else.
    fn fixture(doer_x: f64, edge_mode: &str) -> String {
        format!(
            "name: drift-demo\nversion: '1.0'\nnodes:\n\
             - id: start\n  name: Start\n  type: start\n  outputs:\n  - name: user_prompt\n  view:\n    x: 300\n    y: 60\n\
             - id: doer\n  name: Doer\n  type: doc-only\n  outputs:\n  - name: out\n  view:\n    x: {doer_x}\n    y: 260\n\
             - id: end\n  name: End\n  type: end\n  inputs:\n  - name: result\n  view:\n    x: 300\n    y: 460\n\
             edges:\n\
             - source: {{node: start, port: user_prompt}}\n  target: {{node: doer, port: in}}\n  {edge_mode}\n\
             - source: {{node: doer, port: out}}\n  target: {{node: end, port: result}}\n"
        )
    }

    #[test]
    fn moving_a_node_does_not_change_the_projection() {
        assert_eq!(
            canonical(&fixture(300.0, "")),
            canonical(&fixture(540.0, ""))
        );
    }

    #[test]
    fn pinned_route_and_arrow_side_do_not_change_the_projection() {
        let auto = fixture(300.0, "");
        let pinned = fixture(
            300.0,
            "mode: manual\n  waypoints:\n  - {x: 10, y: 20}\n  target_side: top",
        );
        assert_eq!(canonical(&auto), canonical(&pinned));
    }

    #[test]
    fn notes_do_not_change_the_projection() {
        let with_notes = format!(
            "{}notes:\n- id: n1\n  content: a reminder\n  view: {{x: 5, y: 5}}\n",
            fixture(300.0, "")
        );
        assert_eq!(canonical(&fixture(300.0, "")), canonical(&with_notes));
    }

    #[test]
    fn formatting_noise_does_not_change_the_projection() {
        // Same document: key order swapped, flow vs block style, quoting changed,
        // and the port `side` defaults left to the parser on one side only.
        let block = "name: p\nversion: \"1.0\"\nnodes:\n  - id: start\n    name: Start\n    type: start\n    outputs:\n      - name: user_prompt\n        side: bottom\n  - id: end\n    name: End\n    type: end\n    inputs:\n      - name: result\n        side: left\n";
        let flow = "version: '1.0'\nname: 'p'\nnodes: [{type: start, id: start, name: Start, outputs: [{name: user_prompt, side: bottom}]}, {id: end, type: end, name: End, inputs: [{name: result}]}]\n";
        assert_eq!(canonical(block), canonical(flow));
    }

    #[test]
    fn when_clause_key_order_does_not_change_the_projection() {
        let make = |first: &str, second: &str| {
            format!(
                "name: p\nnodes:\n  - id: start\n    name: Start\n    type: start\n    outputs:\n      - name: user_prompt\n  - id: end\n    name: End\n    type: end\n    inputs:\n      - name: result\nedges:\n  - source: {{node: start, port: user_prompt}}\n    target: {{node: end, port: result}}\n    when:\n      {first}\n      {second}\n"
            )
        };
        assert_eq!(
            canonical(&make("verdict: {eq: PASS}", "score: {gte: 3}")),
            canonical(&make("score: {gte: 3}", "verdict: {eq: PASS}")),
        );
    }

    #[test]
    fn semantic_edits_change_the_projection() {
        let base = fixture(300.0, "");
        let cases = [
            (
                "pipeline rename",
                base.replace("name: drift-demo", "name: renamed"),
            ),
            ("node rename", base.replace("name: Doer", "name: Worker")),
            (
                "port rename",
                base.replace("- name: out", "- name: done")
                    .replace("port: out", "port: done"),
            ),
            (
                "edge retarget",
                base.replace(
                    "target: {node: doer, port: in}",
                    "target: {node: doer, port: task}",
                ),
            ),
            (
                "per-node model",
                base.replace("  type: doc-only", "  type: doc-only\n  model: opus"),
            ),
            (
                // #424. This list is maintained BY HAND — nothing in the build
                // demands an entry, and the compiler is happy with a
                // `let _ = effort;` in `NodeProjection::of`. Without this case, a
                // neutralised field would ship silently and the library's ⚠ drifted
                // badge would never light up on an effort change: the user would
                // launch a stale copy believing it current.
                "per-node effort",
                base.replace("  type: doc-only", "  type: doc-only\n  effort: low"),
            ),
            (
                "edge condition",
                base.replace(
                    "  target: {node: end, port: result}",
                    "  target: {node: end, port: result}\n  when: {verdict: {eq: PASS}}",
                ),
            ),
            (
                "repeated edge",
                base.replace(
                    "  target: {node: end, port: result}",
                    "  target: {node: end, port: result}\n  repeated: true",
                ),
            ),
            (
                "added node",
                base.replace(
                    "edges:",
                    "- id: extra\n  name: Extra\n  type: doc-only\n  outputs:\n  - name: o\nedges:",
                ),
            ),
            (
                "loop region",
                format!(
                    "{base}loops:\n- id: r\n  kind: bounded\n  members: [doer]\n  max_iter: 3\n"
                ),
            ),
            (
                "prompt_required",
                base.replace("version: '1.0'", "version: '1.0'\nprompt_required: false"),
            ),
        ];
        for (label, variant) in cases {
            assert_ne!(
                canonical(&base),
                canonical(&variant),
                "{label} must register as a semantic change"
            );
        }
    }

    #[test]
    fn loop_region_member_change_is_semantic() {
        let with = |members: &str| {
            format!(
                "{}loops:\n- id: r\n  kind: bounded\n  members: {members}\n  max_iter: 2\n",
                fixture(300.0, "")
            )
        };
        assert_ne!(canonical(&with("[doer]")), canonical(&with("[doer, end]")));
    }

    #[test]
    fn projection_is_deterministic_across_calls() {
        // `variables` and `frontmatter` come from `HashMap`s: a naive projection
        // would emit them in a different order on every call.
        let yaml = "name: p\nvariables:\n  alpha: {type: int, default: 1}\n  beta: {type: string, default: \"x\"}\n  gamma: {type: bool, default: true}\nnodes:\n  - id: start\n    name: Start\n    type: start\n    outputs:\n      - name: user_prompt\n        frontmatter:\n          verdict: {type: enum, allowed: [PASS, FAIL]}\n          score: {type: int}\n          notes: {type: string}\n  - id: end\n    name: End\n    type: end\n    inputs:\n      - name: result\n";
        let first = canonical(yaml);
        for _ in 0..16 {
            assert_eq!(first, canonical(yaml));
        }
    }

    /// The cross-language guard behind acceptance criterion 3: our LAYOUT sets must
    /// equal the owner's, scope by scope. Parsed with a deliberately dumb reader —
    /// it must break loudly if `layoutFields.ts` is restructured, not silently pass.
    #[test]
    fn layout_fields_match_frontend_owner() {
        let owner_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("frontend/src/lib/layoutFields.ts");
        let source = std::fs::read_to_string(&owner_path)
            .unwrap_or_else(|e| panic!("cannot read the partition owner {owner_path:?}: {e}"));

        let owner = parse_layout_fields(&source);
        let mine: BTreeMap<String, Vec<String>> = LAYOUT_FIELDS
            .iter()
            .map(|(scope, fields)| {
                (
                    (*scope).to_string(),
                    fields.iter().map(|f| (*f).to_string()).collect(),
                )
            })
            .collect();

        assert_eq!(
            owner, mine,
            "LAYOUT_FIELDS drifted from frontend/src/lib/layoutFields.ts (#395): \
             the daemon's semantic projection and the canvas star would disagree again"
        );
    }

    /// Extract `LAYOUT_FIELDS`'s `scope: [ "a", "b" ]` entries from the TS source.
    /// Comments are dropped first, then each entry is read up to its `]`.
    fn parse_layout_fields(source: &str) -> BTreeMap<String, Vec<String>> {
        let body = source
            .split_once("export const LAYOUT_FIELDS")
            .expect("LAYOUT_FIELDS not found — layoutFields.ts was restructured")
            .1
            .split_once('{')
            .expect("LAYOUT_FIELDS body not found")
            .1;
        let uncommented: String = body
            .lines()
            .take_while(|l| !l.starts_with("};"))
            .map(|l| l.split("//").next().unwrap_or(""))
            .collect::<Vec<_>>()
            .join("\n");

        let mut out = BTreeMap::new();
        let mut rest = uncommented.as_str();
        while let Some((head, tail)) = rest.split_once('[') {
            let (list, remainder) = tail.split_once(']').expect("unterminated array literal");
            rest = remainder;
            // The key is the trailing identifier before the `:` — everything ahead
            // of it is the previous entry's tail (`, `, newlines, indentation).
            // Each entry also ends with `satisfies (keyof X)[]`, whose `[` has no
            // `:` in front of it at all; skip those rather than stopping, or only
            // the first scope would ever be compared.
            let Some(scope) = head
                .rsplit_once(':')
                .and_then(|(key, _)| {
                    key.rsplit(|c: char| !c.is_ascii_alphanumeric())
                        .next()
                        .map(str::to_string)
                })
                .filter(|s| !s.is_empty())
            else {
                continue;
            };
            let fields: Vec<String> = list
                .split(',')
                .map(|f| f.trim().trim_matches('"').trim_matches('\'').to_string())
                .filter(|f| !f.is_empty())
                .collect();
            out.insert(scope, fields);
        }
        assert_eq!(
            out.len(),
            LAYOUT_FIELDS.len(),
            "read {} scopes out of layoutFields.ts, expected {} — the reader, not the \
             partition, is what broke",
            out.len(),
            LAYOUT_FIELDS.len(),
        );
        out
    }
}
