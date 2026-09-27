//! A pipeline edge: what it carries from one node to another, when it fires,
//! and how it is drawn.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::node::{is_false, PortSide};

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct EdgeEndpoint {
    pub node: String,
    pub port: String,
}

/// The source end of an edge: one node, and the output port(s) the edge
/// **carries** (ADR-0073 / #843). A multi-port edge is still ONE edge for the
/// runtime — it fires once, at the source node's completion, and drops one
/// emergent input per carried port on the target.
///
/// Two YAML shapes, and the single-port one is the one this re-emits whenever a
/// single port is carried, so an existing file round-trips byte for byte:
///
/// ```yaml
/// source: { node: design, port: out }          # one port — unchanged
/// source: { node: design, ports: [out, spec] } # several — additive
/// ```
///
/// `ports` is ALWAYS non-empty after a parse: an edge that carries nothing has
/// no meaning, so [`deserialize`](EdgeSource::deserialize) rejects both an empty
/// `ports:` list and a source that names neither key.
#[derive(Debug, Clone, Default, PartialEq, Eq, TS)]
#[ts(as = "EdgeSourceWire")]
pub struct EdgeSource {
    pub node: String,
    /// Carried output ports, in authored order. Order is presentation only — the
    /// semantic projection compares the SET (ADR-0073 §2).
    pub ports: Vec<String>,
}

impl EdgeSource {
    /// The single-port source every pre-#843 edge has.
    pub fn single(node: impl Into<String>, port: impl Into<String>) -> Self {
        Self {
            node: node.into(),
            ports: vec![port.into()],
        }
    }

    /// The edge's *primary* port: the first carried one. This is what the legacy
    /// single-port reading of an edge means — the artifact a `Switch` routes, the
    /// port a `body`/`done` structural edge names, the port an `EdgeInfo` shows.
    /// Returns `""` for a `Default`-constructed source (never a parsed one).
    pub fn port(&self) -> &str {
        self.ports.first().map(String::as_str).unwrap_or("")
    }

    /// Whether this edge carries `port`.
    pub fn carries(&self, port: &str) -> bool {
        self.ports.iter().any(|p| p == port)
    }

    /// `design.out`, or `design.out+spec` when several ports are carried — the
    /// spelling every diagnostic uses for an edge's source end.
    pub fn label(&self) -> String {
        format!("{}.{}", self.node, self.ports.join("+"))
    }
}

/// The TypeScript shape of [`EdgeSource`], whose `Serialize` is hand-written:
/// exactly one of `port` (one carried port) and `ports` (several) is set.
#[derive(TS)]
#[ts(export, rename = "EdgeSource")]
#[allow(dead_code)]
struct EdgeSourceWire {
    node: String,
    #[ts(optional)]
    port: Option<String>,
    #[ts(optional)]
    ports: Option<Vec<String>>,
}

impl Serialize for EdgeSource {
    /// Emits `port:` for a single carried port and `ports:` for several. The
    /// single-port shape is byte-identical to the pre-#843 one — that is the
    /// backward-compatibility contract, not a convenience.
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry("node", &self.node)?;
        if self.ports.len() == 1 {
            map.serialize_entry("port", &self.ports[0])?;
        } else {
            map.serialize_entry("ports", &self.ports)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for EdgeSource {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            node: String,
            #[serde(default)]
            port: Option<String>,
            #[serde(default)]
            ports: Option<Vec<String>>,
        }
        let raw = Raw::deserialize(deserializer)?;
        // Both keys at once is an authoring mistake, not a shape: reading one and
        // dropping the other would silently lose a carried port on the next save.
        if raw.port.is_some() && raw.ports.is_some() {
            return Err(serde::de::Error::custom(format!(
                "edge source '{}' has both `port:` and `ports:` — use one",
                raw.node
            )));
        }
        let ports = match (raw.port, raw.ports) {
            (Some(p), None) => vec![p],
            (None, Some(list)) if !list.is_empty() => list,
            (None, Some(_)) => {
                return Err(serde::de::Error::custom(format!(
                "edge source '{}' has an empty `ports:` list — an edge carries at least one output",
                raw.node
            )))
            }
            (Some(_), Some(_)) => unreachable!("rejected above"),
            (None, None) => {
                return Err(serde::de::Error::custom(format!(
                    "edge source '{}' names no output — expected `port:` or `ports:`",
                    raw.node
                )))
            }
        };
        Ok(Self {
            node: raw.node,
            ports,
        })
    }
}

/// Edge routing mode (#154). `Auto` edges store no waypoints — their
/// right-angle path is recomputed deterministically and re-routes on node move.
/// `Manual` edges pin the route to persisted `waypoints`. Routing is *layout*,
/// not semantics: it persists in the pipeline file (so a shared workflow keeps
/// its arrows) but is excluded from the semantic pipeline-diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum EdgeRouteMode {
    Auto,
    Manual,
}

/// A pinned waypoint on a manually-routed edge — absolute canvas coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct EdgeWaypoint {
    pub x: f64,
    pub y: f64,
}

/// Where on a card's side a wire leaves from or lands on (#844). The side is the
/// coarse choice `target_side` already carried; `offset` is the distance *along*
/// that side, in canvas px, from the side's start. Absent ⇒ the side's middle,
/// which is the pre-#844 geometry — so an untouched pipeline round-trips byte for
/// byte. Layout, like `mode`/`waypoints`/`target_side`: it persists in the file
/// and is excluded from the semantic pipeline-diff.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct EdgeAnchor {
    pub side: PortSide,
    pub offset: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct EdgeDef {
    pub source: EdgeSource,
    pub target: EdgeEndpoint,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Optional `when:` clause: a mechanical predicate (ADR-0002 grammar)
    /// evaluated against the source node's frontmatter, `iter`, and pipeline
    /// variables. When present, the edge fires only if the clause is satisfied.
    /// Conditional routing lives on the edge (ADR-0011).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional = nullable, type = "Record<string, unknown> | null")]
    pub when: Option<serde_yaml::Value>,
    /// `else: true` marks a fallback edge that fires iff no sibling edge on the
    /// same source port matched (ADR-0011). Mutually exclusive with `when:`.
    #[serde(default, rename = "else", skip_serializing_if = "is_false")]
    #[ts(optional = nullable)]
    pub is_else: bool,
    /// `repeated: true` marks an edge whose source artifact accumulates across
    /// iterations: the resolver globs `iter-*` and pools every match into the
    /// emergent input. Loop accumulation ("read all laps") lives on the edge,
    /// not on a declared input port (ADR-0011 / #149).
    #[serde(default, skip_serializing_if = "is_false")]
    #[ts(optional = nullable)]
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
    /// Where on the SOURCE card's border the wire leaves (#844). Layout, like the
    /// three above. Absent ⇒ the middle of the departure side, never written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_anchor: Option<EdgeAnchor>,
    /// Where on the TARGET card's side the arrow lands (#844). Its `side` agrees
    /// with `target_side`; the offset is the per-pixel position along it. Layout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_anchor: Option<EdgeAnchor>,
    /// Whether the carried outputs are NAMED on the canvas, near the arrow's
    /// base (#845). Absent ⇒ the frontend's derived default (on iff the source
    /// node declares two or more outputs), so the file stays silent until the
    /// author decides. *Layout*, like the four fields around it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_output_labels: Option<bool>,
    /// Pinned absolute canvas position of an output label, keyed by carried
    /// port (#845). A port absent from the map sits at its default spot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_label_pos: Option<BTreeMap<String, EdgeWaypoint>>,
    /// Pinned absolute canvas position of the `when`/`else` pill (#845).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition_label_pos: Option<EdgeWaypoint>,
    /// Draw order (#845): edges draw ABOVE the node cards by default, `true`
    /// drops this one below. Absent/false ⇒ above, and never written.
    #[serde(default, skip_serializing_if = "is_false")]
    #[ts(optional = nullable)]
    pub below_nodes: bool,
}

impl EdgeDef {
    /// The inputs this edge drops on its target: one per carried port (ADR-0073).
    /// Each pair is `(source port, target input name)`.
    ///
    /// A single-port edge keeps `target.port` as the input name — that is what a
    /// declared handle (a `merge`'s input, End's `result`) and every pre-#843
    /// file mean, and renaming it on the way through would break them. As soon as
    /// the edge carries several ports there is no single name left to honour, so
    /// each input takes its own port's name — which is exactly the emergent rule
    /// ("connecting `debugger.repro_steps` creates a `repro_steps` input").
    pub fn emergent_inputs(&self) -> Vec<(String, String)> {
        if self.source.ports.len() <= 1 {
            return vec![(self.source.port().to_string(), self.target.port.clone())];
        }
        self.source
            .ports
            .iter()
            .map(|p| (p.clone(), p.clone()))
            .collect()
    }
}
