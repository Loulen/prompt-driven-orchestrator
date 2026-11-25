//! The line diff `pdo service refresh --dry-run` shows between the unit in
//! place and the one the binary would write. Pure: a longest common
//! subsequence over the lines, which is plenty for a file of a few dozen.

/// `-` for a line only in `old`, `+` for a line only in `new`, two spaces for
/// a line both keep; one line of output per line of input.
pub fn unit_diff(old: &str, new: &str) -> String {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    // lcs[i][j]: length of the longest common subsequence of a[i..] and b[j..].
    let mut lcs = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            lcs[i][j] = if a[i] == b[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let mut out = String::new();
    let (mut i, mut j) = (0, 0);
    while i < a.len() || j < b.len() {
        if i < a.len() && j < b.len() && a[i] == b[j] {
            out.push_str(&format!("  {}\n", a[i]));
            i += 1;
            j += 1;
        } else if i < a.len() && (j == b.len() || lcs[i + 1][j] >= lcs[i][j + 1]) {
            out.push_str(&format!("- {}\n", a[i]));
            i += 1;
        } else {
            out.push_str(&format!("+ {}\n", b[j]));
            j += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_changed_line_is_removed_then_added_the_rest_kept() {
        let old = "[Service]\nExecStart=/c/Cellar/pdo/1.0/bin/pdo daemon\nKillMode=process\n";
        let new = "[Service]\nExecStart=/c/bin/pdo daemon\nKillMode=process\n";
        assert_eq!(
            unit_diff(old, new),
            "  [Service]\n\
             - ExecStart=/c/Cellar/pdo/1.0/bin/pdo daemon\n\
             + ExecStart=/c/bin/pdo daemon\n\
             \x20 KillMode=process\n"
        );
    }

    #[test]
    fn added_and_dropped_lines_at_the_edges() {
        assert_eq!(unit_diff("a\nb\n", "b\nc\n"), "- a\n  b\n+ c\n");
        assert_eq!(unit_diff("", "x\n"), "+ x\n");
        assert_eq!(unit_diff("x\n", "x\n"), "  x\n");
    }
}
