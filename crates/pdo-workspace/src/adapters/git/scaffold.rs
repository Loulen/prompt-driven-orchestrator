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
    // A missing parent is created with the repository (`~/code/new-repository`
    // with no `~/code` yet); one that is a file is refused.
    if parent_path.exists() && !parent_path.is_dir() {
        return Err(format!("parent is not a directory: {parent}"));
    }

    let trimmed = check_name(name)?;
    for file in files {
        check_relative(&file.path)?;
    }

    let target = parent_path.join(trimmed);
    let shown = target.display().to_string();

    // `symlink_metadata`: a link someone else left there is theirs, wherever it points.
    let existed = std::fs::symlink_metadata(&target).is_ok();
    if existed {
        // Whatever stands under a shared parent (`/tmp`) and is not ours — a
        // repository carrying git hooks or agent settings, an empty folder to
        // scaffold into, a link to one of our repositories — is never taken
        // over: the user's Runs would work in it (CORE-232).
        if !owned_by_us(&target) {
            return Err(format!(
                "{shown} belongs to another user: remove it, or create the repository elsewhere"
            ));
        }
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
        if !is_empty_dir(&target) {
            // Outcome 3: refuse, by name. This exact sentence reaches the user.
            return Err(format!("{shown} exists and is not a git repository"));
        }
        // Outcome 1 on an empty folder: nothing in it is anyone's yet.
    } else {
        std::fs::create_dir_all(&target).map_err(|e| format!("could not create {shown}: {e}"))?;
    }

    // From here on a failure leaves a half-built directory behind, which would
    // make the NEXT call take the "exists and is not a git repository" branch and
    // refuse forever. Unwind instead: this directory did not exist a moment ago,
    // or was empty, so removing what is in it takes nothing that was not ours.
    match scaffold_into(&target, files) {
        Ok(()) => Ok(ScaffoldOutcome {
            path: target,
            created: true,
        }),
        Err(e) => {
            if existed {
                empty_dir(&target);
            } else {
                let _ = std::fs::remove_dir_all(&target);
            }
            Err(e)
        }
    }
}

/// Whether the daemon's user owns `dir`. Off Unix there is no owner to compare:
/// the folder is taken as ours, as before.
#[cfg(unix)]
fn owned_by_us(dir: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    // SAFETY: `geteuid` takes no argument, dereferences nothing and always succeeds.
    let me = unsafe { libc::geteuid() };
    std::fs::symlink_metadata(dir).is_ok_and(|m| m.uid() == me)
}

#[cfg(not(unix))]
fn owned_by_us(_dir: &Path) -> bool {
    true
}

fn is_empty_dir(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|mut entries| entries.next().is_none())
}

/// Remove what a failed scaffold wrote into a folder that was empty before it.
fn empty_dir(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let _ = if path.is_dir() {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
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
    // `--allow-empty`: a repository asked for without files still starts with
    // its first commit, so it has a branch to cut a Run from.
    git(
        target,
        &[
            "commit",
            "--no-verify",
            "--allow-empty",
            "-m",
            "Initial commit",
        ],
    )?;
    Ok(())
}

#[cfg(all(test, unix))]
mod owner_tests {
    use super::*;

    #[test]
    fn only_a_folder_of_ours_is_reused() {
        let ours = tempfile::tempdir().unwrap();
        assert!(owned_by_us(ours.path()));
        // `/` is root's: anyone else does not own it (CORE-232).
        // SAFETY: as in `owned_by_us`.
        if unsafe { libc::geteuid() } != 0 {
            assert!(!owned_by_us(Path::new("/")));
        }
    }

    #[test]
    fn a_folder_of_another_user_is_refused_by_name_never_taken_over() {
        // SAFETY: as in `owned_by_us`.
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        // `/usr` is root's: as a training repository left under `/tmp` by
        // someone else would be.
        let err = create_repo("/", "usr", &[]).unwrap_err();
        assert_eq!(
            err,
            "/usr belongs to another user: remove it, or create the repository elsewhere"
        );
    }

    #[test]
    fn a_link_we_did_not_make_is_judged_by_its_own_owner() {
        let dir = tempfile::tempdir().unwrap();
        let ours = dir.path().join("link");
        std::os::unix::fs::symlink("/usr", &ours).unwrap();
        // Our own link to a folder of root's: ours, by the link's owner.
        assert!(owned_by_us(&ours));
    }
}
