//! The run context's routes: a Run's detail and its events, the `/ws` stream,
//! the node routes (the agent's `done` / `fail` / `skip`, the canvas buttons,
//! what a node reads), the Pipeline Manager's `/runs/{id}/commands` and the
//! liveness sweep's heartbeat. The legacy router merges [`router`] while it
//! still assembles the daemon, behind the `identify` middleware of the
//! composition root.
//!
//! ROUTER TRAP: a NEW top-level path prefix also needs its entry in the vite dev
//! proxy whitelist (`frontend/vite.config.ts`).

mod commands;
pub mod nodes;
mod run_detail;
mod stale;
pub mod verdicts;
mod ws;

use std::sync::Arc;

use axum::routing::{get, post};
use axum::Router;

use crate::Runs;
use nodes::{gestures, read};

/// The routes of the run context, with their state.
pub fn router(runs: Arc<Runs>) -> Router {
    Router::new()
        .route("/ws", get(ws::upgrade))
        .route("/runs/{run_id}", get(run_detail::detail))
        .route("/runs/{run_id}/events", get(run_detail::events))
        .route("/runs/{run_id}/commands", post(commands::run_command))
        .route("/runs/{run_id}/nodes/{node_id}/done", post(gestures::done))
        .route(
            "/runs/{run_id}/nodes/{node_id}/release",
            post(gestures::release),
        )
        .route(
            "/runs/{run_id}/nodes/{node_id}/children/wait",
            get(read::node_children_wait),
        )
        .route("/runs/{run_id}/nodes/{node_id}/fail", post(gestures::fail))
        .route("/runs/{run_id}/nodes/{node_id}/skip", post(gestures::skip))
        .route(
            "/runs/{run_id}/nodes/{node_id}/prompt",
            get(read::node_prompt),
        )
        .route("/runs/{run_id}/nodes/{node_id}/io", get(read::node_io))
        .route("/runs/{run_id}/nodes/{node_id}/diff", get(read::node_diff))
        .route(
            "/runs/{run_id}/nodes/{node_id}/start",
            post(gestures::start),
        )
        .route("/runs/{run_id}/nodes/{node_id}/stop", post(gestures::stop))
        .route(
            "/runs/{run_id}/nodes/{node_id}/retry",
            post(gestures::retry),
        )
        .route(
            "/runs/{run_id}/nodes/{node_id}/retry/preview",
            get(read::retry_preview),
        )
        .route("/stale/health", get(stale::health))
        .with_state(runs)
}
