//! The session context's ports that still reach into this crate (the Runs,
//! the templates, the instance settings), implemented over the daemon's state.
//! They move to their own crates when those contexts are extracted.

mod daemon;
mod owners;
mod runs;

use std::sync::{Arc, Weak};

use pdo_session::application::ports::RunLookup;

use crate::{event_log, AppState};

/// Every Run-side port of the session context, over the daemon's state. The
/// state is reached through a `Weak`: the state owns the context, the context's
/// ports reach back into the state.
pub(crate) struct LegacySessionPorts {
    pub(crate) state: Weak<AppState>,
}

impl LegacySessionPorts {
    pub(super) fn state(&self) -> Result<Arc<AppState>, String> {
        self.state
            .upgrade()
            .ok_or_else(|| "the daemon is shutting down".to_string())
    }

    pub(super) async fn projected(
        &self,
        run_id: &str,
    ) -> Result<(Arc<AppState>, Vec<event_log::Event>, event_log::RunState), RunLookup> {
        let state = self.state().map_err(RunLookup::Storage)?;
        let events = crate::load_events(&state.db, run_id)
            .await
            .map_err(|e| RunLookup::Storage(e.to_string()))?;
        let run_state = event_log::project(&events).ok_or(RunLookup::NotFound)?;
        Ok((state, events, run_state))
    }

    pub(super) async fn record(
        &self,
        run_id: &str,
        node_id: &str,
        iter: i64,
        kind: event_log::EventKind,
        payload: serde_json::Value,
    ) -> Result<(), String> {
        let state = self.state()?;
        let event = event_log::Event {
            id: None,
            run_id: run_id.to_string(),
            ts: pdo_kernel::clock::now_iso(),
            kind,
            node_id: Some(node_id.to_string()),
            iter: Some(iter),
            payload: Some(payload),
        };
        crate::append_event(&state, &event)
            .await
            .map(|_| ())
            .map_err(|e| format!("{e:#}"))
    }
}
