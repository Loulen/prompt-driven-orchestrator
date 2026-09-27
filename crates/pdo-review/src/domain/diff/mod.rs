//! A Run's diff as data (#748, ADR-0067): a `git diff` between two refs
//! turned into files → hunks → lines, so the Diff tab and the Review page
//! render from data instead of re-parsing the raw patch in the browser.
//!
//! The server-side successor of the frontend's `parseUnifiedDiff.ts`: same
//! contract (best-effort, never fails on a chunk it does not fully
//! understand), same path ladder (`rename from/to` → `---`/`+++` → `Binary
//! files` → `diff --git` header), same counting rule (`+`/`-` lines inside
//! hunks; a binary file counts as a file with 0/0).

mod parse;
mod paths;

pub use parse::parse_patch;

use serde::Serialize;
use ts_rs::TS;

#[derive(Serialize, Debug, PartialEq, Eq, Clone, Copy, TS)]
#[ts(export, rename = "DiffFileStatus")]
#[serde(rename_all = "snake_case")]
pub enum FileStatus {
    Added,
    Deleted,
    Modified,
    Renamed,
    Copied,
}

#[derive(Serialize, Debug, PartialEq, Eq, Clone, Copy, TS)]
#[ts(export, rename = "DiffLineKind")]
#[serde(rename_all = "snake_case")]
pub enum LineKind {
    Context,
    Add,
    Del,
}

/// One line of a hunk. `old_no`/`new_no` are the 1-based line numbers on each
/// side — `None` on the side the line does not exist on (`Add` has no `old_no`,
/// `Del` has no `new_no`).
#[derive(Serialize, Debug, PartialEq, Eq, Clone, TS)]
#[ts(export)]
pub struct DiffLine {
    pub kind: LineKind,
    /// The line text WITHOUT its leading `+`/`-`/space marker.
    pub content: String,
    pub old_no: Option<u32>,
    pub new_no: Option<u32>,
}

#[derive(Serialize, Debug, PartialEq, Eq, Clone, TS)]
#[ts(export, rename = "DiffHunk")]
pub struct Hunk {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    /// The function context git prints after the second `@@` (may be empty).
    pub header: String,
    pub lines: Vec<DiffLine>,
}

#[derive(Serialize, Debug, PartialEq, Eq, Clone, TS)]
#[ts(export, rename = "DiffFile")]
pub struct FileDiff {
    /// Source path, `None` for a pure addition.
    pub old_path: Option<String>,
    /// Destination path, `None` for a pure deletion.
    pub new_path: Option<String>,
    pub status: FileStatus,
    pub binary: bool,
    pub additions: u32,
    pub deletions: u32,
    pub hunks: Vec<Hunk>,
}

/// The whole answer of `GET /runs/<id>/diff/structured`.
#[derive(Serialize, Debug, Clone, TS)]
#[ts(export)]
pub struct StructuredDiff {
    /// The source ref as asked (or defaulted), verbatim.
    pub from_ref: String,
    /// The destination ref as asked (or defaulted), verbatim.
    pub to_ref: String,
    /// `from_ref` resolved to a commit SHA (best-effort, `None` if unresolvable).
    pub from_sha: Option<String>,
    /// `to_ref` resolved to a commit SHA.
    pub to_sha: Option<String>,
    /// `true` when the pair was compared with the three-dot (merge-base) form —
    /// the fork → tip default, identical to the LOC stat.
    pub three_dot: bool,
    pub files: Vec<FileDiff>,
    pub additions: u32,
    pub deletions: u32,
    pub files_changed: u32,
}

impl StructuredDiff {
    /// The answer for the parsed `files` of `from_ref` → `to_ref`, with the
    /// totals the summary shows.
    pub fn of(
        from_ref: &str,
        to_ref: &str,
        from_sha: Option<String>,
        to_sha: Option<String>,
        three_dot: bool,
        files: Vec<FileDiff>,
    ) -> StructuredDiff {
        let additions = files.iter().map(|f| f.additions).sum();
        let deletions = files.iter().map(|f| f.deletions).sum();
        let files_changed = files.len() as u32;
        StructuredDiff {
            from_ref: from_ref.to_string(),
            to_ref: to_ref.to_string(),
            from_sha,
            to_sha,
            three_dot,
            files,
            additions,
            deletions,
            files_changed,
        }
    }
}
