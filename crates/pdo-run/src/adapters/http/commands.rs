//! `POST /runs/{run_id}/commands` — the Pipeline Manager's commands, handed to
//! the `RunCommand` use case with the Actor `identify` set.

use std::sync::Arc;

use axum::extract::{Json, Path, State};
use axum::response::Response;
use axum::Extension;
use pdo_kernel::Actor;

use crate::application::commands::RunCommandRequest;
use crate::Runs;

pub(super) async fn run_command(
    State(runs): State<Arc<Runs>>,
    Extension(actor): Extension<Actor>,
    Path(run_id): Path<String>,
    Json(request): Json<RunCommandRequest>,
) -> Response {
    runs.run_command(&actor, run_id, request).await
}
