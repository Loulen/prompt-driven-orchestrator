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
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "lowercase")]
    pub enum Scope {
        Repo,
        User,
    }

    impl Scope {
        pub fn parse(s: &str) -> Option<Scope> {
            match s {
                "repo" => Some(Scope::Repo),
                "user" => Some(Scope::User),
                _ => None,
            }
        }
        pub fn as_str(self) -> &'static str {
            match self {
                Scope::Repo => "repo",
                Scope::User => "user",
            }
        }
    }

    pub fn user_pipelines_dir() -> Option<PathBuf> {
        std::env::var_os("HOME").map(|h| {
            PathBuf::from(h)
                .join(".maestro")
                .join("library")
                .join("pipelines")
        })
    }

    pub fn repo_pipelines_dir(repo_root: &Path) -> PathBuf {
        repo_root.join(".maestro").join("library").join("pipelines")
    }

    fn scope_dir(repo_root: &Path, scope: Scope) -> Option<PathBuf> {
        match scope {
            Scope::Repo => Some(repo_pipelines_dir(repo_root)),
            Scope::User => user_pipelines_dir(),
        }
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
        pub scope: Scope,
        pub node_count: usize,
        pub modified: Option<String>,
        pub yaml: String,
        /// Per-node prompts mirrored from `<id>.prompts/<node_id>.md`. The frontend
        /// needs these to detect divergence when only a prompt was edited — without
        /// them the star would stay "synced" after a prompt-only change.
        #[serde(default)]
        pub prompts: HashMap<String, String>,
    }

    fn read_prompts_dir(prompts_dir: &Path) -> HashMap<String, String> {
        let mut prompts = HashMap::new();
        let Ok(read_dir) = std::fs::read_dir(prompts_dir) else {
            return prompts;
        };
        for entry in read_dir.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let Some(node_id) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if let Ok(content) = std::fs::read_to_string(&path) {
                prompts.insert(node_id.to_string(), content);
            }
        }
        prompts
    }

    fn list_scope(dir: &Path, scope: Scope) -> Vec<PipelineLibraryEntry> {
        let Ok(read_dir) = std::fs::read_dir(dir) else {
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
            let prompts = read_prompts_dir(&path.with_extension("prompts"));
            entries.push(PipelineLibraryEntry {
                id,
                name: parsed.pipeline.name.clone(),
                scope,
                node_count: parsed.pipeline.nodes.len(),
                modified,
                yaml: contents,
                prompts,
            });
        }
        entries
    }

    pub fn list(repo_root: &Path) -> Vec<PipelineLibraryEntry> {
        let mut entries: Vec<PipelineLibraryEntry> = Vec::new();
        entries.extend(list_scope(&repo_pipelines_dir(repo_root), Scope::Repo));
        if let Some(user_dir) = user_pipelines_dir() {
            entries.extend(list_scope(&user_dir, Scope::User));
        }
        entries.sort_by_key(|a| a.name.to_lowercase());
        entries
    }

    fn locate(repo_root: &Path, id: &str) -> Option<(PathBuf, Scope)> {
        let repo_path = repo_pipelines_dir(repo_root).join(format!("{id}.yaml"));
        if repo_path.exists() {
            return Some((repo_path, Scope::Repo));
        }
        if let Some(user_dir) = user_pipelines_dir() {
            let user_path = user_dir.join(format!("{id}.yaml"));
            if user_path.exists() {
                return Some((user_path, Scope::User));
            }
        }
        None
    }

    pub fn get_yaml(repo_root: &Path, id: &str) -> Option<String> {
        let (path, _) = locate(repo_root, id)?;
        std::fs::read_to_string(&path).ok()
    }

    pub fn get_path(repo_root: &Path, id: &str) -> Option<PathBuf> {
        locate(repo_root, id).map(|(p, _)| p)
    }

    pub fn get_scope(repo_root: &Path, id: &str) -> Option<Scope> {
        locate(repo_root, id).map(|(_, s)| s)
    }

    /// Save a library pipeline, supporting rename-in-place.
    ///
    /// - If `id` is `Some`, the file at `<id>.yaml` is overwritten in place even if `name`
    ///   has changed — this is the rename path that prevents orphaned entries.
    /// - If `id` is `None`, a new id is derived from `name` and a fresh file is created
    ///   (with a numeric suffix on slug collision).
    /// - `scope` picks the on-disk directory. If `id` resolves to an existing file in
    ///   a *different* scope, the file is moved to the requested scope.
    pub fn save(
        repo_root: &Path,
        id: Option<&str>,
        name: &str,
        yaml: &str,
        prompts: &HashMap<String, String>,
        scope: Scope,
    ) -> Result<String, String> {
        let target_dir = scope_dir(repo_root, scope).ok_or("HOME not set")?;
        std::fs::create_dir_all(&target_dir)
            .map_err(|e| format!("failed to create library pipelines dir: {e}"))?;

        crate::pipeline::parse_pipeline(yaml).map_err(|e| format!("invalid pipeline YAML: {e}"))?;

        let final_id: String = if let Some(existing) = id {
            // Rename-in-place: preserve the existing file's stem regardless of `name`.
            // If the existing file is in a different scope, remove the old artefacts after
            // writing the new ones so the entry effectively migrates between dirs.
            let existing_id = existing.to_string();
            if let Some((old_path, old_scope)) = locate(repo_root, &existing_id) {
                if old_scope != scope {
                    let _ = std::fs::remove_file(&old_path);
                    let old_prompts = old_path.with_extension("prompts");
                    if old_prompts.exists() {
                        let _ = std::fs::remove_dir_all(&old_prompts);
                    }
                }
            }
            existing_id
        } else {
            // Fresh save: derive slug from name, with collision suffix across both scopes
            // so that starring "foo" doesn't trample a user-scope "foo" or vice-versa.
            let base = slugify(name);
            let mut candidate = base.clone();
            let mut suffix = 2u32;
            while locate(repo_root, &candidate).is_some() {
                candidate = format!("{base}-{suffix}");
                suffix += 1;
            }
            candidate
        };

        let path = target_dir.join(format!("{final_id}.yaml"));
        std::fs::write(&path, yaml).map_err(|e| format!("write error: {e}"))?;

        let prompts_dir = target_dir.join(format!("{final_id}.prompts"));
        // Drop a stale prompts dir wholesale so renamed/removed nodes don't leave files
        // behind that would spawn dead role prompts on next run.
        if prompts_dir.exists() {
            std::fs::remove_dir_all(&prompts_dir)
                .map_err(|e| format!("failed to clear prompts dir: {e}"))?;
        }
        if !prompts.is_empty() {
            std::fs::create_dir_all(&prompts_dir)
                .map_err(|e| format!("failed to create prompts dir: {e}"))?;
            for (node_id, content) in prompts {
                let prompt_path = prompts_dir.join(format!("{node_id}.md"));
                std::fs::write(&prompt_path, content)
                    .map_err(|e| format!("failed to write prompt {node_id}: {e}"))?;
            }
        }

        Ok(final_id)
    }

    pub fn delete(repo_root: &Path, id: &str) -> Result<bool, String> {
        let Some((path, _)) = locate(repo_root, id) else {
            return Ok(false);
        };
        let prompts_dir = path.with_extension("prompts");
        std::fs::remove_file(&path).map_err(|e| format!("delete error: {e}"))?;
        if prompts_dir.exists() {
            std::fs::remove_dir_all(&prompts_dir)
                .map_err(|e| format!("delete prompts error: {e}"))?;
        }
        Ok(true)
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

    /// Like `with_temp_home` but also provides an isolated repo root for the
    /// repo-scoped library tests. The repo root lives under the temp HOME.
    fn with_temp_repo<F: FnOnce(&std::path::Path)>(f: F) {
        with_temp_home(|| {
            let repo = std::env::var("HOME")
                .map(PathBuf::from)
                .unwrap()
                .join("repo");
            std::fs::create_dir_all(&repo).unwrap();
            f(&repo);
        });
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
                port_type: pipeline::PortType::Markdown,
                frontmatter: None,
                when: None,
                description: None,
            }],
            outputs: vec![pipeline::Port {
                name: "out".to_string(),
                repeated: false,
                side: Some(pipeline::PortSide::Right),
                port_type: pipeline::PortType::Markdown,
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
                    port_type: pipeline::PortType::Markdown,
                    frontmatter: None,
                    when: None,
                    description: None,
                },
                pipeline::Port {
                    name: "reviews".to_string(),
                    repeated: true,
                    side: Some(pipeline::PortSide::Top),
                    port_type: pipeline::PortType::Markdown,
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
                port_type: pipeline::PortType::Markdown,
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
        with_temp_repo(|repo| {
            let yaml = sample_pipeline_yaml("Review Pipeline");
            let id = pipelines::save(
                repo,
                None,
                "Review Pipeline",
                &yaml,
                &HashMap::new(),
                pipelines::Scope::Repo,
            )
            .unwrap();
            assert_eq!(id, "review-pipeline");

            let all = pipelines::list(repo);
            assert_eq!(all.len(), 1);
            assert_eq!(all[0].id, "review-pipeline");
            assert_eq!(all[0].name, "Review Pipeline");
            assert_eq!(all[0].scope, pipelines::Scope::Repo);
            assert_eq!(all[0].node_count, 3);
            assert_eq!(all[0].yaml, yaml);

            let got = pipelines::get_yaml(repo, "review-pipeline").unwrap();
            assert_eq!(got, yaml);

            let deleted = pipelines::delete(repo, "review-pipeline").unwrap();
            assert!(deleted);

            assert!(pipelines::get_yaml(repo, "review-pipeline").is_none());
            assert!(pipelines::list(repo).is_empty());
        });
    }

    #[test]
    fn pipeline_library_delete_nonexistent() {
        with_temp_repo(|repo| {
            let result = pipelines::delete(repo, "ghost").unwrap();
            assert!(!result);
        });
    }

    #[test]
    fn pipeline_library_save_invalid_yaml_errors() {
        with_temp_repo(|repo| {
            let result = pipelines::save(
                repo,
                None,
                "Bad",
                "not: valid: yaml: [[[",
                &HashMap::new(),
                pipelines::Scope::Repo,
            );
            assert!(result.is_err());
        });
    }

    #[test]
    fn pipeline_library_overwrite_by_id() {
        with_temp_repo(|repo| {
            let yaml1 = sample_pipeline_yaml("My Pipeline");
            let yaml2 = sample_pipeline_yaml("My Pipeline v2");
            let id = pipelines::save(
                repo,
                None,
                "My Pipeline",
                &yaml1,
                &HashMap::new(),
                pipelines::Scope::Repo,
            )
            .unwrap();
            // Second save with the same id must rewrite the same file in place,
            // not create a duplicate or migrate to a different slug.
            pipelines::save(
                repo,
                Some(&id),
                "My Pipeline v2",
                &yaml2,
                &HashMap::new(),
                pipelines::Scope::Repo,
            )
            .unwrap();

            let got = pipelines::get_yaml(repo, &id).unwrap();
            assert!(got.contains("My Pipeline v2"));
            assert_eq!(pipelines::list(repo).len(), 1);
            // id remains stable across rename.
            assert_eq!(pipelines::list(repo)[0].id, id);
            assert_eq!(pipelines::list(repo)[0].name, "My Pipeline v2");
        });
    }

    #[test]
    fn pipeline_library_save_without_id_avoids_collisions() {
        with_temp_repo(|repo| {
            let yaml = sample_pipeline_yaml("My Pipeline");
            let id1 = pipelines::save(
                repo,
                None,
                "My Pipeline",
                &yaml,
                &HashMap::new(),
                pipelines::Scope::Repo,
            )
            .unwrap();
            let id2 = pipelines::save(
                repo,
                None,
                "My Pipeline",
                &yaml,
                &HashMap::new(),
                pipelines::Scope::Repo,
            )
            .unwrap();
            // No id given + slug taken → numeric suffix, not silent overwrite.
            assert_eq!(id1, "my-pipeline");
            assert_eq!(id2, "my-pipeline-2");
            assert_eq!(pipelines::list(repo).len(), 2);
        });
    }

    #[test]
    fn pipeline_library_sorted_alphabetically() {
        with_temp_repo(|repo| {
            for name in ["Zebra Pipeline", "Alpha Pipeline", "middle pipeline"] {
                let yaml = sample_pipeline_yaml(name);
                pipelines::save(
                    repo,
                    None,
                    name,
                    &yaml,
                    &HashMap::new(),
                    pipelines::Scope::Repo,
                )
                .unwrap();
            }

            let names: Vec<String> = pipelines::list(repo).into_iter().map(|e| e.name).collect();
            assert_eq!(
                names,
                vec!["Alpha Pipeline", "middle pipeline", "Zebra Pipeline"]
            );
        });
    }

    #[test]
    fn pipeline_library_does_not_affect_node_library() {
        with_temp_repo(|repo| {
            let node = make_node("Worker");
            let entry = entry_from_node(&node, "node prompt");
            save(&entry).unwrap();

            let yaml = sample_pipeline_yaml("My Pipeline");
            pipelines::save(
                repo,
                None,
                "My Pipeline",
                &yaml,
                &HashMap::new(),
                pipelines::Scope::Repo,
            )
            .unwrap();

            assert_eq!(list().len(), 1);
            assert_eq!(list()[0].name, "Worker");
            assert_eq!(pipelines::list(repo).len(), 1);
            assert_eq!(pipelines::list(repo)[0].name, "My Pipeline");
        });
    }

    #[test]
    fn pipeline_library_scope_split() {
        with_temp_repo(|repo| {
            let yaml = sample_pipeline_yaml("My Pipeline");
            pipelines::save(
                repo,
                None,
                "My Pipeline",
                &yaml,
                &HashMap::new(),
                pipelines::Scope::Repo,
            )
            .unwrap();
            pipelines::save(
                repo,
                None,
                "Other Pipeline",
                &sample_pipeline_yaml("Other Pipeline"),
                &HashMap::new(),
                pipelines::Scope::User,
            )
            .unwrap();

            let all = pipelines::list(repo);
            assert_eq!(all.len(), 2);
            let repo_entry = all.iter().find(|e| e.name == "My Pipeline").unwrap();
            assert_eq!(repo_entry.scope, pipelines::Scope::Repo);
            let user_entry = all.iter().find(|e| e.name == "Other Pipeline").unwrap();
            assert_eq!(user_entry.scope, pipelines::Scope::User);

            // Files live in distinct on-disk locations.
            assert!(pipelines::repo_pipelines_dir(repo)
                .join("my-pipeline.yaml")
                .exists());
            assert!(pipelines::user_pipelines_dir()
                .unwrap()
                .join("other-pipeline.yaml")
                .exists());
        });
    }

    #[test]
    fn pipeline_library_save_migrates_scope_when_id_known() {
        with_temp_repo(|repo| {
            let yaml = sample_pipeline_yaml("Mover");
            let id = pipelines::save(
                repo,
                None,
                "Mover",
                &yaml,
                &HashMap::new(),
                pipelines::Scope::Repo,
            )
            .unwrap();
            // Switch scope while keeping the same id: file moves repo → user.
            let new_id = pipelines::save(
                repo,
                Some(&id),
                "Mover",
                &yaml,
                &HashMap::new(),
                pipelines::Scope::User,
            )
            .unwrap();
            assert_eq!(new_id, id);
            assert!(!pipelines::repo_pipelines_dir(repo)
                .join(format!("{id}.yaml"))
                .exists());
            assert!(pipelines::user_pipelines_dir()
                .unwrap()
                .join(format!("{id}.yaml"))
                .exists());
            let all = pipelines::list(repo);
            assert_eq!(all.len(), 1);
            assert_eq!(all[0].scope, pipelines::Scope::User);
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
                    port_type: pipeline::PortType::Markdown,
                    frontmatter: None,
                    when: None,
                    description: None,
                },
                pipeline::Port {
                    name: "reviews".to_string(),
                    repeated: true,
                    side: Some(pipeline::PortSide::Top),
                    port_type: pipeline::PortType::Markdown,
                    frontmatter: None,
                    when: None,
                    description: None,
                },
            ],
            outputs: vec![pipeline::Port {
                name: "review".to_string(),
                repeated: false,
                side: Some(pipeline::PortSide::Right),
                port_type: pipeline::PortType::Markdown,
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
                    port_type: pipeline::PortType::Markdown,
                    frontmatter: None,
                    when: None,
                    description: None,
                }],
                outputs: vec![pipeline::Port {
                    name: "result".to_string(),
                    repeated: false,
                    side: Some(pipeline::PortSide::Right),
                    port_type: pipeline::PortType::Markdown,
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
        with_temp_repo(|repo| {
            let yaml = sample_pipeline_yaml("Test Path");
            pipelines::save(
                repo,
                None,
                "Test Path",
                &yaml,
                &HashMap::new(),
                pipelines::Scope::Repo,
            )
            .unwrap();

            let path = pipelines::get_path(repo, "test-path").unwrap();
            assert!(path.exists());
            assert!(path
                .to_str()
                .unwrap()
                .contains("library/pipelines/test-path.yaml"));

            assert!(pipelines::get_path(repo, "nonexistent").is_none());
        });
    }

    #[test]
    fn pipeline_library_save_writes_prompts_dir() {
        with_temp_repo(|repo| {
            let yaml = sample_pipeline_yaml("Promptful");
            let mut prompts = HashMap::new();
            prompts.insert("planner".to_string(), "You are a planner.".to_string());
            prompts.insert("end".to_string(), "Wrap up.".to_string());

            let id = pipelines::save(
                repo,
                None,
                "Promptful",
                &yaml,
                &prompts,
                pipelines::Scope::Repo,
            )
            .unwrap();
            assert_eq!(id, "promptful");

            let prompts_dir = pipelines::repo_pipelines_dir(repo).join("promptful.prompts");
            assert!(prompts_dir.is_dir());
            assert_eq!(
                std::fs::read_to_string(prompts_dir.join("planner.md")).unwrap(),
                "You are a planner."
            );
            assert_eq!(
                std::fs::read_to_string(prompts_dir.join("end.md")).unwrap(),
                "Wrap up."
            );
        });
    }

    #[test]
    fn pipeline_library_save_replaces_prompts_dir() {
        with_temp_repo(|repo| {
            let yaml = sample_pipeline_yaml("Promptful");
            let mut p1 = HashMap::new();
            p1.insert("planner".to_string(), "v1".to_string());
            p1.insert("ghost".to_string(), "removed-soon".to_string());
            let id = pipelines::save(repo, None, "Promptful", &yaml, &p1, pipelines::Scope::Repo)
                .unwrap();

            let mut p2 = HashMap::new();
            p2.insert("planner".to_string(), "v2".to_string());
            pipelines::save(
                repo,
                Some(&id),
                "Promptful",
                &yaml,
                &p2,
                pipelines::Scope::Repo,
            )
            .unwrap();

            let prompts_dir = pipelines::repo_pipelines_dir(repo).join("promptful.prompts");
            assert_eq!(
                std::fs::read_to_string(prompts_dir.join("planner.md")).unwrap(),
                "v2"
            );
            // The node removed from the second save must not linger on disk; otherwise it
            // would still be materialised into future run worktrees as a dead prompt file.
            assert!(!prompts_dir.join("ghost.md").exists());
        });
    }

    #[test]
    fn pipeline_library_list_returns_prompts() {
        with_temp_repo(|repo| {
            let yaml = sample_pipeline_yaml("Promptful");
            let mut prompts = HashMap::new();
            prompts.insert("planner".to_string(), "You are a planner.".to_string());
            prompts.insert("end".to_string(), "Wrap up.".to_string());

            pipelines::save(
                repo,
                None,
                "Promptful",
                &yaml,
                &prompts,
                pipelines::Scope::Repo,
            )
            .unwrap();

            let all = pipelines::list(repo);
            assert_eq!(all.len(), 1);
            // Without these, the frontend cannot detect prompt-only divergence and
            // the star stays "synced" after the user edits a node prompt.
            assert_eq!(all[0].prompts.len(), 2);
            assert_eq!(
                all[0].prompts.get("planner").map(String::as_str),
                Some("You are a planner."),
            );
            assert_eq!(
                all[0].prompts.get("end").map(String::as_str),
                Some("Wrap up.")
            );
        });
    }

    #[test]
    fn pipeline_library_list_returns_empty_prompts_when_dir_missing() {
        with_temp_repo(|repo| {
            let yaml = sample_pipeline_yaml("Bare");
            pipelines::save(
                repo,
                None,
                "Bare",
                &yaml,
                &HashMap::new(),
                pipelines::Scope::Repo,
            )
            .unwrap();

            let all = pipelines::list(repo);
            assert_eq!(all.len(), 1);
            assert!(all[0].prompts.is_empty());
        });
    }

    #[test]
    fn pipeline_library_delete_removes_prompts_dir() {
        with_temp_repo(|repo| {
            let yaml = sample_pipeline_yaml("Promptful");
            let mut prompts = HashMap::new();
            prompts.insert("planner".to_string(), "p".to_string());
            pipelines::save(
                repo,
                None,
                "Promptful",
                &yaml,
                &prompts,
                pipelines::Scope::Repo,
            )
            .unwrap();

            let dir = pipelines::repo_pipelines_dir(repo);
            assert!(dir.join("promptful.prompts").is_dir());

            assert!(pipelines::delete(repo, "promptful").unwrap());
            assert!(!dir.join("promptful.yaml").exists());
            assert!(!dir.join("promptful.prompts").exists());
        });
    }
}
