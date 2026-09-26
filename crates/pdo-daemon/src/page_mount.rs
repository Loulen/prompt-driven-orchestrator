use std::ffi::CString;
use std::fs::File;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub(crate) struct PageMount {
    pub name: String,
    pub directory: String,
    pub run_id: Option<String>,
    pub created_at: String,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum MountError {
    #[error("{0}")]
    Invalid(String),
    #[error("page mount `{0}` is already mounted")]
    Duplicate(String),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

pub(crate) async fn init(db: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS page_mounts (
            name       TEXT PRIMARY KEY,
            directory  TEXT NOT NULL,
            run_id     TEXT,
            created_at TEXT NOT NULL
        )",
    )
    .execute(db)
    .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_page_mounts_run_id ON page_mounts(run_id)")
        .execute(db)
        .await?;
    Ok(())
}

pub(crate) async fn create(
    db: &SqlitePool,
    name: &str,
    directory: &Path,
    run_id: Option<&str>,
) -> Result<PageMount, MountError> {
    validate_name(name)?;
    let directory = canonical_mount_directory(directory)?;
    let mount = PageMount {
        name: name.to_string(),
        directory: directory.to_string_lossy().into_owned(),
        run_id: run_id.map(str::to_string),
        created_at: pdo_kernel::clock::now_iso(),
    };

    let result = sqlx::query(
        "INSERT INTO page_mounts (name, directory, run_id, created_at) VALUES (?, ?, ?, ?)",
    )
    .bind(&mount.name)
    .bind(&mount.directory)
    .bind(&mount.run_id)
    .bind(&mount.created_at)
    .execute(db)
    .await;

    match result {
        Ok(_) => Ok(mount),
        Err(error) if is_unique_violation(&error) => Err(MountError::Duplicate(name.to_string())),
        Err(error) => Err(MountError::Database(error)),
    }
}

pub(crate) async fn get(db: &SqlitePool, name: &str) -> Result<Option<PageMount>, sqlx::Error> {
    let row =
        sqlx::query("SELECT name, directory, run_id, created_at FROM page_mounts WHERE name = ?")
            .bind(name)
            .fetch_optional(db)
            .await?;
    row.as_ref().map(row_to_mount).transpose()
}

pub(crate) async fn list(db: &SqlitePool) -> Result<Vec<PageMount>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT name, directory, run_id, created_at FROM page_mounts ORDER BY name ASC",
    )
    .fetch_all(db)
    .await?;
    rows.iter().map(row_to_mount).collect()
}

pub(crate) async fn remove(db: &SqlitePool, name: &str) -> Result<Option<PageMount>, sqlx::Error> {
    let Some(mount) = get(db, name).await? else {
        return Ok(None);
    };
    sqlx::query("DELETE FROM page_mounts WHERE name = ?")
        .bind(name)
        .execute(db)
        .await?;
    Ok(Some(mount))
}

pub(crate) async fn remove_for_run(db: &SqlitePool, run_id: &str) -> Result<u64, sqlx::Error> {
    Ok(sqlx::query("DELETE FROM page_mounts WHERE run_id = ?")
        .bind(run_id)
        .execute(db)
        .await?
        .rows_affected())
}

pub(crate) async fn restore(db: &SqlitePool, mount: &PageMount) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO page_mounts (name, directory, run_id, created_at) VALUES (?, ?, ?, ?)",
    )
    .bind(&mount.name)
    .bind(&mount.directory)
    .bind(&mount.run_id)
    .bind(&mount.created_at)
    .execute(db)
    .await?;
    Ok(())
}

pub(crate) struct OpenedPage {
    pub file: File,
    pub mime_path: PathBuf,
}

pub(crate) fn open_file(mount: &PageMount, requested: &str) -> Option<OpenedPage> {
    let relative = Path::new(requested);
    let mut components = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(name) => components.push(name.to_owned()),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }

    let root = PathBuf::from(&mount.directory);
    let mut directory = open_directory_path(&root)?;

    if components.is_empty() {
        return open_index(&directory);
    }

    for component in &components[..components.len() - 1] {
        directory = open_at(&directory, component, true)?;
    }
    let last = components.last()?;
    let file = open_at(&directory, last, false)?;
    if file.metadata().ok()?.is_dir() {
        return open_index(&file);
    }
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    Some(OpenedPage {
        file,
        mime_path: PathBuf::from(last),
    })
}

fn open_directory_path(path: &Path) -> Option<File> {
    if !path.is_absolute() {
        return None;
    }
    let mut directory = File::open("/").ok()?;
    for component in path.components() {
        match component {
            Component::RootDir | Component::CurDir => {}
            Component::Normal(name) => directory = open_at(&directory, name, true)?,
            Component::ParentDir | Component::Prefix(_) => return None,
        }
    }
    Some(directory)
}

fn open_index(directory: &File) -> Option<OpenedPage> {
    let file = open_at(directory, std::ffi::OsStr::new("index.html"), false)?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    Some(OpenedPage {
        file,
        mime_path: PathBuf::from("index.html"),
    })
}

fn open_at(directory: &File, name: &std::ffi::OsStr, require_directory: bool) -> Option<File> {
    let name = CString::new(name.as_bytes()).ok()?;
    let mut flags = libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    if require_directory {
        flags |= libc::O_DIRECTORY;
    }
    // The returned descriptor is owned on success and immediately wrapped in
    // `File`; every path component is resolved beneath the already-open parent.
    let fd = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), flags) };
    if fd < 0 {
        return None;
    }
    // SAFETY: `openat` returned a fresh owned descriptor and no other owner exists.
    Some(unsafe { File::from_raw_fd(fd) })
}

fn validate_name(name: &str) -> Result<(), MountError> {
    if name.is_empty()
        || name.len() > 64
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_-".contains(&byte))
    {
        return Err(MountError::Invalid(
            "page mount name must match [a-z0-9_-]{1,64}".to_string(),
        ));
    }
    Ok(())
}

fn canonical_mount_directory(directory: &Path) -> Result<PathBuf, MountError> {
    let canonical = directory.canonicalize().map_err(|error| {
        MountError::Invalid(format!("cannot mount `{}`: {error}", directory.display()))
    })?;
    if !canonical.is_dir() {
        return Err(MountError::Invalid(format!(
            "cannot mount `{}`: path is not a directory",
            canonical.display()
        )));
    }
    let components: Vec<_> = canonical.components().collect();
    if components
        .windows(2)
        .any(|pair| pair[0].as_os_str() == ".pdo" && pair[1].as_os_str() == "artifacts")
    {
        return Err(MountError::Invalid(
            "directories under .pdo/artifacts cannot be mounted".to_string(),
        ));
    }
    Ok(canonical)
}

fn is_unique_violation(error: &sqlx::Error) -> bool {
    matches!(
        error,
        sqlx::Error::Database(database_error) if database_error.is_unique_violation()
    )
}

fn row_to_mount(row: &sqlx::sqlite::SqliteRow) -> Result<PageMount, sqlx::Error> {
    Ok(PageMount {
        name: row.try_get("name")?,
        directory: row.try_get("directory")?,
        run_id: row.try_get("run_id")?,
        created_at: row.try_get("created_at")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requested_files_must_remain_inside_the_mount() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("inside.html"), "inside").unwrap();
        let mount = PageMount {
            name: "demo".to_string(),
            directory: root.path().canonicalize().unwrap().display().to_string(),
            run_id: None,
            created_at: "now".to_string(),
        };

        assert!(open_file(&mount, "inside.html").is_some());
        assert!(open_file(&mount, "../../etc/passwd").is_none());
        assert!(open_file(&mount, "missing.html").is_none());
    }
}
