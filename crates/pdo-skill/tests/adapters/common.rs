//! Real temporary repositories for the adapter tests (stage 3, ADR-0080).

#![allow(dead_code)]

use std::path::{Path, PathBuf};

pub fn git(dir: &Path, args: &[&str]) -> String {
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

/// A repository with one commit on `main`.
pub fn init_repo(repo: &Path) {
    std::fs::create_dir_all(repo).unwrap();
    git(repo, &["init", "-q", "-b", "main"]);
    git(repo, &["config", "user.email", "t@e.com"]);
    git(repo, &["config", "user.name", "T"]);
    git(repo, &["config", "commit.gpgsign", "false"]);
    std::fs::write(repo.join("README.md"), "hi\n").unwrap();
    std::fs::write(repo.join(".gitignore"), ".pdo/runs/\n").unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-q", "-m", "init"]);
}

/// A repo with one commit, plus a linked worktree on a `pdo/run-<id>` branch —
/// the shape a Run works in.
pub fn repo_and_worktree(run_id: &str) -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    init_repo(&repo);
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

/// The repository a worktree made by [`repo_and_worktree`] belongs to.
pub fn repo_of(wt: &Path) -> PathBuf {
    wt.ancestors().nth(4).unwrap().to_path_buf()
}

/// A skill folder `name` holding a `SKILL.md` and `references/notes.md`.
pub fn write_skill(dir: &Path, name: &str) {
    std::fs::create_dir_all(dir.join("references")).unwrap();
    std::fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: d\n---\n\n# {name}\n"),
    )
    .unwrap();
    std::fs::write(dir.join("references").join("notes.md"), "ref\n").unwrap();
}
