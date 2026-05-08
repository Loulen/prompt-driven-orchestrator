use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::frontmatter_parser;
use crate::pipeline::PipelineDef;

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

    let mut inputs: Vec<PortIO> = Vec::new();

    for input_port in &node.inputs {
        let mut files = Vec::new();
        let mut found_edge = false;

        for edge in &pipeline.edges {
            if edge.target.node != node_id || edge.target.port != input_port.name {
                continue;
            }
            found_edge = true;

            if input_port.repeated {
                let source_dir = artifacts_dir.join(&edge.source.node);
                files.extend(glob_repeated(&source_dir, &edge.source.port));
            } else {
                let path = artifacts_dir
                    .join(&edge.source.node)
                    .join(format!("iter-{iter}"))
                    .join(format!("{}.md", edge.source.port));
                files.push(file_info(artifacts_dir, &path));
            }
        }

        if !found_edge && input_port.name == "task" {
            let path = artifacts_dir.join("_input.md");
            files.push(file_info(artifacts_dir, &path));
        }

        inputs.push(PortIO {
            port: input_port.name.clone(),
            repeated: input_port.repeated,
            files,
        });
    }

    let mut outputs: Vec<PortIO> = Vec::new();
    for output_port in &node.outputs {
        let path = artifacts_dir
            .join(node_id)
            .join(format!("iter-{iter}"))
            .join(format!("{}.md", output_port.name));
        let info = file_info_with_frontmatter(artifacts_dir, &path);
        outputs.push(PortIO {
            port: output_port.name.clone(),
            repeated: output_port.repeated,
            files: vec![info],
        });
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

fn glob_repeated(source_dir: &Path, port_name: &str) -> Vec<FileInfo> {
    let mut results = Vec::new();
    let filename = format!("{port_name}.md");

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
        let file_path = dir.join(&filename);
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
    use crate::pipeline::{EdgeDef, EdgeEndpoint, NodeDef, NodeType, Port};
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
                        frontmatter: None,
                        when: None,
                    }],
                    outputs: vec![Port {
                        name: "plan".into(),
                        repeated: false,
                        side: None,
                        frontmatter: None,
                        when: None,
                    }],
                    interactive: false,
                    view: None,
                    max_iter: None,
                },
                NodeDef {
                    id: "implementer".into(),
                    name: "implementer".into(),
                    node_type: NodeType::CodeMutating,
                    inputs: vec![Port {
                        name: "plan".into(),
                        repeated: false,
                        side: None,
                        frontmatter: None,
                        when: None,
                    }],
                    outputs: vec![Port {
                        name: "summary".into(),
                        repeated: false,
                        side: None,
                        frontmatter: None,
                        when: None,
                    }],
                    interactive: false,
                    view: None,
                    max_iter: None,
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
            }],
            auto_merge_resolver: true,
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
        assert_eq!(io.inputs[0].files[0].path, "planner/iter-1/plan.md");
        assert!(!io.inputs[0].files[0].exists);
    }

    #[test]
    fn simple_wire_detects_existing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let artifacts = tmp.path().join("artifacts");
        let dir = artifacts.join("planner/iter-1");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("plan.md"), "# My plan\nDo stuff.").unwrap();

        let pipeline = simple_pipeline();
        let io = resolve(&pipeline, &artifacts, "implementer", 1);

        assert!(io.inputs[0].files[0].exists);
        assert!(io.inputs[0].files[0].size.unwrap() > 0);
    }

    #[test]
    fn output_port_returns_path_with_frontmatter() {
        let tmp = tempfile::tempdir().unwrap();
        let artifacts = tmp.path().join("artifacts");
        let dir = artifacts.join("implementer/iter-1");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("summary.md"),
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
            let dir = artifacts.join(format!("reviewer/iter-{i}"));
            fs::create_dir_all(&dir).unwrap();
            fs::write(
                dir.join("review.md"),
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
                        frontmatter: None,
                        when: None,
                    }],
                    interactive: false,
                    view: None,
                    max_iter: None,
                },
                NodeDef {
                    id: "implementer".into(),
                    name: "implementer".into(),
                    node_type: NodeType::CodeMutating,
                    inputs: vec![Port {
                        name: "reviews".into(),
                        repeated: true,
                        side: None,
                        frontmatter: None,
                        when: None,
                    }],
                    outputs: vec![Port {
                        name: "code".into(),
                        repeated: false,
                        side: None,
                        frontmatter: None,
                        when: None,
                    }],
                    interactive: false,
                    view: None,
                    max_iter: None,
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
            }],
            auto_merge_resolver: true,
        };

        let io = resolve(&pipeline, &artifacts, "implementer", 4);

        assert_eq!(io.inputs.len(), 1);
        assert!(io.inputs[0].repeated);
        assert_eq!(io.inputs[0].files.len(), 3);
        assert_eq!(io.inputs[0].files[0].path, "reviewer/iter-1/review.md");
        assert_eq!(io.inputs[0].files[1].path, "reviewer/iter-2/review.md");
        assert_eq!(io.inputs[0].files[2].path, "reviewer/iter-3/review.md");
    }

    #[test]
    fn entry_node_with_no_edges_gets_input_md() {
        let tmp = tempfile::tempdir().unwrap();
        let artifacts = tmp.path().join("artifacts");
        fs::create_dir_all(&artifacts).unwrap();
        fs::write(artifacts.join("_input.md"), "Do the thing").unwrap();

        let pipeline = simple_pipeline();
        let io = resolve(&pipeline, &artifacts, "planner", 1);

        assert_eq!(io.inputs.len(), 1);
        assert_eq!(io.inputs[0].port, "task");
        assert_eq!(io.inputs[0].files.len(), 1);
        assert_eq!(io.inputs[0].files[0].path, "_input.md");
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
                        frontmatter: None,
                        when: None,
                    }],
                    interactive: false,
                    view: None,
                    max_iter: None,
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
                        frontmatter: None,
                        when: None,
                    }],
                    interactive: false,
                    view: None,
                    max_iter: None,
                },
                NodeDef {
                    id: "merger".into(),
                    name: "merger".into(),
                    node_type: NodeType::DocOnly,
                    inputs: vec![Port {
                        name: "docs".into(),
                        repeated: false,
                        side: None,
                        frontmatter: None,
                        when: None,
                    }],
                    outputs: vec![],
                    interactive: false,
                    view: None,
                    max_iter: None,
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
                },
            ],
            auto_merge_resolver: true,
        };

        for dir_name in ["a", "b"] {
            let dir = artifacts.join(format!("{dir_name}/iter-1"));
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("out.md"), format!("from {dir_name}")).unwrap();
        }

        let io = resolve(&pipeline, &artifacts, "merger", 1);

        assert_eq!(io.inputs.len(), 1);
        assert_eq!(io.inputs[0].port, "docs");
        assert_eq!(io.inputs[0].files.len(), 2);
        assert!(io.inputs[0].files.iter().all(|f| f.exists));
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
