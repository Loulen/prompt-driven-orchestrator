//! `validate_target_repo` / `validate_source_branch` against real repositories.

use pdo_workspace::adapters::git::repository::{validate_source_branch, validate_target_repo};

use crate::common::*;

// --- Multi-repo run creation tests (issue #114) ---

#[test]
fn validate_target_repo_rejects_relative_path() {
    let result = validate_target_repo("relative/path");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("absolute path"));
}

#[test]
fn validate_target_repo_rejects_nonexistent_path() {
    let result = validate_target_repo("/nonexistent/path/that/does/not/exist");
    assert_eq!(
        result.unwrap_err(),
        "/nonexistent/path/that/does/not/exist does not exist or is not a directory"
    );
}

#[test]
fn validate_target_repo_rejects_non_git_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().to_str().unwrap();
    let result = validate_target_repo(path);
    // The phrase reaches the UI as is: it names the folder, not an API field.
    assert_eq!(
        result.unwrap_err(),
        format!("{path} is not a git repository")
    );
}

#[test]
fn validate_target_repo_accepts_git_repo() {
    let tmp = tempfile::tempdir().unwrap();
    init_test_repo(tmp.path());
    let result = validate_target_repo(tmp.path().to_str().unwrap());
    assert!(result.is_ok());
}

#[test]
fn validate_source_branch_rejects_nonexistent() {
    let tmp = tempfile::tempdir().unwrap();
    init_test_repo(tmp.path());
    let result = validate_source_branch(tmp.path(), "nonexistent-branch");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("does not exist"));
}

#[test]
fn validate_source_branch_accepts_existing_branch() {
    let tmp = tempfile::tempdir().unwrap();
    init_test_repo(tmp.path());
    // init_test_repo creates a default branch, find its name
    let output = std::process::Command::new("git")
        .args(["branch", "--format=%(refname:short)"])
        .current_dir(tmp.path())
        .output()
        .unwrap();
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert!(!branch.is_empty());
    let result = validate_source_branch(tmp.path(), &branch);
    assert!(result.is_ok());
}

#[test]
fn validate_source_branch_accepts_remote_tracking_ref() {
    let (_origin, work) = init_repo_with_remote();
    // The remote-only branch has no local counterpart — this was the 400 the
    // change fixes.
    let result = validate_source_branch(work.path(), "origin/feature-remote-only");
    assert!(
        result.is_ok(),
        "remote-tracking ref must validate: {result:?}"
    );
}

#[test]
fn validate_source_branch_rejects_unknown_remote_ref() {
    let (_origin, work) = init_repo_with_remote();
    let err = validate_source_branch(work.path(), "origin/nope").unwrap_err();
    assert!(err.contains("does not exist"), "message was: {err}");
    assert!(
        err.contains("origin/nope"),
        "message must name the ref: {err}"
    );
}

#[test]
fn validate_source_branch_does_not_glob() {
    let tmp = tempfile::tempdir().unwrap();
    init_test_repo(tmp.path());
    // A default branch exists (main/master); a glob would match it. The exact
    // `show-ref --verify` probe must NOT — `mai*` is not a real branch.
    let err = validate_source_branch(tmp.path(), "mai*").unwrap_err();
    assert!(err.contains("does not exist"), "message was: {err}");
}

#[test]
fn remote_url_reads_origin_else_the_first_remote() {
    use pdo_workspace::adapters::git::repository::remote_url;
    let tmp = tempfile::tempdir().unwrap();
    init_test_repo(tmp.path());
    assert_eq!(remote_url(tmp.path()), None);

    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(tmp.path())
            .output()
            .unwrap()
    };
    git(&["remote", "add", "upstream", "https://example.com/up.git"]);
    assert_eq!(
        remote_url(tmp.path()).as_deref(),
        Some("https://example.com/up.git")
    );
    git(&["remote", "add", "origin", "git@example.com:acme/app.git"]);
    assert_eq!(
        remote_url(tmp.path()).as_deref(),
        Some("git@example.com:acme/app.git")
    );
}
