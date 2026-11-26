//! Append one event to a Run's log, then announce it on the bus.
//!
//! A transition of the Run, never an actor's gesture: the daemon, the
//! scheduler and the agents' completions reach it once the use case that
//! decided the transition has asked whatever it had to ask. It does not ask
//! the Autorisation d'opération (D10).

use std::sync::Arc;

use pdo_kernel::event_bus::EventBus;
use tracing::{info, warn};

use crate::application::ports::EventStore;
use crate::domain::event::{Event, EventKind};
use crate::domain::projection::project;
use crate::domain::run_names::RunNameBook;
use crate::domain::run_projects::RunProjects;
use crate::domain::transition_guard::{validate_transition, Verdict};

pub struct AppendEvent {
    store: Arc<dyn EventStore>,
    bus: Arc<dyn EventBus>,
    run_names: Arc<RunNameBook>,
    /// Held from the read of a Résumé to the update of the names: of two
    /// concurrent appends on a Run, the last to refresh reads the Résumé of
    /// both, so a stale name never overwrites a fresher one.
    names_refresh: tokio::sync::Mutex<()>,
    /// The Projet of each Run (API 0.36): a `RunStarted` records its own.
    run_projects: Arc<RunProjects>,
}

impl AppendEvent {
    pub fn new(
        store: Arc<dyn EventStore>,
        bus: Arc<dyn EventBus>,
        run_names: Arc<RunNameBook>,
        run_projects: Arc<RunProjects>,
    ) -> Self {
        Self {
            store,
            bus,
            run_names,
            names_refresh: tokio::sync::Mutex::new(()),
            run_projects,
        }
    }

    /// Append `event` and publish it.
    ///
    /// Transition-guard backstop: every node lifecycle event is validated
    /// against the freshly projected state, so no emitter can bypass the
    /// guard. It protects only the log: an emitter with side effects (spawn,
    /// merge) must also validate before acting. A no-op verdict appends
    /// nothing and is not an error; a rejection is.
    ///
    /// The single point that names, on the events of a gesture, the Actor
    /// who made it ([`crate::application::gesture`]), and the one that
    /// records the Projet of a Run that starts (API 0.36): once the event is
    /// stored, before the bus announces it, so whoever hears of the Run can
    /// read its Projet.
    pub async fn execute(&self, event: &Event) -> anyhow::Result<()> {
        let stamped = crate::application::gesture::stamped(event);
        let event = stamped.as_ref().unwrap_or(event);
        if event.is_node_transition() || event.kind == EventKind::RunCompleted {
            let events = self.store.events(&event.run_id).await?;
            match validate_transition(project(&events).as_ref(), event) {
                Verdict::Allow => {}
                Verdict::NoOp { reason } => {
                    info!("append_event no-op ({:?}): {reason}", event.kind);
                    return Ok(());
                }
                Verdict::Reject { reason } => {
                    anyhow::bail!("transition rejected ({:?}): {reason}", event.kind);
                }
            }
        }
        self.store.append(event).await?;
        self.refresh_run_names(event).await;
        if let Some(project_id) = event.started_project_id() {
            self.run_projects.record(&event.run_id, project_id);
        }
        self.bus.publish(event.bus_message());
        Ok(())
    }

    /// Keep the names an extension reads (API 0.37) up to date with the
    /// Résumé of a Run that started, was renamed or archived, before anyone
    /// hears of it. A Résumé that cannot be read forgets the names: the caller
    /// shows the ids rather than a stale name.
    async fn refresh_run_names(&self, event: &Event) {
        if !matches!(
            event.kind,
            EventKind::RunStarted | EventKind::RunRenamed | EventKind::RunArchived
        ) {
            return;
        }
        let _refreshing = self.names_refresh.lock().await;
        match self.store.summary(&event.run_id).await {
            Ok(Some(summary)) => self.run_names.apply(&summary),
            Ok(None) => self.run_names.remove(&event.run_id),
            Err(e) => {
                warn!("the names of run {} are forgotten: {e}", event.run_id);
                self.run_names.remove(&event.run_id);
            }
        }
    }
}
