//! The unified patch, parsed: one [`FileDiff`] per `diff --git` chunk.

use super::paths::{decode_path, header_path, marker_path, strip_prefix_or_null};
use super::{DiffLine, FileDiff, FileStatus, Hunk, LineKind};

/// Parse a full unified `git diff` into per-file sections, in patch order.
/// `[]` for empty/whitespace input. Never fails: a chunk it cannot understand
/// still yields a `FileDiff` with empty paths.
pub fn parse_patch(raw: &str) -> Vec<FileDiff> {
    if raw.trim().is_empty() {
        return Vec::new();
    }
    let mut chunks: Vec<Vec<&str>> = Vec::new();
    for line in raw.split('\n') {
        if line.starts_with("diff --git ") {
            chunks.push(vec![line]);
        } else if let Some(cur) = chunks.last_mut() {
            cur.push(line);
        }
        // Preamble before the first `diff --git` is dropped.
    }
    chunks.iter().map(|c| parse_chunk(c)).collect()
}

fn parse_chunk(lines: &[&str]) -> FileDiff {
    let mut rename_from: Option<String> = None;
    let mut rename_to: Option<String> = None;
    let mut copy_from: Option<String> = None;
    let mut copy_to: Option<String> = None;
    // `Some(None)` = marker seen and it was /dev/null.
    let mut minus_path: Option<Option<String>> = None;
    let mut plus_path: Option<Option<String>> = None;
    let mut binary_old: Option<String> = None;
    let mut binary_new: Option<String> = None;
    let mut binary = false;
    let mut new_file_mode = false;
    let mut deleted_file_mode = false;
    let mut hunks: Vec<Hunk> = Vec::new();
    let mut additions = 0u32;
    let mut deletions = 0u32;
    // Running line counters for the current hunk.
    let mut old_no = 0u32;
    let mut new_no = 0u32;
    let mut in_hunk = false;

    for (i, line) in lines.iter().enumerate() {
        if i == 0 {
            continue; // the `diff --git` header, used only as a last-resort path
        }
        if in_hunk {
            if let Some(rest) = line.strip_prefix('+') {
                if let Some(h) = hunks.last_mut() {
                    h.lines.push(DiffLine {
                        kind: LineKind::Add,
                        content: rest.to_string(),
                        old_no: None,
                        new_no: Some(new_no),
                    });
                }
                new_no += 1;
                additions += 1;
                continue;
            }
            if let Some(rest) = line.strip_prefix('-') {
                if let Some(h) = hunks.last_mut() {
                    h.lines.push(DiffLine {
                        kind: LineKind::Del,
                        content: rest.to_string(),
                        old_no: Some(old_no),
                        new_no: None,
                    });
                }
                old_no += 1;
                deletions += 1;
                continue;
            }
            if let Some(rest) = line.strip_prefix(' ') {
                if let Some(h) = hunks.last_mut() {
                    h.lines.push(DiffLine {
                        kind: LineKind::Context,
                        content: rest.to_string(),
                        old_no: Some(old_no),
                        new_no: Some(new_no),
                    });
                }
                old_no += 1;
                new_no += 1;
                continue;
            }
            if line.starts_with('\\') {
                // `\ No newline at end of file` — not a line of either side.
                continue;
            }
            if line.is_empty() {
                // A trailing empty string from the final `\n` split; git never
                // emits an empty hunk line (context lines carry a leading space).
                continue;
            }
            // Anything else ends the hunk (a new `@@` is handled below).
            in_hunk = false;
        }

        if let Some(h) = parse_hunk_header(line) {
            old_no = h.old_start;
            new_no = h.new_start;
            hunks.push(h);
            in_hunk = true;
        } else if let Some(p) = line.strip_prefix("rename from ") {
            rename_from = decode_path(p);
        } else if let Some(p) = line.strip_prefix("rename to ") {
            rename_to = decode_path(p);
        } else if let Some(p) = line.strip_prefix("copy from ") {
            copy_from = decode_path(p);
        } else if let Some(p) = line.strip_prefix("copy to ") {
            copy_to = decode_path(p);
        } else if line.starts_with("new file mode ") {
            new_file_mode = true;
        } else if line.starts_with("deleted file mode ") {
            deleted_file_mode = true;
        } else if let Some(rest) = line.strip_prefix("--- ") {
            minus_path = Some(marker_path(rest, "a/"));
        } else if let Some(rest) = line.strip_prefix("+++ ") {
            plus_path = Some(marker_path(rest, "b/"));
        } else if *line == "GIT binary patch" {
            binary = true;
        } else if let Some(rest) = line.strip_prefix("Binary files ") {
            binary = true;
            if let Some(mid) = rest.strip_suffix(" differ") {
                if let Some((a, b)) = mid.split_once(" and ") {
                    binary_old = strip_prefix_or_null(a, "a/");
                    binary_new = strip_prefix_or_null(b, "b/");
                }
            }
        }
        // `index`, `similarity index`, mode lines: ignored.
    }

    // Path ladder: rename/copy → ---/+++ → Binary files → `diff --git` header.
    let (old_path, new_path) = if rename_from.is_some() || rename_to.is_some() {
        (rename_from.clone(), rename_to.clone())
    } else if copy_from.is_some() || copy_to.is_some() {
        (copy_from.clone(), copy_to.clone())
    } else if minus_path.is_some() || plus_path.is_some() {
        (minus_path.flatten(), plus_path.flatten())
    } else if binary_old.is_some() || binary_new.is_some() {
        (binary_old, binary_new)
    } else {
        let p = header_path(lines[0]);
        (p.clone(), p)
    };

    let is_rename = rename_from.is_some() || rename_to.is_some();
    let is_copy = !is_rename && (copy_from.is_some() || copy_to.is_some());
    let status = if is_rename {
        FileStatus::Renamed
    } else if is_copy {
        FileStatus::Copied
    } else if new_file_mode || old_path.is_none() {
        FileStatus::Added
    } else if deleted_file_mode || new_path.is_none() {
        FileStatus::Deleted
    } else {
        FileStatus::Modified
    };

    if binary {
        additions = 0;
        deletions = 0;
        hunks.clear();
    }

    FileDiff {
        old_path,
        new_path,
        status,
        binary,
        additions,
        deletions,
        hunks,
    }
}

/// `@@ -a[,b] +c[,d] @@[ header]`.
fn parse_hunk_header(line: &str) -> Option<Hunk> {
    let rest = line.strip_prefix("@@ -")?;
    let (ranges, header) = rest.split_once(" @@")?;
    let (old, new) = ranges.split_once(" +")?;
    let (old_start, old_lines) = parse_range(old)?;
    let (new_start, new_lines) = parse_range(new)?;
    Some(Hunk {
        old_start,
        old_lines,
        new_start,
        new_lines,
        header: header.strip_prefix(' ').unwrap_or(header).to_string(),
        lines: Vec::new(),
    })
}

fn parse_range(s: &str) -> Option<(u32, u32)> {
    match s.split_once(',') {
        Some((a, b)) => Some((a.parse().ok()?, b.parse().ok()?)),
        None => Some((s.parse().ok()?, 1)),
    }
}

#[cfg(test)]
mod tests {
    //! Migrated from `frontend/src/lib/parseUnifiedDiff.test.ts` (#748): the
    //! parser moved server-side, its cases came with it.
    use super::*;

    fn one(raw: &str) -> FileDiff {
        let files = parse_patch(raw);
        assert_eq!(files.len(), 1, "expected one file: {files:?}");
        files.into_iter().next().unwrap()
    }

    #[test]
    fn empty_and_whitespace_yield_no_files() {
        assert!(parse_patch("").is_empty());
        assert!(parse_patch("  \n\n\t").is_empty());
    }

    #[test]
    fn modified_file_with_counts_and_line_numbers() {
        let raw = "diff --git a/src/a.rs b/src/a.rs\nindex 111..222 100644\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1,3 +1,3 @@ fn ctx\n ctx\n-old a\n+new a\n ctx2\n";
        let f = one(raw);
        assert_eq!(f.old_path.as_deref(), Some("src/a.rs"));
        assert_eq!(f.new_path.as_deref(), Some("src/a.rs"));
        assert_eq!(f.status, FileStatus::Modified);
        assert!(!f.binary);
        assert_eq!((f.additions, f.deletions), (1, 1));
        assert_eq!(f.hunks.len(), 1);
        let h = &f.hunks[0];
        assert_eq!(
            (h.old_start, h.old_lines, h.new_start, h.new_lines),
            (1, 3, 1, 3)
        );
        assert_eq!(h.header, "fn ctx");
        assert_eq!(
            h.lines,
            vec![
                DiffLine {
                    kind: LineKind::Context,
                    content: "ctx".into(),
                    old_no: Some(1),
                    new_no: Some(1)
                },
                DiffLine {
                    kind: LineKind::Del,
                    content: "old a".into(),
                    old_no: Some(2),
                    new_no: None
                },
                DiffLine {
                    kind: LineKind::Add,
                    content: "new a".into(),
                    old_no: None,
                    new_no: Some(2)
                },
                DiffLine {
                    kind: LineKind::Context,
                    content: "ctx2".into(),
                    old_no: Some(3),
                    new_no: Some(3)
                },
            ]
        );
    }

    #[test]
    fn multiple_files_preserve_order() {
        let raw = [
            "diff --git a/src/a.rs b/src/a.rs",
            "--- a/src/a.rs",
            "+++ b/src/a.rs",
            "@@ -1 +1 @@",
            "-old a",
            "+new a",
            "diff --git a/src/b.rs b/src/b.rs",
            "new file mode 100644",
            "index 0000000..333",
            "--- /dev/null",
            "+++ b/src/b.rs",
            "@@ -0,0 +1,2 @@",
            "+line 1",
            "+line 2",
            "",
        ]
        .join("\n");
        let files = parse_patch(&raw);
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].new_path.as_deref(), Some("src/a.rs"));
        assert_eq!(files[1].new_path.as_deref(), Some("src/b.rs"));
        assert_eq!(files[1].status, FileStatus::Added);
        assert!(files[1].old_path.is_none());
        assert_eq!((files[1].additions, files[1].deletions), (2, 0));
        // A one-line range `@@ -0,0 +1,2 @@` numbers the new side from 1.
        assert_eq!(files[1].hunks[0].lines[0].new_no, Some(1));
        assert_eq!(files[1].hunks[0].lines[1].new_no, Some(2));
    }

    #[test]
    fn deletion_has_no_new_path() {
        let raw = "diff --git a/gone.rs b/gone.rs\ndeleted file mode 100644\n--- a/gone.rs\n+++ /dev/null\n@@ -1,2 +0,0 @@\n-a\n-b\n";
        let f = one(raw);
        assert_eq!(f.status, FileStatus::Deleted);
        assert_eq!(f.old_path.as_deref(), Some("gone.rs"));
        assert!(f.new_path.is_none());
        assert_eq!((f.additions, f.deletions), (0, 2));
    }

    #[test]
    fn pure_rename_without_hunks() {
        let raw = "diff --git a/old.rs b/new.rs\nsimilarity index 100%\nrename from old.rs\nrename to new.rs\n";
        let f = one(raw);
        assert_eq!(f.status, FileStatus::Renamed);
        assert_eq!(f.old_path.as_deref(), Some("old.rs"));
        assert_eq!(f.new_path.as_deref(), Some("new.rs"));
        assert_eq!((f.additions, f.deletions), (0, 0));
        assert!(f.hunks.is_empty());
    }

    #[test]
    fn rename_with_edits_keeps_counts() {
        let raw = "diff --git a/old.rs b/new.rs\nsimilarity index 90%\nrename from old.rs\nrename to new.rs\n--- a/old.rs\n+++ b/new.rs\n@@ -1 +1 @@\n-x\n+y\n";
        let f = one(raw);
        assert_eq!(f.status, FileStatus::Renamed);
        assert_eq!(f.new_path.as_deref(), Some("new.rs"));
        assert_eq!((f.additions, f.deletions), (1, 1));
    }

    #[test]
    fn copy_is_recognised() {
        let raw = "diff --git a/a.rs b/b.rs\nsimilarity index 100%\ncopy from a.rs\ncopy to b.rs\n";
        let f = one(raw);
        assert_eq!(f.status, FileStatus::Copied);
        assert_eq!(f.old_path.as_deref(), Some("a.rs"));
        assert_eq!(f.new_path.as_deref(), Some("b.rs"));
    }

    #[test]
    fn modified_binary_is_binary_with_zero_counts() {
        let raw = "diff --git a/img.png b/img.png\nindex 111..222 100644\nBinary files a/img.png and b/img.png differ\n";
        let f = one(raw);
        assert!(f.binary);
        assert_eq!(f.status, FileStatus::Modified);
        assert_eq!(f.new_path.as_deref(), Some("img.png"));
        assert_eq!((f.additions, f.deletions), (0, 0));
    }

    #[test]
    fn new_binary_is_added() {
        let raw = "diff --git a/img.png b/img.png\nnew file mode 100644\nindex 0000000..222\nBinary files /dev/null and b/img.png differ\n";
        let f = one(raw);
        assert!(f.binary);
        assert_eq!(f.status, FileStatus::Added);
        assert!(f.old_path.is_none());
        assert_eq!(f.new_path.as_deref(), Some("img.png"));
    }

    #[test]
    fn git_binary_patch_marker_is_binary() {
        let raw = "diff --git a/x.bin b/x.bin\nindex 111..222 100644\nGIT binary patch\nliteral 3\nKcmZQzWMKEfU|<7;00000\n\n";
        let f = one(raw);
        assert!(f.binary);
        assert_eq!(f.new_path.as_deref(), Some("x.bin"));
        assert!(f.hunks.is_empty());
    }

    #[test]
    fn path_with_spaces_uses_trailing_tab() {
        let raw = "diff --git a/my file.txt b/my file.txt\n--- a/my file.txt\t\n+++ b/my file.txt\t\n@@ -1 +1 @@\n-a\n+b\n";
        let f = one(raw);
        assert_eq!(f.new_path.as_deref(), Some("my file.txt"));
    }

    #[test]
    fn c_quoted_unicode_path_is_decoded() {
        // "é" = \303\251 in git's C-quoting.
        let raw = "diff --git \"a/caf\\303\\251.txt\" \"b/caf\\303\\251.txt\"\n--- \"a/caf\\303\\251.txt\"\n+++ \"b/caf\\303\\251.txt\"\n@@ -1 +1 @@\n-a\n+b\n";
        let f = one(raw);
        assert_eq!(f.new_path.as_deref(), Some("café.txt"));
    }

    #[test]
    fn no_newline_marker_is_not_counted() {
        let raw = "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1 +1 @@\n-a\n\\ No newline at end of file\n+b\n\\ No newline at end of file\n";
        let f = one(raw);
        assert_eq!((f.additions, f.deletions), (1, 1));
        assert_eq!(f.hunks[0].lines.len(), 2);
    }

    #[test]
    fn malformed_chunk_never_panics() {
        let f = one("diff --git nonsense\n@@ garbage @@\n+x\n");
        assert!(f.old_path.is_none() && f.new_path.is_none());
        // `+x` outside a recognised hunk is not counted.
        assert_eq!(f.additions, 0);
    }

    #[test]
    fn mode_only_change_takes_path_from_header() {
        let raw = "diff --git a/run.sh b/run.sh\nold mode 100644\nnew mode 100755\n";
        let f = one(raw);
        assert_eq!(f.new_path.as_deref(), Some("run.sh"));
        assert_eq!(f.status, FileStatus::Modified);
    }

    #[test]
    fn two_hunks_restart_numbering() {
        let raw = "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1,2 +1,2 @@\n a\n-b\n+B\n@@ -10,2 +10,3 @@\n j\n+K\n k\n";
        let f = one(raw);
        assert_eq!(f.hunks.len(), 2);
        assert_eq!(f.hunks[1].lines[0].old_no, Some(10));
        assert_eq!(f.hunks[1].lines[1].new_no, Some(11));
        assert_eq!(f.hunks[1].lines[2].old_no, Some(11));
        assert_eq!(f.hunks[1].lines[2].new_no, Some(12));
    }
}
