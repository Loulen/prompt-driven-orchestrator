//! Append one event to a Run's log, then announce it on the bus.
//!
//! A transition of the Run, never an actor's gesture: the daemon, the
//! scheduler and the agents' completions reach it once the use case that
//! decided the transition has asked whatever it had to ask. It does not ask
//! the Autorisation d'opération (D10).

use std::sync::Arc;

use pdo_kernel::event_bus::EventBus;
use tracing::info;

use crate::application::ports::EventStore;
use crate::domain::event::{Event, EventKind};
use crate::domain::projection::project;
use crate::domain::transition_guard::{validate_transition, Verdict};

pub struct AppendEvent {
    store: Arc<dyn EventStore>,
    bus: Arc<dyn EventBus>,
}

impl AppendEvent {
    pub fn new(store: Arc<dyn EventStore>, bus: Arc<dyn EventBus>) -> Self {
        Self { store, bus }
    }

    /// Append `event` and publish it.
    ///
    /// Transition-guard backstop: every node lifecycle event is validated
    /// against the freshly projected state, so no emitter can bypass the
    /// guard. It protects only the log: an emitter with side effects (spawn,
    /// merge) must also validate before acting. A no-op verdict appends
    /// nothing and is not an error; a rejection is.
    pub async fn execute(&self, event: &Event) -> anyhow::Result<()> {
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
        self.bus.publish(event.bus_message());
        Ok(())
    }
}
