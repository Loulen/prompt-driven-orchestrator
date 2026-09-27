//! Provisioning on disk: list the source repository, resolve the rules over it,
//! and materialize the plan into a worktree (copy, hardlink, symlink).

use std::path::Path;

use anyhow::{Context, Result};

use crate::adapters::git::manifest::{git_tracked_paths, record_materialized_paths};
use crate::domain::provisioning::{
    ProvisioningMode, ProvisioningPlan, ProvisioningRules, ProvisioningScope, ScopedRules,
};

fn repository_paths(repository: &Path) -> Result<Vec<(String, bool)>> {
    fn visit(root: &Path, dir: &Path, out: &mut Vec<(String, bool)>) -> Result<()> {
        for entry in std::fs::read_dir(dir)
            .with_context(|| format!("read provisioning source {}", dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            let rel = path
                .strip_prefix(root)
                .expect("walk stays below root")
                .to_string_lossy()
                .replace('\\', "/");
            if dir == root && (rel == ".git" || rel == ".pdo") {
                continue;
            }
            let ty = entry.file_type()?;
            if ty.is_dir() {
                out.push((rel, true));
                visit(root, &path, out)?;
            } else {
                out.push((rel, false));
            }
        }
        Ok(())
    }

    let mut paths = Vec::new();
    visit(repository, repository, &mut paths)?;
    paths.sort();
    Ok(paths)
}

/// Resolve `scoped` over `repository`, flagging what git provides at `git_ref`.
pub fn resolve_at_git_ref(
    repository: &Path,
    scoped: &[ScopedRules],
    git_ref: &str,
) -> Result<ProvisioningPlan> {
    let paths = repository_paths(repository)?;
    let git_tracked = git_tracked_paths(repository, git_ref)?;
    crate::domain::provisioning::resolve(repository, &paths, &git_tracked, scoped)
}

/// [`resolve_at_git_ref`] at `HEAD`.
pub fn resolve(repository: &Path, scoped: &[ScopedRules]) -> Result<ProvisioningPlan> {
    resolve_at_git_ref(repository, scoped, "HEAD")
}

fn ensure_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create provisioning parent {}", parent.display()))?;
    }
    Ok(())
}

pub fn provision_missing(
    repository: &Path,
    worktree: &Path,
    plan: &ProvisioningPlan,
) -> Result<()> {
    if let Some(conflict) = plan.conflicts.first() {
        anyhow::bail!(
            "mode conflict in {:?}: {} is declared in {} modes",
            conflict.scope,
            conflict.relative_path,
            conflict.modes.len()
        );
    }

    let mut materialized = Vec::new();
    for entry in &plan.entries {
        let source = repository.join(&entry.relative_path);
        let target = worktree.join(&entry.relative_path);
        if std::fs::symlink_metadata(&target).is_ok() {
            continue;
        }
        ensure_parent(&target)?;
        let metadata = std::fs::symlink_metadata(&source)
            .with_context(|| format!("inspect provisioning source {}", source.display()))?;
        let effect = match entry.mode {
            ProvisioningMode::Copy if metadata.file_type().is_symlink() => {
                let link_target = std::fs::read_link(&source)?;
                #[cfg(unix)]
                {
                    std::os::unix::fs::symlink(link_target, &target)
                }
                #[cfg(not(unix))]
                {
                    std::fs::copy(&source, &target).map(|_| ())
                }
            }
            ProvisioningMode::Copy => std::fs::copy(&source, &target).map(|_| ()),
            ProvisioningMode::Hardlink => std::fs::hard_link(&source, &target),
            ProvisioningMode::Symlink => {
                #[cfg(unix)]
                {
                    std::os::unix::fs::symlink(&source, &target)
                }
                #[cfg(not(unix))]
                {
                    std::os::windows::fs::symlink_file(&source, &target)
                }
            }
        };
        effect.with_context(|| {
            format!(
                "{} `{}` from {} to {}",
                entry.mode,
                entry.relative_path,
                source.display(),
                target.display()
            )
        })?;
        materialized.push(entry.relative_path.clone());
    }
    record_materialized_paths(worktree, &materialized)?;
    Ok(())
}

pub fn provision_node_worktree(
    repository: &Path,
    worktree: &Path,
    inherited: &[ScopedRules],
    node_rules: &ProvisioningRules,
    git_ref: &str,
) -> Result<ProvisioningPlan> {
    let mut scoped = inherited.to_vec();
    if !node_rules.is_empty() {
        scoped.push(ScopedRules {
            scope: ProvisioningScope::IsolatedNode,
            rules: node_rules.clone(),
        });
    }
    let plan = resolve_at_git_ref(repository, &scoped, git_ref)?;
    provision_missing(repository, worktree, &plan)?;
    Ok(plan)
}
