//! The reading guide of a mounted page: entries (title, text, link) the agent
//! that mounts the page writes, so the reader knows where to look, what to
//! expect, how it was before. PDO shows the page in a frame with the guide in a
//! side panel.
//!
//! The rules here are pure: the guide file's format and bounds, and what a link
//! may point at. A link leads either into PDO (a Run, a node, an artifact, a
//! file of a Run's Review, an extension's page) or into the page (an anchor, a
//! path beneath the mount). Whether its target exists is the use case's check.

use std::ffi::OsString;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::page_mount::requested_components;

/// The most entries a guide holds.
pub const MAX_ENTRIES: usize = 50;
/// The longest title, in characters.
pub const MAX_TITLE: usize = 200;
/// The longest text, in characters.
pub const MAX_TEXT: usize = 2000;
/// The longest link, in characters.
pub const MAX_LINK: usize = 2000;

/// One entry of a reading guide.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct GuideEntry {
    pub title: String,
    /// What to look at there; may be empty.
    #[serde(default)]
    pub text: String,
    /// Where the entry leads: see [`GuideLink`].
    pub link: String,
}

/// Where a guide link leads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuideLink {
    /// `#/runs/<run>`
    Run { run_id: String },
    /// `#/runs/<run>/nodes/<node>`
    Node { run_id: String, node_id: String },
    /// `#/runs/<run>/nodes/<node>/artifacts/<port>`
    Artifact {
        run_id: String,
        node_id: String,
        port: String,
    },
    /// `/runs/<run>/review`, optionally `?file=<path>`.
    Review { run_id: String },
    /// `#/ext/<extension>/…`: an extension's page.
    Extension,
    /// `#<anchor>` in the page.
    Anchor,
    /// A path beneath the mount (`a/b.html`, `a/b.html#x`), as components.
    Path(Vec<OsString>),
}

const RUN_HASH: &str = "#/runs/";
const EXT_HASH: &str = "#/ext/";
const RUNS_PATH: &str = "/runs/";

/// What `link` points at, or why it can point at nothing.
pub fn classify(link: &str) -> Result<GuideLink, String> {
    let invalid = || {
        Err(format!(
            "link `{link}` leads nowhere: use a PDO link (#/runs/<run>[/nodes/<node>[/artifacts/<port>]], \
             /runs/<run>/review[?file=<path>]) or a place in the page (#anchor, a relative path)"
        ))
    };
    if link.trim() != link || link.is_empty() || link.chars().any(char::is_control) {
        return invalid();
    }
    if let Some(rest) = link.strip_prefix(RUN_HASH) {
        let segments: Vec<&str> = rest.trim_end_matches('/').split('/').collect();
        if segments.iter().any(|s| s.is_empty()) {
            return invalid();
        }
        let run_id = segments[0].to_string();
        return match segments.as_slice() {
            [_] => Ok(GuideLink::Run { run_id }),
            [_, "nodes", node] => Ok(GuideLink::Node {
                run_id,
                node_id: node.to_string(),
            }),
            [_, "nodes", node, "artifacts", port] => Ok(GuideLink::Artifact {
                run_id,
                node_id: node.to_string(),
                port: port.to_string(),
            }),
            _ => invalid(),
        };
    }
    if let Some(rest) = link.strip_prefix(EXT_HASH) {
        return if rest.is_empty() {
            invalid()
        } else {
            Ok(GuideLink::Extension)
        };
    }
    if let Some(rest) = link.strip_prefix(RUNS_PATH) {
        let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
        let query_ok = query.is_empty() || query.split('&').all(|p| p.starts_with("file="));
        return match path.trim_end_matches('/').split_once('/') {
            Some((run, "review")) if !run.is_empty() && query_ok && !link.contains('#') => {
                Ok(GuideLink::Review {
                    run_id: run.to_string(),
                })
            }
            _ => invalid(),
        };
    }
    if let Some(anchor) = link.strip_prefix('#') {
        return if anchor.is_empty() || anchor.starts_with('/') {
            invalid()
        } else {
            Ok(GuideLink::Anchor)
        };
    }
    // A path in the page: no scheme, never absolute, never leaving the mount.
    let path = link.split(['#', '?']).next().unwrap_or_default();
    let scheme = path.split('/').next().unwrap_or_default().contains(':');
    if path.is_empty() || path.starts_with('/') || path.starts_with('\\') || scheme {
        return invalid();
    }
    match requested_components(path) {
        Some(components) if !components.is_empty() => Ok(GuideLink::Path(components)),
        _ => invalid(),
    }
}

/// The entries of a guide file (YAML, or JSON which YAML reads): a list of
/// `{title, text, link}`, within the bounds above, every link classified.
pub fn parse(raw: &str) -> Result<Vec<(GuideEntry, GuideLink)>, String> {
    let entries: Vec<GuideEntry> = serde_yaml::from_str(raw).map_err(|e| {
        format!("guide is not a list of entries {{title, text, link}}: {e}")
    })?;
    if entries.is_empty() {
        return Err("guide has no entry".to_string());
    }
    if entries.len() > MAX_ENTRIES {
        return Err(format!(
            "guide has {} entries, at most {MAX_ENTRIES}",
            entries.len()
        ));
    }
    entries
        .into_iter()
        .enumerate()
        .map(|(index, entry)| {
            let at = |why: String| format!("guide entry {}: {why}", index + 1);
            let title = entry.title.trim();
            if title.is_empty() {
                return Err(at("title is empty".to_string()));
            }
            if title.chars().count() > MAX_TITLE {
                return Err(at(format!("title is longer than {MAX_TITLE} characters")));
            }
            if entry.text.chars().count() > MAX_TEXT {
                return Err(at(format!("text is longer than {MAX_TEXT} characters")));
            }
            if entry.link.chars().count() > MAX_LINK {
                return Err(at(format!("link is longer than {MAX_LINK} characters")));
            }
            let link = classify(&entry.link).map_err(at)?;
            Ok((
                GuideEntry {
                    title: title.to_string(),
                    text: entry.text.trim().to_string(),
                    link: entry.link,
                },
                link,
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pdo_links_name_a_run_a_node_an_artifact_or_a_review() {
        assert_eq!(
            classify("#/runs/r1").unwrap(),
            GuideLink::Run {
                run_id: "r1".into()
            }
        );
        assert_eq!(
            classify("#/runs/r1/nodes/design/").unwrap(),
            GuideLink::Node {
                run_id: "r1".into(),
                node_id: "design".into()
            }
        );
        assert_eq!(
            classify("#/runs/r1/nodes/design/artifacts/out").unwrap(),
            GuideLink::Artifact {
                run_id: "r1".into(),
                node_id: "design".into(),
                port: "out".into()
            }
        );
        assert_eq!(
            classify("/runs/r1/review?file=src%2Fa.ts").unwrap(),
            GuideLink::Review {
                run_id: "r1".into()
            }
        );
        assert_eq!(classify("/runs/r1/review").unwrap(), GuideLink::Review { run_id: "r1".into() });
        assert_eq!(classify("#/ext/demo/topbar/end/x").unwrap(), GuideLink::Extension);
    }

    #[test]
    fn page_links_are_an_anchor_or_a_path_inside_the_mount() {
        assert_eq!(classify("#88-snap:A").unwrap(), GuideLink::Anchor);
        assert_eq!(
            classify("sub/b.html#x").unwrap(),
            GuideLink::Path(vec!["sub".into(), "b.html".into()])
        );
        assert_eq!(
            classify("./index.html?v=1").unwrap(),
            GuideLink::Path(vec!["index.html".into()])
        );
    }

    #[test]
    fn every_other_link_is_refused() {
        for link in [
            "",
            " #a",
            "#",
            "#/",
            "#/runs/",
            "#/runs/r1/review",
            "#/runs/r1/nodes",
            "#/runs/r1//x",
            "#/ext/",
            "/runs/r1",
            "/runs//review",
            "/runs/r1/review?from=a",
            "/etc/passwd",
            "../secret.html",
            "a/../../b",
            "https://example.com",
            "javascript:alert(1)",
            "mailto:a@b",
        ] {
            assert!(classify(link).is_err(), "{link:?} should be refused");
        }
    }

    #[test]
    fn a_guide_is_a_yaml_or_json_list_of_entries() {
        let yaml = "- title: Start here\n  text: The chosen option.\n  link: '#a'\n- title: The Run\n  link: '#/runs/r1'\n";
        let entries = parse(yaml).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].0.text, "", "text is optional");
        let json = r##"[{"title":"t","text":"x","link":"index.html"}]"##;
        assert_eq!(parse(json).unwrap()[0].0.link, "index.html");
    }

    #[test]
    fn a_bad_guide_says_which_entry_and_why() {
        let missing = parse("- title: t\n").unwrap_err();
        assert!(missing.contains("link"), "{missing}");
        let unknown = parse("- title: t\n  link: '#a'\n  colour: red\n").unwrap_err();
        assert!(unknown.contains("colour"), "{unknown}");
        let empty_title = parse("- title: ' '\n  link: '#a'\n").unwrap_err();
        assert_eq!(empty_title, "guide entry 1: title is empty");
        let bad_link = parse("- title: a\n  link: '#a'\n- title: b\n  link: 'http://x'\n").unwrap_err();
        assert!(bad_link.starts_with("guide entry 2: link `http://x`"), "{bad_link}");
        assert_eq!(parse("[]").unwrap_err(), "guide has no entry");
        let many = "- {title: t, link: '#a'}\n".repeat(MAX_ENTRIES + 1);
        assert!(parse(&many).unwrap_err().contains("at most"));
        assert!(parse("title: t").is_err(), "a map is not a list");
    }
}
