//! The Run's diff as `pdo review` reads it: the structured diff of the Review
//! page, printed as a patch or one line per file, and the lines a comment may
//! anchor on.

use anyhow::Result;
use pdo_platform::adapters::cli::daemon::{segment, Daemon};
use serde_json::Value;

/// `GET /runs/<id>/diff/structured`, between `from` and `to` when given
/// (fork → tip by default).
pub(super) fn structured_diff(
    daemon: &Daemon,
    run_id: &str,
    from: Option<String>,
    to: Option<String>,
) -> Result<Value> {
    let query: Vec<String> = [("from", from), ("to", to)]
        .into_iter()
        .filter_map(|(key, value)| value.map(|value| format!("{key}={}", segment(&value))))
        .collect();
    daemon
        .get(&format!(
            "/runs/{}/diff/structured?{}",
            segment(run_id),
            query.join("&")
        ))?
        .ok()?
        .json()
}

/// Whether the Review page lets one comment `line` of `path` on `side` (`new`
/// or `old`), the diff read between the same refs: a line of a hunk, or one
/// of the file's lines its context expanders reveal. The page names a file
/// by its new path (its old one once deleted), on both sides.
pub(super) fn comments_line(
    daemon: &Daemon,
    run_id: &str,
    diff: &Value,
    path: &str,
    side: &str,
    line: i64,
) -> Result<bool> {
    Ok(match anchor(diff, path, side, line) {
        Anchor::Shown => true,
        Anchor::Outside => false,
        Anchor::Expandable { path, git_ref } => {
            let content = daemon
                .get(&format!(
                    "/runs/{}/file?path={}&ref={}",
                    segment(run_id),
                    segment(&path),
                    segment(&git_ref)
                ))?
                .ok()?
                .text();
            usize::try_from(line).is_ok_and(|line| line <= content.lines().count())
        }
    })
}

/// Where a line stands for the Review page.
#[derive(Debug, PartialEq, Eq)]
enum Anchor {
    /// A line of a hunk.
    Shown,
    /// Outside the hunks of a file the page shows with its context: a line
    /// of that side's content, read at `git_ref`, may be revealed.
    Expandable { path: String, git_ref: String },
    /// Not a line the page shows.
    Outside,
}

fn anchor(diff: &Value, path: &str, side: &str, line: i64) -> Anchor {
    let Some(file) = diff["files"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|file| file_path(file) == path)
    else {
        return Anchor::Outside;
    };
    let (path_key, number_key, ref_key, absent) = match side {
        "old" => ("old_path", "old_no", "from_ref", "added"),
        _ => ("new_path", "new_no", "to_ref", "deleted"),
    };
    let hunks = file["hunks"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    if hunks
        .iter()
        .flat_map(|hunk| hunk["lines"].as_array().into_iter().flatten())
        .any(|shown| shown[number_key].as_i64() == Some(line))
    {
        return Anchor::Shown;
    }
    // Context is only expanded in a body of hunks, on a side the file has.
    match (file[path_key].as_str(), diff[ref_key].as_str()) {
        (Some(side_path), Some(git_ref))
            if line >= 1
                && !hunks.is_empty()
                && file["binary"] != true
                && file["status"] != absent =>
        {
            Anchor::Expandable {
                path: side_path.to_string(),
                git_ref: git_ref.to_string(),
            }
        }
        _ => Anchor::Outside,
    }
}

fn file_path(file: &Value) -> &str {
    file["new_path"]
        .as_str()
        .or_else(|| file["old_path"].as_str())
        .unwrap_or("?")
}

/// One line per file of a structured diff: status, additions, deletions, path.
pub(super) fn stat_lines(diff: &Value) -> String {
    diff["files"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|file| {
            format!(
                "{}\t+{}\t-{}\t{}\n",
                file["status"].as_str().unwrap_or("?"),
                file["additions"],
                file["deletions"],
                file_path(file)
            )
        })
        .collect()
}

/// A structured diff read as a patch: each file, each hunk's `@@` line, its
/// lines with their `+`, `-` or ` `.
pub(super) fn patch_text(diff: &Value) -> String {
    let mut out = String::new();
    for file in diff["files"].as_array().into_iter().flatten() {
        out.push_str(&format!("--- {}\n", file_path(file)));
        for hunk in file["hunks"].as_array().into_iter().flatten() {
            out.push_str(&format!(
                "@@ -{},{} +{},{} @@ {}\n",
                hunk["old_start"],
                hunk["old_lines"],
                hunk["new_start"],
                hunk["new_lines"],
                hunk["header"].as_str().unwrap_or("")
            ));
            for line in hunk["lines"].as_array().into_iter().flatten() {
                let mark = match line["kind"].as_str() {
                    Some("add") => '+',
                    Some("del") => '-',
                    _ => ' ',
                };
                out.push(mark);
                out.push_str(line["content"].as_str().unwrap_or(""));
                out.push('\n');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn diff() -> Value {
        json!({ "from_ref": "fork", "to_ref": "worktree", "files": [{
            "old_path": "src/a.rs", "new_path": "src/a.rs", "status": "modified",
            "binary": false, "additions": 1, "deletions": 1,
            "hunks": [{ "old_start": 3, "old_lines": 2, "new_start": 3, "new_lines": 2, "header": "fn a()",
                "lines": [
                    { "kind": "context", "content": "let x = 1;", "old_no": 3, "new_no": 3 },
                    { "kind": "del", "content": "x", "old_no": 4, "new_no": null },
                    { "kind": "add", "content": "y", "old_no": null, "new_no": 4 },
                ] }],
        }, {
            "old_path": null, "new_path": "b.md", "status": "added",
            "binary": false, "additions": 4, "deletions": 0, "hunks": [],
        }, {
            "old_path": null, "new_path": "c.md", "status": "added",
            "binary": false, "additions": 1, "deletions": 0,
            "hunks": [{ "old_start": 0, "old_lines": 0, "new_start": 1, "new_lines": 1, "header": "",
                "lines": [{ "kind": "add", "content": "c", "old_no": null, "new_no": 1 }] }],
        }, {
            "old_path": "old.rs", "new_path": "new.rs", "status": "renamed",
            "binary": false, "additions": 0, "deletions": 1,
            "hunks": [{ "old_start": 7, "old_lines": 1, "new_start": 6, "new_lines": 0, "header": "",
                "lines": [{ "kind": "del", "content": "z", "old_no": 7, "new_no": null }] }],
        }]})
    }

    #[test]
    fn the_stat_is_one_line_per_file() {
        assert_eq!(
            stat_lines(&diff()),
            "modified\t+1\t-1\tsrc/a.rs\nadded\t+4\t-0\tb.md\nadded\t+1\t-0\tc.md\n\
             renamed\t+0\t-1\tnew.rs\n"
        );
    }

    #[test]
    fn a_structured_diff_reads_back_as_a_patch() {
        assert_eq!(
            patch_text(&diff()),
            "--- src/a.rs\n@@ -3,2 +3,2 @@ fn a()\n let x = 1;\n-x\n+y\n--- b.md\n\
             --- c.md\n@@ -0,0 +1,1 @@ \n+c\n--- new.rs\n@@ -7,1 +6,0 @@ \n-z\n"
        );
    }

    #[test]
    fn a_comment_anchors_on_a_hunk_line_or_a_line_its_context_reveals() {
        let diff = diff();
        assert_eq!(anchor(&diff, "src/a.rs", "new", 3), Anchor::Shown);
        assert_eq!(anchor(&diff, "src/a.rs", "new", 4), Anchor::Shown);
        assert_eq!(anchor(&diff, "src/a.rs", "old", 4), Anchor::Shown);
        // Outside the hunks: the side's content says whether the line exists.
        assert_eq!(
            anchor(&diff, "src/a.rs", "new", 99),
            Anchor::Expandable {
                path: "src/a.rs".into(),
                git_ref: "worktree".into()
            }
        );
        assert_eq!(
            anchor(&diff, "src/a.rs", "old", 1),
            Anchor::Expandable {
                path: "src/a.rs".into(),
                git_ref: "fork".into()
            }
        );
        assert_eq!(anchor(&diff, "src/a.rs", "new", 0), Anchor::Outside);
        // A file outside the diff, a file without hunks, an added file's old side.
        assert_eq!(anchor(&diff, "nope.md", "new", 1), Anchor::Outside);
        assert_eq!(anchor(&diff, "b.md", "new", 1), Anchor::Outside);
        assert_eq!(anchor(&diff, "c.md", "old", 1), Anchor::Outside);
        assert_eq!(anchor(&diff, "c.md", "new", 1), Anchor::Shown);
    }

    #[test]
    fn a_renamed_file_is_named_by_its_new_path_on_both_sides() {
        let diff = diff();
        assert_eq!(anchor(&diff, "new.rs", "old", 7), Anchor::Shown);
        assert_eq!(
            anchor(&diff, "new.rs", "old", 1),
            Anchor::Expandable {
                path: "old.rs".into(),
                git_ref: "fork".into()
            }
        );
        assert_eq!(anchor(&diff, "old.rs", "old", 7), Anchor::Outside);
    }
}
