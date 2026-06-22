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
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".pdo").join("library"))
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
    use sha2::{Digest, Sha256};

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "lowercase")]
    pub enum Scope {
        Repo,
        User,
        Library,
    }

    impl Scope {
        pub fn parse(s: &str) -> Option<Scope> {
            match s {
                "repo" => Some(Scope::Repo),
                "user" => Some(Scope::User),
                "library" => Some(Scope::Library),
                _ => None,
            }
        }
        pub fn as_str(self) -> &'static str {
            match self {
                Scope::Repo => "repo",
                Scope::User => "user",
                Scope::Library => "library",
            }
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct PromotedFrom {
        pub repo: String,
        pub path: String,
        pub content_hash: String,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, Default)]
    pub struct PipelineMeta {
        #[serde(skip_serializing_if = "Option::is_none")]
        pub promoted_from: Option<PromotedFrom>,
    }

    pub fn content_hash(yaml: &str, prompts: &HashMap<String, String>) -> String {
        let mut hasher = Sha256::new();
        hasher.update(yaml.as_bytes());
        let mut keys: Vec<&String> = prompts.keys().collect();
        keys.sort();
        for key in keys {
            hasher.update(key.as_bytes());
            hasher.update(prompts[key].as_bytes());
        }
        format!("{:x}", hasher.finalize())
    }

    fn read_meta(pipeline_path: &Path) -> PipelineMeta {
        let meta_path = pipeline_path.with_extension("meta.json");
        std::fs::read_to_string(&meta_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn write_meta(pipeline_path: &Path, meta: &PipelineMeta) -> Result<(), String> {
        let meta_path = pipeline_path.with_extension("meta.json");
        let json =
            serde_json::to_string_pretty(meta).map_err(|e| format!("meta serialize error: {e}"))?;
        std::fs::write(&meta_path, json).map_err(|e| format!("meta write error: {e}"))
    }

    fn compute_drift(promoted_from: &PromotedFrom) -> Option<bool> {
        let source = Path::new(&promoted_from.path);
        let yaml = std::fs::read_to_string(source).ok()?;
        let prompts = read_prompts_dir(&source.with_extension("prompts"));
        let current = content_hash(&yaml, &prompts);
        Some(current != promoted_from.content_hash)
    }

    pub fn user_pipelines_dir() -> Option<PathBuf> {
        std::env::var_os("HOME").map(|h| {
            PathBuf::from(h)
                .join(".pdo")
                .join("library")
                .join("pipelines")
        })
    }

    pub fn repo_pipelines_dir(repo_root: &Path) -> PathBuf {
        repo_root.join(".pdo").join("library").join("pipelines")
    }

    fn scope_dir(repo_root: &Path, scope: Scope) -> Option<PathBuf> {
        match scope {
            Scope::Repo => Some(repo_pipelines_dir(repo_root)),
            Scope::User | Scope::Library => user_pipelines_dir(),
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
        /// Parsed form of `yaml`, normalized by the same parser the run/pipeline
        /// endpoints use. Clients must compare against this (not the raw text):
        /// textual comparison flags formatting noise — key order, defaults the
        /// parser fills in, serializer version drift — as divergence.
        pub pipeline: crate::pipeline::PipelineDef,
        #[serde(default)]
        pub prompts: HashMap<String, String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub promoted_from: Option<PromotedFrom>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub drifted: Option<bool>,
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

            let meta = read_meta(&path);
            let drifted = meta.promoted_from.as_ref().and_then(compute_drift);

            entries.push(PipelineLibraryEntry {
                id,
                name: parsed.pipeline.name.clone(),
                scope,
                node_count: parsed.pipeline.nodes.len(),
                modified,
                yaml: contents,
                pipeline: parsed.pipeline,
                prompts,
                promoted_from: meta.promoted_from,
                drifted,
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
        let meta_path = path.with_extension("meta.json");
        std::fs::remove_file(&path).map_err(|e| format!("delete error: {e}"))?;
        if prompts_dir.exists() {
            std::fs::remove_dir_all(&prompts_dir)
                .map_err(|e| format!("delete prompts error: {e}"))?;
        }
        let _ = std::fs::remove_file(&meta_path);
        Ok(true)
    }

    pub fn promote(repo_root: &Path, pipeline_id: &str) -> Result<String, String> {
        let repo_dir = repo_root.join(".pdo").join("pipelines");
        let source_path = repo_dir.join(format!("{pipeline_id}.yaml"));
        let yaml = std::fs::read_to_string(&source_path)
            .map_err(|e| format!("cannot read repo pipeline: {e}"))?;
        crate::pipeline::parse_pipeline(&yaml)
            .map_err(|e| format!("invalid pipeline YAML: {e}"))?;

        let prompts = read_prompts_dir(&source_path.with_extension("prompts"));
        let hash = content_hash(&yaml, &prompts);

        let lib_dir = user_pipelines_dir().ok_or("HOME not set")?;
        std::fs::create_dir_all(&lib_dir)
            .map_err(|e| format!("failed to create library dir: {e}"))?;

        let lib_path = lib_dir.join(format!("{pipeline_id}.yaml"));
        std::fs::write(&lib_path, &yaml).map_err(|e| format!("write error: {e}"))?;

        let lib_prompts_dir = lib_dir.join(format!("{pipeline_id}.prompts"));
        if lib_prompts_dir.exists() {
            std::fs::remove_dir_all(&lib_prompts_dir)
                .map_err(|e| format!("clear prompts error: {e}"))?;
        }
        if !prompts.is_empty() {
            std::fs::create_dir_all(&lib_prompts_dir)
                .map_err(|e| format!("create prompts dir error: {e}"))?;
            for (node_id, content) in &prompts {
                let prompt_path = lib_prompts_dir.join(format!("{node_id}.md"));
                std::fs::write(&prompt_path, content)
                    .map_err(|e| format!("write prompt error: {e}"))?;
            }
        }

        let meta = PipelineMeta {
            promoted_from: Some(PromotedFrom {
                repo: repo_root.to_string_lossy().to_string(),
                path: source_path.to_string_lossy().to_string(),
                content_hash: hash,
            }),
        };
        write_meta(&lib_path, &meta)?;

        Ok(pipeline_id.to_string())
    }

    /// Strip a single trailing ` (copy)` / ` (copy <digits>)` tail from a name,
    /// yielding the base. Case-sensitive; removes exactly one tail (plus one
    /// preceding space if present) then re-trims. An all-tail name (e.g.
    /// `(copy)`) collapses to the empty string — the caller maps that to
    /// `Untitled`.
    fn strip_copy_tail(name: &str) -> String {
        let s = name.trim();
        strip_one_copy_tail(s).unwrap_or(s).trim().to_string()
    }

    /// If `s` ends with a `(copy)` / `(copy <digits>)` parenthetical, return the
    /// slice before the opening `(` (which may carry a trailing space the caller
    /// re-trims). `None` if there is no such tail. Case-sensitive on `copy`.
    fn strip_one_copy_tail(s: &str) -> Option<&str> {
        let inner = s.strip_suffix(')')?; // drop the closing ')'
        let open = inner.rfind('(')?; // opening paren of the last group
        let body = &inner[open + 1..]; // content between '(' and ')'
        let is_copy = body == "copy"
            || body
                .strip_prefix("copy ")
                .map(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()))
                .unwrap_or(false);
        if is_copy {
            // `open` is a byte offset into `inner`, which is a prefix of `s`
            // (only the final ')' was dropped), so it indexes `s` too.
            Some(&s[..open])
        } else {
            None
        }
    }

    /// Pick the first free `"{base} (copy)"`, `"{base} (copy 2)"`, … name,
    /// compared case-sensitively against `existing`. Empty base ⇒ `Untitled`.
    fn compute_copy_name(base: &str, existing: &[String]) -> String {
        let base = if base.is_empty() { "Untitled" } else { base };
        let first = format!("{base} (copy)");
        if !existing.iter().any(|e| e == &first) {
            return first;
        }
        let mut n = 2u32;
        loop {
            let candidate = format!("{base} (copy {n})");
            if !existing.iter().any(|e| e == &candidate) {
                return candidate;
            }
            n += 1;
        }
    }

    /// Emit `s` as a double-quoted, escaped YAML scalar. A JSON string literal
    /// is a valid YAML double-quoted scalar (same `\`/`"`/control-char escapes),
    /// so we lean on `serde_json` rather than hand-rolling an escaper.
    fn quote_yaml_scalar(s: &str) -> String {
        serde_json::to_string(s).unwrap_or_else(|_| format!("{s:?}"))
    }

    /// Rewrite **only** the value of the single column-0 `name:` line, preserving
    /// every other byte (comments, key order, unknown top-level keys like
    /// `auto_merge_resolver`, and per-line terminators). Node `name:` lines are
    /// indented, so anchoring on a column-0 `name:` is unambiguous. Returns `Err`
    /// if no such line exists (the source would fail the required-`name` parse).
    fn rewrite_top_level_name(yaml: &str, new_name: &str) -> Result<String, String> {
        let quoted = quote_yaml_scalar(new_name);
        let mut out = String::with_capacity(yaml.len() + quoted.len());
        let mut replaced = false;
        for line in yaml.split_inclusive('\n') {
            if !replaced {
                let (content, term): (&str, &str) = if let Some(c) = line.strip_suffix("\r\n") {
                    (c, "\r\n")
                } else if let Some(c) = line.strip_suffix('\n') {
                    (c, "\n")
                } else {
                    (line, "")
                };
                // `starts_with("name:")` already implies column 0 (no leading
                // whitespace) — a 'n' can't follow indentation.
                if content.starts_with("name:") {
                    out.push_str("name: ");
                    out.push_str(&quoted);
                    out.push_str(term);
                    replaced = true;
                    continue;
                }
            }
            out.push_str(line);
        }
        if !replaced {
            return Err("no top-level `name:` line found".to_string());
        }
        Ok(out)
    }

    /// Duplicate a library pipeline into a clean, **unlinked** fork: fresh id,
    /// name suffixed `(copy)` / `(copy N)`, and NO `meta.json` / `promoted_from`.
    /// The copy lands in the same scope as the source. Returns the new id.
    ///
    /// The source YAML is rewritten **verbatim except its top-level `name:`
    /// line** (never re-serialized) so unknown top-level keys, comments, and
    /// formatting survive a round-trip — see `rewrite_top_level_name`.
    pub fn duplicate(repo_root: &Path, id: &str) -> Result<String, String> {
        // 1. Locate source (repo OR user) — one call yields path + scope.
        let (source_path, scope) =
            locate(repo_root, id).ok_or_else(|| format!("pipeline not found: {id}"))?;

        // 2. Read raw YAML verbatim (do NOT re-serialize the document).
        let yaml = std::fs::read_to_string(&source_path)
            .map_err(|e| format!("cannot read source pipeline: {e}"))?;

        // 3. Source prompts (empty map if .prompts/ is absent — not an error).
        let prompts = read_prompts_dir(&source_path.with_extension("prompts"));

        // 4. Compute a unique "(copy)" name against all library names (both scopes).
        let parsed = crate::pipeline::parse_pipeline(&yaml)
            .map_err(|e| format!("invalid source pipeline YAML: {e}"))?;
        let base = strip_copy_tail(&parsed.pipeline.name);
        let existing: Vec<String> = list(repo_root).into_iter().map(|e| e.name).collect();
        let new_name = compute_copy_name(&base, &existing);

        // 5. Textual rewrite of the single column-0 `name:` line + re-parse
        //    assertion (parse alone won't catch a no-op rewrite).
        let rewritten = rewrite_top_level_name(&yaml, &new_name)?;
        let reparsed = crate::pipeline::parse_pipeline(&rewritten)
            .map_err(|e| format!("rewritten pipeline YAML invalid: {e}"))?;
        if reparsed.pipeline.name != new_name {
            return Err(format!(
                "name rewrite failed: expected {new_name:?}, got {:?}",
                reparsed.pipeline.name
            ));
        }

        // 6. Clean fork: id=None ⇒ fresh slug, writes YAML + prompts, no meta.json.
        save(repo_root, None, &new_name, &rewritten, &prompts, scope)
    }

    pub fn check_drift(library_id: &str) -> Option<bool> {
        let lib_dir = user_pipelines_dir()?;
        let lib_path = lib_dir.join(format!("{library_id}.yaml"));
        let meta = read_meta(&lib_path);
        meta.promoted_from.as_ref().and_then(compute_drift)
    }

    pub fn get_meta(library_id: &str) -> Option<PipelineMeta> {
        let lib_dir = user_pipelines_dir()?;
        let lib_path = lib_dir.join(format!("{library_id}.yaml"));
        if lib_path.exists() {
            Some(read_meta(&lib_path))
        } else {
            None
        }
    }

    /// Unit tests for the pure `(copy)`-naming and YAML-rewrite helpers. They are
    /// private to this module, so this nested test mod (a descendant) is the only
    /// place that can exercise them directly — the behavioral `duplicate` tests
    /// live in the outer `library_store::tests` module against the public fn.
    #[cfg(test)]
    mod copy_helpers_tests {
        use super::*;

        /// A minimal pipeline that satisfies `parse_pipeline` (one start, one
        /// end) with a column-0 `name:` and indented node `name:` lines.
        const VALID_YAML: &str = "name: original\nversion: \"1.0\"\nnodes:\n  - id: start\n    name: Start\n    type: start\n    outputs:\n      - name: user_prompt\n  - id: end\n    name: End\n    type: end\n    inputs:\n      - name: result\n";

        #[test]
        fn strip_copy_tail_table() {
            // (input, expected base) — mirrors the spec truth table.
            let cases = [
                ("foo (copy)", "foo"),
                ("foo (copy 2)", "foo"),
                ("foo (copy 10)", "foo"),
                ("foo (copy)(copy)", "foo (copy)"), // strip only ONE tail
                ("foo (copy) (copy)", "foo (copy)"), // single-strip (chosen)
                ("(copy)", ""),                     // empty base -> Untitled later
                (" (copy) ", ""),                   // trim -> strip -> trim
                ("", ""),                           // empty
                ("foo (copyright)", "foo (copyright)"), // ')' must follow copy/digits
                ("foo (Copy)", "foo (Copy)"),       // case-sensitive
                ("foo  (copy)", "foo"),             // eats one space, re-trim removes rest
            ];
            for (input, expected) in cases {
                assert_eq!(strip_copy_tail(input), expected, "input: {input:?}");
            }
        }

        #[test]
        fn compute_copy_name_first_free() {
            assert_eq!(compute_copy_name("foo", &[]), "foo (copy)");
        }

        #[test]
        fn compute_copy_name_collision_bumps() {
            let existing = vec!["foo (copy)".to_string()];
            assert_eq!(compute_copy_name("foo", &existing), "foo (copy 2)");
            let existing = vec!["foo (copy)".to_string(), "foo (copy 2)".to_string()];
            assert_eq!(compute_copy_name("foo", &existing), "foo (copy 3)");
        }

        #[test]
        fn compute_copy_name_empty_base_is_untitled() {
            assert_eq!(compute_copy_name("", &[]), "Untitled (copy)");
        }

        #[test]
        fn compute_copy_name_is_case_sensitive() {
            // "Foo (copy)" must NOT satisfy a request for base "foo".
            let existing = vec!["Foo (copy)".to_string()];
            assert_eq!(compute_copy_name("foo", &existing), "foo (copy)");
        }

        #[test]
        fn rewrite_top_level_name_replaces_only_first_column0_name() {
            let out = rewrite_top_level_name(VALID_YAML, "new name").unwrap();
            assert!(out.starts_with("name: \"new name\"\n"));
            // Indented node `name:` lines are untouched.
            assert!(out.contains("    name: Start\n"));
            assert!(out.contains("    name: End\n"));
            // Re-parsing yields exactly the requested name.
            let parsed = crate::pipeline::parse_pipeline(&out).unwrap();
            assert_eq!(parsed.pipeline.name, "new name");
        }

        #[test]
        fn rewrite_top_level_name_preserves_unknown_keys_and_comments() {
            let yaml = "# leading comment\nname: review-loop\nversion: \"1.0\"\nnodes:\n  - id: end\n    name: End\n    type: end\nauto_merge_resolver: true\n";
            let out = rewrite_top_level_name(yaml, "review-loop (copy)").unwrap();
            assert!(out.contains("# leading comment\n"));
            assert!(out.contains("\nauto_merge_resolver: true\n"));
            assert!(out.contains("name: \"review-loop (copy)\"\n"));
            // Everything except the name: line is byte-identical.
            let strip_name = |s: &str| {
                s.lines()
                    .filter(|l| !l.starts_with("name:"))
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            assert_eq!(strip_name(yaml), strip_name(&out));
        }

        #[test]
        fn rewrite_top_level_name_escapes_special_chars() {
            let tricky = "a\"b\\c\td";
            let out = rewrite_top_level_name(VALID_YAML, tricky).unwrap();
            let parsed = crate::pipeline::parse_pipeline(&out).unwrap();
            assert_eq!(parsed.pipeline.name, tricky);
        }

        #[test]
        fn rewrite_top_level_name_errors_without_name_line() {
            let yaml = "version: \"1.0\"\nnodes: []\n";
            assert!(rewrite_top_level_name(yaml, "x").is_err());
        }

        #[test]
        fn rewrite_top_level_name_preserves_missing_final_newline() {
            // No trailing newline on the last line must survive.
            let yaml = "name: a\nversion: \"1.0\"";
            let out = rewrite_top_level_name(yaml, "b").unwrap();
            assert!(!out.ends_with('\n'));
            assert!(out.ends_with("version: \"1.0\""));
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
        // Poison-tolerant: the lock only serializes HOME mutation; a panic in
        // another test must not cascade into every later HOME-using test.
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = std::env::temp_dir().join(format!("pdo-lib-test-{}", std::process::id()));
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
    fn pipeline_library_list_exposes_parsed_pipeline() {
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

            let all = pipelines::list(repo);
            assert_eq!(all.len(), 1);
            let parsed = &all[0].pipeline;
            assert_eq!(parsed.name, "My Pipeline");
            assert_eq!(parsed.nodes.len(), 3);
            assert_eq!(parsed.edges.len(), 2);
            // The parser's normalizations are baked in — e.g. port sides get
            // their defaults — so clients comparing two parsed pipelines see
            // both sides normalized identically.
            let planner = parsed.nodes.iter().find(|n| n.id == "planner").unwrap();
            assert_eq!(planner.inputs[0].side, Some(pipeline::PortSide::Left));
            assert_eq!(planner.outputs[0].side, Some(pipeline::PortSide::Right));
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

    fn create_repo_pipeline(repo: &std::path::Path, name: &str) -> String {
        let yaml = sample_pipeline_yaml(name);
        let pipelines_dir = repo.join(".pdo").join("pipelines");
        std::fs::create_dir_all(&pipelines_dir).unwrap();
        let slug = name.to_lowercase().replace(' ', "-");
        std::fs::write(pipelines_dir.join(format!("{slug}.yaml")), &yaml).unwrap();
        slug
    }

    fn create_repo_pipeline_with_prompts(
        repo: &std::path::Path,
        name: &str,
        prompts: &HashMap<String, String>,
    ) -> String {
        let slug = create_repo_pipeline(repo, name);
        if !prompts.is_empty() {
            let prompts_dir = repo
                .join(".pdo")
                .join("pipelines")
                .join(format!("{slug}.prompts"));
            std::fs::create_dir_all(&prompts_dir).unwrap();
            for (node_id, content) in prompts {
                std::fs::write(prompts_dir.join(format!("{node_id}.md")), content).unwrap();
            }
        }
        slug
    }

    #[test]
    fn content_hash_deterministic() {
        let yaml = sample_pipeline_yaml("Test");
        let prompts = HashMap::new();
        let h1 = pipelines::content_hash(&yaml, &prompts);
        let h2 = pipelines::content_hash(&yaml, &prompts);
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64); // SHA-256 hex
    }

    #[test]
    fn content_hash_changes_on_yaml_diff() {
        let prompts = HashMap::new();
        let h1 = pipelines::content_hash(&sample_pipeline_yaml("A"), &prompts);
        let h2 = pipelines::content_hash(&sample_pipeline_yaml("B"), &prompts);
        assert_ne!(h1, h2);
    }

    #[test]
    fn content_hash_changes_on_prompt_diff() {
        let yaml = sample_pipeline_yaml("Same");
        let mut p1 = HashMap::new();
        p1.insert("planner".to_string(), "Version 1".to_string());
        let mut p2 = HashMap::new();
        p2.insert("planner".to_string(), "Version 2".to_string());
        assert_ne!(
            pipelines::content_hash(&yaml, &p1),
            pipelines::content_hash(&yaml, &p2),
        );
    }

    #[test]
    fn promote_copies_to_library_with_metadata() {
        with_temp_repo(|repo| {
            let slug = create_repo_pipeline(repo, "My Pipeline");

            let id = pipelines::promote(repo, &slug).unwrap();
            assert_eq!(id, slug);

            let lib_dir = pipelines::user_pipelines_dir().unwrap();
            assert!(lib_dir.join(format!("{slug}.yaml")).exists());
            assert!(lib_dir.join(format!("{slug}.meta.json")).exists());

            let meta = pipelines::get_meta(&slug).unwrap();
            assert!(meta.promoted_from.is_some());
            let pf = meta.promoted_from.unwrap();
            assert_eq!(pf.repo, repo.to_string_lossy());
            assert!(!pf.content_hash.is_empty());
        });
    }

    #[test]
    fn promote_copies_prompts_to_library() {
        with_temp_repo(|repo| {
            let mut prompts = HashMap::new();
            prompts.insert("planner".to_string(), "You plan things.".to_string());
            let slug = create_repo_pipeline_with_prompts(repo, "Prompted", &prompts);

            pipelines::promote(repo, &slug).unwrap();

            let lib_dir = pipelines::user_pipelines_dir().unwrap();
            let lib_prompts = lib_dir.join(format!("{slug}.prompts"));
            assert!(lib_prompts.is_dir());
            assert_eq!(
                std::fs::read_to_string(lib_prompts.join("planner.md")).unwrap(),
                "You plan things."
            );
        });
    }

    #[test]
    fn promote_nonexistent_pipeline_errors() {
        with_temp_repo(|repo| {
            let result = pipelines::promote(repo, "nonexistent");
            assert!(result.is_err());
        });
    }

    #[test]
    fn drift_detection_no_drift_when_unchanged() {
        with_temp_repo(|repo| {
            let slug = create_repo_pipeline(repo, "Stable");
            pipelines::promote(repo, &slug).unwrap();

            let drifted = pipelines::check_drift(&slug);
            assert_eq!(drifted, Some(false));
        });
    }

    #[test]
    fn drift_detection_detects_yaml_change() {
        with_temp_repo(|repo| {
            let slug = create_repo_pipeline(repo, "Drifter");
            pipelines::promote(repo, &slug).unwrap();

            let repo_yaml_path = repo.join(".pdo/pipelines").join(format!("{slug}.yaml"));
            std::fs::write(&repo_yaml_path, sample_pipeline_yaml("Drifter Modified")).unwrap();

            let drifted = pipelines::check_drift(&slug);
            assert_eq!(drifted, Some(true));
        });
    }

    #[test]
    fn drift_detection_detects_prompt_change() {
        with_temp_repo(|repo| {
            let mut prompts = HashMap::new();
            prompts.insert("planner".to_string(), "Original".to_string());
            let slug = create_repo_pipeline_with_prompts(repo, "PromptDrift", &prompts);
            pipelines::promote(repo, &slug).unwrap();

            let prompt_path = repo
                .join(".pdo/pipelines")
                .join(format!("{slug}.prompts/planner.md"));
            std::fs::write(&prompt_path, "Changed prompt").unwrap();

            let drifted = pipelines::check_drift(&slug);
            assert_eq!(drifted, Some(true));
        });
    }

    #[test]
    fn re_promote_updates_hash() {
        with_temp_repo(|repo| {
            let slug = create_repo_pipeline(repo, "Updater");
            pipelines::promote(repo, &slug).unwrap();

            let repo_yaml_path = repo.join(".pdo/pipelines").join(format!("{slug}.yaml"));
            std::fs::write(&repo_yaml_path, sample_pipeline_yaml("Updater v2")).unwrap();

            assert_eq!(pipelines::check_drift(&slug), Some(true));

            pipelines::promote(repo, &slug).unwrap();

            assert_eq!(pipelines::check_drift(&slug), Some(false));
        });
    }

    #[test]
    fn drift_returns_none_when_no_promoted_from() {
        with_temp_repo(|repo| {
            pipelines::save(
                repo,
                None,
                "Manual",
                &sample_pipeline_yaml("Manual"),
                &HashMap::new(),
                pipelines::Scope::User,
            )
            .unwrap();

            let drifted = pipelines::check_drift("manual");
            assert_eq!(drifted, None);
        });
    }

    #[test]
    fn drift_returns_none_when_source_deleted() {
        with_temp_repo(|repo| {
            let slug = create_repo_pipeline(repo, "Ephemeral");
            pipelines::promote(repo, &slug).unwrap();

            let repo_yaml_path = repo.join(".pdo/pipelines").join(format!("{slug}.yaml"));
            std::fs::remove_file(&repo_yaml_path).unwrap();

            let drifted = pipelines::check_drift(&slug);
            assert_eq!(drifted, None);
        });
    }

    #[test]
    fn delete_removes_meta_json() {
        with_temp_repo(|repo| {
            let slug = create_repo_pipeline(repo, "CleanMe");
            pipelines::promote(repo, &slug).unwrap();

            let lib_dir = pipelines::user_pipelines_dir().unwrap();
            assert!(lib_dir.join(format!("{slug}.meta.json")).exists());

            pipelines::delete(repo, &slug).unwrap();
            assert!(!lib_dir.join(format!("{slug}.meta.json")).exists());
        });
    }

    // --- #224: duplicate a library pipeline (unlinked clone) ---

    /// A library YAML with a leading comment and a non-`PipelineDef` top-level
    /// key (`auto_merge_resolver`) — the byte-fidelity bait, mirrors
    /// `review-loop.yaml`.
    fn fixture_with_extras(name: &str) -> String {
        format!(
            "# a comment that must survive\nname: {name}\nversion: \"1.0\"\nnodes:\n  - id: start\n    name: Start\n    type: start\n    outputs:\n      - name: user_prompt\n  - id: end\n    name: End\n    type: end\n    inputs:\n      - name: result\nauto_merge_resolver: true\n"
        )
    }

    #[test]
    fn duplicate_creates_unlinked_copy_with_copy_name() {
        with_temp_repo(|repo| {
            let id = pipelines::save(
                repo,
                None,
                "My Pipeline",
                &sample_pipeline_yaml("My Pipeline"),
                &HashMap::new(),
                pipelines::Scope::User,
            )
            .unwrap();

            let copy_id = pipelines::duplicate(repo, &id).unwrap();
            assert_ne!(copy_id, id, "copy must have a fresh id");

            let all = pipelines::list(repo);
            let copy = all.iter().find(|e| e.id == copy_id).unwrap();
            assert_eq!(copy.name, "My Pipeline (copy)");
            assert_eq!(copy.scope, pipelines::Scope::User);
            // Source is untouched.
            assert!(all.iter().any(|e| e.id == id && e.name == "My Pipeline"));
        });
    }

    #[test]
    fn duplicate_preserves_yaml_verbatim_except_name() {
        with_temp_repo(|repo| {
            let yaml = fixture_with_extras("Fixture");
            let id = pipelines::save(
                repo,
                None,
                "Fixture",
                &yaml,
                &HashMap::new(),
                pipelines::Scope::User,
            )
            .unwrap();

            let copy_id = pipelines::duplicate(repo, &id).unwrap();
            let lib_dir = pipelines::user_pipelines_dir().unwrap();
            let copy_yaml =
                std::fs::read_to_string(lib_dir.join(format!("{copy_id}.yaml"))).unwrap();

            // The unknown top-level key and the comment survive verbatim.
            assert!(
                copy_yaml.contains("\nauto_merge_resolver: true\n"),
                "unknown top-level key must survive: {copy_yaml}"
            );
            assert!(copy_yaml.starts_with("# a comment that must survive\n"));
            // Only the name: line changed.
            assert!(copy_yaml.contains("name: \"Fixture (copy)\"\n"));
            let strip_name = |s: &str| {
                s.lines()
                    .filter(|l| !l.starts_with("name:"))
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            assert_eq!(
                strip_name(&yaml),
                strip_name(&copy_yaml),
                "every non-name line must be byte-identical"
            );
        });
    }

    #[test]
    fn duplicate_copies_prompts() {
        with_temp_repo(|repo| {
            let mut prompts = HashMap::new();
            prompts.insert("planner".to_string(), "You plan things.".to_string());
            let id = pipelines::save(
                repo,
                None,
                "Prompted",
                &sample_pipeline_yaml("Prompted"),
                &prompts,
                pipelines::Scope::User,
            )
            .unwrap();

            let copy_id = pipelines::duplicate(repo, &id).unwrap();
            let lib_dir = pipelines::user_pipelines_dir().unwrap();
            let copy_prompt = lib_dir.join(format!("{copy_id}.prompts")).join("planner.md");
            assert_eq!(
                std::fs::read_to_string(&copy_prompt).unwrap(),
                "You plan things."
            );
        });
    }

    #[test]
    fn duplicate_of_promoted_source_has_no_meta() {
        with_temp_repo(|repo| {
            // Promote a repo pipeline -> a user-scope library entry WITH meta.json.
            let slug = create_repo_pipeline(repo, "Promoted");
            pipelines::promote(repo, &slug).unwrap();
            let lib_dir = pipelines::user_pipelines_dir().unwrap();
            assert!(lib_dir.join(format!("{slug}.meta.json")).exists());

            // Duplicating it yields a clean fork: no meta.json, no promoted_from.
            let copy_id = pipelines::duplicate(repo, &slug).unwrap();
            assert!(
                !lib_dir.join(format!("{copy_id}.meta.json")).exists(),
                "the copy must carry no promotion sidecar"
            );
            let meta = pipelines::get_meta(&copy_id).unwrap();
            assert!(meta.promoted_from.is_none());
        });
    }

    #[test]
    fn duplicate_preserves_repo_scope() {
        with_temp_repo(|repo| {
            let id = pipelines::save(
                repo,
                None,
                "Repo Source",
                &sample_pipeline_yaml("Repo Source"),
                &HashMap::new(),
                pipelines::Scope::Repo,
            )
            .unwrap();

            let copy_id = pipelines::duplicate(repo, &id).unwrap();
            assert!(pipelines::repo_pipelines_dir(repo)
                .join(format!("{copy_id}.yaml"))
                .exists());
            let all = pipelines::list(repo);
            let copy = all.iter().find(|e| e.id == copy_id).unwrap();
            assert_eq!(copy.scope, pipelines::Scope::Repo);
        });
    }

    #[test]
    fn duplicate_twice_yields_copy_then_copy_2() {
        with_temp_repo(|repo| {
            let id = pipelines::save(
                repo,
                None,
                "Twice",
                &sample_pipeline_yaml("Twice"),
                &HashMap::new(),
                pipelines::Scope::User,
            )
            .unwrap();

            let copy1 = pipelines::duplicate(repo, &id).unwrap();
            let copy2 = pipelines::duplicate(repo, &id).unwrap();
            assert_ne!(copy1, copy2);

            let all = pipelines::list(repo);
            let n1 = &all.iter().find(|e| e.id == copy1).unwrap().name;
            let n2 = &all.iter().find(|e| e.id == copy2).unwrap().name;
            assert_eq!(n1, "Twice (copy)");
            assert_eq!(n2, "Twice (copy 2)");
        });
    }

    #[test]
    fn duplicate_nonexistent_errors() {
        with_temp_repo(|repo| {
            assert!(pipelines::duplicate(repo, "nonexistent").is_err());
        });
    }

    #[test]
    fn duplicate_invalid_yaml_errors() {
        with_temp_repo(|repo| {
            // Write a name-less YAML straight to the library dir (bypassing
            // `save`'s validation) so `duplicate` hits the parse failure.
            let lib_dir = pipelines::user_pipelines_dir().unwrap();
            std::fs::create_dir_all(&lib_dir).unwrap();
            std::fs::write(lib_dir.join("broken.yaml"), "version: \"1.0\"\nnodes: []\n").unwrap();

            assert!(pipelines::duplicate(repo, "broken").is_err());
        });
    }
}
