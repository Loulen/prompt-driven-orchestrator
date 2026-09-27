//! Loop nodes become bounded regions (ADR-0011 / #148).

use std::collections::{HashMap, HashSet};

use super::switches::edge_endpoint;

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
pub(crate) fn dissolve_loops(doc: &mut serde_yaml::Value) -> Result<(), String> {
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

    let mut loop_entry: HashMap<String, (String, String)> = HashMap::new();
    // Keep the whole `U.p -> L.in` edge, not just its endpoint: its routing clauses
    // must survive the rewire (`U.p -> L.in [when W]` becomes `U.p -> E.q [when W]`).
    // Dropping `W` turned a guarded loop entry into an unconditional one, so every
    // verdict entered the fix loop and the `else` route to End became dead code.
    let mut loop_in_sources: HashMap<String, Vec<(String, String, serde_yaml::Value)>> =
        HashMap::new();
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
                        if let Some((snode, sport)) = &src {
                            loop_in_sources.entry(tnode.clone()).or_default().push((
                                snode.clone(),
                                sport.clone(),
                                edge.clone(),
                            ));
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

        // U.p -> L.in [when W / else / reason]  +  L.body -> E.q
        //   ==>   U.p -> E.q [when W / else / reason]
        //
        // The routing clauses ride along: the guard that decided whether the loop
        // was entered at all must now decide whether its entry node is spawned.
        // Layout (`mode`/`waypoints`/`target_side`, and the #845 label positions
        // and draw order) is deliberately NOT copied — it described a route, and
        // labels along it, for a node that no longer exists.
        if let Some(sources) = loop_in_sources.get(loop_id) {
            for (unode, uport, in_edge) in sources {
                let mut m = serde_yaml::Mapping::new();
                m.insert(
                    serde_yaml::Value::String("source".into()),
                    mk_endpoint(unode, uport),
                );
                m.insert(
                    serde_yaml::Value::String("target".into()),
                    mk_endpoint(&entry.0, &entry.1),
                );
                for key in ["when", "else", "reason"] {
                    if let Some(v) = in_edge.get(key) {
                        m.insert(serde_yaml::Value::String(key.into()), v.clone());
                    }
                }
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

    // Derive members from the ORIGINAL edges, not the rewired graph: BFS from each
    // loop's entry, stopping at the loop node and at other loops.
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
pub(crate) fn body_members(
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
