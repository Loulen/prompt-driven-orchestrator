//! Layer 3a (real-daemon) tests for `GET /fs/browse` (issue #131, renamed + generalised
//! in #431).
//!
//! Boots a real `TestDaemon` and drives the explicit-`?path=` branch against a known
//! directory tree seeded on the real filesystem. The default-root branch
//! (`$HOME → repo_root → /`) is covered by the pure `resolve_browse_root` unit tests
//! in `lib.rs` — driving it here would couple the test to the CI environment's
//! `$HOME`, so we deliberately stay on the explicit branch.
//!
//! **The SPA-fallback trap.** `build_router` ends with `.fallback(static_handler)`,
//! which serves `index.html` for every unmatched path AND every method. A route that
//! was never registered therefore answers **200 + `text/html`**, so a status-only
//! assertion would pass on a missing route. Every route-existence assertion here binds
//! to the **content-type**, and the retired `/repos/browse` path is asserted NOT-json
//! rather than 404 (the fallback never 404s).

use std::os::unix::fs::symlink;
use std::process::Command;

use crate::common::TestDaemon;
use tempfile::TempDir;

/// Seed a deterministic tree the assertions bind to:
/// - `alpha-project`  git repo (has `.git`)         → `is_git_repo: true`
/// - `beta-plain`     plain dir (no `.git`)          → `is_git_repo: false`
/// - `.hidden-dir`    dotfile dir                    → hidden unless `?hidden=true`
/// - `zeta-link`      symlink → alpha-project        → listed, `is_symlink: true`
/// - `notes.txt`      plain file                     → listed only with `?files=true`
/// - `.hidden-file`   dotfile regular file           → needs BOTH flags
/// - `broken-link`    dangling symlink               → invisible in every mode
/// - `sock`           unix socket (a "special")      → invisible in every mode
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
    std::fs::write(p.join(".hidden-file"), "secret").unwrap();
    // A dangling link: `std::fs::metadata` errs on it → unclassifiable → dropped.
    symlink(p.join("nonexistent-target"), p.join("broken-link")).unwrap();
    // A "special" (neither dir nor regular file). The socket FILE survives the
    // listener's drop, so binding here is enough to seed the fixture — std only, no
    // `libc` dev-dependency needed.
    let listener = std::os::unix::net::UnixListener::bind(p.join("sock")).unwrap();
    drop(listener);
    assert!(
        std::fs::symlink_metadata(p.join("sock")).is_ok(),
        "the socket file must survive the listener drop"
    );

    root
}

/// `GET /fs/browse` with an optional `?path=` plus any extra query params.
async fn browse(
    daemon: &TestDaemon,
    path: Option<&str>,
    extra: &[(&str, &str)],
) -> reqwest::Response {
    let mut qs: Vec<String> = Vec::new();
    if let Some(p) = path {
        qs.push(format!("path={}", urlencoding_encode(p)));
    }
    for (k, v) in extra {
        qs.push(format!(
            "{}={}",
            urlencoding_encode(k),
            urlencoding_encode(v)
        ));
    }
    let url = if qs.is_empty() {
        format!("{}/fs/browse", daemon.url())
    } else {
        format!("{}/fs/browse?{}", daemon.url(), qs.join("&"))
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

fn content_type(resp: &reqwest::Response) -> String {
    resp.headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string()
}

fn names(json: &serde_json::Value) -> Vec<String> {
    json["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["name"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn browse_lists_dirs_only_with_flags_sorted() {
    let daemon = TestDaemon::spawn(|_repo_root| Ok(())).await.unwrap();
    let tree = seed_tree();
    let tree_path = tree.path().to_str().unwrap();

    let resp = browse(&daemon, Some(tree_path), &[]).await;
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
    let resp = browse(&daemon, Some("relative/not/absolute"), &[]).await;
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

    let resp = browse(&daemon, Some(file.to_str().unwrap()), &[]).await;
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
    let resp = browse(&daemon, Some("/this/path/does/not/exist/131"), &[]).await;
    assert_eq!(resp.status(), 200);
    let json: serde_json::Value = resp.json().await.unwrap();
    assert!(json["error"].is_null(), "clamped open is a clean 200");
    assert!(json["path"].is_string());
}

// --- #431: the rename, the two flags, `is_dir` ---

#[tokio::test]
async fn fs_browse_is_registered_and_returns_json() {
    // THE ANTI-SPA GATE. `static_handler` serves index.html for every unmatched path
    // and every method, so a forgotten route answers 200 + text/html and a status-only
    // assertion would pass. Assert all three: status, content-type, body shape.
    let daemon = TestDaemon::spawn(|_repo_root| Ok(())).await.unwrap();
    let tree = seed_tree();

    let resp = browse(&daemon, Some(tree.path().to_str().unwrap()), &[]).await;
    assert_eq!(resp.status(), 200);
    let ct = content_type(&resp);
    assert!(
        ct.starts_with("application/json"),
        "route must be registered — the SPA fallback would answer text/html, got {ct}"
    );
    let json: serde_json::Value = resp.json().await.unwrap();
    assert!(json["entries"].is_array());
}

#[tokio::test]
async fn retired_repos_browse_path_is_no_longer_the_endpoint() {
    // #431 renames outright, with no alias. `/repos/browse` now falls through to the
    // SPA — so assert on the content-type, NOT on 404 (the fallback never 404s).
    let daemon = TestDaemon::spawn(|_repo_root| Ok(())).await.unwrap();
    let resp = reqwest::get(format!("{}/repos/browse", daemon.url()))
        .await
        .unwrap();
    let ct = content_type(&resp);
    assert!(
        !ct.starts_with("application/json"),
        "/repos/browse must no longer be the endpoint, got content-type {ct}"
    );
}

#[tokio::test]
async fn browse_default_response_is_additive_only() {
    // The AC: without a parameter the response is the pre-#431 one plus `is_dir`.
    // Exact key SETS, so a stray new field fails here rather than in the UI.
    let daemon = TestDaemon::spawn(|_repo_root| Ok(())).await.unwrap();
    let tree = seed_tree();

    let resp = browse(&daemon, Some(tree.path().to_str().unwrap()), &[]).await;
    let json: serde_json::Value = resp.json().await.unwrap();

    let mut keys: Vec<&str> = json
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        vec!["entries", "error", "parent", "path", "truncated"]
    );

    for e in json["entries"].as_array().unwrap() {
        let mut k: Vec<&str> = e.as_object().unwrap().keys().map(String::as_str).collect();
        k.sort_unstable();
        assert_eq!(
            k,
            vec!["is_dir", "is_git_repo", "is_symlink", "name", "path"],
            "exactly the 4 historical keys + is_dir"
        );
        assert_eq!(
            e["is_dir"], true,
            "under the dirs-only default every entry is a dir"
        );
    }
}

#[tokio::test]
async fn browse_files_true_lists_regular_files_dirs_first() {
    let daemon = TestDaemon::spawn(|_repo_root| Ok(())).await.unwrap();
    let tree = seed_tree();

    let resp = browse(
        &daemon,
        Some(tree.path().to_str().unwrap()),
        &[("files", "true")],
    )
    .await;
    assert_eq!(resp.status(), 200);
    let json: serde_json::Value = resp.json().await.unwrap();

    assert_eq!(
        names(&json),
        vec!["alpha-project", "beta-plain", "zeta-link", "notes.txt"],
        "dirs first (genre beats name), then the regular files"
    );
    let notes = json["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["name"] == "notes.txt")
        .unwrap();
    assert_eq!(notes["is_dir"], false);
    assert_eq!(
        notes["is_git_repo"], false,
        "is_git_repo is hard-false for a file"
    );
    assert_eq!(notes["is_symlink"], false);
}

#[tokio::test]
async fn browse_hidden_true_lists_dotfiles() {
    let daemon = TestDaemon::spawn(|_repo_root| Ok(())).await.unwrap();
    let tree = seed_tree();
    let p = tree.path().to_str().unwrap();

    // `hidden` alone reveals the dot DIR only — `.hidden-file` is still a file.
    let json: serde_json::Value = browse(&daemon, Some(p), &[("hidden", "true")])
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(
        names(&json),
        vec![".hidden-dir", "alpha-project", "beta-plain", "zeta-link"],
        "'.' (0x2E) sorts before 'a'"
    );

    // Both flags: the dot FILE joins the file bucket, after every directory.
    let json: serde_json::Value =
        browse(&daemon, Some(p), &[("hidden", "true"), ("files", "true")])
            .await
            .json()
            .await
            .unwrap();
    assert_eq!(
        names(&json),
        vec![
            ".hidden-dir",
            "alpha-project",
            "beta-plain",
            "zeta-link",
            ".hidden-file",
            "notes.txt",
        ],
    );
}

#[tokio::test]
async fn browse_drops_broken_symlinks_and_specials_in_every_mode() {
    let daemon = TestDaemon::spawn(|_repo_root| Ok(())).await.unwrap();
    let tree = seed_tree();
    let p = tree.path().to_str().unwrap();

    for extra in [
        vec![],
        vec![("files", "true")],
        vec![("hidden", "true")],
        vec![("files", "true"), ("hidden", "true")],
    ] {
        let json: serde_json::Value = browse(&daemon, Some(p), &extra).await.json().await.unwrap();
        let got = names(&json);
        assert!(
            !got.iter().any(|n| n == "broken-link"),
            "a dangling link must never be offered ({extra:?}): {got:?}"
        );
        assert!(
            !got.iter().any(|n| n == "sock"),
            "a special file must never be offered ({extra:?}): {got:?}"
        );
    }
}

#[tokio::test]
async fn browse_symlink_to_dir_is_is_dir_true() {
    // Guards the `DirEntry::metadata` trap: it lstat's (no follow) and would report a
    // symlinked dir as non-dir, dropping it from the dirs-only listing entirely.
    let daemon = TestDaemon::spawn(|_repo_root| Ok(())).await.unwrap();
    let tree = seed_tree();

    let json: serde_json::Value = browse(&daemon, Some(tree.path().to_str().unwrap()), &[])
        .await
        .json()
        .await
        .unwrap();
    let zeta = json["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["name"] == "zeta-link")
        .unwrap();
    assert_eq!(zeta["is_dir"], true, "metadata follows the link");
    assert_eq!(
        zeta["is_symlink"], true,
        "and the lstat flag still says link"
    );
}

#[tokio::test]
async fn browse_non_literal_bool_is_400_plain_text() {
    // Pins the strict wire vocabulary at the HTTP boundary, so nobody discovers the 400
    // by hand. `request()` on the frontend swallows this body
    // (`resp.json().catch(() => null)`), which is fine — the FE never emits this shape.
    let daemon = TestDaemon::spawn(|_repo_root| Ok(())).await.unwrap();
    let tree = seed_tree();

    let resp = browse(
        &daemon,
        Some(tree.path().to_str().unwrap()),
        &[("files", "1")],
    )
    .await;
    assert_eq!(resp.status(), 400, "`files=1` is not a bool literal");
    let body = resp.text().await.unwrap();
    assert!(
        body.contains("Failed to deserialize query string"),
        "axum's own rejection message is the contract, got: {body}"
    );
    assert!(
        serde_json::from_str::<serde_json::Value>(&body).is_err(),
        "the rejection body is plain text, not JSON"
    );
}
