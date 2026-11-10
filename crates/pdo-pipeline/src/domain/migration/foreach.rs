//! ForEach nodes become collection regions (ADR-0011 / #151); declared inputs
//! of regular nodes become emergent (#149).

use std::collections::{HashMap, HashSet};

use super::loops::body_members;
use super::switches::edge_endpoint;

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
///   finish, so the downstream node converges on every lap (ADR-0082).
/// - the ForEach node is removed and a `{ id, kind: collection, over, members }`
///   entry is appended to the pipeline's `loops:` block (no `max_iter`).
pub(crate) fn dissolve_foreaches(doc: &mut serde_yaml::Value) -> Result<(), String> {
    let nodes = match doc.get("nodes").and_then(|n| n.as_sequence()) {
        Some(seq) => seq.clone(),
        None => return Ok(()),
    };

    // `fe_order` keeps document order so the emitted regions are stable across
    // runs — a HashSet walk is not deterministic.
    let mut fe_ids: HashSet<String> = HashSet::new();
    let mut fe_order: Vec<String> = Vec::new();
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
        fe_order.push(id.clone());
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

    // Region ids default to the ForEach node's *name*, user-chosen free text:
    // guard against collisions with surviving node ids, existing region ids, and
    // sibling ForEach names — two ForEaches named "per-item" would otherwise
    // corrupt each other's loop counters. The removed ForEach ids are free to reuse.
    let mut taken_ids: HashSet<String> = nodes
        .iter()
        .filter_map(|n| n.get("id").and_then(|v| v.as_str()).map(String::from))
        .filter(|id| !fe_ids.contains(id))
        .collect();
    if let Some(serde_yaml::Value::Sequence(existing)) = doc.get("loops") {
        for r in existing {
            if let Some(id) = r.get("id").and_then(|v| v.as_str()) {
                taken_ids.insert(id.to_string());
            }
        }
    }

    let mut regions: Vec<serde_yaml::Value> = Vec::new();
    for fe_id in &fe_order {
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
        let mut region_id = name.clone();
        let mut suffix = 2;
        while taken_ids.contains(&region_id) {
            region_id = format!("{name}-{suffix}");
            suffix += 1;
        }
        taken_ids.insert(region_id.clone());
        let mut region = serde_yaml::Mapping::new();
        region.insert(
            serde_yaml::Value::String("id".into()),
            serde_yaml::Value::String(region_id),
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
pub(crate) fn body_terminal(
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

/// #149: inputs are emergent. Strip declared `inputs` from regular (non-isolated /
/// isolated) nodes. Structural nodes (start/end/switch/loop/for-each) keep
/// their required ports. Any `repeated: true` declared input is migrated onto
/// the matching incoming edge so loop accumulation is preserved.
pub(crate) fn drop_declared_inputs(doc: &mut serde_yaml::Value) {
    // Pass 1: collect (node_id, input_name) pairs that carried `repeated: true`,
    // and the set of regular node ids whose inputs we will drop.
    let mut repeated_targets: Vec<(String, String)> = Vec::new();
    let mut regular_node_ids: Vec<String> = Vec::new();

    if let Some(nodes) = doc.get("nodes").and_then(|n| n.as_sequence()) {
        for node in nodes {
            let node_type = node.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if matches!(node_type, "start" | "end" | "switch" | "loop" | "for-each") {
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
