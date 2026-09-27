//! Switch nodes become guarded edges (ADR-0011).

use std::collections::{HashMap, HashSet};

/// Dissolves every `switch` node into guarded edges (ADR-0011). For a switch
/// `SW` fed by `(P, p_port)`, each switch output port becomes a direct edge from
/// `(P, p_port)` to wherever that switch port wired to, carrying the port's
/// `when:` clause. A switch's `default` port (no `when:`) becomes `else: true`
/// edges. The switch node and all edges touching it are removed.
pub(crate) fn dissolve_switches(doc: &mut serde_yaml::Value) -> Result<(), String> {
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

    // The shared reader, which also understands a multi-port `ports:` source
    // (#843) — an inline copy here would quietly stop seeing those edges.
    let endpoint = edge_endpoint;

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
///
/// A source may carry several ports (`ports: [a, b]`, ADR-0073 / #843); the
/// migrations here — ForEach, Loop and Switch dissolution — rewire by *node* and
/// only ever need one port name, so they read the FIRST carried port. Reading
/// none would make a migration silently skip a multi-port edge, which is how a
/// legacy pipeline would come out of the migrator half-rewired.
pub(crate) fn edge_endpoint(edge: &serde_yaml::Value, key: &str) -> Option<(String, String)> {
    let ep = edge.get(key)?.as_mapping()?;
    let node = ep
        .get(serde_yaml::Value::String("node".into()))?
        .as_str()?
        .to_string();
    let port = match ep.get(serde_yaml::Value::String("port".into())) {
        Some(v) => v.as_str()?.to_string(),
        None => ep
            .get(serde_yaml::Value::String("ports".into()))?
            .as_sequence()?
            .first()?
            .as_str()?
            .to_string(),
    };
    Some((node, port))
}
