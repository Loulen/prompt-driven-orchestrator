use std::path::Path;

use crate::pipeline::PipelineDef;

fn iter_dirs_containing(node_dir: &Path, filename: &str) -> usize {
    let entries = match std::fs::read_dir(node_dir) {
        Ok(e) => e,
        Err(_) => return 0,
    };
    entries
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.file_name().to_string_lossy().starts_with("iter-")
                && entry.path().join(filename).exists()
        })
        .count()
}

pub fn validate(
    pipeline: &PipelineDef,
    node_id: &str,
    iter: i64,
    artifacts_dir: &Path,
) -> Result<(), Vec<String>> {
    let node = match pipeline.nodes.iter().find(|n| n.id == node_id) {
        Some(n) => n,
        None => return Ok(()),
    };

    if node.outputs.is_empty() {
        return Ok(());
    }

    let mut missing = Vec::new();

    for port in &node.outputs {
        if port.repeated {
            let node_dir = artifacts_dir.join(node_id);
            let found = iter_dirs_containing(&node_dir, &format!("{}.md", port.name));
            if found == 0 {
                missing.push(port.name.clone());
            }
        } else {
            let path = artifacts_dir
                .join(node_id)
                .join(format!("iter-{iter}"))
                .join(format!("{}.md", port.name));
            if !path.exists() {
                missing.push(port.name.clone());
            }
        }
    }

    if missing.is_empty() {
        Ok(())
    } else {
        Err(missing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::{NodeDef, NodeType, Port};
    use std::collections::HashMap;
    use tempfile::TempDir;

    fn make_pipeline(nodes: Vec<NodeDef>) -> PipelineDef {
        PipelineDef {
            name: "test".into(),
            version: None,
            variables: HashMap::new(),
            nodes,
            edges: Vec::new(),
        }
    }

    fn make_node(id: &str, outputs: Vec<Port>) -> NodeDef {
        NodeDef {
            id: id.into(),
            node_type: NodeType::DocOnly,
            prompt_file: None,
            inputs: Vec::new(),
            outputs,
            interactive: false,
            view: None,
        }
    }

    fn port(name: &str) -> Port {
        Port {
            name: name.into(),
            repeated: false,
            frontmatter: None,
        }
    }

    fn repeated_port(name: &str) -> Port {
        Port {
            name: name.into(),
            repeated: true,
            frontmatter: None,
        }
    }

    #[test]
    fn all_outputs_present() {
        let tmp = TempDir::new().unwrap();
        let artifacts = tmp.path();

        let dir = artifacts.join("reviewer").join("iter-1");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("review.md"), "---\nverdict: PASS\n---\nLGTM").unwrap();

        let pipeline = make_pipeline(vec![make_node("reviewer", vec![port("review")])]);

        assert!(validate(&pipeline, "reviewer", 1, artifacts).is_ok());
    }

    #[test]
    fn single_output_missing() {
        let tmp = TempDir::new().unwrap();
        let artifacts = tmp.path();

        let pipeline = make_pipeline(vec![make_node(
            "reviewer",
            vec![port("review"), port("summary")],
        )]);

        // Create only 'summary', not 'review'
        let dir = artifacts.join("reviewer").join("iter-1");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("summary.md"), "done").unwrap();

        let result = validate(&pipeline, "reviewer", 1, artifacts);
        assert!(result.is_err());
        let missing = result.unwrap_err();
        assert_eq!(missing, vec!["review"]);
    }

    #[test]
    fn repeated_port_with_zero_files() {
        let tmp = TempDir::new().unwrap();
        let artifacts = tmp.path();

        let pipeline = make_pipeline(vec![make_node("impl", vec![repeated_port("patches")])]);

        let result = validate(&pipeline, "impl", 1, artifacts);
        assert!(result.is_err());
        let missing = result.unwrap_err();
        assert_eq!(missing, vec!["patches"]);
    }

    #[test]
    fn repeated_port_with_one_file() {
        let tmp = TempDir::new().unwrap();
        let artifacts = tmp.path();

        let dir = artifacts.join("impl").join("iter-1");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("patches.md"), "patch 1").unwrap();

        let pipeline = make_pipeline(vec![make_node("impl", vec![repeated_port("patches")])]);

        assert!(validate(&pipeline, "impl", 1, artifacts).is_ok());
    }

    #[test]
    fn mix_of_present_and_missing() {
        let tmp = TempDir::new().unwrap();
        let artifacts = tmp.path();

        let dir = artifacts.join("worker").join("iter-2");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("summary.md"), "done").unwrap();

        let pipeline = make_pipeline(vec![make_node(
            "worker",
            vec![port("summary"), port("report"), port("metrics")],
        )]);

        let result = validate(&pipeline, "worker", 2, artifacts);
        assert!(result.is_err());
        let mut missing = result.unwrap_err();
        missing.sort();
        assert_eq!(missing, vec!["metrics", "report"]);
    }

    #[test]
    fn node_with_zero_outputs_always_ok() {
        let tmp = TempDir::new().unwrap();
        let artifacts = tmp.path();

        let pipeline = make_pipeline(vec![make_node("noop", vec![])]);

        assert!(validate(&pipeline, "noop", 1, artifacts).is_ok());
    }

    #[test]
    fn unknown_node_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let artifacts = tmp.path();

        let pipeline = make_pipeline(vec![]);

        assert!(validate(&pipeline, "ghost", 1, artifacts).is_ok());
    }
}
