//! The versions of a registry pipeline (ADR-0088, CONTEXT.md « Version de
//! pipeline »): a Save freezes the draft as a numbered, titled version. The
//! file of the registry is always the last one.

use std::collections::BTreeMap;

use pdo_kernel::Actor;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A version as a Run retains it and the list shows the last one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PipelineVersionRef {
    pub number: u32,
    pub title: String,
}

/// What is known of a version, without its content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionMeta {
    pub number: u32,
    pub title: String,
    /// Who saved it; `None` for the version the file became at the migration,
    /// and for a file written outside PDO.
    pub author: Option<Actor>,
    /// RFC 3339.
    pub saved_at: String,
    /// The file was written outside PDO.
    pub external: bool,
}

impl VersionMeta {
    pub fn reference(&self) -> PipelineVersionRef {
        PipelineVersionRef {
            number: self.number,
            title: self.title.clone(),
        }
    }
}

/// A version with what it holds: the YAML of the file and its prompts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineVersion {
    pub meta: VersionMeta,
    pub yaml: String,
    pub prompts: BTreeMap<String, String>,
}

/// How a Save lands among the versions.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SaveAs {
    /// `false`: the last version is replaced, under its number.
    #[serde(default = "yes")]
    pub new_version: bool,
    /// The title; blank or absent is the default one.
    #[serde(default)]
    pub title: Option<String>,
    /// The version saved until now is kept as a pipeline of its own.
    #[serde(default)]
    pub keep_old: bool,
}

fn yes() -> bool {
    true
}

impl Default for SaveAs {
    fn default() -> Self {
        Self::new_version()
    }
}

impl SaveAs {
    /// A new version under the default title.
    pub fn new_version() -> Self {
        Self {
            new_version: true,
            title: None,
            keep_old: false,
        }
    }
}

/// The title of a version a write outside PDO made.
pub const EXTERNAL_EDIT: &str = "external edit";

/// The title a version gets when none is given.
pub fn default_title(number: u32) -> String {
    format!("v{number}")
}

/// The title the user gave, else the default one.
pub fn title_or_default(title: Option<&str>, number: u32) -> String {
    match title.map(str::trim) {
        Some(title) if !title.is_empty() => title.to_string(),
        _ => default_title(number),
    }
}

/// The title of the version a file becomes at the migration: the free
/// « Version » of its identity when it has one, else `v1`.
pub fn first_title(declared: Option<&str>) -> String {
    title_or_default(declared, 1)
}

/// The number and title a Save gives, after `last` (none: the first).
pub fn next_version(last: Option<&VersionMeta>, save_as: &SaveAs) -> (u32, String) {
    match last {
        Some(last) if !save_as.new_version => {
            // Replaced: the number stays; a title only an outside write gave
            // does not say what is saved now.
            let kept = if last.external {
                default_title(last.number)
            } else {
                last.title.clone()
            };
            (
                last.number,
                title_or_default(save_as.title.as_deref().or(Some(&kept)), last.number),
            )
        }
        Some(last) => {
            let number = last.number + 1;
            (number, title_or_default(save_as.title.as_deref(), number))
        }
        None => (1, title_or_default(save_as.title.as_deref(), 1)),
    }
}

/// The name of the pipeline a version is kept as: `<name> (vN)`.
pub fn kept_name(name: &str, number: u32) -> String {
    format!("{} (v{number})", name.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(number: u32, title: &str, external: bool) -> VersionMeta {
        VersionMeta {
            number,
            title: title.into(),
            author: None,
            saved_at: "2026-10-19T00:00:00Z".into(),
            external,
        }
    }

    #[test]
    fn a_new_version_takes_the_next_number_and_its_default_title() {
        let last = meta(8, "Add reviewer loop", false);
        assert_eq!(
            next_version(Some(&last), &SaveAs::new_version()),
            (9, "v9".into())
        );
        let titled = SaveAs {
            title: Some("  Split prompt ".into()),
            ..SaveAs::new_version()
        };
        assert_eq!(
            next_version(Some(&last), &titled),
            (9, "Split prompt".into())
        );
        assert_eq!(next_version(None, &SaveAs::new_version()), (1, "v1".into()));
    }

    #[test]
    fn a_replacement_keeps_the_number_and_the_title_unless_an_outside_write_gave_it() {
        let replace = SaveAs {
            new_version: false,
            ..SaveAs::default()
        };
        let last = meta(8, "Add reviewer loop", false);
        assert_eq!(
            next_version(Some(&last), &replace),
            (8, "Add reviewer loop".into())
        );
        let external = meta(8, "external edit", true);
        assert_eq!(next_version(Some(&external), &replace), (8, "v8".into()));
        let titled = SaveAs {
            title: Some("Fixed".into()),
            ..replace
        };
        assert_eq!(next_version(Some(&external), &titled), (8, "Fixed".into()));
    }

    #[test]
    fn the_first_version_is_titled_by_the_free_version_field() {
        assert_eq!(first_title(Some("1.0")), "1.0");
        assert_eq!(first_title(Some("  ")), "v1");
        assert_eq!(first_title(None), "v1");
    }

    #[test]
    fn a_kept_version_is_named_after_its_pipeline_and_number() {
        assert_eq!(
            kept_name("feature-with-review ", 8),
            "feature-with-review (v8)"
        );
    }

    #[test]
    fn a_save_without_options_is_a_new_version() {
        let parsed: SaveAs = serde_json::from_str("{}").unwrap();
        assert!(parsed.new_version && !parsed.keep_old && parsed.title.is_none());
    }
}
