//! What git says of a Run's repository: its remote and its current branch.
//! Read with `git` itself, off the runtime; a failure is an unknown, never an
//! error.

use std::path::Path;
use std::process::Command;

fn git(repo: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
}

/// The URL of `origin`, else of the first remote.
pub fn remote_url(repo: &Path) -> Option<String> {
    git(repo, &["remote", "get-url", "origin"]).or_else(|| {
        let first = git(repo, &["remote"])?.lines().next()?.to_string();
        git(repo, &["remote", "get-url", &first])
    })
}

/// The branch checked out, `None` on a detached head.
pub fn current_branch(repo: &Path) -> Option<String> {
    git(repo, &["rev-parse", "--abbrev-ref", "HEAD"]).filter(|b| b != "HEAD")
}
