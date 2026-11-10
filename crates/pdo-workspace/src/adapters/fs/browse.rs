//! The filesystem explorer (`GET /fs/browse`): one level of a directory, read
//! only. The rules of the listing live in [`crate::domain::browse`].

use std::path::{Path, PathBuf};

use crate::application::ports::{FileBrowser, Listing};
use crate::domain::browse::{
    classify_entry, merge_browse_entries, BrowseEntry, BrowseRootError, ChildType, EntryKind,
    BROWSE_CAP,
};

/// First existing directory in `[home, repo_root, "/"]`. `None` only when none of the
/// three is an existing directory.
pub fn default_chain(home: Option<&Path>, repo_root: &Path) -> Option<PathBuf> {
    if let Some(h) = home {
        if h.is_dir() {
            return Some(h.to_path_buf());
        }
    }
    if repo_root.is_dir() {
        return Some(repo_root.to_path_buf());
    }
    let root = Path::new("/");
    if root.is_dir() {
        return Some(root.to_path_buf());
    }
    None
}

/// Resolve the directory to list. `home`/`repo_root` are injected rather than read
/// from the real env, so it stays deterministic under cargo's parallel test threads.
pub fn resolve_browse_root(
    path: Option<&str>,
    home: Option<&Path>,
    repo_root: &Path,
) -> Result<PathBuf, BrowseRootError> {
    match path {
        Some(p) => {
            let pb = PathBuf::from(p);
            if !pb.is_absolute() {
                return Err(BrowseRootError::Relative);
            }
            if pb.is_dir() {
                Ok(pb)
            } else if pb.is_file() {
                Ok(pb.parent().map(Path::to_path_buf).unwrap_or(pb))
            } else {
                // Non-existent (e.g. a stale/half-typed combobox value) → clamp to the
                // default so the explorer opens gracefully instead of erroring.
                default_chain(home, repo_root).ok_or(BrowseRootError::NoDefault)
            }
        }
        None => default_chain(home, repo_root).ok_or(BrowseRootError::NoDefault),
    }
}

/// Human label for the in-body `error` when a navigable directory can't be listed.
pub fn browse_io_label(e: &std::io::Error) -> &'static str {
    match e.kind() {
        std::io::ErrorKind::PermissionDenied => "permission denied",
        std::io::ErrorKind::NotFound => "not found",
        _ => "could not read directory",
    }
}

/// What `std::fs::metadata` (which FOLLOWS symlinks) says a child is.
pub fn child_type(meta: &std::fs::Metadata) -> ChildType {
    if meta.is_dir() {
        ChildType::Dir
    } else if meta.is_file() {
        ChildType::File
    } else {
        ChildType::Special
    }
}

/// The explorer over the real filesystem. The default chain starts at `$HOME`,
/// read at each request, then falls back to the daemon's repository root.
pub struct FsBrowser {
    repo_root: PathBuf,
}

impl FsBrowser {
    pub fn new(repo_root: impl Into<PathBuf>) -> Self {
        Self {
            repo_root: repo_root.into(),
        }
    }
}

impl FileBrowser for FsBrowser {
    fn browse(
        &self,
        path: Option<&str>,
        include_files: bool,
        hidden: bool,
    ) -> Result<Listing, BrowseRootError> {
        let home = std::env::var("HOME").ok().map(PathBuf::from);
        let resolved = resolve_browse_root(path, home.as_deref(), &self.repo_root)?;
        Ok(list_level(&resolved, include_files, hidden))
    }
}

/// List one level of `resolved`.
pub fn list_level(resolved: &Path, include_files: bool, hidden: bool) -> Listing {
    // Canonicalize the requested dir ONLY. Don't re-canonicalize children: that
    // collapses symlinks and shows a path the user didn't click. On failure fall back
    // to the un-canonicalized path so `read_dir` below produces a clean in-body error
    // rather than a 500.
    let canonical_dir = std::fs::canonicalize(resolved).unwrap_or_else(|_| resolved.to_path_buf());
    let parent = canonical_dir
        .parent()
        .map(|p| p.to_string_lossy().into_owned());
    let path = canonical_dir.to_string_lossy().into_owned();

    let read = match std::fs::read_dir(&canonical_dir) {
        Ok(rd) => rd,
        Err(e) => {
            // Navigable-but-unlistable (EACCES, etc.): an in-body error with the
            // breadcrumb preserved, so the user is never stranded.
            return Listing {
                error: Some(format!(
                    "{}: {}",
                    browse_io_label(&e),
                    canonical_dir.display()
                )),
                path,
                parent,
                entries: Vec::new(),
                truncated: false,
            };
        }
    };

    // Every child is collected, then sorted, then capped (`merge_browse_entries`).
    // An early stop at CAP+1 kept the alphabetically-first CAP of an arbitrary
    // `read_dir` sample: in a `/tmp` of 1 118 folders, the repository the user was
    // sent to pick was not listed at all. The stat per child was already paid for
    // every listed entry; a level is one `read_dir`, names only.
    let mut dirs: Vec<BrowseEntry> = Vec::new();
    let mut files: Vec<BrowseEntry> = Vec::new();
    for child in read {
        let Ok(child) = child else {
            continue; // skip an entry that can't be stat'd mid-listing
        };
        let name = child.file_name().to_string_lossy().into_owned();
        if !hidden && name.starts_with('.') {
            continue; // dot-entries hidden unless `?hidden=true`; `.`/`..` never yielded
        }
        let is_symlink = child.file_type().map(|t| t.is_symlink()).unwrap_or(false);
        // Don't use `DirEntry::metadata()`: it lstat's, so every symlinked dir would
        // report as non-dir. The free `std::fs::metadata` FOLLOWS the link.
        let meta = std::fs::metadata(child.path()).ok();
        let kind = classify_entry(meta.as_ref().map(child_type), include_files);
        if kind == EntryKind::Skip {
            continue;
        }
        let is_dir = kind == EntryKind::Dir;
        let entry_path = canonical_dir.join(&name);
        let is_git_repo = is_dir && entry_path.join(".git").exists();
        let entry = BrowseEntry {
            name,
            path: entry_path.to_string_lossy().into_owned(),
            is_git_repo,
            is_symlink,
            is_dir,
        };
        if is_dir {
            dirs.push(entry);
        } else {
            files.push(entry);
        }
    }

    let (entries, truncated) = merge_browse_entries(dirs, files, BROWSE_CAP);
    Listing {
        path,
        parent,
        entries,
        truncated,
        error: None,
    }
}
