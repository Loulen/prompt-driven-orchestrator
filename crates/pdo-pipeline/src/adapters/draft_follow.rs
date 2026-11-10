//! A file of the registry written by anyone (an editor outside PDO, an agent,
//! an overwrite, a rename) lands in its shared draft (ADR-0088): this task
//! listens to the `pipeline_changed` announcements of the live stream. One
//! the watcher marks `outside` is kept as a version « external edit » first.

use std::sync::Arc;

use serde_json::Value;
use tokio::sync::broadcast::{self, error::RecvError};
use tracing::warn;

use crate::application::Pipelines;

/// Follow the announcements of `stream` until it closes.
pub fn spawn(pipelines: Arc<Pipelines>, mut stream: broadcast::Receiver<Value>) {
    tokio::spawn(async move {
        loop {
            match stream.recv().await {
                Ok(message) => {
                    if message["type"] != "pipeline_changed" {
                        continue;
                    }
                    let Some(id) = message["pipeline_id"].as_str() else {
                        continue;
                    };
                    if message["outside"] == true {
                        let path = message["path"].as_str().unwrap_or(id);
                        pipelines.file_written_outside(id, path).await;
                    } else {
                        pipelines.draft_file_changed(id).await;
                    }
                }
                Err(RecvError::Lagged(n)) => {
                    warn!("drafts: {n} announcements missed, a written file may not show in its draft")
                }
                Err(RecvError::Closed) => return,
            }
        }
    });
}
