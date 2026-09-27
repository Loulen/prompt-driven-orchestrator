//! The explorer's root resolution and entry classification on a real
//! filesystem.

use std::path::Path;

use pdo_workspace::adapters::fs::browse::{child_type, list_level, resolve_browse_root};
use pdo_workspace::domain::browse::{classify_entry, BrowseRootError, EntryKind};

fn classify_browse_entry(meta: Option<&std::fs::Metadata>, include_files: bool) -> EntryKind {
    classify_entry(meta.map(child_type), include_files)
}

#[test]
fn a_level_lists_dirs_first_hides_dot_entries_and_flags_repositories() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("repo/.git")).unwrap();
    std::fs::create_dir(tmp.path().join(".hidden")).unwrap();
    std::fs::write(tmp.path().join("a.txt"), "x").unwrap();

    let listing = list_level(tmp.path(), true, false);
    let names: Vec<_> = listing.entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, vec!["repo", "a.txt"]);
    assert!(listing.entries[0].is_git_repo && listing.entries[0].is_dir);
    assert!(!listing.truncated && listing.error.is_none());

    let with_hidden = list_level(tmp.path(), false, true);
    let names: Vec<_> = with_hidden
        .entries
        .iter()
        .map(|e| e.name.as_str())
        .collect();
    assert_eq!(names, vec![".hidden", "repo"]);
}

#[test]
fn an_unlistable_level_keeps_its_breadcrumb_and_names_the_error() {
    let listing = list_level(Path::new("/this/does/not/exist/917"), false, false);
    assert_eq!(listing.path, "/this/does/not/exist/917");
    assert_eq!(listing.parent.as_deref(), Some("/this/does/not/exist"));
    assert!(listing.error.unwrap().starts_with("not found: "));
}

// GET /fs/browse default-root resolution (issue #131)
//
// `resolve_browse_root` is pure w.r.t. the environment (home/repo_root injected),
// so these never touch the real `$HOME` and are safe under cargo's parallel test
// threads. Each test seeds real temp dirs so the `is_dir`/`is_file` probes are
// deterministic.

#[test]
fn resolve_browse_root_explicit_existing_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let repo_root = tempfile::tempdir().unwrap();
    let got =
        resolve_browse_root(Some(tmp.path().to_str().unwrap()), None, repo_root.path()).unwrap();
    assert_eq!(got, tmp.path());
}

#[test]
fn resolve_browse_root_file_resolves_to_parent() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("notes.txt");
    std::fs::write(&file, "x").unwrap();
    let repo_root = tempfile::tempdir().unwrap();
    let got = resolve_browse_root(Some(file.to_str().unwrap()), None, repo_root.path()).unwrap();
    assert_eq!(got, tmp.path());
}

#[test]
fn resolve_browse_root_nonexistent_clamps_to_home() {
    let home = tempfile::tempdir().unwrap();
    let repo_root = tempfile::tempdir().unwrap();
    let got = resolve_browse_root(
        Some("/this/path/definitely/does/not/exist/131"),
        Some(home.path()),
        repo_root.path(),
    )
    .unwrap();
    assert_eq!(
        got,
        home.path(),
        "non-existent path clamps to the default (home)"
    );
}

#[test]
fn resolve_browse_root_relative_path_errors() {
    let repo_root = tempfile::tempdir().unwrap();
    let err = resolve_browse_root(Some("relative/path"), None, repo_root.path()).unwrap_err();
    assert_eq!(err, BrowseRootError::Relative);
}

#[test]
fn resolve_browse_root_none_uses_home() {
    let home = tempfile::tempdir().unwrap();
    let repo_root = tempfile::tempdir().unwrap();
    let got = resolve_browse_root(None, Some(home.path()), repo_root.path()).unwrap();
    assert_eq!(got, home.path());
}

#[test]
fn resolve_browse_root_none_without_home_uses_repo_root() {
    let repo_root = tempfile::tempdir().unwrap();
    let got = resolve_browse_root(None, None, repo_root.path()).unwrap();
    assert_eq!(got, repo_root.path());
}

#[test]
fn resolve_browse_root_falls_through_to_filesystem_root() {
    // No home, and a repo_root that does not exist → falls through to `/`
    // (which always exists), so the chain never collapses in practice.
    let got =
        resolve_browse_root(None, None, Path::new("/this/repo/root/does/not/exist/131")).unwrap();
    assert_eq!(got, Path::new("/"));
}

#[test]
fn classify_browse_entry_offers_dirs_always_and_files_only_on_demand() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("a-dir");
    std::fs::create_dir(&dir).unwrap();
    let file = tmp.path().join("a-file");
    std::fs::write(&file, "x").unwrap();
    // A REAL symlink → dir, so `std::fs::metadata` genuinely follows it (the
    // `DirEntry::metadata` trap this guards against).
    let link = tmp.path().join("a-link");
    std::os::unix::fs::symlink(&dir, &link).unwrap();

    let m = |p: &Path| std::fs::metadata(p).ok();

    // Directories are offered in BOTH modes.
    assert_eq!(
        classify_browse_entry(m(&dir).as_ref(), false),
        EntryKind::Dir
    );
    assert_eq!(
        classify_browse_entry(m(&dir).as_ref(), true),
        EntryKind::Dir
    );
    // A symlink to a dir is a dir (metadata follows).
    assert_eq!(
        classify_browse_entry(m(&link).as_ref(), false),
        EntryKind::Dir
    );
    // A regular file only when asked for.
    assert_eq!(
        classify_browse_entry(m(&file).as_ref(), false),
        EntryKind::Skip
    );
    assert_eq!(
        classify_browse_entry(m(&file).as_ref(), true),
        EntryKind::File
    );
}

#[test]
fn classify_browse_entry_skips_broken_links_and_specials_in_every_mode() {
    // Unclassifiable (broken link / ELOOP): `metadata` errs → None.
    assert_eq!(classify_browse_entry(None, false), EntryKind::Skip);
    assert_eq!(
        classify_browse_entry(None, true),
        EntryKind::Skip,
        "a dangling link must stay invisible even with files=true"
    );

    // A special file: `Some(meta)` that is neither a dir nor a regular file.
    let tmp = tempfile::tempdir().unwrap();
    let sock = tmp.path().join("sock");
    let _listener = std::os::unix::net::UnixListener::bind(&sock).unwrap();
    let meta = std::fs::metadata(&sock).unwrap();
    assert!(!meta.is_dir() && !meta.is_file(), "fixture must be special");
    assert_eq!(classify_browse_entry(Some(&meta), false), EntryKind::Skip);
    assert_eq!(
        classify_browse_entry(Some(&meta), true),
        EntryKind::Skip,
        "a socket/fifo must stay invisible: nothing downstream can consume one"
    );
}
