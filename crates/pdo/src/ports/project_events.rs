//! A Projet change, announced on the raw channel of the kernel's bus, which
//! `/ws` relays to the UI.

use pdo_workspace::application::ports::ProjectEvents;
use tokio::sync::broadcast;

pub(crate) struct BusProjectEvents(pub(crate) broadcast::Sender<serde_json::Value>);

impl ProjectEvents for BusProjectEvents {
    fn project_changed(&self, project_id: &str) {
        let _ = self.0.send(serde_json::json!({
            "type": "project_changed",
            "project_id": project_id,
        }));
    }
}
