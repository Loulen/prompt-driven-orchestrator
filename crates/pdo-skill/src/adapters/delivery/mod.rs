//! The [`SkillDelivery`] port: the bank's folders frozen into the Run's
//! snapshot, then copied into a worktree with a `.claude/skills` link each,
//! excluded from git per skill (see [`crate::domain::delivery`]). Git is read
//! through the workspace's adapter: the index (`ls-files`), the repository's
//! `info/exclude`, and the worktree's provisioning manifest.

mod tree;

use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use pdo_workspace::adapters::git::manifest::{materialized_paths, record_materialized_paths};
use pdo_workspace::adapters::git::repository::{info_exclude_path, is_tracked};

use crate::adapters::fs::skill_dir;
use crate::application::ports::SkillDelivery;
use crate::domain::delivery::{
    exclusions_to_append, run_exclusions, usable_folder_name, without_run_exclusions,
    DeliveryReport, SkippedSkill,
};
use crate::domain::selection::ActiveSkill;
use crate::domain::skill::SKILL_MD;
use tree::{copy_dir, symlink_dir};

/// The delivery from the bank of the daemon whose repo root is `bank_root`.
pub struct WorktreeDelivery {
    bank_root: PathBuf,
}

impl WorktreeDelivery {
    pub fn new(bank_root: &Path) -> Self {
        Self {
            bank_root: bank_root.to_path_buf(),
        }
    }
}

impl SkillDelivery for WorktreeDelivery {
    fn snapshot(
        &self,
        run_repo_root: &Path,
        run_id: &str,
        skills: &[ActiveSkill],
    ) -> Result<Vec<SkippedSkill>> {
        snapshot_skills(&self.bank_root, run_repo_root, run_id, skills)
    }

    fn deliver(
        &self,
        worktree: &Path,
        snapshot_root: &Path,
        run_id: &str,
        skills: &[ActiveSkill],
    ) -> Result<DeliveryReport> {
        deliver(worktree, snapshot_root, run_id, skills)
    }

    fn withdraw(&self, repo_root: &Path, run_id: &str) -> Result<()> {
        remove_exclusions(repo_root, run_id)
    }
}

/// Copy the bank folders of `skills` into the Run snapshot, **additively**: a
/// skill already snapshotted is left exactly as it is (frozen at the Run), one
/// absent from the bank is reported back (its content cannot be frozen — the
/// spawn will list it as skipped). `bank_root` is the daemon's repo root;
/// `run_repo_root` is the Run's target repo.
pub fn snapshot_skills(
    bank_root: &Path,
    run_repo_root: &Path,
    run_id: &str,
    skills: &[ActiveSkill],
) -> Result<Vec<SkippedSkill>> {
    let root = crate::domain::delivery::snapshot_root(run_repo_root, run_id);
    let mut skipped = Vec::new();
    for skill in skills {
        let dest = root.join(&skill.id);
        if dest.is_dir() {
            continue;
        }
        let source = skill_dir(bank_root, &skill.id);
        if !source.join(SKILL_MD).is_file() {
            skipped.push(SkippedSkill::new(
                skill,
                &skill.name,
                "skill content is absent from the bank",
            ));
            continue;
        }
        // Write beside, then rename: a crash mid-copy never leaves a half
        // snapshot that a later spawn would take for a frozen one.
        let staging = root.join(format!(".{}.tmp", skill.id));
        let _ = std::fs::remove_dir_all(&staging);
        copy_dir(&source, &staging)
            .with_context(|| format!("snapshot skill {} from {}", skill.id, source.display()))?;
        std::fs::rename(&staging, &dest)
            .with_context(|| format!("commit snapshot of skill {}", skill.id))?;
    }
    Ok(skipped)
}

/// Deliver `skills` from the Run snapshot into `worktree`. Idempotent per
/// worktree: a skill already delivered there (recorded in the worktree's
/// provisioning manifest) is counted delivered and left untouched.
pub fn deliver(
    worktree: &Path,
    snapshot_root: &Path,
    run_id: &str,
    skills: &[ActiveSkill],
) -> Result<DeliveryReport> {
    let mut report = DeliveryReport::default();
    if skills.is_empty() {
        return Ok(report);
    }
    let manifest: BTreeSet<Vec<u8>> = materialized_paths(worktree)?.into_iter().collect();
    let mut exclusions: Vec<String> = Vec::new();
    let mut recorded: Vec<String> = Vec::new();

    for skill in skills {
        let name = skill.name.trim();
        if !usable_folder_name(name) {
            report.skipped.push(SkippedSkill::new(
                skill,
                &skill.name,
                "skill name cannot be used as a folder name",
            ));
            continue;
        }
        let agents_rel = format!(".agents/skills/{name}");
        let claude_rel = format!(".claude/skills/{name}");
        let dest = worktree.join(&agents_rel);
        let link = worktree.join(&claude_rel);

        if manifest.contains(agents_rel.as_bytes()) && dest.is_dir() {
            report.delivered.push(name.to_string());
            continue;
        }
        // A versioned homonym (any tracked path under either location) is never
        // touched: the repo's own skill stays tracked, PDO's is skipped, loudly.
        if is_tracked(worktree, &agents_rel)? || is_tracked(worktree, &claude_rel)? {
            report.skipped.push(SkippedSkill::new(
                skill,
                name,
                format!("a versioned skill named '{name}' exists in the target repo"),
            ));
            continue;
        }
        if std::fs::symlink_metadata(&dest).is_ok() {
            report.skipped.push(SkippedSkill::new(
                skill,
                name,
                format!("{agents_rel} already exists in the worktree"),
            ));
            continue;
        }
        let source = snapshot_root.join(&skill.id);
        if !source.join(SKILL_MD).is_file() {
            report.skipped.push(SkippedSkill::new(
                skill,
                name,
                "skill content is absent from the Run snapshot",
            ));
            continue;
        }

        copy_dir(&source, &dest)
            .with_context(|| format!("copy skill '{name}' into {}", dest.display()))?;
        exclusions.push(format!("/{agents_rel}/"));
        recorded.push(agents_rel.clone());

        // The `.claude/skills/<name>` link, relative so it survives a bind mount
        // (sandbox) at another absolute path. A pre-existing `.claude/skills`
        // (folder or symlink) is used as is; an occupied `<name>` slot is never
        // overwritten (it may already resolve to our copy when `.claude/skills`
        // itself links to `.agents/skills`).
        let claude_dir = worktree.join(".claude").join("skills");
        std::fs::create_dir_all(&claude_dir)
            .with_context(|| format!("create {}", claude_dir.display()))?;
        match std::fs::symlink_metadata(&link) {
            Ok(_) => {
                let resolves_to_copy = link
                    .canonicalize()
                    .ok()
                    .zip(dest.canonicalize().ok())
                    .is_some_and(|(a, b)| a == b);
                if !resolves_to_copy {
                    report.skipped.push(SkippedSkill::new(
                        skill,
                        name,
                        format!(
                            "{claude_rel} already exists and was left alone; the skill is only \
                             in {agents_rel}"
                        ),
                    ));
                }
            }
            Err(_) => {
                symlink_dir(Path::new("../../.agents/skills").join(name), &link)
                    .with_context(|| format!("link {}", link.display()))?;
                // The link is only excluded when `.claude/skills` is a real folder
                // of this worktree: through a symlinked `.claude/skills`, Git sees
                // the link blob, not what is behind it.
                if std::fs::symlink_metadata(&claude_dir).is_ok_and(|m| !m.file_type().is_symlink())
                {
                    exclusions.push(format!("/{claude_rel}"));
                    recorded.push(claude_rel.clone());
                }
            }
        }
        report.delivered.push(name.to_string());
    }

    if !exclusions.is_empty() {
        add_exclusions(worktree, run_id, &exclusions)?;
        record_materialized_paths(worktree, &recorded)?;
    }
    Ok(report)
}

/// Remove every `# pdo <run-id>` marker and the path line it introduces from the
/// repo's `info/exclude`. A repo without an exclude file is a no-op.
pub fn remove_exclusions(repo_root: &Path, run_id: &str) -> Result<()> {
    let Some(exclude) = info_exclude_path(repo_root)? else {
        return Ok(());
    };
    let content = match std::fs::read_to_string(&exclude) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e).with_context(|| format!("read {}", exclude.display())),
    };
    std::fs::write(&exclude, without_run_exclusions(&content, run_id))
        .with_context(|| format!("write {}", exclude.display()))
}

/// The exclusion lines of one Run, as currently written.
pub fn exclusions_of(repo_root: &Path, run_id: &str) -> Vec<String> {
    let Ok(Some(exclude)) = info_exclude_path(repo_root) else {
        return Vec::new();
    };
    run_exclusions(
        &std::fs::read_to_string(exclude).unwrap_or_default(),
        run_id,
    )
}

fn add_exclusions(worktree: &Path, run_id: &str, patterns: &[String]) -> Result<()> {
    let Some(exclude) = info_exclude_path(worktree)? else {
        anyhow::bail!(
            "{} is not inside a Git work tree; cannot exclude delivered skills",
            worktree.display()
        );
    };
    let existing = std::fs::read_to_string(&exclude).unwrap_or_default();
    let appended = exclusions_to_append(&existing, run_id, patterns);
    if appended.is_empty() {
        return Ok(());
    }
    if let Some(parent) = exclude.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&exclude)
        .with_context(|| format!("open {}", exclude.display()))?;
    file.write_all(appended.as_bytes())?;
    Ok(())
}
