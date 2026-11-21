//! What an Agent de support reads besides its prompt: the skills delivered
//! into its folder (the core's and the extensions', from the Banque de skills)
//! and the core's code at the tag of the instance, a shallow clone of the
//! public repository kept in a cache. Where that clone lives, which tag it is,
//! and what the agent's instructions say of both.

use std::path::{Path, PathBuf};

/// The tag of the public repository the core `version` was released at.
pub fn core_source_tag(version: &str) -> String {
    format!("v{version}")
}

/// The clone of the core at `tag`, under `cache_root`.
pub fn core_source_dir(cache_root: &Path, tag: &str) -> PathBuf {
    cache_root.join(tag)
}

/// Where the core's code at the instance's tag stands, as the agent's
/// instructions say it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreSource {
    /// Cloned already, at `dir`.
    Cloned { tag: String, dir: PathBuf },
    /// The tag is public: cloned at the first demand.
    Published { tag: String },
    /// The tag is not public: nothing is cloned, ever.
    NoPublicTag { tag: String },
    /// Whether the tag is public could not be checked (no network, no git).
    Unchecked { tag: String, reason: String },
}

/// The clone of the core at the instance's tag, ready to read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreCheckout {
    pub tag: String,
    pub dir: PathBuf,
}

/// Why the core's code was not given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreSourceRefusal {
    /// The version has no public tag: no clone was tried.
    NoPublicTag { tag: String },
    /// Whether the tag is public could not be checked: no clone was tried.
    Unchecked { tag: String, reason: String },
    /// The tag is public, the clone failed.
    CloneFailed { tag: String, reason: String },
}

impl std::fmt::Display for CoreSourceRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CoreSourceRefusal::NoPublicTag { tag } => write!(
                f,
                "this version of the core has no public tag (`{tag}`): its code is not available"
            ),
            CoreSourceRefusal::Unchecked { tag, reason } => write!(
                f,
                "could not check whether the tag `{tag}` of the core is public: {reason}"
            ),
            CoreSourceRefusal::CloneFailed { tag, reason } => {
                write!(f, "the clone of the core at `{tag}` failed: {reason}")
            }
        }
    }
}

/// The part of the agent's instructions that depends on the instance: the
/// skills delivered into its folder, and where the core's code stands.
pub fn instance_section(skills: &[String], core: &CoreSource) -> String {
    let skills_line = if skills.is_empty() {
        "- No skill was delivered to your working folder.".to_string()
    } else {
        let names: Vec<String> = skills.iter().map(|name| format!("`{name}`")).collect();
        format!(
            "- Skills delivered in your working folder (`.agents/skills/`): {}. An extension's \
             skill is how that extension is used: read its `SKILL.md` before answering about it.",
            names.join(", ")
        )
    };
    let cite = "Read it there and cite the file (`path:line`) when an answer depends on it.";
    let core_line = match core {
        CoreSource::Cloned { tag, dir } => format!(
            "- The core's code at this instance's version (tag `{tag}`) is cloned in `{}`. {cite}",
            dir.display()
        ),
        CoreSource::Published { tag } => format!(
            "- The core's code at this instance's version (tag `{tag}`) is public: \
             `pdo support source` prints the folder of its clone (cloned the first time, then \
             reused). {cite}"
        ),
        CoreSource::NoPublicTag { tag } => format!(
            "- The core's code is not available: this version has no public tag (`{tag}`). Do \
             not clone or download it; say so when an answer depends on it."
        ),
        CoreSource::Unchecked { tag, reason } => format!(
            "- Whether the core's code at tag `{tag}` is public could not be checked ({reason}). \
             `pdo support source` checks again and clones it; if it refuses, answer without the \
             code and say so."
        ),
    };
    format!("## This instance\n\n{skills_line}\n{core_line}\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tag_is_the_version_and_its_clone_lives_under_the_cache() {
        assert_eq!(core_source_tag("2.80.4"), "v2.80.4");
        assert_eq!(
            core_source_dir(Path::new("/state/core-source"), "v2.80.4"),
            Path::new("/state/core-source/v2.80.4")
        );
    }

    #[test]
    fn the_section_names_the_delivered_skills() {
        let section = instance_section(
            &["pdo-orchestrate".into(), "test-cli".into()],
            &CoreSource::Published {
                tag: "v2.80.4".into(),
            },
        );
        assert!(section.starts_with("## This instance\n"), "{section}");
        assert!(
            section.contains("(`.agents/skills/`): `pdo-orchestrate`, `test-cli`."),
            "{section}"
        );
        let none = instance_section(
            &[],
            &CoreSource::Published {
                tag: "v2.80.4".into(),
            },
        );
        assert!(none.contains("No skill was delivered"), "{none}");
    }

    #[test]
    fn the_section_says_where_the_core_code_is_or_that_there_is_none() {
        let published = instance_section(
            &[],
            &CoreSource::Published {
                tag: "v2.80.4".into(),
            },
        );
        assert!(published.contains("tag `v2.80.4`"), "{published}");
        assert!(published.contains("`pdo support source`"), "{published}");

        let cloned = instance_section(
            &[],
            &CoreSource::Cloned {
                tag: "v2.80.4".into(),
                dir: "/state/core-source/v2.80.4".into(),
            },
        );
        assert!(
            cloned.contains("cloned in `/state/core-source/v2.80.4`"),
            "{cloned}"
        );

        let none = instance_section(
            &[],
            &CoreSource::NoPublicTag {
                tag: "v9.0.0".into(),
            },
        );
        assert!(none.contains("not available"), "{none}");
        assert!(none.contains("no public tag (`v9.0.0`)"), "{none}");
        assert!(!none.contains("pdo support source"), "never tried: {none}");

        let unchecked = instance_section(
            &[],
            &CoreSource::Unchecked {
                tag: "v2.80.4".into(),
                reason: "offline".into(),
            },
        );
        assert!(
            unchecked.contains("could not be checked (offline)"),
            "{unchecked}"
        );
    }

    #[test]
    fn a_refusal_says_why_the_code_was_not_given() {
        let none = CoreSourceRefusal::NoPublicTag {
            tag: "v9.0.0".into(),
        };
        assert!(none.to_string().contains("no public tag (`v9.0.0`)"));
        let failed = CoreSourceRefusal::CloneFailed {
            tag: "v2.80.4".into(),
            reason: "disk full".into(),
        };
        assert!(failed.to_string().ends_with("disk full"));
    }
}
