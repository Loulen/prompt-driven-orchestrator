//! The bodies `pdo pipeline` sends, built as the UI builds them.

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use base64::Engine as _;
use pdo_platform::adapters::cli::daemon::read_text;
use serde_json::{json, Value};

/// The prompts of `--prompt <node>=<file>`: each node's prompt read from its
/// file.
pub(super) fn prompts_of(flags: &[String]) -> Result<BTreeMap<String, String>> {
    let mut prompts = BTreeMap::new();
    for flag in flags {
        let (node, path) = split_prompt_flag(flag)?;
        prompts.insert(node.to_string(), read_text(path)?);
    }
    Ok(prompts)
}

fn split_prompt_flag(flag: &str) -> Result<(&str, &str)> {
    flag.split_once('=')
        .filter(|(node, path)| !node.trim().is_empty() && !path.trim().is_empty())
        .map(|(node, path)| (node.trim(), path.trim()))
        .with_context(|| format!("--prompt {flag}: expected <node>=<file>"))
}

/// `PUT /pipelines/{id}`: the YAML and the prompts, as the editor saves a
/// pipeline it holds no shared draft of. The daemon freezes it as the next
/// version.
pub(super) fn save_body(yaml: String, prompts: &BTreeMap<String, String>) -> Value {
    json!({ "yaml": yaml, "prompts": prompts })
}

/// `POST /pipelines/import`: the portable document, with the skills sidecar
/// zip in base64 when there is one, as the import window sends them.
pub(super) fn import_body(document: String, sidecar: Option<&[u8]>) -> Value {
    match sidecar {
        Some(zip) => json!({
            "document": document,
            "skills_sidecar": base64::engine::general_purpose::STANDARD.encode(zip),
        }),
        None => json!({ "document": document }),
    }
}

/// `PUT /pipelines/{id}/validation`: none, these Projets only, or every
/// Projet (listed first for `first`).
pub(super) fn validation_body(none: bool, projects: Vec<String>, first: Vec<String>) -> Value {
    if none {
        json!({ "kind": "none" })
    } else if !projects.is_empty() {
        json!({ "kind": "projects", "project_ids": projects })
    } else if !first.is_empty() {
        json!({ "kind": "all", "project_ids": first })
    } else {
        json!({ "kind": "all" })
    }
}

/// `POST /library/import`: the workflow's source and its file name, which
/// only names the pipeline when the source does not.
pub(super) fn workflow_body(path: &str, content: String) -> Value {
    let filename = std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string());
    json!({ "filename": filename, "content": content })
}

/// A library entry from its file: JSON, or the same fields in YAML.
pub(super) fn library_entry(text: &str) -> Result<Value> {
    serde_yaml::from_str::<Value>(text).context("the entry is neither JSON nor YAML")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_validation_is_none_some_projets_or_all() {
        assert_eq!(
            validation_body(true, vec![], vec![]),
            json!({ "kind": "none" })
        );
        assert_eq!(
            validation_body(false, vec!["p1".into()], vec![]),
            json!({ "kind": "projects", "project_ids": ["p1"] })
        );
        assert_eq!(
            validation_body(false, vec![], vec!["p2".into()]),
            json!({ "kind": "all", "project_ids": ["p2"] })
        );
        assert_eq!(
            validation_body(false, vec![], vec![]),
            json!({ "kind": "all" })
        );
    }

    #[test]
    fn an_import_carries_its_sidecar_in_base64() {
        assert_eq!(
            import_body("name: x\n".into(), None),
            json!({ "document": "name: x\n" })
        );
        assert_eq!(
            import_body("d".into(), Some(b"zip"))["skills_sidecar"],
            "emlw"
        );
    }

    #[test]
    fn a_prompt_flag_names_its_node_and_file() {
        assert_eq!(
            split_prompt_flag("worker=w.md").unwrap(),
            ("worker", "w.md")
        );
        assert!(split_prompt_flag("worker").is_err());
        assert!(split_prompt_flag("=w.md").is_err());
    }

    #[test]
    fn a_workflow_travels_with_its_file_name() {
        assert_eq!(
            workflow_body("/tmp/flows/review.js", "x".into()),
            json!({ "filename": "review.js", "content": "x" })
        );
    }

    #[test]
    fn a_library_entry_reads_as_json_or_yaml() {
        let json_entry = library_entry(r#"{"name":"a","type":"agent"}"#).unwrap();
        let yaml_entry = library_entry("name: a\ntype: agent\n").unwrap();
        assert_eq!(json_entry, yaml_entry);
    }
}
