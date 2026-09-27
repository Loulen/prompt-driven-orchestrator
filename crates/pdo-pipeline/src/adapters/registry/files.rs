//! Reading the registry: its pipeline files, a pipeline's prompts.

use std::collections::HashMap;
use std::path::Path;

use crate::application::ports::RegistryFile;

/// Every `.yaml` file directly under `dir`, in directory order, read.
pub fn pipeline_files(dir: &Path) -> Vec<RegistryFile> {
    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files = Vec::new();
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
        let modified = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .map(|t| {
                let dt: chrono::DateTime<chrono::Utc> = t.into();
                dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
            });
        let yaml = std::fs::read_to_string(&path).ok();
        files.push(RegistryFile {
            id,
            path,
            modified,
            yaml,
        });
    }
    files
}

/// The prompts of the pipeline file at `path`: `<stem>.prompts/*.md`.
pub fn pipeline_prompts(path: &Path) -> HashMap<String, String> {
    read_prompts_dir(&path.with_extension("prompts"))
}

/// Every `<node>.md` of a prompts directory, by node id.
pub fn read_prompts_dir(prompts_dir: &Path) -> HashMap<String, String> {
    let mut prompts = HashMap::new();
    let Ok(entries) = std::fs::read_dir(prompts_dir) else {
        return prompts;
    };
    for entry in entries.flatten() {
        let prompt_path = entry.path();
        if prompt_path.extension().and_then(|value| value.to_str()) != Some("md") {
            continue;
        }
        if let (Some(id), Ok(content)) = (
            prompt_path.file_stem().and_then(|value| value.to_str()),
            std::fs::read_to_string(&prompt_path),
        ) {
            prompts.insert(id.to_string(), content);
        }
    }
    prompts
}
