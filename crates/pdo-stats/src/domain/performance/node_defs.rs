//! What the Run's frozen snapshot says of a Node, and the declared group of a
//! subagent transcript.

use std::collections::BTreeMap;

use serde_json::Value;

/// One node of the Run's frozen pipeline snapshot (`run_started.node_defs`) —
/// the fallback every per-execution reader falls back to when the startup event
/// predates the field it wants.
pub struct NodeDefSnapshot {
    pub node_type: String,
    /// #653/ADR-0060 — where the node works.
    pub isolated: bool,
    /// #723/#724 — the « Orchestrator » toggle as the Run froze it. Present in
    /// every snapshot since #723, so it covers the `NodeStarted` payloads that
    /// predate the per-execution freeze of #810.
    pub orchestrator: bool,
}

pub fn node_defs_from_payload(payload: &Value) -> BTreeMap<String, NodeDefSnapshot> {
    let mut defs = BTreeMap::new();
    if let Some(list) = payload.get("node_defs").and_then(|v| v.as_array()) {
        for def in list {
            let Some(id) = def.get("id").and_then(|v| v.as_str()) else {
                continue;
            };
            let node_type = def
                .get("node_type")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            // #653/ADR-0060: where the node works. The snapshot states it for an
            // `agent`/`script`; the type's default stands in for a pre-#653 Run.
            let isolated = def
                .get("isolated_worktree")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(matches!(node_type.as_str(), "agent" | "merge"));
            let orchestrator = def
                .get("orchestrator")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            defs.insert(
                id.to_string(),
                NodeDefSnapshot {
                    node_type,
                    isolated,
                    orchestrator,
                },
            );
        }
    }
    defs
}

pub fn looks_like_opaque_id(stem: &str) -> bool {
    let is_uuid = stem.len() == 36
        && stem.as_bytes().iter().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => *b == b'-',
            _ => b.is_ascii_hexdigit(),
        });
    let is_long_hex = stem.len() >= 16 && stem.chars().all(|c| c.is_ascii_hexdigit());
    is_uuid || is_long_hex
}

/// The declared group a discovered subagent transcript file falls under: its
/// filename, unless that reads as an opaque generated id — then the explicit
/// "Unidentified subagent" bucket, so it is never silently dropped.
///
/// **Scoped assumption**: no Claude Code subagent naming convention is
/// confirmed anywhere in this repo, so filename is the only signal short of
/// inventing an unconfirmed in-transcript field. This is the seam to extend if
/// subagent files are later found to embed a `subagent_type` key.
pub fn claude_subagent_group(file_stem: &str) -> String {
    if file_stem.is_empty() || looks_like_opaque_id(file_stem) {
        "Unidentified subagent".to_string()
    } else {
        file_stem.to_string()
    }
}
