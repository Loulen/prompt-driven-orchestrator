//! Read one Run: its events as the log holds them, and its detail.

use std::sync::Arc;

use crate::application::ports::{EventStore, LiveFacts, ManagerSessions};
use crate::application::projections::overlay_child_awaiting;
use crate::application::tree_cost::TreeCosts;
use crate::domain::event::Event;
use crate::domain::projection::project;
use crate::domain::state::RunState;

/// A Run's detail: its projection, overlaid and filled with what the disk
/// says (its tree cost included), the events it was folded from, and what is
/// observed live.
#[derive(Debug, Clone)]
pub struct RunDetail {
    pub state: RunState,
    pub events: Vec<Event>,
    /// Whether the Pipeline Manager has a live session: an observed fact,
    /// never a projected one (a projected flag would drift from the orphan
    /// sweep, a manual stop or a daemon restart).
    pub has_manager: bool,
}

pub struct ReadRun {
    store: Arc<dyn EventStore>,
    live_facts: Arc<dyn LiveFacts>,
    managers: Arc<dyn ManagerSessions>,
}

impl ReadRun {
    pub fn new(
        store: Arc<dyn EventStore>,
        live_facts: Arc<dyn LiveFacts>,
        managers: Arc<dyn ManagerSessions>,
    ) -> Self {
        Self {
            store,
            live_facts,
            managers,
        }
    }

    /// The events of `run_id`, in append order; empty for an unknown Run.
    pub async fn events(&self, run_id: &str) -> anyhow::Result<Vec<Event>> {
        self.store.events(run_id).await
    }

    /// The detail of `run_id`; `None` when the log holds no started Run
    /// under that id.
    pub async fn detail(&self, run_id: &str) -> anyhow::Result<Option<RunDetail>> {
        let events = self.store.events(run_id).await?;
        let Some(mut state) = project(&events) else {
            return Ok(None);
        };
        overlay_child_awaiting(self.store.as_ref(), &mut state).await;
        self.live_facts.fill(&mut state, &events).await;
        state.tree_cost = TreeCosts::of_run(
            self.store.as_ref(),
            self.live_facts.as_ref(),
            run_id,
            state.cost.clone(),
        )
        .await;
        let has_manager = self.managers.exists(run_id);
        Ok(Some(RunDetail {
            state,
            events,
            has_manager,
        }))
    }
}
