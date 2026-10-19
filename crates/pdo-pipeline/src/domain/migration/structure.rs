//! The structural passes: `halt` edges, the Start and End nodes,
//! and the rewiring of node references after an id change.

use std::collections::HashMap;

use super::detect::{has_start_end_nodes, nodes_contain_type};

pub(crate) fn rewrite_halt_edge(edge: &mut serde_yaml::Value) {
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

pub(crate) fn inject_start_end_nodes(doc: &mut serde_yaml::Value) {
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

/// Remaps node references inside the `loops:` block (`members` entries) through
/// `id_map`, mirroring [`rewrite_edge_endpoint`] for edges. Region `id`s are
/// free text, not node references, and are left alone.
pub(crate) fn rewrite_loop_members(doc: &mut serde_yaml::Value, id_map: &HashMap<String, String>) {
    let Some(regions) = doc.get_mut("loops").and_then(|l| l.as_sequence_mut()) else {
        return;
    };
    for region in regions.iter_mut() {
        let Some(members) = region.get_mut("members").and_then(|m| m.as_sequence_mut()) else {
            continue;
        };
        for member in members.iter_mut() {
            if let Some(new_id) = member.as_str().and_then(|old| id_map.get(old)) {
                *member = serde_yaml::Value::String(new_id.clone());
            }
        }
    }
}

pub(crate) fn rewrite_edge_endpoint(
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
