//! The **Archive de skills** (pdo-issues#2, ADR-0084, CONTEXT.md §*Banque de
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

// ---------------------------------------------------------------------------
// Import d'archive — the reader (pdo-issues#2, #1027)
// ---------------------------------------------------------------------------

/// The archive-level ceiling of an uploaded `.zip`: the body limit of the
/// bank's other upload routes (`SKILL_UPLOAD_BODY_LIMIT`, 64 MB).
pub(crate) const MAX_ARCHIVE_BYTES: u64 = 64 * 1024 * 1024;
/// What an archive may expand to on disk, all entries together: a zip bomb is
/// refused before it fills the temp directory.
pub(crate) const MAX_EXTRACTED_BYTES: u64 = 512 * 1024 * 1024;
/// Entries an archive may carry (files and folders).
pub(crate) const MAX_ENTRIES: usize = 20_000;

/// Which of the three layouts the reader recognised (CONTEXT.md §*Import
/// d'archive*). Drives where the ids come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ArchiveFormat {
    /// An Archive de skills: the `pdo-skills.json` manifest maps folder → id.
    PdoArchive,
    /// The `<pipeline>.skills.zip` of a pipeline export: the folder is the id.
    PipelineSidecar,
    /// Any zip of `SKILL.md` folders, without manifest: imported by name.
    PlainZip,
}

/// Why an uploaded file is not read. Every variant names what is wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ArchiveError {
    /// Not a zip at all (no `PK` signature): a PDF, an image…
    NotAZip(String),
    /// A zip signature, but the central directory cannot be read.
    Corrupt(String),
    /// An entry that would land outside the extraction folder (`../`, an
    /// absolute path, a drive letter): the whole archive is refused.
    UnsafeEntry(String),
    /// Over [`MAX_ARCHIVE_BYTES`] (or expanding over [`MAX_EXTRACTED_BYTES`] /
    /// [`MAX_ENTRIES`]).
    TooLarge(String),
    /// A readable zip with no `SKILL.md` anywhere.
    NoSkills(String),
    Io(String),
}

impl std::fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // Shown as is in the import popup: plain sentences, no Markdown.
            Self::NotAZip(file) => write!(f, "{file} is not a .zip."),
            Self::Corrupt(reason) => {
                write!(f, "The archive is corrupted or truncated ({reason}).")
            }
            Self::UnsafeEntry(entry) => write!(
                f,
                "The entry {entry} points outside its folder: archive refused."
            ),
            Self::TooLarge(reason) => write!(f, "{reason}."),
            Self::NoSkills(file) => write!(
                f,
                "{file} holds no SKILL.md: drop a PDO archive, a pipeline's skills sidecar, or a zip of folders holding a SKILL.md."
            ),
            Self::Io(reason) => write!(f, "Failed to read the archive ({reason})."),
        }
    }
}

/// A reference file the reader left out because it is over the bank's
/// per-file limit: its skill fails on its own row, not the whole import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OversizeFile {
    /// `/`-separated path from the archive root.
    pub path: String,
    pub size: u64,
}

/// What an extraction produced, beside the files written under `dest`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Extracted {
    pub format: ArchiveFormat,
    /// Manifest folder → id (PDO archive only).
    manifest_ids: std::collections::HashMap<String, String>,
    pub oversize: Vec<OversizeFile>,
}

impl Extracted {
    /// The id the archive carries for the skill folder at `path` (`/`-separated,
    /// from the archive root), or `None` when it is imported by name.
    pub(crate) fn id_of(&self, path: &str) -> Option<String> {
        let id = match self.format {
            ArchiveFormat::PdoArchive => self.manifest_ids.get(path).cloned(),
            ArchiveFormat::PipelineSidecar => path
                .rsplit('/')
                .next()
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            ArchiveFormat::PlainZip => None,
        }?;
        skill_bank::is_safe_skill_id(&id).then_some(id)
    }

    /// The files over the per-file limit inside the skill folder at `path`.
    pub(crate) fn oversize_in(&self, path: &str) -> Vec<&OversizeFile> {
        self.oversize
            .iter()
            .filter(|file| {
                path.is_empty()
                    || file
                        .path
                        .strip_prefix(path)
                        .is_some_and(|rest| rest.starts_with('/'))
            })
            .collect()
    }
}

/// `team-review.skills.zip` → `team-review`: the name of the folder the
/// created skills land in (`.zip`, then `.skills`, dropped).
pub(crate) fn folder_stem(file_name: &str) -> String {
    let last = file_name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(file_name)
        .trim();
    fn strip_suffix_ci<'a>(s: &'a str, suffix: &str) -> &'a str {
        match s
            .len()
            .checked_sub(suffix.len())
            .and_then(|at| s.get(at..).map(|tail| (at, tail)))
        {
            Some((at, tail)) if tail.eq_ignore_ascii_case(suffix) => &s[..at],
            _ => s,
        }
    }
    let stem = strip_suffix_ci(strip_suffix_ci(last, ".zip"), ".skills").trim();
    if stem.is_empty() {
        "archive".to_string()
    } else {
        stem.to_string()
    }
}

/// `stem`, or `stem 2`, `stem 3`… — the first name no folder of `taken` carries
/// (case-insensitively): the pre-filled « New folder » of an archive import.
pub(crate) fn free_folder_name(stem: &str, taken: &[&str]) -> String {
    let is_taken = |name: &str| taken.iter().any(|t| t.trim().eq_ignore_ascii_case(name));
    if !is_taken(stem) {
        return stem.to_string();
    }
    (2u32..)
        .map(|n| format!("{stem} {n}"))
        .find(|name| !is_taken(name))
        .expect("an unbounded range always yields a free name")
}

/// Split an entry name into safe, plain segments. `None` for an entry to ignore
/// silently (macOS resource forks); an error for one that would escape.
fn entry_segments(name: &str) -> Result<Option<Vec<String>>, ArchiveError> {
    let unsafe_entry = || ArchiveError::UnsafeEntry(name.to_string());
    if name.starts_with('/') || name.starts_with('\\') || name.contains('\0') {
        return Err(unsafe_entry());
    }
    let mut segments = Vec::new();
    for segment in name.split(['/', '\\']) {
        match segment {
            "" | "." => continue,
            ".." => return Err(unsafe_entry()),
            s if segments.is_empty() && s.len() == 2 && s.ends_with(':') => {
                // `C:` — a Windows absolute path.
                return Err(unsafe_entry());
            }
            s => segments.push(s.to_string()),
        }
    }
    if segments.iter().any(|s| s == "__MACOSX" || s == ".DS_Store") {
        return Ok(None);
    }
    Ok(Some(segments))
}

/// Read an uploaded archive into `dest` (created, expected empty): every safe
/// entry is written verbatim; an entry over the per-file limit is **left out**
/// and reported in [`Extracted::oversize`]; an entry escaping the folder refuses
/// the whole archive. The layout is recognised from what was read: the manifest
/// (PDO archive), a `<pipeline>.skills/` folder or UUID folders (pipeline
/// sidecar), or neither (plain zip, by name).
pub(crate) fn extract(
    bytes: &[u8],
    file_name: &str,
    dest: &Path,
) -> Result<Extracted, ArchiveError> {
    use std::io::Read;
    if bytes.len() as u64 > MAX_ARCHIVE_BYTES {
        return Err(ArchiveError::TooLarge(format!(
            "The archive is {} MB, over the {} MB limit",
            bytes.len() as u64 / (1024 * 1024),
            MAX_ARCHIVE_BYTES / (1024 * 1024)
        )));
    }
    if !bytes.starts_with(b"PK") {
        return Err(ArchiveError::NotAZip(file_name.to_string()));
    }
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| ArchiveError::Corrupt(e.to_string()))?;
    if archive.len() > MAX_ENTRIES {
        return Err(ArchiveError::TooLarge(format!(
            "The archive holds {} entries, over the {MAX_ENTRIES} limit",
            archive.len()
        )));
    }
    let mut oversize = Vec::new();
    let mut manifest_bytes = None;
    let mut skill_dirs: Vec<Vec<String>> = Vec::new();
    let mut total: u64 = 0;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|e| ArchiveError::Corrupt(e.to_string()))?;
        let name = entry.name().to_string();
        let Some(segments) = entry_segments(&name)? else {
            continue;
        };
        if segments.is_empty() || entry.is_dir() {
            continue;
        }
        // A symlink entry is skipped: the bank stores plain files only.
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            continue;
        }
        let rel = segments.join("/");
        // A skill folder is a skill even when its own SKILL.md is oversized: it
        // is listed and fails on its row, never vanishes from the list.
        if segments.last().map(String::as_str) == Some(SKILL_MD) {
            skill_dirs.push(segments[..segments.len() - 1].to_vec());
        }
        if entry.size() > skill_bank::MAX_FILE_BYTES {
            oversize.push(OversizeFile {
                path: rel,
                size: entry.size(),
            });
            continue;
        }
        // Never trust the declared size: read one byte past the limit.
        let mut data = Vec::new();
        (&mut entry)
            .take(skill_bank::MAX_FILE_BYTES + 1)
            .read_to_end(&mut data)
            .map_err(|e| ArchiveError::Corrupt(format!("{name}: {e}")))?;
        if data.len() as u64 > skill_bank::MAX_FILE_BYTES {
            oversize.push(OversizeFile {
                path: rel,
                size: data.len() as u64,
            });
            continue;
        }
        total += data.len() as u64;
        if total > MAX_EXTRACTED_BYTES {
            return Err(ArchiveError::TooLarge(format!(
                "The archive expands over {} MB",
                MAX_EXTRACTED_BYTES / (1024 * 1024)
            )));
        }
        if segments.len() == 1 && segments[0] == MANIFEST {
            manifest_bytes = Some(data.clone());
        }
        let mut path = dest.to_path_buf();
        for segment in &segments {
            path.push(segment);
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| ArchiveError::Corrupt(format!("{name}: {e}")))?;
        }
        std::fs::write(&path, &data).map_err(|e| ArchiveError::Io(format!("{name}: {e}")))?;
    }
    if skill_dirs.is_empty() {
        return Err(ArchiveError::NoSkills(file_name.to_string()));
    }

    let manifest = manifest_bytes
        .and_then(|bytes| serde_json::from_slice::<Manifest>(&bytes).ok())
        .filter(|manifest| manifest.format == FORMAT);
    if let Some(manifest) = manifest {
        let manifest_ids = manifest
            .skills
            .into_iter()
            .filter(|entry| skill_bank::is_safe_skill_id(&entry.id))
            .filter_map(|entry| {
                let folder = entry_segments(&entry.folder).ok().flatten()?.join("/");
                (!folder.is_empty()).then_some((folder, entry.id))
            })
            .collect();
        return Ok(Extracted {
            format: ArchiveFormat::PdoArchive,
            manifest_ids,
            oversize,
        });
    }
    // A sidecar's folders are skill ids: the bank's UUIDs, or a seeded id. A
    // hand-made `my.skills/tdd/` stays a plain zip, imported by name.
    let is_id = |name: &str| uuid::Uuid::parse_str(name).is_ok() || skill_bank::is_seeded(name);
    let under_sidecar = |dir: &Vec<String>| {
        dir.len() >= 2
            && dir[dir.len() - 2].ends_with(crate::skill_sidecar::SIDECAR_SUFFIX)
            && is_id(&dir[dir.len() - 1])
    };
    let uuid_folder = |dir: &Vec<String>| dir.len() == 1 && is_id(&dir[0]);
    let format = if skill_dirs
        .iter()
        .all(|dir| under_sidecar(dir) || uuid_folder(dir))
    {
        ArchiveFormat::PipelineSidecar
    } else {
        ArchiveFormat::PlainZip
    };
    Ok(Extracted {
        format,
        manifest_ids: Default::default(),
        oversize,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        for (name, data) in entries {
            zip.start_file(*name, options).unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    const MD: &[u8] = b"---\nname: a\ndescription: d\n---\n\nbody\n";

    #[test]
    fn the_three_layouts_are_recognised_and_carry_their_ids() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = br#"{"format":"pdo-skills","version":1,"exported_at":"x","skills":[{"folder":"a","id":"id-a","name":"a"}]}"#;
        let pdo = zip_of(&[
            ("a/SKILL.md", MD),
            ("b/SKILL.md", MD),
            ("pdo-skills.json", manifest),
        ]);
        let read = extract(&pdo, "x.zip", dir.path()).unwrap();
        assert_eq!(read.format, ArchiveFormat::PdoArchive);
        assert_eq!(read.id_of("a").as_deref(), Some("id-a"));
        assert_eq!(
            read.id_of("b"),
            None,
            "a folder the manifest omits goes by name"
        );

        let dir = tempfile::tempdir().unwrap();
        let id = "0b7c5f0e-8a2b-4d0e-9a51-4f1d2c3b4a59";
        let sidecar = zip_of(&[
            (&format!("p.skills/{id}/SKILL.md"), MD),
            (&format!("p.skills/{id}/ref.md"), b"r"),
            ("p.skills/pdo-interactive/SKILL.md", MD),
        ]);
        let read = extract(&sidecar, "p.skills.zip", dir.path()).unwrap();
        assert_eq!(read.format, ArchiveFormat::PipelineSidecar);
        assert_eq!(read.id_of(&format!("p.skills/{id}")).as_deref(), Some(id));
        assert_eq!(
            read.id_of("p.skills/pdo-interactive").as_deref(),
            Some("pdo-interactive")
        );
        assert!(dir.path().join(format!("p.skills/{id}/ref.md")).is_file());

        // A hand-made folder that merely ends in `.skills` is not a sidecar.
        let dir = tempfile::tempdir().unwrap();
        let named = zip_of(&[("my.skills/tdd/SKILL.md", MD)]);
        let read = extract(&named, "my.zip", dir.path()).unwrap();
        assert_eq!(read.format, ArchiveFormat::PlainZip);
        assert_eq!(read.id_of("my.skills/tdd"), None);

        let dir = tempfile::tempdir().unwrap();
        let id = "0b7c5f0e-8a2b-4d0e-9a51-4f1d2c3b4a59";
        let rezipped = zip_of(&[(&format!("{id}/SKILL.md"), MD)]);
        let read = extract(&rezipped, "s.zip", dir.path()).unwrap();
        assert_eq!(read.format, ArchiveFormat::PipelineSidecar);
        assert_eq!(read.id_of(id).as_deref(), Some(id));

        let dir = tempfile::tempdir().unwrap();
        let plain = zip_of(&[("tdd/SKILL.md", MD), ("__MACOSX/tdd/._SKILL.md", b"junk")]);
        let read = extract(&plain, "s.zip", dir.path()).unwrap();
        assert_eq!(read.format, ArchiveFormat::PlainZip);
        assert_eq!(read.id_of("tdd"), None);
        assert!(!dir.path().join("__MACOSX").exists());
    }

    #[test]
    fn garbage_traversal_and_empty_archives_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            extract(b"%PDF-1.7", "notes.pdf", dir.path()).unwrap_err(),
            ArchiveError::NotAZip("notes.pdf".into())
        );
        assert!(matches!(
            extract(b"PK\x03\x04truncated", "x.zip", dir.path()).unwrap_err(),
            ArchiveError::Corrupt(_)
        ));
        let evil = zip_of(&[("a/SKILL.md", MD), ("../../evil.sh", b"x")]);
        assert_eq!(
            extract(&evil, "x.zip", dir.path()).unwrap_err(),
            ArchiveError::UnsafeEntry("../../evil.sh".into())
        );
        let empty = zip_of(&[("readme.txt", b"hi")]);
        assert_eq!(
            extract(&empty, "x.zip", dir.path()).unwrap_err(),
            ArchiveError::NoSkills("x.zip".into())
        );
    }

    #[test]
    fn an_archive_over_the_upload_limit_is_refused_before_reading() {
        let dir = tempfile::tempdir().unwrap();
        let mut bytes = vec![0u8; (MAX_ARCHIVE_BYTES + 1) as usize];
        bytes[..2].copy_from_slice(b"PK");
        assert!(matches!(
            extract(&bytes, "huge.zip", dir.path()).unwrap_err(),
            ArchiveError::TooLarge(_)
        ));
    }

    #[test]
    fn a_file_over_the_per_file_limit_is_left_out_and_reported() {
        let dir = tempfile::tempdir().unwrap();
        let big = vec![0u8; (skill_bank::MAX_FILE_BYTES + 1) as usize];
        let bytes = zip_of(&[("a/SKILL.md", MD), ("a/data.bin", &big), ("b/SKILL.md", MD)]);
        let read = extract(&bytes, "x.zip", dir.path()).unwrap();
        assert_eq!(read.oversize.len(), 1);
        assert_eq!(read.oversize_in("a").len(), 1);
        assert!(read.oversize_in("b").is_empty());
        assert!(!dir.path().join("a/data.bin").exists());

        // An oversized SKILL.md still lists its skill, which then fails its row.
        let dir = tempfile::tempdir().unwrap();
        let bytes = zip_of(&[("c/SKILL.md", &big)]);
        let read = extract(&bytes, "x.zip", dir.path()).unwrap();
        assert_eq!(read.oversize_in("c").len(), 1);
    }

    #[test]
    fn folder_names_follow_the_file_and_get_a_free_suffix() {
        assert_eq!(folder_stem("team-review.skills.zip"), "team-review");
        assert_eq!(folder_stem("Team.ZIP"), "Team");
        assert_eq!(folder_stem(".zip"), "archive");
        assert_eq!(free_folder_name("team", &["ops"]), "team");
        assert_eq!(free_folder_name("team", &["Team", "team 2"]), "team 3");
    }

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
