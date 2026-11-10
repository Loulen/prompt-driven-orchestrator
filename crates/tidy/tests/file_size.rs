//! At most 400 lines per production file, Rust and TypeScript (ADR-0079, D12).
//!
//! Tests are not counted: `crates/*/tests/`, a Rust file's `#[cfg(test)]`
//! module, `*.test.ts(x)` and `frontend/src/test/`. Neither is the TypeScript
//! generated from the Rust DTOs (`frontend/src/shared/generated/`).
//!
//! The files that were already too big when the rule landed are listed, with
//! their size, in `crates/tidy/file_size_baseline.txt`. That list may only shrink: a file
//! outside it must stay within the limit, a file in it must not grow, and an
//! entry whose file is gone or back within the limit must be removed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const MAX_LINES: usize = 400;
const BASELINE: &str = "crates/tidy/file_size_baseline.txt";

/// Why the size check failed for one file.
#[derive(Debug, PartialEq, Eq)]
pub enum Failure {
    /// A file outside the baseline is over the limit.
    TooBig { file: String, lines: usize },
    /// A baseline file grew past its recorded size.
    Grew {
        file: String,
        lines: usize,
        baseline: usize,
    },
    /// A baseline entry whose file is gone or back within the limit.
    Stale { file: String },
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Failure::TooBig { file, lines } => write!(
                f,
                "{file}: {lines} lines, over the {MAX_LINES}-line limit. Split it by responsibility."
            ),
            Failure::Grew {
                file,
                lines,
                baseline,
            } => write!(
                f,
                "{file}: {lines} lines, grown past its baseline of {baseline}. A file over the \
                 limit may only shrink: move code out instead of adding to it."
            ),
            Failure::Stale { file } => write!(
                f,
                "{file}: gone or within the limit now. Remove its line from {BASELINE}."
            ),
        }
    }
}

/// Compare the production files' sizes with the baseline.
pub fn check(sizes: &BTreeMap<String, usize>, baseline: &BTreeMap<String, usize>) -> Vec<Failure> {
    let mut failures = Vec::new();
    for (file, &lines) in sizes {
        match baseline.get(file) {
            None if lines > MAX_LINES => failures.push(Failure::TooBig {
                file: file.clone(),
                lines,
            }),
            Some(&base) if lines > base => failures.push(Failure::Grew {
                file: file.clone(),
                lines,
                baseline: base,
            }),
            _ => {}
        }
    }
    for file in baseline.keys() {
        if sizes.get(file).is_none_or(|&lines| lines <= MAX_LINES) {
            failures.push(Failure::Stale { file: file.clone() });
        }
    }
    failures
}

/// The lines of a Rust file that are not its top-level `#[cfg(test)]` items.
/// rustfmt closes a top-level block with a `}` in column 0, which is what ends
/// a test module here.
pub fn rust_production_lines(source: &str) -> usize {
    let lines: Vec<&str> = source.lines().collect();
    let mut count = 0;
    let mut i = 0;
    while i < lines.len() {
        if lines[i].trim_end() == "#[cfg(test)]" {
            let mut j = i + 1;
            while j < lines.len() && lines[j].starts_with("#[") {
                j += 1;
            }
            if j < lines.len() && lines[j].trim_end().ends_with('{') {
                while j < lines.len() && lines[j] != "}" {
                    j += 1;
                }
            }
            i = j + 1;
            continue;
        }
        count += 1;
        i += 1;
    }
    count
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out);
        } else {
            out.push(path);
        }
    }
}

/// Every production file of the repository and its size, keyed by its path
/// relative to the repository root.
fn production_sizes(root: &Path) -> BTreeMap<String, usize> {
    let mut files = Vec::new();
    for krate in std::fs::read_dir(root.join("crates")).unwrap().flatten() {
        walk(&krate.path().join("src"), &mut files);
    }
    walk(&root.join("frontend/src"), &mut files);

    let mut sizes = BTreeMap::new();
    for file in files {
        let rel = file
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let name = file.file_name().unwrap().to_string_lossy();
        let lines = if name.ends_with(".rs") {
            rust_production_lines(&std::fs::read_to_string(&file).unwrap())
        } else if (name.ends_with(".ts") || name.ends_with(".tsx"))
            && !name.contains(".test.")
            && !rel.starts_with("frontend/src/test/")
            && !rel.starts_with("frontend/src/shared/generated/")
        {
            std::fs::read_to_string(&file).unwrap().lines().count()
        } else {
            continue;
        };
        sizes.insert(rel, lines);
    }
    sizes
}

fn read_baseline(root: &Path) -> BTreeMap<String, usize> {
    let path = root.join(BASELINE);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            let (file, lines) = l
                .rsplit_once(' ')
                .unwrap_or_else(|| panic!("baseline line `{l}`: expected `<path> <lines>`"));
            (file.to_string(), lines.parse().unwrap())
        })
        .collect()
}

#[test]
fn production_files_stay_within_400_lines() {
    let root = tidy::workspace_root();
    let sizes = production_sizes(&root);
    assert!(
        sizes.contains_key("crates/pdo/src/lib.rs") && sizes.contains_key("frontend/src/main.tsx"),
        "the walk found no production file: {} entries",
        sizes.len()
    );
    let failures: Vec<String> = check(&sizes, &read_baseline(&root))
        .iter()
        .map(|f| f.to_string())
        .collect();
    assert!(
        failures.is_empty(),
        "file size rule (docs/agents/module-layout.md):\n{}",
        failures.join("\n")
    );
}

fn map(entries: &[(&str, usize)]) -> BTreeMap<String, usize> {
    entries.iter().map(|(f, n)| (f.to_string(), *n)).collect()
}

#[test]
fn a_new_file_over_the_limit_fails() {
    let failures = check(&map(&[("a.rs", 401), ("b.ts", 400)]), &map(&[]));
    assert_eq!(
        failures,
        vec![Failure::TooBig {
            file: "a.rs".into(),
            lines: 401
        }]
    );
}

#[test]
fn a_baseline_file_that_grows_fails_and_one_that_shrinks_passes() {
    let baseline = map(&[("big.rs", 900), ("huge.tsx", 2000)]);
    let failures = check(&map(&[("big.rs", 901), ("huge.tsx", 1500)]), &baseline);
    assert_eq!(
        failures,
        vec![Failure::Grew {
            file: "big.rs".into(),
            lines: 901,
            baseline: 900
        }]
    );
}

#[test]
fn a_baseline_entry_back_within_the_limit_or_gone_must_be_removed() {
    let baseline = map(&[("fixed.rs", 900), ("deleted.rs", 700)]);
    let failures = check(&map(&[("fixed.rs", 380)]), &baseline);
    assert_eq!(
        failures,
        vec![
            Failure::Stale {
                file: "deleted.rs".into()
            },
            Failure::Stale {
                file: "fixed.rs".into()
            },
        ]
    );
}

#[test]
fn a_rust_test_module_is_not_counted() {
    let source = "use x;\n\nfn a() {}\n\n#[cfg(test)]\nmod tests {\n    use super::*;\n\n    #[test]\n    fn t() {}\n}\n";
    assert_eq!(rust_production_lines(source), 4);
    let with_attr = "fn a() {}\n#[cfg(test)]\n#[allow(dead_code)]\nmod helpers {\n    fn h() {}\n}\nfn b() {}\n";
    assert_eq!(rust_production_lines(with_attr), 2);
    let one_liner = "#[cfg(test)]\nmod fixtures;\nfn a() {}\n";
    assert_eq!(rust_production_lines(one_liner), 1);
}
