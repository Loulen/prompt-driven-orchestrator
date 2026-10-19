//! The cycles of a pipeline's edge graph.

use std::collections::{HashMap, HashSet};

use super::pipeline::PipelineDef;

/// Detects the cycles in a pipeline's edge graph (ADR-0011 / #148).
///
/// A *cycle* is either a strongly-connected component of two or more nodes, or a
/// single node carrying a self-edge (a node looping on itself is a valid bounded
/// region of one member). Each returned cycle is the set of its member node ids,
/// ordered by their position in `pipeline.nodes` for determinism. The list of
/// cycles is itself ordered by the position of each cycle's first member.
///
/// This is the topological signature a bounded loop region is auto-materialized
/// from, so that no cycle is ever accidentally unbounded.
pub fn detect_cycles(pipeline: &PipelineDef) -> Vec<Vec<String>> {
    let order: HashMap<&str, usize> = pipeline
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id.as_str(), i))
        .collect();

    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut has_self_edge: HashSet<&str> = HashSet::new();
    for edge in &pipeline.edges {
        let (s, t) = (edge.source.node.as_str(), edge.target.node.as_str());
        if !order.contains_key(s) || !order.contains_key(t) {
            continue;
        }
        if s == t {
            has_self_edge.insert(s);
        }
        adj.entry(s).or_default().push(t);
    }

    // Tarjan SCC. Recursion is safe only because pipelines are small graphs.
    struct Tarjan<'a> {
        adj: &'a HashMap<&'a str, Vec<&'a str>>,
        index: HashMap<&'a str, usize>,
        lowlink: HashMap<&'a str, usize>,
        on_stack: HashSet<&'a str>,
        stack: Vec<&'a str>,
        next_index: usize,
        sccs: Vec<Vec<&'a str>>,
    }

    impl<'a> Tarjan<'a> {
        fn strongconnect(&mut self, v: &'a str) {
            self.index.insert(v, self.next_index);
            self.lowlink.insert(v, self.next_index);
            self.next_index += 1;
            self.stack.push(v);
            self.on_stack.insert(v);

            if let Some(targets) = self.adj.get(v) {
                for &w in targets {
                    if !self.index.contains_key(w) {
                        self.strongconnect(w);
                        let low_w = self.lowlink[w];
                        let low_v = self.lowlink[v];
                        self.lowlink.insert(v, low_v.min(low_w));
                    } else if self.on_stack.contains(w) {
                        let idx_w = self.index[w];
                        let low_v = self.lowlink[v];
                        self.lowlink.insert(v, low_v.min(idx_w));
                    }
                }
            }

            if self.lowlink[v] == self.index[v] {
                let mut component = Vec::new();
                while let Some(w) = self.stack.pop() {
                    self.on_stack.remove(w);
                    component.push(w);
                    if w == v {
                        break;
                    }
                }
                self.sccs.push(component);
            }
        }
    }

    let mut tarjan = Tarjan {
        adj: &adj,
        index: HashMap::new(),
        lowlink: HashMap::new(),
        on_stack: HashSet::new(),
        stack: Vec::new(),
        next_index: 0,
        sccs: Vec::new(),
    };
    for node in &pipeline.nodes {
        let id = node.id.as_str();
        if !tarjan.index.contains_key(id) {
            tarjan.strongconnect(id);
        }
    }

    let mut cycles: Vec<Vec<String>> = tarjan
        .sccs
        .into_iter()
        .filter(|comp| comp.len() >= 2 || comp.first().is_some_and(|n| has_self_edge.contains(n)))
        .map(|mut comp| {
            comp.sort_by_key(|id| order.get(id).copied().unwrap_or(usize::MAX));
            comp.into_iter().map(String::from).collect()
        })
        .collect();

    cycles.sort_by_key(|members| {
        members
            .first()
            .and_then(|id| order.get(id.as_str()).copied())
            .unwrap_or(usize::MAX)
    });

    cycles
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::pipeline::{
        EdgeDef, EdgeEndpoint, EdgeSource, NodeDef, NodeType, Port, PortType,
    };
    use pretty_assertions::assert_eq;
    use std::collections::HashMap;

    fn make_node(id: &str, node_type: NodeType, inputs: &[&str], outputs: &[&str]) -> NodeDef {
        NodeDef {
            skills: Vec::new(),
            isolated_worktree: None,
            id: id.into(),
            name: id.into(),
            node_type,
            inputs: inputs
                .iter()
                .map(|n| Port {
                    name: (*n).into(),
                    port_type: PortType::Markdown,
                    frontmatter: None,
                    when: None,
                    description: None,
                    instructions: None,
                    required: false,
                    intermediate: false,
                })
                .collect(),
            outputs: outputs
                .iter()
                .map(|n| Port {
                    name: (*n).into(),
                    port_type: PortType::Markdown,
                    frontmatter: None,
                    when: None,
                    description: None,
                    instructions: None,
                    required: false,
                    intermediate: false,
                })
                .collect(),
            interactive: false,
            view: None,
            max_iter: None,
            over: None,
            pin_harness: None,
            harnesses: Default::default(),
            agent_choice: None,
            auto_fail: None,
            orchestrator: false,
            on_demand: false,
        }
    }

    fn make_edge(src_node: &str, src_port: &str, tgt_node: &str, tgt_port: &str) -> EdgeDef {
        EdgeDef {
            source: EdgeSource::single(src_node, src_port),
            target: EdgeEndpoint {
                node: tgt_node.into(),
                port: tgt_port.into(),
            },
            reason: None,
            when: None,
            is_else: false,
            repeated: false,
            ..Default::default()
        }
    }

    fn make_pipeline(nodes: Vec<NodeDef>, edges: Vec<EdgeDef>) -> PipelineDef {
        PipelineDef {
            name: "test".into(),
            version: None,
            variables: HashMap::new(),
            nodes,
            edges,
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: true,
            grid_size: None,
            wait_for_started_nodes: false,
        }
    }

    #[test]
    fn detect_cycles_finds_a_two_node_cycle() {
        // ADR-0011 / #148: a bounded loop is born by auto-detection of a cycle.
        // implementer -> reviewer -> implementer is one cycle of two members.
        let pipeline = make_pipeline(
            vec![
                make_node("impl", NodeType::Agent, &["review"], &["code"]),
                make_node("rev", NodeType::Agent, &["code"], &["review"]),
            ],
            vec![
                make_edge("impl", "code", "rev", "code"),
                make_edge("rev", "review", "impl", "review"),
            ],
        );

        let cycles = detect_cycles(&pipeline);
        assert_eq!(cycles.len(), 1);
        assert_eq!(cycles[0], vec!["impl".to_string(), "rev".to_string()]);
    }

    #[test]
    fn detect_cycles_finds_a_self_edge() {
        // A self-looping member is a valid bounded region of ONE member
        // (ADR-0011 / #148: self-edge included).
        let pipeline = make_pipeline(
            vec![
                make_node("a", NodeType::Agent, &["in"], &["out"]),
                make_node("worker", NodeType::Agent, &["seed", "again"], &["out"]),
                make_node("b", NodeType::Agent, &["in"], &["out"]),
            ],
            vec![
                make_edge("a", "out", "worker", "seed"),
                make_edge("worker", "out", "worker", "again"),
                make_edge("worker", "out", "b", "in"),
            ],
        );

        let cycles = detect_cycles(&pipeline);
        assert_eq!(cycles.len(), 1);
        assert_eq!(cycles[0], vec!["worker".to_string()]);
    }

    #[test]
    fn detect_cycles_ignores_acyclic_graph() {
        let pipeline = make_pipeline(
            vec![
                make_node("a", NodeType::Agent, &["in"], &["out"]),
                make_node("b", NodeType::Agent, &["in"], &["out"]),
                make_node("c", NodeType::Agent, &["in"], &["out"]),
            ],
            vec![
                make_edge("a", "out", "b", "in"),
                make_edge("b", "out", "c", "in"),
            ],
        );
        assert!(detect_cycles(&pipeline).is_empty());
    }
}
