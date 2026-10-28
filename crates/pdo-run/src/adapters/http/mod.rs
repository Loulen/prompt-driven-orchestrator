//! The run context's routes: the Run's surfaces (create, list, forget, the
//! children, the artifacts, the refs, the source drift, the Run's own pipeline,
//! its repositories, its Pipeline Manager), a Run's detail and its events, the
//! `/ws` stream, the node routes (the agent's `done` / `fail` / `skip`, the
//! canvas buttons, what a node reads), the Pipeline Manager's
//! `/runs/{id}/commands`, the Presences on the Runs and the liveness sweep's
//! heartbeat. The legacy router
//! merges [`router`] while it still assembles the daemon, behind the
//! `identify` middleware of the composition root.
//!
//! ROUTER TRAP: a NEW top-level path prefix also needs its entry in the vite dev
//! proxy whitelist (`frontend/vite.config.ts`).

mod commands;
pub mod nodes;
mod presence;
pub mod read_guard;
mod run_detail;
pub mod runs;
mod stale;
pub mod verdicts;
mod ws;

use std::sync::Arc;

use axum::extract::DefaultBodyLimit;
use axum::routing::{delete, get, patch, post, put};
use axum::Router;

use crate::Runs;
use nodes::{gestures, import, read};

/// The routes of the run context, with their state.
pub fn router(runs: Arc<Runs>) -> Router {
    Router::new()
        // #779: the multipart body of `POST /runs` is bounded by the
        // `max_attachments_mb` setting, enforced IN the handler on the attachment
        // parts (read fresh per request) — not by a static limit frozen at
        // router build. The JSON branch keeps its own 10 MB `to_bytes` cap.
        .route(
            "/runs",
            post(runs::create).route_layer(DefaultBodyLimit::disable()),
        )
        .route("/runs", get(runs::list))
        // Static path — must stay declared before `/runs/{run_id}`.
        .route("/runs/reapable", get(runs::reapable))
        // API 0.12: who has which Run open. Static too, before `/runs/{run_id}`.
        .route("/runs/presences", get(presence::list))
        // The cost of the Runs a list shows (ADR-0052). Static, before `/runs/{run_id}`.
        .route("/runs/costs", get(runs::costs))
        .route("/runs/{run_id}", delete(runs::forget))
        .route("/runs/{run_id}/children", get(runs::children))
        .route("/runs/{run_id}/file", get(runs::file))
        .route("/runs/{run_id}/refs", get(runs::refs))
        .route("/runs/{run_id}/source-drift", get(runs::source_drift))
        .route("/runs/{run_id}/artifact", get(runs::artifact))
        .route("/runs/{run_id}/artifact/zip", get(runs::artifact_zip))
        .route("/runs/{run_id}/pipeline", get(runs::pipeline))
        .route(
            "/runs/{run_id}/pipeline/document",
            get(runs::pipeline_document),
        )
        .route(
            "/runs/{run_id}/pipeline/document/skills",
            get(runs::pipeline_document_skills),
        )
        .route("/runs/{run_id}/repos", patch(runs::edit_repos))
        .route("/runs/{run_id}/pipeline", put(runs::save_pipeline))
        // ADR-0085: what « Overwrite default pipeline » would touch, then the
        // overwrite itself — the only way a Run's edit reaches the shared one.
        .route(
            "/runs/{run_id}/pipeline/overwrite-preview",
            get(runs::overwrite_preview),
        )
        .route(
            "/runs/{run_id}/pipeline/overwrite-default",
            post(runs::overwrite_default),
        )
        // Manager on demand: start/stop the Run's Pipeline Manager from the
        // Manager tab. Start is idempotent (a double-click is a benign
        // re-answer); stop on a dead session is a calm no-op.
        .route("/runs/{run_id}/manager/start", post(runs::start_manager))
        .route("/runs/{run_id}/manager/stop", post(runs::stop_manager))
        .route("/ws", get(ws::upgrade))
        .route("/runs/{run_id}", get(run_detail::detail))
        .route("/runs/{run_id}/events", get(run_detail::events))
        .route(
            "/runs/{run_id}/transcripts/search",
            get(run_detail::transcript_search),
        )
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
            "/runs/{run_id}/nodes/{node_id}/transcript",
            get(read::node_transcript),
        )
        .route(
            "/runs/{run_id}/nodes/{node_id}/transcript/tools/{index}",
            get(read::node_transcript_tool),
        )
        .route(
            "/runs/{run_id}/nodes/{node_id}/start",
            post(gestures::start),
        )
        .route("/runs/{run_id}/nodes/{node_id}/stop", post(gestures::stop))
        // The import into a running node: its budget is `max_attachments_mb`,
        // enforced IN the handler (read fresh per request), as `POST /runs`.
        .route(
            "/runs/{run_id}/nodes/{node_id}/attachments",
            post(import::import).route_layer(DefaultBodyLimit::disable()),
        )
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
