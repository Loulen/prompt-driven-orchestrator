//! The line diff of a tool call that replaces text in a file: what its folded
//! row counts (`+18 −3`) and what its detail shows, the same for every harness.

use serde::Serialize;
use serde_json::Value;
use ts_rs::TS;

/// The `(old, new)` input keys of a replacement, harness by harness: claude's
/// `Edit` / `MultiEdit`, copilot's `str_replace`, pi's `edit`.
const EDIT_KEYS: [(&str, &str); 3] = [
    ("old_string", "new_string"),
    ("old_str", "new_str"),
    ("oldText", "newText"),
];

/// Past this many line pairs, a replacement is shown as all removed then all
/// added: the diff stays linear on a huge edit.
const DIFF_CELLS_CAP: usize = 250_000;

/// What a diff line does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, rename = "TranscriptDiffOp")]
pub enum DiffOp {
    Keep,
    Add,
    Remove,
}

/// One line of a replacement's diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, rename = "TranscriptDiffLine")]
pub struct DiffLine {
    pub op: DiffOp,
    pub text: String,
}

/// The lines a replacement adds and removes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, TS)]
#[ts(export, rename = "TranscriptDiffStat")]
pub struct DiffStat {
    pub added: u32,
    pub removed: u32,
}

/// The replacements a tool call's input makes, in order: one for an edit,
/// several for an `edits` array. Empty when the call replaces nothing.
fn replacements(input: &Value) -> Vec<(&str, &str)> {
    if let Some(one) = pair(input) {
        return vec![one];
    }
    input
        .get("edits")
        .and_then(Value::as_array)
        .map(|edits| edits.iter().filter_map(pair).collect())
        .unwrap_or_default()
}

/// The `(old, new)` of one replacement object, under any harness's keys.
fn pair(v: &Value) -> Option<(&str, &str)> {
    EDIT_KEYS
        .iter()
        .find_map(|(old, new)| Some((v.get(*old)?.as_str()?, v.get(*new)?.as_str()?)))
}

/// The line diff of a tool call's replacements, or nothing when it makes none.
pub fn diff_of(input: &Value) -> Vec<DiffLine> {
    replacements(input)
        .into_iter()
        .flat_map(|(old, new)| line_diff(old, new))
        .collect()
}

/// The lines a tool call's replacements add and remove, `None` when it makes
/// none.
pub fn diff_stat(input: &Value) -> Option<DiffStat> {
    let lines = diff_of(input);
    if lines.is_empty() {
        return None;
    }
    let count = |op| lines.iter().filter(|l| l.op == op).count() as u32;
    Some(DiffStat {
        added: count(DiffOp::Add),
        removed: count(DiffOp::Remove),
    })
}

/// The line diff of `old` into `new`: common head and tail kept, the middle
/// aligned on its longest common subsequence.
fn line_diff(old: &str, new: &str) -> Vec<DiffLine> {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    let head = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let tail = a[head..]
        .iter()
        .rev()
        .zip(b[head..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let (mid_a, mid_b) = (&a[head..a.len() - tail], &b[head..b.len() - tail]);
    let line = |op, text: &str| DiffLine {
        op,
        text: text.to_string(),
    };
    let mut out: Vec<DiffLine> = a[..head].iter().map(|t| line(DiffOp::Keep, t)).collect();
    if mid_a.len() * mid_b.len() > DIFF_CELLS_CAP {
        out.extend(mid_a.iter().map(|t| line(DiffOp::Remove, t)));
        out.extend(mid_b.iter().map(|t| line(DiffOp::Add, t)));
    } else {
        out.extend(
            lcs_diff(mid_a, mid_b)
                .into_iter()
                .map(|(op, t)| line(op, t)),
        );
    }
    out.extend(a[a.len() - tail..].iter().map(|t| line(DiffOp::Keep, t)));
    out
}

/// The classic LCS table walk: removals before additions at each change.
fn lcs_diff<'a>(a: &[&'a str], b: &[&'a str]) -> Vec<(DiffOp, &'a str)> {
    let (n, m) = (a.len(), b.len());
    // lcs[i][j]: the longest common subsequence of a[i..] and b[j..].
    let mut lcs = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if a[i] == b[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let (mut i, mut j, mut out) = (0, 0, Vec::with_capacity(n + m));
    while i < n || j < m {
        if i < n && j < m && a[i] == b[j] {
            out.push((DiffOp::Keep, a[i]));
            (i, j) = (i + 1, j + 1);
        } else if i < n && (j == m || lcs[i + 1][j] >= lcs[i][j + 1]) {
            out.push((DiffOp::Remove, a[i]));
            i += 1;
        } else {
            out.push((DiffOp::Add, b[j]));
            j += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ops(lines: &[DiffLine]) -> Vec<String> {
        lines
            .iter()
            .map(|l| {
                let mark = match l.op {
                    DiffOp::Keep => ' ',
                    DiffOp::Add => '+',
                    DiffOp::Remove => '-',
                };
                format!("{mark}{}", l.text)
            })
            .collect()
    }

    #[test]
    fn an_edit_diffs_its_lines_around_what_stays() {
        let input = json!({
            "file_path": "/r/a.ts",
            "old_string": "a\nb\nreturn fetch(q)\nz",
            "new_string": "a\nb\nconst d = debounce(q)\nreturn fetch(d)\nz",
        });
        assert_eq!(
            ops(&diff_of(&input)),
            [
                " a",
                " b",
                "-return fetch(q)",
                "+const d = debounce(q)",
                "+return fetch(d)",
                " z"
            ]
        );
        assert_eq!(
            diff_stat(&input),
            Some(DiffStat {
                added: 2,
                removed: 1
            })
        );
    }

    #[test]
    fn every_harness_key_pair_and_multi_edits_are_read() {
        for (old, new) in EDIT_KEYS {
            let input = json!({ old: "x", new: "y" });
            assert_eq!(ops(&diff_of(&input)), ["-x", "+y"], "{old}/{new}");
        }
        let multi = json!({"edits": [
            {"old_string": "a", "new_string": "b"},
            {"old_string": "c", "new_string": "c\nd"},
        ]});
        assert_eq!(ops(&diff_of(&multi)), ["-a", "+b", " c", "+d"]);
    }

    #[test]
    fn a_call_that_replaces_nothing_has_no_diff() {
        let read = json!({"file_path": "/r/a.ts"});
        assert!(diff_of(&read).is_empty());
        assert_eq!(diff_stat(&read), None);
    }

    #[test]
    fn a_huge_replacement_stays_linear() {
        let old = (0..1000)
            .map(|i| format!("o{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let new = (0..1000)
            .map(|i| format!("n{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let stat = diff_stat(&json!({"old_string": old, "new_string": new})).unwrap();
        assert_eq!(
            stat,
            DiffStat {
                added: 1000,
                removed: 1000
            }
        );
    }
}
