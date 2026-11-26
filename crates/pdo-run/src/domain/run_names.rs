//! The names of the Runs and of their Nodes, by Run id, in memory (API 0.37):
//! what an extension's [`Host::run_names`](pdo_extension_api::Host::run_names)
//! reads, synchronously.
//!
//! Fed from the Résumés de Run only (ADR-0084): every Résumé at boot, then the
//! Résumé of a Run each time it starts, is renamed or archived. An archived or
//! a forgotten Run has no entry: the caller shows its id. Nothing here does
//! I/O.

use std::collections::HashMap;
use std::sync::RwLock;

use pdo_extension_api::RunNames;

use crate::domain::run_summary::RunSummary;
use crate::domain::state::RunStatus;

/// Run id → its names.
#[derive(Debug, Default)]
pub struct RunNameBook(RwLock<HashMap<String, RunNames>>);

impl RunNameBook {
    /// The names of `run_id`; `None` for a Run the book does not hold.
    pub fn get(&self, run_id: &str) -> Option<RunNames> {
        self.read().get(run_id).cloned()
    }

    /// Hold the names `summary` gives its Run, or forget the Run once it is
    /// archived.
    pub fn apply(&self, summary: &RunSummary) {
        match names_of(summary) {
            Some(names) => {
                self.write().insert(summary.run_id.clone(), names);
            }
            None => self.remove(&summary.run_id),
        }
    }

    /// Forget `run_id`.
    pub fn remove(&self, run_id: &str) {
        self.write().remove(run_id);
    }

    /// Hold exactly the names of `summaries`, archived Runs left out,
    /// dropping every other one: the loading at boot.
    pub fn replace<'a>(&self, summaries: impl IntoIterator<Item = &'a RunSummary>) {
        *self.write() = summaries
            .into_iter()
            .filter_map(|summary| Some((summary.run_id.clone(), names_of(summary)?)))
            .collect();
    }

    // A panic while holding the lock leaves a table that is still a table:
    // keep serving it.
    fn read(&self) -> std::sync::RwLockReadGuard<'_, HashMap<String, RunNames>> {
        self.0
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn write(&self) -> std::sync::RwLockWriteGuard<'_, HashMap<String, RunNames>> {
        self.0
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// The names of the Run `summary` resumes: its name, else its Pipeline's;
/// `None` once it is archived.
fn names_of(summary: &RunSummary) -> Option<RunNames> {
    if summary.status == RunStatus::Archived {
        return None;
    }
    let name = summary
        .name
        .clone()
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| summary.pipeline_name.clone());
    Some(
        summary
            .node_names
            .iter()
            .fold(RunNames::new(name), |names, (id, name)| {
                names.with_node(id, name)
            }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::event::{Event, EventKind};
    use crate::domain::run_summary::summarize;

    fn summary(run: &str, name: Option<&str>) -> RunSummary {
        let mut payload = serde_json::json!({
            "pipeline_name": "p",
            "node_defs": [
                { "id": "chat", "name": "Assistant", "node_type": "agent", "view_x": null, "view_y": null, "inputs": [], "outputs": [] },
            ],
        });
        if let Some(name) = name {
            payload["name"] = serde_json::json!(name);
        }
        summarize(&[Event {
            id: None,
            run_id: run.into(),
            ts: "2026-11-26T10:00:00Z".into(),
            kind: EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(payload),
        }])
        .unwrap()
    }

    fn archived(run: &str) -> RunSummary {
        RunSummary {
            status: RunStatus::Archived,
            ..summary(run, Some("Vieux"))
        }
    }

    #[test]
    fn a_run_is_named_by_its_name_else_its_pipeline_and_an_archived_one_not_at_all() {
        let book = RunNameBook::default();
        book.replace(&[
            summary("r1", Some("Démo")),
            summary("r2", None),
            archived("r3"),
        ]);
        assert_eq!(
            book.get("r1"),
            Some(RunNames::new("Démo").with_node("chat", "Assistant"))
        );
        assert_eq!(book.get("r2").map(|names| names.name).as_deref(), Some("p"));
        assert_eq!(book.get("r3"), None);
        assert_eq!(book.get("unknown"), None);
    }

    #[test]
    fn an_empty_name_falls_back_to_the_pipeline() {
        let book = RunNameBook::default();
        book.apply(&summary("r1", Some("")));
        assert_eq!(book.get("r1").map(|names| names.name).as_deref(), Some("p"));
    }

    #[test]
    fn a_run_archived_or_removed_has_no_name_any_more() {
        let book = RunNameBook::default();
        book.apply(&summary("r1", Some("Démo")));
        book.apply(&summary("r2", Some("Autre")));
        book.apply(&archived("r1"));
        assert_eq!(book.get("r1"), None);
        book.remove("r2");
        assert_eq!(book.get("r2"), None);
        book.remove("never");
    }

    #[test]
    fn replacing_drops_every_run_not_listed() {
        let book = RunNameBook::default();
        book.apply(&summary("r1", Some("Démo")));
        book.replace(&[summary("r2", None)]);
        assert_eq!(book.get("r1"), None);
        assert!(book.get("r2").is_some());
    }
}
