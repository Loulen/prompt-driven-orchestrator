use std::collections::HashMap;
use std::path::Path;

use crate::frontmatter_parser;
use crate::pipeline::{self, FrontmatterFieldDecl, PipelineDef, Port, PortType};

fn iter_dirs_containing(node_dir: &Path, port_name: &str) -> usize {
    let entries = match std::fs::read_dir(node_dir) {
        Ok(e) => e,
        Err(_) => return 0,
    };
    entries
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.file_name().to_string_lossy().starts_with("iter-")
                && entry.path().join(port_name).join("output.md").exists()
        })
        .count()
}

fn count_image_files(dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .filter_map(Result::ok)
        .filter(|e| {
            e.file_type().ok().is_some_and(|ft| ft.is_file()) && pipeline::is_image_file(&e.path())
        })
        .count()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldViolation {
    pub port: String,
    pub field: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    MissingOutputs(Vec<String>),
    FrontmatterMismatch(Vec<FieldViolation>),
}

pub fn validate(
    pipeline: &PipelineDef,
    node_id: &str,
    iter: i64,
    artifacts_dir: &Path,
) -> Result<(), ValidationError> {
    let node = match pipeline.nodes.iter().find(|n| n.id == node_id) {
        Some(n) => n,
        None => return Ok(()),
    };

    if node.outputs.is_empty() {
        return Ok(());
    }

    let mut missing = Vec::new();
    let mut violations = Vec::new();

    for port in &node.outputs {
        match port.port_type {
            PortType::Image => {
                let port_dir =
                    crate::blackboard::port_dir(artifacts_dir, node_id, iter, &port.name);
                let count = count_image_files(&port_dir);
                if count == 0 {
                    missing.push(port.name.clone());
                } else if count > 1 {
                    violations.push(FieldViolation {
                        port: port.name.clone(),
                        field: "(image)".into(),
                        reason: format!("image port expects exactly 1 image file, found {count}"),
                    });
                }
            }
            PortType::ImageList => {
                let port_dir =
                    crate::blackboard::port_dir(artifacts_dir, node_id, iter, &port.name);
                let count = count_image_files(&port_dir);
                if count == 0 {
                    missing.push(port.name.clone());
                }
            }
            PortType::Markdown => {
                if port.repeated {
                    let node_dir = artifacts_dir.join(node_id);
                    let found = iter_dirs_containing(&node_dir, &port.name);
                    if found == 0 {
                        missing.push(port.name.clone());
                    }
                } else {
                    let path =
                        crate::blackboard::artifact_path(artifacts_dir, node_id, iter, &port.name);
                    if !path.exists() {
                        missing.push(port.name.clone());
                    }
                }
            }
        }
    }

    if !missing.is_empty() {
        return Err(ValidationError::MissingOutputs(missing));
    }

    if !violations.is_empty() {
        return Err(ValidationError::FrontmatterMismatch(violations));
    }

    let fm_violations = validate_frontmatter_schemas(&node.outputs, node_id, iter, artifacts_dir);
    if !fm_violations.is_empty() {
        return Err(ValidationError::FrontmatterMismatch(fm_violations));
    }

    Ok(())
}

fn validate_frontmatter_schemas(
    outputs: &[Port],
    node_id: &str,
    iter: i64,
    artifacts_dir: &Path,
) -> Vec<FieldViolation> {
    let mut violations = Vec::new();

    for port in outputs {
        if port.port_type != PortType::Markdown {
            continue;
        }
        let schema = match &port.frontmatter {
            Some(s) if !s.is_empty() => s,
            _ => continue,
        };

        let path = crate::blackboard::artifact_path(artifacts_dir, node_id, iter, &port.name);

        let fields = match frontmatter_parser::parse_frontmatter_from_file(&path) {
            Ok(f) => f,
            Err(_) => {
                for field_name in schema.keys() {
                    violations.push(FieldViolation {
                        port: port.name.clone(),
                        field: field_name.clone(),
                        reason: "frontmatter could not be parsed".into(),
                    });
                }
                continue;
            }
        };

        for (field_name, decl) in schema {
            validate_field(&port.name, field_name, decl, &fields, &mut violations);
        }
    }

    violations
}

fn validate_field(
    port_name: &str,
    field_name: &str,
    decl: &FrontmatterFieldDecl,
    fields: &HashMap<String, serde_yaml::Value>,
    violations: &mut Vec<FieldViolation>,
) {
    let value = match fields.get(field_name) {
        Some(v) => v,
        None => {
            violations.push(FieldViolation {
                port: port_name.into(),
                field: field_name.into(),
                reason: "missing required field".into(),
            });
            return;
        }
    };

    match decl.field_type.as_str() {
        "enum" => {
            let val_str = yaml_value_to_string(value);
            if let Some(allowed) = &decl.allowed {
                if !allowed.contains(&val_str) {
                    violations.push(FieldViolation {
                        port: port_name.into(),
                        field: field_name.into(),
                        reason: format!("value '{}' not in allowed values: {:?}", val_str, allowed),
                    });
                }
            }
        }
        "int" if !value.is_i64() && !value.is_u64() => {
            violations.push(FieldViolation {
                port: port_name.into(),
                field: field_name.into(),
                reason: format!("expected int, got '{}'", yaml_value_to_string(value)),
            });
        }
        "string" if !value.is_string() => {
            violations.push(FieldViolation {
                port: port_name.into(),
                field: field_name.into(),
                reason: format!("expected string, got '{}'", yaml_value_to_string(value)),
            });
        }
        "bool" if !value.is_bool() => {
            violations.push(FieldViolation {
                port: port_name.into(),
                field: field_name.into(),
                reason: format!("expected bool, got '{}'", yaml_value_to_string(value)),
            });
        }
        "list" if !value.is_sequence() => {
            violations.push(FieldViolation {
                port: port_name.into(),
                field: field_name.into(),
                reason: format!("expected list, got '{}'", yaml_value_to_string(value)),
            });
        }
        _ => {}
    }
}

fn yaml_value_to_string(v: &serde_yaml::Value) -> String {
    match v {
        serde_yaml::Value::String(s) => s.clone(),
        serde_yaml::Value::Bool(b) => b.to_string(),
        serde_yaml::Value::Number(n) => n.to_string(),
        serde_yaml::Value::Null => "null".into(),
        other => serde_yaml::to_string(other)
            .unwrap_or_default()
            .trim()
            .to_string(),
    }
}

pub fn corrective_message(violations: &[FieldViolation]) -> String {
    let mut msg = String::from(
        "Your output frontmatter does not match the declared schema. Please fix the following and retry:\n",
    );
    for v in violations {
        msg.push_str(&format!(
            "  - port '{}', field '{}': {}\n",
            v.port, v.field, v.reason
        ));
    }
    msg.push_str("After correcting, call `pdo complete` again.");
    msg
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::{FrontmatterFieldDecl, NodeDef, NodeType, Port, PortType};
    use std::collections::HashMap;
    use tempfile::TempDir;

    fn make_pipeline(nodes: Vec<NodeDef>) -> PipelineDef {
        PipelineDef {
            name: "test".into(),
            version: None,
            variables: HashMap::new(),
            nodes,
            edges: Vec::new(),
            loops: Vec::new(),
            prompt_required: true,
        }
    }

    fn make_node(id: &str, outputs: Vec<Port>) -> NodeDef {
        NodeDef {
            id: id.into(),
            name: id.into(),
            node_type: NodeType::DocOnly,
            inputs: Vec::new(),
            outputs,
            interactive: false,
            view: None,
            max_iter: None,
            over: None,
        }
    }

    fn port(name: &str) -> Port {
        Port {
            name: name.into(),
            repeated: false,
            side: None,
            port_type: PortType::Markdown,
            frontmatter: None,
            when: None,
            description: None,
        }
    }

    fn repeated_port(name: &str) -> Port {
        Port {
            name: name.into(),
            repeated: true,
            side: None,
            port_type: PortType::Markdown,
            frontmatter: None,
            when: None,
            description: None,
        }
    }

    fn typed_port(name: &str, schema: HashMap<String, FrontmatterFieldDecl>) -> Port {
        Port {
            name: name.into(),
            repeated: false,
            side: None,
            port_type: PortType::Markdown,
            frontmatter: Some(schema),
            when: None,
            description: None,
        }
    }

    fn field_decl(field_type: &str, allowed: Option<Vec<&str>>) -> FrontmatterFieldDecl {
        FrontmatterFieldDecl {
            field_type: field_type.into(),
            allowed: allowed.map(|a| a.into_iter().map(String::from).collect()),
        }
    }

    fn write_artifact(dir: &Path, node_id: &str, iter: i64, port_name: &str, content: &str) {
        let d = dir
            .join(node_id)
            .join(format!("iter-{iter}"))
            .join(port_name);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("output.md"), content).unwrap();
    }

    // --- existence checks (unchanged behavior) ---

    #[test]
    fn all_outputs_present() {
        let tmp = TempDir::new().unwrap();
        let artifacts = tmp.path();
        write_artifact(
            artifacts,
            "reviewer",
            1,
            "review",
            "---\nverdict: PASS\n---\nLGTM",
        );
        let pipeline = make_pipeline(vec![make_node("reviewer", vec![port("review")])]);
        assert!(validate(&pipeline, "reviewer", 1, artifacts).is_ok());
    }

    #[test]
    fn single_output_missing() {
        let tmp = TempDir::new().unwrap();
        let artifacts = tmp.path();
        write_artifact(artifacts, "reviewer", 1, "summary", "done");
        let pipeline = make_pipeline(vec![make_node(
            "reviewer",
            vec![port("review"), port("summary")],
        )]);
        let result = validate(&pipeline, "reviewer", 1, artifacts);
        assert!(matches!(result, Err(ValidationError::MissingOutputs(ref m)) if m == &["review"]));
    }

    #[test]
    fn repeated_port_with_zero_files() {
        let tmp = TempDir::new().unwrap();
        let artifacts = tmp.path();
        let pipeline = make_pipeline(vec![make_node("impl", vec![repeated_port("patches")])]);
        let result = validate(&pipeline, "impl", 1, artifacts);
        assert!(matches!(result, Err(ValidationError::MissingOutputs(ref m)) if m == &["patches"]));
    }

    #[test]
    fn repeated_port_with_one_file() {
        let tmp = TempDir::new().unwrap();
        let artifacts = tmp.path();
        write_artifact(artifacts, "impl", 1, "patches", "patch 1");
        let pipeline = make_pipeline(vec![make_node("impl", vec![repeated_port("patches")])]);
        assert!(validate(&pipeline, "impl", 1, artifacts).is_ok());
    }

    #[test]
    fn mix_of_present_and_missing() {
        let tmp = TempDir::new().unwrap();
        let artifacts = tmp.path();
        write_artifact(artifacts, "worker", 2, "summary", "done");
        let pipeline = make_pipeline(vec![make_node(
            "worker",
            vec![port("summary"), port("report"), port("metrics")],
        )]);
        let result = validate(&pipeline, "worker", 2, artifacts);
        match result {
            Err(ValidationError::MissingOutputs(mut m)) => {
                m.sort();
                assert_eq!(m, vec!["metrics", "report"]);
            }
            other => panic!("expected MissingOutputs, got {other:?}"),
        }
    }

    #[test]
    fn node_with_zero_outputs_always_ok() {
        let tmp = TempDir::new().unwrap();
        let pipeline = make_pipeline(vec![make_node("noop", vec![])]);
        assert!(validate(&pipeline, "noop", 1, tmp.path()).is_ok());
    }

    #[test]
    fn unknown_node_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let pipeline = make_pipeline(vec![]);
        assert!(validate(&pipeline, "ghost", 1, tmp.path()).is_ok());
    }

    // --- frontmatter schema validation: enum ---

    #[test]
    fn enum_valid_value_passes() {
        let tmp = TempDir::new().unwrap();
        let artifacts = tmp.path();
        write_artifact(
            artifacts,
            "rev",
            1,
            "review",
            "---\nverdict: PASS\n---\nLGTM",
        );
        let schema = HashMap::from([(
            "verdict".into(),
            field_decl("enum", Some(vec!["PASS", "FAIL"])),
        )]);
        let pipeline = make_pipeline(vec![make_node("rev", vec![typed_port("review", schema)])]);
        assert!(validate(&pipeline, "rev", 1, artifacts).is_ok());
    }

    #[test]
    fn enum_invalid_value_fails() {
        let tmp = TempDir::new().unwrap();
        let artifacts = tmp.path();
        write_artifact(
            artifacts,
            "rev",
            1,
            "review",
            "---\nverdict: MAYBE\n---\nbody",
        );
        let schema = HashMap::from([(
            "verdict".into(),
            field_decl("enum", Some(vec!["PASS", "FAIL"])),
        )]);
        let pipeline = make_pipeline(vec![make_node("rev", vec![typed_port("review", schema)])]);
        let result = validate(&pipeline, "rev", 1, artifacts);
        match result {
            Err(ValidationError::FrontmatterMismatch(v)) => {
                assert_eq!(v.len(), 1);
                assert_eq!(v[0].field, "verdict");
                assert!(v[0].reason.contains("MAYBE"));
            }
            other => panic!("expected FrontmatterMismatch, got {other:?}"),
        }
    }

    // --- frontmatter schema validation: int ---

    #[test]
    fn int_valid_value_passes() {
        let tmp = TempDir::new().unwrap();
        let artifacts = tmp.path();
        write_artifact(artifacts, "node", 1, "out", "---\nscore: 42\n---\nbody");
        let schema = HashMap::from([("score".into(), field_decl("int", None))]);
        let pipeline = make_pipeline(vec![make_node("node", vec![typed_port("out", schema)])]);
        assert!(validate(&pipeline, "node", 1, artifacts).is_ok());
    }

    #[test]
    fn int_string_value_fails() {
        let tmp = TempDir::new().unwrap();
        let artifacts = tmp.path();
        write_artifact(artifacts, "node", 1, "out", "---\nscore: high\n---\nbody");
        let schema = HashMap::from([("score".into(), field_decl("int", None))]);
        let pipeline = make_pipeline(vec![make_node("node", vec![typed_port("out", schema)])]);
        let result = validate(&pipeline, "node", 1, artifacts);
        match result {
            Err(ValidationError::FrontmatterMismatch(v)) => {
                assert_eq!(v[0].field, "score");
                assert!(v[0].reason.contains("expected int"));
            }
            other => panic!("expected FrontmatterMismatch, got {other:?}"),
        }
    }

    // --- frontmatter schema validation: string ---

    #[test]
    fn string_valid_value_passes() {
        let tmp = TempDir::new().unwrap();
        let artifacts = tmp.path();
        write_artifact(
            artifacts,
            "node",
            1,
            "out",
            "---\ntitle: hello world\n---\nbody",
        );
        let schema = HashMap::from([("title".into(), field_decl("string", None))]);
        let pipeline = make_pipeline(vec![make_node("node", vec![typed_port("out", schema)])]);
        assert!(validate(&pipeline, "node", 1, artifacts).is_ok());
    }

    #[test]
    fn string_int_value_fails() {
        let tmp = TempDir::new().unwrap();
        let artifacts = tmp.path();
        write_artifact(artifacts, "node", 1, "out", "---\ntitle: 42\n---\nbody");
        let schema = HashMap::from([("title".into(), field_decl("string", None))]);
        let pipeline = make_pipeline(vec![make_node("node", vec![typed_port("out", schema)])]);
        let result = validate(&pipeline, "node", 1, artifacts);
        match result {
            Err(ValidationError::FrontmatterMismatch(v)) => {
                assert_eq!(v[0].field, "title");
                assert!(v[0].reason.contains("expected string"));
            }
            other => panic!("expected FrontmatterMismatch, got {other:?}"),
        }
    }

    // --- frontmatter schema validation: bool ---

    #[test]
    fn bool_valid_value_passes() {
        let tmp = TempDir::new().unwrap();
        let artifacts = tmp.path();
        write_artifact(
            artifacts,
            "node",
            1,
            "out",
            "---\napproved: true\n---\nbody",
        );
        let schema = HashMap::from([("approved".into(), field_decl("bool", None))]);
        let pipeline = make_pipeline(vec![make_node("node", vec![typed_port("out", schema)])]);
        assert!(validate(&pipeline, "node", 1, artifacts).is_ok());
    }

    #[test]
    fn bool_string_value_fails() {
        let tmp = TempDir::new().unwrap();
        let artifacts = tmp.path();
        write_artifact(
            artifacts,
            "node",
            1,
            "out",
            "---\napproved: yes_please\n---\nbody",
        );
        let schema = HashMap::from([("approved".into(), field_decl("bool", None))]);
        let pipeline = make_pipeline(vec![make_node("node", vec![typed_port("out", schema)])]);
        let result = validate(&pipeline, "node", 1, artifacts);
        match result {
            Err(ValidationError::FrontmatterMismatch(v)) => {
                assert_eq!(v[0].field, "approved");
                assert!(v[0].reason.contains("expected bool"));
            }
            other => panic!("expected FrontmatterMismatch, got {other:?}"),
        }
    }

    // --- frontmatter schema validation: list ---

    #[test]
    fn list_valid_value_passes() {
        let tmp = TempDir::new().unwrap();
        let artifacts = tmp.path();
        write_artifact(
            artifacts,
            "node",
            1,
            "out",
            "---\nissues:\n  - foo\n  - bar\n---\nbody",
        );
        let schema = HashMap::from([("issues".into(), field_decl("list", None))]);
        let pipeline = make_pipeline(vec![make_node("node", vec![typed_port("out", schema)])]);
        assert!(validate(&pipeline, "node", 1, artifacts).is_ok());
    }

    #[test]
    fn list_scalar_value_fails() {
        let tmp = TempDir::new().unwrap();
        let artifacts = tmp.path();
        write_artifact(artifacts, "node", 1, "out", "---\nissues: none\n---\nbody");
        let schema = HashMap::from([("issues".into(), field_decl("list", None))]);
        let pipeline = make_pipeline(vec![make_node("node", vec![typed_port("out", schema)])]);
        let result = validate(&pipeline, "node", 1, artifacts);
        match result {
            Err(ValidationError::FrontmatterMismatch(v)) => {
                assert_eq!(v[0].field, "issues");
                assert!(v[0].reason.contains("expected list"));
            }
            other => panic!("expected FrontmatterMismatch, got {other:?}"),
        }
    }

    // --- missing required field ---

    #[test]
    fn missing_required_field_fails() {
        let tmp = TempDir::new().unwrap();
        let artifacts = tmp.path();
        write_artifact(artifacts, "node", 1, "out", "---\nother: value\n---\nbody");
        let schema = HashMap::from([(
            "verdict".into(),
            field_decl("enum", Some(vec!["PASS", "FAIL"])),
        )]);
        let pipeline = make_pipeline(vec![make_node("node", vec![typed_port("out", schema)])]);
        let result = validate(&pipeline, "node", 1, artifacts);
        match result {
            Err(ValidationError::FrontmatterMismatch(v)) => {
                assert_eq!(v[0].field, "verdict");
                assert!(v[0].reason.contains("missing"));
            }
            other => panic!("expected FrontmatterMismatch, got {other:?}"),
        }
    }

    // --- no schema = no validation ---

    #[test]
    fn port_without_schema_skips_content_validation() {
        let tmp = TempDir::new().unwrap();
        let artifacts = tmp.path();
        write_artifact(artifacts, "node", 1, "out", "just text no frontmatter");
        let pipeline = make_pipeline(vec![make_node("node", vec![port("out")])]);
        assert!(validate(&pipeline, "node", 1, artifacts).is_ok());
    }

    // --- corrective message ---

    #[test]
    fn corrective_message_lists_all_violations() {
        let violations = vec![
            FieldViolation {
                port: "review".into(),
                field: "verdict".into(),
                reason: "value 'MAYBE' not in allowed values: [\"PASS\", \"FAIL\"]".into(),
            },
            FieldViolation {
                port: "review".into(),
                field: "score".into(),
                reason: "expected int, got 'high'".into(),
            },
        ];
        let msg = corrective_message(&violations);
        assert!(msg.contains("verdict"));
        assert!(msg.contains("score"));
        assert!(msg.contains("pdo complete"));
    }

    // --- image port helpers ---

    fn image_port(name: &str) -> Port {
        Port {
            name: name.into(),
            repeated: false,
            side: None,
            port_type: PortType::Image,
            frontmatter: None,
            when: None,
            description: None,
        }
    }

    fn image_list_port(name: &str) -> Port {
        Port {
            name: name.into(),
            repeated: false,
            side: None,
            port_type: PortType::ImageList,
            frontmatter: None,
            when: None,
            description: None,
        }
    }

    fn write_image(dir: &Path, node_id: &str, iter: i64, port_name: &str, filename: &str) {
        let d = dir
            .join(node_id)
            .join(format!("iter-{iter}"))
            .join(port_name);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join(filename), b"fake image data").unwrap();
    }

    // --- image port: exactly one image required ---

    #[test]
    fn image_port_with_one_image_passes() {
        let tmp = TempDir::new().unwrap();
        let artifacts = tmp.path();
        write_image(artifacts, "node", 1, "screenshot", "output.png");
        let pipeline = make_pipeline(vec![make_node("node", vec![image_port("screenshot")])]);
        assert!(validate(&pipeline, "node", 1, artifacts).is_ok());
    }

    #[test]
    fn image_port_with_no_files_missing() {
        let tmp = TempDir::new().unwrap();
        let artifacts = tmp.path();
        let pipeline = make_pipeline(vec![make_node("node", vec![image_port("screenshot")])]);
        let result = validate(&pipeline, "node", 1, artifacts);
        assert!(
            matches!(result, Err(ValidationError::MissingOutputs(ref m)) if m == &["screenshot"])
        );
    }

    #[test]
    fn image_port_with_two_images_fails() {
        let tmp = TempDir::new().unwrap();
        let artifacts = tmp.path();
        write_image(artifacts, "node", 1, "screenshot", "a.png");
        write_image(artifacts, "node", 1, "screenshot", "b.jpg");
        let pipeline = make_pipeline(vec![make_node("node", vec![image_port("screenshot")])]);
        let result = validate(&pipeline, "node", 1, artifacts);
        match result {
            Err(ValidationError::FrontmatterMismatch(v)) => {
                assert_eq!(v.len(), 1);
                assert!(v[0].reason.contains("exactly 1"));
            }
            other => panic!("expected FrontmatterMismatch, got {other:?}"),
        }
    }

    #[test]
    fn image_port_ignores_non_image_files() {
        let tmp = TempDir::new().unwrap();
        let artifacts = tmp.path();
        let d = artifacts.join("node/iter-1/screenshot");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("notes.txt"), "not an image").unwrap();
        let pipeline = make_pipeline(vec![make_node("node", vec![image_port("screenshot")])]);
        let result = validate(&pipeline, "node", 1, artifacts);
        assert!(matches!(result, Err(ValidationError::MissingOutputs(_))));
    }

    #[test]
    fn image_port_accepts_all_extensions() {
        for ext in &["png", "jpg", "jpeg", "webp", "gif"] {
            let tmp = TempDir::new().unwrap();
            let artifacts = tmp.path();
            write_image(artifacts, "node", 1, "img", &format!("photo.{ext}"));
            let pipeline = make_pipeline(vec![make_node("node", vec![image_port("img")])]);
            assert!(
                validate(&pipeline, "node", 1, artifacts).is_ok(),
                "extension .{ext} should be accepted"
            );
        }
    }

    // --- image_list port: at least one image ---

    #[test]
    fn image_list_port_with_one_image_passes() {
        let tmp = TempDir::new().unwrap();
        let artifacts = tmp.path();
        write_image(artifacts, "node", 1, "gallery", "photo.png");
        let pipeline = make_pipeline(vec![make_node("node", vec![image_list_port("gallery")])]);
        assert!(validate(&pipeline, "node", 1, artifacts).is_ok());
    }

    #[test]
    fn image_list_port_with_multiple_images_passes() {
        let tmp = TempDir::new().unwrap();
        let artifacts = tmp.path();
        write_image(artifacts, "node", 1, "gallery", "a.png");
        write_image(artifacts, "node", 1, "gallery", "b.jpg");
        write_image(artifacts, "node", 1, "gallery", "c.webp");
        let pipeline = make_pipeline(vec![make_node("node", vec![image_list_port("gallery")])]);
        assert!(validate(&pipeline, "node", 1, artifacts).is_ok());
    }

    #[test]
    fn image_list_port_with_no_images_missing() {
        let tmp = TempDir::new().unwrap();
        let artifacts = tmp.path();
        let pipeline = make_pipeline(vec![make_node("node", vec![image_list_port("gallery")])]);
        let result = validate(&pipeline, "node", 1, artifacts);
        assert!(matches!(result, Err(ValidationError::MissingOutputs(ref m)) if m == &["gallery"]));
    }
}
