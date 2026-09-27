//! The skills sidecar as a zip archive (#673): what a browser downloads beside
//! the pipeline's YAML, and hands back base64 on import.

use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};

use crate::domain::error::MAX_FILE_BYTES;
use crate::domain::file_path::normalise_file_path;
use crate::domain::sidecar::{locate, sidecar_dir_name, SidecarSkill};
use crate::domain::skill::{is_safe_skill_id, SKILL_MD};

/// Serialise the sidecar as a zip: `<pipeline>.skills/<id>/<path>`, deflated,
/// entries sorted and stamped with a fixed date so two exports of the same bank
/// are byte-identical.
pub fn write_zip(pipeline_name: &str, skills: &[SidecarSkill]) -> Result<Vec<u8>, String> {
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

/// Parse a sidecar zip. Entries with an unsafe id or path are skipped (they
/// would be refused by the bank anyway); a skill without `SKILL.md` is dropped
/// too — there is nothing to validate or index.
pub fn read_zip(bytes: &[u8]) -> Result<Vec<SidecarSkill>, String> {
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
        if !is_safe_skill_id(&id) {
            continue;
        }
        let Ok(rel) = normalise_file_path(&rel) else {
            continue;
        };
        if entry.size() > MAX_FILE_BYTES {
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
}
