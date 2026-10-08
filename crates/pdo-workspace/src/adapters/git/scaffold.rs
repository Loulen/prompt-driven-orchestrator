//! Create a git repository from scratch (#824): the git and filesystem side of
//! [`crate::domain::scaffold`], whose module docs state the three outcomes.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::domain::commit_identity::GIT_IDENTITY_VARS;
use crate::domain::scaffold::{
    check_name, check_relative, ScaffoldFile, ScaffoldOutcome, AUTHOR_EMAIL, AUTHOR_NAME,
    INITIAL_BRANCH,
};

/// Is this directory inside a git repository **of its own**? `rev-parse
/// --git-dir` alone answers yes for any subdirectory of a repo, so a folder
/// created under an existing checkout would read as "already a repository" and be
/// handed back untouched. `--show-toplevel` is compared against the path itself.
fn is_repo_root(dir: &Path) -> bool {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(dir)
        .output();
    let Ok(o) = output else { return false };
    if !o.status.success() {
        return false;
    }
    let top = String::from_utf8_lossy(&o.stdout).trim().to_string();
    if top.is_empty() {
        return false;
    }
    // Both sides canonicalised: `/tmp` is a symlink to `/private/tmp` on macOS, and
    // git answers with the resolved form.
    match (std::fs::canonicalize(&top), std::fs::canonicalize(dir)) {
        (Ok(a), Ok(b)) => a == b,
        _ => top == dir.to_string_lossy(),
    }
}

fn git(repo: &Path, args: &[&str]) -> Result<(), String> {
    let mut command = Command::new("git");
    // An inherited identity (a daemon started from an agent's shell) would sign
    // the initial commit instead of the repository's own identity.
    for var in GIT_IDENTITY_VARS {
        command.env_remove(var);
    }
    let output = command
        .args(args)
        .current_dir(repo)
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let detail = stderr.trim();
    Err(format!(
        "git {} failed{}",
        args.join(" "),
        if detail.is_empty() {
            String::new()
        } else {
            format!(": {detail}")
        }
    ))
}

/// Create (or reuse) `<parent>/<name>` as a git repository holding `files`.
///
/// Every refusal is a sentence a user can act on: it names the path and says what
/// about it is wrong. See the module docs for the three outcomes.
pub fn create_repo(
    parent: &str,
    name: &str,
    files: &[ScaffoldFile],
) -> Result<ScaffoldOutcome, String> {
    let parent_path = PathBuf::from(parent);
    if !parent_path.is_absolute() {
        return Err(format!("parent must be an absolute path: {parent}"));
    }
    if !parent_path.is_dir() {
        return Err(format!(
            "parent does not exist or is not a directory: {parent}"
        ));
    }

    let trimmed = check_name(name)?;
    for file in files {
        check_relative(&file.path)?;
    }

    let target = parent_path.join(trimmed);
    let shown = target.display().to_string();

    if target.exists() {
        if !target.is_dir() {
            return Err(format!("{shown} exists and is not a directory"));
        }
        if is_repo_root(&target) {
            // Outcome 2: reuse. No write, no commit — the caller asked for a
            // repository at that path, and there is one.
            return Ok(ScaffoldOutcome {
                path: target,
                created: false,
            });
        }
        // Outcome 3: refuse, by name. This exact sentence reaches the user.
        return Err(format!("{shown} exists and is not a git repository"));
    }

    std::fs::create_dir_all(&target).map_err(|e| format!("could not create {shown}: {e}"))?;

    // From here on a failure leaves a half-built directory behind, which would
    // make the NEXT call take the "exists and is not a git repository" branch and
    // refuse forever. Unwind instead: this directory did not exist a moment ago,
    // so removing it takes nothing that was not ours.
    match scaffold_into(&target, files) {
        Ok(()) => Ok(ScaffoldOutcome {
            path: target,
            created: true,
        }),
        Err(e) => {
            let _ = std::fs::remove_dir_all(&target);
            Err(e)
        }
    }
}

fn scaffold_into(target: &Path, files: &[ScaffoldFile]) -> Result<(), String> {
    // `-b` needs git ≥ 2.28; the fallback renames the branch after the fact so a
    // repository created here is on `main` on either.
    if git(target, &["init", "-b", INITIAL_BRANCH]).is_err() {
        git(target, &["init"])?;
        git(target, &["symbolic-ref", "HEAD", "refs/heads/main"])?;
    }
    git(target, &["config", "user.name", AUTHOR_NAME])?;
    git(target, &["config", "user.email", AUTHOR_EMAIL])?;
    // A host-wide `commit.gpgsign = true` would make the commit below hang on a
    // passphrase prompt inside the daemon. Turn it off for this repository only.
    git(target, &["config", "commit.gpgsign", "false"])?;

    for file in files {
        let path = target.join(file.path.trim());
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|e| format!("could not create {}: {e}", dir.display()))?;
        }
        std::fs::write(&path, &file.content)
            .map_err(|e| format!("could not write {}: {e}", path.display()))?;
    }

    git(target, &["add", "-A"])?;
    // `--no-verify` because a global `core.hooksPath` would otherwise run the
    // user's hooks inside a repository they have never seen.
    git(target, &["commit", "--no-verify", "-m", "Initial commit"])?;
    Ok(())
}
