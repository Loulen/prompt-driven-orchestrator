//! The live stream, as the pipeline context announces its changes on it.

use tokio::sync::broadcast;

use crate::application::ports::PipelineEvents;
use crate::domain::draft::DraftSummary;

/// The daemon's broadcast channel of JSON messages (the `/ws` stream).
pub struct BroadcastPipelineEvents(pub broadcast::Sender<serde_json::Value>);

impl PipelineEvents for BroadcastPipelineEvents {
    fn changed(&self, id: &str, path: &str) {
        let _ = self.0.send(serde_json::json!({
            "type": "pipeline_changed",
            "pipeline_id": id,
            "path": path,
        }));
    }

    fn validation_changed(&self, id: &str) {
        let _ = self.0.send(serde_json::json!({
            "type": "pipeline_validation_changed",
            "pipeline_id": id,
        }));
    }

    fn draft_changed(&self, id: &str, draft: &DraftSummary) {
        let _ = self.0.send(serde_json::json!({
            "type": "pipeline_draft_changed",
            "pipeline_id": id,
            "draft": draft,
        }));
    }

    fn version_changed(&self, id: &str) {
        let _ = self.0.send(serde_json::json!({
            "type": "pipeline_version_changed",
            "pipeline_id": id,
        }));
    }
}
