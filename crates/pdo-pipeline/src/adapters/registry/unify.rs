//! The registry unification (#774): the retired registries (a repository's
//! `.pdo/pipelines`, `~/.pdo/library/pipelines`) are folded into the instance
//! registry once per repository, collisions kept side by side.

use std::path::Path;

use super::paths::{home_dir, instance_pipeline_dir};

fn copy_directory(source: &Path, target: &Path) -> Result<(), String> {
    std::fs::create_dir_all(target).map_err(|e| format!("create {}: {e}", target.display()))?;
    for entry in std::fs::read_dir(source).map_err(|e| format!("read {}: {e}", source.display()))? {
        let entry = entry.map_err(|e| format!("read migration entry: {e}"))?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        if source_path.is_dir() {
            copy_directory(&source_path, &target_path)?;
        } else {
            std::fs::copy(&source_path, &target_path)
                .map_err(|e| format!("copy {}: {e}", source_path.display()))?;
        }
    }
    Ok(())
}

fn directories_match(left: &Path, right: &Path) -> Result<bool, String> {
    if left.is_dir() != right.is_dir() {
        return Ok(false);
    }
    if !left.is_dir() {
        return Ok(!right.exists());
    }
    let mut left_entries = std::fs::read_dir(left)
        .map_err(|e| format!("read {}: {e}", left.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("read {}: {e}", left.display()))?;
    let mut right_entries = std::fs::read_dir(right)
        .map_err(|e| format!("read {}: {e}", right.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("read {}: {e}", right.display()))?;
    left_entries.sort_by_key(|entry| entry.file_name());
    right_entries.sort_by_key(|entry| entry.file_name());
    if left_entries.len() != right_entries.len() {
        return Ok(false);
    }
    for (left_entry, right_entry) in left_entries.iter().zip(&right_entries) {
        if left_entry.file_name() != right_entry.file_name() {
            return Ok(false);
        }
        let left_path = left_entry.path();
        let right_path = right_entry.path();
        if left_path.is_dir() {
            if !directories_match(&left_path, &right_path)? {
                return Ok(false);
            }
        } else if std::fs::read(&left_path)
            .map_err(|e| format!("read {}: {e}", left_path.display()))?
            != std::fs::read(&right_path)
                .map_err(|e| format!("read {}: {e}", right_path.display()))?
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn publish_migrated_pipeline(
    source_yaml: &Path,
    source_bytes: &[u8],
    candidate: &Path,
) -> Result<(), String> {
    let source_prompts = source_yaml.with_extension("prompts");
    let candidate_prompts = candidate.with_extension("prompts");
    let nonce = format!(
        "{}.{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    );
    let staged_yaml = candidate.with_extension(format!("yaml.migrating-{nonce}"));
    let staged_prompts = candidate.with_extension(format!("prompts.migrating-{nonce}"));
    if !candidate.exists() && candidate_prompts.is_dir() {
        std::fs::remove_dir_all(&candidate_prompts)
            .map_err(|e| format!("recover {}: {e}", candidate_prompts.display()))?;
    }
    let result = (|| {
        std::fs::write(&staged_yaml, source_bytes)
            .map_err(|e| format!("stage {}: {e}", candidate.display()))?;
        if source_prompts.is_dir() {
            copy_directory(&source_prompts, &staged_prompts)?;
            std::fs::rename(&staged_prompts, &candidate_prompts)
                .map_err(|e| format!("publish {}: {e}", candidate_prompts.display()))?;
        }
        std::fs::rename(&staged_yaml, candidate)
            .map_err(|e| format!("publish {}: {e}", candidate.display()))
    })();
    let _ = std::fs::remove_file(&staged_yaml);
    if staged_prompts.is_dir() {
        let _ = std::fs::remove_dir_all(&staged_prompts);
    }
    if result.is_err() && candidate_prompts.is_dir() {
        let _ = std::fs::remove_dir_all(&candidate_prompts);
    }
    result
}

/// Fold the retired registries of `repo_root` and of `$HOME` into the
/// instance registry, once per repository (a marker beside the registry
/// remembers it). A same-named file whose content differs lands under the
/// next free `-N` id; an identical one is not copied twice.
pub fn unify_legacy_registries(repo_root: &Path) -> Result<(), String> {
    let Some(target) = instance_pipeline_dir(repo_root) else {
        return Err("cannot determine instance pipeline directory".into());
    };
    unify_into(repo_root, &target, home_dir().as_deref())
}

pub(super) fn unify_into(
    repo_root: &Path,
    target: &Path,
    home: Option<&Path>,
) -> Result<(), String> {
    let target = target.to_path_buf();
    use sha2::{Digest, Sha256};
    let source_key = Sha256::digest(repo_root.to_string_lossy().as_bytes());
    let marker = target
        .parent()
        .expect("pipeline registry has a .pdo parent")
        .join(format!(
            ".pipelines-unified-v1-{}",
            &format!("{source_key:x}")[..12]
        ));
    if marker.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(&target).map_err(|e| format!("create pipeline registry: {e}"))?;
    // The former user-scoped `~/.pdo/pipelines` directory is the unified
    // registry itself, so its entries remain in place. Include it in the scan
    // to make that no-loss case explicit and collision-safe.
    let mut sources = vec![target.clone(), repo_root.join(".pdo").join("pipelines")];
    if let Some(home) = home {
        sources.push(home.join(".pdo").join("library").join("pipelines"));
    }

    for source in sources {
        let Ok(entries) = std::fs::read_dir(&source) else {
            continue;
        };
        for entry in entries.flatten() {
            let source_yaml = entry.path();
            if source_yaml.extension().and_then(|value| value.to_str()) != Some("yaml") {
                continue;
            }
            let Some(base_id) = source_yaml.file_stem().and_then(|value| value.to_str()) else {
                continue;
            };
            let source_bytes = std::fs::read(&source_yaml)
                .map_err(|e| format!("read {}: {e}", source_yaml.display()))?;
            let mut id = base_id.to_string();
            let mut suffix = 2u32;
            loop {
                let candidate = target.join(format!("{id}.yaml"));
                let source_prompts = source_yaml.with_extension("prompts");
                let candidate_prompts = candidate.with_extension("prompts");
                if !candidate.exists() {
                    publish_migrated_pipeline(&source_yaml, &source_bytes, &candidate)?;
                    break;
                }
                if std::fs::read(&candidate).ok().as_deref() == Some(source_bytes.as_slice())
                    && directories_match(&source_prompts, &candidate_prompts)?
                {
                    break;
                }
                id = format!("{base_id}-{suffix}");
                suffix += 1;
            }
        }
    }
    std::fs::write(marker, b"1\n").map_err(|e| format!("finish pipeline migration: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrated_pipeline_is_published_with_its_prompt_sidecars() {
        let source = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        let source_yaml = source.path().join("review.yaml");
        let source_prompts = source.path().join("review.prompts");
        std::fs::write(&source_yaml, "name: review\n").unwrap();
        std::fs::create_dir(&source_prompts).unwrap();
        std::fs::write(source_prompts.join("worker.md"), "review carefully").unwrap();
        let candidate = target.path().join("review.yaml");
        let interrupted_prompts = target.path().join("review.prompts");
        std::fs::create_dir(&interrupted_prompts).unwrap();
        std::fs::write(interrupted_prompts.join("worker.md"), "stale partial copy").unwrap();

        publish_migrated_pipeline(&source_yaml, b"name: review\n", &candidate).unwrap();

        assert_eq!(std::fs::read(&candidate).unwrap(), b"name: review\n");
        assert_eq!(
            std::fs::read_to_string(target.path().join("review.prompts/worker.md")).unwrap(),
            "review carefully"
        );
        assert!(directories_match(&source_prompts, &target.path().join("review.prompts")).unwrap());
    }

    #[test]
    fn migration_collision_comparison_includes_prompt_contents() {
        let left = tempfile::tempdir().unwrap();
        let right = tempfile::tempdir().unwrap();
        std::fs::write(left.path().join("worker.md"), "left").unwrap();
        std::fs::write(right.path().join("worker.md"), "right").unwrap();

        assert!(!directories_match(left.path(), right.path()).unwrap());
    }
}
