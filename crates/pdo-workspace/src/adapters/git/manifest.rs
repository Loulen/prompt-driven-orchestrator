//! The provisioning manifest: which paths of a worktree PDO materialized, kept
//! in the worktree's private git dir so the delivery never commits them
//! (ADR-0061), and the paths git itself provides at a ref.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};

pub fn git_tracked_paths(repository: &Path, git_ref: &str) -> Result<BTreeSet<String>> {
    if !repository.join(".git").exists() {
        return Ok(BTreeSet::new());
    }
    let output = std::process::Command::new("git")
        .args(["ls-tree", "-r", "--name-only", "-z", git_ref])
        .current_dir(repository)
        .output()
        .with_context(|| format!("list Git-provided paths in {}", repository.display()))?;
    if !output.status.success() {
        anyhow::bail!(
            "list Git-provided paths in {}: {}",
            repository.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| String::from_utf8_lossy(path).replace('\\', "/"))
        .collect())
}

const MATERIALIZED_PATHS_FILE: &str = "pdo/provisioned-paths-v1";

fn parse_materialized_paths(bytes: &[u8]) -> BTreeSet<Vec<u8>> {
    bytes
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(<[u8]>::to_vec)
        .collect()
}

fn provisioning_manifest_path(worktree: &Path) -> Result<Option<std::path::PathBuf>> {
    let output = std::process::Command::new("git")
        .args([
            "rev-parse",
            "--is-inside-work-tree",
            "--git-path",
            MATERIALIZED_PATHS_FILE,
        ])
        .current_dir(worktree)
        .output()
        .with_context(|| format!("locate Git metadata for {}", worktree.display()))?;
    if !output.status.success() {
        return Ok(None);
    }
    let mut lines = output.stdout.split(|byte| *byte == b'\n');
    if lines.next() != Some(b"true".as_slice()) {
        return Ok(None);
    }
    let path = lines
        .next()
        .filter(|line| !line.is_empty())
        .context("git rev-parse did not return a provisioning manifest path")?;
    let path = std::path::PathBuf::from(String::from_utf8_lossy(path).into_owned());
    Ok(Some(if path.is_absolute() {
        path
    } else {
        worktree.join(path)
    }))
}

pub fn record_materialized_paths(worktree: &Path, paths: &[String]) -> Result<()> {
    let Some(manifest) = provisioning_manifest_path(worktree)? else {
        return Ok(());
    };
    let mut materialized: BTreeSet<Vec<u8>> = match std::fs::read(&manifest) {
        Ok(bytes) => parse_materialized_paths(&bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeSet::new(),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("read provisioning metadata {}", manifest.display()));
        }
    };
    materialized.extend(paths.iter().map(|path| path.as_bytes().to_vec()));
    if let Some(parent) = manifest.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create provisioning metadata {}", parent.display()))?;
    }
    let mut file = std::fs::File::create(&manifest)
        .with_context(|| format!("write provisioning metadata {}", manifest.display()))?;
    for path in materialized {
        file.write_all(&path)?;
        file.write_all(&[0])?;
    }
    Ok(())
}

pub fn materialized_paths(worktree: &Path) -> Result<Vec<Vec<u8>>> {
    let Some(manifest) = provisioning_manifest_path(worktree)? else {
        return Ok(Vec::new());
    };
    let bytes = match std::fs::read(&manifest) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("read provisioning metadata {}", manifest.display()));
        }
    };
    Ok(parse_materialized_paths(&bytes).into_iter().collect())
}
