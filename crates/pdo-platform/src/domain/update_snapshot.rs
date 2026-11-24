//! The Retour automatique of an Update: what is kept before the update command
//! runs, so the executor can put the instance back when the new version does
//! not answer, and the readiness marker that tells it a daemon answers.
//!
//! The daemon takes the snapshot itself, before it spawns the executor: a
//! consistent copy of its SQLite databases (the core's and those under the
//! extensions' data folder, WAL included) and a copy of the binary in place.
//! The executor only copies files back. One snapshot is kept: the next
//! Update replaces it, a success removes it.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// How long the executor waits, after the restart, for the daemon to answer
/// in another version before it rolls back.
pub const DEFAULT_VERIFY_TIMEOUT_SECS: u64 = 90;

/// A file kept by the snapshot: where it lives, where its copy is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedFile {
    pub original: PathBuf,
    pub saved: PathBuf,
}

/// What the snapshot kept before an Update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateSnapshot {
    /// The snapshot's folder, removed after a success.
    pub dir: PathBuf,
    /// The binary in place (`original` is the stable path the service runs).
    pub binary: SavedFile,
    /// Every SQLite database of the daemon and of its extensions.
    pub databases: Vec<SavedFile>,
}

impl UpdateSnapshot {
    /// One line for the attempt's log: `4 databases, binary 2.88.0 kept`.
    pub fn summary(&self, from_version: &str) -> String {
        let n = self.databases.len();
        let dbs = if n == 1 {
            "1 database".to_string()
        } else {
            format!("{n} databases")
        };
        format!("{dbs}, binary {from_version} kept")
    }
}

/// What a daemon writes once it serves: its version and its pid. The
/// executor reads it to know which version answers after a restart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadyMarker {
    pub version: String,
    pub pid: u32,
}

impl ReadyMarker {
    /// The marker on one line, the form the executor's `sed` reads.
    pub fn to_line(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(n: usize) -> UpdateSnapshot {
        UpdateSnapshot {
            dir: PathBuf::from("/s"),
            binary: SavedFile {
                original: PathBuf::from("/bin/pdo"),
                saved: PathBuf::from("/s/pdo"),
            },
            databases: (0..n)
                .map(|i| SavedFile {
                    original: PathBuf::from(format!("/d/{i}.db")),
                    saved: PathBuf::from(format!("/s/{i}.db")),
                })
                .collect(),
        }
    }

    #[test]
    fn the_summary_counts_the_databases_and_names_the_binary() {
        assert_eq!(
            snapshot(4).summary("2.88.0"),
            "4 databases, binary 2.88.0 kept"
        );
        assert_eq!(
            snapshot(1).summary("1.0.0"),
            "1 database, binary 1.0.0 kept"
        );
    }

    #[test]
    fn the_marker_is_one_line_of_json() {
        let m = ReadyMarker {
            version: "2.88.0".into(),
            pid: 42,
        };
        assert_eq!(m.to_line(), r#"{"version":"2.88.0","pid":42}"#);
    }
}
