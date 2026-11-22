//! The snapshot of the Retour automatique: a consistent copy of the daemon's
//! SQLite databases and of the binary in place, taken before an Update.
//!
//! Each database is copied with `VACUUM INTO`, which reads it in one
//! transaction: the copy is consistent even while the daemon (or an extension)
//! writes, WAL included. The databases are the ones named at composition (the
//! core's) and every SQLite file found under the data roots (the extensions'
//! data folder), recognised by their header, not by their name.

use std::path::{Path, PathBuf};

use sqlx::sqlite::SqliteConnectOptions;
use sqlx::ConnectOptions;

use crate::application::ports::{BoxFuture, UpdateSnapshotter};
use crate::domain::update_snapshot::{SavedFile, UpdateSnapshot};

/// The first 16 bytes of every SQLite database file.
const SQLITE_HEADER: &[u8; 16] = b"SQLite format 3\0";

/// How deep the data roots are searched.
const MAX_DEPTH: usize = 6;

pub struct SqliteUpdateSnapshot {
    /// Databases always kept when they exist (the core's).
    databases: Vec<PathBuf>,
    /// Folders searched for more databases (the extensions' data folder).
    roots: Vec<PathBuf>,
}

impl SqliteUpdateSnapshot {
    pub fn new(databases: Vec<PathBuf>, roots: Vec<PathBuf>) -> Self {
        Self { databases, roots }
    }

    /// Every database to keep: the named ones that exist, then the ones under
    /// the roots, each once, in a stable order.
    fn databases(&self) -> Vec<PathBuf> {
        let mut found: Vec<PathBuf> = self
            .databases
            .iter()
            .filter(|p| is_sqlite(p))
            .cloned()
            .collect();
        let mut more = Vec::new();
        for root in &self.roots {
            find_sqlite(root, MAX_DEPTH, &mut more);
        }
        more.sort();
        for path in more {
            if !found.iter().any(|f| same_file(f, &path)) {
                found.push(path);
            }
        }
        found
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

fn is_sqlite(path: &Path) -> bool {
    use std::io::Read;
    let mut header = [0u8; 16];
    std::fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut header))
        .map(|()| &header == SQLITE_HEADER)
        .unwrap_or(false)
}

fn find_sqlite(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if kind.is_dir() && depth > 0 {
            find_sqlite(&path, depth - 1, out);
        } else if kind.is_file() && is_sqlite(&path) {
            out.push(path);
        }
    }
}

/// `VACUUM INTO` the copy, on a connection of its own.
async fn copy_database(original: &Path, saved: &Path) -> Result<(), String> {
    let mut conn = SqliteConnectOptions::new()
        .filename(original)
        .create_if_missing(false)
        .connect()
        .await
        .map_err(|e| format!("cannot open {}: {e}", original.display()))?;
    sqlx::query("VACUUM INTO ?")
        .bind(saved.display().to_string())
        .execute(&mut conn)
        .await
        .map_err(|e| format!("cannot copy {}: {e}", original.display()))?;
    Ok(())
}

/// The name of a database's copy: its index, then its file name.
fn saved_name(index: usize, original: &Path) -> String {
    let name = original
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "db".to_string());
    format!("db-{index}-{name}")
}

impl UpdateSnapshotter for SqliteUpdateSnapshot {
    fn take<'a>(
        &'a self,
        dir: &'a Path,
        exe: &'a Path,
    ) -> BoxFuture<'a, Result<UpdateSnapshot, String>> {
        Box::pin(async move {
            // One snapshot at a time: the previous one goes.
            if dir.exists() {
                std::fs::remove_dir_all(dir)
                    .map_err(|e| format!("cannot clear {}: {e}", dir.display()))?;
            }
            std::fs::create_dir_all(dir)
                .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
            // `copy` follows a symlink: the copy is the binary itself.
            let saved_exe = dir.join("pdo");
            std::fs::copy(exe, &saved_exe)
                .map_err(|e| format!("cannot copy the binary {}: {e}", exe.display()))?;
            let mut databases = Vec::new();
            for (index, original) in self.databases().into_iter().enumerate() {
                let saved = dir.join(saved_name(index, &original));
                copy_database(&original, &saved).await?;
                databases.push(SavedFile { original, saved });
            }
            Ok(UpdateSnapshot {
                dir: dir.to_path_buf(),
                binary: SavedFile {
                    original: exe.to_path_buf(),
                    saved: saved_exe,
                },
                databases,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::Connection;

    async fn database(path: &Path, value: &str) -> sqlx::SqliteConnection {
        let mut conn = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            .connect()
            .await
            .unwrap();
        sqlx::query("CREATE TABLE t (v TEXT)")
            .execute(&mut conn)
            .await
            .unwrap();
        sqlx::query("INSERT INTO t VALUES (?)")
            .bind(value)
            .execute(&mut conn)
            .await
            .unwrap();
        conn
    }

    async fn value(path: &Path) -> String {
        let mut conn = SqliteConnectOptions::new()
            .filename(path)
            .connect()
            .await
            .unwrap();
        let v: (String,) = sqlx::query_as("SELECT v FROM t")
            .fetch_one(&mut conn)
            .await
            .unwrap();
        conn.close().await.unwrap();
        v.0
    }

    #[tokio::test]
    async fn the_snapshot_copies_the_binary_and_every_database_consistently() {
        let tmp = tempfile::tempdir().unwrap();
        let exe = tmp.path().join("bin/pdo");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, "binary v1").unwrap();
        let core = tmp.path().join("repo/.pdo/pdo.db");
        std::fs::create_dir_all(core.parent().unwrap()).unwrap();
        // Kept open in WAL mode: the rows sit in the WAL, not yet in the file.
        let _core = database(&core, "core").await;
        let ext = tmp.path().join("home/.pdo/extensions/acme/data.sqlite");
        std::fs::create_dir_all(ext.parent().unwrap()).unwrap();
        let _ext = database(&ext, "acme").await;
        std::fs::write(ext.with_file_name("notes.db"), "not a database").unwrap();

        let snapshotter = SqliteUpdateSnapshot::new(
            vec![core.clone(), tmp.path().join("missing.db")],
            vec![tmp.path().join("home/.pdo/extensions")],
        );
        let dir = tmp.path().join("home/.pdo/update/snapshot");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("stale"), "previous snapshot").unwrap();
        let snapshot = snapshotter.take(&dir, &exe).await.unwrap();

        assert!(!dir.join("stale").exists(), "one snapshot at a time");
        assert_eq!(snapshot.binary.original, exe);
        assert_eq!(
            std::fs::read_to_string(&snapshot.binary.saved).unwrap(),
            "binary v1"
        );
        let originals: Vec<_> = snapshot.databases.iter().map(|d| &d.original).collect();
        assert_eq!(originals, vec![&core, &ext], "the text file is no database");
        assert_eq!(value(&snapshot.databases[0].saved).await, "core");
        assert_eq!(value(&snapshot.databases[1].saved).await, "acme");
    }

    #[tokio::test]
    async fn a_binary_that_cannot_be_copied_refuses_the_snapshot() {
        let tmp = tempfile::tempdir().unwrap();
        let err = SqliteUpdateSnapshot::new(vec![], vec![])
            .take(&tmp.path().join("snap"), &tmp.path().join("nope"))
            .await
            .unwrap_err();
        assert!(err.starts_with("cannot copy the binary"), "{err}");
    }
}
