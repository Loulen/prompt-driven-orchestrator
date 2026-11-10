//! #752 — the re-map of an anchor onto another destination (ADR-0067 §5).

use serde::Serialize;
use ts_rs::TS;

use super::{ReviewComment, ReviewSide};
use crate::domain::diff::{FileDiff, FileStatus, LineKind};

/// Where a comment's line is once its anchored side is read at another ref:
/// **reported** at `line` (the line is unchanged — it may have shifted), or
/// **outdated** (the line was edited or deleted). Computed on read, never
/// written to the log: the anchor in the event stays the truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineMapping {
    Reported { line: i64 },
    Outdated,
}

/// Follow `line` (1-based, on the **old** side of `files`) through the hunks of
/// the two-dot diff `orig → target` of one path. A context line keeps its
/// content: reported at its new number. A deleted line — an edit is a delete
/// plus an add — is outdated. A line the diff never mentions shifts by the net
/// growth of the hunks above it. An empty `files` means identical content: the
/// line stays where it is. A file added or deleted between the refs has no
/// line to follow.
pub fn map_line(files: &[FileDiff], line: i64) -> LineMapping {
    if line < 1 {
        return LineMapping::Outdated;
    }
    let Some(file) = files.first() else {
        return LineMapping::Reported { line };
    };
    if file.binary || matches!(file.status, FileStatus::Added | FileStatus::Deleted) {
        return LineMapping::Outdated;
    }
    let line_u = line as u32;
    let mut delta: i64 = 0;
    for hunk in &file.hunks {
        if hunk.old_lines == 0 {
            // Pure insertion: `-N,0` inserts AFTER old line N. Line N and
            // everything above stay; everything below shifts.
            if line_u <= hunk.old_start {
                break;
            }
            delta += hunk.new_lines as i64;
            continue;
        }
        if line_u < hunk.old_start {
            break;
        }
        if line_u < hunk.old_start + hunk.old_lines {
            // Inside the hunk: the line itself is listed, one way or another.
            for l in &hunk.lines {
                if l.old_no == Some(line_u) {
                    return match (l.kind, l.new_no) {
                        (LineKind::Context, Some(n)) => LineMapping::Reported { line: n as i64 },
                        _ => LineMapping::Outdated,
                    };
                }
            }
            // Listed hunk without our line (a malformed patch): be conservative.
            return LineMapping::Outdated;
        }
        delta += hunk.new_lines as i64 - hunk.old_lines as i64;
    }
    LineMapping::Reported { line: line + delta }
}

/// The annotation `GET …/review/comments?from=&to=` and `pdo review list` add to
/// a comment for a displayed pair: `outdated`, and the reported line when the
/// comment still has one. `None` when the mapping cannot be computed (no SHA
/// recorded at send time, a ref that no longer resolves): the caller shows the
/// comment where it was written, without pretending.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[ts(export, rename = "ReviewAnchorMapping")]
pub struct AnchorMapping {
    pub outdated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub mapped_line: Option<i64>,
    /// The reported line differs from the written one.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    #[ts(optional = nullable)]
    pub moved: bool,
}

impl AnchorMapping {
    pub fn from_line(original: i64, mapping: LineMapping) -> AnchorMapping {
        match mapping {
            LineMapping::Reported { line } => AnchorMapping {
                outdated: false,
                mapped_line: Some(line),
                moved: line != original,
            },
            LineMapping::Outdated => AnchorMapping {
                outdated: true,
                mapped_line: None,
                moved: false,
            },
        }
    }
}

/// The SHA a comment's anchored side was written against: `to_sha` for the
/// destination (`new`) side, `from_sha` for the source (`old`) side — old-side
/// comments follow the same rule on the source ref.
pub fn anchored_sha(c: &ReviewComment) -> Option<&str> {
    match c.side {
        ReviewSide::New => c.to_sha.as_deref(),
        ReviewSide::Old => c.from_sha.as_deref(),
    }
}

/// `(orig, target, path) -> parsed two-dot diff of that path`, or `None` when
/// git cannot answer — the one seam [`map_comment`] needs from the repository.
pub type DiffPathFn<'a> = &'a dyn Fn(&str, &str, &str) -> Option<Vec<FileDiff>>;

/// Map one comment onto the displayed pair `(from_sha, to_sha)`. `diff_path`
/// yields the parsed two-dot diff of the comment's path between two SHAs
/// (`None` when git cannot answer — unknown revision). Pure apart from that
/// closure, so the algorithm is testable without a repository.
pub fn map_comment(
    c: &ReviewComment,
    from_sha: Option<&str>,
    to_sha: Option<&str>,
    diff_path: DiffPathFn<'_>,
) -> Option<AnchorMapping> {
    let orig = anchored_sha(c)?;
    let target = match c.side {
        ReviewSide::New => to_sha?,
        ReviewSide::Old => from_sha?,
    };
    if orig == target {
        return Some(AnchorMapping::from_line(
            c.line,
            LineMapping::Reported { line: c.line },
        ));
    }
    let files = diff_path(orig, target, &c.path)?;
    Some(AnchorMapping::from_line(c.line, map_line(&files, c.line)))
}

#[cfg(test)]
mod tests {
    use super::super::fixtures::sent;
    use super::*;

    fn patch(files: &str) -> Vec<FileDiff> {
        crate::domain::diff::parse_patch(files)
    }

    const EDIT_ONE_LINE: &str = "diff --git a/f.rs b/f.rs\nindex 1..2 100644\n--- a/f.rs\n+++ b/f.rs\n@@ -3 +3 @@\n-fn c() {}\n+fn c() { 1 }\n";
    const INSERT_ABOVE: &str = "diff --git a/f.rs b/f.rs\nindex 1..2 100644\n--- a/f.rs\n+++ b/f.rs\n@@ -1,0 +2,2 @@\n+// one\n+// two\n";
    const DELETE_FILE: &str = "diff --git a/f.rs b/f.rs\ndeleted file mode 100644\nindex 1..0\n--- a/f.rs\n+++ /dev/null\n@@ -1,3 +0,0 @@\n-a\n-b\n-c\n";

    #[test]
    fn map_line_reports_an_untouched_line_and_shifts_it_below_an_insertion() {
        // Identical content: the line stays.
        assert_eq!(map_line(&[], 7), LineMapping::Reported { line: 7 });
        // Two lines inserted after line 1: line 1 stays, line 2 becomes 4.
        let p = patch(INSERT_ABOVE);
        assert_eq!(map_line(&p, 1), LineMapping::Reported { line: 1 });
        assert_eq!(map_line(&p, 2), LineMapping::Reported { line: 4 });
        assert_eq!(map_line(&p, 9), LineMapping::Reported { line: 11 });
    }

    #[test]
    fn map_line_marks_an_edited_or_deleted_line_outdated_and_keeps_its_neighbours() {
        let p = patch(EDIT_ONE_LINE);
        assert_eq!(map_line(&p, 3), LineMapping::Outdated);
        assert_eq!(map_line(&p, 2), LineMapping::Reported { line: 2 });
        assert_eq!(map_line(&p, 4), LineMapping::Reported { line: 4 });
        assert_eq!(map_line(&patch(DELETE_FILE), 2), LineMapping::Outdated);
        assert_eq!(map_line(&[], 0), LineMapping::Outdated);
    }

    #[test]
    fn map_comment_uses_the_anchored_side_and_short_circuits_on_the_same_sha() {
        let c = sent("rc-001", 3);
        let calls = std::cell::Cell::new(0);
        let diff = |_: &str, _: &str, _: &str| {
            calls.set(calls.get() + 1);
            Some(patch(EDIT_ONE_LINE))
        };
        // Same destination SHA as written: reported in place, no git.
        let m = map_comment(
            &c,
            Some("a1b2c3d4e5f6a7b8"),
            Some("d4e5f6a7b8c9d0e1"),
            &diff,
        )
        .unwrap();
        assert_eq!(
            m,
            AnchorMapping {
                outdated: false,
                mapped_line: Some(3),
                moved: false
            }
        );
        assert_eq!(calls.get(), 0);
        // Destination moved: the new side is mapped, line 3 was edited.
        let m = map_comment(&c, Some("a1b2c3d4e5f6a7b8"), Some("ffff"), &diff).unwrap();
        assert!(m.outdated && m.mapped_line.is_none());
        assert_eq!(calls.get(), 1);
        // Old-side comment follows the SOURCE ref: destination moving is irrelevant.
        let mut old = sent("rc-002", 2);
        old.side = ReviewSide::Old;
        let m = map_comment(&old, Some("a1b2c3d4e5f6a7b8"), Some("ffff"), &diff).unwrap();
        assert_eq!(
            m,
            AnchorMapping {
                outdated: false,
                mapped_line: Some(2),
                moved: false
            }
        );
        let m = map_comment(&old, Some("eeee"), Some("ffff"), &diff).unwrap();
        assert_eq!(
            m.mapped_line,
            Some(2),
            "line 2 is untouched by the edit of line 3"
        );
        // No SHA recorded / ref gone: no verdict.
        let mut bare = sent("rc-003", 3);
        bare.to_sha = None;
        assert!(map_comment(&bare, Some("x"), Some("y"), &diff).is_none());
        assert!(map_comment(&c, Some("x"), None, &diff).is_none());
        // git cannot answer: no verdict either.
        assert!(map_comment(&c, Some("x"), Some("y"), &|_, _, _| None).is_none());
        let wire = serde_json::to_value(AnchorMapping::from_line(
            3,
            LineMapping::Reported { line: 5 },
        ))
        .unwrap();
        assert_eq!(
            wire,
            serde_json::json!({ "outdated": false, "mapped_line": 5, "moved": true })
        );
    }
}
