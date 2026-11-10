//! The Artefacts live of the Sessions hébergées (ADR-0089, ADR-0092): the
//! files their agent writes in its `PDO_LIVE_DIR`, under their state folder,
//! listed and read for their extension, and a change published each time the
//! list differs from the last one published.

use std::path::Path;

use pdo_extension_api::{HostedLiveArtifactsChanged, HostedRefusal, LiveArtifact};

use super::Hosted;
use crate::domain::hosted::{live_dir, live_file, HostedRecord};

impl Hosted {
    /// The Artefacts live of the session `key` of `extension`, by path;
    /// `404` for a key it never gave.
    pub fn live_artifacts(
        &self,
        extension: &str,
        key: &str,
    ) -> Result<Vec<LiveArtifact>, HostedRefusal> {
        self.recorded(extension, key)
            .map(|record| self.live_of(&record))
    }

    /// The content of the Artefact live `path` of the session `key` of
    /// `extension`: `404` for a key it never gave or a file that is not
    /// there, `400` for a path that leaves its `PDO_LIVE_DIR`.
    pub fn live_artifact(
        &self,
        extension: &str,
        key: &str,
        path: &str,
    ) -> Result<Vec<u8>, HostedRefusal> {
        let record = self.recorded(extension, key)?;
        let dir = live_dir(&self.registry.store().state_dir(record.id()));
        let file = live_file(&dir, path).ok_or_else(|| {
            HostedRefusal::new(400, format!("`{path}` is not a path under PDO_LIVE_DIR"))
        })?;
        // A link the agent laid there may point anywhere: read only what
        // really lives under the folder.
        let inside = match (dir.canonicalize(), file.canonicalize()) {
            (Ok(dir), Ok(file)) if file.starts_with(&dir) && file.is_file() => Some(file),
            _ => None,
        };
        let Some(file) = inside else {
            return Err(HostedRefusal::new(
                404,
                format!("no Artefact live `{path}` in the hosted session `{key}`"),
            ));
        };
        std::fs::read(&file).map_err(|e| HostedRefusal::new(500, e.to_string()))
    }

    /// Publish the Artefacts live of every recorded session whose list
    /// changed since it was last published. Blocking: it reads the disk.
    pub fn scan_live(&self) {
        for record in self.registry.all() {
            let artifacts = self.live_of(&record);
            let changed = {
                let mut seen = self.live_seen.lock().unwrap_or_else(|p| p.into_inner());
                // Forgotten since the snapshot: nothing more is said of it.
                if self.registry.by_id(record.id()).is_none() {
                    continue;
                }
                let before = seen.get(record.id()).cloned().unwrap_or_default();
                if before == artifacts {
                    false
                } else {
                    seen.insert(record.id().to_string(), artifacts.clone());
                    true
                }
            };
            if changed {
                let event = HostedLiveArtifactsChanged::new(&record.session, artifacts).to_event();
                self.registry.announce(&record, event);
            }
        }
    }

    /// Forget what was published of the session `id`.
    pub(super) fn forget_live(&self, id: &str) {
        self.live_seen
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(id);
    }

    pub(super) fn live_of(&self, record: &HostedRecord) -> Vec<LiveArtifact> {
        let dir = live_dir(&self.registry.store().state_dir(record.id()));
        let mut artifacts = Vec::new();
        collect(&dir, "", &mut artifacts);
        artifacts.sort_by(|a, b| a.path.cmp(&b.path));
        artifacts
    }
}

/// Every file under `dir` (whose path under the live folder is `prefix`),
/// links not followed.
fn collect(dir: &Path, prefix: &str, into: &mut Vec<LiveArtifact>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(meta) = entry.path().symlink_metadata() else {
            continue;
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        if meta.is_dir() {
            collect(&entry.path(), &path, into);
        } else if meta.is_file() {
            let modified_at = meta
                .modified()
                .map(|at| {
                    chrono::DateTime::<chrono::Utc>::from(at)
                        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
                })
                .unwrap_or_default();
            into.push(LiveArtifact::new(path, meta.len(), modified_at));
        }
    }
}
