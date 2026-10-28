//! The records of the Sessions hébergées on disk: one folder per session
//! under the daemon's `~/.pdo/hosted/`, holding `session.json` and the
//! session's state (primer, hooks, Artefacts live).

use std::path::PathBuf;

use tracing::warn;

use crate::application::hosted::HostedStore;
use crate::domain::hosted::HostedRecord;

/// The file of a session's record, in its folder.
const RECORD: &str = "session.json";

/// The Sessions hébergées under `root` (`~/.pdo/hosted`).
pub struct FsHostedStore {
    root: PathBuf,
}

impl FsHostedStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

impl HostedStore for FsHostedStore {
    /// Every record that reads, oldest first; one that does not read is
    /// skipped with a warning, never fatal to the daemon.
    fn load(&self) -> Vec<HostedRecord> {
        let Ok(entries) = std::fs::read_dir(&self.root) else {
            return Vec::new();
        };
        let mut records: Vec<HostedRecord> = entries
            .flatten()
            .map(|entry| entry.path().join(RECORD))
            .filter(|file| file.is_file())
            .filter_map(|file| {
                let read = std::fs::read_to_string(&file).map_err(|e| e.to_string());
                match read.and_then(|text| serde_json::from_str(&text).map_err(|e| e.to_string())) {
                    Ok(record) => Some(record),
                    Err(e) => {
                        warn!("hosted session record {} skipped: {e}", file.display());
                        None
                    }
                }
            })
            .collect();
        records
            .sort_by(|a, b| (&a.session.created_at, a.id()).cmp(&(&b.session.created_at, b.id())));
        records
    }

    /// Written to a temporary file then renamed: a crash never leaves half a
    /// record.
    fn save(&self, record: &HostedRecord) -> Result<(), String> {
        let dir = self.state_dir(record.id());
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let text = serde_json::to_string_pretty(record).map_err(|e| e.to_string())?;
        let partial = dir.join(format!("{RECORD}.partial"));
        std::fs::write(&partial, text).map_err(|e| e.to_string())?;
        std::fs::rename(&partial, dir.join(RECORD)).map_err(|e| e.to_string())
    }

    fn remove(&self, id: &str) {
        let dir = self.state_dir(id);
        if let Err(e) = std::fs::remove_dir_all(&dir) {
            if e.kind() != std::io::ErrorKind::NotFound {
                warn!("the state of hosted session {id} could not be removed: {e}");
            }
        }
    }

    fn state_dir(&self, id: &str) -> PathBuf {
        self.root.join(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdo_extension_api::OpenSession;
    use std::path::Path;

    fn record(id: &str, key: &str, at: &str) -> HostedRecord {
        HostedRecord::opening(id, "test", &OpenSession::shell(key), Path::new("/h"), at)
    }

    #[test]
    fn records_round_trip_in_their_order_and_are_forgotten_with_their_state() {
        let root = tempfile::tempdir().unwrap();
        let store = FsHostedStore::new(root.path().join("hosted"));
        assert!(store.load().is_empty(), "no folder yet: no record");
        let second = record("pdo-hosted-b", "two", "2026-10-28T10:00:01Z");
        let first = record("pdo-hosted-a", "one", "2026-10-28T10:00:00Z");
        store.save(&second).unwrap();
        store.save(&first).unwrap();
        let mut changed = first.clone();
        changed.run("2026-10-28T10:00:02Z");
        store.save(&changed).unwrap();
        assert_eq!(store.load(), vec![changed, second.clone()]);

        std::fs::write(store.state_dir("pdo-hosted-b").join("primer.md"), "x").unwrap();
        std::fs::create_dir_all(root.path().join("hosted/broken")).unwrap();
        std::fs::write(root.path().join("hosted/broken/session.json"), "{").unwrap();
        store.remove("pdo-hosted-b");
        assert!(!store.state_dir("pdo-hosted-b").exists());
        assert_eq!(
            store.load().len(),
            1,
            "a record that does not read is skipped"
        );
        store.remove("pdo-hosted-never");
    }
}
