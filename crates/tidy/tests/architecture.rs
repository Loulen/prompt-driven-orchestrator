//! The hexagonal rule inside every bounded-context crate (ADR-0079): a
//! `domain/` module imports nothing technical — no sqlx, no axum, no
//! `tokio::process`, no `adapters::`. The compiler guards the frontiers between
//! crates; this test guards the one inside each crate.
//!
//! It reads the source, not the compiled crate: a `domain/` file is scanned
//! line by line, comments and string literals stripped, for the forbidden paths.
//! The fixtures under `tests/fixtures/architecture/` prove it catches a faulty
//! domain and lets a clean one through.

use std::path::{Path, PathBuf};

/// A path a domain module must never name, and why.
const FORBIDDEN: &[(&str, &str)] = &[
    ("sqlx", "storage belongs to an adapter"),
    ("axum", "HTTP belongs to an adapter"),
    ("tokio::process", "running processes belongs to an adapter"),
    ("adapters", "the domain never depends on the adapters"),
];

/// One forbidden import found in a domain file.
#[derive(Debug, PartialEq, Eq)]
pub struct Violation {
    pub file: PathBuf,
    pub line: usize,
    pub path: &'static str,
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let why = FORBIDDEN
            .iter()
            .find(|(p, _)| *p == self.path)
            .map(|(_, why)| *why)
            .unwrap_or("");
        write!(
            f,
            "{}:{} names `{}` ({why})",
            self.file.display(),
            self.line,
            self.path
        )
    }
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// `line` without its `//` comment and its string literals, so a word in a
/// comment or a message is never mistaken for an import.
fn code_of(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    let mut in_string = false;
    while let Some(c) = chars.next() {
        if in_string {
            match c {
                '\\' => {
                    chars.next();
                }
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '/' if chars.peek() == Some(&'/') => break,
            _ => out.push(c),
        }
    }
    out
}

/// Whether `code` names `path` as a path segment: `sqlx::…`, `use sqlx;`,
/// `crate::adapters::…`, but not `my_sqlx` or `adapters_count`.
fn names(code: &str, path: &str) -> bool {
    let bytes = code.as_bytes();
    let mut from = 0;
    while let Some(i) = code[from..].find(path) {
        let start = from + i;
        let end = start + path.len();
        let before_ok = start == 0 || !is_ident(bytes[start - 1]);
        let rest = &code[end..];
        let after_ok = rest.starts_with("::")
            || rest.starts_with(';')
            || rest.starts_with(',')
            || rest.starts_with('}')
            || rest.trim_start().is_empty()
            || rest.starts_with(" as ");
        if before_ok && after_ok {
            return true;
        }
        from = end;
    }
    false
}

fn is_ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Every forbidden path named under `crate_dir/src/domain`.
pub fn domain_violations(crate_dir: &Path) -> Vec<Violation> {
    let mut files = Vec::new();
    rust_files(&crate_dir.join("src/domain"), &mut files);
    files.sort();
    let mut found = Vec::new();
    for file in files {
        let source = std::fs::read_to_string(&file).unwrap();
        let mut in_block_comment = false;
        for (index, line) in source.lines().enumerate() {
            let trimmed = line.trim_start();
            if in_block_comment {
                if trimmed.contains("*/") {
                    in_block_comment = false;
                }
                continue;
            }
            if trimmed.starts_with("/*") {
                in_block_comment = !trimmed.contains("*/");
                continue;
            }
            let code = code_of(line);
            for (path, _) in FORBIDDEN {
                if names(&code, path) {
                    found.push(Violation {
                        file: file.clone(),
                        line: index + 1,
                        path,
                    });
                }
            }
        }
    }
    found
}

/// The bounded-context crates: every `crates/*` with a `src/domain/`.
fn context_crates() -> Vec<PathBuf> {
    let mut crates: Vec<PathBuf> = std::fs::read_dir(tidy::workspace_root().join("crates"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("src/domain").is_dir())
        .collect();
    crates.sort();
    crates
}

/// The thirteen bounded contexts of the context map (ADR-0079).
const CONTEXTS: [&str; 13] = [
    "pdo-harness",
    "pdo-pipeline",
    "pdo-platform",
    "pdo-review",
    "pdo-run",
    "pdo-sandbox",
    "pdo-session",
    "pdo-settings",
    "pdo-skill",
    "pdo-stats",
    "pdo-telemetry",
    "pdo-trigger",
    "pdo-workspace",
];

#[test]
fn the_architecture_test_covers_the_thirteen_context_crates() {
    let covered: Vec<String> = context_crates()
        .iter()
        .map(|c| c.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        covered, CONTEXTS,
        "every bounded context is a crate with a src/domain/, and only they are"
    );
}

#[test]
fn no_context_domain_imports_anything_technical() {
    let violations: Vec<String> = context_crates()
        .iter()
        .flat_map(|c| domain_violations(c))
        .map(|v| v.to_string())
        .collect();
    assert!(
        violations.is_empty(),
        "a domain/ module must stay pure (ADR-0079, docs/agents/module-layout.md). \
         Move the technical code to adapters/ behind a port:\n{}",
        violations.join("\n")
    );
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/architecture")
        .join(name)
}

#[test]
fn a_faulty_domain_fails_the_architecture_test() {
    let found: Vec<(usize, &str)> = domain_violations(&fixture("faulty"))
        .iter()
        .map(|v| (v.line, v.path))
        .collect();
    assert_eq!(
        found,
        vec![
            (4, "sqlx"),
            (6, "adapters"),
            (9, "tokio::process"),
            (12, "axum"),
            (13, "axum"),
        ]
    );
}

#[test]
fn a_clean_domain_passes_the_architecture_test() {
    assert_eq!(domain_violations(&fixture("clean")), Vec::new());
}

#[test]
fn only_whole_path_segments_count() {
    assert!(names("use sqlx::SqlitePool;", "sqlx"));
    assert!(names("use crate::adapters::sqlite;", "adapters"));
    assert!(names("use super::{adapters, rules};", "adapters"));
    assert!(names("extern crate axum;", "axum"));
    assert!(!names("let my_sqlx = 1;", "sqlx"));
    assert!(!names("let adapters_count = 2;", "adapters"));
    assert!(!names(&code_of("let s = \"axum::Router\";"), "axum"));
    assert!(!names(&code_of("do(); // sqlx::query"), "sqlx"));
}
