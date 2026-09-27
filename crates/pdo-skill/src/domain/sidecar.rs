//! The **skills sidecar** of a portable pipeline document (#673, ADR-0062
//! "Voyage par document", CONTEXT.md §*Banque de skills*).
//!
//! ADR-0059 draws the document's frontier at "no instance configuration". A
//! skill is **content**, not configuration: a node references it by an id the
//! receiving instance cannot know, so the export ships the content beside the
//! YAML and the import adds it to the bank. Two halves, one layout:
//!
//! - **Export** — a zip whose entries are `<pipeline>.skills/<id>/SKILL.md` and
//!   the skill's reference files. Unzipped next to `<pipeline>.pdo.yaml` it is
//!   exactly the on-disk sidecar the ticket names; zipped it is what a browser
//!   can download. The YAML itself is untouched: `skills` already travels on the
//!   node (#669).
//! - **Import** — an **unknown id is created** (same id, so the node's reference
//!   resolves at once) in a folder named after the pipeline; a **known id is
//!   left as is** (the bank's copy wins, never overwritten); a **label already
//!   taken** by another id is suffixed and the rename is a warning the UI shows;
//!   an id **absent from both** bank and sidecar is a warning too — never a
//!   failure: the pipeline stays importable and launchable, its node shows the
//!   missing-skill warning (#669).
//!
//! The reader accepts the sidecar as PDO writes it, as a user re-zips the
//! `<pipeline>.skills/` folder (entries `<id>/…` at the root), or bundled with
//! the YAML in one archive (`…/<pipeline>.skills/<id>/…` at any depth).

use std::collections::BTreeMap;

use serde::Serialize;
use ts_rs::TS;

use super::selection::SkillRef;
use super::skill::{SkillFolder, SKILL_MD};

/// The folder suffix of the sidecar: `<pipeline>.skills/`.
pub const SIDECAR_SUFFIX: &str = ".skills";

/// One skill as the sidecar carries it: its id and every file of its folder,
/// `SKILL.md` included, keyed by the `/`-separated path relative to the folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidecarSkill {
    pub id: String,
    pub files: BTreeMap<String, Vec<u8>>,
}

impl SidecarSkill {
    pub fn skill_md(&self) -> Option<&[u8]> {
        self.files.get(SKILL_MD).map(Vec::as_slice)
    }
}

/// The unique skill references of a pipeline's nodes (one selection per node),
/// in first-seen order.
pub fn referenced_skills<'a>(nodes: impl IntoIterator<Item = &'a [SkillRef]>) -> Vec<SkillRef> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for skills in nodes {
        for skill in skills {
            if seen.insert(skill.id.clone()) {
                out.push(skill.clone());
            }
        }
    }
    out
}

/// The folder name the sidecar unzips to, next to the YAML.
pub fn sidecar_dir_name(pipeline_name: &str) -> String {
    let stem = pipeline_name
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | '\0') {
                '_'
            } else {
                c
            }
        })
        .collect::<String>();
    let stem = if stem.trim().is_empty() {
        "pipeline".to_string()
    } else {
        stem
    };
    format!("{stem}{SIDECAR_SUFFIX}")
}

/// Locate `(id, relative path)` in a zip entry name. Accepts the three shapes
/// the module doc lists; anything else (a file at the root, a `__MACOSX/`
/// resource fork) is ignored.
pub fn locate(entry_name: &str) -> Option<(String, String)> {
    let parts = entry_name
        .split('/')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();
    if parts.iter().any(|p| *p == "__MACOSX" || *p == ".DS_Store") {
        return None;
    }
    let start = match parts.iter().position(|p| p.ends_with(SIDECAR_SUFFIX)) {
        Some(index) => index + 1,
        None => 0,
    };
    if parts.len() < start + 2 {
        return None;
    }
    let id = parts[start].to_string();
    let rel = parts[start + 1..].join("/");
    Some((id, rel))
}

/// What the import did to the bank, per skill — the structured half of the
/// answer; the human-readable half is the `warnings` list.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, rename = "SkillSidecarImportReport")]
pub struct SidecarImportReport {
    /// Ids created from the sidecar, with their final label.
    pub created: Vec<SkillRef>,
    /// Ids the bank already knew — left untouched.
    pub kept: Vec<SkillRef>,
    /// Created skills whose label had to be suffixed.
    pub renamed: Vec<RenamedSkill>,
    /// Ids neither in the bank nor in the sidecar (or with an invalid SKILL.md).
    pub missing: Vec<SkillRef>,
    /// The folder the created skills were filed under, when any was.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub folder: Option<SkillFolder>,
    /// Non-fatal diagnostics, in the document's `skills.<id>: …` idiom.
    pub warnings: Vec<String>,
}

impl SidecarImportReport {
    /// The label every referenced id ended up with in the bank: the pipeline
    /// relabels its node references with them (the id never changes).
    pub fn final_names(&self) -> BTreeMap<String, String> {
        self.kept
            .iter()
            .chain(&self.created)
            .map(|s| (s.id.clone(), s.name.clone()))
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, rename = "SidecarRenamedSkill")]
pub struct RenamedSkill {
    pub id: String,
    pub from: String,
    pub to: String,
}

/// The folder created skills are filed under: `importés avec <pipeline>`.
pub fn import_folder_name(pipeline_name: &str) -> String {
    format!("importés avec {pipeline_name}")
}

/// `name`, then `name-2`, `name-3`… — the candidates for a free label.
pub fn label_candidates(wanted: &str) -> impl Iterator<Item = String> + '_ {
    std::iter::once(wanted.to_string()).chain((2..10_000u32).map(move |n| format!("{wanted}-{n}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(id: &str, name: &str) -> SkillRef {
        SkillRef {
            id: id.into(),
            name: name.into(),
        }
    }

    #[test]
    fn sidecar_dir_name_never_carries_a_separator() {
        assert_eq!(sidecar_dir_name("a/b"), "a_b.skills");
        assert_eq!(sidecar_dir_name("  "), "pipeline.skills");
    }

    #[test]
    fn entries_are_located_under_any_sidecar_folder_or_at_the_root() {
        assert_eq!(
            locate("p.skills/aaaa/examples/one.md"),
            Some(("aaaa".into(), "examples/one.md".into()))
        );
        assert_eq!(
            locate("bundle/p.skills/bbbb/SKILL.md"),
            Some(("bbbb".into(), "SKILL.md".into()))
        );
        assert_eq!(
            locate("aaaa/SKILL.md"),
            Some(("aaaa".into(), "SKILL.md".into()))
        );
        assert_eq!(locate("__MACOSX/aaaa/._SKILL.md"), None);
        assert_eq!(locate("root.txt"), None);
    }

    #[test]
    fn references_are_unique_in_first_seen_order() {
        let a = [r("a", "tdd"), r("b", "x")];
        let b = [r("b", "x"), r("c", "y")];
        let refs = referenced_skills([&a[..], &b[..]]);
        assert_eq!(refs, vec![r("a", "tdd"), r("b", "x"), r("c", "y")]);
    }

    #[test]
    fn free_labels_are_suffixed_from_two() {
        let mut labels = label_candidates("tdd");
        assert_eq!(labels.next().as_deref(), Some("tdd"));
        assert_eq!(labels.next().as_deref(), Some("tdd-2"));
    }
}
