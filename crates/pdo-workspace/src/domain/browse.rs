//! The filesystem explorer's rules (`GET /fs/browse`): what one level of a
//! directory lists, in which order, and within which budget.
//!
//! Exposure, stated exactly: the daemon binds `0.0.0.0`, and the LAN reachability
//! is assumed by the owner, so this surface has no authentication and any LAN peer
//! can enumerate, one level per request, whatever the daemon's uid can traverse.
//! It emits names, a path and three booleans — never file CONTENTS, and it has no
//! read path at all. Don't invent a partial mitigation here: it would give a false
//! sense of a boundary and break the staging-profile picker, whose entries live in
//! `$HOME` dotfiles. The right boundary is authenticating the whole HTTP surface.

use serde::Serialize;
use ts_rs::TS;

/// Listing cap, applied **per genre** with directories claiming the budget first.
/// Don't make it one shared budget: files would starve directories, and 50 000 files
/// beside two subdirectories in `~/Downloads` would yield zero navigable rows.
pub const BROWSE_CAP: usize = 1000;

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct BrowseEntry {
    pub name: String,
    /// `canonical_dir.join(name)` — NOT re-canonicalized, so a symlinked child keeps
    /// the path the user clicked rather than collapsing to its target.
    pub path: String,
    /// Cheap `.git`-presence hint (NOT a `git rev-parse` subprocess). A hint, not a
    /// gate: every folder stays pickable and the authoritative validation runs at
    /// selection time via `/repos/validate` (ADR-0001 — sharp tool, not safe tool).
    /// Hard `false` for a file, where it has no meaning.
    pub is_git_repo: bool,
    pub is_symlink: bool,
    /// `true` for a directory (symlinks FOLLOWED), `false` for a regular file. Always
    /// emitted, including under the dirs-only default where it is invariably `true`,
    /// so an entry's shape never depends on the query.
    pub is_dir: bool,
}

/// What a `read_dir` child is, for the purposes of this listing.
#[derive(Debug, PartialEq, Eq)]
pub enum EntryKind {
    Dir,
    File,
    /// Not offered in either mode.
    Skip,
}

/// What the filesystem says a child is, symlinks FOLLOWED.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChildType {
    Dir,
    File,
    /// A fifo, a socket, a device.
    Special,
}

/// Classify one child. `child` is `None` when unclassifiable (broken link,
/// `ELOOP`) → `Skip`.
///
/// Special files (fifo / socket / device) `Skip` in both modes: nothing downstream
/// can consume one — every consumer resolves the picked path, so the row would be
/// unpickable by construction, and a fifo handed to `ensure_image` blocks forever.
pub fn classify_entry(child: Option<ChildType>, include_files: bool) -> EntryKind {
    match child {
        None => EntryKind::Skip,
        Some(ChildType::Dir) => EntryKind::Dir,
        Some(ChildType::File) if include_files => EntryKind::File,
        _ => EntryKind::Skip,
    }
}

/// Merge the two genre buckets: each sorted case-insensitively, directories first,
/// **directories claim the budget first**, total `<= cap`.
///
/// `truncated` is `total > cap`, which is why the caller collects at most `cap + 1` per
/// genre — a `(cap + 1)`th survivor is all `truncated` needs.
pub fn merge_browse_entries(
    mut dirs: Vec<BrowseEntry>,
    mut files: Vec<BrowseEntry>,
    cap: usize,
) -> (Vec<BrowseEntry>, bool) {
    dirs.sort_by_key(|a| a.name.to_lowercase());
    files.sort_by_key(|a| a.name.to_lowercase());
    let truncated = dirs.len() + files.len() > cap;
    dirs.truncate(cap);
    files.truncate(cap - dirs.len());
    dirs.extend(files);
    (dirs, truncated)
}

/// Why the requested browse root could not be resolved into a listable directory.
#[derive(Debug, PartialEq, Eq)]
pub enum BrowseRootError {
    /// `path` was provided but is not absolute (caller bug → 400).
    Relative,
    /// No directory in the default chain (`$HOME → repo_root → /`) exists (→ 500).
    NoDefault,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dirs_are_offered_always_and_files_only_on_demand() {
        assert_eq!(classify_entry(Some(ChildType::Dir), false), EntryKind::Dir);
        assert_eq!(
            classify_entry(Some(ChildType::File), false),
            EntryKind::Skip
        );
        assert_eq!(classify_entry(Some(ChildType::File), true), EntryKind::File);
        assert_eq!(
            classify_entry(Some(ChildType::Special), true),
            EntryKind::Skip
        );
        assert_eq!(classify_entry(None, true), EntryKind::Skip);
    }

    fn browse_entry(name: &str, is_dir: bool) -> BrowseEntry {
        BrowseEntry {
            name: name.to_string(),
            path: format!("/x/{name}"),
            is_git_repo: false,
            is_symlink: false,
            is_dir,
        }
    }

    #[test]
    fn merge_browse_entries_puts_dirs_before_files() {
        let (entries, truncated) = merge_browse_entries(
            vec![browse_entry("z-dir", true)],
            vec![browse_entry("a-file", false)],
            BROWSE_CAP,
        );
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["z-dir", "a-file"],
            "genre beats name: a navigable dir is never buried under files"
        );
        assert!(!truncated);
    }

    #[test]
    fn merge_browse_entries_gives_dirs_first_claim() {
        // 3 dirs + 2000 files: the dirs are never starved, files fill what's left.
        let dirs: Vec<BrowseEntry> = (0..3)
            .map(|i| browse_entry(&format!("d{i}"), true))
            .collect();
        let files: Vec<BrowseEntry> = (0..2000)
            .map(|i| browse_entry(&format!("f{i:04}"), false))
            .collect();
        let (entries, truncated) = merge_browse_entries(dirs, files, 1000);
        assert_eq!(entries.len(), 1000);
        assert_eq!(entries.iter().filter(|e| e.is_dir).count(), 3);
        assert_eq!(entries.iter().filter(|e| !e.is_dir).count(), 997);
        assert!(entries[..3].iter().all(|e| e.is_dir), "dirs come first");
        assert!(truncated);

        // 1500 dirs + 10 files: dirs consume the whole budget, files get nothing.
        let dirs: Vec<BrowseEntry> = (0..1500)
            .map(|i| browse_entry(&format!("d{i:04}"), true))
            .collect();
        let files: Vec<BrowseEntry> = (0..10)
            .map(|i| browse_entry(&format!("f{i}"), false))
            .collect();
        let (entries, truncated) = merge_browse_entries(dirs, files, 1000);
        assert_eq!(entries.len(), 1000);
        assert!(
            entries.iter().all(|e| e.is_dir),
            "dirs claim the budget first"
        );
        assert!(truncated);
    }

    #[test]
    fn merge_browse_entries_dirs_only_matches_pre_431() {
        // The default path: `files` empty ⇒ identical to the pre-#431
        // `sort_by_key(name.to_lowercase()) + truncate(cap)`, and NOT truncated under
        // the cap.
        let dirs = vec![
            browse_entry("Zeta", true),
            browse_entry("alpha", true),
            browse_entry("Beta", true),
        ];
        let (entries, truncated) = merge_browse_entries(dirs, Vec::new(), BROWSE_CAP);
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "Beta", "Zeta"]);
        assert!(!truncated);

        // Exactly `cap` dirs is NOT truncated (the pre-#431 `len > cap` semantics).
        let dirs: Vec<BrowseEntry> = (0..1000)
            .map(|i| browse_entry(&format!("d{i:04}"), true))
            .collect();
        let (entries, truncated) = merge_browse_entries(dirs, Vec::new(), 1000);
        assert_eq!(entries.len(), 1000);
        assert!(!truncated, "exactly `cap` entries is not a truncation");
    }
}
