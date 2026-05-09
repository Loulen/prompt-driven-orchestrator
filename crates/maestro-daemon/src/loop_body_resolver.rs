use std::collections::HashSet;

use crate::pipeline::{NodeType, PipelineDef};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum BodyResolutionError {
    #[error("loop node '{0}' not found in pipeline")]
    LoopNotFound(String),
    #[error("loop '{0}' has an empty body (body port wired to nothing)")]
    EmptyBody(String),
    #[error("loop '{0}' body has no exit path back to break or done")]
    NoExitToBreakOrDone(String),
}

pub fn compute_body_subgraph(
    pipeline: &PipelineDef,
    loop_node_id: &str,
) -> Result<HashSet<String>, BodyResolutionError> {
    pipeline
        .nodes
        .iter()
        .find(|n| {
            n.id == loop_node_id
                && (n.node_type == NodeType::Loop || n.node_type == NodeType::ForEach)
        })
        .ok_or_else(|| BodyResolutionError::LoopNotFound(loop_node_id.to_string()))?;

    let body_targets: Vec<&str> = pipeline
        .edges
        .iter()
        .filter(|e| e.source.node == loop_node_id && e.source.port == "body")
        .map(|e| e.target.node.as_str())
        .collect();

    if body_targets.is_empty() {
        return Err(BodyResolutionError::EmptyBody(loop_node_id.to_string()));
    }

    let mut body = HashSet::new();
    let mut queue: Vec<&str> = body_targets.clone();

    while let Some(current) = queue.pop() {
        if current == loop_node_id {
            continue;
        }

        let current_node = pipeline.nodes.iter().find(|n| n.id == current);
        if let Some(cn) = current_node {
            if (cn.node_type == NodeType::Loop || cn.node_type == NodeType::ForEach)
                && cn.id != loop_node_id
            {
                body.insert(current.to_string());
                continue;
            }
        }

        if !body.insert(current.to_string()) {
            continue;
        }

        for edge in &pipeline.edges {
            if edge.source.node != current {
                continue;
            }
            let target = edge.target.node.as_str();
            if target == loop_node_id {
                continue;
            }
            if !body.contains(target) {
                queue.push(target);
            }
        }
    }

    let has_exit = pipeline.edges.iter().any(|e| {
        body.contains(&e.source.node)
            && e.target.node == loop_node_id
            && (e.target.port == "break" || e.target.port == "done")
    });

    if !has_exit {
        return Err(BodyResolutionError::NoExitToBreakOrDone(
            loop_node_id.to_string(),
        ));
    }

    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::{EdgeDef, EdgeEndpoint, NodeDef, NodeType, Port};
    use pretty_assertions::assert_eq;
    use std::collections::HashMap;

    fn make_node(id: &str, node_type: NodeType, inputs: &[&str], outputs: &[&str]) -> NodeDef {
        NodeDef {
            id: id.into(),
            name: id.into(),
            node_type,
            inputs: inputs
                .iter()
                .map(|n| Port {
                    name: (*n).into(),
                    repeated: false,
                    side: None,
                    frontmatter: None,
                    when: None,
                })
                .collect(),
            outputs: outputs
                .iter()
                .map(|n| Port {
                    name: (*n).into(),
                    repeated: false,
                    side: None,
                    frontmatter: None,
                    when: None,
                })
                .collect(),
            interactive: false,
            view: None,
            max_iter: None,
        }
    }

    fn make_loop_node(id: &str, max_iter: i64) -> NodeDef {
        NodeDef {
            id: id.into(),
            name: id.into(),
            node_type: NodeType::Loop,
            inputs: vec![
                Port {
                    name: "in".into(),
                    repeated: false,
                    side: None,
                    frontmatter: None,
                    when: None,
                },
                Port {
                    name: "break".into(),
                    repeated: false,
                    side: None,
                    frontmatter: None,
                    when: None,
                },
            ],
            outputs: vec![
                Port {
                    name: "body".into(),
                    repeated: false,
                    side: None,
                    frontmatter: None,
                    when: None,
                },
                Port {
                    name: "done".into(),
                    repeated: false,
                    side: None,
                    frontmatter: None,
                    when: None,
                },
            ],
            interactive: false,
            view: None,
            max_iter: Some(serde_yaml::Value::Number(serde_yaml::Number::from(
                max_iter,
            ))),
        }
    }

    fn make_edge(src_node: &str, src_port: &str, tgt_node: &str, tgt_port: &str) -> EdgeDef {
        EdgeDef {
            source: EdgeEndpoint {
                node: src_node.into(),
                port: src_port.into(),
            },
            target: EdgeEndpoint {
                node: tgt_node.into(),
                port: tgt_port.into(),
            },
            reason: None,
        }
    }

    fn make_pipeline(nodes: Vec<NodeDef>, edges: Vec<EdgeDef>) -> PipelineDef {
        PipelineDef {
            name: "test".into(),
            version: None,
            variables: HashMap::new(),
            nodes,
            edges,
            auto_merge_resolver: true,
        }
    }

    #[test]
    fn linear_body_returns_all_nodes() {
        // Loop.body → A → B → Switch → Loop.break
        let pipeline = make_pipeline(
            vec![
                make_loop_node("loop1", 5),
                make_node("a", NodeType::DocOnly, &["in"], &["out"]),
                make_node("b", NodeType::DocOnly, &["in"], &["out"]),
                make_node("sw", NodeType::Switch, &["in"], &["pass", "default"]),
            ],
            vec![
                make_edge("loop1", "body", "a", "in"),
                make_edge("a", "out", "b", "in"),
                make_edge("b", "out", "sw", "in"),
                make_edge("sw", "pass", "loop1", "break"),
            ],
        );

        let body = compute_body_subgraph(&pipeline, "loop1").unwrap();
        let expected: HashSet<String> = ["a", "b", "sw"].iter().map(|s| s.to_string()).collect();
        assert_eq!(body, expected);
    }

    #[test]
    fn body_with_internal_switch_all_branches_stay() {
        // Loop.body → impl → reviewer → Switch
        // Switch.pass → Loop.break
        // Switch.default → impl (back-loop within body)
        let pipeline = make_pipeline(
            vec![
                make_loop_node("loop1", 5),
                make_node("impl", NodeType::CodeMutating, &["in"], &["out"]),
                make_node("reviewer", NodeType::DocOnly, &["in"], &["review"]),
                make_node("sw", NodeType::Switch, &["in"], &["pass", "default"]),
            ],
            vec![
                make_edge("loop1", "body", "impl", "in"),
                make_edge("impl", "out", "reviewer", "in"),
                make_edge("reviewer", "review", "sw", "in"),
                make_edge("sw", "pass", "loop1", "break"),
                make_edge("sw", "default", "impl", "in"),
            ],
        );

        let body = compute_body_subgraph(&pipeline, "loop1").unwrap();
        let expected: HashSet<String> = ["impl", "reviewer", "sw"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(body, expected);
    }

    #[test]
    fn nested_loops_outer_excludes_inner_body() {
        // outer_loop.body → inner_loop (Loop node)
        // inner_loop.body → inner_worker
        // inner_worker → inner_loop.break
        // inner_loop.done → outer_loop.break
        let pipeline = make_pipeline(
            vec![
                make_loop_node("outer", 3),
                make_loop_node("inner", 5),
                make_node("inner_worker", NodeType::DocOnly, &["in"], &["out"]),
            ],
            vec![
                make_edge("outer", "body", "inner", "in"),
                make_edge("inner", "body", "inner_worker", "in"),
                make_edge("inner_worker", "out", "inner", "break"),
                make_edge("inner", "done", "outer", "break"),
            ],
        );

        let body = compute_body_subgraph(&pipeline, "outer").unwrap();
        // inner Loop is treated as opaque — included but its transitive nodes are NOT
        let expected: HashSet<String> = ["inner"].iter().map(|s| s.to_string()).collect();
        assert_eq!(body, expected);
    }

    #[test]
    fn empty_body_returns_error() {
        // Loop with body port not wired
        let pipeline = make_pipeline(vec![make_loop_node("loop1", 5)], vec![]);

        let result = compute_body_subgraph(&pipeline, "loop1");
        assert_eq!(result, Err(BodyResolutionError::EmptyBody("loop1".into())));
    }

    #[test]
    fn no_exit_returns_error() {
        // Loop.body → A → B but B doesn't go back to break or done
        let pipeline = make_pipeline(
            vec![
                make_loop_node("loop1", 5),
                make_node("a", NodeType::DocOnly, &["in"], &["out"]),
                make_node("b", NodeType::DocOnly, &["in"], &["out"]),
            ],
            vec![
                make_edge("loop1", "body", "a", "in"),
                make_edge("a", "out", "b", "in"),
            ],
        );

        let result = compute_body_subgraph(&pipeline, "loop1");
        assert_eq!(
            result,
            Err(BodyResolutionError::NoExitToBreakOrDone("loop1".into()))
        );
    }

    #[test]
    fn loop_not_found_returns_error() {
        let pipeline = make_pipeline(vec![], vec![]);
        let result = compute_body_subgraph(&pipeline, "nonexistent");
        assert_eq!(
            result,
            Err(BodyResolutionError::LoopNotFound("nonexistent".into()))
        );
    }

    #[test]
    fn non_loop_node_returns_error() {
        let pipeline = make_pipeline(
            vec![make_node("a", NodeType::DocOnly, &["in"], &["out"])],
            vec![],
        );
        let result = compute_body_subgraph(&pipeline, "a");
        assert_eq!(result, Err(BodyResolutionError::LoopNotFound("a".into())));
    }
}
