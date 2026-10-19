//! The retired per-port `side` and `repeated`: what a document that still
//! carries them turns into.
//!
//! A port has neither any more. Where a wire leaves and lands belongs to the
//! edge (`source_anchor`, `target_side`, #844), and so does accumulation
//! (`EdgeDef::repeated`, #149). [`convert_legacy_ports`] runs on the raw YAML in
//! [`super::parse_pipeline`] and in the migrator, so a persisted rewrite and an
//! in-memory parse agree (precedent: [`super::fold_flat_model_effort_into_harnesses`]).

use std::collections::HashMap;

use serde_yaml::{Mapping, Value};

const SIDE: &str = "side";
const REPEATED: &str = "repeated";

/// What [`convert_legacy_ports`] changed in a document. All zero ⇒ untouched.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct LegacyPortsReport {
    /// Edges that received a `source_anchor` from their output's old side.
    pub source_anchors: usize,
    /// Edges into an End that received a `target_side` from its `result`'s old side.
    pub target_sides: usize,
    /// Ports whose `side` was removed.
    pub sides_removed: usize,
    /// Ports whose `repeated` was removed.
    pub repeated_removed: usize,
}

impl LegacyPortsReport {
    pub fn changed(&self) -> bool {
        *self != Self::default()
    }

    /// One line for `pdo migrate`: what moved where, or `None` when nothing did.
    pub fn summary(&self) -> Option<String> {
        if !self.changed() {
            return None;
        }
        let mut parts = Vec::new();
        if self.sides_removed > 0 {
            parts.push(format!("{} port side(s) removed", self.sides_removed));
        }
        if self.source_anchors > 0 {
            parts.push(format!("{} source_anchor(s) added", self.source_anchors));
        }
        if self.target_sides > 0 {
            parts.push(format!("{} target_side(s) added", self.target_sides));
        }
        if self.repeated_removed > 0 {
            parts.push(format!("{} port repeated removed", self.repeated_removed));
        }
        Some(parts.join(", "))
    }
}

/// Whether a node still declares a `side` or a `repeated` on one of its ports.
pub fn node_has_legacy_port_key(node: &Value) -> bool {
    ["inputs", "outputs"].iter().any(|key| {
        node.get(*key)
            .and_then(Value::as_sequence)
            .is_some_and(|ports| {
                ports.iter().any(|p| {
                    p.as_mapping()
                        .is_some_and(|m| m.contains_key(SIDE) || m.contains_key(REPEATED))
                })
            })
    })
}

/// Converts the legacy port keys of a whole pipeline document:
///
/// - an edge with no `source_anchor` whose primary carried output declares a side
///   other than `right` gets `source_anchor: { side }`, without `offset`: the
///   middle of the side, where the per-output dot used to sit;
/// - an edge with neither `target_anchor` nor `target_side` that lands on an End
///   whose `result` declares a side other than `left` gets that `target_side`;
/// - `side` and `repeated` are then removed from every input and output. An
///   output's `repeated` had no effect left; the migrator moves a declared
///   input's `repeated` onto its edge before this runs.
///
/// An edge that already carries an anchor keeps it. A second run is a no-op.
pub fn convert_legacy_ports(doc: &mut Value) -> LegacyPortsReport {
    let mut report = LegacyPortsReport::default();
    if !doc
        .get("nodes")
        .and_then(Value::as_sequence)
        .is_some_and(|nodes| nodes.iter().any(node_has_legacy_port_key))
    {
        return report;
    }
    convert_sides_onto_edges(doc, &mut report);
    if let Some(nodes) = doc.get_mut("nodes").and_then(Value::as_sequence_mut) {
        for node in nodes.iter_mut() {
            for key in ["inputs", "outputs"] {
                if let Some(ports) = node.get_mut(key).and_then(Value::as_sequence_mut) {
                    let (sides, repeated) = strip_legacy_port_keys(ports);
                    report.sides_removed += sides;
                    report.repeated_removed += repeated;
                }
            }
        }
    }
    report
}

/// Removes `side` and `repeated` from each port of `ports`; returns how many of
/// each went. A node-library entry has no edges, so this is all it needs.
pub fn strip_legacy_port_keys(ports: &mut [Value]) -> (usize, usize) {
    let mut sides = 0;
    let mut repeated = 0;
    for port in ports.iter_mut().filter_map(Value::as_mapping_mut) {
        sides += usize::from(port.remove(SIDE).is_some());
        repeated += usize::from(port.remove(REPEATED).is_some());
    }
    (sides, repeated)
}

fn declared_side(port: &Value) -> Option<String> {
    port.get(SIDE).and_then(Value::as_str).map(str::to_string)
}

fn convert_sides_onto_edges(doc: &mut Value, report: &mut LegacyPortsReport) {
    // An output as declared: its name and its legacy side, either possibly absent.
    type DeclaredOutput = (Option<String>, Option<String>);
    // node id -> its outputs in order; End id -> its `result`'s declared side.
    let mut output_sides: HashMap<String, Vec<DeclaredOutput>> = HashMap::new();
    let mut end_sides: HashMap<String, String> = HashMap::new();
    for node in doc
        .get("nodes")
        .and_then(Value::as_sequence)
        .into_iter()
        .flatten()
    {
        let Some(id) = node.get("id").and_then(Value::as_str) else {
            continue;
        };
        let ports = |key: &str| node.get(key).and_then(Value::as_sequence);
        let outputs = ports("outputs")
            .into_iter()
            .flatten()
            .map(|p| {
                let name = p.get("name").and_then(Value::as_str).map(str::to_string);
                (name, declared_side(p))
            })
            .collect();
        output_sides.insert(id.to_string(), outputs);
        if node.get("type").and_then(Value::as_str) == Some("end") {
            if let Some(side) = ports("inputs")
                .and_then(|s| s.first())
                .and_then(declared_side)
            {
                end_sides.insert(id.to_string(), side);
            }
        }
    }

    let Some(edges) = doc.get_mut("edges").and_then(Value::as_sequence_mut) else {
        return;
    };
    for edge in edges.iter_mut() {
        let source = edge.get("source");
        let source_node = source
            .and_then(|s| s.get("node"))
            .and_then(Value::as_str)
            .map(str::to_string);
        let primary_port = source
            .and_then(|s| {
                s.get("port").and_then(Value::as_str).or_else(|| {
                    s.get("ports")
                        .and_then(Value::as_sequence)
                        .and_then(|seq| seq.first())
                        .and_then(Value::as_str)
                })
            })
            .map(str::to_string);
        let target_node = edge
            .get("target")
            .and_then(|t| t.get("node"))
            .and_then(Value::as_str)
            .map(str::to_string);
        let Some(edge_map) = edge.as_mapping_mut() else {
            continue;
        };

        if !edge_map.contains_key("source_anchor") {
            // The port the edge names, else the node's first output: the renderer
            // read the side the same way before the conversion.
            let side = source_node
                .as_deref()
                .and_then(|n| output_sides.get(n))
                .and_then(|outs| {
                    outs.iter()
                        .find(|(name, _)| name.is_some() && *name == primary_port)
                        .or_else(|| outs.first())
                })
                .and_then(|(_, side)| side.clone());
            if let Some(side) = side.filter(|s| s != "right") {
                let mut anchor = Mapping::new();
                anchor.insert(Value::String(SIDE.into()), Value::String(side));
                edge_map.insert(
                    Value::String("source_anchor".into()),
                    Value::Mapping(anchor),
                );
                report.source_anchors += 1;
            }
        }

        if !edge_map.contains_key("target_anchor") && !edge_map.contains_key("target_side") {
            let side = target_node.as_deref().and_then(|n| end_sides.get(n));
            if let Some(side) = side.filter(|s| s.as_str() != "left") {
                edge_map.insert(
                    Value::String("target_side".into()),
                    Value::String(side.clone()),
                );
                report.target_sides += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    /// start -> worker(bottom-out, right-out, top-out, plain-out) -> end, the
    /// worker's ports and the End's `result` declaring a legacy `side`.
    fn legacy_doc(end_side: &str, extra: &str) -> Value {
        serde_yaml::from_str(&format!(
            r#"
name: legacy-sides
nodes:
  - id: start
    type: start
    outputs:
      - {{ name: user_prompt, side: right }}
  - id: ab12cd34
    type: agent
    outputs:
      - {{ name: bottom-out, side: bottom, repeated: true }}
      - {{ name: right-out, side: right }}
      - {{ name: top-out, side: top }}
      - {{ name: plain-out }}
  - id: end
    type: end
    inputs:
      - {{ name: result, side: {end_side} }}
edges:
  - source: {{ node: start, port: user_prompt }}
    target: {{ node: ab12cd34, port: in }}
  - source: {{ node: ab12cd34, port: right-out }}
    target: {{ node: end, port: result }}
  - source: {{ node: ab12cd34, ports: [top-out, plain-out] }}
    target: {{ node: end, port: result }}
  - source: {{ node: ab12cd34, port: plain-out }}
    target: {{ node: end, port: result }}
  - source: {{ node: ab12cd34, port: bottom-out }}
    target: {{ node: end, port: result }}
    repeated: true{extra}
"#
        ))
        .unwrap()
    }

    fn anchor(side: &str) -> Value {
        serde_yaml::from_str(&format!("{{ side: {side} }}")).unwrap()
    }

    #[test]
    fn an_output_side_becomes_a_source_anchor_without_offset() {
        let mut doc = legacy_doc("left", "");
        let report = convert_legacy_ports(&mut doc);
        let edges = doc["edges"].as_sequence().unwrap();
        // `right` is the default departure: nothing written.
        assert!(edges[0].get("source_anchor").is_none());
        assert!(edges[1].get("source_anchor").is_none());
        // A multi-port edge reads its primary (first) carried output.
        assert_eq!(edges[2]["source_anchor"], anchor("top"));
        // An output without a side leaves no anchor.
        assert!(edges[3].get("source_anchor").is_none());
        assert_eq!(edges[4]["source_anchor"], anchor("bottom"));
        // An End declared `left` lands on the default side: nothing written.
        assert!(edges.iter().all(|e| e.get("target_side").is_none()));
        assert_eq!(report.source_anchors, 2);
        assert_eq!(report.target_sides, 0);
    }

    #[test]
    fn an_end_result_side_becomes_the_target_side_of_its_edges() {
        let mut doc = legacy_doc("top", "");
        let report = convert_legacy_ports(&mut doc);
        let edges = doc["edges"].as_sequence().unwrap();
        for edge in &edges[1..] {
            assert_eq!(edge["target_side"].as_str(), Some("top"));
        }
        // Only an End converts: the edge into a work node is untouched.
        assert!(edges[0].get("target_side").is_none());
        assert_eq!(report.target_sides, 4);
    }

    #[test]
    fn an_edge_that_already_has_an_anchor_keeps_it() {
        let extra = "\n    source_anchor: { side: left, offset: 12 }\n    target_anchor: { side: right, offset: 40 }";
        let mut doc = legacy_doc("top", extra);
        convert_legacy_ports(&mut doc);
        let edge = &doc["edges"][4];
        assert_eq!(edge["source_anchor"]["side"].as_str(), Some("left"));
        assert_eq!(edge["source_anchor"]["offset"].as_f64(), Some(12.0));
        assert!(edge.get("target_side").is_none());

        // A `target_side` alone also blocks the End conversion.
        let mut doc = legacy_doc("top", "\n    target_side: bottom");
        convert_legacy_ports(&mut doc);
        assert_eq!(doc["edges"][4]["target_side"].as_str(), Some("bottom"));
    }

    #[test]
    fn every_port_loses_its_side_and_repeated_but_the_edge_keeps_its_own() {
        let mut doc = legacy_doc("top", "");
        let report = convert_legacy_ports(&mut doc);
        for node in doc["nodes"].as_sequence().unwrap() {
            for key in ["inputs", "outputs"] {
                for port in node[key].as_sequence().into_iter().flatten() {
                    assert!(port.get("side").is_none(), "{port:?}");
                    assert!(port.get("repeated").is_none(), "{port:?}");
                }
            }
        }
        assert_eq!(doc["edges"][4]["repeated"].as_bool(), Some(true));
        assert_eq!(report.sides_removed, 5);
        assert_eq!(report.repeated_removed, 1);
        assert_eq!(
            report.summary().as_deref(),
            Some("5 port side(s) removed, 2 source_anchor(s) added, 4 target_side(s) added, 1 port repeated removed")
        );
    }

    #[test]
    fn the_conversion_is_idempotent() {
        let mut doc = legacy_doc("top", "");
        assert!(convert_legacy_ports(&mut doc).changed());
        let once = doc.clone();
        let again = convert_legacy_ports(&mut doc);
        assert!(!again.changed());
        assert_eq!(again.summary(), None);
        assert_eq!(doc, once);
    }
}
