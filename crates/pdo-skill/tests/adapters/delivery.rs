//! The delivery into real git worktrees: copy + link, per-skill exclusion,
//! provisioning manifest, frozen snapshot.

use std::path::{Path, PathBuf};

use pdo_skill::adapters::delivery::{
    deliver, exclusions_of, remove_exclusions, snapshot_skills, WorktreeDelivery,
};
use pdo_skill::adapters::fs::skill_dir;
use pdo_skill::application::ports::SkillDelivery;
use pdo_skill::domain::delivery::snapshot_root;
use pdo_skill::domain::selection::{ActiveSkill, SkillTier};
use pdo_workspace::adapters::git::manifest::materialized_paths;
use pdo_workspace::adapters::git::repository::info_exclude_path;
use pdo_workspace::adapters::git::worktrees::stage_and_commit;

use crate::common::{git, repo_and_worktree, repo_of, write_skill};

fn bank_with(tmp: &Path, id: &str, name: &str) -> PathBuf {
    let bank_root = tmp.join("bank");
    write_skill(&skill_dir(&bank_root, id), name);
    bank_root
}

fn eff(id: &str, name: &str) -> ActiveSkill {
    ActiveSkill {
        id: id.into(),
        name: name.into(),
        tiers: vec![SkillTier::Instance],
    }
}

#[cfg(unix)]
fn symlink(target: &str, link: &Path) {
    std::os::unix::fs::symlink(target, link).unwrap();
}

#[test]
fn delivers_copy_and_link_and_keeps_git_status_clean() {
    let (tmp, wt) = repo_and_worktree("r1");
    let repo = repo_of(&wt);
    let bank = bank_with(tmp.path(), "s1", "tdd");
    let skills = [eff("s1", "tdd")];
    let port = WorktreeDelivery::new(&bank);
    assert!(port.snapshot(&repo, "r1", &skills).unwrap().is_empty());
    let report = port
        .deliver(&wt, &snapshot_root(&repo, "r1"), "r1", &skills)
        .unwrap();
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

    assert_eq!(
        exclusions_of(&repo, "r1"),
        vec![
            "/.agents/skills/tdd/".to_string(),
            "/.claude/skills/tdd".to_string()
        ]
    );
    // The provisioning manifest lists both, so the completion commit filters them.
    let recorded = materialized_paths(&wt).unwrap();
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

#[cfg(unix)]
#[test]
fn a_symlinked_claude_skills_pointing_at_agents_skills_needs_no_second_link() {
    let (tmp, wt) = repo_and_worktree("r5");
    let repo = repo_of(&wt);
    std::fs::create_dir_all(repo.join(".claude")).unwrap();
    symlink("../.agents/skills", &repo.join(".claude/skills"));
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
    let exclude = info_exclude_path(&repo).unwrap().unwrap();
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

    WorktreeDelivery::new(&bank).withdraw(&repo, "r6").unwrap();
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
        skill_dir(&bank, "s1").join("SKILL.md"),
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
    assert!(snapshot_skills(&bank, &repo, "r7", &both)
        .unwrap()
        .is_empty());
    assert!(snapshot_root(&repo, "r7").join("s2/SKILL.md").is_file());
    let report = deliver(&wt, &snapshot_root(&repo, "r7"), "r7", &both).unwrap();
    assert_eq!(
        report.delivered,
        vec!["tdd".to_string(), "grilling".to_string()]
    );

    // A skill deleted from the bank before it was ever snapshotted is reported.
    let gone = [eff("s3", "gone")];
    assert_eq!(snapshot_skills(&bank, &repo, "r7", &gone).unwrap().len(), 1);
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
    let sha = stage_and_commit(
        &wt,
        "n",
        1,
        &pdo_workspace::adapters::git::worktrees::CommitIdentity::default(),
    )
    .unwrap();
    assert!(sha.is_some());
    let files = git(&wt, &["show", "--name-only", "--format=", "HEAD"]);
    assert_eq!(files.trim(), "work.txt");
    assert_eq!(git(&wt, &["status", "--porcelain"]), "");
}

#[test]
fn a_name_that_cannot_name_a_folder_is_skipped() {
    let (tmp, wt) = repo_and_worktree("r9");
    let repo = repo_of(&wt);
    let bank = bank_with(tmp.path(), "s1", "tdd");
    let skills = [eff("s1", ".hidden")];
    snapshot_skills(&bank, &repo, "r9", &skills).unwrap();
    let report = deliver(&wt, &snapshot_root(&repo, "r9"), "r9", &skills).unwrap();
    assert_eq!(
        report.skipped[0].reason,
        "skill name cannot be used as a folder name"
    );
    assert!(exclusions_of(&repo, "r9").is_empty());
}
