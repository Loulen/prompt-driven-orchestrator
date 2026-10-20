//! Create a repository from scratch (#824), in real temporary directories.

use std::path::Path;
use std::process::Command;

use pdo_workspace::adapters::git::scaffold::create_repo;
use pdo_workspace::domain::scaffold::{ScaffoldFile, AUTHOR_EMAIL, AUTHOR_NAME};

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
    let outcome = create_repo(tmp.path().to_str().unwrap(), "r", &[file("a.txt", "a")]).unwrap();

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
    std::fs::write(tmp.path().join("pdo-tutorial/notes.txt"), "mine").unwrap();

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
    let outer = create_repo(tmp.path().to_str().unwrap(), "outer", &[file("a.txt", "a")]).unwrap();
    std::fs::create_dir(outer.path.join("inner")).unwrap();
    std::fs::write(outer.path.join("inner/b.txt"), "b").unwrap();

    // `rev-parse --git-dir` would say yes here; `--show-toplevel` says outer.
    let err = create_repo(outer.path.to_str().unwrap(), "inner", &[]).unwrap_err();
    assert!(err.ends_with("exists and is not a git repository"), "{err}");
}

#[test]
fn an_empty_existing_folder_becomes_a_repo_with_a_first_commit() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir(tmp.path().join("fresh")).unwrap();

    let outcome = create_repo(tmp.path().to_str().unwrap(), "fresh", &[]).unwrap();

    assert!(outcome.created);
    assert_eq!(outcome.path, tmp.path().join("fresh"));
    assert_eq!(head_branch(&outcome.path), "main");
    assert_eq!(commit_count(&outcome.path), 1);
}

#[test]
fn a_new_repo_without_files_still_gets_its_first_commit() {
    let tmp = tempfile::tempdir().unwrap();

    let outcome = create_repo(tmp.path().to_str().unwrap(), "empty", &[]).unwrap();

    assert!(outcome.created);
    assert_eq!(commit_count(&outcome.path), 1);
}

#[test]
fn an_empty_folder_inside_another_repo_becomes_a_repo_of_its_own() {
    let tmp = tempfile::tempdir().unwrap();
    let outer = create_repo(tmp.path().to_str().unwrap(), "outer", &[file("a.txt", "a")]).unwrap();
    std::fs::create_dir(outer.path.join("inner")).unwrap();

    let inner = create_repo(outer.path.to_str().unwrap(), "inner", &[]).unwrap();

    assert!(inner.created);
    assert!(inner.path.join(".git").exists());
    assert_eq!(commit_count(&inner.path), 1);
    assert_eq!(commit_count(&outer.path), 1);
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
