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
        let node_type = node.get("type").and_then(|v| v.as_str()).unwrap_or("");
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

    // Dissolve Loop nodes into a `loops:` region + rewired body edges (#148),
    // also before id-rewriting so members/edges reference the original ids.
    dissolve_loops(&mut doc)?;

    // Dissolve ForEach nodes into a `loops:` collection region + rewired body
    // edges (#151), also before id-rewriting.
    dissolve_foreaches(&mut doc)?;

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

    let yaml_text =
        serde_yaml::to_string(&doc).map_err(|e| format!("YAML serialize error: {e}"))?;

    Ok(MigrateResult {
        migrated: true,
        yaml_text,
        prompt_moves,
    })
}

/// Dissolves `ForEach` nodes into a `loops:` collection region plus rewired body
/// edges (ADR-0011 / #151). For each ForEach node `FE` with `over: O`:
///
/// - `members` = the body subgraph reachable from `FE.body`'s target (the entry),
///   stopping at other control nodes. `entry` = the target of `FE.body`.
/// - `U.p -> FE.in` followed by `FE.body -> E.q` becomes `U.p -> E.q` (the
///   collection is entered directly at its body entry).
/// - `FE.done -> D.r` becomes the **barrier** edge `T.o -> D.r`, where `T` is the
///   body terminal (the member with no outgoing edge to another member) and `o`
///   its first output port. The region's outgoing edges fire once when all items
///   finish, preserving `done -> Merge` convergence (ADR-0006).
/// - the ForEach node is removed and a `{ id, kind: collection, over, members }`
///   entry is appended to the pipeline's `loops:` block (no `max_iter`).
fn dissolve_foreaches(doc: &mut serde_yaml::Value) -> Result<(), String> {
    let nodes = match doc.get("nodes").and_then(|n| n.as_sequence()) {
        Some(seq) => seq.clone(),
        None => return Ok(()),
    };

    // Collect ForEach nodes: id -> (name, over).
    let mut fe_ids: HashSet<String> = HashSet::new();
    let mut fe_meta: HashMap<String, (String, String)> = HashMap::new();
    for node in &nodes {
        let is_fe = node
            .get("type")
            .and_then(|v| v.as_str())
            .is_some_and(|t| t == "for-each");
        if !is_fe {
            continue;
        }
        let id = match node.get("id").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => return Err("for-each node missing 'id'".into()),
        };
        let name = node
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or(&id)
            .to_string();
        // `over` defaults to `items` when absent (legacy ForEach default).
        let over = node
            .get("over")
            .and_then(|v| v.as_str())
            .unwrap_or("items")
            .to_string();
        fe_ids.insert(id.clone());
        fe_meta.insert(id, (name, over));
    }

    if fe_ids.is_empty() {
        return Ok(());
    }

    let edges = match doc.get("edges").and_then(|e| e.as_sequence()) {
        Some(seq) => seq.clone(),
        None => Vec::new(),
    };

    let endpoint = edge_endpoint;
    let mk_endpoint = |node: &str, port: &str| -> serde_yaml::Value {
        let mut m = serde_yaml::Mapping::new();
        m.insert(
            serde_yaml::Value::String("node".into()),
            serde_yaml::Value::String(node.to_string()),
        );
        m.insert(
            serde_yaml::Value::String("port".into()),
            serde_yaml::Value::String(port.to_string()),
        );
        serde_yaml::Value::Mapping(m)
    };

    // Per-ForEach wiring: entry (body target), upstream-in edges, done targets.
    let mut fe_entry: HashMap<String, (String, String)> = HashMap::new();
    let mut fe_in_sources: HashMap<String, Vec<(String, String)>> = HashMap::new();
    let mut fe_done_targets: HashMap<String, Vec<(String, String)>> = HashMap::new();

    for edge in &edges {
        let src = endpoint(edge, "source");
        let tgt = endpoint(edge, "target");
        if let Some((snode, sport)) = &src {
            if fe_ids.contains(snode) {
                match sport.as_str() {
                    "body" => {
                        if let Some((tnode, tport)) = &tgt {
                            fe_entry.insert(snode.clone(), (tnode.clone(), tport.clone()));
                        }
                    }
                    "done" => {
                        if let Some((tnode, tport)) = &tgt {
                            fe_done_targets
                                .entry(snode.clone())
                                .or_default()
                                .push((tnode.clone(), tport.clone()));
                        }
                    }
                    _ => {}
                }
            }
        }
        if let Some((tnode, tport)) = &tgt {
            if fe_ids.contains(tnode) && tport == "in" {
                if let Some(s) = &src {
                    fe_in_sources
                        .entry(tnode.clone())
                        .or_default()
                        .push(s.clone());
                }
            }
        }
    }

    // Rebuild the edge list, dropping every edge touching a ForEach port and
    // emitting the rewired equivalents.
    let mut new_edges: Vec<serde_yaml::Value> = Vec::new();
    for edge in &edges {
        let src = endpoint(edge, "source");
        let tgt = endpoint(edge, "target");
        let touches_fe = src.as_ref().is_some_and(|(n, _)| fe_ids.contains(n))
            || tgt.as_ref().is_some_and(|(n, _)| fe_ids.contains(n));
        if !touches_fe {
            new_edges.push(edge.clone());
        }
    }

    let mut regions: Vec<serde_yaml::Value> = Vec::new();
    for fe_id in &fe_ids {
        let entry = match fe_entry.get(fe_id) {
            Some(e) => e.clone(),
            None => continue, // ForEach with no body — nothing to enter
        };

        let members = body_members(&edges, fe_id, &entry.0, &fe_ids);

        // Entering edge: U.p -> FE.in  +  FE.body -> E.q  ==>  U.p -> E.q.
        if let Some(sources) = fe_in_sources.get(fe_id) {
            for (unode, uport) in sources {
                let mut m = serde_yaml::Mapping::new();
                m.insert(
                    serde_yaml::Value::String("source".into()),
                    mk_endpoint(unode, uport),
                );
                m.insert(
                    serde_yaml::Value::String("target".into()),
                    mk_endpoint(&entry.0, &entry.1),
                );
                new_edges.push(serde_yaml::Value::Mapping(m));
            }
        }

        // Barrier edge: FE.done -> D.r  ==>  T.o -> D.r, where T is the body
        // terminal (a member with no outgoing edge to another member) and o its
        // first output port.
        let (terminal, out_port) = body_terminal(&nodes, &edges, &members, &fe_ids);
        if let Some(done_targets) = fe_done_targets.get(fe_id) {
            for (dnode, dport) in done_targets {
                let mut m = serde_yaml::Mapping::new();
                m.insert(
                    serde_yaml::Value::String("source".into()),
                    mk_endpoint(&terminal, &out_port),
                );
                m.insert(
                    serde_yaml::Value::String("target".into()),
                    mk_endpoint(dnode, dport),
                );
                new_edges.push(serde_yaml::Value::Mapping(m));
            }
        }

        let (name, over) = fe_meta.get(fe_id).cloned().unwrap_or_default();
        let mut region = serde_yaml::Mapping::new();
        region.insert(
            serde_yaml::Value::String("id".into()),
            serde_yaml::Value::String(name),
        );
        region.insert(
            serde_yaml::Value::String("kind".into()),
            serde_yaml::Value::String("collection".into()),
        );
        region.insert(
            serde_yaml::Value::String("over".into()),
            serde_yaml::Value::String(over),
        );
        region.insert(
            serde_yaml::Value::String("members".into()),
            serde_yaml::Value::Sequence(
                members.into_iter().map(serde_yaml::Value::String).collect(),
            ),
        );
        regions.push(serde_yaml::Value::Mapping(region));
    }

    // Remove ForEach nodes.
    if let Some(nodes_mut) = doc.get_mut("nodes").and_then(|n| n.as_sequence_mut()) {
        nodes_mut.retain(|n| {
            n.get("type")
                .and_then(|v| v.as_str())
                .is_none_or(|t| t != "for-each")
        });
    }

    if let Some(m) = doc.as_mapping_mut() {
        m.insert(
            serde_yaml::Value::String("edges".into()),
            serde_yaml::Value::Sequence(new_edges),
        );
        if !regions.is_empty() {
            // Append to any existing `loops:` (e.g. a Loop region migrated first).
            let mut all = match m.get(serde_yaml::Value::String("loops".into())) {
                Some(serde_yaml::Value::Sequence(s)) => s.clone(),
                _ => Vec::new(),
            };
            all.extend(regions);
            m.insert(
                serde_yaml::Value::String("loops".into()),
                serde_yaml::Value::Sequence(all),
            );
        }
    }

    Ok(())
}

/// Picks the body terminal of a collection region: the member with no outgoing
/// edge to another member (a sink within the region), and its first output port.
/// For a single-member region this is the member itself. The terminal feeds the
/// barrier edge to each done-target. Falls back to the first member / output
/// `out` when ambiguous (sharp tool — deterministic, never panics).
fn body_terminal(
    nodes: &[serde_yaml::Value],
    edges: &[serde_yaml::Value],
    members: &[String],
    fe_ids: &HashSet<String>,
) -> (String, String) {
    let member_set: HashSet<&str> = members.iter().map(String::as_str).collect();
    let endpoint = edge_endpoint;

    let terminal = members
        .iter()
        .find(|m| {
            // A terminal has no outgoing edge to another member (its output
            // leaves the region or dangles).
            !edges.iter().any(|e| {
                let src = endpoint(e, "source");
                let tgt = endpoint(e, "target");
                matches!((&src, &tgt), (Some((s, _)), Some((t, _)))
                    if s == *m && member_set.contains(t.as_str()) && !fe_ids.contains(t))
            })
        })
        .or_else(|| members.last())
        .cloned()
        .unwrap_or_default();

    let out_port = nodes
        .iter()
        .find(|n| n.get("id").and_then(|v| v.as_str()) == Some(terminal.as_str()))
        .and_then(|n| n.get("outputs").and_then(|v| v.as_sequence()))
        .and_then(|outs| outs.first())
        .and_then(|p| p.get("name").and_then(|v| v.as_str()))
        .unwrap_or("out")
        .to_string();

    (terminal, out_port)
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

/// Extracts an edge endpoint's `(node, port)` for the given side key
/// (`"source"` / `"target"`). Returns `None` if the endpoint is malformed.
fn edge_endpoint(edge: &serde_yaml::Value, key: &str) -> Option<(String, String)> {
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
}

/// Dissolves `Loop` nodes into a `loops:` region plus rewired body edges
/// (ADR-0011 / #148). For each Loop node `L`:
///
/// - `members` = the body subgraph (nodes reachable from `L.body` until they
///   route back to `L`'s `break`/`done`). `entry` = the target of `L.body`.
/// - `U.p -> L.in` followed by `L.body -> E.q` becomes `U.p -> E.q` (the loop is
///   entered directly at its body entry).
/// - each break/exit edge `X.p -> L.break [when W]` paired with each completion
///   edge `L.done -> D.r` becomes the exhaustion/exit edge `X.p -> D.r [when W]`,
///   plus the continuation back-edge `X.p -> E.q [else]` (loop again when the
///   exit guard is false). When the break edge is unconditional, only the exit
///   edge is emitted (the loop always leaves).
/// - the Loop node is removed and a `{ id, kind: bounded, members, max_iter }`
///   entry is appended to the pipeline's `loops:` block.
fn dissolve_loops(doc: &mut serde_yaml::Value) -> Result<(), String> {
    let nodes = match doc.get("nodes").and_then(|n| n.as_sequence()) {
        Some(seq) => seq.clone(),
        None => return Ok(()),
    };

    // Collect loop nodes: id -> (name, max_iter).
    let mut loop_ids: HashSet<String> = HashSet::new();
    let mut loop_meta: HashMap<String, (String, Option<serde_yaml::Value>)> = HashMap::new();
    for node in &nodes {
        let is_loop = node
            .get("type")
            .and_then(|v| v.as_str())
            .is_some_and(|t| t == "loop");
        if !is_loop {
            continue;
        }
        let id = match node.get("id").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => return Err("loop node missing 'id'".into()),
        };
        let name = node
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or(&id)
            .to_string();
        let max_iter = node.get("max_iter").cloned();
        loop_ids.insert(id.clone());
        loop_meta.insert(id, (name, max_iter));
    }

    if loop_ids.is_empty() {
        return Ok(());
    }

    let edges = match doc.get("edges").and_then(|e| e.as_sequence()) {
        Some(seq) => seq.clone(),
        None => Vec::new(),
    };

    let endpoint = edge_endpoint;
    let mk_endpoint = |node: &str, port: &str| -> serde_yaml::Value {
        let mut m = serde_yaml::Mapping::new();
        m.insert(
            serde_yaml::Value::String("node".into()),
            serde_yaml::Value::String(node.to_string()),
        );
        m.insert(
            serde_yaml::Value::String("port".into()),
            serde_yaml::Value::String(port.to_string()),
        );
        serde_yaml::Value::Mapping(m)
    };

    // Per-loop wiring: entry (body target), upstream-in edges, break edges,
    // done edges.
    let mut loop_entry: HashMap<String, (String, String)> = HashMap::new(); // loop -> (entry node, entry port)
    let mut loop_in_sources: HashMap<String, Vec<(String, String)>> = HashMap::new(); // loop -> [(U node, U port)]
    let mut loop_break_edges: HashMap<String, Vec<serde_yaml::Value>> = HashMap::new();
    let mut loop_done_targets: HashMap<String, Vec<(String, String)>> = HashMap::new();

    for edge in &edges {
        let src = endpoint(edge, "source");
        let tgt = endpoint(edge, "target");
        if let Some((snode, sport)) = &src {
            if loop_ids.contains(snode) {
                match sport.as_str() {
                    "body" => {
                        if let Some((tnode, tport)) = &tgt {
                            loop_entry.insert(snode.clone(), (tnode.clone(), tport.clone()));
                        }
                    }
                    "done" => {
                        if let Some((tnode, tport)) = &tgt {
                            loop_done_targets
                                .entry(snode.clone())
                                .or_default()
                                .push((tnode.clone(), tport.clone()));
                        }
                    }
                    _ => {}
                }
            }
        }
        if let Some((tnode, tport)) = &tgt {
            if loop_ids.contains(tnode) {
                match tport.as_str() {
                    "in" => {
                        if let Some(s) = &src {
                            loop_in_sources
                                .entry(tnode.clone())
                                .or_default()
                                .push(s.clone());
                        }
                    }
                    "break" => {
                        loop_break_edges
                            .entry(tnode.clone())
                            .or_default()
                            .push(edge.clone());
                    }
                    _ => {}
                }
            }
        }
    }

    // Rebuild the edge list, dropping every edge touching a loop port and
    // emitting the rewired equivalents.
    let mut new_edges: Vec<serde_yaml::Value> = Vec::new();
    for edge in &edges {
        let src = endpoint(edge, "source");
        let tgt = endpoint(edge, "target");
        let touches_loop = src.as_ref().is_some_and(|(n, _)| loop_ids.contains(n))
            || tgt.as_ref().is_some_and(|(n, _)| loop_ids.contains(n));
        if !touches_loop {
            new_edges.push(edge.clone());
        }
    }

    for loop_id in &loop_ids {
        let entry = match loop_entry.get(loop_id) {
            Some(e) => e.clone(),
            None => continue, // loop with no body — nothing to enter
        };

        // U.p -> L.in  +  L.body -> E.q   ==>   U.p -> E.q
        if let Some(sources) = loop_in_sources.get(loop_id) {
            for (unode, uport) in sources {
                let mut m = serde_yaml::Mapping::new();
                m.insert(
                    serde_yaml::Value::String("source".into()),
                    mk_endpoint(unode, uport),
                );
                m.insert(
                    serde_yaml::Value::String("target".into()),
                    mk_endpoint(&entry.0, &entry.1),
                );
                new_edges.push(serde_yaml::Value::Mapping(m));
            }
        }

        // X.p -> L.break [when W]  +  L.done -> D.r   ==>
        //   X.p -> D.r [when W]                (exit / exhaustion route)
        //   X.p -> E.q [else]                  (continuation back-edge, if W)
        let done_targets = loop_done_targets.get(loop_id).cloned().unwrap_or_default();
        if let Some(break_edges) = loop_break_edges.get(loop_id) {
            for be in break_edges {
                let bsrc = match endpoint(be, "source") {
                    Some(s) => s,
                    None => continue,
                };
                let when = be.get("when").cloned();
                let reason = be.get("reason").cloned();

                for (dnode, dport) in &done_targets {
                    let mut m = serde_yaml::Mapping::new();
                    m.insert(
                        serde_yaml::Value::String("source".into()),
                        mk_endpoint(&bsrc.0, &bsrc.1),
                    );
                    m.insert(
                        serde_yaml::Value::String("target".into()),
                        mk_endpoint(dnode, dport),
                    );
                    if let Some(w) = &when {
                        m.insert(serde_yaml::Value::String("when".into()), w.clone());
                    }
                    if let Some(r) = &reason {
                        m.insert(serde_yaml::Value::String("reason".into()), r.clone());
                    }
                    new_edges.push(serde_yaml::Value::Mapping(m));
                }

                // Continuation back-edge (loop again when the guard is false). Only
                // needed when the break edge is guarded; an unconditional break
                // always leaves the loop.
                if when.is_some() {
                    let mut m = serde_yaml::Mapping::new();
                    m.insert(
                        serde_yaml::Value::String("source".into()),
                        mk_endpoint(&bsrc.0, &bsrc.1),
                    );
                    m.insert(
                        serde_yaml::Value::String("target".into()),
                        mk_endpoint(&entry.0, &entry.1),
                    );
                    m.insert(
                        serde_yaml::Value::String("else".into()),
                        serde_yaml::Value::Bool(true),
                    );
                    new_edges.push(serde_yaml::Value::Mapping(m));
                }
            }
        }
    }

    // Compute members (body subgraph) for each loop from the *rewired* graph is
    // tricky; instead derive them from the original edges: BFS from each loop's
    // entry, following edges, stopping at the loop node and at other loops.
    let mut regions: Vec<serde_yaml::Value> = Vec::new();
    for loop_id in &loop_ids {
        let entry = match loop_entry.get(loop_id) {
            Some(e) => e.0.clone(),
            None => continue,
        };
        let members = body_members(&edges, loop_id, &entry, &loop_ids);
        let (name, max_iter) = loop_meta.get(loop_id).cloned().unwrap_or_default();

        let mut region = serde_yaml::Mapping::new();
        region.insert(
            serde_yaml::Value::String("id".into()),
            serde_yaml::Value::String(name),
        );
        region.insert(
            serde_yaml::Value::String("kind".into()),
            serde_yaml::Value::String("bounded".into()),
        );
        region.insert(
            serde_yaml::Value::String("members".into()),
            serde_yaml::Value::Sequence(
                members.into_iter().map(serde_yaml::Value::String).collect(),
            ),
        );
        if let Some(mi) = max_iter {
            region.insert(serde_yaml::Value::String("max_iter".into()), mi);
        }
        regions.push(serde_yaml::Value::Mapping(region));
    }

    // Remove loop nodes.
    if let Some(nodes_mut) = doc.get_mut("nodes").and_then(|n| n.as_sequence_mut()) {
        nodes_mut.retain(|n| {
            n.get("type")
                .and_then(|v| v.as_str())
                .is_none_or(|t| t != "loop")
        });
    }

    if let Some(m) = doc.as_mapping_mut() {
        m.insert(
            serde_yaml::Value::String("edges".into()),
            serde_yaml::Value::Sequence(new_edges),
        );
        if !regions.is_empty() {
            m.insert(
                serde_yaml::Value::String("loops".into()),
                serde_yaml::Value::Sequence(regions),
            );
        }
    }

    Ok(())
}

/// BFS the body subgraph of a loop from its entry, following edges, stopping at
/// the loop node itself and at any other loop. Returns members sorted for a
/// stable migration output.
fn body_members(
    edges: &[serde_yaml::Value],
    loop_id: &str,
    entry: &str,
    loop_ids: &HashSet<String>,
) -> Vec<String> {
    let endpoint = edge_endpoint;
    let mut members: HashSet<String> = HashSet::new();
    let mut queue = vec![entry.to_string()];
    while let Some(current) = queue.pop() {
        if current == loop_id || (loop_ids.contains(&current) && current != entry) {
            continue;
        }
        if !members.insert(current.clone()) {
            continue;
        }
        for edge in edges {
            if let Some((snode, _)) = endpoint(edge, "source") {
                if snode != current {
                    continue;
                }
                if let Some((tnode, _)) = endpoint(edge, "target") {
                    // Don't traverse into the loop control node or other loops.
                    if tnode == loop_id || loop_ids.contains(&tnode) {
                        continue;
                    }
                    if !members.contains(&tnode) {
                        queue.push(tnode);
                    }
                }
            }
        }
    }
    let mut out: Vec<String> = members.into_iter().collect();
    out.sort();
    out
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

/// One-shot boot migration for #231. The run-scoped pipeline save used to write
/// node prompts to a flat `<pipelines>/prompts/<id>.md` directory that no reader
/// ever consults — the canonical location is
/// `<pipelines>/<stem>.prompts/<id>.md`. As a result, prompts edited from inside
/// a run were saved to disk but appeared blank in the editor and to freshly
/// spawned nodes.
///
/// For each pipeline YAML in `pipelines_dir`, every declared node id whose prompt
/// is stranded in the flat dir is moved into that pipeline's canonical
/// `<stem>.prompts/` dir. Node ids are globally unique (nanoids), so each flat
/// file reattaches to at most one pipeline.
///
/// Move semantics mirror [`migrate_pipeline_file`]: a flat prompt is moved only
/// when the canonical file is *missing*. An existing canonical file is never
/// clobbered — it is authoritative (the writer fix keeps it current), and the
/// stale flat duplicate is left in place so the conflict stays visible.
///
/// Afterward the flat dir is removed *only if the migration emptied it*. A
/// non-empty remainder (a prompt for a deleted pipeline, or a flat duplicate of
/// an existing canonical file) is preserved rather than destroyed.
///
/// Returns the number of prompt files moved.
pub fn migrate_stranded_flat_prompts(pipelines_dir: &Path) -> Result<usize, String> {
    let flat_dir = pipelines_dir.join("prompts");
    if !flat_dir.is_dir() {
        return Ok(0);
    }

    let entries = std::fs::read_dir(pipelines_dir)
        .map_err(|e| format!("read dir {}: {e}", pipelines_dir.display()))?;

    let mut moved = 0usize;
    for entry in entries {
        let entry = entry.map_err(|e| format!("dir entry: {e}"))?;
        let path = entry.path();
        let is_yaml = path
            .extension()
            .is_some_and(|ext| ext == "yaml" || ext == "yml");
        if !is_yaml {
            continue;
        }

        let yaml_text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                warn!(path = %path.display(), error = %e,
                    "skipped flat-prompt migration: read failed");
                continue;
            }
        };
        let doc: serde_yaml::Value = match serde_yaml::from_str(&yaml_text) {
            Ok(v) => v,
            Err(e) => {
                warn!(path = %path.display(), error = %e,
                    "skipped flat-prompt migration: YAML parse failed");
                continue;
            }
        };
        let nodes = match doc.get("nodes").and_then(|n| n.as_sequence()) {
            Some(seq) => seq,
            None => continue,
        };

        for node in nodes {
            let Some(node_id) = node.get("id").and_then(|v| v.as_str()) else {
                continue;
            };
            let flat_path = flat_dir.join(format!("{node_id}.md"));
            if !flat_path.is_file() {
                continue;
            }
            let canonical = pipeline::canonical_prompt_path(&path, node_id);
            if canonical.exists() {
                // Canonical is authoritative; never clobber. Leave the dead flat
                // duplicate so the flat dir survives and the conflict is visible.
                warn!(
                    flat = %flat_path.display(),
                    canonical = %canonical.display(),
                    "flat prompt has a canonical counterpart — leaving flat copy in place (#231)"
                );
                continue;
            }
            if let Some(parent) = canonical.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
            }
            std::fs::rename(&flat_path, &canonical).map_err(|e| {
                format!(
                    "rename {} -> {}: {e}",
                    flat_path.display(),
                    canonical.display()
                )
            })?;
            info!(
                from = %flat_path.display(),
                to = %canonical.display(),
                "moved stranded flat prompt to canonical dir (#231)"
            );
            moved += 1;
        }
    }

    // Remove the flat dir only if the migration emptied it. A non-empty
    // remainder means files we could not reattach — preserve them.
    match std::fs::read_dir(&flat_dir) {
        Ok(mut it) => {
            if it.next().is_none() {
                match std::fs::remove_dir(&flat_dir) {
                    Ok(()) => info!(path = %flat_dir.display(),
                        "removed dead flat prompts dir (#231)"),
                    Err(e) => warn!(path = %flat_dir.display(), error = %e,
                        "failed to remove empty flat prompts dir"),
                }
            } else {
                warn!(path = %flat_dir.display(),
                    "flat prompts dir not empty after migration — \
                     leaving unreattached prompts in place (#231)");
            }
        }
        Err(e) => warn!(path = %flat_dir.display(), error = %e,
            "could not re-scan flat prompts dir after migration"),
    }

    Ok(moved)
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
    prompt_file: .pdo/prompts/implementer.md
    inputs:
      - name: review
    outputs:
      - name: code
    view: { x: 100, y: 160 }
  - id: reviewer
    type: doc-only
    prompt_file: .pdo/prompts/reviewer.md
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
            model: None,
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
            model: None,
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
            model: None,
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
            prompt_required: true,
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
            prompt_required: true,
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
            prompt_required: true,
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
            prompt_required: true,
        };
        let diags = lint_missing_merge(&pipeline);
        assert!(diags.is_empty());
    }

    // --- ForEach `over` migration tests (issue #65) ---

    #[test]
    fn migrates_foreach_without_over_defaults_to_items() {
        // ADR-0011 / #151: a ForEach node without an explicit `over` dissolves
        // into a collection region whose `over` defaults to `items` (the legacy
        // ForEach default). The ForEach node itself is gone.
        let yaml = r#"
name: test
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
        side: right
  - id: aBcD1234
    name: lister
    type: doc-only
    outputs:
      - name: plan
        side: right
  - id: feNODE01
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
  - id: wRkR5678
    name: worker
    type: code-mutating
    outputs:
      - name: out
        side: right
  - id: end
    name: End
    type: end
    inputs:
      - name: result
        side: left
edges:
  - source: { node: aBcD1234, port: plan }
    target: { node: feNODE01, port: in }
  - source: { node: feNODE01, port: body }
    target: { node: wRkR5678, port: in }
  - source: { node: feNODE01, port: done }
    target: { node: end, port: result }
"#;
        let parsed = migrate_str_and_parse(yaml);
        assert!(
            !parsed
                .nodes
                .iter()
                .any(|n| n.node_type == NodeType::ForEach),
            "ForEach node must be dissolved"
        );
        assert_eq!(parsed.loops.len(), 1);
        assert_eq!(parsed.loops[0].kind, pipeline::LoopKind::Collection);
        assert_eq!(parsed.loops[0].over.as_deref(), Some("items"));
    }

    #[test]
    fn migrates_foreach_node_to_collection_region() {
        // ADR-0011 / #151: a ForEach node dissolves into a `loops:` collection
        // entry (kind: collection + over) + rewired body edges. The lister ->
        // FE(over: issues) -> worker, FE.done -> end shape must produce a
        // single-member collection region whose member is the body node, with the
        // ForEach node and its ports gone.
        let yaml = r#"
name: foreach-migrate
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
        side: right
  - id: aBcD1234
    name: lister
    type: doc-only
    outputs:
      - name: plan
        side: right
  - id: feNODE01
    name: per-issue
    type: for-each
    over: issues
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
  - id: wRkR5678
    name: worker
    type: code-mutating
    outputs:
      - name: out
        side: right
  - id: end
    name: End
    type: end
    inputs:
      - name: result
        side: left
edges:
  - source: { node: start, port: user_prompt }
    target: { node: aBcD1234, port: task }
  - source: { node: aBcD1234, port: plan }
    target: { node: feNODE01, port: in }
  - source: { node: feNODE01, port: body }
    target: { node: wRkR5678, port: in }
  - source: { node: feNODE01, port: done }
    target: { node: end, port: result }
"#;
        let parsed = migrate_str_and_parse(yaml);

        // No ForEach node may remain, nor any edge referencing it.
        assert!(
            !parsed
                .nodes
                .iter()
                .any(|n| n.node_type == NodeType::ForEach),
            "no ForEach node may remain after migration"
        );
        assert!(
            !parsed
                .edges
                .iter()
                .any(|e| e.source.node == "feNODE01" || e.target.node == "feNODE01"),
            "no edge may reference the dissolved ForEach node"
        );

        // Exactly one collection region, single member = the body node `worker`,
        // over = issues, no max_iter.
        assert_eq!(parsed.loops.len(), 1, "one collection region expected");
        let region = &parsed.loops[0];
        assert_eq!(region.kind, pipeline::LoopKind::Collection);
        assert_eq!(region.over.as_deref(), Some("issues"));
        assert_eq!(region.members, vec!["wRkR5678".to_string()]);
        assert_eq!(region.max_iter, None);

        // The entering edge enters the member directly: lister:plan -> worker.
        assert!(
            parsed
                .edges
                .iter()
                .any(|e| { e.source.node == "aBcD1234" && e.target.node == "wRkR5678" }),
            "entering edge should land on the member directly"
        );
        // The barrier edge leaves the member to the done target: worker -> end.
        assert!(
            parsed
                .edges
                .iter()
                .any(|e| { e.source.node == "wRkR5678" && e.target.node == "end" }),
            "barrier edge should leave the member to the done target"
        );

        // Idempotent: migrating the result again is a no-op.
        let first = migrate_pipeline_yaml(yaml, Path::new("/tmp/fixture.yaml")).unwrap();
        let second =
            migrate_pipeline_yaml(&first.yaml_text, Path::new("/tmp/fixture.yaml")).unwrap();
        assert!(!second.migrated, "foreach migration must be idempotent");
    }

    #[test]
    fn migrate_str_and_parse_rejects_foreach() {
        // Helper guard: after migration there must be no ForEach node left, which
        // `migrate_str_and_parse` already enforces via parse. This documents that
        // the ForEach node type is retired (ADR-0011 / #151).
        let yaml = r#"
name: foreach-empty-over
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
        side: right
  - id: aBcD1234
    name: lister
    type: doc-only
    outputs:
      - name: plan
        side: right
  - id: feNODE01
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
  - id: wRkR5678
    name: worker
    type: code-mutating
    outputs:
      - name: out
        side: right
  - id: end
    name: End
    type: end
    inputs:
      - name: result
        side: left
edges:
  - source: { node: aBcD1234, port: plan }
    target: { node: feNODE01, port: in }
  - source: { node: feNODE01, port: body }
    target: { node: wRkR5678, port: in }
  - source: { node: feNODE01, port: done }
    target: { node: end, port: result }
"#;
        let parsed = migrate_str_and_parse(yaml);
        let region = &parsed.loops[0];
        assert_eq!(region.over.as_deref(), Some("tasks"));
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
    fn migrates_loop_node_to_bounded_region() {
        // ADR-0011 / #148: a Loop node dissolves into a `loops:` entry + rewired
        // body edges. The review-loop fixture (start -> loop -> body -> ...) must
        // produce a bounded region whose members are the body nodes, with the
        // Loop node and its ports gone.
        let yaml = include_str!("../../../.pdo/pipelines/review-loop.yaml");
        let parsed = migrate_str_and_parse(yaml);

        // No Loop node may remain, nor any edge referencing it.
        assert!(
            !parsed.nodes.iter().any(|n| n.node_type == NodeType::Loop),
            "no Loop node may remain after migration"
        );

        // Exactly one bounded region, members = the two body nodes, max_iter 3.
        assert_eq!(parsed.loops.len(), 1, "one bounded region expected");
        let region = &parsed.loops[0];
        assert_eq!(region.kind, pipeline::LoopKind::Bounded);
        let mut members = region.members.clone();
        members.sort();
        assert_eq!(
            members,
            vec!["Qws9KzRZ".to_string(), "XBG5Cxkn".to_string()]
        );
        assert_eq!(
            region.max_iter,
            Some(serde_yaml::Value::Number(serde_yaml::Number::from(3)))
        );

        // No edge may reference the dissolved loop's ports.
        assert!(
            !parsed
                .edges
                .iter()
                .any(|e| e.source.node == "qdtXejYS" || e.target.node == "qdtXejYS"),
            "no edge may reference the dissolved Loop node"
        );

        // The region matches what cycle auto-detection would find on the rewired
        // graph (the back-edge makes the loop a real cycle).
        let cycles = crate::graph_resolver::detect_cycles(&parsed);
        assert_eq!(cycles.len(), 1, "rewired graph has exactly one cycle");
        let mut cyc = cycles[0].clone();
        cyc.sort();
        assert_eq!(cyc, members);

        // Idempotent: migrating the result again is a no-op.
        let first = migrate_pipeline_yaml(yaml, Path::new("/tmp/fixture.yaml")).unwrap();
        let second =
            migrate_pipeline_yaml(&first.yaml_text, Path::new("/tmp/fixture.yaml")).unwrap();
        assert!(!second.migrated, "loop migration must be idempotent");
    }

    #[test]
    fn migrates_review_loop_fixture_switch_to_guarded_edges() {
        let yaml = include_str!("../../../.pdo/pipelines/review-loop.yaml");
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
        let yaml = include_str!("../../../.pdo/pipelines/simple-bugfix.yaml");
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
        let yaml = include_str!("../../../.pdo/pipelines/planner.yaml");
        let result = migrate_pipeline_yaml(yaml, Path::new("/tmp/fixture.yaml")).unwrap();
        let parsed = pipeline::parse_pipeline(&result.yaml_text)
            .unwrap()
            .pipeline;
        assert!(
            !parsed.edges.iter().any(|e| e.when.is_some() || e.is_else),
            "planner has no switch, so no conditional edges should appear"
        );
    }

    // --- #231: stranded flat-prompt boot migration ---

    /// Writes a minimal pipeline YAML declaring the given node ids.
    fn write_pipeline_with_nodes(dir: &Path, stem: &str, node_ids: &[&str]) {
        let mut yaml = String::from("name: ");
        yaml.push_str(stem);
        yaml.push_str("\nversion: \"1.0\"\nnodes:\n");
        for id in node_ids {
            yaml.push_str(&format!(
                "  - id: {id}\n    name: {id}\n    type: doc-only\n    outputs:\n      - name: out\n"
            ));
        }
        yaml.push_str("edges: []\n");
        std::fs::write(dir.join(format!("{stem}.yaml")), yaml).unwrap();
    }

    #[test]
    fn migrate_flat_prompts_moves_to_canonical_and_removes_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        write_pipeline_with_nodes(dir, "auto-issue", &["AAAAAAAA", "BBBBBBBB"]);

        let flat = dir.join("prompts");
        std::fs::create_dir_all(&flat).unwrap();
        std::fs::write(flat.join("AAAAAAAA.md"), "prompt A").unwrap();
        std::fs::write(flat.join("BBBBBBBB.md"), "prompt B").unwrap();

        let moved = migrate_stranded_flat_prompts(dir).unwrap();
        assert_eq!(moved, 2);

        // Both prompts now live in the canonical `<stem>.prompts/` dir, intact.
        let canon = dir.join("auto-issue.prompts");
        assert_eq!(
            std::fs::read_to_string(canon.join("AAAAAAAA.md")).unwrap(),
            "prompt A"
        );
        assert_eq!(
            std::fs::read_to_string(canon.join("BBBBBBBB.md")).unwrap(),
            "prompt B"
        );
        // The emptied flat dir is removed.
        assert!(
            !flat.exists(),
            "flat prompts dir should be removed once emptied"
        );
    }

    #[test]
    fn migrate_flat_prompts_does_not_clobber_existing_canonical() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        write_pipeline_with_nodes(dir, "bugfix", &["CCCCCCCC"]);

        // Canonical already holds authoritative text.
        let canon = dir.join("bugfix.prompts");
        std::fs::create_dir_all(&canon).unwrap();
        std::fs::write(canon.join("CCCCCCCC.md"), "canonical text").unwrap();

        // A stale flat duplicate exists.
        let flat = dir.join("prompts");
        std::fs::create_dir_all(&flat).unwrap();
        std::fs::write(flat.join("CCCCCCCC.md"), "stale flat text").unwrap();

        let moved = migrate_stranded_flat_prompts(dir).unwrap();
        assert_eq!(moved, 0, "must not move when canonical exists");

        // Canonical is untouched; the flat duplicate is preserved (not destroyed),
        // so the conflict stays visible and the dir is kept.
        assert_eq!(
            std::fs::read_to_string(canon.join("CCCCCCCC.md")).unwrap(),
            "canonical text"
        );
        assert!(flat.join("CCCCCCCC.md").exists());
        assert!(flat.exists(), "non-empty flat dir must be preserved");
    }

    #[test]
    fn migrate_flat_prompts_preserves_orphan_for_unknown_node() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        write_pipeline_with_nodes(dir, "alpha", &["KNOWN001"]);

        let flat = dir.join("prompts");
        std::fs::create_dir_all(&flat).unwrap();
        std::fs::write(flat.join("KNOWN001.md"), "known").unwrap();
        // No pipeline declares this id (e.g. a deleted pipeline).
        std::fs::write(flat.join("ORPHAN99.md"), "orphan").unwrap();

        let moved = migrate_stranded_flat_prompts(dir).unwrap();
        assert_eq!(moved, 1);

        assert_eq!(
            std::fs::read_to_string(dir.join("alpha.prompts/KNOWN001.md")).unwrap(),
            "known"
        );
        // The unmatched orphan is preserved, and so is the dir holding it.
        assert!(
            flat.join("ORPHAN99.md").exists(),
            "orphan prompt must not be destroyed"
        );
        assert!(flat.exists());
    }

    #[test]
    fn migrate_flat_prompts_noop_without_flat_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        write_pipeline_with_nodes(dir, "alpha", &["AAAAAAAA"]);
        // No flat `prompts/` dir at all.
        let moved = migrate_stranded_flat_prompts(dir).unwrap();
        assert_eq!(moved, 0);
    }

    #[test]
    fn migrate_flat_prompts_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        write_pipeline_with_nodes(dir, "auto-issue", &["AAAAAAAA"]);

        let flat = dir.join("prompts");
        std::fs::create_dir_all(&flat).unwrap();
        std::fs::write(flat.join("AAAAAAAA.md"), "prompt A").unwrap();

        assert_eq!(migrate_stranded_flat_prompts(dir).unwrap(), 1);
        // Second run: flat dir is gone → no-op.
        assert_eq!(migrate_stranded_flat_prompts(dir).unwrap(), 0);
        assert_eq!(
            std::fs::read_to_string(dir.join("auto-issue.prompts/AAAAAAAA.md")).unwrap(),
            "prompt A"
        );
    }

    #[test]
    fn migrate_flat_prompts_routes_each_id_to_its_owning_pipeline() {
        // Two pipelines, one flat prompt each — each must land in its own
        // canonical dir (ids are globally unique).
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        write_pipeline_with_nodes(dir, "alpha", &["AAAAAAAA"]);
        write_pipeline_with_nodes(dir, "beta", &["BBBBBBBB"]);

        let flat = dir.join("prompts");
        std::fs::create_dir_all(&flat).unwrap();
        std::fs::write(flat.join("AAAAAAAA.md"), "a").unwrap();
        std::fs::write(flat.join("BBBBBBBB.md"), "b").unwrap();

        let moved = migrate_stranded_flat_prompts(dir).unwrap();
        assert_eq!(moved, 2);
        assert_eq!(
            std::fs::read_to_string(dir.join("alpha.prompts/AAAAAAAA.md")).unwrap(),
            "a"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("beta.prompts/BBBBBBBB.md")).unwrap(),
            "b"
        );
        assert!(!flat.exists());
    }
}
