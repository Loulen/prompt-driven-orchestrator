//! The Projet of each Run, by Run id, in memory (API 0.36): what an
//! extension's [`Host::run_project`](pdo_extension_api::Host::run_project)
//! reads, synchronously, even from its own `authorize`.
//!
//! The Projet of a Run is recorded at launch and never changes (ADR-0087), so
//! the index only grows when a Run starts and shrinks when it is forgotten.
//! The daemon fills it from the Résumés before it serves. A Run launched in no
//! Projet is not held: it answers `None`, as an unknown Run does.

use std::collections::HashMap;
use std::sync::RwLock;

/// Run id → Projet id.
#[derive(Debug, Default)]
pub struct RunProjects(RwLock<HashMap<String, String>>);

impl RunProjects {
    /// The Projet `run_id` was launched in; `None` for a Run the index does
    /// not hold.
    pub fn project_of(&self, run_id: &str) -> Option<String> {
        self.read().get(run_id).cloned()
    }

    /// Hold `project_id` for `run_id`: the Run just started in it.
    pub fn record(&self, run_id: &str, project_id: &str) {
        self.write()
            .insert(run_id.to_string(), project_id.to_string());
    }

    /// Forget `run_id`: a forgotten Run has no Projet any more.
    pub fn forget(&self, run_id: &str) {
        self.write().remove(run_id);
    }

    /// Hold `runs` (Run id, Projet id), the loading at startup, without
    /// replacing what a Run started meanwhile already recorded.
    pub fn fill<I, K, V>(&self, runs: I)
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        let mut held = self.write();
        for (run_id, project_id) in runs {
            held.entry(run_id.into())
                .or_insert_with(|| project_id.into());
        }
    }

    // A panic while holding the lock leaves an index that is still an index:
    // keep serving it.
    fn read(&self) -> std::sync::RwLockReadGuard<'_, HashMap<String, String>> {
        self.0
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn write(&self) -> std::sync::RwLockWriteGuard<'_, HashMap<String, String>> {
        self.0
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_keeps_its_projet_until_it_is_forgotten() {
        let index = RunProjects::default();
        assert_eq!(index.project_of("r1"), None);
        index.record("r1", "p1");
        assert_eq!(index.project_of("r1").as_deref(), Some("p1"));
        assert_eq!(index.project_of("r2"), None);
        index.forget("r1");
        assert_eq!(index.project_of("r1"), None);
    }

    #[test]
    fn filling_never_overwrites_a_run_recorded_meanwhile() {
        let index = RunProjects::default();
        index.record("r1", "live");
        index.fill([("r1", "stale"), ("r2", "p2")]);
        assert_eq!(index.project_of("r1").as_deref(), Some("live"));
        assert_eq!(index.project_of("r2").as_deref(), Some("p2"));
    }

    #[test]
    fn a_poisoned_lock_still_answers() {
        let index = std::sync::Arc::new(RunProjects::default());
        index.record("r1", "p1");
        let held = index.clone();
        let _ = std::thread::spawn(move || {
            let _guard = held.write();
            panic!("poison the lock");
        })
        .join();
        assert_eq!(index.project_of("r1").as_deref(), Some("p1"));
        index.record("r2", "p2");
        assert_eq!(index.project_of("r2").as_deref(), Some("p2"));
    }
}
