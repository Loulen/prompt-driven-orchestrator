//! The files a node's inputs and outputs are: which are tracked by git, which
//! were inherited from upstream, and what each one holds.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use pdo_pipeline::domain::pipeline;

use super::*;

/// The output files of `node_id` at `iter` that git tracks **unmodified** in
/// the worktree holding `artifacts_dir` (#796), as absolute paths. PDO never
/// commits `.pdo/artifacts/` (ADR-0060), so a tracked artifact came in with the
/// checkout — a previous run's report, inherited through the source branch —
/// whatever its mtime says. A tracked file the working copy has since
/// **modified** (or deleted) is left out: the current execution rewrote it, so
/// "tracked" no longer means "came with the checkout" and the mtime gate takes
/// over (FP1d — the same port path is reused by every run of a pipeline).
/// Two `git ls-files` per call; an artifacts dir outside any repository (unit
/// tests, a broken worktree) yields the empty set, which leaves the mtime gate
/// as the only signal.
pub fn git_tracked_paths(artifacts_dir: &Path, node_id: &str, iter: i64) -> HashSet<PathBuf> {
    let rel = format!("{node_id}/iter-{iter}");
    let ls_files = |flag: Option<&str>| -> Option<HashSet<PathBuf>> {
        let mut cmd = std::process::Command::new("git");
        cmd.arg("ls-files").arg("-z");
        if let Some(flag) = flag {
            cmd.arg(flag);
        }
        let output = cmd
            .args(["--", &rel])
            .current_dir(artifacts_dir)
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        Some(
            output
                .stdout
                .split(|b| *b == 0)
                .filter(|p| !p.is_empty())
                .map(|p| artifacts_dir.join(String::from_utf8_lossy(p).as_ref()))
                .collect(),
        )
    };
    let Some(mut tracked) = ls_files(None) else {
        return HashSet::new();
    };
    if tracked.is_empty() {
        return tracked;
    }
    // `-m`: tracked paths whose working copy differs from the index (modified
    // or deleted). Without the verdict, be safe and trust nothing from git.
    let Some(modified) = ls_files(Some("-m")) else {
        return HashSet::new();
    };
    tracked.retain(|p| !modified.contains(p));
    tracked
}

/// Is `path` a previous execution's file rather than this one's (#796)? Yes
/// when git tracks it unmodified (see [`git_tracked_paths`]) or when its mtime predates
/// `not_before` — `None` when the execution start is unknown, leaving only the
/// git signal.
pub fn is_inherited(
    path: &Path,
    not_before: Option<chrono::DateTime<chrono::Utc>>,
    tracked: &HashSet<PathBuf>,
) -> bool {
    tracked.contains(path) || not_before.is_some_and(|start| predates(path, start))
}

pub(super) fn mark_inherited(
    artifacts_dir: &Path,
    files: &mut [FileInfo],
    not_before: Option<chrono::DateTime<chrono::Utc>>,
    tracked: &HashSet<PathBuf>,
) {
    if not_before.is_none() && tracked.is_empty() {
        return;
    }
    for f in files.iter_mut().filter(|f| f.exists) {
        f.inherited = is_inherited(&artifacts_dir.join(&f.path), not_before, tracked);
    }
}

pub(super) fn relative_path(artifacts_dir: &Path, abs_path: &Path) -> String {
    abs_path
        .strip_prefix(artifacts_dir)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| abs_path.to_string_lossy().to_string())
}

/// The non-image attachments of `_input/` as `FileInfo`s (#779) — the same
/// discovery the preamble's `## Input Files` uses, so the two never disagree.
pub(super) fn input_attachment_files(artifacts_dir: &Path) -> Vec<FileInfo> {
    let input_dir = artifacts_dir.join("_input");
    crate::application::node::prompt::discover_input_files(artifacts_dir)
        .iter()
        .map(|name| file_info(artifacts_dir, &input_dir.join(name)))
        .collect()
}

pub(super) fn file_info(artifacts_dir: &Path, path: &Path) -> FileInfo {
    let exists = path.exists();
    let size = if exists {
        std::fs::metadata(path).ok().map(|m| m.len())
    } else {
        None
    };
    FileInfo {
        path: relative_path(artifacts_dir, path),
        exists,
        size,
        frontmatter: None,
        inherited: false,
    }
}

pub(super) fn file_info_with_frontmatter(artifacts_dir: &Path, path: &Path) -> FileInfo {
    let exists = path.exists();
    let size = if exists {
        std::fs::metadata(path).ok().map(|m| m.len())
    } else {
        None
    };
    let frontmatter = if exists {
        pdo_pipeline::adapters::frontmatter_file::parse_frontmatter_from_file(path)
            .ok()
            .filter(|m| !m.is_empty())
            .map(|m| {
                m.into_iter()
                    .map(|(k, v)| {
                        let json_val = serde_yaml_to_json(&v);
                        (k, json_val)
                    })
                    .collect()
            })
    } else {
        None
    };
    FileInfo {
        path: relative_path(artifacts_dir, path),
        exists,
        size,
        frontmatter,
        inherited: false,
    }
}

pub(super) fn serde_yaml_to_json(v: &serde_yaml::Value) -> serde_json::Value {
    match v {
        serde_yaml::Value::Null => serde_json::Value::Null,
        serde_yaml::Value::Bool(b) => serde_json::Value::Bool(*b),
        serde_yaml::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                serde_json::Value::Number(i.into())
            } else if let Some(f) = n.as_f64() {
                serde_json::Number::from_f64(f)
                    .map(serde_json::Value::Number)
                    .unwrap_or(serde_json::Value::Null)
            } else {
                serde_json::Value::Null
            }
        }
        serde_yaml::Value::String(s) => serde_json::Value::String(s.clone()),
        serde_yaml::Value::Sequence(seq) => {
            serde_json::Value::Array(seq.iter().map(serde_yaml_to_json).collect())
        }
        serde_yaml::Value::Mapping(map) => {
            let obj: serde_json::Map<String, serde_json::Value> = map
                .iter()
                .filter_map(|(k, v)| k.as_str().map(|s| (s.to_string(), serde_yaml_to_json(v))))
                .collect();
            serde_json::Value::Object(obj)
        }
        serde_yaml::Value::Tagged(tagged) => serde_yaml_to_json(&tagged.value),
    }
}

pub(super) fn list_image_files(artifacts_dir: &Path, port_dir: &Path) -> Vec<FileInfo> {
    let Ok(entries) = std::fs::read_dir(port_dir) else {
        return vec![];
    };
    let mut files: Vec<FileInfo> = entries
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_type().ok().is_some_and(|ft| ft.is_file()) && pipeline::is_image_file(&e.path())
        })
        .map(|e| {
            let path = e.path();
            let size = std::fs::metadata(&path).ok().map(|m| m.len());
            FileInfo {
                path: relative_path(artifacts_dir, &path),
                exists: true,
                size,
                frontmatter: None,
                inherited: false,
            }
        })
        .collect();
    files.sort_by(|a, b| a.path.cmp(&b.path));
    files
}
