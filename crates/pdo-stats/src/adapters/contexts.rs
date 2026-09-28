//! The stats context's ports over the contexts it reads or tells: the Projets
//! of the workspace, the audit journal of the platform, the kernel's bus.

use std::sync::Arc;

use pdo_kernel::event_bus::EventBus;
use pdo_platform::domain::audit::NewAuditEntry;
use pdo_platform::Platform;
use pdo_workspace::application::Workspace;
use pdo_workspace::domain::project::Project;

use crate::application::ports::{Announcements, AuditJournal, BoxFuture, Projects};

/// The Projets, over the workspace context.
pub struct WorkspaceProjects(pub Arc<Workspace>);

impl Projects for WorkspaceProjects {
    fn list(&self) -> BoxFuture<'_, Result<Vec<Project>, String>> {
        Box::pin(async move { self.0.list_projects().await.map_err(|e| e.to_string()) })
    }
}

/// The audit journal, over the platform context.
pub struct PlatformAudit(pub Arc<Platform>);

impl AuditJournal for PlatformAudit {
    fn record(&self, entry: NewAuditEntry) -> BoxFuture<'_, ()> {
        Box::pin(async move { self.0.record_audit().execute(entry).await })
    }
}

/// What every connected client is told, on the kernel's bus (the same JSON
/// `/ws` relays).
pub struct BusAnnouncements(pub Arc<dyn EventBus>);

impl Announcements for BusAnnouncements {
    fn cost_prices_synced(&self, rows: usize) {
        self.0.publish(serde_json::json!({
            "type": "cost_prices_synced",
            "rows": rows,
        }));
    }
}
