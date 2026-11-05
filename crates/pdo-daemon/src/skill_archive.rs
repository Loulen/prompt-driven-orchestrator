//! The **Archive de skills** (pdo-issues#2, ADR-0082, CONTEXT.md §*Banque de
//! skills*): skills leave the bank in one `.zip`, **without a pipeline**.
//!
//! The archive is **flat**: one folder per skill — `SKILL.md` byte for byte as
//! the bank stores it, plus every reference file under that folder — and, at the
//! root, the **manifest** `pdo-skills.json` that ties each folder to the skill's
//! **id** (format version, export timestamp, `{ folder, id, name }` per skill).
//! The ids travel in the manifest, never in the frontmatter, so the content (and
//! its hash) is untouched; unzipped, the archive is a valid « local folder »
//! Source for the existing import.
//!
//! The folder of a skill is its **name**, sanitised and made unique within the
//! archive: the bank's hierarchy does not travel (the importer picks where the
//! skills land). Seeded skills (locked, « Managed by PDO ») never enter an
//! archive: every instance seeds its own.

use std::collections::HashSet;
use std::io::{Cursor, Write};
use std::path::Path;

use chrono::{DateTime, Local, Utc};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::skill_bank::{self, SKILL_MD};

/// The manifest's file name, at the archive's root.
pub(crate) const MANIFEST: &str = "pdo-skills.json";
/// The manifest's `format` tag: what an importer checks before trusting ids.
pub(crate) const FORMAT: &str = "pdo-skills";
/// Bumped when the archive layout or the manifest changes incompatibly.
pub(crate) const FORMAT_VERSION: u32 = 1;

/// `pdo-skills.json`: what the archive carries, folder by folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Manifest {
    pub format: String,
    pub version: u32,
    /// RFC 3339, UTC.
    pub exported_at: String,
    pub skills: Vec<ManifestEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ManifestEntry {
    /// The archive folder holding the skill (`<folder>/SKILL.md`).
    pub folder: String,
    /// The skill's id in the exporting bank — kept by the import.
    pub id: String,
    /// The skill's label in the exporting bank.
    pub name: String,
}

/// Why an export was refused. Every variant names what to fix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExportError {
    /// No id at all: an empty archive is never produced.
    EmptySelection,
    /// An id the bank does not know.
    UnknownSkill(String),
    /// A seeded (locked) skill: every instance seeds its own.
    Locked(String),
    Storage(String),
}

impl std::fmt::Display for ExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptySelection => write!(f, "nothing to export: check at least one skill"),
            Self::UnknownSkill(id) => write!(f, "unknown skill id `{id}`: it is not in the bank"),
            Self::Locked(id) => write!(
                f,
                "skill `{id}` is managed by PDO (seeded at startup) and cannot be exported"
            ),
            Self::Storage(reason) => write!(f, "export failed: {reason}"),
        }
    }
}

/// A finished export: the resolved file name, the zip, and what it holds.
#[derive(Debug, Clone)]
pub(crate) struct Export {
    pub file_name: String,
    pub bytes: Vec<u8>,
    pub manifest: Manifest,
}

/// Characters a file or folder name never carries across systems.
fn is_forbidden(c: char) -> bool {
    c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
}

/// The archive folder of a skill named `name`: separators and reserved
/// characters become `-`, surrounding blanks and dots are dropped, and an empty
/// result falls back to `skill`.
pub(crate) fn sanitise_folder(name: &str) -> String {
    let replaced = name
        .chars()
        .map(|c| if is_forbidden(c) { '-' } else { c })
        .collect::<String>();
    let trimmed = replaced.trim_matches(|c: char| c.is_whitespace() || c == '.');
    if trimmed.is_empty() {
        "skill".to_string()
    } else {
        trimmed.to_string()
    }
}

/// `sanitise_folder(name)`, suffixed `-2`, `-3`… until no folder of the archive
/// (case-insensitively — the archive may be unzipped on macOS or Windows) nor the
/// manifest carries it. Records the result in `used`.
fn unique_folder(name: &str, used: &mut HashSet<String>) -> String {
    let base = sanitise_folder(name);
    let mut candidate = base.clone();
    let mut n = 2u32;
    while used.contains(&candidate.to_lowercase()) || candidate.eq_ignore_ascii_case(MANIFEST) {
        candidate = format!("{base}-{n}");
        n += 1;
    }
    used.insert(candidate.to_lowercase());
    candidate
}

/// The default file name of an archive (pdo-issues#2): `<name>.skills.zip` for
/// one skill, `pdo-skills-<YYYYMMDD-HHMM>.zip` otherwise (local time, as the UI
/// pre-fills it).
pub(crate) fn default_file_name(names: &[&str], now: DateTime<Local>) -> String {
    match names {
        [one] => format!("{}.skills.zip", sanitise_folder(one)),
        _ => format!("pdo-skills-{}.zip", now.format("%Y%m%d-%H%M")),
    }
}

/// The file name the download carries: the requested one when any (its last
/// path segment, reserved characters dropped), else [`default_file_name`];
/// `.zip` appended when missing.
pub(crate) fn resolve_file_name(
    requested: Option<&str>,
    names: &[&str],
    now: DateTime<Local>,
) -> String {
    let requested = requested
        .map(|raw| {
            let last = raw.rsplit(['/', '\\']).next().unwrap_or("");
            last.chars()
                .filter(|c| !is_forbidden(*c))
                .collect::<String>()
                .trim()
                .to_string()
        })
        .filter(|name| !name.trim_matches('.').is_empty());
    let name = requested.unwrap_or_else(|| default_file_name(names, now));
    if name.to_lowercase().ends_with(".zip") {
        name
    } else {
        format!("{name}.zip")
    }
}

/// Build the archive of `ids` (in that order, duplicates dropped). Refuses the
/// whole export on the first unknown or seeded id — never a partial archive.
pub(crate) async fn export(
    db: &SqlitePool,
    repo_root: &Path,
    ids: &[String],
    requested_name: Option<&str>,
    now: DateTime<Utc>,
) -> Result<Export, ExportError> {
    let mut seen = HashSet::new();
    let ids = ids
        .iter()
        .map(|id| id.trim())
        .filter(|id| !id.is_empty() && seen.insert(id.to_string()))
        .collect::<Vec<_>>();
    if ids.is_empty() {
        return Err(ExportError::EmptySelection);
    }
    let mut skills = Vec::with_capacity(ids.len());
    for id in ids {
        if !skill_bank::is_safe_skill_id(id) {
            return Err(ExportError::UnknownSkill(id.to_string()));
        }
        let skill = skill_bank::get(db, id)
            .await
            .map_err(|e| ExportError::Storage(e.to_string()))?
            .ok_or_else(|| ExportError::UnknownSkill(id.to_string()))?;
        if skill.locked || skill_bank::is_seeded(&skill.id) {
            return Err(ExportError::Locked(skill.id));
        }
        skills.push(skill);
    }

    let mut used = HashSet::new();
    let mut entries = Vec::with_capacity(skills.len());
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::default());
    let put = |zip: &mut zip::ZipWriter<Cursor<Vec<u8>>>, path: &str, data: &[u8]| {
        zip.start_file(path, options)
            .and_then(|()| zip.write_all(data).map_err(Into::into))
            .map_err(|e| ExportError::Storage(format!("failed to write `{path}`: {e}")))
    };
    for skill in &skills {
        let folder = unique_folder(&skill.name, &mut used);
        let dir = skill_bank::skill_dir(repo_root, &skill.id);
        let skill_md = std::fs::read(dir.join(SKILL_MD)).map_err(|e| {
            ExportError::Storage(format!("cannot read the SKILL.md of `{}`: {e}", skill.name))
        })?;
        put(&mut zip, &format!("{folder}/{SKILL_MD}"), &skill_md)?;
        let files = skill_bank::list_files(repo_root, &skill.id)
            .map_err(|e| ExportError::Storage(e.to_string()))?;
        for file in files {
            let (_, data) = skill_bank::read_file(repo_root, &skill.id, &file.path)
                .map_err(|e| ExportError::Storage(e.to_string()))?;
            put(&mut zip, &format!("{folder}/{}", file.path), &data)?;
        }
        entries.push(ManifestEntry {
            folder,
            id: skill.id.clone(),
            name: skill.name.clone(),
        });
    }
    let manifest = Manifest {
        format: FORMAT.to_string(),
        version: FORMAT_VERSION,
        exported_at: now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        skills: entries,
    };
    let json = serde_json::to_vec_pretty(&manifest)
        .map_err(|e| ExportError::Storage(format!("failed to write the manifest: {e}")))?;
    put(&mut zip, MANIFEST, &json)?;
    let bytes = zip
        .finish()
        .map_err(|e| ExportError::Storage(format!("failed to finish the archive: {e}")))?
        .into_inner();

    let names = skills.iter().map(|s| s.name.as_str()).collect::<Vec<_>>();
    let file_name = resolve_file_name(requested_name, &names, now.with_timezone(&Local));
    Ok(Export {
        file_name,
        bytes,
        manifest,
    })
}

/// `attachment; filename="…"; filename*=UTF-8''…` — an ASCII fallback for old
/// clients, the exact UTF-8 name (RFC 5987) for the others.
pub(crate) fn content_disposition(file_name: &str) -> String {
    let ascii = file_name
        .chars()
        .map(|c| {
            if c.is_ascii() && !c.is_ascii_control() && c != '"' && c != '\\' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();
    let mut encoded = String::new();
    for byte in file_name.bytes() {
        if byte.is_ascii_alphanumeric() || b"!#$&+-.^_`|~".contains(&byte) {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    format!("attachment; filename=\"{ascii}\"; filename*=UTF-8''{encoded}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<Local> {
        Local.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap()
    }

    #[test]
    fn folder_names_are_sanitised_and_unique_case_insensitively() {
        let mut used = HashSet::new();
        assert_eq!(unique_folder("code-review", &mut used), "code-review");
        assert_eq!(unique_folder("Code-Review", &mut used), "Code-Review-2");
        assert_eq!(unique_folder("a/b:c", &mut used), "a-b-c");
        assert_eq!(unique_folder("  ..  ", &mut used), "skill");
        assert_eq!(
            unique_folder("pdo-skills.json", &mut used),
            "pdo-skills.json-2"
        );
    }

    #[test]
    fn default_name_is_the_skill_for_one_and_dated_otherwise() {
        let now = at(2026, 11, 5, 9, 7);
        assert_eq!(default_file_name(&["tdd"], now), "tdd.skills.zip");
        assert_eq!(
            default_file_name(&["a", "b"], now),
            "pdo-skills-20261105-0907.zip"
        );
    }

    #[test]
    fn requested_names_get_zip_appended_and_lose_their_path() {
        let now = at(2026, 11, 5, 9, 7);
        assert_eq!(
            resolve_file_name(Some("team-skills"), &["a", "b"], now),
            "team-skills.zip"
        );
        assert_eq!(resolve_file_name(Some("team.ZIP"), &["a"], now), "team.ZIP");
        assert_eq!(resolve_file_name(Some("../../etc/x"), &["a"], now), "x.zip");
        assert_eq!(resolve_file_name(Some("   "), &["a"], now), "a.skills.zip");
        assert_eq!(
            resolve_file_name(None, &["a", "b"], now),
            "pdo-skills-20261105-0907.zip"
        );
    }

    #[test]
    fn content_disposition_carries_an_ascii_fallback_and_the_utf8_name() {
        assert_eq!(
            content_disposition("équipe \"x\".zip"),
            "attachment; filename=\"_quipe _x_.zip\"; filename*=UTF-8''%C3%A9quipe%20%22x%22.zip"
        );
    }
}
