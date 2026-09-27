//! The migrator on disk: a pipeline file, a registry directory, and the
//! prompts stranded by the flat `.prompts` layout.

use std::path::Path;

use tracing::{info, warn};

use crate::domain::migration::migrate_pipeline_yaml;
use crate::domain::pipeline;

pub fn migrate_pipeline_file(pipeline_path: &Path) -> Result<bool, String> {
    let yaml_text = std::fs::read_to_string(pipeline_path)
        .map_err(|e| format!("read {}: {e}", pipeline_path.display()))?;

    let result = migrate_pipeline_yaml(&yaml_text, pipeline_path)?;
    if !result.migrated {
        return Ok(false);
    }

    for (src, dst) in &result.prompt_moves {
        let dst_path = Path::new(dst);
        if let Some(parent) = dst_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
        }
        let src_path = Path::new(src);
        if src_path.exists() {
            std::fs::rename(src_path, dst_path)
                .map_err(|e| format!("rename {} -> {}: {e}", src, dst))?;
            info!(from = %src, to = %dst, "moved prompt file");
        }
    }

    std::fs::write(pipeline_path, &result.yaml_text)
        .map_err(|e| format!("write {}: {e}", pipeline_path.display()))?;

    info!(path = %pipeline_path.display(), "migrated pipeline YAML");
    Ok(true)
}

#[allow(dead_code)]
pub fn migrate_all(pipelines_dir: &Path) -> Result<usize, String> {
    let mut count = 0;
    let entries = std::fs::read_dir(pipelines_dir)
        .map_err(|e| format!("read dir {}: {e}", pipelines_dir.display()))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("dir entry: {e}"))?;
        let path = entry.path();
        let is_yaml = path
            .extension()
            .is_some_and(|ext| ext == "yaml" || ext == "yml");
        if !is_yaml {
            continue;
        }
        match migrate_pipeline_file(&path) {
            Ok(true) => count += 1,
            Ok(false) => {}
            Err(e) => warn!(path = %path.display(), error = %e, "skipped pipeline migration"),
        }
    }
    Ok(count)
}

/// One-shot boot migration: node prompts stranded in the flat
/// `<pipelines>/prompts/<id>.md` dir (which no reader consults) move to the
/// canonical `<pipelines>/<stem>.prompts/<id>.md`. Node ids are globally unique, so
/// each flat file reattaches to at most one pipeline.
///
/// A flat prompt moves only when the canonical file is *missing*: an existing
/// canonical file is authoritative and never clobbered, and the stale flat
/// duplicate stays put so the conflict remains visible.
///
/// The flat dir is removed only if the migration emptied it; a non-empty remainder
/// is preserved rather than destroyed.
///
/// Returns the number of prompt files moved.
pub fn migrate_stranded_flat_prompts(pipelines_dir: &Path) -> Result<usize, String> {
    let flat_dir = pipelines_dir.join("prompts");
    if !flat_dir.is_dir() {
        return Ok(0);
    }

    let entries = std::fs::read_dir(pipelines_dir)
        .map_err(|e| format!("read dir {}: {e}", pipelines_dir.display()))?;

    let mut moved = 0usize;
    for entry in entries {
        let entry = entry.map_err(|e| format!("dir entry: {e}"))?;
        let path = entry.path();
        let is_yaml = path
            .extension()
            .is_some_and(|ext| ext == "yaml" || ext == "yml");
        if !is_yaml {
            continue;
        }

        let yaml_text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                warn!(path = %path.display(), error = %e,
                    "skipped flat-prompt migration: read failed");
                continue;
            }
        };
        let doc: serde_yaml::Value = match serde_yaml::from_str(&yaml_text) {
            Ok(v) => v,
            Err(e) => {
                warn!(path = %path.display(), error = %e,
                    "skipped flat-prompt migration: YAML parse failed");
                continue;
            }
        };
        let nodes = match doc.get("nodes").and_then(|n| n.as_sequence()) {
            Some(seq) => seq,
            None => continue,
        };

        for node in nodes {
            let Some(node_id) = node.get("id").and_then(|v| v.as_str()) else {
                continue;
            };
            let flat_path = flat_dir.join(format!("{node_id}.md"));
            if !flat_path.is_file() {
                continue;
            }
            let canonical = pipeline::canonical_prompt_path(&path, node_id);
            if canonical.exists() {
                // Canonical is authoritative; never clobber. Leave the dead flat
                // duplicate so the flat dir survives and the conflict is visible.
                warn!(
                    flat = %flat_path.display(),
                    canonical = %canonical.display(),
                    "flat prompt has a canonical counterpart — leaving flat copy in place (#231)"
                );
                continue;
            }
            if let Some(parent) = canonical.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
            }
            std::fs::rename(&flat_path, &canonical).map_err(|e| {
                format!(
                    "rename {} -> {}: {e}",
                    flat_path.display(),
                    canonical.display()
                )
            })?;
            info!(
                from = %flat_path.display(),
                to = %canonical.display(),
                "moved stranded flat prompt to canonical dir (#231)"
            );
            moved += 1;
        }
    }

    // Remove the flat dir only if the migration emptied it. A non-empty
    // remainder means files we could not reattach — preserve them.
    match std::fs::read_dir(&flat_dir) {
        Ok(mut it) => {
            if it.next().is_none() {
                match std::fs::remove_dir(&flat_dir) {
                    Ok(()) => info!(path = %flat_dir.display(),
                        "removed dead flat prompts dir (#231)"),
                    Err(e) => warn!(path = %flat_dir.display(), error = %e,
                        "failed to remove empty flat prompts dir"),
                }
            } else {
                warn!(path = %flat_dir.display(),
                    "flat prompts dir not empty after migration — \
                     leaving unreattached prompts in place (#231)");
            }
        }
        Err(e) => warn!(path = %flat_dir.display(), error = %e,
            "could not re-scan flat prompts dir after migration"),
    }

    Ok(moved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::migration::deterministic_id;

    #[test]
    fn migrate_file_on_disk() {
        let tmp = tempfile::tempdir().unwrap();
        let yaml_path = tmp.path().join("test.yaml");
        let old_prompt_dir = tmp.path().join("old_prompts");
        std::fs::create_dir_all(&old_prompt_dir).unwrap();
        std::fs::write(old_prompt_dir.join("mynode.md"), "hello prompt").unwrap();

        let yaml = "name: test\nversion: '1.0'\nnodes:\n  - id: mynode\n    type: agent\n    isolated_worktree: false\n    prompt_file: old_prompts/mynode.md\n    inputs: []\n    outputs: []\nedges: []\n".to_string();
        std::fs::write(&yaml_path, &yaml).unwrap();

        let migrated = migrate_pipeline_file(&yaml_path).unwrap();
        assert!(migrated);

        let new_yaml = std::fs::read_to_string(&yaml_path).unwrap();
        assert!(!new_yaml.contains("prompt_file"));
        assert!(new_yaml.contains("mynode")); // as name

        let new_id = deterministic_id("mynode");
        let canonical = tmp.path().join(format!("test.prompts/{new_id}.md"));
        assert!(canonical.exists());
        assert_eq!(std::fs::read_to_string(canonical).unwrap(), "hello prompt");

        let not_migrated = migrate_pipeline_file(&yaml_path).unwrap();
        assert!(!not_migrated);
    }

    #[test]
    fn migrate_all_skips_non_yaml() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("readme.md"), "# hi").unwrap();
        std::fs::write(
            tmp.path().join("pipe.yaml"),
            "name: p\nversion: '1.0'\nnodes:\n  - id: n1\n    type: agent\n    isolated_worktree: false\n    inputs: []\n    outputs: []\nedges: []\n",
        )
        .unwrap();

        let count = migrate_all(tmp.path()).unwrap();
        assert_eq!(count, 1);

        let count2 = migrate_all(tmp.path()).unwrap();
        assert_eq!(count2, 0);
    }

    /// Writes a minimal pipeline YAML declaring the given node ids.
    fn write_pipeline_with_nodes(dir: &Path, stem: &str, node_ids: &[&str]) {
        let mut yaml = String::from("name: ");
        yaml.push_str(stem);
        yaml.push_str("\nversion: \"1.0\"\nnodes:\n");
        for id in node_ids {
            yaml.push_str(&format!(
                "  - id: {id}\n    name: {id}\n    type: agent\n    isolated_worktree: false\n    outputs:\n      - name: out\n"
            ));
        }
        yaml.push_str("edges: []\n");
        std::fs::write(dir.join(format!("{stem}.yaml")), yaml).unwrap();
    }

    #[test]
    fn migrate_flat_prompts_moves_to_canonical_and_removes_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        write_pipeline_with_nodes(dir, "auto-issue", &["AAAAAAAA", "BBBBBBBB"]);

        let flat = dir.join("prompts");
        std::fs::create_dir_all(&flat).unwrap();
        std::fs::write(flat.join("AAAAAAAA.md"), "prompt A").unwrap();
        std::fs::write(flat.join("BBBBBBBB.md"), "prompt B").unwrap();

        let moved = migrate_stranded_flat_prompts(dir).unwrap();
        assert_eq!(moved, 2);

        // Both prompts now live in the canonical `<stem>.prompts/` dir, intact.
        let canon = dir.join("auto-issue.prompts");
        assert_eq!(
            std::fs::read_to_string(canon.join("AAAAAAAA.md")).unwrap(),
            "prompt A"
        );
        assert_eq!(
            std::fs::read_to_string(canon.join("BBBBBBBB.md")).unwrap(),
            "prompt B"
        );
        // The emptied flat dir is removed.
        assert!(
            !flat.exists(),
            "flat prompts dir should be removed once emptied"
        );
    }

    #[test]
    fn migrate_flat_prompts_does_not_clobber_existing_canonical() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        write_pipeline_with_nodes(dir, "bugfix", &["CCCCCCCC"]);

        // Canonical already holds authoritative text.
        let canon = dir.join("bugfix.prompts");
        std::fs::create_dir_all(&canon).unwrap();
        std::fs::write(canon.join("CCCCCCCC.md"), "canonical text").unwrap();

        // A stale flat duplicate exists.
        let flat = dir.join("prompts");
        std::fs::create_dir_all(&flat).unwrap();
        std::fs::write(flat.join("CCCCCCCC.md"), "stale flat text").unwrap();

        let moved = migrate_stranded_flat_prompts(dir).unwrap();
        assert_eq!(moved, 0, "must not move when canonical exists");

        // Canonical is untouched; the flat duplicate is preserved (not destroyed),
        // so the conflict stays visible and the dir is kept.
        assert_eq!(
            std::fs::read_to_string(canon.join("CCCCCCCC.md")).unwrap(),
            "canonical text"
        );
        assert!(flat.join("CCCCCCCC.md").exists());
        assert!(flat.exists(), "non-empty flat dir must be preserved");
    }

    #[test]
    fn migrate_flat_prompts_preserves_orphan_for_unknown_node() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        write_pipeline_with_nodes(dir, "alpha", &["KNOWN001"]);

        let flat = dir.join("prompts");
        std::fs::create_dir_all(&flat).unwrap();
        std::fs::write(flat.join("KNOWN001.md"), "known").unwrap();
        // No pipeline declares this id (e.g. a deleted pipeline).
        std::fs::write(flat.join("ORPHAN99.md"), "orphan").unwrap();

        let moved = migrate_stranded_flat_prompts(dir).unwrap();
        assert_eq!(moved, 1);

        assert_eq!(
            std::fs::read_to_string(dir.join("alpha.prompts/KNOWN001.md")).unwrap(),
            "known"
        );
        // The unmatched orphan is preserved, and so is the dir holding it.
        assert!(
            flat.join("ORPHAN99.md").exists(),
            "orphan prompt must not be destroyed"
        );
        assert!(flat.exists());
    }

    #[test]
    fn migrate_flat_prompts_noop_without_flat_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        write_pipeline_with_nodes(dir, "alpha", &["AAAAAAAA"]);
        // No flat `prompts/` dir at all.
        let moved = migrate_stranded_flat_prompts(dir).unwrap();
        assert_eq!(moved, 0);
    }

    #[test]
    fn migrate_flat_prompts_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        write_pipeline_with_nodes(dir, "auto-issue", &["AAAAAAAA"]);

        let flat = dir.join("prompts");
        std::fs::create_dir_all(&flat).unwrap();
        std::fs::write(flat.join("AAAAAAAA.md"), "prompt A").unwrap();

        assert_eq!(migrate_stranded_flat_prompts(dir).unwrap(), 1);
        // Second run: flat dir is gone → no-op.
        assert_eq!(migrate_stranded_flat_prompts(dir).unwrap(), 0);
        assert_eq!(
            std::fs::read_to_string(dir.join("auto-issue.prompts/AAAAAAAA.md")).unwrap(),
            "prompt A"
        );
    }

    #[test]
    fn migrate_flat_prompts_routes_each_id_to_its_owning_pipeline() {
        // Two pipelines, one flat prompt each — each must land in its own
        // canonical dir (ids are globally unique).
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        write_pipeline_with_nodes(dir, "alpha", &["AAAAAAAA"]);
        write_pipeline_with_nodes(dir, "beta", &["BBBBBBBB"]);

        let flat = dir.join("prompts");
        std::fs::create_dir_all(&flat).unwrap();
        std::fs::write(flat.join("AAAAAAAA.md"), "a").unwrap();
        std::fs::write(flat.join("BBBBBBBB.md"), "b").unwrap();

        let moved = migrate_stranded_flat_prompts(dir).unwrap();
        assert_eq!(moved, 2);
        assert_eq!(
            std::fs::read_to_string(dir.join("alpha.prompts/AAAAAAAA.md")).unwrap(),
            "a"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("beta.prompts/BBBBBBBB.md")).unwrap(),
            "b"
        );
        assert!(!flat.exists());
    }
}
