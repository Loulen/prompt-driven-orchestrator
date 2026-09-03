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
//! - **Import** — the policy of [`import_into_bank`]: an **unknown id is created**
//!   (same id, so the node's reference resolves at once) in a folder named after
//!   the pipeline; a **known id is left as is** (the bank's copy wins, never
//!   overwritten); a **label already taken** by another id is suffixed and the
//!   rename is a warning the UI shows; an id **absent from both** bank and
//!   sidecar is a warning too — never a failure: the pipeline stays importable
//!   and launchable, its node shows the missing-skill warning (#669).
//!
//! The reader accepts the sidecar as PDO writes it, as a user re-zips the
//! `<pipeline>.skills/` folder (entries `<id>/…` at the root), or bundled with
//! the YAML in one archive (`…/<pipeline>.skills/<id>/…` at any depth).

use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};
use std::path::Path;

use serde::Serialize;
use sqlx::SqlitePool;

use crate::pipeline::PipelineDef;
use crate::skill_bank::{self, SkillError, SKILL_MD};
use crate::skill_selection::SkillRef;

/// The folder suffix of the sidecar: `<pipeline>.skills/`.
pub(crate) const SIDECAR_SUFFIX: &str = ".skills";

/// One skill as the sidecar carries it: its id and every file of its folder,
/// `SKILL.md` included, keyed by the `/`-separated path relative to the folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SidecarSkill {
    pub id: String,
    pub files: BTreeMap<String, Vec<u8>>,
}

impl SidecarSkill {
    pub(crate) fn skill_md(&self) -> Option<&[u8]> {
        self.files.get(SKILL_MD).map(Vec::as_slice)
    }
}

/// The unique skill references of a pipeline's nodes, in first-seen order.
pub(crate) fn referenced_skills(pipeline: &PipelineDef) -> Vec<SkillRef> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for node in &pipeline.nodes {
        for skill in &node.skills {
            if seen.insert(skill.id.clone()) {
                out.push(skill.clone());
            }
        }
    }
    out
}

/// Read the content of every skill the pipeline references from the bank's disk
/// root. A referenced id with no folder is simply not in the sidecar (the
/// exporting instance already shows the warning for it); the ids skipped are
/// returned beside the skills so the caller can say so.
pub(crate) fn collect(
    repo_root: &Path,
    pipeline: &PipelineDef,
) -> (Vec<SidecarSkill>, Vec<String>) {
    let mut skills = Vec::new();
    let mut absent = Vec::new();
    for reference in referenced_skills(pipeline) {
        if !skill_bank::is_safe_skill_id(&reference.id) {
            absent.push(reference.id);
            continue;
        }
        let Ok(skill_md) = skill_bank::read_skill_md(repo_root, &reference.id) else {
            absent.push(reference.id);
            continue;
        };
        let mut files = BTreeMap::new();
        files.insert(SKILL_MD.to_string(), skill_md.into_bytes());
        for file in skill_bank::list_files(repo_root, &reference.id).unwrap_or_default() {
            if let Ok((_, data)) = skill_bank::read_file(repo_root, &reference.id, &file.path) {
                files.insert(file.path, data);
            }
        }
        skills.push(SidecarSkill {
            id: reference.id,
            files,
        });
    }
    (skills, absent)
}

/// The folder name the sidecar unzips to, next to the YAML.
pub(crate) fn sidecar_dir_name(pipeline_name: &str) -> String {
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

/// Serialise the sidecar as a zip: `<pipeline>.skills/<id>/<path>`, deflated,
/// entries sorted and stamped with a fixed date so two exports of the same bank
/// are byte-identical.
pub(crate) fn write_zip(pipeline_name: &str, skills: &[SidecarSkill]) -> Result<Vec<u8>, String> {
    let dir = sidecar_dir_name(pipeline_name);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::default());
    let mut sorted = skills.iter().collect::<Vec<_>>();
    sorted.sort_by(|a, b| a.id.cmp(&b.id));
    for skill in sorted {
        for (path, data) in &skill.files {
            zip.start_file(format!("{dir}/{}/{path}", skill.id), options)
                .map_err(|e| format!("failed to write sidecar entry {path}: {e}"))?;
            zip.write_all(data)
                .map_err(|e| format!("failed to write sidecar entry {path}: {e}"))?;
        }
    }
    let cursor = zip
        .finish()
        .map_err(|e| format!("failed to finish the skills sidecar: {e}"))?;
    Ok(cursor.into_inner())
}

/// Locate `(id, relative path)` in a zip entry name. Accepts the three shapes
/// the module doc lists; anything else (a file at the root, a `__MACOSX/`
/// resource fork) is ignored.
fn locate(entry_name: &str) -> Option<(String, String)> {
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

/// Parse a sidecar zip. Entries with an unsafe id or path are skipped (they
/// would be refused by the bank anyway); a skill without `SKILL.md` is dropped
/// too — there is nothing to validate or index.
pub(crate) fn read_zip(bytes: &[u8]) -> Result<Vec<SidecarSkill>, String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| format!("the skills sidecar is not a valid zip archive: {e}"))?;
    let mut by_id: BTreeMap<String, BTreeMap<String, Vec<u8>>> = BTreeMap::new();
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|e| format!("the skills sidecar is not a valid zip archive: {e}"))?;
        if entry.is_dir() {
            continue;
        }
        let Some((id, rel)) = locate(entry.name()) else {
            continue;
        };
        if !skill_bank::is_safe_skill_id(&id) {
            continue;
        }
        let Ok(rel) = skill_bank::normalise_file_path(&rel) else {
            continue;
        };
        if entry.size() > skill_bank::MAX_FILE_BYTES {
            continue;
        }
        let mut data = Vec::with_capacity(entry.size() as usize);
        entry
            .read_to_end(&mut data)
            .map_err(|e| format!("failed to read sidecar entry {}: {e}", entry.name()))?;
        by_id.entry(id).or_default().insert(rel, data);
    }
    Ok(by_id
        .into_iter()
        .filter(|(_, files)| files.contains_key(SKILL_MD))
        .map(|(id, files)| SidecarSkill { id, files })
        .collect())
}

/// What the import did to the bank, per skill — the structured half of the
/// answer; the human-readable half is the `warnings` list.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ImportReport {
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
    pub folder: Option<skill_bank::SkillFolder>,
    /// Non-fatal diagnostics, in the document's `skills.<id>: …` idiom.
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct RenamedSkill {
    pub id: String,
    pub from: String,
    pub to: String,
}

/// The folder created skills are filed under: `importés avec <pipeline>`.
pub(crate) fn import_folder_name(pipeline_name: &str) -> String {
    format!("importés avec {pipeline_name}")
}

async fn import_folder(
    db: &SqlitePool,
    pipeline_name: &str,
) -> Result<skill_bank::SkillFolder, SkillError> {
    let name = import_folder_name(pipeline_name);
    if let Some(existing) = skill_bank::list_folders(db)
        .await?
        .into_iter()
        .find(|f| f.parent_id.is_none() && f.name.eq_ignore_ascii_case(&name))
    {
        return Ok(existing);
    }
    skill_bank::create_folder(db, &name, None).await
}

/// Find a label the bank does not use yet: `name`, then `name-2`, `name-3`…
async fn free_label(db: &SqlitePool, wanted: &str) -> Result<String, SkillError> {
    if skill_bank::find_by_name_ci(db, wanted).await?.is_none() {
        return Ok(wanted.to_string());
    }
    for n in 2..10_000u32 {
        let candidate = format!("{wanted}-{n}");
        if skill_bank::find_by_name_ci(db, &candidate).await?.is_none() {
            return Ok(candidate);
        }
    }
    Err(SkillError::Storage(format!(
        "could not find a free label for `{wanted}`"
    )))
}

/// Apply the import policy to the bank and **relabel the pipeline's references**
/// with the names the bank ended up with (the reference's `name` is a label; the
/// id is the identity and never changes).
pub(crate) async fn import_into_bank(
    db: &SqlitePool,
    repo_root: &Path,
    pipeline: &mut PipelineDef,
    sidecar: &[SidecarSkill],
) -> Result<ImportReport, SkillError> {
    let mut report = ImportReport::default();
    let references = referenced_skills(pipeline);
    if references.is_empty() {
        return Ok(report);
    }
    let mut final_names: BTreeMap<String, String> = BTreeMap::new();
    for reference in references {
        let label = if reference.name.trim().is_empty() {
            None
        } else {
            Some(reference.name.trim().to_string())
        };
        if let Some(existing) = skill_bank::get(db, &reference.id).await? {
            final_names.insert(reference.id.clone(), existing.name.clone());
            report.kept.push(SkillRef {
                id: existing.id,
                name: existing.name,
            });
            continue;
        }
        let Some(carried) = sidecar.iter().find(|s| s.id == reference.id) else {
            report.warnings.push(format!(
                "skills.{}: skill `{}` is absent from the bank and from the sidecar — the \
                 pipeline keeps the reference and its node shows a warning until the skill \
                 is added",
                reference.id,
                label.clone().unwrap_or_else(|| reference.id.clone())
            ));
            report.missing.push(reference);
            continue;
        };
        let content = match carried
            .skill_md()
            .map(|bytes| String::from_utf8(bytes.to_vec()))
        {
            Some(Ok(content)) => content,
            _ => {
                report.warnings.push(format!(
                    "skills.{}: the sidecar's SKILL.md is not UTF-8 text — skill not imported",
                    reference.id
                ));
                report.missing.push(reference);
                continue;
            }
        };
        let parsed = match skill_bank::validate_skill_md(&content) {
            Ok(parsed) => parsed,
            Err(error) => {
                report.warnings.push(format!(
                    "skills.{}: the sidecar's SKILL.md is invalid ({error}) — skill not imported",
                    reference.id
                ));
                report.missing.push(reference);
                continue;
            }
        };
        let wanted = label.clone().unwrap_or_else(|| parsed.name.clone());
        let name = free_label(db, &wanted).await?;
        let folder = match &report.folder {
            Some(folder) => folder.clone(),
            None => {
                let folder = import_folder(db, &pipeline.name).await?;
                report.folder = Some(folder.clone());
                folder
            }
        };
        let created = skill_bank::create_with_id(
            db,
            repo_root,
            Some(&reference.id),
            &content,
            Some(&name),
            Some(&folder.id),
        )
        .await?;
        for (path, data) in &carried.files {
            if path == SKILL_MD {
                continue;
            }
            if let Err(error) = skill_bank::write_file(repo_root, &created.id, path, data) {
                report.warnings.push(format!(
                    "skills.{}: reference file `{path}` not imported: {error}",
                    created.id
                ));
            }
        }
        if name != wanted {
            report.warnings.push(format!(
                "skills.{}: the name `{wanted}` is already used by another skill — imported as \
                 `{name}`",
                created.id
            ));
            report.renamed.push(RenamedSkill {
                id: created.id.clone(),
                from: wanted,
                to: name.clone(),
            });
        }
        final_names.insert(created.id.clone(), created.name.clone());
        report.created.push(SkillRef {
            id: created.id,
            name: created.name,
        });
    }
    for node in &mut pipeline.nodes {
        for skill in &mut node.skills {
            if let Some(name) = final_names.get(&skill.id) {
                skill.name = name.clone();
            }
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skill(id: &str, name: &str) -> SidecarSkill {
        let mut files = BTreeMap::new();
        files.insert(
            SKILL_MD.to_string(),
            format!(
                "---\nname: {name}\ndescription: The {name} method.\n---\n\n# {name}\n\nBody.\n"
            )
            .into_bytes(),
        );
        files.insert("examples/one.md".to_string(), b"# example\n".to_vec());
        SidecarSkill {
            id: id.into(),
            files,
        }
    }

    #[test]
    fn zip_round_trips_under_the_pipeline_skills_folder() {
        let skills = vec![skill("bbbb", "tdd"), skill("aaaa", "grilling")];
        let bytes = write_zip("My pipeline", &skills).unwrap();

        let names = {
            let mut archive = zip::ZipArchive::new(Cursor::new(bytes.as_slice())).unwrap();
            (0..archive.len())
                .map(|i| archive.by_index(i).unwrap().name().to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            names,
            vec![
                "My pipeline.skills/aaaa/SKILL.md",
                "My pipeline.skills/aaaa/examples/one.md",
                "My pipeline.skills/bbbb/SKILL.md",
                "My pipeline.skills/bbbb/examples/one.md",
            ]
        );

        let mut back = read_zip(&bytes).unwrap();
        back.sort_by(|a, b| a.id.cmp(&b.id));
        let mut expected = skills;
        expected.sort_by(|a, b| a.id.cmp(&b.id));
        assert_eq!(back, expected);
    }

    #[test]
    fn export_is_deterministic() {
        let skills = vec![skill("bbbb", "tdd"), skill("aaaa", "grilling")];
        assert_eq!(
            write_zip("p", &skills).unwrap(),
            write_zip("p", &skills).unwrap()
        );
    }

    #[test]
    fn reader_accepts_root_ids_and_nested_sidecars_and_skips_junk() {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        for (name, body) in [
            ("aaaa/SKILL.md", "---\nname: a\ndescription: d\n---\nbody"),
            ("aaaa/ref.txt", "ref"),
            (
                "bundle/p.skills/bbbb/SKILL.md",
                "---\nname: b\ndescription: d\n---\nbody",
            ),
            ("bundle/p.pdo.yaml", "pdo_pipeline: 1"),
            ("__MACOSX/aaaa/._SKILL.md", "junk"),
            ("cccc/notes.md", "no SKILL.md here"),
            ("../evil/SKILL.md", "escape"),
            ("dddd/../SKILL.md", "escape"),
        ] {
            zip.start_file(name, options).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        }
        let bytes = zip.finish().unwrap().into_inner();

        let skills = read_zip(&bytes).unwrap();
        let ids = skills.iter().map(|s| s.id.as_str()).collect::<Vec<_>>();
        assert_eq!(ids, vec!["aaaa", "bbbb"]);
        assert_eq!(skills[0].files.len(), 2);
        assert_eq!(skills[0].files["ref.txt"], b"ref");
    }

    #[test]
    fn garbage_is_not_a_sidecar() {
        assert!(read_zip(b"not a zip").is_err());
    }

    #[test]
    fn sidecar_dir_name_never_carries_a_separator() {
        assert_eq!(sidecar_dir_name("a/b"), "a_b.skills");
        assert_eq!(sidecar_dir_name("  "), "pipeline.skills");
    }
}
