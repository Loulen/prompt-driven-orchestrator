//! The names of the Projets, by id, in memory (API 0.11): what an extension's
//! [`Host::project_name`](pdo_extension_api::Host::project_name) reads,
//! synchronously, even from its own `authorize`.
//!
//! The daemon fills it from the Projets' store before it serves, then keeps it
//! current at each change of a Projet: created or renamed, the Projet is read
//! again and its name set; deleted, it is forgotten. The table holds names
//! only, never anything a refusal should not say.

use std::collections::HashMap;
use std::sync::RwLock;

/// Projet id → name.
#[derive(Debug, Default)]
pub struct ProjectNames(RwLock<HashMap<String, String>>);

impl ProjectNames {
    /// The name of `project_id`; `None` for an id the table does not hold.
    pub fn name(&self, project_id: &str) -> Option<String> {
        self.read().get(project_id).cloned()
    }

    /// Hold `name` for `project_id`, replacing the one held.
    pub fn set(&self, project_id: &str, name: &str) {
        self.write()
            .insert(project_id.to_string(), name.to_string());
    }

    /// Forget `project_id`: a deleted Projet has no name any more.
    pub fn forget(&self, project_id: &str) {
        self.write().remove(project_id);
    }

    /// Hold exactly `projects` (id, name), dropping every other one: the
    /// loading at startup.
    pub fn replace<I, K, V>(&self, projects: I)
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        *self.write() = projects
            .into_iter()
            .map(|(id, name)| (id.into(), name.into()))
            .collect();
    }

    // A panic while holding the lock leaves a table that is still a table:
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
    fn a_name_follows_its_projet_and_an_unknown_id_has_none() {
        let names = ProjectNames::default();
        assert_eq!(names.name("p1"), None);
        names.replace([("p1", "P1"), ("p2", "P2")]);
        assert_eq!(names.name("p1").as_deref(), Some("P1"));
        names.set("p1", "Front");
        assert_eq!(names.name("p1").as_deref(), Some("Front"));
        names.forget("p2");
        assert_eq!(names.name("p2"), None);
        names.replace([("p3", "P3")]);
        assert_eq!(
            names.name("p1"),
            None,
            "the loading drops what it does not hold"
        );
        assert_eq!(names.name("p3").as_deref(), Some("P3"));
    }
}
