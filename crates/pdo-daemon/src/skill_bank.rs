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

// The seeded skills (#722, spec #719, ADR-0064; #588, ADR-0069) — see the `Seed`
// section below. One per node toggle: `orchestrator` seeds `pdo-orchestrate`,
// `interactive` seeds `pdo-interactive`.
pub(crate) const SEEDED_SKILL_ID: &str = "pdo-orchestrate";
pub(crate) const INTERACTIVE_SKILL_ID: &str = "pdo-interactive";
pub(crate) const SEEDED_FOLDER_ID: &str = "skf-pdo";
pub(crate) const SEEDED_FOLDER_NAME: &str = "PDO";

/// Where an imported skill comes from (#670, CONTEXT.md §*Source*): the
/// repository URL (or local folder), the ref that was asked for, the commit the
/// content was read at, and the skill's folder path inside the source. Carried
/// by the skill itself — a skill moved out of its Source folder keeps it — and,
/// with a few counters, by the folder the import created.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Provenance {
    pub url: String,
    #[serde(default, rename = "ref")]
    pub git_ref: Option<String>,
    #[serde(default)]
    pub commit: Option<String>,
    /// `/`-separated path of the skill's folder inside the source (`""` = root).
    #[serde(default)]
    pub path: String,
}

/// A Source folder's provenance: the scan root (`path` is the sub-folder the
/// scan was pointed at), plus what the last import / update saw there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FolderProvenance {
    pub url: String,
    #[serde(default, rename = "ref")]
    pub git_ref: Option<String>,
    #[serde(default)]
    pub commit: Option<String>,
    #[serde(default)]
    pub path: String,
    /// When the folder was last imported or updated from the source.
    pub imported_at: String,
    /// Skills found at the source at that time (valid or not).
    pub found: i64,
    /// Of which not importable (invalid frontmatter).
    pub invalid: i64,
}

/// One row of the bank's index. `folder_id` is `None` at the root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Skill {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub folder_id: Option<String>,
    /// Set on the skill PDO seeds at startup (#722): locked against edit and
    /// delete, everywhere the bank is written.
    #[serde(default)]
    pub locked: bool,
    /// Provenance of an import. `None` for a pasted skill.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<Provenance>,
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
    /// Set on a folder created by an import (a **Source folder**).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<FolderProvenance>,
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
    /// The seeded skill (`pdo-orchestrate`, #722): no edit, no delete, on any
    /// bank surface — it is recreated at every daemon start.
    Locked {
        id: String,
    },
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
            Self::Locked { id } => write!(
                f,
                "`{id}` is seeded and managed by PDO: it is recreated at every daemon start \
                 and locked against editing and deletion"
            ),
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
/// `agent_profile::init`. The seed itself runs at daemon startup ([`seed`]),
/// not here.
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
    // #670: provenance grows from `(source, source_commit)` to a full object on
    // the skill, and lands on the folder too. Additive columns, guarded so a
    // 1.51 database opens unchanged.
    for (table, column, ty) in [
        ("skills", "source_ref", "TEXT"),
        ("skills", "source_path", "TEXT"),
        ("skill_folders", "source", "TEXT"),
        ("skill_folders", "source_ref", "TEXT"),
        ("skill_folders", "source_commit", "TEXT"),
        ("skill_folders", "source_path", "TEXT"),
        ("skill_folders", "source_imported_at", "TEXT"),
        ("skill_folders", "source_found", "INTEGER"),
        ("skill_folders", "source_invalid", "INTEGER"),
    ] {
        add_column_if_missing(db, table, column, ty).await?;
    }
    // Sources the operator has scanned, for the import popup's "Recent sources".
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS skill_sources (
            url          TEXT NOT NULL,
            git_ref      TEXT NOT NULL DEFAULT '',
            path         TEXT NOT NULL DEFAULT '',
            last_used_at TEXT NOT NULL,
            PRIMARY KEY (url, git_ref, path)
        )",
    )
    .execute(db)
    .await?;
    Ok(())
}

async fn add_column_if_missing(
    db: &SqlitePool,
    table: &str,
    column: &str,
    ty: &str,
) -> Result<(), sqlx::Error> {
    let rows = sqlx::query(&format!("PRAGMA table_info({table})"))
        .fetch_all(db)
        .await?;
    let present = rows
        .iter()
        .any(|row| row.get::<String, _>("name") == column);
    if !present {
        sqlx::query(&format!("ALTER TABLE {table} ADD COLUMN {column} {ty}"))
            .execute(db)
            .await?;
    }
    Ok(())
}

fn row_to_skill(row: &sqlx::sqlite::SqliteRow) -> Skill {
    let url: Option<String> = row.get("source");
    Skill {
        id: row.get("id"),
        name: row.get("name"),
        description: row.get("description"),
        folder_id: row.get("folder_id"),
        locked: is_seeded(row.get("id")),
        source: url.map(|url| Provenance {
            url,
            git_ref: row.get("source_ref"),
            commit: row.get("source_commit"),
            path: row
                .get::<Option<String>, _>("source_path")
                .unwrap_or_default(),
        }),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

fn row_to_folder(row: &sqlx::sqlite::SqliteRow) -> SkillFolder {
    let url: Option<String> = row.get("source");
    SkillFolder {
        id: row.get("id"),
        name: row.get("name"),
        parent_id: row.get("parent_id"),
        source: url.map(|url| FolderProvenance {
            url,
            git_ref: row.get("source_ref"),
            commit: row.get("source_commit"),
            path: row
                .get::<Option<String>, _>("source_path")
                .unwrap_or_default(),
            imported_at: row
                .get::<Option<String>, _>("source_imported_at")
                .unwrap_or_default(),
            found: row.get::<Option<i64>, _>("source_found").unwrap_or(0),
            invalid: row.get::<Option<i64>, _>("source_invalid").unwrap_or(0),
        }),
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

/// The `id → name` map of the whole bank, read ONCE per resolution — the
/// snapshot `skill_selection::resolve` names skills from (identity is the id;
/// the stored label is only a fallback for a deleted one). Same posture as
/// `agent_profile::snapshot` (ADR-0057 ¶4).
pub(crate) async fn snapshot_names(
    db: &SqlitePool,
) -> Result<std::collections::BTreeMap<String, String>, sqlx::Error> {
    let rows = sqlx::query("SELECT id, name FROM skills")
        .fetch_all(db)
        .await?;
    Ok(rows
        .iter()
        .map(|row| (row.get::<String, _>("id"), row.get::<String, _>("name")))
        .collect())
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
    refuse_locked(id)?;
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
    refuse_locked(id)?;
    let (rel, path) = file_path(repo_root, id, rel, false)?;
    if !path.is_file() {
        return Err(SkillError::FileNotFound(rel));
    }
    write_file(repo_root, id, &rel, text.as_bytes())
}

/// Delete a reference file, then prune the sub-folders it leaves empty (a
/// `examples/` that held one spec disappears with it; the skill folder stays).
pub(crate) fn delete_file(repo_root: &Path, id: &str, rel: &str) -> Result<(), SkillError> {
    refuse_locked(id)?;
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
    refuse_locked(id)?;
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

/// The seed lock (#722): every write verb on the seeded skill is refused with
/// the same named error, wherever it comes from — the panel, the endpoints, an
/// import. The seed itself writes the disk directly and never passes here.
fn refuse_locked(id: &str) -> Result<(), SkillError> {
    if is_seeded(id) {
        return Err(SkillError::Locked { id: id.to_string() });
    }
    Ok(())
}

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
    create_with_id(db, repo_root, None, content, label, folder_id).await
}

/// A skill id that can name a folder on disk: plain characters only, no path
/// separators, no `.`/`..`. The bank's own ids are UUIDs; an imported document
/// (#673) supplies its own, and this is the one gate before it reaches
/// [`skill_dir`].
pub(crate) fn is_safe_skill_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id != "."
        && id != ".."
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// [`create`] with the id chosen by the caller — the seam the document import
/// uses (#673, ADR-0062 "identité par id"): a skill recreated from a sidecar
/// keeps the id the pipeline references, so the round-trip export → delete →
/// import leaves the pipeline without a warning. `None` generates a fresh one.
/// An id already indexed is a [`SkillError::Storage`] (the importer checks
/// first and never overwrites); an unsafe id is an [`SkillError::InvalidPath`].
pub(crate) async fn create_with_id(
    db: &SqlitePool,
    repo_root: &Path,
    id: Option<&str>,
    content: &str,
    label: Option<&str>,
    folder_id: Option<&str>,
) -> Result<Skill, SkillError> {
    if let Some(id) = id {
        if !is_safe_skill_id(id) {
            return Err(SkillError::InvalidPath(id.to_string()));
        }
        if get(db, id).await?.is_some() {
            return Err(SkillError::Storage(format!(
                "a skill with id `{id}` already exists"
            )));
        }
    }
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
    let id = id.map(str::to_string).unwrap_or_else(generate_skill_id);
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
        locked: false,
        source: None,
        created_at: now.clone(),
        updated_at: now,
    })
}

/// Copy a directory tree (files and sub-directories; symlinks are followed as
/// files, `.git` folders skipped). Used to bring a skill's folder — `SKILL.md`
/// and its reference files, verbatim — from a scanned source into the bank.
pub(crate) fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == ".git" {
            continue;
        }
        let from = entry.path();
        let to = dst.join(&name);
        if from.is_dir() {
            copy_dir_all(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

/// Create a skill from a **folder** (the import path, #670): same gate and same
/// order as [`create`] — validate the `SKILL.md`, check the label, insert the
/// row with its provenance, then copy the whole folder. A refusal leaves no byte
/// on disk; a copy failure rolls the row back.
pub(crate) async fn create_from_dir(
    db: &SqlitePool,
    repo_root: &Path,
    src_dir: &Path,
    label: Option<&str>,
    folder_id: Option<&str>,
    provenance: &Provenance,
) -> Result<Skill, SkillError> {
    let content = std::fs::read_to_string(src_dir.join(SKILL_MD))?;
    let parsed = validate_skill_md(&content)?;
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
        "INSERT INTO skills (id, name, description, folder_id, source, source_ref, source_commit, source_path, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&name)
    .bind(&parsed.description)
    .bind(&folder_id)
    .bind(&provenance.url)
    .bind(&provenance.git_ref)
    .bind(&provenance.commit)
    .bind(&provenance.path)
    .bind(&now)
    .bind(&now)
    .execute(db)
    .await?;

    let dir = skill_dir(repo_root, &id);
    if let Err(error) = copy_dir_all(src_dir, &dir) {
        let _ = std::fs::remove_dir_all(&dir);
        let _ = sqlx::query("DELETE FROM skills WHERE id = ?")
            .bind(&id)
            .execute(db)
            .await;
        return Err(SkillError::Storage(format!(
            "failed to copy {} into {}: {error}",
            src_dir.display(),
            dir.display()
        )));
    }

    Ok(Skill {
        id,
        name,
        description: parsed.description,
        folder_id,
        locked: false,
        source: Some(provenance.clone()),
        created_at: now.clone(),
        updated_at: now,
    })
}

/// Replace a skill's **content** with a folder from a source, keeping its id,
/// label, folder and referents (the *Replace* choice of a collision, and the
/// *updated* rows of an Update from source). The old folder is swapped out only
/// after the new content validated; a failed copy restores it.
pub(crate) async fn replace_content_from_dir(
    db: &SqlitePool,
    repo_root: &Path,
    id: &str,
    src_dir: &Path,
    provenance: &Provenance,
) -> Result<Skill, SkillError> {
    get(db, id).await?.ok_or(SkillError::NotFound)?;
    let content = std::fs::read_to_string(src_dir.join(SKILL_MD))?;
    let parsed = validate_skill_md(&content)?;
    let dir = skill_dir(repo_root, id);
    let backup = skills_root(repo_root).join(format!(".{id}.replacing"));
    let _ = std::fs::remove_dir_all(&backup);
    if dir.exists() {
        std::fs::rename(&dir, &backup)?;
    }
    if let Err(error) = copy_dir_all(src_dir, &dir) {
        let _ = std::fs::remove_dir_all(&dir);
        if backup.exists() {
            let _ = std::fs::rename(&backup, &dir);
        }
        return Err(SkillError::Storage(format!(
            "failed to copy {} into {}: {error}",
            src_dir.display(),
            dir.display()
        )));
    }
    let _ = std::fs::remove_dir_all(&backup);
    let now = crate::event_log::now_iso();
    sqlx::query(
        "UPDATE skills SET description = ?, source = ?, source_ref = ?, source_commit = ?, source_path = ?, updated_at = ? WHERE id = ?",
    )
    .bind(&parsed.description)
    .bind(&provenance.url)
    .bind(&provenance.git_ref)
    .bind(&provenance.commit)
    .bind(&provenance.path)
    .bind(&now)
    .bind(id)
    .execute(db)
    .await?;
    get(db, id).await?.ok_or(SkillError::NotFound)
}

/// Refresh the provenance commit of a skill whose content is identical at a
/// newer commit of its source (an Update from source that found it *unchanged*).
pub(crate) async fn touch_provenance(
    db: &SqlitePool,
    id: &str,
    provenance: &Provenance,
) -> Result<(), SkillError> {
    sqlx::query(
        "UPDATE skills SET source = ?, source_ref = ?, source_commit = ?, source_path = ? WHERE id = ?",
    )
    .bind(&provenance.url)
    .bind(&provenance.git_ref)
    .bind(&provenance.commit)
    .bind(&provenance.path)
    .bind(id)
    .execute(db)
    .await?;
    Ok(())
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
    refuse_locked(id)?;
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
    refuse_locked(id)?;
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
// Seed (#722, spec #719, ADR-0064)
// ---------------------------------------------------------------------------

/// One skill PDO seeds at startup: its bank id (also its name) and the built-in
/// `SKILL.md`. The seed carries a **list** (ADR-0069 §2): adding a toggle skill
/// is one more entry here, nothing else.
pub(crate) struct SeededSkill {
    pub(crate) id: &'static str,
    pub(crate) content: &'static str,
}

pub(crate) const SEEDED_SKILLS: &[SeededSkill] = &[
    SeededSkill {
        id: SEEDED_SKILL_ID,
        content: SEEDED_SKILL_MD,
    },
    SeededSkill {
        id: INTERACTIVE_SKILL_ID,
        content: INTERACTIVE_SKILL_MD,
    },
];

/// Is `id` one of the seeded skills? The one question the lock and the UI ask.
pub(crate) fn is_seeded(id: &str) -> bool {
    SEEDED_SKILLS.iter().any(|skill| skill.id == id)
}

/// The seeded `pdo-orchestrate` `SKILL.md`, versioned with this binary. Bump
/// `skill_version` in the frontmatter whenever the guidance changes: the next
/// start overwrites the copy in every bank, keeping ids and referents intact.
///
/// v3 (#588 / ADR-0069): « Waiting for your children » around `pdo run wait`
/// (no more periodic polling), and the binding contract moved here from the
/// preamble's Orchestration amendment (now the bare `/pdo-orchestrate` line).
pub(crate) const SEEDED_SKILL_MD: &str = r#"---
name: pdo-orchestrate
description: Orchestrate child runs with PDO — create runs from this node session, wait for them with `pdo run wait`, and complete truthfully.
skill_version: 3
---

# Orchestrating with PDO

You are running inside a PDO **node session**. Your identity travels in the
environment: `PDO_RUN_ID` (your run), `PDO_NODE_ID` (your node),
`PDO_NODE_ITER` (your iteration) and `PDO_DAEMON_URL` (the daemon to talk to).
Every run you create from this session is **mechanically linked** to your run
and node — the daemon sets the parent itself; you never declare it.

## Creating a child run

Prefer the CLI (same session, one short command):

```bash
pdo run create <pipeline-name> --input "<what the child must do>"
```

The pipeline is **positional**. The child inherits your run's project by
default; steer it with `--target-repo <path>`, `--variables '{"key":"value"}'`
(one JSON object), `--name "<title>"`, `--skills a,b`. Out of a node session the
same command works, without a parent.

### Passing files and images to the child

Your inputs are files on disk — `PDO_INPUT_<PORT>` names each input port's
path (a file, or a directory for an image list). Hand them to the child with:

- `--input-file <path>`: the child's prompt read from a file (instead of
  `--input`) — e.g. the spec an upstream node wrote.
- `--image <path>` (repeatable): sent as `images`; the child's entry node sees
  them under `## Input Images`.
- `--file <path>` (repeatable): any other file, sent as `files`; listed under
  `## Input Files` and exposed on the entry node's input port.

```bash
pdo run create <pipeline-name> \
  --input-file "$PDO_INPUT_SPEC" \
  --image "$PDO_INPUT_PROTO"/*.png \
  --file "$PDO_INPUT_FIXTURES" \
  --name "Implement the prototype"
```

All attachments land in the child's `.pdo/artifacts/_input/`. One size budget
per run (default 50 MB, `max_attachments_mb` in Settings); a duplicate
filename or an over-budget upload is refused with a named error — rename or
attach less, then retry.

The HTTP surface is equivalent — the CLI is a thin client:

```bash
curl -X POST "$PDO_DAEMON_URL/runs" \
  -H "Content-Type: application/json" \
  -d '{"pipeline": "<pipeline-name>", "input": "<what the child must do>"}'
```

The response carries `{"run_id": "..."}`. A child is an **ordinary run**: its
own graph, its own worktree, its own harness. Recursive orchestration works —
a child may orchestrate in turn.

## Waiting for your children

Never poll on a timer. Block on the daemon instead:

```bash
pdo run wait --timeout 600
```

`pdo run wait` returns as soon as **one** child of this node becomes terminal
(`--all`: once every child is terminal). Its exit code tells you what happened:

- **0** — something settled: one JSON line per child on stdout —
  `{"run_id","name","status","reason","children_active"}`. Read the child's
  outputs (its artifacts, its `reason` when it failed), then decide: create a
  corrected child, move on, or `pdo fail` if the plan is dead.
- **2** — `--timeout` elapsed, nothing settled, nothing on stdout. **Not a
  failure**: call `pdo run wait` again (loop `while ! pdo run wait --timeout 600;
  do :; done` when your harness caps the length of one tool call).
- **0 with `{"noop":true,…}`** — no child of this node is active: you have
  nothing to wait for.
- **1** — the daemon is unreachable, or you are not in a node session.

A child that is itself `awaiting_user` (its agent asked its user a question)
does **not** wake you: PDO shows that wait on your node and your run — « child
run <name> is awaiting you » — and lifts it when the user answers. Keep waiting.

If you need the full picture, `curl -s "$PDO_DAEMON_URL/runs/$PDO_RUN_ID/children"`
lists every child with status, duration and cost — a read, not a wait.

## Completing truthfully

The linkage is strong (ADR-0064): this node completes only once **every** child
run is terminal.

- `pdo complete` is **refused** (`children_pending`, exit 3) while any child is
  still running: wait for them with `pdo run wait`, then complete again.
- When every child is terminal and none failed, the node **completes by itself**
  the moment you give your turn back — you may simply call `pdo complete`.
- A **failed** child parks this node `awaiting_user`: the decision is the
  user's — retry the child, or force the node through (« Mark complete »).
  Say what failed and stop; do not decide for them.

Never declare completion while work you spawned is still in flight; the daemon
enforces it, and the honest move is to wait, follow up, or fail loudly.

## Good practice

- Give each child a precise, self-sufficient input: it cannot see your context.
- Orchestrate wide, not deep, unless the work truly nests.
- Your children outlive your own gestures (stop, archive) — do not use that to
  abandon them; finish what you spawned or leave it in a decided state.
"#;

/// The seeded `pdo-interactive` `SKILL.md` (#588 / ADR-0069): how a node whose
/// `interactive` toggle is on conducts itself — declare the wait, never block
/// the conversation, the release / exit-3 flow.
pub(crate) const INTERACTIVE_SKILL_MD: &str = r#"---
name: pdo-interactive
description: Talk with a human inside a PDO node — declare when you wait for them (`pdo wait-user`), keep the conversation open, complete once released.
skill_version: 1
---

# Talking with a human inside a PDO node

You are running inside a PDO **node session** a human can talk to, in a terminal
they open from the PDO UI. Your identity travels in the environment
(`PDO_RUN_ID`, `PDO_NODE_ID`, `PDO_NODE_ITER`, `PDO_DAEMON_URL`).

PDO does **not** know when you are waiting for them — you have to say it.

## Declare your wait

Every time you stop and need the user (a question, a choice, a review), run:

```bash
pdo wait-user --message "<your question, one line, under 100 characters>"
```

Then ask the question in the conversation as usual. The node and its run turn
**awaiting-user** in the PDO UI, with your message on the banner: that is how
the user, who may be elsewhere, learns it is their turn. Do not skip it — a
node that stays silently « running » while waiting is the failure this skill
exists to prevent.

- The wait **lifts by itself** when the user types an answer in the PDO
  terminal (their Enter), or when they release your completion. You have
  nothing to run afterwards — just continue the conversation.
- Ask again later ⇒ declare again. A repeat with the same message is a no-op;
  a new message refreshes the banner.
- `--message` is optional but always better: a bare `pdo wait-user` shows
  « the agent is waiting for you » with no hint of what about.
- The command is refused (exit 3) only when there is no live session to wait
  in — nothing to do then. Never follow a refusal with `pdo fail`.

## Never block the conversation

`pdo wait-user` returns immediately. Do not loop, sleep, or poll for the
answer; do not run a blocking command « until the user replies ». The user
answers in the same conversation you are in.

## Finishing

Your completion is **guarded**: PDO refuses `pdo complete` on this node until
the user clicks **« Mark ready for completion »** in the PDO UI.

1. When the user says they are done, finish your work and write your outputs.
2. Run `pdo complete`. Two outcomes:
   - **exit 0** — the user had already released you: the node is complete.
   - **exit 3, `completion_not_released`** — not yet released. PDO has just
     declared the wait for you (the banner says you are done and wait for
     their go). Tell the user to click **« Mark ready for completion »**, or
     **« Mark complete »** to take the artifacts as they are. When they say
     they clicked, run `pdo complete` again.
3. **Do NOT run `pdo fail`** after an exit 3: nothing failed, the node is still
   yours.

## Good practice

- One question at a time, in the message and in the conversation.
- Summarise the decision taken before moving on: the user may read the pane
  later, not live.
- If the user goes silent, stay declared and keep the pane readable; do not
  invent their answer.
"#;

/// What one pass of the seed did — logged at startup, asserted in tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SeedOutcome {
    /// The skill was absent: row and folder written.
    Created,
    /// The content on disk differed from the built-in copy: overwritten.
    Updated,
    /// Row and content already matched: nothing written.
    Unchanged,
    /// The seed could not claim its identity (another skill owns the label):
    /// nothing written, the daemon runs without the seed.
    Skipped { reason: String },
}

/// Seed at startup: [`seed_with`] for every entry of [`SEEDED_SKILLS`], in
/// order, into the one « PDO » folder. One outcome per skill — a skipped skill
/// (its label owned by a user skill) never blocks the others.
pub(crate) async fn seed(
    db: &SqlitePool,
    repo_root: &Path,
) -> Result<Vec<(&'static str, SeedOutcome)>, SkillError> {
    let mut outcomes = Vec::with_capacity(SEEDED_SKILLS.len());
    for skill in SEEDED_SKILLS {
        let outcome = seed_with(
            db,
            repo_root,
            skill.id,
            SEEDED_FOLDER_ID,
            SEEDED_FOLDER_NAME,
            skill.content,
        )
        .await?;
        outcomes.push((skill.id, outcome));
    }
    Ok(outcomes)
}

/// The one seed pass, parameterised for tests. Idempotent: `Created` once,
/// then `Unchanged` until the built-in content changes.
pub(crate) async fn seed_with(
    db: &SqlitePool,
    repo_root: &Path,
    id: &str,
    folder_id: &str,
    folder_name: &str,
    content: &str,
) -> Result<SeedOutcome, SkillError> {
    // Our own content passes the same gate a pasted SKILL.md would: the seed
    // never writes a skill the harness would ignore.
    let parsed = validate_skill_md(content)?;
    let now = crate::event_log::now_iso();

    // The « PDO » folder: a fixed id, or an existing root folder already
    // carrying the name (an operator's folder named PDO is adopted, not
    // duplicated — the folder is a UI gesture, nothing references it).
    let folder_pk = match get_folder(db, folder_id).await? {
        Some(_) => folder_id.to_string(),
        None => {
            let adopted: Option<String> = sqlx::query_scalar(
                "SELECT id FROM skill_folders \
                 WHERE parent_id IS NULL AND lower(trim(name)) = lower(trim(?)) LIMIT 1",
            )
            .bind(folder_name)
            .fetch_optional(db)
            .await?;
            match adopted {
                Some(existing) => existing,
                None => {
                    sqlx::query(
                        "INSERT INTO skill_folders (id, name, parent_id, created_at, updated_at) \
                         VALUES (?, ?, NULL, ?, ?)",
                    )
                    .bind(folder_id)
                    .bind(folder_name)
                    .bind(&now)
                    .bind(&now)
                    .execute(db)
                    .await?;
                    folder_id.to_string()
                }
            }
        }
    };

    match get(db, id).await? {
        Some(_) => {
            // Re-seed whenever the disk content is not exactly the built-in
            // copy: a version bump, an erased file, a corrupted folder. The id
            // and the references that select the skill stay put.
            let on_disk = std::fs::read_to_string(skill_dir(repo_root, id).join(SKILL_MD)).ok();
            if on_disk.as_deref() == Some(content) {
                return Ok(SeedOutcome::Unchanged);
            }
            let dir = skill_dir(repo_root, id);
            std::fs::create_dir_all(&dir)?;
            std::fs::write(dir.join(SKILL_MD), content)?;
            sqlx::query(
                "UPDATE skills SET description = ?, folder_id = ?, updated_at = ? WHERE id = ?",
            )
            .bind(&parsed.description)
            .bind(&folder_pk)
            .bind(&now)
            .bind(id)
            .execute(db)
            .await?;
            Ok(SeedOutcome::Updated)
        }
        None => {
            // The label is unique in the bank: if a user skill already carries
            // it, skip rather than fight the index — the operator renames
            // theirs, and the next start seeds.
            if let Some(other) = find_by_name_ci(db, &parsed.name).await? {
                return Ok(SeedOutcome::Skipped {
                    reason: format!(
                        "a skill named `{}` already exists (id `{}`)",
                        other.name, other.id
                    ),
                });
            }
            // Same order as [`create`]: validate, index, then write — a disk
            // failure rolls the row back, leaving no half seed.
            sqlx::query(
                "INSERT INTO skills (id, name, description, folder_id, source, source_commit, created_at, updated_at) \
                 VALUES (?, ?, ?, ?, NULL, NULL, ?, ?)",
            )
            .bind(id)
            .bind(&parsed.name)
            .bind(&parsed.description)
            .bind(&folder_pk)
            .bind(&now)
            .bind(&now)
            .execute(db)
            .await?;

            let dir = skill_dir(repo_root, id);
            let written = std::fs::create_dir_all(&dir)
                .and_then(|_| std::fs::write(dir.join(SKILL_MD), content));
            if let Err(error) = written {
                let _ = sqlx::query("DELETE FROM skills WHERE id = ?")
                    .bind(id)
                    .execute(db)
                    .await;
                return Err(SkillError::Storage(format!(
                    "failed to write {}: {error}",
                    dir.display()
                )));
            }
            Ok(SeedOutcome::Created)
        }
    }
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
        source: None,
        created_at: now.clone(),
        updated_at: now,
    })
}

/// Stamp (or refresh) a folder's Source provenance. Called by the import when it
/// creates the folder, and by every Update from source afterwards.
pub(crate) async fn set_folder_provenance(
    db: &SqlitePool,
    id: &str,
    provenance: &FolderProvenance,
) -> Result<SkillFolder, SkillError> {
    get_folder(db, id)
        .await?
        .ok_or(SkillError::FolderNotFound)?;
    let now = crate::event_log::now_iso();
    sqlx::query(
        "UPDATE skill_folders SET source = ?, source_ref = ?, source_commit = ?, source_path = ?, \
         source_imported_at = ?, source_found = ?, source_invalid = ?, updated_at = ? WHERE id = ?",
    )
    .bind(&provenance.url)
    .bind(&provenance.git_ref)
    .bind(&provenance.commit)
    .bind(&provenance.path)
    .bind(&provenance.imported_at)
    .bind(provenance.found)
    .bind(provenance.invalid)
    .bind(&now)
    .bind(id)
    .execute(db)
    .await?;
    get_folder(db, id).await?.ok_or(SkillError::FolderNotFound)
}

/// Remember a scanned source for the popup's "Recent sources" list.
pub(crate) async fn remember_source(
    db: &SqlitePool,
    url: &str,
    git_ref: Option<&str>,
    path: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO skill_sources (url, git_ref, path, last_used_at) VALUES (?, ?, ?, ?) \
         ON CONFLICT(url, git_ref, path) DO UPDATE SET last_used_at = excluded.last_used_at",
    )
    .bind(url)
    .bind(git_ref.unwrap_or(""))
    .bind(path)
    .bind(crate::event_log::now_iso())
    .execute(db)
    .await?;
    Ok(())
}

/// A remembered source, most recent first, with the Source folder that holds
/// it in the bank (if any).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RecentSource {
    pub url: String,
    #[serde(rename = "ref")]
    pub git_ref: Option<String>,
    pub path: String,
    pub last_used_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder_name: Option<String>,
}

pub(crate) async fn recent_sources(db: &SqlitePool) -> Result<Vec<RecentSource>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT url, git_ref, path, last_used_at FROM skill_sources ORDER BY last_used_at DESC LIMIT 12",
    )
    .fetch_all(db)
    .await?;
    let folders = list_folders(db).await?;
    Ok(rows
        .iter()
        .map(|row| {
            let url: String = row.get("url");
            let git_ref: String = row.get("git_ref");
            let path: String = row.get("path");
            let holder = folders.iter().find(|folder| {
                folder
                    .source
                    .as_ref()
                    .is_some_and(|s| s.url == url && s.path == path)
            });
            RecentSource {
                url,
                git_ref: if git_ref.is_empty() {
                    None
                } else {
                    Some(git_ref)
                },
                path,
                last_used_at: row.get("last_used_at"),
                folder_id: holder.map(|f| f.id.clone()),
                folder_name: holder.map(|f| f.name.clone()),
            }
        })
        .collect())
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

    // -----------------------------------------------------------------------
    // Seed (#722, spec #719, ADR-0064)
    // -----------------------------------------------------------------------

    fn other_skill_md() -> String {
        "---\nname: tdd\ndescription: Test-driven development.\n---\n\n# TDD\n\nRed, green.\n"
            .to_string()
    }

    /// The outcome of one seed pass for `pdo-orchestrate` — the historical
    /// assertions read that one; `pdo-interactive` has its own test.
    async fn seed_orchestrate(db: &SqlitePool, root: &Path) -> SeedOutcome {
        seed(db, root)
            .await
            .unwrap()
            .into_iter()
            .find(|(id, _)| *id == SEEDED_SKILL_ID)
            .map(|(_, outcome)| outcome)
            .expect("pdo-orchestrate is seeded")
    }

    #[tokio::test]
    async fn first_seed_creates_row_folder_and_content() {
        let db = mem_db().await;
        let root = tempfile::tempdir().unwrap();
        assert_eq!(
            seed_orchestrate(&db, root.path()).await,
            SeedOutcome::Created
        );
        let skill = get(&db, SEEDED_SKILL_ID).await.unwrap().unwrap();
        assert_eq!(skill.name, "pdo-orchestrate");
        let folder = get_folder(&db, SEEDED_FOLDER_ID).await.unwrap().unwrap();
        assert_eq!(folder.name, "PDO");
        assert_eq!(skill.folder_id.as_deref(), Some(SEEDED_FOLDER_ID));
        let on_disk =
            std::fs::read_to_string(skill_dir(root.path(), SEEDED_SKILL_ID).join(SKILL_MD))
                .unwrap();
        assert_eq!(on_disk, SEEDED_SKILL_MD);
    }

    #[tokio::test]
    async fn the_seed_carries_a_list_both_toggle_skills_land_in_the_pdo_folder() {
        // #588 / ADR-0069 §2: two seeded skills, one per node toggle, one pass.
        let db = mem_db().await;
        let root = tempfile::tempdir().unwrap();
        let outcomes = seed(&db, root.path()).await.unwrap();
        assert_eq!(
            outcomes,
            vec![
                (SEEDED_SKILL_ID, SeedOutcome::Created),
                (INTERACTIVE_SKILL_ID, SeedOutcome::Created),
            ]
        );
        let interactive = get(&db, INTERACTIVE_SKILL_ID).await.unwrap().unwrap();
        assert_eq!(interactive.name, "pdo-interactive");
        assert_eq!(interactive.folder_id.as_deref(), Some(SEEDED_FOLDER_ID));
        assert!(interactive.locked);
        let on_disk =
            std::fs::read_to_string(skill_dir(root.path(), INTERACTIVE_SKILL_ID).join(SKILL_MD))
                .unwrap();
        assert_eq!(on_disk, INTERACTIVE_SKILL_MD);
        assert!(on_disk.contains("pdo wait-user --message"));
        assert!(on_disk.contains("Mark ready for completion"));
        // Both are locked against every bank write, like the first seed.
        assert!(is_seeded(SEEDED_SKILL_ID) && is_seeded(INTERACTIVE_SKILL_ID));
        assert!(!is_seeded("tdd"));
        assert!(matches!(
            delete(&db, root.path(), INTERACTIVE_SKILL_ID).await,
            Err(SkillError::Locked { .. })
        ));
        // The orchestrate skill's v3 guidance is the blocking pull, no polling.
        assert!(SEEDED_SKILL_MD.contains("pdo run wait"));
        assert!(!SEEDED_SKILL_MD.contains("Check it periodically"));
        assert!(SEEDED_SKILL_MD.contains("skill_version: 3"));
    }

    #[tokio::test]
    async fn a_second_seed_is_a_noop() {
        let db = mem_db().await;
        let root = tempfile::tempdir().unwrap();
        seed(&db, root.path()).await.unwrap();
        assert!(seed(&db, root.path())
            .await
            .unwrap()
            .iter()
            .all(|(_, outcome)| *outcome == SeedOutcome::Unchanged));
    }

    #[tokio::test]
    async fn a_version_bump_rewrites_the_content_keeping_id_and_row() {
        let db = mem_db().await;
        let root = tempfile::tempdir().unwrap();
        seed(&db, root.path()).await.unwrap();
        let before = get(&db, SEEDED_SKILL_ID).await.unwrap().unwrap();

        let bumped = SEEDED_SKILL_MD.replace("skill_version: 3", "skill_version: 4");
        let outcome = seed_with(
            &db,
            root.path(),
            SEEDED_SKILL_ID,
            SEEDED_FOLDER_ID,
            SEEDED_FOLDER_NAME,
            &bumped,
        )
        .await
        .unwrap();
        assert_eq!(outcome, SeedOutcome::Updated);

        let after = get(&db, SEEDED_SKILL_ID).await.unwrap().unwrap();
        assert_eq!(after.id, before.id);
        assert_eq!(after.created_at, before.created_at);
        assert_eq!(after.folder_id.as_deref(), Some(SEEDED_FOLDER_ID));
        assert_ne!(after.updated_at, before.updated_at);
        let on_disk =
            std::fs::read_to_string(skill_dir(root.path(), SEEDED_SKILL_ID).join(SKILL_MD))
                .unwrap();
        assert_eq!(on_disk, bumped);
    }

    #[tokio::test]
    async fn an_erased_or_corrupted_folder_is_reseeded() {
        let db = mem_db().await;
        let root = tempfile::tempdir().unwrap();
        seed(&db, root.path()).await.unwrap();

        // Erased: the whole folder is gone from disk.
        std::fs::remove_dir_all(skill_dir(root.path(), SEEDED_SKILL_ID)).unwrap();
        assert_eq!(
            seed_orchestrate(&db, root.path()).await,
            SeedOutcome::Updated
        );
        let on_disk =
            std::fs::read_to_string(skill_dir(root.path(), SEEDED_SKILL_ID).join(SKILL_MD))
                .unwrap();
        assert_eq!(on_disk, SEEDED_SKILL_MD);

        // Corrupted: the file exists but its content drifted.
        std::fs::write(
            skill_dir(root.path(), SEEDED_SKILL_ID).join(SKILL_MD),
            "corrupted",
        )
        .unwrap();
        assert_eq!(
            seed_orchestrate(&db, root.path()).await,
            SeedOutcome::Updated
        );
        let on_disk =
            std::fs::read_to_string(skill_dir(root.path(), SEEDED_SKILL_ID).join(SKILL_MD))
                .unwrap();
        assert_eq!(on_disk, SEEDED_SKILL_MD);
    }

    #[tokio::test]
    async fn a_user_skill_already_owing_the_label_is_left_alone() {
        let db = mem_db().await;
        let root = tempfile::tempdir().unwrap();
        let user = create(
            &db,
            root.path(),
            &other_skill_md(),
            Some("pdo-orchestrate"),
            None,
        )
        .await
        .unwrap();
        assert!(matches!(
            seed_orchestrate(&db, root.path()).await,
            SeedOutcome::Skipped { .. }
        ));
        assert!(get(&db, SEEDED_SKILL_ID).await.unwrap().is_none());
        // The other seeded skill is not held hostage by the skipped one.
        assert!(get(&db, INTERACTIVE_SKILL_ID).await.unwrap().is_some());
        assert_eq!(
            get(&db, &user.id).await.unwrap().unwrap().name,
            "pdo-orchestrate"
        );
    }

    #[tokio::test]
    async fn an_existing_root_folder_named_pdo_is_adopted_not_duplicated() {
        let db = mem_db().await;
        let root = tempfile::tempdir().unwrap();
        let existing = create_folder(&db, "PDO", None).await.unwrap();
        seed(&db, root.path()).await.unwrap();
        let skill = get(&db, SEEDED_SKILL_ID).await.unwrap().unwrap();
        assert_eq!(skill.folder_id.as_deref(), Some(existing.id.as_str()));
        assert!(get_folder(&db, SEEDED_FOLDER_ID).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn the_seeded_skill_is_locked_against_every_bank_write() {
        let db = mem_db().await;
        let root = tempfile::tempdir().unwrap();
        seed(&db, root.path()).await.unwrap();
        let id = SEEDED_SKILL_ID;

        assert!(matches!(
            delete(&db, root.path(), id).await,
            Err(SkillError::Locked { .. })
        ));
        assert!(matches!(
            update(&db, id, Some("renamed"), None).await,
            Err(SkillError::Locked { .. })
        ));
        assert!(matches!(
            update_skill_md(&db, root.path(), id, &other_skill_md()).await,
            Err(SkillError::Locked { .. })
        ));
        assert!(matches!(
            write_file(root.path(), id, "notes.md", b"x"),
            Err(SkillError::Locked { .. })
        ));
        assert!(matches!(
            overwrite_file(root.path(), id, "notes.md", "x"),
            Err(SkillError::Locked { .. })
        ));
        assert!(matches!(
            delete_file(root.path(), id, "notes.md"),
            Err(SkillError::Locked { .. })
        ));
        // Still there after every refusal.
        assert!(get(&db, id).await.unwrap().is_some());
    }
}
