//! No dead relative link in the showcase README, its translation and the docs it
//! points to (#855).
//!
//! The README is a showcase whose every row sends the reader to an anchor of
//! `docs/features.md`, and whose media, language toggle and reference pages are
//! all relative paths. GitHub renders a dead one as a 404 or as a jump to the top
//! of the page, silently. This test resolves every relative `](…)`, `href`, `src`
//! and `srcset` against the file system, and every `#anchor` against the GitHub
//! slugs of the target's headings.

use std::collections::{BTreeSet, HashMap};
use std::path::{Component, Path, PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The checked documents, relative to the repository root.
fn documents() -> Vec<String> {
    let mut docs: Vec<String> = [
        "README.md",
        "docs/readme/README.fr.md",
        "docs/features.md",
        "CONTRIBUTING.md",
        "scripts/readme-media/README.md",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let mut reference: Vec<String> = std::fs::read_dir(root().join("docs/reference"))
        .expect("docs/reference exists")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".md"))
        .map(|name| format!("docs/reference/{name}"))
        .collect();
    reference.sort();
    docs.extend(reference);
    docs
}

/// Lines outside fenced code blocks.
fn prose_lines(document: &str) -> Vec<&str> {
    let mut in_fence = false;
    document
        .lines()
        .filter(|line| {
            if line.trim_start().starts_with("```") {
                in_fence = !in_fence;
                return false;
            }
            !in_fence
        })
        .collect()
}

/// Every link target written in `document`: markdown `](target)` and the
/// `href` / `src` / `srcset` HTML attributes.
fn link_targets(document: &str) -> Vec<String> {
    let mut targets = Vec::new();
    for line in prose_lines(document) {
        let mut rest = line;
        while let Some(at) = rest.find("](") {
            let after = &rest[at + 2..];
            match after.find(')') {
                Some(end) => {
                    targets.push(after[..end].trim().to_string());
                    rest = &after[end..];
                }
                None => break,
            }
        }
        for attribute in ["href=\"", "src=\"", "srcset=\""] {
            let mut rest = line;
            while let Some(at) = rest.find(attribute) {
                let after = &rest[at + attribute.len()..];
                let Some(end) = after.find('"') else { break };
                // A srcset may list `url 2x, url2 3x`; each URL is a target.
                for candidate in after[..end].split(',') {
                    if let Some(url) = candidate.split_whitespace().next() {
                        targets.push(url.to_string());
                    }
                }
                rest = &after[end..];
            }
        }
    }
    targets
}

fn is_external(target: &str) -> bool {
    target.starts_with("http://") || target.starts_with("https://") || target.starts_with("mailto:")
}

/// GitHub's heading anchor: lowercase, punctuation dropped, spaces to `-`.
fn slug(heading: &str) -> String {
    let text = heading
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">");
    text.trim()
        .to_lowercase()
        .chars()
        .filter_map(|c| match c {
            ' ' => Some('-'),
            '-' | '_' => Some(c),
            c if c.is_alphanumeric() => Some(c),
            _ => None,
        })
        .collect()
}

/// The anchors GitHub generates for `document`'s headings, duplicates suffixed.
fn anchors(document: &str) -> BTreeSet<String> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut out = BTreeSet::new();
    for line in prose_lines(document) {
        let trimmed = line.trim_start();
        let hashes = trimmed.chars().take_while(|c| *c == '#').count();
        if hashes == 0 || hashes > 6 || !trimmed[hashes..].starts_with(' ') {
            continue;
        }
        let base = slug(&trimmed[hashes..].replace('`', "").replace("**", ""));
        let n = seen.entry(base.clone()).or_insert(0);
        out.insert(if *n == 0 {
            base.clone()
        } else {
            format!("{base}-{n}")
        });
        *n += 1;
    }
    out
}

/// `base/relative`, with `..` folded, so two spellings of one file compare equal.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

/// Every dead relative link of `document`, as `target: why`.
fn dead_links(document_path: &str) -> Vec<String> {
    let absolute = root().join(document_path);
    let document = std::fs::read_to_string(&absolute)
        .unwrap_or_else(|e| panic!("failed to read {document_path}: {e}"));
    let dir = absolute.parent().expect("a document has a directory");
    let mut dead = Vec::new();
    for target in link_targets(&document) {
        if target.is_empty() || is_external(&target) {
            continue;
        }
        let (path, anchor) = match target.split_once('#') {
            Some((path, anchor)) => (path, Some(anchor)),
            None => (target.as_str(), None),
        };
        let resolved = if path.is_empty() {
            absolute.clone()
        } else {
            normalize(&dir.join(path))
        };
        if !resolved.exists() {
            dead.push(format!("{target}: no such file"));
            continue;
        }
        if let Some(anchor) = anchor {
            if resolved.extension().and_then(|e| e.to_str()) != Some("md") {
                dead.push(format!("{target}: an anchor into a non-markdown file"));
                continue;
            }
            let content = std::fs::read_to_string(&resolved).expect("target is readable");
            if !anchors(&content).contains(anchor) {
                dead.push(format!("{target}: no heading with anchor #{anchor}"));
            }
        }
    }
    dead
}

#[test]
fn no_relative_link_of_the_readme_or_the_docs_is_dead() {
    let mut failures = Vec::new();
    for document in documents() {
        for why in dead_links(&document) {
            failures.push(format!("{document} → {why}"));
        }
    }
    assert!(
        failures.is_empty(),
        "dead relative links:\n  {}",
        failures.join("\n  ")
    );
}

#[test]
fn the_language_line_toggles_between_english_and_french_both_ways() {
    let english = std::fs::read_to_string(root().join("README.md")).unwrap();
    let french = std::fs::read_to_string(root().join("docs/readme/README.fr.md")).unwrap();
    assert!(
        english.contains("<a href=\"docs/readme/README.fr.md\">Français</a>"),
        "README.md does not link to its French translation"
    );
    assert!(
        french.contains("<a href=\"../../README.md\">English</a>"),
        "README.fr.md does not link back to the English README"
    );
}

#[test]
fn every_feature_row_of_both_readmes_points_to_its_features_section() {
    let rows = [
        "visual-pipelines",
        "conditional-routing--loops",
        "typed-outputs",
        "diff-review",
        "triggers",
        "run-stats-by-model",
        "recursive-orchestration",
        "agent-profiles",
        "skill-bank",
    ];
    let features = std::fs::read_to_string(root().join("docs/features.md")).unwrap();
    let sections = anchors(&features);
    for (readme, prefix) in [
        ("README.md", "docs/features.md"),
        ("docs/readme/README.fr.md", "../../docs/features.md"),
    ] {
        let document = std::fs::read_to_string(root().join(readme)).unwrap();
        // The rows appear in this order, each with its "Docs →" link.
        let mut from = 0;
        for row in rows {
            let link = format!("[Docs →]({prefix}#{row})");
            let at = document[from..]
                .find(&link)
                .unwrap_or_else(|| panic!("{readme}: row `{row}` missing or out of order"));
            from += at + link.len();
            assert!(sections.contains(row), "docs/features.md has no #{row}");
        }
    }
}

#[test]
fn the_slug_follows_github() {
    assert_eq!(
        slug("Conditional routing &amp; loops"),
        "conditional-routing--loops"
    );
    assert_eq!(slug("Service & in-app update"), "service--in-app-update");
    assert_eq!(slug("Install — macOS, Linux"), "install--macos-linux");
    assert_eq!(slug("Outputs typés"), "outputs-typés");
}
