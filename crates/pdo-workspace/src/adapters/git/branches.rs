//! The branch list a Run can be cut from, read locally, without a fetch (#571,
//! #802).

use std::path::Path;

use crate::domain::branches::{parse_branch_refs, BranchEntry, BranchList};

use super::repository::last_fetch_at;

/// The configured remote names, longest first so stripping a remote's `<name>/`
/// prefix off a tracking ref is unambiguous even when a remote name itself
/// contains `/` (or is a prefix of another). Empty on a repo with no remotes.
pub fn git_remote_names(repo: &Path) -> Result<Vec<String>, String> {
    let output = std::process::Command::new("git")
        .args(["remote"])
        .current_dir(repo)
        .output();
    match output {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            let mut names: Vec<String> = stdout
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect();
            names.sort_by_key(|n| std::cmp::Reverse(n.len()));
            Ok(names)
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            Err(format!("git remote failed: {stderr}"))
        }
        Err(e) => Err(format!("failed to run git: {e}")),
    }
}

/// List the branches a Run can be cut from: local branches AND remote-tracking refs.
/// Everything is resolved **locally, without a single `git fetch`** — freshness of
/// the tracking refs is the operator's own concern.
///
/// Don't go back to `git branch -a`: it emits a `(HEAD detached at …)` pseudo-entry,
/// obeys the operator's `branch.sort`, and is porcelain. The `%(symref)` column is the
/// only robust discriminant of `origin/HEAD`, which otherwise appears as a bare
/// `origin` entry.
///
/// A remote-tracking ref whose bare name already exists as a local branch is dropped
/// unconditionally — no divergence test — on the convention that the local branch is
/// what the user manipulates. Remote-vs-remote twins (`origin/foo` + `upstream/foo`)
/// are BOTH kept: two remotes may point at different SHAs, so merging them would lie.
///
/// Locals before remotes via an EXPLICIT partition — never lean on
/// `refs/heads` < `refs/remotes` lexical luck, since the first entry may become the
/// client's default and a local `main` must win.
pub fn list_branches(repo: &Path) -> Result<Vec<BranchEntry>, String> {
    let remotes = git_remote_names(repo)?;

    let output = std::process::Command::new("git")
        .args([
            "for-each-ref",
            "--sort=refname",
            // Subject LAST: it is the only field that may itself contain a tab, so a
            // `splitn` on the count of the fields before it cannot be derailed by it.
            "--format=%(refname)%09%(refname:short)%09%(symref)%09%(upstream:short)\
             %09%(upstream:track)%09%(HEAD)%09%(committerdate:iso-strict)%09%(contents:subject)",
            "refs/heads",
            "refs/remotes",
        ])
        // Belt and braces on a guarantee we depend on but do not control: the
        // `ahead %d` / `behind %d` / `gone` words of `%(upstream:track)` are git's
        // UNTRANSLATED plumbing strings — `for-each-ref` never calls
        // `setup_ref_filter_porcelain_msg()`, unlike `git branch -vv`, whose
        // localized output must never be parsed. Pinning the locale costs nothing
        // and keeps that distinction from becoming load-bearing by accident.
        .env("LC_ALL", "C")
        .current_dir(repo)
        .output();
    let stdout = match output {
        Ok(o) if o.status.success() => o.stdout,
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            return Err(format!("git for-each-ref failed: {stderr}"));
        }
        Err(e) => return Err(format!("failed to run git: {e}")),
    };
    Ok(parse_branch_refs(
        &String::from_utf8_lossy(&stdout),
        &remotes,
    ))
}

/// The whole answer of `GET /repos/branches`: the list plus the date it is good as of.
pub fn branch_list(repo: &Path) -> Result<BranchList, String> {
    Ok(BranchList {
        branches: list_branches(repo)?,
        last_fetch_at: last_fetch_at(repo),
        fetch_error: None,
    })
}
