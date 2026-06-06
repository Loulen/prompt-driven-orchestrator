use std::collections::{HashMap, HashSet};
use std::path::Path;

use tracing::{info, warn};

use crate::pipeline;
use crate::pipeline::{Diagnostic, NodeType, PipelineDef, Severity};

const NANOID_ALPHABET: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
const NANOID_LEN: usize = 8;

fn deterministic_id(old_id: &str) -> String {
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

fn looks_like_nanoid(id: &str) -> bool {
    id.len() == NANOID_LEN && id.bytes().all(|b| NANOID_ALPHABET.contains(&b))
}

fn port_missing_side(ports: &[serde_yaml::Value]) -> bool {
    ports.iter().any(|p| {
        p.as_mapping()
            .is_some_and(|m| !m.contains_key(serde_yaml::Value::String("side".into())))
    })
}

fn nodes_contain_type(nodes: &[serde_yaml::Value], node_type: &str) -> bool {
    nodes.iter().any(|n| {
        n.get("type")
            .and_then(|v| v.as_str())
            .is_some_and(|t| t == node_type)
    })
}

fn has_start_end_nodes(yaml_value: &serde_yaml::Value) -> bool {
    let nodes = match yaml_value.get("nodes").and_then(|n| n.as_sequence()) {
        Some(seq) => seq,
        None => return false,
    };
    nodes_contain_type(nodes, "start") && nodes_contain_type(nodes, "end")
}

fn has_halt_edges(yaml_value: &serde_yaml::Value) -> bool {
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

fn needs_migration(yaml_value: &serde_yaml::Value) -> bool {
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
    for node in nodes {
        let node_type = node.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if node_type == "for-each" && node.get("over").is_none() {
            return true;
        }
        if matches!(node_type, "start" | "end" | "switch" | "loop" | "merge") {
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
        // Inputs are emergent (#149): a *regular* node (doc-only / code-mutating)
        // that still declares any input needs migration so the declared port is
        // dropped (and a `repeated` flag migrated onto its edge). Structural
        // nodes (for-each here; start/end/switch/loop/merge already `continue`d
        // above) keep their required ports.
        if node_type != "for-each"
            && node
                .get("inputs")
                .and_then(|v| v.as_sequence())
                .is_some_and(|s| !s.is_empty())
        {
            return true;
        }
        if let Some(outputs) = node.get("outputs").and_then(|v| v.as_sequence()) {
            if port_missing_side(outputs) {
                return true;
            }
        }
    }
    false
}

#[derive(Debug)]
pub struct MigrateResult {
    pub migrated: bool,
    pub yaml_text: String,
    pub prompt_moves: Vec<(String, String)>,
}

pub fn migrate_pipeline_yaml(
    yaml_text: &str,
    pipeline_path: &Path,
) -> Result<MigrateResult, String> {
    let mut doc: serde_yaml::Value =
        serde_yaml::from_str(yaml_text).map_err(|e| format!("YAML parse error: {e}"))?;

    if !needs_migration(&doc) {
        return Ok(MigrateResult {
            migrated: false,
            yaml_text: yaml_text.to_string(),
            prompt_moves: vec![],
        });
    }

    let pipeline_dir = pipeline_path.parent().unwrap_or(Path::new("."));

    // Dissolve Switch nodes into guarded edges (ADR-0011) before id-rewriting,
    // so the rewritten edges still reference the original node ids.
    dissolve_switches(&mut doc)?;

    let nodes = doc
        .get_mut("nodes")
        .and_then(|n| n.as_sequence_mut())
        .ok_or("missing 'nodes' sequence")?;

    let mut id_map: HashMap<String, String> = HashMap::new();
    let mut prompt_moves: Vec<(String, String)> = Vec::new();

    for node in nodes.iter_mut() {
        let mapping = node.as_mapping_mut().ok_or("node is not a mapping")?;

        let node_type = mapping
            .get(serde_yaml::Value::String("type".into()))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if matches!(node_type, "start" | "end" | "switch" | "loop" | "merge") {
            continue;
        }

        let old_id = mapping
            .get(serde_yaml::Value::String("id".into()))
            .and_then(|v| v.as_str())
            .ok_or("node missing 'id'")?
            .to_string();

        if looks_like_nanoid(&old_id)
            && mapping
                .get(serde_yaml::Value::String("name".into()))
                .is_some()
        {
            continue;
        }

        let new_id = deterministic_id(&old_id);
        id_map.insert(old_id.clone(), new_id.clone());

        mapping.insert(
            serde_yaml::Value::String("id".into()),
            serde_yaml::Value::String(new_id.clone()),
        );

        if mapping
            .get(serde_yaml::Value::String("name".into()))
            .is_none()
        {
            mapping.insert(
                serde_yaml::Value::String("name".into()),
                serde_yaml::Value::String(old_id.clone()),
            );
        }

        if let Some(pf) = mapping
            .remove(serde_yaml::Value::String("prompt_file".into()))
            .and_then(|v| v.as_str().map(String::from))
        {
            let old_path = pipeline_dir.join(&pf);
            let new_path = pipeline::canonical_prompt_path(pipeline_path, &new_id);
            if old_path.to_string_lossy() != new_path.to_string_lossy() {
                prompt_moves.push((
                    old_path.to_string_lossy().into_owned(),
                    new_path.to_string_lossy().into_owned(),
                ));
            }
        }
    }

    let edges = doc.get_mut("edges").and_then(|e| e.as_sequence_mut());

    if let Some(edges) = edges {
        for edge in edges.iter_mut() {
            rewrite_edge_endpoint(edge, "source", &id_map);
            rewrite_halt_edge(edge);
            rewrite_edge_endpoint(edge, "target", &id_map);
        }
    }

    // Inputs are emergent (#149): strip declared inputs from regular nodes,
    // migrating any `repeated: true` flag onto the matching incoming edge so the
    // accumulate-across-iterations behavior is preserved. Run before side
    // backfill so we don't bother backfilling sides on inputs we're dropping.
    drop_declared_inputs(&mut doc);

    let nodes_for_side = doc.get_mut("nodes").and_then(|n| n.as_sequence_mut());
    if let Some(nodes_for_side) = nodes_for_side {
        for node in nodes_for_side.iter_mut() {
            backfill_port_sides(node, "inputs", "left");
            backfill_port_sides(node, "outputs", "right");
        }
    }

    inject_start_end_nodes(&mut doc);
    backfill_foreach_over(&mut doc);

    let yaml_text =
        serde_yaml::to_string(&doc).map_err(|e| format!("YAML serialize error: {e}"))?;

    Ok(MigrateResult {
        migrated: true,
        yaml_text,
        prompt_moves,
    })
}

fn backfill_foreach_over(doc: &mut serde_yaml::Value) {
    let nodes = match doc.get_mut("nodes").and_then(|n| n.as_sequence_mut()) {
        Some(seq) => seq,
        None => return,
    };
    let over_key = serde_yaml::Value::String("over".into());
    for node in nodes.iter_mut() {
        let is_foreach = node
            .get("type")
            .and_then(|v| v.as_str())
            .is_some_and(|t| t == "for-each");
        if !is_foreach {
            continue;
        }
        if let Some(m) = node.as_mapping_mut() {
            if !m.contains_key(&over_key) {
                m.insert(over_key.clone(), serde_yaml::Value::String("items".into()));
            }
        }
    }
}

/// #149: inputs are emergent. Strip declared `inputs` from regular (doc-only /
/// code-mutating) nodes. Structural nodes (start/end/merge/loop/for-each) keep
/// their required ports. Any `repeated: true` declared input is migrated onto
/// the matching incoming edge so loop accumulation is preserved.
fn drop_declared_inputs(doc: &mut serde_yaml::Value) {
    // Pass 1: collect (node_id, input_name) pairs that carried `repeated: true`,
    // and the set of regular node ids whose inputs we will drop.
    let mut repeated_targets: Vec<(String, String)> = Vec::new();
    let mut regular_node_ids: Vec<String> = Vec::new();

    if let Some(nodes) = doc.get("nodes").and_then(|n| n.as_sequence()) {
        for node in nodes {
            let node_type = node.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if matches!(
                node_type,
                "start" | "end" | "switch" | "merge" | "loop" | "for-each"
            ) {
                continue;
            }
            let Some(node_id) = node.get("id").and_then(|v| v.as_str()) else {
                continue;
            };
            regular_node_ids.push(node_id.to_string());
            if let Some(inputs) = node.get("inputs").and_then(|v| v.as_sequence()) {
                for port in inputs {
                    let is_repeated = port
                        .get("repeated")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    if is_repeated {
                        if let Some(name) = port.get("name").and_then(|v| v.as_str()) {
                            repeated_targets.push((node_id.to_string(), name.to_string()));
                        }
                    }
                }
            }
        }
    }

    // Pass 2: migrate `repeated` onto matching edges.
    if !repeated_targets.is_empty() {
        if let Some(edges) = doc.get_mut("edges").and_then(|e| e.as_sequence_mut()) {
            for edge in edges.iter_mut() {
                let matches = edge
                    .get("target")
                    .and_then(|t| t.as_mapping())
                    .map(|m| {
                        let tn = m
                            .get(serde_yaml::Value::String("node".into()))
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let tp = m
                            .get(serde_yaml::Value::String("port".into()))
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        repeated_targets.iter().any(|(n, p)| n == tn && p == tp)
                    })
                    .unwrap_or(false);
                if matches {
                    if let Some(m) = edge.as_mapping_mut() {
                        m.insert(
                            serde_yaml::Value::String("repeated".into()),
                            serde_yaml::Value::Bool(true),
                        );
                    }
                }
            }
        }
    }

    // Pass 3: drop the declared `inputs` key from regular nodes.
    if let Some(nodes) = doc.get_mut("nodes").and_then(|n| n.as_sequence_mut()) {
        for node in nodes.iter_mut() {
            let node_id = node
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if regular_node_ids.contains(&node_id) {
                if let Some(m) = node.as_mapping_mut() {
                    m.remove(serde_yaml::Value::String("inputs".into()));
                }
            }
        }
    }
}

fn backfill_port_sides(node: &mut serde_yaml::Value, key: &str, default_side: &str) {
    let ports = match node.get_mut(key).and_then(|v| v.as_sequence_mut()) {
        Some(seq) => seq,
        None => return,
    };
    let side_key = serde_yaml::Value::String("side".into());
    for port in ports.iter_mut() {
        if let Some(m) = port.as_mapping_mut() {
            if !m.contains_key(&side_key) {
                m.insert(
                    side_key.clone(),
                    serde_yaml::Value::String(default_side.into()),
                );
            }
        }
    }
}

fn rewrite_halt_edge(edge: &mut serde_yaml::Value) {
    let target = match edge.get("target") {
        Some(t) => t.clone(),
        None => return,
    };
    let halt = match target
        .as_mapping()
        .and_then(|m| m.get(serde_yaml::Value::String("halt".into())))
    {
        Some(h) => h.clone(),
        None => return,
    };

    let reason = halt
        .as_mapping()
        .and_then(|m| m.get(serde_yaml::Value::String("message".into())))
        .and_then(|v| v.as_str())
        .map(String::from);

    let mut new_target = serde_yaml::Mapping::new();
    new_target.insert(
        serde_yaml::Value::String("node".into()),
        serde_yaml::Value::String("end".into()),
    );
    new_target.insert(
        serde_yaml::Value::String("port".into()),
        serde_yaml::Value::String("result".into()),
    );

    if let Some(e) = edge.as_mapping_mut() {
        e.insert(
            serde_yaml::Value::String("target".into()),
            serde_yaml::Value::Mapping(new_target),
        );
        if let Some(reason) = reason {
            e.insert(
                serde_yaml::Value::String("reason".into()),
                serde_yaml::Value::String(reason),
            );
        }
    }
}

fn inject_start_end_nodes(doc: &mut serde_yaml::Value) {
    if has_start_end_nodes(doc) {
        return;
    }

    let nodes = match doc.get_mut("nodes").and_then(|n| n.as_sequence_mut()) {
        Some(seq) => seq,
        None => return,
    };

    let has_start = nodes_contain_type(nodes, "start");
    let has_end = nodes_contain_type(nodes, "end");

    if !has_start {
        let mut start = serde_yaml::Mapping::new();
        start.insert(
            serde_yaml::Value::String("id".into()),
            serde_yaml::Value::String("start".into()),
        );
        start.insert(
            serde_yaml::Value::String("name".into()),
            serde_yaml::Value::String("Start".into()),
        );
        start.insert(
            serde_yaml::Value::String("type".into()),
            serde_yaml::Value::String("start".into()),
        );
        let mut output_port = serde_yaml::Mapping::new();
        output_port.insert(
            serde_yaml::Value::String("name".into()),
            serde_yaml::Value::String("user_prompt".into()),
        );
        start.insert(
            serde_yaml::Value::String("outputs".into()),
            serde_yaml::Value::Sequence(vec![serde_yaml::Value::Mapping(output_port)]),
        );
        nodes.insert(0, serde_yaml::Value::Mapping(start));
    }

    if !has_end {
        let mut end = serde_yaml::Mapping::new();
        end.insert(
            serde_yaml::Value::String("id".into()),
            serde_yaml::Value::String("end".into()),
        );
        end.insert(
            serde_yaml::Value::String("name".into()),
            serde_yaml::Value::String("End".into()),
        );
        end.insert(
            serde_yaml::Value::String("type".into()),
            serde_yaml::Value::String("end".into()),
        );
        let mut input_port = serde_yaml::Mapping::new();
        input_port.insert(
            serde_yaml::Value::String("name".into()),
            serde_yaml::Value::String("result".into()),
        );
        end.insert(
            serde_yaml::Value::String("inputs".into()),
            serde_yaml::Value::Sequence(vec![serde_yaml::Value::Mapping(input_port)]),
        );
        nodes.push(serde_yaml::Value::Mapping(end));
    }
}

fn rewrite_edge_endpoint(
    edge: &mut serde_yaml::Value,
    key: &str,
    id_map: &HashMap<String, String>,
) {
    if let Some(ep) = edge.get_mut(key).and_then(|v| v.as_mapping_mut()) {
        let node_key = serde_yaml::Value::String("node".into());
        if let Some(old) = ep.get(&node_key).and_then(|v| v.as_str()).map(String::from) {
            if let Some(new_id) = id_map.get(&old) {
                ep.insert(node_key, serde_yaml::Value::String(new_id.clone()));
            }
        }
    }
}

/// Dissolves every `switch` node into guarded edges (ADR-0011). For a switch
/// `SW` fed by `(P, p_port)`, each switch output port becomes a direct edge from
/// `(P, p_port)` to wherever that switch port wired to, carrying the port's
/// `when:` clause. A switch's `default` port (no `when:`) becomes `else: true`
/// edges. The switch node and all edges touching it are removed.
fn dissolve_switches(doc: &mut serde_yaml::Value) -> Result<(), String> {
    let nodes = match doc.get("nodes").and_then(|n| n.as_sequence()) {
        Some(seq) => seq.clone(),
        None => return Ok(()),
    };

    // Collect switch node ids and their output-port `when:` clauses.
    let mut switch_ids: HashSet<String> = HashSet::new();
    let mut switch_port_when: HashMap<(String, String), Option<serde_yaml::Value>> = HashMap::new();
    for node in &nodes {
        let is_switch = node
            .get("type")
            .and_then(|v| v.as_str())
            .is_some_and(|t| t == "switch");
        if !is_switch {
            continue;
        }
        let id = match node.get("id").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => return Err("switch node missing 'id'".into()),
        };
        switch_ids.insert(id.clone());
        if let Some(outputs) = node.get("outputs").and_then(|v| v.as_sequence()) {
            for port in outputs {
                if let Some(pname) = port.get("name").and_then(|v| v.as_str()) {
                    let when = port.get("when").cloned();
                    switch_port_when.insert((id.clone(), pname.to_string()), when);
                }
            }
        }
    }

    if switch_ids.is_empty() {
        return Ok(());
    }

    let edges = match doc.get("edges").and_then(|e| e.as_sequence()) {
        Some(seq) => seq.clone(),
        None => Vec::new(),
    };

    let endpoint = |edge: &serde_yaml::Value, key: &str| -> Option<(String, String)> {
        let ep = edge.get(key)?.as_mapping()?;
        let node = ep
            .get(serde_yaml::Value::String("node".into()))?
            .as_str()?
            .to_string();
        let port = ep
            .get(serde_yaml::Value::String("port".into()))?
            .as_str()?
            .to_string();
        Some((node, port))
    };

    // Map each switch id to its inbound source `(node, port)` (the edge feeding
    // its `in` port). A switch with no inbound edge is dropped with its outbound
    // edges (sharp tool: no error — nothing routes through it).
    let mut switch_source: HashMap<String, (String, String)> = HashMap::new();
    for edge in &edges {
        if let (Some(src), Some(tgt)) = (endpoint(edge, "source"), endpoint(edge, "target")) {
            if switch_ids.contains(&tgt.0) {
                switch_source.insert(tgt.0.clone(), src);
            }
        }
    }

    // Build the new edge list: keep edges not touching a switch; for each edge
    // leaving a switch port, emit a guarded edge from the switch's source.
    let mut new_edges: Vec<serde_yaml::Value> = Vec::new();
    for edge in &edges {
        let src = endpoint(edge, "source");
        let tgt = endpoint(edge, "target");

        // Drop the edge feeding a switch's input — it is folded into the source.
        if let Some((tnode, _)) = &tgt {
            if switch_ids.contains(tnode) {
                continue;
            }
        }

        match &src {
            Some((snode, sport)) if switch_ids.contains(snode) => {
                // Edge leaving a switch output port → guarded edge from source.
                let (real_src_node, real_src_port) = match switch_source.get(snode) {
                    Some(s) => s.clone(),
                    None => continue, // switch had no inbound: nothing to route
                };
                let target = match edge.get("target") {
                    Some(t) => t.clone(),
                    None => continue,
                };

                let mut m = serde_yaml::Mapping::new();
                let mut source_map = serde_yaml::Mapping::new();
                source_map.insert(
                    serde_yaml::Value::String("node".into()),
                    serde_yaml::Value::String(real_src_node),
                );
                source_map.insert(
                    serde_yaml::Value::String("port".into()),
                    serde_yaml::Value::String(real_src_port),
                );
                m.insert(
                    serde_yaml::Value::String("source".into()),
                    serde_yaml::Value::Mapping(source_map),
                );
                m.insert(serde_yaml::Value::String("target".into()), target);

                match switch_port_when.get(&(snode.clone(), sport.clone())) {
                    Some(Some(when)) => {
                        m.insert(serde_yaml::Value::String("when".into()), when.clone());
                    }
                    _ => {
                        // `default` (or any when-less) port → else edge.
                        m.insert(
                            serde_yaml::Value::String("else".into()),
                            serde_yaml::Value::Bool(true),
                        );
                    }
                }

                // Preserve a `reason:` on the original switch-output edge if any.
                if let Some(reason) = edge.get("reason") {
                    m.insert(serde_yaml::Value::String("reason".into()), reason.clone());
                }

                new_edges.push(serde_yaml::Value::Mapping(m));
            }
            _ => {
                // Edge untouched by any switch — keep verbatim.
                new_edges.push(edge.clone());
            }
        }
    }

    // Remove switch nodes.
    if let Some(nodes_mut) = doc.get_mut("nodes").and_then(|n| n.as_sequence_mut()) {
        nodes_mut.retain(|n| {
            n.get("type")
                .and_then(|v| v.as_str())
                .is_none_or(|t| t != "switch")
        });
    }

    if let Some(m) = doc.as_mapping_mut() {
        m.insert(
            serde_yaml::Value::String("edges".into()),
            serde_yaml::Value::Sequence(new_edges),
        );
    }

    Ok(())
}

pub fn migrate_pipeline_file(pipeline_path: &Path) -> Result<bool, String> {
    let yaml_text = std::fs::read_to_string(pipeline_path)
        .map_err(|e| format!("read {}: {e}", pipeline_path.display()))?;

    let result = migrate_pipeline_yaml(&yaml_text, pipeline_path)?;
    if !result.migrated {
        return Ok(false);
    }

    for (src, dst) in &result.prompt_moves {
        let dst_path = Path::new(dst);
        if let Some(parent) = dst_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
        }
        let src_path = Path::new(src);
        if src_path.exists() {
            std::fs::rename(src_path, dst_path)
                .map_err(|e| format!("rename {} -> {}: {e}", src, dst))?;
            info!(from = %src, to = %dst, "moved prompt file");
        }
    }

    std::fs::write(pipeline_path, &result.yaml_text)
        .map_err(|e| format!("write {}: {e}", pipeline_path.display()))?;

    info!(path = %pipeline_path.display(), "migrated pipeline YAML");
    Ok(true)
}

pub fn migrate_all(pipelines_dir: &Path) -> Result<usize, String> {
    let mut count = 0;
    let entries = std::fs::read_dir(pipelines_dir)
        .map_err(|e| format!("read dir {}: {e}", pipelines_dir.display()))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("dir entry: {e}"))?;
        let path = entry.path();
        let is_yaml = path
            .extension()
            .is_some_and(|ext| ext == "yaml" || ext == "yml");
        if !is_yaml {
            continue;
        }
        match migrate_pipeline_file(&path) {
            Ok(true) => count += 1,
            Ok(false) => {}
            Err(e) => warn!(path = %path.display(), error = %e, "skipped pipeline migration"),
        }
    }
    Ok(count)
}

/// Detects fan-outs where 2+ code-mutating nodes share a common downstream
/// target but no Merge node sits between them and the target.
///
/// Returns info-only diagnostics (ADR-0001: non-blocking).
pub fn lint_missing_merge(pipeline: &PipelineDef) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    let cm_ids: HashSet<&str> = pipeline
        .nodes
        .iter()
        .filter(|n| n.node_type == NodeType::CodeMutating)
        .map(|n| n.id.as_str())
        .collect();

    let merge_ids: HashSet<&str> = pipeline
        .nodes
        .iter()
        .filter(|n| n.node_type == NodeType::Merge)
        .map(|n| n.id.as_str())
        .collect();

    let mut target_cm_sources: HashMap<&str, Vec<&str>> = HashMap::new();

    for edge in &pipeline.edges {
        let src = edge.source.node.as_str();
        let tgt = edge.target.node.as_str();
        if cm_ids.contains(src) && !merge_ids.contains(tgt) {
            target_cm_sources.entry(tgt).or_default().push(src);
        }
    }

    for (target_id, sources) in &target_cm_sources {
        if sources.len() >= 2 {
            diagnostics.push(Diagnostic {
                severity: Severity::Warning,
                message: format!(
                    "node '{}' receives edges from {} code-mutating nodes ({}) without a Merge node — \
                     parallel code changes may conflict at merge time",
                    target_id,
                    sources.len(),
                    sources.join(", "),
                ),
            });
        }
    }

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_id_is_stable() {
        let a = deterministic_id("implementer");
        let b = deterministic_id("implementer");
        assert_eq!(a, b);
        assert_eq!(a.len(), NANOID_LEN);
        assert!(a.bytes().all(|b| NANOID_ALPHABET.contains(&b)));
    }

    #[test]
    fn deterministic_id_differs_for_different_inputs() {
        let a = deterministic_id("implementer");
        let b = deterministic_id("reviewer");
        assert_ne!(a, b);
    }

    #[test]
    fn looks_like_nanoid_accepts_valid() {
        assert!(looks_like_nanoid("aBcD1234"));
        assert!(looks_like_nanoid("00000000"));
    }

    #[test]
    fn looks_like_nanoid_rejects_invalid() {
        assert!(!looks_like_nanoid("implementer"));
        assert!(!looks_like_nanoid("ab-cd_12"));
        assert!(!looks_like_nanoid("short"));
        assert!(!looks_like_nanoid(""));
    }

    #[test]
    fn idempotent_on_already_migrated() {
        let yaml = r#"
name: test
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: aBcD1234
    name: implementer
    type: code-mutating
    outputs:
      - name: code
        side: right
    view: { x: 100, y: 160 }
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges: []
"#;
        // A canonical pipeline (nanoid ids, no declared inputs on regular nodes —
        // inputs are emergent per #149) is already migrated: a no-op.
        let result = migrate_pipeline_yaml(yaml, Path::new("/tmp/test.yaml")).unwrap();
        assert!(!result.migrated);
    }

    #[test]
    fn drops_declared_inputs_on_regular_nodes() {
        // #149: inputs are emergent (derived from edges). The migrator strips the
        // now-redundant declared `inputs` from doc-only / code-mutating nodes.
        let yaml = r#"
name: test
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
  - id: aBcD1234
    name: planner
    type: doc-only
    inputs:
      - name: task
        side: left
    outputs:
      - name: plan
        side: right
edges:
  - source: { node: start, port: user_prompt }
    target: { node: aBcD1234, port: task }
"#;
        let result = migrate_pipeline_yaml(yaml, Path::new("/tmp/test.yaml")).unwrap();
        assert!(result.migrated);

        let parsed: serde_yaml::Value = serde_yaml::from_str(&result.yaml_text).unwrap();
        let nodes = parsed["nodes"].as_sequence().unwrap();
        let planner = nodes
            .iter()
            .find(|n| n["name"].as_str() == Some("planner"))
            .unwrap();
        // Declared inputs are gone; outputs are kept.
        assert!(
            planner.get("inputs").is_none()
                || planner["inputs"]
                    .as_sequence()
                    .is_some_and(|s| s.is_empty()),
            "regular node should have no declared inputs after migration"
        );
        assert!(planner["outputs"].as_sequence().is_some());
        // The End node keeps its structural `result` input.
        let end = nodes
            .iter()
            .find(|n| n["type"].as_str() == Some("end"))
            .unwrap();
        assert!(end["inputs"].as_sequence().is_some_and(|s| !s.is_empty()));
    }

    #[test]
    fn migrates_repeated_input_flag_onto_edge() {
        // #149: behavior preservation — a `repeated: true` declared input becomes
        // `repeated: true` on the matching incoming edge so accumulation survives.
        let yaml = r#"
name: cycle
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
  - id: aBcD1234
    name: reviewer
    type: doc-only
    outputs:
      - name: review
        side: right
  - id: eFgH5678
    name: implementer
    type: code-mutating
    inputs:
      - name: reviews
        side: left
        repeated: true
    outputs:
      - name: code
        side: right
edges:
  - source: { node: aBcD1234, port: review }
    target: { node: eFgH5678, port: reviews }
"#;
        let result = migrate_pipeline_yaml(yaml, Path::new("/tmp/test.yaml")).unwrap();
        assert!(result.migrated);

        let parsed: serde_yaml::Value = serde_yaml::from_str(&result.yaml_text).unwrap();
        let edges = parsed["edges"].as_sequence().unwrap();
        let edge = edges
            .iter()
            .find(|e| e["target"]["port"].as_str() == Some("reviews"))
            .unwrap();
        assert_eq!(
            edge["repeated"].as_bool(),
            Some(true),
            "repeated should migrate from the input port onto the edge"
        );

        // And the declared input is dropped.
        let nodes = parsed["nodes"].as_sequence().unwrap();
        let implementer = nodes
            .iter()
            .find(|n| n["name"].as_str() == Some("implementer"))
            .unwrap();
        assert!(
            implementer.get("inputs").is_none()
                || implementer["inputs"]
                    .as_sequence()
                    .is_some_and(|s| s.is_empty()),
            "implementer should have no declared inputs after migration"
        );
    }

    #[test]
    fn migrates_old_format_nodes() {
        let yaml = r#"
name: review-loop
version: "1.0"
nodes:
  - id: implementer
    type: code-mutating
    prompt_file: .maestro/prompts/implementer.md
    inputs:
      - name: review
    outputs:
      - name: code
    view: { x: 100, y: 160 }
  - id: reviewer
    type: doc-only
    prompt_file: .maestro/prompts/reviewer.md
    inputs:
      - name: code
    outputs:
      - name: review
    view: { x: 500, y: 160 }
edges:
  - source: { node: implementer, port: code }
    target: { node: reviewer, port: code }
  - source: { node: reviewer, port: review }
    target: { node: implementer, port: review }
"#;
        let result = migrate_pipeline_yaml(yaml, Path::new("/pipelines/review-loop.yaml")).unwrap();
        assert!(result.migrated);

        let new_impl_id = deterministic_id("implementer");
        let new_rev_id = deterministic_id("reviewer");

        let parsed: serde_yaml::Value = serde_yaml::from_str(&result.yaml_text).unwrap();

        let nodes = parsed["nodes"].as_sequence().unwrap();
        assert_eq!(nodes[0]["type"].as_str().unwrap(), "start");
        assert_eq!(nodes[1]["id"].as_str().unwrap(), new_impl_id);
        assert_eq!(nodes[1]["name"].as_str().unwrap(), "implementer");
        assert!(nodes[1].get("prompt_file").is_none());
        assert_eq!(nodes[2]["id"].as_str().unwrap(), new_rev_id);
        assert_eq!(nodes[2]["name"].as_str().unwrap(), "reviewer");
        assert_eq!(nodes[3]["type"].as_str().unwrap(), "end");

        let edges = parsed["edges"].as_sequence().unwrap();
        assert_eq!(edges[0]["source"]["node"].as_str().unwrap(), new_impl_id);
        assert_eq!(edges[0]["target"]["node"].as_str().unwrap(), new_rev_id);
        assert_eq!(edges[1]["source"]["node"].as_str().unwrap(), new_rev_id);
        assert_eq!(edges[1]["target"]["node"].as_str().unwrap(), new_impl_id);

        assert_eq!(result.prompt_moves.len(), 2);
    }

    #[test]
    fn migrates_edges_with_halt_target() {
        let yaml = r#"
name: test
version: "1.0"
nodes:
  - id: worker
    type: doc-only
    inputs: []
    outputs:
      - name: out
edges:
  - source: { node: worker, port: out }
    target:
      halt: { message: "done" }
"#;
        let result = migrate_pipeline_yaml(yaml, Path::new("/pipelines/test.yaml")).unwrap();
        assert!(result.migrated);

        let parsed: serde_yaml::Value = serde_yaml::from_str(&result.yaml_text).unwrap();

        let new_id = deterministic_id("worker");
        let edges = parsed["edges"].as_sequence().unwrap();
        assert_eq!(edges[0]["source"]["node"].as_str().unwrap(), new_id);
        assert_eq!(edges[0]["target"]["node"].as_str().unwrap(), "end");
        assert_eq!(edges[0]["target"]["port"].as_str().unwrap(), "result");
        assert_eq!(edges[0]["reason"].as_str().unwrap(), "done");

        let nodes = parsed["nodes"].as_sequence().unwrap();
        assert!(
            nodes.iter().any(|n| n["type"].as_str() == Some("start")),
            "start node should be injected"
        );
        assert!(
            nodes.iter().any(|n| n["type"].as_str() == Some("end")),
            "end node should be injected"
        );
    }

    #[test]
    fn prompt_moves_use_canonical_path() {
        let yaml = r#"
name: demo
version: "1.0"
nodes:
  - id: agent
    type: doc-only
    prompt_file: old/path/agent.md
    inputs: []
    outputs: []
edges: []
"#;
        let result = migrate_pipeline_yaml(yaml, Path::new("/pipelines/demo.yaml")).unwrap();
        assert!(result.migrated);
        assert_eq!(result.prompt_moves.len(), 1);

        let new_id = deterministic_id("agent");
        let expected_dst = format!("/pipelines/demo.prompts/{new_id}.md");
        assert_eq!(result.prompt_moves[0].0, "/pipelines/old/path/agent.md");
        assert_eq!(result.prompt_moves[0].1, expected_dst);
    }

    #[test]
    fn backfills_output_port_side_defaults() {
        // Output ports get a default `side: right` on migration. Declared inputs
        // are dropped (emergent, #149) so there are no input sides to backfill.
        let yaml = r#"
name: test
version: "1.0"
nodes:
  - id: aBcD1234
    name: worker
    type: doc-only
    inputs:
      - name: task
      - name: context
    outputs:
      - name: plan
      - name: summary
edges: []
"#;
        let result = migrate_pipeline_yaml(yaml, Path::new("/tmp/test.yaml")).unwrap();
        assert!(result.migrated);

        let parsed: serde_yaml::Value = serde_yaml::from_str(&result.yaml_text).unwrap();
        let nodes = parsed["nodes"].as_sequence().unwrap();
        let worker = nodes
            .iter()
            .find(|n| n["name"].as_str() == Some("worker"))
            .unwrap();
        let outputs = worker["outputs"].as_sequence().unwrap();

        assert!(
            worker.get("inputs").is_none()
                || worker["inputs"].as_sequence().is_some_and(|s| s.is_empty()),
            "declared inputs are dropped (emergent)"
        );
        assert_eq!(outputs[0]["side"].as_str().unwrap(), "right");
        assert_eq!(outputs[1]["side"].as_str().unwrap(), "right");
    }

    #[test]
    fn preserves_existing_output_port_side() {
        // An explicit output `side` is never overwritten by migration. Declared
        // inputs are dropped regardless (emergent, #149).
        let yaml = r#"
name: test
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
  - id: aBcD1234
    name: worker
    type: doc-only
    outputs:
      - name: plan
        side: top
edges: []
"#;
        // No declared inputs on the regular node and an explicit output side:
        // already canonical, so a no-op.
        let result = migrate_pipeline_yaml(yaml, Path::new("/tmp/test.yaml")).unwrap();
        assert!(!result.migrated);
    }

    #[test]
    fn migrate_file_on_disk() {
        let tmp = tempfile::tempdir().unwrap();
        let yaml_path = tmp.path().join("test.yaml");
        let old_prompt_dir = tmp.path().join("old_prompts");
        std::fs::create_dir_all(&old_prompt_dir).unwrap();
        std::fs::write(old_prompt_dir.join("mynode.md"), "hello prompt").unwrap();

        let yaml = "name: test\nversion: '1.0'\nnodes:\n  - id: mynode\n    type: doc-only\n    prompt_file: old_prompts/mynode.md\n    inputs: []\n    outputs: []\nedges: []\n".to_string();
        std::fs::write(&yaml_path, &yaml).unwrap();

        let migrated = migrate_pipeline_file(&yaml_path).unwrap();
        assert!(migrated);

        let new_yaml = std::fs::read_to_string(&yaml_path).unwrap();
        assert!(!new_yaml.contains("prompt_file"));
        assert!(new_yaml.contains("mynode")); // as name

        let new_id = deterministic_id("mynode");
        let canonical = tmp.path().join(format!("test.prompts/{new_id}.md"));
        assert!(canonical.exists());
        assert_eq!(std::fs::read_to_string(canonical).unwrap(), "hello prompt");

        // idempotent: second run does nothing
        let not_migrated = migrate_pipeline_file(&yaml_path).unwrap();
        assert!(!not_migrated);
    }

    #[test]
    fn migrate_all_skips_non_yaml() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("readme.md"), "# hi").unwrap();
        std::fs::write(
            tmp.path().join("pipe.yaml"),
            "name: p\nversion: '1.0'\nnodes:\n  - id: n1\n    type: doc-only\n    inputs: []\n    outputs: []\nedges: []\n",
        )
        .unwrap();

        let count = migrate_all(tmp.path()).unwrap();
        assert_eq!(count, 1);

        // second run: already migrated
        let count2 = migrate_all(tmp.path()).unwrap();
        assert_eq!(count2, 0);
    }

    #[test]
    fn switch_loop_nodes_pass_through_unchanged() {
        let yaml = r#"
name: test
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: aBcD1234
    name: my-loop
    type: loop
    max_iter: 3
    inputs:
      - name: in
        side: left
      - name: break
        side: left
    outputs:
      - name: body
        side: right
      - name: done
        side: right
  - id: xYzW5678
    name: my-switch
    type: switch
    inputs:
      - name: in
        side: left
    outputs:
      - name: pass
        side: right
        when:
          verdict: { in: [PASS] }
      - name: default
        side: right
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges: []
"#;
        let result = migrate_pipeline_yaml(yaml, Path::new("/tmp/test.yaml")).unwrap();
        // The Loop alone (everything already nanoid + sided) needs no migration;
        // but the Switch is now dissolved into guarded edges (ADR-0011).
        assert!(result.migrated);
        let migrated: PipelineDef = serde_yaml::from_str(&result.yaml_text).unwrap();
        assert!(
            !migrated
                .nodes
                .iter()
                .any(|n| n.node_type == NodeType::Switch),
            "Switch node must be removed by the migrator"
        );
    }

    #[test]
    fn accepts_when_on_edges() {
        // ADR-0011 supersedes #45: edge-level `when:` clauses are supported.
        let yaml = r#"
name: test
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: aBcD1234
    name: worker
    type: doc-only
    inputs:
      - name: task
        side: left
    outputs:
      - name: result
        side: right
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: aBcD1234, port: result }
    target: { node: end, port: result }
    when:
      iter: { lt: 3 }
"#;
        let result = migrate_pipeline_yaml(yaml, Path::new("/tmp/test.yaml"));
        assert!(result.is_ok(), "edge when: must be accepted: {result:?}");
    }

    #[test]
    fn migrates_switch_node_to_guarded_edges() {
        // A producer → Switch → {pass, default} dissolves into two edges leaving
        // the producer's output port directly: a guarded edge (when:) for `pass`,
        // an `else: true` edge for `default`. The Switch node disappears.
        // Nanoid ids + sided ports so the ONLY migration is switch dissolution.
        let yaml = r#"
name: switch-migrate
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
        side: right
  - id: aBcD1234
    name: reviewer
    type: doc-only
    inputs:
      - name: code
        side: left
    outputs:
      - name: review
        side: right
  - id: eFgH5678
    name: implementer
    type: code-mutating
    inputs:
      - name: review
        side: left
    outputs:
      - name: code
        side: right
  - id: sW1tcH00
    name: verdict-switch
    type: switch
    inputs:
      - name: in
        side: left
    outputs:
      - name: pass
        side: right
        when:
          verdict: { in: [PASS, APPROVED] }
      - name: default
        side: right
  - id: end
    name: End
    type: end
    inputs:
      - name: result
        side: left
edges:
  - source: { node: start, port: user_prompt }
    target: { node: eFgH5678, port: review }
  - source: { node: eFgH5678, port: code }
    target: { node: aBcD1234, port: code }
  - source: { node: aBcD1234, port: review }
    target: { node: sW1tcH00, port: in }
  - source: { node: sW1tcH00, port: pass }
    target: { node: end, port: result }
  - source: { node: sW1tcH00, port: default }
    target: { node: eFgH5678, port: review }
"#;
        let result = migrate_pipeline_yaml(yaml, Path::new("/tmp/test.yaml")).unwrap();
        assert!(result.migrated);

        let migrated: PipelineDef = serde_yaml::from_str(&result.yaml_text).unwrap();

        // Switch node is gone.
        assert!(
            !migrated
                .nodes
                .iter()
                .any(|n| n.node_type == NodeType::Switch),
            "Switch node must be removed"
        );

        // No edge references the switch node any more.
        assert!(
            !migrated
                .edges
                .iter()
                .any(|e| e.source.node == "sW1tcH00" || e.target.node == "sW1tcH00"),
            "no edge may reference the dissolved switch"
        );

        // The guarded `pass` branch becomes a when-edge from reviewer:review → end.
        let pass_edge = migrated
            .edges
            .iter()
            .find(|e| {
                e.source.node == "aBcD1234" && e.source.port == "review" && e.target.node == "end"
            })
            .expect("guarded pass edge from reviewer to end");
        assert!(pass_edge.when.is_some(), "pass edge keeps its when: clause");
        assert!(!pass_edge.is_else);

        // The `default` branch becomes an else-edge from reviewer:review → implementer.
        let else_edge = migrated
            .edges
            .iter()
            .find(|e| {
                e.source.node == "aBcD1234"
                    && e.source.port == "review"
                    && e.target.node == "eFgH5678"
            })
            .expect("else edge from reviewer to implementer");
        assert!(else_edge.is_else, "default branch becomes else: true");
        assert!(else_edge.when.is_none());
    }

    #[test]
    fn migrate_switch_is_idempotent() {
        let yaml = r#"
name: switch-migrate
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
        side: right
  - id: reviewer1
    name: reviewer
    type: doc-only
    inputs:
      - name: code
        side: left
    outputs:
      - name: review
        side: right
  - id: sw1
    name: verdict-switch
    type: switch
    inputs:
      - name: in
        side: left
    outputs:
      - name: pass
        side: right
        when:
          verdict: { eq: PASS }
      - name: default
        side: right
  - id: end
    name: End
    type: end
    inputs:
      - name: result
        side: left
edges:
  - source: { node: start, port: user_prompt }
    target: { node: reviewer1, port: code }
  - source: { node: reviewer1, port: review }
    target: { node: sw1, port: in }
  - source: { node: sw1, port: pass }
    target: { node: end, port: result }
  - source: { node: sw1, port: default }
    target: { node: end, port: result }
"#;
        let first = migrate_pipeline_yaml(yaml, Path::new("/tmp/test.yaml")).unwrap();
        assert!(first.migrated);
        let second = migrate_pipeline_yaml(&first.yaml_text, Path::new("/tmp/test.yaml")).unwrap();
        assert!(
            !second.migrated,
            "a migrated switch pipeline must not migrate again"
        );
    }

    // --- lint_missing_merge tests (issue #61) ---

    use crate::pipeline::{EdgeDef, EdgeEndpoint, NodeDef, Port, PortSide, PortType};

    fn make_cm_node(id: &str) -> NodeDef {
        NodeDef {
            id: id.into(),
            name: id.into(),
            node_type: NodeType::CodeMutating,
            inputs: vec![Port {
                name: "in".into(),
                repeated: false,
                side: Some(PortSide::Left),
                port_type: PortType::Markdown,
                frontmatter: None,
                when: None,
                description: None,
            }],
            outputs: vec![Port {
                name: "out".into(),
                repeated: false,
                side: Some(PortSide::Right),
                port_type: PortType::Markdown,
                frontmatter: None,
                when: None,
                description: None,
            }],
            interactive: false,
            view: None,
            max_iter: None,
            over: None,
        }
    }

    fn make_merge_node(id: &str) -> NodeDef {
        NodeDef {
            id: id.into(),
            name: id.into(),
            node_type: NodeType::Merge,
            inputs: vec![Port {
                name: "branches".into(),
                repeated: true,
                side: Some(PortSide::Left),
                port_type: PortType::Markdown,
                frontmatter: None,
                when: None,
                description: None,
            }],
            outputs: vec![Port {
                name: "merged".into(),
                repeated: false,
                side: Some(PortSide::Right),
                port_type: PortType::Markdown,
                frontmatter: None,
                when: None,
                description: None,
            }],
            interactive: false,
            view: None,
            max_iter: None,
            over: None,
        }
    }

    fn make_doc_node(id: &str) -> NodeDef {
        NodeDef {
            id: id.into(),
            name: id.into(),
            node_type: NodeType::DocOnly,
            inputs: vec![Port {
                name: "in".into(),
                repeated: false,
                side: Some(PortSide::Left),
                port_type: PortType::Markdown,
                frontmatter: None,
                when: None,
                description: None,
            }],
            outputs: vec![Port {
                name: "out".into(),
                repeated: false,
                side: Some(PortSide::Right),
                port_type: PortType::Markdown,
                frontmatter: None,
                when: None,
                description: None,
            }],
            interactive: false,
            view: None,
            max_iter: None,
            over: None,
        }
    }

    fn make_edge(src: &str, src_port: &str, tgt: &str, tgt_port: &str) -> EdgeDef {
        EdgeDef {
            source: EdgeEndpoint {
                node: src.into(),
                port: src_port.into(),
            },
            target: EdgeEndpoint {
                node: tgt.into(),
                port: tgt_port.into(),
            },
            reason: None,
            when: None,
            is_else: false,
            repeated: false,
            ..Default::default()
        }
    }

    #[test]
    fn lint_flags_fan_out_cm_without_merge() {
        let pipeline = PipelineDef {
            name: "fan-out-no-merge".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_cm_node("impl-a"),
                make_cm_node("impl-b"),
                make_doc_node("reviewer"),
            ],
            edges: vec![
                make_edge("impl-a", "out", "reviewer", "in"),
                make_edge("impl-b", "out", "reviewer", "in"),
            ],
            loops: Vec::new(),
        };
        let diags = lint_missing_merge(&pipeline);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("reviewer"));
        assert!(diags[0].message.contains("impl-a"));
        assert!(diags[0].message.contains("impl-b"));
    }

    #[test]
    fn lint_no_warning_when_merge_present() {
        let pipeline = PipelineDef {
            name: "fan-out-with-merge".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_cm_node("impl-a"),
                make_cm_node("impl-b"),
                make_merge_node("merger"),
                make_doc_node("downstream"),
            ],
            edges: vec![
                make_edge("impl-a", "out", "merger", "branches"),
                make_edge("impl-b", "out", "merger", "branches"),
                make_edge("merger", "merged", "downstream", "in"),
            ],
            loops: Vec::new(),
        };
        let diags = lint_missing_merge(&pipeline);
        assert!(
            diags.is_empty(),
            "Merge downstream should suppress lint, got: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn lint_no_warning_for_single_cm_source() {
        let pipeline = PipelineDef {
            name: "single-cm".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![make_cm_node("impl-a"), make_doc_node("reviewer")],
            edges: vec![make_edge("impl-a", "out", "reviewer", "in")],
            loops: Vec::new(),
        };
        let diags = lint_missing_merge(&pipeline);
        assert!(diags.is_empty());
    }

    #[test]
    fn lint_no_warning_for_doc_only_fan_out() {
        let pipeline = PipelineDef {
            name: "doc-fan-out".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_doc_node("plan-a"),
                make_doc_node("plan-b"),
                make_doc_node("summary"),
            ],
            edges: vec![
                make_edge("plan-a", "out", "summary", "in"),
                make_edge("plan-b", "out", "summary", "in"),
            ],
            loops: Vec::new(),
        };
        let diags = lint_missing_merge(&pipeline);
        assert!(diags.is_empty());
    }

    // --- ForEach `over` migration tests (issue #65) ---

    #[test]
    fn migrates_foreach_without_over_to_over_items() {
        let yaml = r#"
name: test
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: aBcD1234
    name: per-issue
    type: for-each
    inputs:
      - name: in
        side: left
      - name: break
        side: left
    outputs:
      - name: body
        side: right
      - name: done
        side: right
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges: []
"#;
        let result = migrate_pipeline_yaml(yaml, Path::new("/tmp/test.yaml")).unwrap();
        assert!(result.migrated);

        let parsed: serde_yaml::Value = serde_yaml::from_str(&result.yaml_text).unwrap();
        let nodes = parsed["nodes"].as_sequence().unwrap();
        let fe = nodes
            .iter()
            .find(|n| n["type"].as_str() == Some("for-each"))
            .unwrap();
        assert_eq!(fe["over"].as_str().unwrap(), "items");
    }

    #[test]
    fn foreach_with_existing_over_not_overwritten() {
        let yaml = r#"
name: test
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: aBcD1234
    name: per-issue
    type: for-each
    over: tasks
    inputs:
      - name: in
        side: left
      - name: break
        side: left
    outputs:
      - name: body
        side: right
      - name: done
        side: right
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges: []
"#;
        let result = migrate_pipeline_yaml(yaml, Path::new("/tmp/test.yaml")).unwrap();
        assert!(
            !result.migrated,
            "pipeline with over already set should not need migration"
        );
    }

    // --- Real fixtures: Switch → guarded edges (issue #144) ---

    fn migrate_str_and_parse(yaml: &str) -> PipelineDef {
        let result = migrate_pipeline_yaml(yaml, Path::new("/tmp/fixture.yaml")).unwrap();
        // After migration there must be no Switch left, and the result re-parses.
        let parsed = pipeline::parse_pipeline(&result.yaml_text)
            .expect("migrated fixture must parse")
            .pipeline;
        assert!(
            !parsed.nodes.iter().any(|n| n.node_type == NodeType::Switch),
            "no Switch node may remain after migration"
        );
        assert!(
            !parsed.edges.iter().any(|e| {
                parsed.nodes.iter().any(|n| {
                    n.node_type == NodeType::Switch
                        && (n.id == e.source.node || n.id == e.target.node)
                })
            }),
            "no edge may reference a Switch node"
        );
        parsed
    }

    #[test]
    fn migrates_review_loop_fixture_switch_to_guarded_edges() {
        let yaml = include_str!("../../../.maestro/pipelines/review-loop.yaml");
        let parsed = migrate_str_and_parse(yaml);
        // The dissolved switch leaves at least one guarded (when:) or else edge.
        assert!(
            parsed.edges.iter().any(|e| e.when.is_some() || e.is_else),
            "review-loop should have conditional edges after migration"
        );
        // Idempotent: migrating the result again is a no-op.
        let first = migrate_pipeline_yaml(yaml, Path::new("/tmp/fixture.yaml")).unwrap();
        let second =
            migrate_pipeline_yaml(&first.yaml_text, Path::new("/tmp/fixture.yaml")).unwrap();
        assert!(!second.migrated, "migration must be idempotent");
    }

    #[test]
    fn migrates_simple_bugfix_fixture_switches_to_guarded_edges() {
        let yaml = include_str!("../../../.maestro/pipelines/simple-bugfix.yaml");
        let parsed = migrate_str_and_parse(yaml);
        assert!(
            parsed.edges.iter().any(|e| e.when.is_some() || e.is_else),
            "simple-bugfix should have conditional edges after migration"
        );
        let first = migrate_pipeline_yaml(yaml, Path::new("/tmp/fixture.yaml")).unwrap();
        let second =
            migrate_pipeline_yaml(&first.yaml_text, Path::new("/tmp/fixture.yaml")).unwrap();
        assert!(!second.migrated, "migration must be idempotent");
    }

    #[test]
    fn migrates_planner_fixture_no_switch_unchanged_topology() {
        // planner has no Switch — migration must not introduce conditional edges.
        let yaml = include_str!("../../../.maestro/pipelines/planner.yaml");
        let result = migrate_pipeline_yaml(yaml, Path::new("/tmp/fixture.yaml")).unwrap();
        let parsed = pipeline::parse_pipeline(&result.yaml_text)
            .unwrap()
            .pipeline;
        assert!(
            !parsed.edges.iter().any(|e| e.when.is_some() || e.is_else),
            "planner has no switch, so no conditional edges should appear"
        );
    }
}
