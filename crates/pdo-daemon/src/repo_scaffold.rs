//! Create a git repository from scratch, files and first commit included (#824).
//!
//! A **generic** verb: parent folder, name, initial files in, absolute path out. It
//! knows nothing about tours (ADR-0071 §3) — the *First run* tour is merely its
//! first caller, and asks for `/tmp` + `pdo-tutorial`. Anything else that needs a
//! throwaway repository can call it with its own three arguments.
//!
//! Three outcomes, and they are the whole contract:
//!
//! 1. Nothing at the target path → create it, `git init` on `main`, write the
//!    files, one commit. [`ScaffoldOutcome::created`] is `true`.
//! 2. A **git repository** already there → hand back its path, touch nothing, do
//!    NOT commit again. `created` is `false`. This is what makes a second run of
//!    the tour free, and what lets the user keep whatever they did in there.
//! 3. A directory that is **not** a repository → a named refusal. Never "fix" it
//!    by initialising a repo over someone's folder: the path was not ours, and the
//!    caller is entitled to say so to the user verbatim.
//!
//! The commit is made under an identity written into the repository's own config
//! rather than taken from the machine's. A daemon that borrowed `~/.gitconfig`
//! would fail on a host that has none (`git commit` exits 128 asking who you are),
//! and would sign a throwaway repo with the user's real name for no reason.

use std::path::{Path, PathBuf};
use std::process::Command;

/// One file to write before the first commit. `path` is relative to the new repo
/// and is rejected if it tries to leave it.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub(crate) struct ScaffoldFile {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScaffoldOutcome {
    pub path: PathBuf,
    /// `false` when an existing repository was reused as is.
    pub created: bool,
}

/// The identity of the initial commit, local to the created repository.
const AUTHOR_NAME: &str = "PDO";
const AUTHOR_EMAIL: &str = "pdo@localhost";

/// The branch every repository this verb creates starts on. Stated rather than
/// inherited: `init.defaultBranch` varies per machine, and a caller that then
/// offers "source branch" would show whatever the host happened to prefer.
const INITIAL_BRANCH: &str = "main";

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
    let output = Command::new("git")
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

/// Reject anything that would write outside the new repository — an absolute
/// path, a `..` segment, or an empty name. The caller supplies these, and a
/// caller is not a threat model, but a silently mis-scoped write is a bug that
/// only shows up as a corrupted file somewhere else.
fn check_relative(path: &str) -> Result<(), String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("file path must not be empty".into());
    }
    let p = Path::new(trimmed);
    if p.is_absolute() {
        return Err(format!(
            "file path must be relative to the repository: {path}"
        ));
    }
    if p.components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(format!(
            "file path must not climb out of the repository: {path}"
        ));
    }
    Ok(())
}

/// Create (or reuse) `<parent>/<name>` as a git repository holding `files`.
///
/// Every refusal is a sentence a user can act on: it names the path and says what
/// about it is wrong. See the module docs for the three outcomes.
pub(crate) fn create_repo(
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

    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("name must not be empty".into());
    }
    if trimmed.contains('/') || trimmed.contains('\\') || trimmed == "." || trimmed == ".." {
        return Err(format!("name must be a single folder name: {name}"));
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, content: &str) -> ScaffoldFile {
        ScaffoldFile {
            path: path.into(),
            content: content.into(),
        }
    }

    fn head_branch(repo: &Path) -> String {
        let out = Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .current_dir(repo)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn commit_count(repo: &Path) -> usize {
        let out = Command::new("git")
            .args(["rev-list", "--count", "HEAD"])
            .current_dir(repo)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout)
            .trim()
            .parse()
            .unwrap_or(0)
    }

    #[test]
    fn creates_repo_with_files_and_a_commit_on_main() {
        let tmp = tempfile::tempdir().unwrap();
        let outcome = create_repo(
            tmp.path().to_str().unwrap(),
            "pdo-tutorial",
            &[
                file("README.md", "# Tutorial\nA throwaway repo.\n"),
                file("notes.txt", ""),
            ],
        )
        .unwrap();

        assert!(outcome.created);
        assert_eq!(outcome.path, tmp.path().join("pdo-tutorial"));
        assert!(outcome.path.join(".git").exists());
        assert_eq!(head_branch(&outcome.path), "main");
        assert_eq!(commit_count(&outcome.path), 1);
        assert_eq!(
            std::fs::read_to_string(outcome.path.join("README.md")).unwrap(),
            "# Tutorial\nA throwaway repo.\n"
        );
        assert!(outcome.path.join("notes.txt").exists());
    }

    #[test]
    fn the_initial_commit_is_signed_by_the_repo_local_identity() {
        let tmp = tempfile::tempdir().unwrap();
        let outcome =
            create_repo(tmp.path().to_str().unwrap(), "r", &[file("a.txt", "a")]).unwrap();

        let out = Command::new("git")
            .args(["log", "-1", "--format=%an <%ae>"])
            .current_dir(&outcome.path)
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim(),
            format!("{AUTHOR_NAME} <{AUTHOR_EMAIL}>")
        );
    }

    #[test]
    fn a_second_call_reuses_the_repo_without_committing_again() {
        let tmp = tempfile::tempdir().unwrap();
        let parent = tmp.path().to_str().unwrap();
        let first = create_repo(parent, "pdo-tutorial", &[file("notes.txt", "one")]).unwrap();
        std::fs::write(first.path.join("notes.txt"), "edited by the user").unwrap();

        let second = create_repo(parent, "pdo-tutorial", &[file("notes.txt", "one")]).unwrap();

        assert!(!second.created);
        assert_eq!(second.path, first.path);
        assert_eq!(commit_count(&second.path), 1);
        // The reuse branch writes nothing: the user's edit survives.
        assert_eq!(
            std::fs::read_to_string(second.path.join("notes.txt")).unwrap(),
            "edited by the user"
        );
    }

    #[test]
    fn refuses_an_existing_folder_that_is_not_a_repo() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("pdo-tutorial")).unwrap();

        let err = create_repo(tmp.path().to_str().unwrap(), "pdo-tutorial", &[]).unwrap_err();

        assert_eq!(
            err,
            format!(
                "{} exists and is not a git repository",
                tmp.path().join("pdo-tutorial").display()
            )
        );
    }

    #[test]
    fn a_folder_inside_another_repo_is_not_a_repo_root() {
        let tmp = tempfile::tempdir().unwrap();
        let outer =
            create_repo(tmp.path().to_str().unwrap(), "outer", &[file("a.txt", "a")]).unwrap();
        std::fs::create_dir(outer.path.join("inner")).unwrap();

        // `rev-parse --git-dir` would say yes here; `--show-toplevel` says outer.
        let err = create_repo(outer.path.to_str().unwrap(), "inner", &[]).unwrap_err();
        assert!(err.ends_with("exists and is not a git repository"), "{err}");
    }

    #[test]
    fn refuses_an_existing_file_at_the_target_path() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("taken"), "").unwrap();

        let err = create_repo(tmp.path().to_str().unwrap(), "taken", &[]).unwrap_err();

        assert!(err.ends_with("exists and is not a directory"), "{err}");
    }

    #[test]
    fn refuses_a_relative_parent() {
        let err = create_repo("relative/parent", "r", &[]).unwrap_err();
        assert!(err.contains("absolute path"), "{err}");
    }

    #[test]
    fn refuses_a_parent_that_does_not_exist() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("nope");
        let err = create_repo(missing.to_str().unwrap(), "r", &[]).unwrap_err();
        assert!(err.contains("does not exist"), "{err}");
    }

    #[test]
    fn refuses_a_name_that_is_a_path() {
        let tmp = tempfile::tempdir().unwrap();
        let err = create_repo(tmp.path().to_str().unwrap(), "a/b", &[]).unwrap_err();
        assert!(err.contains("single folder name"), "{err}");
        assert!(!tmp.path().join("a").exists());
    }

    #[test]
    fn refuses_a_file_path_that_climbs_out() {
        let tmp = tempfile::tempdir().unwrap();
        let err = create_repo(
            tmp.path().to_str().unwrap(),
            "r",
            &[file("../escaped.txt", "x")],
        )
        .unwrap_err();
        assert!(err.contains("climb out"), "{err}");
        // Refused BEFORE anything was created.
        assert!(!tmp.path().join("r").exists());
        assert!(!tmp.path().join("escaped.txt").exists());
    }

    #[test]
    fn writes_files_in_subfolders() {
        let tmp = tempfile::tempdir().unwrap();
        let outcome = create_repo(
            tmp.path().to_str().unwrap(),
            "r",
            &[file("docs/readme.md", "nested")],
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(outcome.path.join("docs/readme.md")).unwrap(),
            "nested"
        );
    }
}
