//! **Delivery** of a NodeRun's skills effectifs into the worktree it works in
//! (#672, spec #667, ADR-0062, CONTEXT.md §*Banque de skills* — *Livraison*).
//!
//! One mechanism for every harness: the skills are **copied** into
//! `.agents/skills/<name>/` of the worktree, with one symbolic link per skill in
//! `.claude/skills/<name>` (the only two locations `claude`, `copilot` and
//! `opencode` read without configuration — ADR-0062 measured that `claude` reads
//! `.claude/skills` but not `.agents/skills`). The price — files PDO writes into a
//! repository it does not own — is paid by a **per-skill Git exclusion**:
//!
//! - every delivered path is written to the target repo's `info/exclude`
//!   (shared by all its worktrees, never versioned), one path per skill and never a
//!   parent folder, each preceded by a `# pdo <run-id>` marker line so
//!   [`remove_exclusions`] can take exactly them back at Run cleanup;
//! - every delivered path is also recorded in the worktree's provisioning manifest
//!   (ADR-0061), so the completion commit filters it even when the agent staged it
//!   itself with `git add -A`.
//!
//! A versioned `.agents/skills/` or `.claude/skills/` stays intact and tracked: a
//! skill **homonymous with a versioned one is skipped with a warning**, never
//! overwritten; a pre-existing `.claude/skills` (real folder or symlink) receives
//! the per-skill links beside its own content.
//!
//! **Content is frozen at the Run**: the bank's folders are copied ONCE into the
//! Run's snapshot (`<repo>/.pdo/runs/<run-id>/skills/<id>/`, outside the worktree)
//! and every delivery reads the snapshot, never the bank. The snapshot is
//! **additive** — a node whose selection changed after the Run started gets its new
//! skill snapshotted at its own spawn; nothing is ever purged during the Run.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::skill_selection::EffectiveSkill;

/// Marker written on its own line before each excluded path. The run id follows
/// so the cleanup of one Run never touches another Run's lines.
pub(crate) const EXCLUDE_MARKER: &str = "# pdo";

/// Payload key of the `RunStarted` event carrying the resolved
/// instance + Projet + Run selection (ids, bank names, tiers) frozen at create.
pub(crate) const RUN_STARTED_FROZEN_KEY: &str = "frozen_skills";

/// Where one Run's frozen skill content lives: beside `worktree/`, under the Run
/// directory (removed with it at cleanup), never inside a worktree.
pub(crate) fn snapshot_root(run_repo_root: &Path, run_id: &str) -> PathBuf {
    run_repo_root
        .join(".pdo")
        .join("runs")
        .join(run_id)
        .join("skills")
}

/// A skill the node was promised but that could not be written into the worktree.
/// Never a failure: the node runs without it and the Run view says so.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SkippedSkill {
    pub id: String,
    pub name: String,
    pub reason: String,
}

/// What one [`deliver`] call did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct DeliveryReport {
    /// Names present in the worktree after the call (freshly written or already
    /// delivered by an earlier spawn into the same worktree).
    pub delivered: Vec<String>,
    pub skipped: Vec<SkippedSkill>,
}

/// Copy the bank folders of `skills` into the Run snapshot, **additively**: a
/// skill already snapshotted is left exactly as it is (frozen at the Run), one
/// absent from the bank is reported back (its content cannot be frozen — the spawn
/// will list it as skipped). `bank_root` is the daemon's repo root (where
/// `.pdo/skills/<id>/` lives); `run_repo_root` is the Run's target repo.
pub(crate) fn snapshot_skills(
    bank_root: &Path,
    run_repo_root: &Path,
    run_id: &str,
    skills: &[EffectiveSkill],
) -> Result<Vec<SkippedSkill>> {
    let root = snapshot_root(run_repo_root, run_id);
    let mut skipped = Vec::new();
    for skill in skills {
        let dest = root.join(&skill.id);
        if dest.is_dir() {
            continue;
        }
        let source = crate::skill_bank::skill_dir(bank_root, &skill.id);
        if !source.join(crate::skill_bank::SKILL_MD).is_file() {
            skipped.push(SkippedSkill {
                id: skill.id.clone(),
                name: skill.name.clone(),
                reason: "skill content is absent from the bank".into(),
            });
            continue;
        }
        // Write beside, then rename: a crash mid-copy never leaves a half snapshot
        // that a later spawn would take for a frozen one.
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
pub(crate) fn deliver(
    worktree: &Path,
    snapshot_root: &Path,
    run_id: &str,
    skills: &[EffectiveSkill],
) -> Result<DeliveryReport> {
    let mut report = DeliveryReport::default();
    if skills.is_empty() {
        return Ok(report);
    }
    let manifest: BTreeSet<Vec<u8>> = crate::provisioning::materialized_paths(worktree)?
        .into_iter()
        .collect();
    let mut exclusions: Vec<String> = Vec::new();
    let mut recorded: Vec<String> = Vec::new();

    for skill in skills {
        let name = skill.name.trim();
        if name.is_empty() || name.contains('/') || name.contains('\\') || name.starts_with('.') {
            report.skipped.push(SkippedSkill {
                id: skill.id.clone(),
                name: skill.name.clone(),
                reason: "skill name cannot be used as a folder name".into(),
            });
            continue;
        }
        let agents_rel = format!(".agents/skills/{name}");
        let claude_rel = format!(".claude/skills/{name}");
        let dest = worktree.join(&agents_rel);
        let link = worktree.join(&claude_rel);

        let already_ours = manifest.contains(agents_rel.as_bytes());
        if already_ours && dest.is_dir() {
            report.delivered.push(name.to_string());
            continue;
        }

        // A versioned homonym (any tracked path under either location) is never
        // touched: the repo's own skill stays tracked, PDO's is skipped, loudly.
        if is_tracked(worktree, &agents_rel)? || is_tracked(worktree, &claude_rel)? {
            report.skipped.push(SkippedSkill {
                id: skill.id.clone(),
                name: name.to_string(),
                reason: format!("a versioned skill named '{name}' exists in the target repo"),
            });
            continue;
        }
        if std::fs::symlink_metadata(&dest).is_ok() {
            report.skipped.push(SkippedSkill {
                id: skill.id.clone(),
                name: name.to_string(),
                reason: format!("{agents_rel} already exists in the worktree"),
            });
            continue;
        }

        let source = snapshot_root.join(&skill.id);
        if !source.join(crate::skill_bank::SKILL_MD).is_file() {
            report.skipped.push(SkippedSkill {
                id: skill.id.clone(),
                name: name.to_string(),
                reason: "skill content is absent from the Run snapshot".into(),
            });
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
                    report.skipped.push(SkippedSkill {
                        id: skill.id.clone(),
                        name: name.to_string(),
                        reason: format!(
                            "{claude_rel} already exists and was left alone; the skill is only \
                             in {agents_rel}"
                        ),
                    });
                }
            }
            Err(_) => {
                symlink_dir(Path::new("../../.agents/skills").join(name), &link)
                    .with_context(|| format!("link {}", link.display()))?;
                // The link is only excluded when `.claude/skills` is a real folder of
                // this worktree: through a symlinked `.claude/skills`, Git sees the
                // link blob, not what is behind it.
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
        crate::provisioning::record_materialized_paths(worktree, &recorded)?;
    }
    Ok(report)
}

/// The instance + Projet + Run selection frozen on `RunStarted`: the skills the
/// bank knew at create, and the ids it no longer had (kept so every node of the
/// Run keeps reporting them as missing, with their stored label and tiers). `None`
/// for a Run created before #672 (the spawn then reads those tiers fresh).
pub(crate) fn frozen_run_skills(events: &[crate::event_log::Event]) -> Option<FrozenRunSkills> {
    let payload = events
        .iter()
        .find(|e| e.kind == crate::event_log::EventKind::RunStarted)
        .and_then(|e| e.payload.as_ref())?;
    let skills: Option<Vec<EffectiveSkill>> = payload
        .get(RUN_STARTED_FROZEN_KEY)
        .and_then(|v| serde_json::from_value(v.clone()).ok());
    let missing: Option<Vec<crate::skill_selection::MissingSkill>> = payload
        .get("missing_skills")
        .and_then(|v| serde_json::from_value(v.clone()).ok());
    if skills.is_none() && missing.is_none() {
        return None;
    }
    Some(FrozenRunSkills {
        skills: skills.unwrap_or_default(),
        missing: missing.unwrap_or_default(),
    })
}

/// See [`frozen_run_skills`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct FrozenRunSkills {
    pub skills: Vec<EffectiveSkill>,
    pub missing: Vec<crate::skill_selection::MissingSkill>,
}

/// Remove every `# pdo <run-id>` marker and the path line it introduces from the
/// repo's `info/exclude`. Lines PDO did not write — and other Runs' lines — stay
/// byte for byte. A repo without an exclude file is a no-op.
pub(crate) fn remove_exclusions(repo_root: &Path, run_id: &str) -> Result<()> {
    let Some(exclude) = exclude_path(repo_root)? else {
        return Ok(());
    };
    let content = match std::fs::read_to_string(&exclude) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e).with_context(|| format!("read {}", exclude.display())),
    };
    let marker = format!("{EXCLUDE_MARKER} {run_id}");
    let mut kept: Vec<&str> = Vec::new();
    let mut lines = content.lines().peekable();
    while let Some(line) = lines.next() {
        if line.trim_end() == marker {
            lines.next();
            continue;
        }
        kept.push(line);
    }
    let mut out = kept.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    std::fs::write(&exclude, out).with_context(|| format!("write {}", exclude.display()))
}

/// The exclusion lines of one Run, as currently written (tests and diagnostics).
#[cfg(test)]
pub(crate) fn exclusions_of(repo_root: &Path, run_id: &str) -> Vec<String> {
    let Ok(Some(exclude)) = exclude_path(repo_root) else {
        return Vec::new();
    };
    let content = std::fs::read_to_string(exclude).unwrap_or_default();
    let marker = format!("{EXCLUDE_MARKER} {run_id}");
    let mut out = Vec::new();
    let mut lines = content.lines().peekable();
    while let Some(line) = lines.next() {
        if line.trim_end() == marker {
            if let Some(path) = lines.next() {
                out.push(path.to_string());
            }
        }
    }
    out
}

fn add_exclusions(worktree: &Path, run_id: &str, patterns: &[String]) -> Result<()> {
    let Some(exclude) = exclude_path(worktree)? else {
        anyhow::bail!(
            "{} is not inside a Git work tree; cannot exclude delivered skills",
            worktree.display()
        );
    };
    let existing = std::fs::read_to_string(&exclude).unwrap_or_default();
    let marker = format!("{EXCLUDE_MARKER} {run_id}");
    let present: BTreeSet<&str> = {
        let mut set = BTreeSet::new();
        let mut lines = existing.lines().peekable();
        while let Some(line) = lines.next() {
            if line.trim_end() == marker {
                if let Some(path) = lines.next() {
                    set.insert(path.trim_end());
                }
            }
        }
        set
    };
    let missing: Vec<&String> = patterns
        .iter()
        .filter(|p| !present.contains(p.as_str()))
        .collect();
    if missing.is_empty() {
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
    if !existing.is_empty() && !existing.ends_with('\n') {
        file.write_all(b"\n")?;
    }
    for pattern in missing {
        writeln!(file, "{marker}")?;
        writeln!(file, "{pattern}")?;
    }
    Ok(())
}

/// `<common git dir>/info/exclude` of the repository `dir` belongs to (a worktree
/// resolves to the shared common dir). `None` outside a work tree.
fn exclude_path(dir: &Path) -> Result<Option<PathBuf>> {
    let output = std::process::Command::new("git")
        .args([
            "rev-parse",
            "--is-inside-work-tree",
            "--git-path",
            "info/exclude",
        ])
        .current_dir(dir)
        .output()
        .with_context(|| format!("locate Git metadata for {}", dir.display()))?;
    if !output.status.success() {
        return Ok(None);
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut lines = text.lines();
    if lines.next() != Some("true") {
        return Ok(None);
    }
    let Some(path) = lines.next().filter(|l| !l.is_empty()) else {
        return Ok(None);
    };
    let path = PathBuf::from(path);
    Ok(Some(if path.is_absolute() {
        path
    } else {
        dir.join(path)
    }))
}

fn is_tracked(worktree: &Path, rel: &str) -> Result<bool> {
    let output = std::process::Command::new("git")
        .args(["ls-files", "-z", "--", rel])
        .current_dir(worktree)
        .output()
        .with_context(|| format!("git ls-files {rel} in {}", worktree.display()))?;
    Ok(output.status.success() && !output.stdout.is_empty())
}

/// Recursive copy that reproduces symlinks as symlinks (never dereferenced) and
/// refuses nothing else: a skill folder is plain files.
fn copy_dir(source: &Path, dest: &Path) -> Result<()> {
    std::fs::create_dir_all(dest).with_context(|| format!("create {}", dest.display()))?;
    for entry in std::fs::read_dir(source).with_context(|| format!("read {}", source.display()))? {
        let entry = entry?;
        let from = entry.path();
        let to = dest.join(entry.file_name());
        let meta = std::fs::symlink_metadata(&from)?;
        if meta.file_type().is_symlink() {
            let target = std::fs::read_link(&from)?;
            symlink_any(&target, &to)?;
        } else if meta.is_dir() {
            copy_dir(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)
                .with_context(|| format!("copy {} to {}", from.display(), to.display()))?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn symlink_dir(target: impl AsRef<Path>, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn symlink_dir(target: impl AsRef<Path>, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_dir(target, link)
}

#[cfg(unix)]
fn symlink_any(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn symlink_any(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_file(target, link)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skill_selection::SkillTier;

    fn git(dir: &Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// A repo with one commit, plus a linked worktree on a `pdo/run-<id>` branch —
    /// the shape a Run works in.
    fn repo_and_worktree(run_id: &str) -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "user.email", "t@e.com"]);
        git(&repo, &["config", "user.name", "T"]);
        git(&repo, &["config", "commit.gpgsign", "false"]);
        std::fs::write(repo.join("README.md"), "hi\n").unwrap();
        std::fs::write(repo.join(".gitignore"), ".pdo/runs/\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", "init"]);
        let wt = repo.join(".pdo").join("runs").join(run_id).join("worktree");
        std::fs::create_dir_all(wt.parent().unwrap()).unwrap();
        let branch = format!("pdo/run-{run_id}");
        git(
            &repo,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                &branch,
                wt.to_str().unwrap(),
                "main",
            ],
        );
        (tmp, wt)
    }

    fn bank_with(tmp: &Path, id: &str, name: &str) -> PathBuf {
        let bank_root = tmp.join("bank");
        let dir = crate::skill_bank::skill_dir(&bank_root, id);
        std::fs::create_dir_all(dir.join("references")).unwrap();
        std::fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: d\n---\n\n# {name}\n"),
        )
        .unwrap();
        std::fs::write(dir.join("references").join("notes.md"), "ref\n").unwrap();
        bank_root
    }

    fn eff(id: &str, name: &str) -> EffectiveSkill {
        EffectiveSkill {
            id: id.into(),
            name: name.into(),
            tiers: vec![SkillTier::Instance],
        }
    }

    fn repo_of(wt: &Path) -> PathBuf {
        wt.ancestors().nth(4).unwrap().to_path_buf()
    }

    #[test]
    fn delivers_copy_and_link_and_keeps_git_status_clean() {
        let (tmp, wt) = repo_and_worktree("r1");
        let repo = repo_of(&wt);
        let bank = bank_with(tmp.path(), "s1", "tdd");
        let skills = [eff("s1", "tdd")];
        let skipped = snapshot_skills(&bank, &repo, "r1", &skills).unwrap();
        assert!(skipped.is_empty());
        let report = deliver(&wt, &snapshot_root(&repo, "r1"), "r1", &skills).unwrap();
        assert_eq!(report.delivered, vec!["tdd".to_string()]);
        assert!(report.skipped.is_empty());

        assert!(wt.join(".agents/skills/tdd/SKILL.md").is_file());
        assert!(wt.join(".agents/skills/tdd/references/notes.md").is_file());
        let link = wt.join(".claude/skills/tdd");
        assert!(std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert!(link.join("SKILL.md").is_file(), "link resolves to the copy");

        assert_eq!(
            git(&wt, &["status", "--porcelain"]),
            "",
            "worktree is clean"
        );
        // Even an agent's `git add -A && git commit` cannot take the skill along.
        git(&wt, &["add", "-A"]);
        assert_eq!(git(&wt, &["diff", "--cached", "--name-only"]), "");

        let excl = exclusions_of(&repo, "r1");
        assert_eq!(
            excl,
            vec![
                "/.agents/skills/tdd/".to_string(),
                "/.claude/skills/tdd".to_string()
            ]
        );
        // The provisioning manifest lists both, so the completion commit filters them.
        let recorded = crate::provisioning::materialized_paths(&wt).unwrap();
        assert!(recorded.contains(&b".agents/skills/tdd".to_vec()));
        assert!(recorded.contains(&b".claude/skills/tdd".to_vec()));
    }

    #[test]
    fn a_second_delivery_into_the_same_worktree_is_idempotent() {
        let (tmp, wt) = repo_and_worktree("r2");
        let repo = repo_of(&wt);
        let bank = bank_with(tmp.path(), "s1", "tdd");
        let skills = [eff("s1", "tdd")];
        snapshot_skills(&bank, &repo, "r2", &skills).unwrap();
        let root = snapshot_root(&repo, "r2");
        deliver(&wt, &root, "r2", &skills).unwrap();
        let report = deliver(&wt, &root, "r2", &skills).unwrap();
        assert_eq!(report.delivered, vec!["tdd".to_string()]);
        assert!(report.skipped.is_empty());
        assert_eq!(exclusions_of(&repo, "r2").len(), 2, "no duplicated lines");
    }

    #[test]
    fn a_versioned_homonym_is_kept_tracked_and_the_pdo_skill_is_skipped() {
        let (tmp, wt) = repo_and_worktree("r3");
        let repo = repo_of(&wt);
        // The target repo versions its own `.agents/skills/x`.
        std::fs::create_dir_all(repo.join(".agents/skills/x")).unwrap();
        std::fs::write(repo.join(".agents/skills/x/SKILL.md"), "repo's own\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", "own skill"]);
        git(&wt, &["merge", "-q", "main"]);

        let bank = bank_with(tmp.path(), "sx", "x");
        let skills = [eff("sx", "x")];
        snapshot_skills(&bank, &repo, "r3", &skills).unwrap();
        let report = deliver(&wt, &snapshot_root(&repo, "r3"), "r3", &skills).unwrap();
        assert!(report.delivered.is_empty());
        assert_eq!(report.skipped.len(), 1);
        assert!(report.skipped[0].reason.contains("versioned"));

        assert_eq!(
            std::fs::read_to_string(wt.join(".agents/skills/x/SKILL.md")).unwrap(),
            "repo's own\n"
        );
        assert!(!wt.join(".claude/skills/x").exists());
        assert!(
            exclusions_of(&repo, "r3").is_empty(),
            "nothing of x is ignored"
        );
        assert_eq!(
            git(&wt, &["ls-files", ".agents/skills/x"]).trim(),
            ".agents/skills/x/SKILL.md"
        );
    }

    #[test]
    fn an_existing_claude_skills_folder_receives_the_links_without_being_overwritten() {
        let (tmp, wt) = repo_and_worktree("r4");
        let repo = repo_of(&wt);
        std::fs::create_dir_all(repo.join(".claude/skills/own")).unwrap();
        std::fs::write(repo.join(".claude/skills/own/SKILL.md"), "own\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", "own claude skill"]);
        git(&wt, &["merge", "-q", "main"]);

        let bank = bank_with(tmp.path(), "s1", "tdd");
        let skills = [eff("s1", "tdd")];
        snapshot_skills(&bank, &repo, "r4", &skills).unwrap();
        let report = deliver(&wt, &snapshot_root(&repo, "r4"), "r4", &skills).unwrap();
        assert_eq!(report.delivered, vec!["tdd".to_string()]);
        assert!(wt.join(".claude/skills/own/SKILL.md").is_file());
        assert!(wt.join(".claude/skills/tdd/SKILL.md").is_file());
        assert_eq!(git(&wt, &["status", "--porcelain"]), "");
    }

    #[test]
    fn a_symlinked_claude_skills_pointing_at_agents_skills_needs_no_second_link() {
        let (tmp, wt) = repo_and_worktree("r5");
        let repo = repo_of(&wt);
        std::fs::create_dir_all(repo.join(".claude")).unwrap();
        symlink_dir("../.agents/skills", &repo.join(".claude/skills")).unwrap();
        std::fs::create_dir_all(repo.join(".agents/skills")).unwrap();
        std::fs::write(repo.join(".agents/skills/.keep"), "").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", "symlinked claude skills"]);
        git(&wt, &["merge", "-q", "main"]);

        let bank = bank_with(tmp.path(), "s1", "tdd");
        let skills = [eff("s1", "tdd")];
        snapshot_skills(&bank, &repo, "r5", &skills).unwrap();
        let report = deliver(&wt, &snapshot_root(&repo, "r5"), "r5", &skills).unwrap();
        assert_eq!(report.delivered, vec!["tdd".to_string()]);
        assert!(report.skipped.is_empty());
        assert!(wt.join(".claude/skills/tdd/SKILL.md").is_file());
        assert_eq!(git(&wt, &["status", "--porcelain"]), "");
        assert_eq!(
            exclusions_of(&repo, "r5"),
            vec!["/.agents/skills/tdd/".to_string()]
        );
    }

    #[test]
    fn cleanup_removes_only_this_runs_marked_lines() {
        let (tmp, wt) = repo_and_worktree("r6");
        let repo = repo_of(&wt);
        let exclude = exclude_path(&repo).unwrap().unwrap();
        std::fs::create_dir_all(exclude.parent().unwrap()).unwrap();
        std::fs::write(
            &exclude,
            "# user line\n*.swp\n# pdo other-run\n/.agents/skills/zzz/\n",
        )
        .unwrap();

        let bank = bank_with(tmp.path(), "s1", "tdd");
        let skills = [eff("s1", "tdd")];
        snapshot_skills(&bank, &repo, "r6", &skills).unwrap();
        deliver(&wt, &snapshot_root(&repo, "r6"), "r6", &skills).unwrap();
        assert_eq!(exclusions_of(&repo, "r6").len(), 2);

        remove_exclusions(&repo, "r6").unwrap();
        assert_eq!(
            std::fs::read_to_string(&exclude).unwrap(),
            "# user line\n*.swp\n# pdo other-run\n/.agents/skills/zzz/\n"
        );
        assert!(exclusions_of(&repo, "r6").is_empty());
        // Idempotent, and harmless on a repo with no exclude file at all.
        remove_exclusions(&repo, "r6").unwrap();
        let bare = tempfile::tempdir().unwrap();
        remove_exclusions(bare.path(), "r6").unwrap();
    }

    #[test]
    fn snapshot_is_frozen_and_additive() {
        let (tmp, wt) = repo_and_worktree("r7");
        let repo = repo_of(&wt);
        let bank = bank_with(tmp.path(), "s1", "tdd");
        let first = [eff("s1", "tdd")];
        snapshot_skills(&bank, &repo, "r7", &first).unwrap();
        // Editing the bank after the Run started changes nothing delivered.
        std::fs::write(
            crate::skill_bank::skill_dir(&bank, "s1").join("SKILL.md"),
            "---\nname: tdd\ndescription: edited\n---\n",
        )
        .unwrap();
        snapshot_skills(&bank, &repo, "r7", &first).unwrap();
        deliver(&wt, &snapshot_root(&repo, "r7"), "r7", &first).unwrap();
        let delivered = std::fs::read_to_string(wt.join(".agents/skills/tdd/SKILL.md")).unwrap();
        assert!(
            delivered.contains("description: d"),
            "frozen content, not the edit"
        );

        // A node whose selection adds a skill later gets it snapshotted (additive).
        bank_with(tmp.path(), "s2", "grilling");
        let both = [eff("s1", "tdd"), eff("s2", "grilling")];
        let skipped = snapshot_skills(&bank, &repo, "r7", &both).unwrap();
        assert!(skipped.is_empty());
        assert!(snapshot_root(&repo, "r7").join("s2/SKILL.md").is_file());
        let report = deliver(&wt, &snapshot_root(&repo, "r7"), "r7", &both).unwrap();
        assert_eq!(
            report.delivered,
            vec!["tdd".to_string(), "grilling".to_string()]
        );

        // A skill deleted from the bank before it was ever snapshotted is reported.
        let gone = [eff("s3", "gone")];
        let skipped = snapshot_skills(&bank, &repo, "r7", &gone).unwrap();
        assert_eq!(skipped.len(), 1);
        let report = deliver(&wt, &snapshot_root(&repo, "r7"), "r7", &gone).unwrap();
        assert_eq!(report.skipped.len(), 1);
        assert!(report.skipped[0].reason.contains("snapshot"));
    }

    #[test]
    fn the_completion_commit_filters_a_skill_the_agent_force_added() {
        let (tmp, wt) = repo_and_worktree("r8");
        let repo = repo_of(&wt);
        let bank = bank_with(tmp.path(), "s1", "tdd");
        let skills = [eff("s1", "tdd")];
        snapshot_skills(&bank, &repo, "r8", &skills).unwrap();
        deliver(&wt, &snapshot_root(&repo, "r8"), "r8", &skills).unwrap();
        // The agent bypasses the exclusion on purpose and leaves real work too.
        git(
            &wt,
            &["add", "-f", ".agents/skills/tdd", ".claude/skills/tdd"],
        );
        std::fs::write(wt.join("work.txt"), "done\n").unwrap();
        let sha = crate::worktree_ops::stage_and_commit(&wt, "n", 1).unwrap();
        assert!(sha.is_some());
        let files = git(&wt, &["show", "--name-only", "--format=", "HEAD"]);
        assert_eq!(files.trim(), "work.txt");
        assert_eq!(git(&wt, &["status", "--porcelain"]), "");
    }
}
