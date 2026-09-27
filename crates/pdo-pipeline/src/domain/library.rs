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
    /// what keeps a starred Merge/Start/End file free of a line nobody may edit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub isolated_worktree: Option<bool>,
}

impl LibraryEntry {
    /// This entry's isolation, made explicit (#655, ADR-0060).
    ///
    /// The type has the last word on *whether* there is a choice at all, and the
    /// entry has the last word on *which* — the same precedence
    /// `NodeDef::is_isolated` uses, so the library and the Document cannot read
    /// one silence two ways. A stray value on a Merge is dropped rather than
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
