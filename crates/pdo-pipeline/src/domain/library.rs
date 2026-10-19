//! The node library (`~/.pdo/library`): an entry is a starred node, ready to
//! be dropped onto any canvas. Its isolation is always explicit (#655).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::domain::pipeline;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct LibraryEntry {
    pub name: String,
    #[serde(rename = "type")]
    pub node_type: pipeline::NodeType,
    #[serde(default)]
    pub inputs: Vec<pipeline::Port>,
    #[serde(default)]
    pub outputs: Vec<pipeline::Port>,
    #[serde(default)]
    pub interactive: bool,
    /// Per-node model override. Don't drop `skip_serializing_if`: a node on the
    /// account default must serialize byte-identically to an older entry that
    /// never knew the key, or it reads `diverged` forever.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub model: Option<String>,
    /// Per-node reasoning-effort override. Same `skip_serializing_if` invariant
    /// as `model`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub max_iter: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub branches: Option<u32>,
    /// Defaulted: a structural node exported to YAML carries no prompt, and
    /// `POST /nodes/parse` must accept a prompt-less node map.
    #[serde(default)]
    pub prompt: String,
    /// Where an instance of this entry works (#655, ADR-0060). The library is
    /// isolation-aware for the same reason it became model-aware in #345: an
    /// entry that forgot the field would restore the *type default* on every
    /// instantiation, silently forking a worktree for an Agent its author had
    /// parked in the Run's — and the star would read `diverged` forever.
    ///
    /// Always `Some` for `agent`/`script` (stamped by [`normalized_isolation`]
    /// on every read and write, so a pre-#655 file states its choice too), and
    /// always `None` for the types that carry none. `skip_serializing_if` is
    /// what keeps a starred Start/End file free of a line nobody may edit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub isolated_worktree: Option<bool>,
    /// The « on demand » option (ADR-0086), starred with the node: an entry
    /// that dropped it would instantiate a node the runtime launches at once.
    /// Omitted when `false`, so an entry starred before the option reads
    /// `synced` against a node that does not carry it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[ts(optional = nullable)]
    pub on_demand: bool,
}

impl LibraryEntry {
    /// This entry's isolation, made explicit (#655, ADR-0060).
    ///
    /// The type has the last word on *whether* there is a choice at all, and the
    /// entry has the last word on *which* — the same precedence
    /// `NodeDef::is_isolated` uses, so the library and the Document cannot read
    /// one silence two ways. A stray value on a Start/End is dropped rather than
    /// round-tripped, mirroring `pipeline::stamp_isolation_default`.
    fn normalized_isolation(&self) -> Option<bool> {
        let default = self.node_type.default_isolation()?;
        Some(self.isolated_worktree.unwrap_or(default))
    }

    /// Stamp [`normalized_isolation`] onto the entry. Every path that produces an
    /// entry — a disk read, a star, a `POST /library` — runs this, so "an entry
    /// states where it works" holds by construction instead of by convention.
    pub fn stamp_isolation(&mut self) {
        self.isolated_worktree = self.normalized_isolation();
    }
}

/// Deserialize one library file and make its isolation explicit (#655).
///
/// The single door onto the on-disk format: `list`, `get`, `find_by_name` and
/// `find_path_by_name` all read through it, so a pre-#655 entry (no
/// `isolated_worktree` line) is seen by everything downstream as carrying its
/// type's default — not as a silence that would read `diverged` against a node
/// sitting on that very default.
pub fn parse_entry(contents: &str) -> Option<LibraryEntry> {
    let mut entry = serde_yaml::from_str::<LibraryEntry>(contents).ok()?;
    entry.stamp_isolation();
    Some(entry)
}

/// An entry file rewritten without its ports' retired `side` and `repeated`
/// (`pdo migrate`). `Ok(None)` when it carries neither; a second pass is a no-op.
/// An entry has no edges, so the keys simply go: where a wire leaves is decided
/// on the canvas the node is dropped onto.
pub fn migrate_entry_yaml(
    contents: &str,
) -> Result<Option<(String, pipeline::LegacyPortsReport)>, String> {
    let mut doc: serde_yaml::Value =
        serde_yaml::from_str(contents).map_err(|e| format!("YAML parse error: {e}"))?;
    let mut report = pipeline::LegacyPortsReport::default();
    for key in ["inputs", "outputs"] {
        if let Some(ports) = doc
            .get_mut(key)
            .and_then(serde_yaml::Value::as_sequence_mut)
        {
            let (sides, repeated) = pipeline::strip_legacy_port_keys(ports);
            report.sides_removed += sides;
            report.repeated_removed += repeated;
        }
    }
    if !report.changed() {
        return Ok(None);
    }
    let yaml = serde_yaml::to_string(&doc).map_err(|e| format!("YAML serialize error: {e}"))?;
    Ok(Some((yaml, report)))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SyncState {
    Outline,
    Synced,
    Diverged,
}

pub fn slugify(name: &str) -> String {
    let mut slug = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            slug.push(ch.to_ascii_lowercase());
        } else if ch == ' ' {
            slug.push('-');
        }
    }
    if slug.is_empty() {
        slug.push_str("node");
    }
    slug
}

pub fn entry_from_node(node: &pipeline::NodeDef, prompt: &str) -> LibraryEntry {
    let mut entry = LibraryEntry {
        name: node.name.clone(),
        node_type: node.node_type.clone(),
        inputs: node.inputs.clone(),
        outputs: node.outputs.clone(),
        interactive: node.interactive,
        // The node library's schema is claude-only, so project the
        // `harnesses.claude` entry rather than the whole map.
        model: node
            .harnesses
            .get(pdo_harness::registry::CLAUDE)
            .and_then(|e| e.model.clone()),
        effort: node
            .harnesses
            .get(pdo_harness::registry::CLAUDE)
            .and_then(|e| e.effort.clone()),
        max_iter: None,
        branches: None,
        prompt: prompt.to_string(),
        // #655/ADR-0060: the node's workspace is part of what gets starred. Read
        // through `stamp_isolation` below, so a node whose document is silent
        // stars as its type's default rather than as a silence — otherwise the
        // very node that produced the entry would read `diverged` against it.
        isolated_worktree: node.isolated_worktree,
        on_demand: node.on_demand,
    };
    entry.stamp_isolation();
    entry
}

// Only this module's unit tests call it; kept as a tested helper.

/// Where a canvas node stands against the library: never starred, starred and
/// identical, or starred and edited since. `starred` is the entry of the
/// node's name, when there is one.
pub fn sync_state(
    starred: Option<&LibraryEntry>,
    node: &pipeline::NodeDef,
    prompt: &str,
) -> SyncState {
    let Some(entry) = starred else {
        return SyncState::Outline;
    };
    if entry_from_node(node, prompt) == *entry {
        SyncState::Synced
    } else {
        SyncState::Diverged
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    const LEGACY_ENTRY: &str = "name: Reviewer\ntype: agent\ninputs:\n- name: code\n  repeated: true\n  side: left\noutputs:\n- name: review\n  repeated: true\n  side: bottom\n  port_type: markdown\nprompt: You review.\n";

    #[test]
    fn a_legacy_entry_still_reads_without_its_port_side_and_repeated() {
        let entry = parse_entry(LEGACY_ENTRY).expect("a legacy entry parses");
        assert_eq!(entry.outputs[0].name, "review");
        let served = serde_json::to_value(&entry).unwrap();
        assert!(served["outputs"][0].get("side").is_none());
        assert!(served["outputs"][0].get("repeated").is_none());
    }

    #[test]
    fn migrating_an_entry_drops_the_port_keys_once() {
        let (yaml, report) = migrate_entry_yaml(LEGACY_ENTRY).unwrap().expect("migrated");
        assert!(!yaml.contains("side"), "{yaml}");
        assert!(!yaml.contains("repeated"), "{yaml}");
        assert_eq!(report.sides_removed, 2);
        assert_eq!(report.repeated_removed, 2);
        assert_eq!(parse_entry(&yaml), parse_entry(LEGACY_ENTRY));
        assert_eq!(migrate_entry_yaml(&yaml).unwrap(), None);
    }
}
