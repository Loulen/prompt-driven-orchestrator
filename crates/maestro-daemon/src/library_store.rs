use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::pipeline;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LibraryEntry {
    pub name: String,
    #[serde(rename = "type")]
    pub node_type: pipeline::NodeType,
    #[serde(default)]
    pub inputs: Vec<pipeline::Port>,
    #[serde(default)]
    pub outputs: Vec<pipeline::Port>,
    #[serde(default)]
    pub interactive: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_iter: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branches: Option<u32>,
    pub prompt: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SyncState {
    Outline,
    Synced,
    Diverged,
}

fn library_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".maestro").join("library"))
}

fn slugify(name: &str) -> String {
    let mut slug = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            slug.push(ch.to_ascii_lowercase());
        } else if ch == ' ' {
            slug.push('-');
        }
    }
    if slug.is_empty() {
        slug.push_str("node");
    }
    slug
}

fn resolve_slug(dir: &Path, name: &str) -> String {
    let base = slugify(name);
    if !dir.join(format!("{base}.yaml")).exists() {
        return base;
    }
    let mut suffix = 2u32;
    loop {
        let candidate = format!("{base}-{suffix}");
        if !dir.join(format!("{candidate}.yaml")).exists() {
            return candidate;
        }
        suffix += 1;
    }
}

pub fn list() -> Vec<LibraryEntry> {
    let Some(dir) = library_dir() else {
        return Vec::new();
    };
    let Ok(read_dir) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut entries: Vec<LibraryEntry> = Vec::new();
    for entry in read_dir.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("yaml") {
            continue;
        }
        if let Ok(contents) = std::fs::read_to_string(&path) {
            if let Ok(entry) = serde_yaml::from_str::<LibraryEntry>(&contents) {
                entries.push(entry);
            }
        }
    }
    entries.sort_by_key(|a| a.name.to_lowercase());
    entries
}

pub fn get(name: &str) -> Option<LibraryEntry> {
    let dir = library_dir()?;
    let slug = slugify(name);
    let path = dir.join(format!("{slug}.yaml"));
    let contents = std::fs::read_to_string(&path).ok()?;
    let entry: LibraryEntry = serde_yaml::from_str(&contents).ok()?;
    if entry.name == name {
        Some(entry)
    } else {
        find_by_name(&dir, name)
    }
}

fn find_by_name(dir: &Path, name: &str) -> Option<LibraryEntry> {
    let read_dir = std::fs::read_dir(dir).ok()?;
    for entry in read_dir.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("yaml") {
            continue;
        }
        if let Ok(contents) = std::fs::read_to_string(&path) {
            if let Ok(lib_entry) = serde_yaml::from_str::<LibraryEntry>(&contents) {
                if lib_entry.name == name {
                    return Some(lib_entry);
                }
            }
        }
    }
    None
}

pub fn save(entry: &LibraryEntry) -> Result<(), String> {
    let dir = library_dir().ok_or("HOME not set")?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("failed to create library dir: {e}"))?;

    let existing_path = find_path_by_name(&dir, &entry.name);
    let path = existing_path.unwrap_or_else(|| {
        let slug = resolve_slug(&dir, &entry.name);
        dir.join(format!("{slug}.yaml"))
    });

    let yaml = serde_yaml::to_string(entry).map_err(|e| format!("serialization error: {e}"))?;
    std::fs::write(&path, yaml).map_err(|e| format!("write error: {e}"))?;
    Ok(())
}

pub fn delete(name: &str) -> Result<bool, String> {
    let dir = library_dir().ok_or("HOME not set")?;
    if let Some(path) = find_path_by_name(&dir, name) {
        std::fs::remove_file(&path).map_err(|e| format!("delete error: {e}"))?;
        Ok(true)
    } else {
        Ok(false)
    }
}

fn find_path_by_name(dir: &Path, name: &str) -> Option<PathBuf> {
    let slug = slugify(name);
    let direct = dir.join(format!("{slug}.yaml"));
    if direct.exists() {
        if let Ok(contents) = std::fs::read_to_string(&direct) {
            if let Ok(entry) = serde_yaml::from_str::<LibraryEntry>(&contents) {
                if entry.name == name {
                    return Some(direct);
                }
            }
        }
    }
    let read_dir = std::fs::read_dir(dir).ok()?;
    for entry in read_dir.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("yaml") {
            continue;
        }
        if let Ok(contents) = std::fs::read_to_string(&path) {
            if let Ok(lib_entry) = serde_yaml::from_str::<LibraryEntry>(&contents) {
                if lib_entry.name == name {
                    return Some(path);
                }
            }
        }
    }
    None
}

pub fn entry_from_node(node: &pipeline::NodeDef, prompt: &str) -> LibraryEntry {
    LibraryEntry {
        name: node.name.clone(),
        node_type: node.node_type.clone(),
        inputs: node.inputs.clone(),
        outputs: node.outputs.clone(),
        interactive: node.interactive,
        max_iter: None,
        branches: None,
        prompt: prompt.to_string(),
    }
}

pub fn sync_state(node: &pipeline::NodeDef, prompt: &str) -> SyncState {
    let Some(entry) = get(&node.name) else {
        return SyncState::Outline;
    };
    let candidate = entry_from_node(node, prompt);
    if candidate == entry {
        SyncState::Synced
    } else {
        SyncState::Diverged
    }
}

pub mod pipelines {
    use std::path::PathBuf;

    use serde::{Deserialize, Serialize};

    fn pipelines_dir() -> Option<PathBuf> {
        std::env::var_os("HOME").map(|h| {
            PathBuf::from(h)
                .join(".maestro")
                .join("library")
                .join("pipelines")
        })
    }

    fn slugify(name: &str) -> String {
        let mut slug = String::new();
        for ch in name.chars() {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                slug.push(ch.to_ascii_lowercase());
            } else if ch == ' ' {
                slug.push('-');
            }
        }
        if slug.is_empty() {
            slug.push_str("pipeline");
        }
        slug
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct PipelineLibraryEntry {
        pub id: String,
        pub name: String,
        pub node_count: usize,
        pub modified: Option<String>,
        pub yaml: String,
    }

    pub fn list() -> Vec<PipelineLibraryEntry> {
        let Some(dir) = pipelines_dir() else {
            return Vec::new();
        };
        let Ok(read_dir) = std::fs::read_dir(&dir) else {
            return Vec::new();
        };
        let mut entries = Vec::new();
        for entry in read_dir.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("yaml") {
                continue;
            }
            let id = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            let Ok(contents) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Ok(parsed) = crate::pipeline::parse_pipeline(&contents) else {
                continue;
            };
            let modified = std::fs::metadata(&path)
                .and_then(|m| m.modified())
                .ok()
                .map(|t| {
                    chrono::DateTime::<chrono::Utc>::from(t)
                        .format("%Y-%m-%dT%H:%M:%SZ")
                        .to_string()
                });
            entries.push(PipelineLibraryEntry {
                id,
                name: parsed.pipeline.name.clone(),
                node_count: parsed.pipeline.nodes.len(),
                modified,
                yaml: contents,
            });
        }
        entries.sort_by_key(|a| a.name.to_lowercase());
        entries
    }

    pub fn get_yaml(id: &str) -> Option<String> {
        let dir = pipelines_dir()?;
        let path = dir.join(format!("{id}.yaml"));
        std::fs::read_to_string(&path).ok()
    }

    pub fn get_path(id: &str) -> Option<PathBuf> {
        let dir = pipelines_dir()?;
        let path = dir.join(format!("{id}.yaml"));
        if path.exists() {
            Some(path)
        } else {
            None
        }
    }

    pub fn save(name: &str, yaml: &str) -> Result<String, String> {
        let dir = pipelines_dir().ok_or("HOME not set")?;
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("failed to create library pipelines dir: {e}"))?;

        crate::pipeline::parse_pipeline(yaml).map_err(|e| format!("invalid pipeline YAML: {e}"))?;

        let id = slugify(name);
        let path = dir.join(format!("{id}.yaml"));
        std::fs::write(&path, yaml).map_err(|e| format!("write error: {e}"))?;
        Ok(id)
    }

    pub fn delete(id: &str) -> Result<bool, String> {
        let dir = pipelines_dir().ok_or("HOME not set")?;
        let path = dir.join(format!("{id}.yaml"));
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| format!("delete error: {e}"))?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

#[cfg(test)]
pub(crate) static HOME_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    use super::HOME_TEST_LOCK as TEST_LOCK;

    fn with_temp_home<F: FnOnce()>(f: F) {
        let _guard = TEST_LOCK.lock().unwrap();
        let tmp = std::env::temp_dir().join(format!("maestro-lib-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let prev = std::env::var("HOME").ok();
        std::env::set_var("HOME", &tmp);
        f();
        if let Some(p) = prev {
            std::env::set_var("HOME", p);
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    fn make_node(name: &str) -> pipeline::NodeDef {
        pipeline::NodeDef {
            id: "test-id".to_string(),
            name: name.to_string(),
            node_type: pipeline::NodeType::DocOnly,
            inputs: vec![pipeline::Port {
                name: "in".to_string(),
                repeated: false,
                side: Some(pipeline::PortSide::Left),
                frontmatter: None,
                when: None,
                description: None,
            }],
            outputs: vec![pipeline::Port {
                name: "out".to_string(),
                repeated: false,
                side: Some(pipeline::PortSide::Right),
                frontmatter: None,
                when: None,
                description: None,
            }],
            interactive: false,
            view: None,
            max_iter: None,
            over: None,
        }
    }

    #[test]
    fn slugify_basic() {
        assert_eq!(slugify("My Node"), "my-node");
        assert_eq!(slugify("A_B-C"), "a_b-c");
        assert_eq!(slugify("!!!"), "node");
        assert_eq!(slugify("Review Bot 2"), "review-bot-2");
    }

    #[test]
    fn crud_round_trip() {
        with_temp_home(|| {
            let node = make_node("Reviewer");
            let entry = entry_from_node(&node, "You are a reviewer.");
            save(&entry).unwrap();

            let got = get("Reviewer").unwrap();
            assert_eq!(got, entry);

            let all = list();
            assert_eq!(all.len(), 1);
            assert_eq!(all[0].name, "Reviewer");

            let deleted = delete("Reviewer").unwrap();
            assert!(deleted);

            assert!(get("Reviewer").is_none());
            assert!(list().is_empty());
        });
    }

    #[test]
    fn delete_nonexistent_returns_false() {
        with_temp_home(|| {
            let result = delete("ghost").unwrap();
            assert!(!result);
        });
    }

    #[test]
    fn slug_collision_suffix() {
        with_temp_home(|| {
            let dir = library_dir().unwrap();
            std::fs::create_dir_all(&dir).unwrap();

            let entry1 = LibraryEntry {
                name: "Alpha".to_string(),
                node_type: pipeline::NodeType::DocOnly,
                inputs: vec![],
                outputs: vec![],
                interactive: false,
                max_iter: None,
                branches: None,
                prompt: "first".to_string(),
            };
            save(&entry1).unwrap();
            assert!(dir.join("alpha.yaml").exists());

            // Write a different file at the slug path to force collision
            std::fs::write(
                dir.join("beta.yaml"),
                serde_yaml::to_string(&LibraryEntry {
                    name: "Beta Original".to_string(),
                    ..entry1.clone()
                })
                .unwrap(),
            )
            .unwrap();

            let entry2 = LibraryEntry {
                name: "Beta".to_string(),
                ..entry1
            };
            save(&entry2).unwrap();
            // beta.yaml is taken by "Beta Original", so "Beta" goes to beta-2.yaml
            assert!(dir.join("beta-2.yaml").exists());
        });
    }

    #[test]
    fn sync_state_outline_when_missing() {
        with_temp_home(|| {
            let node = make_node("Ghost");
            assert_eq!(sync_state(&node, "prompt"), SyncState::Outline);
        });
    }

    #[test]
    fn sync_state_synced_when_equal() {
        with_temp_home(|| {
            let node = make_node("Reviewer");
            let entry = entry_from_node(&node, "You review code.");
            save(&entry).unwrap();

            assert_eq!(sync_state(&node, "You review code."), SyncState::Synced);
        });
    }

    #[test]
    fn sync_state_diverged_when_different() {
        with_temp_home(|| {
            let node = make_node("Reviewer");
            let entry = entry_from_node(&node, "Original prompt.");
            save(&entry).unwrap();

            assert_eq!(sync_state(&node, "Changed prompt."), SyncState::Diverged);
        });
    }

    #[test]
    fn yaml_round_trip_lossless() {
        let entry = LibraryEntry {
            name: "Complex Node".to_string(),
            node_type: pipeline::NodeType::CodeMutating,
            inputs: vec![
                pipeline::Port {
                    name: "plan".to_string(),
                    repeated: false,
                    side: Some(pipeline::PortSide::Left),
                    frontmatter: None,
                    when: None,
                    description: None,
                },
                pipeline::Port {
                    name: "reviews".to_string(),
                    repeated: true,
                    side: Some(pipeline::PortSide::Top),
                    frontmatter: None,
                    when: None,
                    description: None,
                },
            ],
            outputs: vec![pipeline::Port {
                name: "summary".to_string(),
                repeated: false,
                side: Some(pipeline::PortSide::Right),
                when: None,
                frontmatter: Some({
                    let mut m = HashMap::new();
                    m.insert(
                        "verdict".to_string(),
                        pipeline::FrontmatterFieldDecl {
                            field_type: "enum".to_string(),
                            allowed: Some(vec!["PASS".to_string(), "FAIL".to_string()]),
                        },
                    );
                    m
                }),
                description: None,
            }],
            interactive: true,
            max_iter: Some(5),
            branches: Some(3),
            prompt: "You are an implementer.\nMulti-line prompt.".to_string(),
        };

        let yaml = serde_yaml::to_string(&entry).unwrap();
        let parsed: LibraryEntry = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed, entry);
    }

    #[test]
    fn list_is_sorted_alphabetically() {
        with_temp_home(|| {
            let base = LibraryEntry {
                name: String::new(),
                node_type: pipeline::NodeType::DocOnly,
                inputs: vec![],
                outputs: vec![],
                interactive: false,
                max_iter: None,
                branches: None,
                prompt: "p".to_string(),
            };

            for name in ["Zebra", "Alpha", "middle"] {
                let mut e = base.clone();
                e.name = name.to_string();
                save(&e).unwrap();
            }

            let names: Vec<String> = list().into_iter().map(|e| e.name).collect();
            assert_eq!(names, vec!["Alpha", "middle", "Zebra"]);
        });
    }

    #[test]
    fn save_overwrites_existing_entry() {
        with_temp_home(|| {
            let node = make_node("Worker");
            let entry1 = entry_from_node(&node, "version 1");
            save(&entry1).unwrap();
            assert_eq!(get("Worker").unwrap().prompt, "version 1");

            let entry2 = entry_from_node(&node, "version 2");
            save(&entry2).unwrap();
            assert_eq!(get("Worker").unwrap().prompt, "version 2");
            assert_eq!(list().len(), 1);
        });
    }

    fn sample_pipeline_yaml(name: &str) -> String {
        format!(
            "name: {name}\nnodes:\n  - id: start\n    name: Start\n    type: start\n    outputs:\n      - name: user_prompt\n  - id: planner\n    name: Planner\n    type: doc-only\n    inputs:\n      - name: in\n    outputs:\n      - name: plan\n  - id: end\n    name: End\n    type: end\n    inputs:\n      - name: result\nedges:\n  - source: {{ node: start, port: user_prompt }}\n    target: {{ node: planner, port: in }}\n  - source: {{ node: planner, port: plan }}\n    target: {{ node: end, port: result }}\n"
        )
    }

    #[test]
    fn pipeline_library_crud_round_trip() {
        with_temp_home(|| {
            let yaml = sample_pipeline_yaml("Review Pipeline");
            let id = pipelines::save("Review Pipeline", &yaml).unwrap();
            assert_eq!(id, "review-pipeline");

            let all = pipelines::list();
            assert_eq!(all.len(), 1);
            assert_eq!(all[0].id, "review-pipeline");
            assert_eq!(all[0].name, "Review Pipeline");
            assert_eq!(all[0].node_count, 3);
            assert_eq!(all[0].yaml, yaml);

            let got = pipelines::get_yaml("review-pipeline").unwrap();
            assert_eq!(got, yaml);

            let deleted = pipelines::delete("review-pipeline").unwrap();
            assert!(deleted);

            assert!(pipelines::get_yaml("review-pipeline").is_none());
            assert!(pipelines::list().is_empty());
        });
    }

    #[test]
    fn pipeline_library_delete_nonexistent() {
        with_temp_home(|| {
            let result = pipelines::delete("ghost").unwrap();
            assert!(!result);
        });
    }

    #[test]
    fn pipeline_library_save_invalid_yaml_errors() {
        with_temp_home(|| {
            let result = pipelines::save("Bad", "not: valid: yaml: [[[");
            assert!(result.is_err());
        });
    }

    #[test]
    fn pipeline_library_overwrite() {
        with_temp_home(|| {
            let yaml1 = sample_pipeline_yaml("My Pipeline");
            let yaml2 = sample_pipeline_yaml("My Pipeline v2");
            pipelines::save("My Pipeline", &yaml1).unwrap();
            pipelines::save("My Pipeline", &yaml2).unwrap();

            let got = pipelines::get_yaml("my-pipeline").unwrap();
            assert!(got.contains("My Pipeline v2"));
            assert_eq!(pipelines::list().len(), 1);
        });
    }

    #[test]
    fn pipeline_library_sorted_alphabetically() {
        with_temp_home(|| {
            for name in ["Zebra Pipeline", "Alpha Pipeline", "middle pipeline"] {
                let yaml = sample_pipeline_yaml(name);
                pipelines::save(name, &yaml).unwrap();
            }

            let names: Vec<String> = pipelines::list().into_iter().map(|e| e.name).collect();
            assert_eq!(
                names,
                vec!["Alpha Pipeline", "middle pipeline", "Zebra Pipeline"]
            );
        });
    }

    #[test]
    fn pipeline_library_does_not_affect_node_library() {
        with_temp_home(|| {
            let node = make_node("Worker");
            let entry = entry_from_node(&node, "node prompt");
            save(&entry).unwrap();

            let yaml = sample_pipeline_yaml("My Pipeline");
            pipelines::save("My Pipeline", &yaml).unwrap();

            assert_eq!(list().len(), 1);
            assert_eq!(list()[0].name, "Worker");
            assert_eq!(pipelines::list().len(), 1);
            assert_eq!(pipelines::list()[0].name, "My Pipeline");
        });
    }

    #[test]
    fn json_round_trip_preserves_all_port_fields() {
        let entry = LibraryEntry {
            name: "Typed Node".to_string(),
            node_type: pipeline::NodeType::DocOnly,
            inputs: vec![
                pipeline::Port {
                    name: "task".to_string(),
                    repeated: false,
                    side: Some(pipeline::PortSide::Left),
                    frontmatter: None,
                    when: None,
                    description: None,
                },
                pipeline::Port {
                    name: "reviews".to_string(),
                    repeated: true,
                    side: Some(pipeline::PortSide::Top),
                    frontmatter: None,
                    when: None,
                    description: None,
                },
            ],
            outputs: vec![pipeline::Port {
                name: "review".to_string(),
                repeated: false,
                side: Some(pipeline::PortSide::Right),
                frontmatter: Some({
                    let mut m = HashMap::new();
                    m.insert(
                        "verdict".to_string(),
                        pipeline::FrontmatterFieldDecl {
                            field_type: "enum".to_string(),
                            allowed: Some(vec!["PASS".to_string(), "FAIL".to_string()]),
                        },
                    );
                    m.insert(
                        "score".to_string(),
                        pipeline::FrontmatterFieldDecl {
                            field_type: "int".to_string(),
                            allowed: None,
                        },
                    );
                    m
                }),
                when: Some(serde_yaml::Value::Mapping({
                    let mut m = serde_yaml::Mapping::new();
                    m.insert(
                        serde_yaml::Value::String("verdict".to_string()),
                        serde_yaml::Value::Mapping({
                            let mut inner = serde_yaml::Mapping::new();
                            inner.insert(
                                serde_yaml::Value::String("eq".to_string()),
                                serde_yaml::Value::String("PASS".to_string()),
                            );
                            inner
                        }),
                    );
                    m
                })),
                description: None,
            }],
            interactive: true,
            max_iter: Some(5),
            branches: None,
            prompt: "You are a reviewer.".to_string(),
        };

        let json = serde_json::to_string(&entry).unwrap();
        let parsed: LibraryEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, entry);

        let output = &parsed.outputs[0];
        assert!(output.frontmatter.is_some());
        let fm = output.frontmatter.as_ref().unwrap();
        assert_eq!(fm["verdict"].field_type, "enum");
        assert_eq!(
            fm["verdict"].allowed,
            Some(vec!["PASS".to_string(), "FAIL".to_string()])
        );
        assert_eq!(fm["score"].field_type, "int");

        assert!(output.when.is_some());
    }

    #[test]
    fn json_round_trip_via_disk_preserves_port_fields() {
        with_temp_home(|| {
            let entry = LibraryEntry {
                name: "Schema Node".to_string(),
                node_type: pipeline::NodeType::CodeMutating,
                inputs: vec![pipeline::Port {
                    name: "in".to_string(),
                    repeated: false,
                    side: Some(pipeline::PortSide::Left),
                    frontmatter: None,
                    when: None,
                    description: None,
                }],
                outputs: vec![pipeline::Port {
                    name: "result".to_string(),
                    repeated: false,
                    side: Some(pipeline::PortSide::Right),
                    frontmatter: Some({
                        let mut m = HashMap::new();
                        m.insert(
                            "status".to_string(),
                            pipeline::FrontmatterFieldDecl {
                                field_type: "enum".to_string(),
                                allowed: Some(vec!["OK".to_string(), "ERROR".to_string()]),
                            },
                        );
                        m
                    }),
                    when: None,
                    description: None,
                }],
                interactive: false,
                max_iter: None,
                branches: None,
                prompt: "Implement changes.".to_string(),
            };

            save(&entry).unwrap();

            let loaded = get("Schema Node").unwrap();
            assert_eq!(loaded, entry);

            let fm = loaded.outputs[0].frontmatter.as_ref().unwrap();
            assert_eq!(fm["status"].field_type, "enum");
            assert_eq!(
                fm["status"].allowed,
                Some(vec!["OK".to_string(), "ERROR".to_string()])
            );
        });
    }

    #[test]
    fn pipeline_library_get_path() {
        with_temp_home(|| {
            let yaml = sample_pipeline_yaml("Test Path");
            pipelines::save("Test Path", &yaml).unwrap();

            let path = pipelines::get_path("test-path").unwrap();
            assert!(path.exists());
            assert!(path
                .to_str()
                .unwrap()
                .contains("library/pipelines/test-path.yaml"));

            assert!(pipelines::get_path("nonexistent").is_none());
        });
    }
}
