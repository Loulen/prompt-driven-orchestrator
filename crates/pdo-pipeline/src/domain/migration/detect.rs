//! Whether a document still carries a retired shape, and the ids the
//! migrator mints.

pub(crate) const NANOID_ALPHABET: &[u8] =
    b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
pub(crate) const NANOID_LEN: usize = 8;

pub(crate) fn deterministic_id(old_id: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in old_id.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    let mut out = String::with_capacity(NANOID_LEN);
    for i in 0..NANOID_LEN {
        let idx = ((hash >> (i * 8)) & 0xFF) as usize % NANOID_ALPHABET.len();
        out.push(NANOID_ALPHABET[idx] as char);
    }
    out
}

pub(crate) fn looks_like_nanoid(id: &str) -> bool {
    id.len() == NANOID_LEN && id.bytes().all(|b| NANOID_ALPHABET.contains(&b))
}

pub(crate) fn nodes_contain_type(nodes: &[serde_yaml::Value], node_type: &str) -> bool {
    nodes.iter().any(|n| {
        n.get("type")
            .and_then(|v| v.as_str())
            .is_some_and(|t| t == node_type)
    })
}

pub(crate) fn has_start_end_nodes(yaml_value: &serde_yaml::Value) -> bool {
    let nodes = match yaml_value.get("nodes").and_then(|n| n.as_sequence()) {
        Some(seq) => seq,
        None => return false,
    };
    nodes_contain_type(nodes, "start") && nodes_contain_type(nodes, "end")
}

pub(crate) fn has_halt_edges(yaml_value: &serde_yaml::Value) -> bool {
    let edges = match yaml_value.get("edges").and_then(|e| e.as_sequence()) {
        Some(seq) => seq,
        None => return false,
    };
    edges.iter().any(|e| {
        e.get("target")
            .and_then(|t| t.as_mapping())
            .is_some_and(|m| m.contains_key(serde_yaml::Value::String("halt".into())))
    })
}

pub fn needs_migration(yaml_value: &serde_yaml::Value) -> bool {
    if !has_start_end_nodes(yaml_value) {
        return true;
    }
    if has_halt_edges(yaml_value) {
        return true;
    }
    let nodes = match yaml_value.get("nodes").and_then(|n| n.as_sequence()) {
        Some(seq) => seq,
        None => return false,
    };
    // Switch nodes are dissolved into guarded edges (ADR-0011).
    if nodes_contain_type(nodes, "switch") {
        return true;
    }
    // Loop nodes are dissolved into a `loops:` region + rewired body edges (#148).
    if nodes_contain_type(nodes, "loop") {
        return true;
    }
    // ForEach nodes are dissolved into a `loops:` collection region + rewired
    // body edges (#151); the ForEach node type is retired.
    if nodes_contain_type(nodes, "for-each") {
        return true;
    }
    for node in nodes {
        // A flat `model:` / `effort:` on ANY node migrates under
        // `harnesses.claude.*`. Checked before the structural-node `continue` below,
        // or a structural node's flat fields would be skipped.
        if node.get("model").is_some() || node.get("effort").is_some() {
            return true;
        }
        // A port that still declares a `side` or a `repeated`, on any node: the
        // side moves onto the edges and both keys go.
        if crate::domain::pipeline::node_has_legacy_port_key(node) {
            return true;
        }
        let node_type = node.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if matches!(node_type, "start" | "end" | "switch" | "loop") {
            continue;
        }
        if node.get("prompt_file").and_then(|v| v.as_str()).is_some() {
            return true;
        }
        if let Some(id) = node.get("id").and_then(|v| v.as_str()) {
            if !looks_like_nanoid(id) {
                return true;
            }
        }
        // Inputs are emergent (#149): a *regular* node (agent / script)
        // that still declares any input needs migration so the declared port is
        // dropped (and a `repeated` flag migrated onto its edge). Structural
        // nodes (for-each here; start/end/switch/loop already `continue`d
        // above) keep their required ports.
        if node_type != "for-each"
            && node
                .get("inputs")
                .and_then(|v| v.as_sequence())
                .is_some_and(|s| !s.is_empty())
        {
            return true;
        }
    }
    false
}
