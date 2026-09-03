//! The **Banque de skills** (#668, spec #667, ADR-0062, CONTEXT.md §*Banque de
//! skills*): the instance-scoped store of Skills PDO delivers into every worktree
//! it creates.
//!
//! Two halves, one seam:
//!
//! - **Content on disk**, one folder per skill under `<repo_root>/.pdo/skills/<id>/`
//!   holding the `SKILL.md` and its reference files (#671: uploaded, edited as
//!   plain text, deleted — always inside the skill's folder, never outside it). The folder is keyed
//!   by the **stable id**, never by the name — renaming a skill touches a row, not a
//!   path (#668 AC "renommer ne déplace rien").
//! - **Index in SQLite**: `skills` (id, name unique case-insensitively, description,
//!   folder, provenance, dates) and `skill_folders` (a free hierarchy). The folder
//!   is a UI gesture, not a reference: no tier ever stores a folder id (ADR-0062
//!   "Dossier = geste, pas référence").
//!
//! Identity is the `id`; the `name` is a **label** the bank keeps unique so a
//! selector never shows two indistinguishable rows. Same discipline as
//! `agent_profile` (ADR-0057 ¶2/¶5), and the same reason the uniqueness check lives
//! in code rather than in a bare `UNIQUE` index: the 409 must **name** the clash.
//!
//! [`validate_skill_md`] is the one gate to disk: a `SKILL.md` whose frontmatter the
//! harness would ignore (no `name`, no `description`, empty body) is refused with a
//! named reason and **nothing is written** (#668 AC "rien n'est écrit sur disque").

use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use std::fmt;
use std::path::{Path, PathBuf};

/// The on-disk root of the bank, relative to the daemon's repo root — the same
/// `.pdo/` the SQLite index lives in, so content and index travel together.
pub(crate) fn skills_root(repo_root: &Path) -> PathBuf {
    repo_root.join(".pdo").join("skills")
}

/// Where one skill's folder lives. Keyed by id, never by name.
pub(crate) fn skill_dir(repo_root: &Path, id: &str) -> PathBuf {
    skills_root(repo_root).join(id)
}

pub(crate) const SKILL_MD: &str = "SKILL.md";

/// One row of the bank's index. `folder_id` is `None` at the root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Skill {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub folder_id: Option<String>,
    /// Provenance of an import (URL / local path). `None` for a pasted skill.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_commit: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// A folder of the bank's free hierarchy. `parent_id` is `None` at the root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SkillFolder {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub parent_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// A reference file of a skill (anything under its folder except `SKILL.md`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SkillFile {
    /// Path relative to the skill's folder, `/`-separated.
    pub path: String,
    pub size: u64,
}

/// The parsed, validated content of a `SKILL.md` — what [`validate_skill_md`]
/// returns and what `create` writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedSkillMd {
    pub name: String,
    pub description: String,
    /// The whole frontmatter, for the read-only detail table. A `Mapping` (not a
    /// `BTreeMap`) so the table keeps the author's key order.
    pub frontmatter: serde_yaml::Mapping,
    /// The markdown body after the closing `---`, trimmed.
    pub body: String,
}

/// Why a write was refused. The HTTP layer maps each variant to a status; the
/// message is the reason the popup shows in place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SkillError {
    /// No `---` block at the top of the text.
    NoFrontmatter,
    MalformedFrontmatter(String),
    MissingName,
    /// `name` must be kebab-case: `^[a-z0-9]+(-[a-z0-9]+)*$`.
    NameNotKebabCase(String),
    MissingDescription,
    EmptyBody,
    /// Another skill already carries this label, case-insensitively.
    DuplicateName {
        existing_id: String,
        name: String,
    },
    EmptyLabel,
    NotFound,
    FolderNotFound,
    EmptyFolderName,
    /// Moving a folder under itself or one of its descendants.
    FolderCycle,
    /// A reference-file path that would leave the skill's folder, is absolute,
    /// empty, or otherwise not a plain relative `a/b/c.ext` (#671 AC: 400).
    InvalidPath(String),
    /// `SKILL.md` is the skill's text, not a reference file: it is neither
    /// uploaded nor deleted through the files endpoints (`PUT` it instead).
    SkillMdReserved,
    FileNotFound(String),
    /// One file above [`MAX_FILE_BYTES`].
    FileTooLarge {
        path: String,
        size: u64,
    },
    /// `from_path` (the explorer pick) is not a readable regular file.
    SourceNotAFile(String),
    Storage(String),
}

impl fmt::Display for SkillError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoFrontmatter => write!(
                f,
                "no frontmatter block: a SKILL.md starts with `---`, then `name:` and \
                 `description:`, then `---`"
            ),
            Self::MalformedFrontmatter(reason) => {
                write!(f, "the frontmatter is not valid YAML: {reason}")
            }
            Self::MissingName => write!(f, "the frontmatter has no `name`"),
            Self::NameNotKebabCase(name) => write!(
                f,
                "`name: {name}` is not kebab-case (lowercase letters, digits and single \
                 hyphens, e.g. `code-review`)"
            ),
            Self::MissingDescription => write!(
                f,
                "the frontmatter has no `description`; the harness would ignore this skill"
            ),
            Self::EmptyBody => write!(f, "the body after the frontmatter is empty"),
            Self::DuplicateName { name, .. } => write!(
                f,
                "a skill named `{name}` already exists (names are unique case-insensitively)"
            ),
            Self::EmptyLabel => write!(f, "a skill name cannot be blank"),
            Self::NotFound => write!(f, "no such skill"),
            Self::FolderNotFound => write!(f, "no such skill folder"),
            Self::EmptyFolderName => write!(f, "a folder name cannot be blank"),
            Self::FolderCycle => write!(f, "a folder cannot be moved under itself"),
            Self::InvalidPath(path) => write!(
                f,
                "`{path}` is not a valid file path inside the skill folder (relative, no `..`, \
                 no leading `/`)"
            ),
            Self::SkillMdReserved => write!(
                f,
                "SKILL.md is the skill's text, not a reference file: edit it, do not upload or \
                 delete it"
            ),
            Self::FileNotFound(path) => write!(f, "no file `{path}` in this skill"),
            Self::FileTooLarge { path, size } => write!(
                f,
                "`{path}` is {} — larger than the {} MB limit",
                human_size(*size),
                MAX_FILE_BYTES / (1024 * 1024)
            ),
            Self::SourceNotAFile(path) => {
                write!(
                    f,
                    "`{path}` is not a readable file (drop files, not folders)"
                )
            }
            Self::Storage(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for SkillError {}

impl From<sqlx::Error> for SkillError {
    fn from(error: sqlx::Error) -> Self {
        Self::Storage(error.to_string())
    }
}

impl From<std::io::Error> for SkillError {
    fn from(error: std::io::Error) -> Self {
        Self::Storage(error.to_string())
    }
}

/// Create the two index tables if absent. Idempotent, same idiom as
/// `agent_profile::init`. Nothing is seeded: an untouched instance has an empty
/// bank (FP step 1).
pub(crate) async fn init(db: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS skill_folders (
            id         TEXT PRIMARY KEY,
            name       TEXT NOT NULL,
            parent_id  TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )",
    )
    .execute(db)
    .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS skills (
            id            TEXT PRIMARY KEY,
            name          TEXT NOT NULL,
            description   TEXT NOT NULL,
            folder_id     TEXT,
            source        TEXT,
            source_commit TEXT,
            created_at    TEXT NOT NULL,
            updated_at    TEXT NOT NULL
        )",
    )
    .execute(db)
    .await?;
    sqlx::query(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_skills_name_nocase \
         ON skills(name COLLATE NOCASE)",
    )
    .execute(db)
    .await?;
    Ok(())
}

fn row_to_skill(row: &sqlx::sqlite::SqliteRow) -> Skill {
    Skill {
        id: row.get("id"),
        name: row.get("name"),
        description: row.get("description"),
        folder_id: row.get("folder_id"),
        source: row.get("source"),
        source_commit: row.get("source_commit"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

fn row_to_folder(row: &sqlx::sqlite::SqliteRow) -> SkillFolder {
    SkillFolder {
        id: row.get("id"),
        name: row.get("name"),
        parent_id: row.get("parent_id"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

/// A skill id: a UUID, because the folder on disk carries it and the id travels
/// in pipeline documents across instances (ADR-0062 "voyage par document").
fn generate_skill_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn generate_folder_id() -> String {
    format!("skf-{}", &uuid::Uuid::new_v4().to_string()[..8])
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// `^[a-z0-9]+(-[a-z0-9]+)*$` without a regex dependency.
pub(crate) fn is_kebab_case(name: &str) -> bool {
    if name.is_empty() || name.starts_with('-') || name.ends_with('-') || name.contains("--") {
        return false;
    }
    name.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn scalar_string(value: &serde_yaml::Value) -> Option<String> {
    match value {
        serde_yaml::Value::String(s) => Some(s.clone()),
        serde_yaml::Value::Number(n) => Some(n.to_string()),
        serde_yaml::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// Split `content` into its frontmatter YAML text and its body. `None` when there
/// is no opening `---` at the very top (leading whitespace tolerated).
fn split_frontmatter(content: &str) -> Option<(&str, &str)> {
    let trimmed = content.trim_start();
    let rest = trimmed.strip_prefix("---")?;
    // The opening fence must end its line.
    let rest = rest
        .strip_prefix("\r\n")
        .or_else(|| rest.strip_prefix('\n'))?;
    // Closing fence: a line that is exactly `---`.
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end_matches(['\r', '\n']) == "---" {
            let yaml = &rest[..offset];
            let body = &rest[offset + line.len()..];
            return Some((yaml, body));
        }
        offset += line.len();
    }
    None
}

/// The single gate to disk. Checks, in the order the paste popup lists them:
/// frontmatter block found, `name` present and kebab-case, `description`
/// present, body not empty. Uniqueness is the store's job (`create`).
pub(crate) fn validate_skill_md(content: &str) -> Result<ParsedSkillMd, SkillError> {
    let (yaml, body) = split_frontmatter(content).ok_or(SkillError::NoFrontmatter)?;
    let frontmatter: serde_yaml::Mapping = if yaml.trim().is_empty() {
        serde_yaml::Mapping::new()
    } else {
        serde_yaml::from_str(yaml).map_err(|e| SkillError::MalformedFrontmatter(e.to_string()))?
    };
    let name = frontmatter
        .get(serde_yaml::Value::from("name"))
        .and_then(scalar_string)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or(SkillError::MissingName)?;
    if !is_kebab_case(&name) {
        return Err(SkillError::NameNotKebabCase(name));
    }
    let description = frontmatter
        .get(serde_yaml::Value::from("description"))
        .and_then(scalar_string)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or(SkillError::MissingDescription)?;
    let body = body.trim();
    if body.is_empty() {
        return Err(SkillError::EmptyBody);
    }
    Ok(ParsedSkillMd {
        name,
        description,
        frontmatter,
        body: body.to_string(),
    })
}

// ---------------------------------------------------------------------------
// Skills — reads
// ---------------------------------------------------------------------------

/// All skills, by label then creation order — a stable listing the tree re-sorts
/// by folder client-side.
pub(crate) async fn list(db: &SqlitePool) -> Result<Vec<Skill>, sqlx::Error> {
    let rows = sqlx::query("SELECT * FROM skills ORDER BY name COLLATE NOCASE ASC, created_at ASC")
        .fetch_all(db)
        .await?;
    Ok(rows.iter().map(row_to_skill).collect())
}

pub(crate) async fn get(db: &SqlitePool, id: &str) -> Result<Option<Skill>, sqlx::Error> {
    let row = sqlx::query("SELECT * FROM skills WHERE id = ?")
        .bind(id)
        .fetch_optional(db)
        .await?;
    Ok(row.as_ref().map(row_to_skill))
}

/// Case-insensitive lookup by label, trimmed on both sides.
pub(crate) async fn find_by_name_ci(
    db: &SqlitePool,
    name: &str,
) -> Result<Option<Skill>, sqlx::Error> {
    let row = sqlx::query("SELECT * FROM skills WHERE lower(trim(name)) = lower(trim(?)) LIMIT 1")
        .bind(name)
        .fetch_optional(db)
        .await?;
    Ok(row.as_ref().map(row_to_skill))
}

async fn check_label_unique(
    db: &SqlitePool,
    name: &str,
    excluding_id: Option<&str>,
) -> Result<String, SkillError> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(SkillError::EmptyLabel);
    }
    if let Some(existing) = find_by_name_ci(db, &name).await? {
        if Some(existing.id.as_str()) != excluding_id {
            return Err(SkillError::DuplicateName {
                existing_id: existing.id,
                name: existing.name,
            });
        }
    }
    Ok(name)
}

/// Read one skill's `SKILL.md` from disk.
pub(crate) fn read_skill_md(repo_root: &Path, id: &str) -> Result<String, SkillError> {
    Ok(std::fs::read_to_string(
        skill_dir(repo_root, id).join(SKILL_MD),
    )?)
}

/// List the reference files of a skill: everything under its folder except
/// `SKILL.md`, recursively, sorted by path.
pub(crate) fn list_files(repo_root: &Path, id: &str) -> Result<Vec<SkillFile>, SkillError> {
    let root = skill_dir(repo_root, id);
    let mut out = Vec::new();
    fn walk(base: &Path, dir: &Path, out: &mut Vec<SkillFile>) -> std::io::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            let meta = entry.metadata()?;
            if meta.is_dir() {
                walk(base, &path, out)?;
            } else {
                let rel = path
                    .strip_prefix(base)
                    .unwrap_or(&path)
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/");
                if rel == SKILL_MD {
                    continue;
                }
                out.push(SkillFile {
                    path: rel,
                    size: meta.len(),
                });
            }
        }
        Ok(())
    }
    if root.is_dir() {
        walk(&root, &root, &mut out)?;
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

/// The per-file ceiling of an upload (#671 design: 10 MB). A reference file is
/// a cheatsheet or a fixture the agent reads, not a dataset.
pub(crate) const MAX_FILE_BYTES: u64 = 10 * 1024 * 1024;

fn human_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

/// Normalise a reference-file path the client sent: `/`-separated, relative,
/// made only of plain components (no `.`, no `..`, no empty segment, no
/// backslash, no NUL). This is the **one** gate keeping a write inside the
/// skill's folder (#671 AC "les chemins sortant du dossier du skill sont
/// refusés (400)"); every file endpoint goes through it.
pub(crate) fn normalise_file_path(raw: &str) -> Result<String, SkillError> {
    let trimmed = raw.trim();
    if trimmed.is_empty()
        || trimmed.starts_with('/')
        || trimmed.contains('\\')
        || trimmed.contains('\0')
    {
        return Err(SkillError::InvalidPath(raw.to_string()));
    }
    let mut parts = Vec::new();
    for segment in trimmed.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return Err(SkillError::InvalidPath(raw.to_string()));
        }
        parts.push(segment);
    }
    Ok(parts.join("/"))
}

/// Resolve a normalised relative path under the skill's folder. Refuses
/// `SKILL.md` itself unless `allow_skill_md` (only the text editor writes it).
fn file_path(
    repo_root: &Path,
    id: &str,
    rel: &str,
    allow_skill_md: bool,
) -> Result<(String, PathBuf), SkillError> {
    let rel = normalise_file_path(rel)?;
    if rel == SKILL_MD && !allow_skill_md {
        return Err(SkillError::SkillMdReserved);
    }
    let mut path = skill_dir(repo_root, id);
    for segment in rel.split('/') {
        path.push(segment);
    }
    Ok((rel, path))
}

/// Write (create or replace) a reference file from bytes. The skill folder must
/// exist (the row was indexed by `create`); intermediate sub-folders are made.
pub(crate) fn write_file(
    repo_root: &Path,
    id: &str,
    rel: &str,
    data: &[u8],
) -> Result<SkillFile, SkillError> {
    let (rel, path) = file_path(repo_root, id, rel, false)?;
    if data.len() as u64 > MAX_FILE_BYTES {
        return Err(SkillError::FileTooLarge {
            path: rel,
            size: data.len() as u64,
        });
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, data)?;
    Ok(SkillFile {
        path: rel,
        size: data.len() as u64,
    })
}

/// Copy a file the explorer picked (an absolute path on the daemon's host) into
/// the skill folder, under `rel` (defaults to the source file name).
pub(crate) fn copy_file_from(
    repo_root: &Path,
    id: &str,
    from: &Path,
    rel: Option<&str>,
) -> Result<SkillFile, SkillError> {
    let meta = std::fs::metadata(from)
        .map_err(|_| SkillError::SourceNotAFile(from.display().to_string()))?;
    if !meta.is_file() {
        return Err(SkillError::SourceNotAFile(from.display().to_string()));
    }
    let rel = match rel {
        Some(rel) => rel.to_string(),
        None => from
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .ok_or_else(|| SkillError::SourceNotAFile(from.display().to_string()))?,
    };
    if meta.len() > MAX_FILE_BYTES {
        return Err(SkillError::FileTooLarge {
            path: normalise_file_path(&rel)?,
            size: meta.len(),
        });
    }
    let data = std::fs::read(from)?;
    write_file(repo_root, id, &rel, &data)
}

/// The bytes of one file of the skill (`SKILL.md` included: the editor reads
/// it through the same seam).
pub(crate) fn read_file(
    repo_root: &Path,
    id: &str,
    rel: &str,
) -> Result<(SkillFile, Vec<u8>), SkillError> {
    let (rel, path) = file_path(repo_root, id, rel, true)?;
    if !path.is_file() {
        return Err(SkillError::FileNotFound(rel));
    }
    let data = std::fs::read(&path)?;
    Ok((
        SkillFile {
            path: rel,
            size: data.len() as u64,
        },
        data,
    ))
}

/// Overwrite the text of a reference file (plain-text editor, #671). Not for
/// `SKILL.md`: that one goes through [`update_skill_md`], which re-validates.
pub(crate) fn overwrite_file(
    repo_root: &Path,
    id: &str,
    rel: &str,
    text: &str,
) -> Result<SkillFile, SkillError> {
    let (rel, path) = file_path(repo_root, id, rel, false)?;
    if !path.is_file() {
        return Err(SkillError::FileNotFound(rel));
    }
    write_file(repo_root, id, &rel, text.as_bytes())
}

/// Delete a reference file, then prune the sub-folders it leaves empty (a
/// `examples/` that held one spec disappears with it; the skill folder stays).
pub(crate) fn delete_file(repo_root: &Path, id: &str, rel: &str) -> Result<(), SkillError> {
    let (rel, path) = file_path(repo_root, id, rel, false)?;
    if !path.is_file() {
        return Err(SkillError::FileNotFound(rel));
    }
    std::fs::remove_file(&path)?;
    let root = skill_dir(repo_root, id);
    let mut cursor = path.parent().map(Path::to_path_buf);
    while let Some(dir) = cursor {
        if dir == root || !dir.starts_with(&root) {
            break;
        }
        if std::fs::remove_dir(&dir).is_err() {
            break;
        }
        cursor = dir.parent().map(Path::to_path_buf);
    }
    Ok(())
}

/// Replace the `SKILL.md` of an existing skill (editor save, or a dropped
/// `SKILL.md` — #671 design: "replaces the text, no confirmation, the five
/// checks re-run"). The same gate as `create`: an invalid text is refused with
/// its named reason and **nothing is written**. The row's `description` follows
/// the new frontmatter; the label (`name`) does not — renaming is its own verb.
pub(crate) async fn update_skill_md(
    db: &SqlitePool,
    repo_root: &Path,
    id: &str,
    content: &str,
) -> Result<Skill, SkillError> {
    get(db, id).await?.ok_or(SkillError::NotFound)?;
    let parsed = validate_skill_md(content)?;
    let dir = skill_dir(repo_root, id);
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join(SKILL_MD), content)?;
    let now = crate::event_log::now_iso();
    sqlx::query("UPDATE skills SET description = ?, updated_at = ? WHERE id = ?")
        .bind(&parsed.description)
        .bind(&now)
        .bind(id)
        .execute(db)
        .await?;
    get(db, id).await?.ok_or(SkillError::NotFound)
}

// ---------------------------------------------------------------------------
// Skills — writes
// ---------------------------------------------------------------------------

async fn folder_exists(db: &SqlitePool, id: &str) -> Result<bool, sqlx::Error> {
    Ok(sqlx::query("SELECT 1 FROM skill_folders WHERE id = ?")
        .bind(id)
        .fetch_optional(db)
        .await?
        .is_some())
}

/// Create a skill from pasted `SKILL.md` text. Order matters and is the AC:
/// validate the content, check the label, insert the row, **then** write the
/// folder — so a refusal never leaves a byte on disk, and a disk failure rolls the
/// row back.
///
/// `label` overrides the bank label (defaults to the frontmatter `name`).
pub(crate) async fn create(
    db: &SqlitePool,
    repo_root: &Path,
    content: &str,
    label: Option<&str>,
    folder_id: Option<&str>,
) -> Result<Skill, SkillError> {
    let parsed = validate_skill_md(content)?;
    let name = check_label_unique(db, label.unwrap_or(&parsed.name), None).await?;
    let folder_id = match folder_id.map(str::trim).filter(|s| !s.is_empty()) {
        Some(folder) => {
            if !folder_exists(db, folder).await? {
                return Err(SkillError::FolderNotFound);
            }
            Some(folder.to_string())
        }
        None => None,
    };
    let id = generate_skill_id();
    let now = crate::event_log::now_iso();
    sqlx::query(
        "INSERT INTO skills (id, name, description, folder_id, source, source_commit, created_at, updated_at) \
         VALUES (?, ?, ?, ?, NULL, NULL, ?, ?)",
    )
    .bind(&id)
    .bind(&name)
    .bind(&parsed.description)
    .bind(&folder_id)
    .bind(&now)
    .bind(&now)
    .execute(db)
    .await?;

    let dir = skill_dir(repo_root, &id);
    let written =
        std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(dir.join(SKILL_MD), content));
    if let Err(error) = written {
        let _ = std::fs::remove_dir_all(&dir);
        let _ = sqlx::query("DELETE FROM skills WHERE id = ?")
            .bind(&id)
            .execute(db)
            .await;
        return Err(SkillError::Storage(format!(
            "failed to write {}: {error}",
            dir.display()
        )));
    }

    Ok(Skill {
        id,
        name,
        description: parsed.description,
        folder_id,
        source: None,
        source_commit: None,
        created_at: now.clone(),
        updated_at: now,
    })
}

/// A sparse edit of the index row: rename (label only — the frontmatter `name`
/// and the folder on disk are untouched, the id is the identity) and/or move to
/// a folder (`Some(None)` = back to the root).
pub(crate) async fn update(
    db: &SqlitePool,
    id: &str,
    name: Option<&str>,
    folder_id: Option<Option<&str>>,
) -> Result<Skill, SkillError> {
    let current = get(db, id).await?.ok_or(SkillError::NotFound)?;
    let name = match name {
        Some(candidate) => check_label_unique(db, candidate, Some(id)).await?,
        None => current.name.clone(),
    };
    let folder_id = match folder_id {
        None => current.folder_id.clone(),
        Some(None) => None,
        Some(Some(folder)) => {
            let folder = folder.trim();
            if folder.is_empty() {
                None
            } else {
                if !folder_exists(db, folder).await? {
                    return Err(SkillError::FolderNotFound);
                }
                Some(folder.to_string())
            }
        }
    };
    let now = crate::event_log::now_iso();
    sqlx::query("UPDATE skills SET name = ?, folder_id = ?, updated_at = ? WHERE id = ?")
        .bind(&name)
        .bind(&folder_id)
        .bind(&now)
        .bind(id)
        .execute(db)
        .await?;
    get(db, id).await?.ok_or(SkillError::NotFound)
}

/// Delete a skill: row first, then its folder on disk. **Unconditional** — no
/// referential integrity, the referents dialog informs the confirmation (same
/// posture as `agent_profile::delete`). Returns `false` when the id is unknown.
pub(crate) async fn delete(
    db: &SqlitePool,
    repo_root: &Path,
    id: &str,
) -> Result<bool, SkillError> {
    let res = sqlx::query("DELETE FROM skills WHERE id = ?")
        .bind(id)
        .execute(db)
        .await?;
    if res.rows_affected() == 0 {
        return Ok(false);
    }
    let dir = skill_dir(repo_root, id);
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    Ok(true)
}

// ---------------------------------------------------------------------------
// Folders
// ---------------------------------------------------------------------------

pub(crate) async fn list_folders(db: &SqlitePool) -> Result<Vec<SkillFolder>, sqlx::Error> {
    let rows =
        sqlx::query("SELECT * FROM skill_folders ORDER BY name COLLATE NOCASE ASC, created_at ASC")
            .fetch_all(db)
            .await?;
    Ok(rows.iter().map(row_to_folder).collect())
}

pub(crate) async fn get_folder(
    db: &SqlitePool,
    id: &str,
) -> Result<Option<SkillFolder>, sqlx::Error> {
    let row = sqlx::query("SELECT * FROM skill_folders WHERE id = ?")
        .bind(id)
        .fetch_optional(db)
        .await?;
    Ok(row.as_ref().map(row_to_folder))
}

async fn normalise_parent(
    db: &SqlitePool,
    parent_id: Option<&str>,
) -> Result<Option<String>, SkillError> {
    match parent_id.map(str::trim).filter(|s| !s.is_empty()) {
        Some(parent) => {
            if !folder_exists(db, parent).await? {
                return Err(SkillError::FolderNotFound);
            }
            Ok(Some(parent.to_string()))
        }
        None => Ok(None),
    }
}

/// Create a folder, at the root or under `parent_id`. Folder names are free
/// (only blank is refused): two sibling folders may share a name, the id tells
/// them apart and nothing references a folder.
pub(crate) async fn create_folder(
    db: &SqlitePool,
    name: &str,
    parent_id: Option<&str>,
) -> Result<SkillFolder, SkillError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(SkillError::EmptyFolderName);
    }
    let parent_id = normalise_parent(db, parent_id).await?;
    let id = generate_folder_id();
    let now = crate::event_log::now_iso();
    sqlx::query(
        "INSERT INTO skill_folders (id, name, parent_id, created_at, updated_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(name)
    .bind(&parent_id)
    .bind(&now)
    .bind(&now)
    .execute(db)
    .await?;
    Ok(SkillFolder {
        id,
        name: name.to_string(),
        parent_id,
        created_at: now.clone(),
        updated_at: now,
    })
}

/// Is `candidate` equal to `folder` or one of its descendants? Guards a move
/// against creating a cycle.
async fn is_self_or_descendant(
    db: &SqlitePool,
    folder: &str,
    candidate: &str,
) -> Result<bool, sqlx::Error> {
    let mut cursor = Some(candidate.to_string());
    let mut hops = 0;
    while let Some(current) = cursor {
        if current == folder {
            return Ok(true);
        }
        hops += 1;
        if hops > 1000 {
            return Ok(true);
        }
        cursor = sqlx::query("SELECT parent_id FROM skill_folders WHERE id = ?")
            .bind(&current)
            .fetch_optional(db)
            .await?
            .and_then(|row| row.get::<Option<String>, _>("parent_id"));
    }
    Ok(false)
}

/// Rename and/or re-parent a folder (`Some(None)` = to the root).
pub(crate) async fn update_folder(
    db: &SqlitePool,
    id: &str,
    name: Option<&str>,
    parent_id: Option<Option<&str>>,
) -> Result<SkillFolder, SkillError> {
    let current = get_folder(db, id)
        .await?
        .ok_or(SkillError::FolderNotFound)?;
    let name = match name {
        Some(candidate) => {
            let candidate = candidate.trim();
            if candidate.is_empty() {
                return Err(SkillError::EmptyFolderName);
            }
            candidate.to_string()
        }
        None => current.name.clone(),
    };
    let parent_id = match parent_id {
        None => current.parent_id.clone(),
        Some(parent) => {
            let parent = normalise_parent(db, parent).await?;
            if let Some(parent) = &parent {
                if is_self_or_descendant(db, id, parent).await? {
                    return Err(SkillError::FolderCycle);
                }
            }
            parent
        }
    };
    let now = crate::event_log::now_iso();
    sqlx::query("UPDATE skill_folders SET name = ?, parent_id = ?, updated_at = ? WHERE id = ?")
        .bind(&name)
        .bind(&parent_id)
        .bind(&now)
        .bind(id)
        .execute(db)
        .await?;
    get_folder(db, id).await?.ok_or(SkillError::FolderNotFound)
}

/// Delete a folder. Its skills and sub-folders move to its parent: deleting a
/// folder **never deletes a skill** (design decision of the #668 mock-up).
pub(crate) async fn delete_folder(db: &SqlitePool, id: &str) -> Result<bool, SkillError> {
    let Some(folder) = get_folder(db, id).await? else {
        return Ok(false);
    };
    let now = crate::event_log::now_iso();
    sqlx::query("UPDATE skills SET folder_id = ?, updated_at = ? WHERE folder_id = ?")
        .bind(&folder.parent_id)
        .bind(&now)
        .bind(id)
        .execute(db)
        .await?;
    sqlx::query("UPDATE skill_folders SET parent_id = ?, updated_at = ? WHERE parent_id = ?")
        .bind(&folder.parent_id)
        .bind(&now)
        .bind(id)
        .execute(db)
        .await?;
    sqlx::query("DELETE FROM skill_folders WHERE id = ?")
        .bind(id)
        .execute(db)
        .await?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = "---\nname: tdd\ndescription: Test-driven development.\n---\n\n# TDD\n\nRed, green, refactor.\n";

    async fn mem_db() -> SqlitePool {
        let db = SqlitePool::connect("sqlite::memory:").await.unwrap();
        init(&db).await.unwrap();
        db
    }

    #[test]
    fn kebab_case_rules() {
        assert!(is_kebab_case("tdd"));
        assert!(is_kebab_case("code-review"));
        assert!(is_kebab_case("a1-b2"));
        assert!(!is_kebab_case("TDD"));
        assert!(!is_kebab_case("code_review"));
        assert!(!is_kebab_case("-lead"));
        assert!(!is_kebab_case("trail-"));
        assert!(!is_kebab_case("double--dash"));
        assert!(!is_kebab_case(""));
        assert!(!is_kebab_case("with space"));
    }

    #[test]
    fn validate_accepts_a_complete_skill_md() {
        let parsed = validate_skill_md(VALID).unwrap();
        assert_eq!(parsed.name, "tdd");
        assert_eq!(parsed.description, "Test-driven development.");
        assert!(parsed.body.starts_with("# TDD"));
        assert!(parsed
            .frontmatter
            .contains_key(serde_yaml::Value::from("name")));
    }

    #[test]
    fn validate_refuses_each_missing_piece_with_a_named_reason() {
        assert_eq!(
            validate_skill_md("# no frontmatter\n\nbody").unwrap_err(),
            SkillError::NoFrontmatter
        );
        assert_eq!(
            validate_skill_md("---\ndescription: x\n---\nbody").unwrap_err(),
            SkillError::MissingName
        );
        assert_eq!(
            validate_skill_md("---\nname: TDD\ndescription: x\n---\nbody").unwrap_err(),
            SkillError::NameNotKebabCase("TDD".into())
        );
        assert_eq!(
            validate_skill_md("---\nname: tdd\n---\nbody").unwrap_err(),
            SkillError::MissingDescription
        );
        assert_eq!(
            validate_skill_md("---\nname: tdd\ndescription: x\n---\n\n  \n").unwrap_err(),
            SkillError::EmptyBody
        );
        assert!(matches!(
            validate_skill_md("---\nname: [\n---\nbody").unwrap_err(),
            SkillError::MalformedFrontmatter(_)
        ));
        // An unclosed fence is "no frontmatter block", not a YAML error.
        assert_eq!(
            validate_skill_md("---\nname: tdd\ndescription: x\nbody").unwrap_err(),
            SkillError::NoFrontmatter
        );
    }

    #[tokio::test]
    async fn create_writes_the_folder_keyed_by_id_and_indexes_the_row() {
        let db = mem_db().await;
        let root = tempfile::tempdir().unwrap();
        let skill = create(&db, root.path(), VALID, None, None).await.unwrap();
        assert_eq!(skill.name, "tdd");
        assert_eq!(skill.description, "Test-driven development.");
        assert_eq!(skill.folder_id, None);
        let on_disk = skill_dir(root.path(), &skill.id).join(SKILL_MD);
        assert_eq!(std::fs::read_to_string(on_disk).unwrap(), VALID);
        assert_eq!(list(&db).await.unwrap().len(), 1);
        assert!(list_files(root.path(), &skill.id).unwrap().is_empty());
    }

    #[tokio::test]
    async fn invalid_content_writes_nothing() {
        let db = mem_db().await;
        let root = tempfile::tempdir().unwrap();
        let err = create(&db, root.path(), "---\nname: tdd\n---\nbody", None, None)
            .await
            .unwrap_err();
        assert_eq!(err, SkillError::MissingDescription);
        assert!(list(&db).await.unwrap().is_empty());
        assert!(!skills_root(root.path()).exists());
    }

    #[tokio::test]
    async fn names_are_unique_case_insensitively() {
        let db = mem_db().await;
        let root = tempfile::tempdir().unwrap();
        create(&db, root.path(), VALID, None, None).await.unwrap();
        let err = create(&db, root.path(), VALID, Some("TDD"), None)
            .await
            .unwrap_err();
        assert!(matches!(err, SkillError::DuplicateName { ref name, .. } if name == "tdd"));
        // Only one folder on disk: the refused create wrote nothing.
        assert_eq!(
            std::fs::read_dir(skills_root(root.path())).unwrap().count(),
            1
        );
    }

    #[tokio::test]
    async fn rename_touches_the_label_only() {
        let db = mem_db().await;
        let root = tempfile::tempdir().unwrap();
        let skill = create(&db, root.path(), VALID, None, None).await.unwrap();
        let renamed = update(&db, &skill.id, Some("tdd-strict"), None)
            .await
            .unwrap();
        assert_eq!(renamed.id, skill.id);
        assert_eq!(renamed.name, "tdd-strict");
        // Disk untouched: same folder, same content, frontmatter name still `tdd`.
        let content = read_skill_md(root.path(), &skill.id).unwrap();
        assert_eq!(content, VALID);
        assert_eq!(validate_skill_md(&content).unwrap().name, "tdd");
    }

    #[tokio::test]
    async fn rename_collision_is_refused_and_self_case_change_allowed() {
        let db = mem_db().await;
        let root = tempfile::tempdir().unwrap();
        let a = create(&db, root.path(), VALID, None, None).await.unwrap();
        create(&db, root.path(), VALID, Some("grilling"), None)
            .await
            .unwrap();
        let err = update(&db, &a.id, Some("Grilling"), None)
            .await
            .unwrap_err();
        assert!(matches!(err, SkillError::DuplicateName { .. }));
        let ok = update(&db, &a.id, Some("TDD"), None).await.unwrap();
        assert_eq!(ok.name, "TDD");
    }

    #[tokio::test]
    async fn move_into_a_folder_and_back_to_root() {
        let db = mem_db().await;
        let root = tempfile::tempdir().unwrap();
        let skill = create(&db, root.path(), VALID, None, None).await.unwrap();
        let folder = create_folder(&db, "méthode", None).await.unwrap();
        let moved = update(&db, &skill.id, None, Some(Some(&folder.id)))
            .await
            .unwrap();
        assert_eq!(moved.folder_id.as_deref(), Some(folder.id.as_str()));
        let back = update(&db, &skill.id, None, Some(None)).await.unwrap();
        assert_eq!(back.folder_id, None);
        let err = update(&db, &skill.id, None, Some(Some("skf-nope")))
            .await
            .unwrap_err();
        assert_eq!(err, SkillError::FolderNotFound);
    }

    #[tokio::test]
    async fn delete_removes_row_and_folder() {
        let db = mem_db().await;
        let root = tempfile::tempdir().unwrap();
        let skill = create(&db, root.path(), VALID, None, None).await.unwrap();
        let dir = skill_dir(root.path(), &skill.id);
        assert!(dir.exists());
        assert!(delete(&db, root.path(), &skill.id).await.unwrap());
        assert!(!dir.exists());
        assert!(get(&db, &skill.id).await.unwrap().is_none());
        assert!(!delete(&db, root.path(), &skill.id).await.unwrap());
    }

    #[tokio::test]
    async fn list_files_excludes_skill_md_and_walks_subdirs() {
        let db = mem_db().await;
        let root = tempfile::tempdir().unwrap();
        let skill = create(&db, root.path(), VALID, None, None).await.unwrap();
        let dir = skill_dir(root.path(), &skill.id);
        std::fs::write(dir.join("checklist.md"), "abc").unwrap();
        std::fs::create_dir_all(dir.join("ref")).unwrap();
        std::fs::write(dir.join("ref").join("a.txt"), "12345").unwrap();
        let files = list_files(root.path(), &skill.id).unwrap();
        assert_eq!(
            files,
            vec![
                SkillFile {
                    path: "checklist.md".into(),
                    size: 3
                },
                SkillFile {
                    path: "ref/a.txt".into(),
                    size: 5
                },
            ]
        );
    }

    #[test]
    fn file_paths_leaving_the_skill_folder_are_refused() {
        for bad in [
            "",
            "/etc/passwd",
            "../x",
            "a/../b",
            "a//b",
            "./a",
            "a\\b",
            "a/./b",
        ] {
            assert!(
                matches!(normalise_file_path(bad), Err(SkillError::InvalidPath(_))),
                "{bad:?} should be refused"
            );
        }
        assert_eq!(
            normalise_file_path(" examples/login.spec.ts ").unwrap(),
            "examples/login.spec.ts"
        );
        assert_eq!(normalise_file_path("notes.md").unwrap(), "notes.md");
    }

    #[tokio::test]
    async fn write_read_delete_a_reference_file_and_prune_empty_subfolders() {
        let db = mem_db().await;
        let root = tempfile::tempdir().unwrap();
        let skill = create(&db, root.path(), VALID, None, None).await.unwrap();
        let dir = skill_dir(root.path(), &skill.id);

        let file = write_file(root.path(), &skill.id, "examples/login.spec.ts", b"test()").unwrap();
        assert_eq!(file.path, "examples/login.spec.ts");
        assert_eq!(file.size, 6);
        assert!(dir.join("examples").join("login.spec.ts").is_file());

        let (meta, bytes) = read_file(root.path(), &skill.id, "examples/login.spec.ts").unwrap();
        assert_eq!(meta.size, 6);
        assert_eq!(bytes, b"test()");

        overwrite_file(root.path(), &skill.id, "examples/login.spec.ts", "edited").unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("examples/login.spec.ts")).unwrap(),
            "edited"
        );
        assert!(matches!(
            overwrite_file(root.path(), &skill.id, "missing.md", "x"),
            Err(SkillError::FileNotFound(_))
        ));

        delete_file(root.path(), &skill.id, "examples/login.spec.ts").unwrap();
        assert!(
            !dir.join("examples").exists(),
            "the emptied sub-folder is pruned"
        );
        assert!(
            dir.join(SKILL_MD).is_file(),
            "the skill folder itself stays"
        );
        assert!(matches!(
            delete_file(root.path(), &skill.id, "examples/login.spec.ts"),
            Err(SkillError::FileNotFound(_))
        ));
    }

    #[tokio::test]
    async fn skill_md_is_reserved_for_the_files_endpoints_but_readable() {
        let db = mem_db().await;
        let root = tempfile::tempdir().unwrap();
        let skill = create(&db, root.path(), VALID, None, None).await.unwrap();
        assert!(matches!(
            write_file(root.path(), &skill.id, "SKILL.md", b"x"),
            Err(SkillError::SkillMdReserved)
        ));
        assert!(matches!(
            delete_file(root.path(), &skill.id, "SKILL.md"),
            Err(SkillError::SkillMdReserved)
        ));
        let (_, bytes) = read_file(root.path(), &skill.id, "SKILL.md").unwrap();
        assert_eq!(bytes, VALID.as_bytes());
    }

    #[tokio::test]
    async fn update_skill_md_revalidates_and_writes_nothing_on_refusal() {
        let db = mem_db().await;
        let root = tempfile::tempdir().unwrap();
        let skill = create(&db, root.path(), VALID, None, None).await.unwrap();
        let err = update_skill_md(&db, root.path(), &skill.id, "no frontmatter")
            .await
            .unwrap_err();
        assert_eq!(err, SkillError::NoFrontmatter);
        assert_eq!(read_skill_md(root.path(), &skill.id).unwrap(), VALID);

        let edited = VALID.replace(
            "description: Test-driven development.",
            "description: Edited.",
        );
        assert_ne!(edited, VALID);
        let updated = update_skill_md(&db, root.path(), &skill.id, &edited)
            .await
            .unwrap();
        assert_eq!(updated.description, "Edited.");
        assert_eq!(updated.name, "tdd", "the label is not renamed by an edit");
        assert_eq!(read_skill_md(root.path(), &skill.id).unwrap(), edited);
    }

    #[tokio::test]
    async fn copy_file_from_the_host_refuses_folders_and_oversize() {
        let db = mem_db().await;
        let root = tempfile::tempdir().unwrap();
        let skill = create(&db, root.path(), VALID, None, None).await.unwrap();
        let src = tempfile::tempdir().unwrap();
        std::fs::write(src.path().join("cheatsheet.md"), "# sheet").unwrap();
        let file = copy_file_from(
            root.path(),
            &skill.id,
            &src.path().join("cheatsheet.md"),
            None,
        )
        .unwrap();
        assert_eq!(file.path, "cheatsheet.md");
        assert_eq!(file.size, 7);
        assert!(matches!(
            copy_file_from(root.path(), &skill.id, src.path(), None),
            Err(SkillError::SourceNotAFile(_))
        ));
        assert!(matches!(
            write_file(
                root.path(),
                &skill.id,
                "big.bin",
                &vec![0u8; MAX_FILE_BYTES as usize + 1]
            ),
            Err(SkillError::FileTooLarge { .. })
        ));
    }

    #[tokio::test]
    async fn deleting_a_folder_moves_its_content_to_the_parent() {
        let db = mem_db().await;
        let root = tempfile::tempdir().unwrap();
        let parent = create_folder(&db, "ippon", None).await.unwrap();
        let child = create_folder(&db, "java", Some(&parent.id)).await.unwrap();
        let grandchild = create_folder(&db, "spring", Some(&child.id)).await.unwrap();
        let skill = create(&db, root.path(), VALID, None, Some(&child.id))
            .await
            .unwrap();
        assert!(delete_folder(&db, &child.id).await.unwrap());
        assert_eq!(
            get(&db, &skill.id)
                .await
                .unwrap()
                .unwrap()
                .folder_id
                .as_deref(),
            Some(parent.id.as_str())
        );
        assert_eq!(
            get_folder(&db, &grandchild.id)
                .await
                .unwrap()
                .unwrap()
                .parent_id
                .as_deref(),
            Some(parent.id.as_str())
        );
        assert!(get_folder(&db, &child.id).await.unwrap().is_none());
        // Skill content untouched.
        assert!(skill_dir(root.path(), &skill.id).join(SKILL_MD).exists());
        assert!(!delete_folder(&db, &child.id).await.unwrap());
    }

    #[tokio::test]
    async fn folder_moves_refuse_cycles_and_blank_names() {
        let db = mem_db().await;
        let a = create_folder(&db, "a", None).await.unwrap();
        let b = create_folder(&db, "b", Some(&a.id)).await.unwrap();
        assert_eq!(
            update_folder(&db, &a.id, None, Some(Some(&b.id)))
                .await
                .unwrap_err(),
            SkillError::FolderCycle
        );
        assert_eq!(
            update_folder(&db, &a.id, None, Some(Some(&a.id)))
                .await
                .unwrap_err(),
            SkillError::FolderCycle
        );
        assert_eq!(
            create_folder(&db, "  ", None).await.unwrap_err(),
            SkillError::EmptyFolderName
        );
        let renamed = update_folder(&db, &b.id, Some("bee"), Some(None))
            .await
            .unwrap();
        assert_eq!(renamed.name, "bee");
        assert_eq!(renamed.parent_id, None);
    }
}
