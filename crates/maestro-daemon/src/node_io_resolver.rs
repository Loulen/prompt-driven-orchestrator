use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::frontmatter_parser;
use crate::pipeline::{self, PipelineDef, PortType};

#[derive(Debug, Clone, Serialize)]
pub struct FileInfo {
    pub path: String,
    pub exists: bool,
    pub size: Option<u64>,
    pub frontmatter: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PortIO {
    pub port: String,
    pub repeated: bool,
    pub port_type: PortType,
    pub files: Vec<FileInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NodeIO {
    pub inputs: Vec<PortIO>,
    pub outputs: Vec<PortIO>,
}

pub fn resolve(pipeline: &PipelineDef, artifacts_dir: &Path, node_id: &str, iter: i64) -> NodeIO {
    let node = pipeline.nodes.iter().find(|n| n.id == node_id);
    let node = match node {
        Some(n) => n,
        None => {
            return NodeIO {
                inputs: vec![],
                outputs: vec![],
            }
        }
    };

    // Inputs are EMERGENT (#149): derived from incoming edges, not declared on
    // the node. Each edge contributes files to a logical input named after its
    // target endpoint (which inherits the source document name). Several
    // same-named edges POOL into one list input. `repeated` (accumulate
    // `iter-*`) is read off the edge, never off a declared port.
    let mut inputs: Vec<PortIO> = Vec::new();
    let mut input_index: HashMap<String, usize> = HashMap::new();

    for edge in &pipeline.edges {
        if edge.target.node != node_id {
            continue;
        }

        let mut files = Vec::new();
        if edge.repeated {
            let source_dir = artifacts_dir.join(&edge.source.node);
            files.extend(glob_repeated(&source_dir, &edge.source.port));
        } else {
            let path = crate::blackboard::artifact_path(
                artifacts_dir,
                &edge.source.node,
                iter,
                &edge.source.port,
            );
            files.push(file_info(artifacts_dir, &path));
        }

        match input_index.get(&edge.target.port) {
            // Pool same-named edges into one logical list input. Once two edges
            // share a target name the input is a list (repeated), regardless of
            // each edge's own flag.
            Some(&idx) => {
                let pooled: &mut PortIO = &mut inputs[idx];
                pooled.repeated = true;
                pooled.files.extend(files);
            }
            None => {
                input_index.insert(edge.target.port.clone(), inputs.len());
                inputs.push(PortIO {
                    port: edge.target.port.clone(),
                    repeated: edge.repeated,
                    port_type: PortType::Markdown,
                    files,
                });
            }
        }
    }

    // Entry-node fallback: a node with no incoming edges that still expects a
    // `task` reads the run's `_input` (preserves existing single-entry pipelines).
    if inputs.is_empty() && node.inputs.iter().any(|p| p.name == "task") {
        let path = crate::blackboard::input_path(artifacts_dir);
        inputs.push(PortIO {
            port: "task".into(),
            repeated: false,
            port_type: PortType::Markdown,
            files: vec![file_info(artifacts_dir, &path)],
        });
    }

    let mut outputs: Vec<PortIO> = Vec::new();
    for output_port in &node.outputs {
        match output_port.port_type {
            PortType::Image | PortType::ImageList => {
                let port_dir =
                    crate::blackboard::port_dir(artifacts_dir, node_id, iter, &output_port.name);
                let files = list_image_files(artifacts_dir, &port_dir);
                outputs.push(PortIO {
                    port: output_port.name.clone(),
                    repeated: output_port.repeated,
                    port_type: output_port.port_type,
                    files,
                });
            }
            PortType::Markdown => {
                let path = crate::blackboard::artifact_path(
                    artifacts_dir,
                    node_id,
                    iter,
                    &output_port.name,
                );
                let info = file_info_with_frontmatter(artifacts_dir, &path);
                outputs.push(PortIO {
                    port: output_port.name.clone(),
                    repeated: output_port.repeated,
                    port_type: output_port.port_type,
                    files: vec![info],
                });
            }
        }
    }

    NodeIO { inputs, outputs }
}

fn relative_path(artifacts_dir: &Path, abs_path: &Path) -> String {
    abs_path
        .strip_prefix(artifacts_dir)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| abs_path.to_string_lossy().to_string())
}

fn file_info(artifacts_dir: &Path, path: &Path) -> FileInfo {
    let exists = path.exists();
    let size = if exists {
        std::fs::metadata(path).ok().map(|m| m.len())
    } else {
        None
    };
    FileInfo {
        path: relative_path(artifacts_dir, path),
        exists,
        size,
        frontmatter: None,
    }
}

fn file_info_with_frontmatter(artifacts_dir: &Path, path: &Path) -> FileInfo {
    let exists = path.exists();
    let size = if exists {
        std::fs::metadata(path).ok().map(|m| m.len())
    } else {
        None
    };
    let frontmatter = if exists {
        frontmatter_parser::parse_frontmatter_from_file(path)
            .ok()
            .filter(|m| !m.is_empty())
            .map(|m| {
                m.into_iter()
                    .map(|(k, v)| {
                        let json_val = serde_yaml_to_json(&v);
                        (k, json_val)
                    })
                    .collect()
            })
    } else {
        None
    };
    FileInfo {
        path: relative_path(artifacts_dir, path),
        exists,
        size,
        frontmatter,
    }
}

fn serde_yaml_to_json(v: &serde_yaml::Value) -> serde_json::Value {
    match v {
        serde_yaml::Value::Null => serde_json::Value::Null,
        serde_yaml::Value::Bool(b) => serde_json::Value::Bool(*b),
        serde_yaml::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                serde_json::Value::Number(i.into())
            } else if let Some(f) = n.as_f64() {
                serde_json::Number::from_f64(f)
                    .map(serde_json::Value::Number)
                    .unwrap_or(serde_json::Value::Null)
            } else {
                serde_json::Value::Null
            }
        }
        serde_yaml::Value::String(s) => serde_json::Value::String(s.clone()),
        serde_yaml::Value::Sequence(seq) => {
            serde_json::Value::Array(seq.iter().map(serde_yaml_to_json).collect())
        }
        serde_yaml::Value::Mapping(map) => {
            let obj: serde_json::Map<String, serde_json::Value> = map
                .iter()
                .filter_map(|(k, v)| k.as_str().map(|s| (s.to_string(), serde_yaml_to_json(v))))
                .collect();
            serde_json::Value::Object(obj)
        }
        serde_yaml::Value::Tagged(tagged) => serde_yaml_to_json(&tagged.value),
    }
}

fn list_image_files(artifacts_dir: &Path, port_dir: &Path) -> Vec<FileInfo> {
    let Ok(entries) = std::fs::read_dir(port_dir) else {
        return vec![];
    };
    let mut files: Vec<FileInfo> = entries
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_type().ok().is_some_and(|ft| ft.is_file()) && pipeline::is_image_file(&e.path())
        })
        .map(|e| {
            let path = e.path();
            let size = std::fs::metadata(&path).ok().map(|m| m.len());
            FileInfo {
                path: relative_path(artifacts_dir, &path),
                exists: true,
                size,
                frontmatter: None,
            }
        })
        .collect();
    files.sort_by(|a, b| a.path.cmp(&b.path));
    files
}

fn glob_repeated(source_dir: &Path, port_name: &str) -> Vec<FileInfo> {
    let mut results = Vec::new();

    let Ok(entries) = std::fs::read_dir(source_dir) else {
        return results;
    };

    let mut iter_dirs: Vec<(i64, PathBuf)> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().ok().is_some_and(|ft| ft.is_dir()))
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let n = name.strip_prefix("iter-")?.parse::<i64>().ok()?;
            Some((n, e.path()))
        })
        .collect();

    iter_dirs.sort_by_key(|(n, _)| *n);

    let artifacts_dir = source_dir.parent().unwrap_or(source_dir);

    for (_, dir) in iter_dirs {
        let file_path = dir.join(port_name).join("output.md");
        let exists = file_path.exists();
        let size = if exists {
            std::fs::metadata(&file_path).ok().map(|m| m.len())
        } else {
            None
        };
        if exists {
            results.push(FileInfo {
                path: relative_path(artifacts_dir, &file_path),
                exists,
                size,
                frontmatter: None,
            });
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::{EdgeDef, EdgeEndpoint, NodeDef, NodeType, Port, PortType};
    use pretty_assertions::assert_eq;
    use std::fs;

    fn simple_pipeline() -> PipelineDef {
        PipelineDef {
            name: "test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                NodeDef {
                    id: "planner".into(),
                    name: "planner".into(),
                    node_type: NodeType::DocOnly,
                    inputs: vec![Port {
                        name: "task".into(),
                        repeated: false,
                        side: None,
                        port_type: PortType::Markdown,
                        frontmatter: None,
                        when: None,
                        description: None,
                    }],
                    outputs: vec![Port {
                        name: "plan".into(),
                        repeated: false,
                        side: None,
                        port_type: PortType::Markdown,
                        frontmatter: None,
                        when: None,
                        description: None,
                    }],
                    interactive: false,
                    view: None,
                    max_iter: None,
                    over: None,
                },
                NodeDef {
                    id: "implementer".into(),
                    name: "implementer".into(),
                    node_type: NodeType::CodeMutating,
                    inputs: vec![Port {
                        name: "plan".into(),
                        repeated: false,
                        side: None,
                        port_type: PortType::Markdown,
                        frontmatter: None,
                        when: None,
                        description: None,
                    }],
                    outputs: vec![Port {
                        name: "summary".into(),
                        repeated: false,
                        side: None,
                        port_type: PortType::Markdown,
                        frontmatter: None,
                        when: None,
                        description: None,
                    }],
                    interactive: false,
                    view: None,
                    max_iter: None,
                    over: None,
                },
            ],
            edges: vec![EdgeDef {
                source: EdgeEndpoint {
                    node: "planner".into(),
                    port: "plan".into(),
                },
                target: EdgeEndpoint {
                    node: "implementer".into(),
                    port: "plan".into(),
                },
                reason: None,
                when: None,
                is_else: false,
                repeated: false,
            }],
        }
    }

    #[test]
    fn simple_wire_resolves_input_path() {
        let tmp = tempfile::tempdir().unwrap();
        let artifacts = tmp.path().join("artifacts");
        fs::create_dir_all(&artifacts).unwrap();

        let pipeline = simple_pipeline();
        let io = resolve(&pipeline, &artifacts, "implementer", 1);

        assert_eq!(io.inputs.len(), 1);
        assert_eq!(io.inputs[0].port, "plan");
        assert!(!io.inputs[0].repeated);
        assert_eq!(io.inputs[0].files.len(), 1);
        assert_eq!(io.inputs[0].files[0].path, "planner/iter-1/plan/output.md");
        assert!(!io.inputs[0].files[0].exists);
    }

    #[test]
    fn simple_wire_detects_existing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let artifacts = tmp.path().join("artifacts");
        let dir = artifacts.join("planner/iter-1/plan");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("output.md"), "# My plan\nDo stuff.").unwrap();

        let pipeline = simple_pipeline();
        let io = resolve(&pipeline, &artifacts, "implementer", 1);

        assert!(io.inputs[0].files[0].exists);
        assert!(io.inputs[0].files[0].size.unwrap() > 0);
    }

    #[test]
    fn output_port_returns_path_with_frontmatter() {
        let tmp = tempfile::tempdir().unwrap();
        let artifacts = tmp.path().join("artifacts");
        let dir = artifacts.join("implementer/iter-1/summary");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("output.md"),
            "---\nverdict: PASS\nscore: 9\n---\n\n## Summary\nAll good.",
        )
        .unwrap();

        let pipeline = simple_pipeline();
        let io = resolve(&pipeline, &artifacts, "implementer", 1);

        assert_eq!(io.outputs.len(), 1);
        assert_eq!(io.outputs[0].port, "summary");
        assert!(io.outputs[0].files[0].exists);
        let fm = io.outputs[0].files[0].frontmatter.as_ref().unwrap();
        assert_eq!(fm["verdict"], serde_json::json!("PASS"));
        assert_eq!(fm["score"], serde_json::json!(9));
    }

    #[test]
    fn output_port_missing_file_no_frontmatter() {
        let tmp = tempfile::tempdir().unwrap();
        let artifacts = tmp.path().join("artifacts");
        fs::create_dir_all(&artifacts).unwrap();

        let pipeline = simple_pipeline();
        let io = resolve(&pipeline, &artifacts, "implementer", 1);

        assert_eq!(io.outputs.len(), 1);
        assert!(!io.outputs[0].files[0].exists);
        assert!(io.outputs[0].files[0].frontmatter.is_none());
    }

    #[test]
    fn repeated_port_glob_expansion() {
        let tmp = tempfile::tempdir().unwrap();
        let artifacts = tmp.path().join("artifacts");

        for i in 1..=3 {
            let dir = artifacts.join(format!("reviewer/iter-{i}/review"));
            fs::create_dir_all(&dir).unwrap();
            fs::write(
                dir.join("output.md"),
                format!("---\nverdict: FAIL\n---\n\nIter {i} review"),
            )
            .unwrap();
        }

        let pipeline = PipelineDef {
            name: "cycle".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                NodeDef {
                    id: "reviewer".into(),
                    name: "reviewer".into(),
                    node_type: NodeType::DocOnly,
                    inputs: vec![],
                    outputs: vec![Port {
                        name: "review".into(),
                        repeated: false,
                        side: None,
                        port_type: PortType::Markdown,
                        frontmatter: None,
                        when: None,
                        description: None,
                    }],
                    interactive: false,
                    view: None,
                    max_iter: None,
                    over: None,
                },
                NodeDef {
                    id: "implementer".into(),
                    name: "implementer".into(),
                    node_type: NodeType::CodeMutating,
                    inputs: vec![Port {
                        name: "reviews".into(),
                        repeated: true,
                        side: None,
                        port_type: PortType::Markdown,
                        frontmatter: None,
                        when: None,
                        description: None,
                    }],
                    outputs: vec![Port {
                        name: "code".into(),
                        repeated: false,
                        side: None,
                        port_type: PortType::Markdown,
                        frontmatter: None,
                        when: None,
                        description: None,
                    }],
                    interactive: false,
                    view: None,
                    max_iter: None,
                    over: None,
                },
            ],
            edges: vec![EdgeDef {
                source: EdgeEndpoint {
                    node: "reviewer".into(),
                    port: "review".into(),
                },
                target: EdgeEndpoint {
                    node: "implementer".into(),
                    port: "reviews".into(),
                },
                reason: None,
                when: None,
                is_else: false,
                // `repeated` lives on the edge now (#149), not the input port.
                repeated: true,
            }],
        };

        let io = resolve(&pipeline, &artifacts, "implementer", 4);

        assert_eq!(io.inputs.len(), 1);
        assert!(io.inputs[0].repeated);
        assert_eq!(io.inputs[0].files.len(), 3);
        assert_eq!(
            io.inputs[0].files[0].path,
            "reviewer/iter-1/review/output.md"
        );
        assert_eq!(
            io.inputs[0].files[1].path,
            "reviewer/iter-2/review/output.md"
        );
        assert_eq!(
            io.inputs[0].files[2].path,
            "reviewer/iter-3/review/output.md"
        );
    }

    #[test]
    fn entry_node_with_no_edges_gets_input_md() {
        let tmp = tempfile::tempdir().unwrap();
        let artifacts = tmp.path().join("artifacts");
        let input_dir = artifacts.join("_input");
        fs::create_dir_all(&input_dir).unwrap();
        fs::write(input_dir.join("output.md"), "Do the thing").unwrap();

        let pipeline = simple_pipeline();
        let io = resolve(&pipeline, &artifacts, "planner", 1);

        assert_eq!(io.inputs.len(), 1);
        assert_eq!(io.inputs[0].port, "task");
        assert_eq!(io.inputs[0].files.len(), 1);
        assert_eq!(io.inputs[0].files[0].path, "_input/output.md");
        assert!(io.inputs[0].files[0].exists);
    }

    #[test]
    fn multi_fan_in_two_sources_for_one_port() {
        let tmp = tempfile::tempdir().unwrap();
        let artifacts = tmp.path().join("artifacts");

        let pipeline = PipelineDef {
            name: "fan-in".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                NodeDef {
                    id: "a".into(),
                    name: "a".into(),
                    node_type: NodeType::DocOnly,
                    inputs: vec![],
                    outputs: vec![Port {
                        name: "out".into(),
                        repeated: false,
                        side: None,
                        port_type: PortType::Markdown,
                        frontmatter: None,
                        when: None,
                        description: None,
                    }],
                    interactive: false,
                    view: None,
                    max_iter: None,
                    over: None,
                },
                NodeDef {
                    id: "b".into(),
                    name: "b".into(),
                    node_type: NodeType::DocOnly,
                    inputs: vec![],
                    outputs: vec![Port {
                        name: "out".into(),
                        repeated: false,
                        side: None,
                        port_type: PortType::Markdown,
                        frontmatter: None,
                        when: None,
                        description: None,
                    }],
                    interactive: false,
                    view: None,
                    max_iter: None,
                    over: None,
                },
                NodeDef {
                    id: "merger".into(),
                    name: "merger".into(),
                    node_type: NodeType::DocOnly,
                    inputs: vec![Port {
                        name: "docs".into(),
                        repeated: false,
                        side: None,
                        port_type: PortType::Markdown,
                        frontmatter: None,
                        when: None,
                        description: None,
                    }],
                    outputs: vec![],
                    interactive: false,
                    view: None,
                    max_iter: None,
                    over: None,
                },
            ],
            edges: vec![
                EdgeDef {
                    source: EdgeEndpoint {
                        node: "a".into(),
                        port: "out".into(),
                    },
                    target: EdgeEndpoint {
                        node: "merger".into(),
                        port: "docs".into(),
                    },
                    reason: None,
                    when: None,
                    is_else: false,
                    repeated: false,
                },
                EdgeDef {
                    source: EdgeEndpoint {
                        node: "b".into(),
                        port: "out".into(),
                    },
                    target: EdgeEndpoint {
                        node: "merger".into(),
                        port: "docs".into(),
                    },
                    reason: None,
                    when: None,
                    is_else: false,
                    repeated: false,
                },
            ],
        };

        for dir_name in ["a", "b"] {
            let dir = artifacts.join(format!("{dir_name}/iter-1/out"));
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("output.md"), format!("from {dir_name}")).unwrap();
        }

        let io = resolve(&pipeline, &artifacts, "merger", 1);

        assert_eq!(io.inputs.len(), 1);
        assert_eq!(io.inputs[0].port, "docs");
        assert_eq!(io.inputs[0].files.len(), 2);
        assert!(io.inputs[0].files.iter().all(|f| f.exists));
    }

    #[test]
    fn emergent_input_derived_from_edge_when_node_declares_none() {
        // #149: inputs are emergent — derived from incoming edges, not declared.
        // The target node has NO declared inputs; the resolver still surfaces one
        // input, named after the target endpoint (which inherits the source name).
        let tmp = tempfile::tempdir().unwrap();
        let artifacts = tmp.path().join("artifacts");
        let dir = artifacts.join("planner/iter-1/plan");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("output.md"), "# Plan").unwrap();

        let pipeline = PipelineDef {
            name: "emergent".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                NodeDef {
                    id: "planner".into(),
                    name: "planner".into(),
                    node_type: NodeType::DocOnly,
                    inputs: vec![],
                    outputs: vec![Port {
                        name: "plan".into(),
                        repeated: false,
                        side: None,
                        port_type: PortType::Markdown,
                        frontmatter: None,
                        when: None,
                        description: None,
                    }],
                    interactive: false,
                    view: None,
                    max_iter: None,
                    over: None,
                },
                NodeDef {
                    id: "implementer".into(),
                    name: "implementer".into(),
                    node_type: NodeType::CodeMutating,
                    // Declares NO inputs — the input is emergent.
                    inputs: vec![],
                    outputs: vec![],
                    interactive: false,
                    view: None,
                    max_iter: None,
                    over: None,
                },
            ],
            edges: vec![EdgeDef {
                source: EdgeEndpoint {
                    node: "planner".into(),
                    port: "plan".into(),
                },
                target: EdgeEndpoint {
                    node: "implementer".into(),
                    port: "plan".into(),
                },
                reason: None,
                when: None,
                is_else: false,
                repeated: false,
            }],
        };

        let io = resolve(&pipeline, &artifacts, "implementer", 1);

        assert_eq!(io.inputs.len(), 1);
        assert_eq!(io.inputs[0].port, "plan");
        assert!(!io.inputs[0].repeated);
        assert_eq!(io.inputs[0].files.len(), 1);
        assert_eq!(io.inputs[0].files[0].path, "planner/iter-1/plan/output.md");
        assert!(io.inputs[0].files[0].exists);
    }

    #[test]
    fn same_named_edges_pool_into_one_list_input() {
        // #149: two incoming edges with the SAME target name pool into a single
        // logical list input. The target node declares no inputs at all.
        let tmp = tempfile::tempdir().unwrap();
        let artifacts = tmp.path().join("artifacts");
        for src in ["a", "b"] {
            let dir = artifacts.join(format!("{src}/iter-1/plan"));
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("output.md"), format!("plan from {src}")).unwrap();
        }

        let mk_node = |id: &str, has_out: bool| NodeDef {
            id: id.into(),
            name: id.into(),
            node_type: NodeType::DocOnly,
            inputs: vec![],
            outputs: if has_out {
                vec![Port {
                    name: "plan".into(),
                    repeated: false,
                    side: None,
                    port_type: PortType::Markdown,
                    frontmatter: None,
                    when: None,
                    description: None,
                }]
            } else {
                vec![]
            },
            interactive: false,
            view: None,
            max_iter: None,
            over: None,
        };
        let mk_edge = |src: &str| EdgeDef {
            source: EdgeEndpoint {
                node: src.into(),
                port: "plan".into(),
            },
            target: EdgeEndpoint {
                node: "sink".into(),
                port: "plan".into(),
            },
            reason: None,
            when: None,
            is_else: false,
            repeated: false,
        };

        let pipeline = PipelineDef {
            name: "pool".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![mk_node("a", true), mk_node("b", true), mk_node("sink", false)],
            edges: vec![mk_edge("a"), mk_edge("b")],
        };

        let io = resolve(&pipeline, &artifacts, "sink", 1);

        // One logical input, pooled into a list of both source files.
        assert_eq!(io.inputs.len(), 1);
        assert_eq!(io.inputs[0].port, "plan");
        assert!(
            io.inputs[0].repeated,
            "pooled same-name edges become a list input"
        );
        assert_eq!(io.inputs[0].files.len(), 2);
        assert!(io.inputs[0].files.iter().all(|f| f.exists));
    }

    #[test]
    fn distinct_named_edges_stay_separate_inputs() {
        // #149: two incoming edges with DISTINCT target names are two separate
        // emergent inputs (no pooling). Order follows edge declaration order.
        let tmp = tempfile::tempdir().unwrap();
        let artifacts = tmp.path().join("artifacts");
        for (src, port) in [("planner", "plan"), ("designer", "spec")] {
            let dir = artifacts.join(format!("{src}/iter-1/{port}"));
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("output.md"), format!("{port} from {src}")).unwrap();
        }

        let mk_node = |id: &str, out: Option<&str>| NodeDef {
            id: id.into(),
            name: id.into(),
            node_type: NodeType::DocOnly,
            inputs: vec![],
            outputs: out
                .map(|o| {
                    vec![Port {
                        name: o.into(),
                        repeated: false,
                        side: None,
                        port_type: PortType::Markdown,
                        frontmatter: None,
                        when: None,
                        description: None,
                    }]
                })
                .unwrap_or_default(),
            interactive: false,
            view: None,
            max_iter: None,
            over: None,
        };
        let mk_edge = |src: &str, port: &str| EdgeDef {
            source: EdgeEndpoint {
                node: src.into(),
                port: port.into(),
            },
            target: EdgeEndpoint {
                node: "sink".into(),
                port: port.into(),
            },
            reason: None,
            when: None,
            is_else: false,
            repeated: false,
        };

        let pipeline = PipelineDef {
            name: "distinct".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                mk_node("planner", Some("plan")),
                mk_node("designer", Some("spec")),
                mk_node("sink", None),
            ],
            edges: vec![mk_edge("planner", "plan"), mk_edge("designer", "spec")],
        };

        let io = resolve(&pipeline, &artifacts, "sink", 1);

        assert_eq!(io.inputs.len(), 2);
        assert_eq!(io.inputs[0].port, "plan");
        assert_eq!(io.inputs[1].port, "spec");
        assert!(!io.inputs[0].repeated);
        assert!(!io.inputs[1].repeated);
        assert_eq!(io.inputs[0].files.len(), 1);
        assert_eq!(io.inputs[1].files.len(), 1);
    }

    #[test]
    fn missing_node_returns_empty_io() {
        let tmp = tempfile::tempdir().unwrap();
        let artifacts = tmp.path().join("artifacts");
        fs::create_dir_all(&artifacts).unwrap();

        let pipeline = simple_pipeline();
        let io = resolve(&pipeline, &artifacts, "nonexistent", 1);

        assert!(io.inputs.is_empty());
        assert!(io.outputs.is_empty());
    }
}
