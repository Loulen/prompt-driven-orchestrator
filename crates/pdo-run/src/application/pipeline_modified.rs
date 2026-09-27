//! An edit of a Run's own pipeline on disk, as the pipeline watcher announces
//! it: recorded as `PipelineModified`, then the Run advances over the new graph.
//! A transition of the daemon: it never asks the Autorisation d'opération.

use std::sync::Arc;

use pdo_pipeline::adapters::watcher::RunPipelineModified;
use tokio::sync::mpsc::UnboundedReceiver;
use tracing::error;

use crate::application::events::append_event;
use crate::application::scheduler::node_completion::spawn_ready_after_event;
use crate::domain::event::{Event, EventKind};
use crate::Runs;

/// Relay every announcement of `rx` until the watcher goes away.
pub async fn relay_pipeline_modifications(
    state: Arc<Runs>,
    mut rx: UnboundedReceiver<RunPipelineModified>,
) {
    while let Some(modified) = rx.recv().await {
        let event = Event {
            id: None,
            run_id: modified.run_id.clone(),
            ts: pdo_kernel::clock::now_iso(),
            kind: EventKind::PipelineModified,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({
                "kind": modified.kind,
                "path": modified.path.to_string_lossy(),
            })),
        };
        if let Err(e) = append_event(&state, &event).await {
            error!("failed to append pipeline_modified: {e}");
            continue;
        }

        spawn_ready_after_event(&state, &modified.run_id).await;
    }
}
