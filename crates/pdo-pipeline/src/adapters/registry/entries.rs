//! The two all-or-nothing writes of the registry: moving an entry to a new
//! stem, and publishing a new entry.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::domain::pipeline;

/// #774 — move one registry entry to a new stem: the YAML (written from
/// `new_yaml`) and its `.prompts/` sidecar travel together. Collisions are the
/// caller's refusal (`pipeline_name_conflict`); this only stages, publishes
/// and rolls the whole move back on any failure — a half-done rename (new
/// yaml without the sidecar, or both files present) must never be observable.
/// The old sidecar is moved wholesale first so prompts the caller didn't carry
/// survive the move (the save path prunes orphans afterwards).
pub(super) fn move_pipeline_entry(
    dir: &Path,
    old_path: &Path,
    new_id: &str,
    new_yaml: &str,
) -> Result<PathBuf, String> {
    let new_path = dir.join(format!("{new_id}.yaml"));
    if new_path == *old_path {
        return Ok(new_path);
    }
    let old_prompts = old_path.with_extension("prompts");
    let new_prompts = new_path.with_extension("prompts");
    let nonce = format!(
        "{}.{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    );
    let staged = dir.join(format!(".{new_id}.yaml.renaming-{nonce}"));
    let prompts_moved = old_prompts.is_dir();
    let result = (|| -> Result<(), String> {
        if prompts_moved {
            std::fs::rename(&old_prompts, &new_prompts)
                .map_err(|e| format!("move prompts: {e}"))?;
        }
        std::fs::write(&staged, new_yaml.as_bytes()).map_err(|e| format!("write pipeline: {e}"))?;
        std::fs::rename(&staged, &new_path).map_err(|e| format!("publish pipeline: {e}"))?;
        std::fs::remove_file(old_path).map_err(|e| format!("retire old pipeline: {e}"))
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&staged);
        let _ = std::fs::remove_file(&new_path);
        if prompts_moved && new_prompts.is_dir() {
            let _ = std::fs::rename(&new_prompts, &old_prompts);
        }
        return result.map(|()| new_path);
    }
    Ok(new_path)
}

pub(super) fn write_pipeline_atomically(
    dir: &Path,
    id: &str,
    pipeline: &pipeline::PipelineDef,
    prompts: &HashMap<String, String>,
) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("create pipeline registry: {e}"))?;
    let path = dir.join(format!("{id}.yaml"));
    let temp_path = dir.join(format!(".{id}.yaml.tmp"));
    let temp_prompts = dir.join(format!(".{id}.prompts.tmp"));
    let prompts_path = dir.join(format!("{id}.prompts"));
    let lock_path = dir.join(format!(".{id}.create.lock"));
    let lock = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock_path)
        .map_err(|e| format!("pipeline identity is no longer available: {e}"))?;
    if path.exists() || prompts_path.exists() {
        drop(lock);
        let _ = std::fs::remove_file(&lock_path);
        return Err("pipeline identity is no longer available".into());
    }

    let yaml = serde_yaml::to_string(pipeline).map_err(|e| format!("serialize pipeline: {e}"))?;
    pipeline::parse_pipeline(&yaml).map_err(|e| format!("pipeline: {e}"))?;
    // Never seed a fresh registry entry with a leftover: duplicate copies
    // the source sidecar dir wholesale, orphans included.
    let (prompts, _) = pipeline::split_live_prompts(pipeline, prompts);
    let result = (|| {
        std::fs::write(&temp_path, yaml).map_err(|e| format!("write pipeline: {e}"))?;

        if !prompts.is_empty() {
            std::fs::create_dir(&temp_prompts).map_err(|e| format!("stage prompts: {e}"))?;
            for (node_id, content) in &prompts {
                std::fs::write(temp_prompts.join(format!("{node_id}.md")), content)
                    .map_err(|e| format!("write prompts.{node_id}: {e}"))?;
            }
            std::fs::rename(&temp_prompts, &prompts_path)
                .map_err(|e| format!("publish prompts: {e}"))?;
        }
        std::fs::rename(&temp_path, &path).map_err(|e| format!("publish pipeline: {e}"))?;
        Ok(path.clone())
    })();
    drop(lock);
    let _ = std::fs::remove_file(&lock_path);
    let _ = std::fs::remove_file(&temp_path);
    if temp_prompts.is_dir() {
        let _ = std::fs::remove_dir_all(&temp_prompts);
    }
    if result.is_err() && prompts_path.is_dir() {
        let _ = std::fs::remove_dir_all(&prompts_path);
    }
    result
}
