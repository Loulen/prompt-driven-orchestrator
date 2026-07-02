//! Layer 3a (real-daemon) tests for `GET /repos/browse` (issue #131).
//!
//! Boots a real `TestDaemon` and drives the explicit-`?path=` branch against a known
//! directory tree seeded on the real filesystem. The default-root branch
//! (`$HOME → repo_root → /`) is covered by the pure `resolve_browse_root` unit tests
//! in `lib.rs` — driving it here would couple the test to the CI environment's
//! `$HOME`, so we deliberately stay on the explicit branch.

mod common;

use std::os::unix::fs::symlink;
use std::process::Command;

use common::TestDaemon;
use tempfile::TempDir;

/// Seed a deterministic tree the assertions bind to:
/// - `alpha-project`  git repo (has `.git`)         → `is_git_repo: true`
/// - `beta-plain`     plain dir (no `.git`)          → `is_git_repo: false`
/// - `.hidden-dir`    dotfile dir                    → hidden (absent)
/// - `zeta-link`      symlink → alpha-project        → listed, `is_symlink: true`
/// - `notes.txt`      plain file                     → filtered out (dirs only)
fn seed_tree() -> TempDir {
    let root = tempfile::tempdir().unwrap();
    let p = root.path();

    // A real git repo (bare `git init` is enough to create `.git`).
    std::fs::create_dir(p.join("alpha-project")).unwrap();
    let out = Command::new("git")
        .args(["init"])
        .current_dir(p.join("alpha-project"))
        .output()
        .unwrap();
    assert!(out.status.success(), "git init should succeed");

    std::fs::create_dir(p.join("beta-plain")).unwrap();
    std::fs::create_dir(p.join(".hidden-dir")).unwrap();
    symlink(p.join("alpha-project"), p.join("zeta-link")).unwrap();
    std::fs::write(p.join("notes.txt"), "notes").unwrap();

    root
}

async fn browse(daemon: &TestDaemon, path: Option<&str>) -> reqwest::Response {
    let url = match path {
        Some(p) => format!(
            "{}/repos/browse?path={}",
            daemon.url(),
            urlencoding_encode(p)
        ),
        None => format!("{}/repos/browse", daemon.url()),
    };
    reqwest::get(url).await.unwrap()
}

/// Minimal percent-encoder for path query values (avoids a new dev-dependency).
/// Only encodes the handful of bytes that matter for a filesystem path in a query.
fn urlencoding_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[tokio::test]
async fn browse_lists_dirs_only_with_flags_sorted() {
    let daemon = TestDaemon::spawn(|_repo_root| Ok(())).await.unwrap();
    let tree = seed_tree();
    let tree_path = tree.path().to_str().unwrap();

    let resp = browse(&daemon, Some(tree_path)).await;
    assert_eq!(resp.status(), 200);
    let json: serde_json::Value = resp.json().await.unwrap();

    assert!(json["error"].is_null(), "no error on a readable dir");
    assert_eq!(json["truncated"], false);
    assert!(
        json["parent"].is_string(),
        "a tempdir is never the filesystem root, so parent is set"
    );

    let entries = json["entries"].as_array().unwrap();
    let names: Vec<&str> = entries
        .iter()
        .map(|e| e["name"].as_str().unwrap())
        .collect();

    // Dirs only, dotfiles hidden, files filtered, case-insensitive alpha order.
    assert_eq!(
        names,
        vec!["alpha-project", "beta-plain", "zeta-link"],
        "dirs-only, .hidden-dir + notes.txt excluded, alpha-sorted"
    );

    let by_name = |n: &str| entries.iter().find(|e| e["name"] == n).unwrap();
    assert_eq!(
        by_name("alpha-project")["is_git_repo"],
        true,
        "alpha-project has .git → flagged"
    );
    assert_eq!(
        by_name("beta-plain")["is_git_repo"],
        false,
        "beta-plain has no .git → not flagged"
    );
    assert_eq!(
        by_name("alpha-project")["is_symlink"],
        false,
        "alpha-project is a real dir"
    );
    assert_eq!(
        by_name("zeta-link")["is_symlink"],
        true,
        "zeta-link is a symlink → flagged"
    );

    // Entry paths are `dir.join(name)` (canonicalized parent + verbatim child name),
    // not re-canonicalized — so the symlink keeps the path the user would click.
    let zeta_path = by_name("zeta-link")["path"].as_str().unwrap();
    assert!(
        zeta_path.ends_with("/zeta-link"),
        "symlink entry path keeps its own name, got {zeta_path}"
    );
}

#[tokio::test]
async fn browse_relative_path_is_400() {
    let daemon = TestDaemon::spawn(|_repo_root| Ok(())).await.unwrap();
    let resp = browse(&daemon, Some("relative/not/absolute")).await;
    assert_eq!(resp.status(), 400, "relative path is a caller bug → 400");
    let json: serde_json::Value = resp.json().await.unwrap();
    assert!(
        json["error"].as_str().unwrap().contains("absolute"),
        "error mentions the absolute-path requirement"
    );
}

#[tokio::test]
async fn browse_file_path_lists_its_parent() {
    let daemon = TestDaemon::spawn(|_repo_root| Ok(())).await.unwrap();
    let tree = seed_tree();
    let file = tree.path().join("notes.txt");

    let resp = browse(&daemon, Some(file.to_str().unwrap())).await;
    assert_eq!(resp.status(), 200);
    let json: serde_json::Value = resp.json().await.unwrap();
    assert!(json["error"].is_null());

    // Listing the file's parent yields the same dir listing as browsing the dir.
    let names: Vec<&str> = json["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["alpha-project", "beta-plain", "zeta-link"]);
}

#[tokio::test]
async fn browse_nonexistent_path_clamps_gracefully() {
    let daemon = TestDaemon::spawn(|_repo_root| Ok(())).await.unwrap();
    // A stale/half-typed absolute path that does not exist → clamps to the default
    // chain and returns 200 (the explorer opens gracefully, never errors).
    let resp = browse(&daemon, Some("/this/path/does/not/exist/131")).await;
    assert_eq!(resp.status(), 200);
    let json: serde_json::Value = resp.json().await.unwrap();
    assert!(json["error"].is_null(), "clamped open is a clean 200");
    assert!(json["path"].is_string());
}
