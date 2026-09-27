//! The run context's routes: a Run's detail and its events, and the `/ws`
//! stream. The legacy router merges [`router`] while it still assembles the
//! daemon, behind the `identify` middleware of the composition root.
//!
//! ROUTER TRAP: a NEW top-level path prefix also needs its entry in the vite dev
//! proxy whitelist (`frontend/vite.config.ts`).

mod run_detail;
pub mod verdicts;
mod ws;

use std::sync::Arc;

use axum::routing::get;
use axum::Router;

use crate::Runs;

/// The routes of the run context, with their state.
pub fn router(runs: Arc<Runs>) -> Router {
    Router::new()
        .route("/ws", get(ws::upgrade))
        .route("/runs/{run_id}", get(run_detail::detail))
        .route("/runs/{run_id}/events", get(run_detail::events))
        .with_state(runs)
}
