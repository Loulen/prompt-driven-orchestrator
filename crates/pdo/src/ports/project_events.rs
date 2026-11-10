//! A Projet change: its name held for the extensions (API 0.11), then the
//! change announced on the raw channel of the kernel's bus, which `/ws` relays
//! to the UI.

use std::sync::Arc;

use pdo_kernel::project_names::ProjectNames;
use pdo_workspace::application::ports::{BoxFuture, ProjectEvents, ProjectRepository};
use tokio::sync::broadcast;
use tracing::warn;

pub(crate) struct BusProjectEvents {
    pub(crate) bus: broadcast::Sender<serde_json::Value>,
    /// The names the extensions' `Host::project_name` reads.
    pub(crate) names: Arc<ProjectNames>,
    /// Where the changed Projet is read again.
    pub(crate) projects: Arc<dyn ProjectRepository>,
    /// One change at a time: a renaming read just before a deletion never
    /// sets its name after the deletion forgot it.
    pub(crate) changing: tokio::sync::Mutex<()>,
}

impl ProjectEvents for BusProjectEvents {
    /// The signal carries the id only: the Projet is read again, so its name
    /// is current before anyone hears of the change. A Projet gone is
    /// forgotten; a store that cannot answer leaves the name held.
    fn project_changed<'a>(&'a self, project_id: &'a str) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            let _one_at_a_time = self.changing.lock().await;
            match self.projects.get(project_id).await {
                Ok(Some(project)) => self.names.set(project_id, &project.name),
                Ok(None) => self.names.forget(project_id),
                Err(e) => warn!("the name of Projet {project_id} cannot be read again: {e}"),
            }
            let _ = self.bus.send(serde_json::json!({
                "type": "project_changed",
                "project_id": project_id,
            }));
        })
    }
}

/// Fill `names` with every Projet of `projects`, before the daemon serves. A
/// store that cannot answer leaves the table empty (logged): the extensions
/// then show the ids.
pub(crate) async fn load_project_names(projects: &dyn ProjectRepository, names: &ProjectNames) {
    match projects.list().await {
        Ok(all) => names.replace(all.into_iter().map(|project| (project.id, project.name))),
        Err(e) => warn!("the names of the Projets cannot be loaded: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdo_workspace::adapters::sqlite::{self, SqliteProjects};

    async fn store() -> Arc<SqliteProjects> {
        let db = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlite::init(&db).await.unwrap();
        Arc::new(SqliteProjects::new(db))
    }

    #[tokio::test]
    async fn a_change_reads_the_projet_again_before_it_is_announced() {
        let projects = store().await;
        let p1 = projects.create("P1").await.unwrap();
        let names = Arc::new(ProjectNames::default());
        load_project_names(projects.as_ref(), &names).await;
        assert_eq!(names.name(&p1.id).as_deref(), Some("P1"));

        let (bus, mut ws) = broadcast::channel(4);
        let events = BusProjectEvents {
            bus,
            names: names.clone(),
            projects: projects.clone(),
            changing: Default::default(),
        };
        projects.rename(&p1.id, "Front").await.unwrap();
        events.project_changed(&p1.id).await;
        assert_eq!(names.name(&p1.id).as_deref(), Some("Front"));
        assert_eq!(
            ws.try_recv().unwrap(),
            serde_json::json!({ "type": "project_changed", "project_id": p1.id })
        );

        projects.delete(&p1.id).await.unwrap();
        events.project_changed(&p1.id).await;
        assert_eq!(names.name(&p1.id), None, "a deleted Projet has no name");
    }
}
