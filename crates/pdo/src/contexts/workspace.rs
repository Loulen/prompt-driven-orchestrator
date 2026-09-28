//! The `workspace` context: the Projets, the provisioning rules and the recent
//! repositories over SQLite, git through its CLI, the source tree and the file
//! browser of the daemon's repo root, the Projets' announcements on the bus.

use std::sync::Arc;

use pdo_kernel::extension_host::ExtensionHost;
use pdo_workspace::adapters::{fs, git, sqlite};
use pdo_workspace::application::{Workspace, WorkspacePorts};

use super::ContextResources;
use crate::ports::project_events::BusProjectEvents;

pub(super) fn workspace(r: &ContextResources, host: Arc<dyn ExtensionHost>) -> Arc<Workspace> {
    Arc::new(Workspace::new(
        WorkspacePorts {
            projects: Arc::new(sqlite::SqliteProjects::new(r.db.clone())),
            provisioning: Arc::new(sqlite::SqliteProvisioningRules::new(r.db.clone())),
            recent: Arc::new(sqlite::SqliteRecentRepositories::new(r.db.clone())),
            events: Arc::new(BusProjectEvents(r.bus_tx.clone())),
            git: Arc::new(git::GitCli),
            source_tree: Arc::new(fs::FsSourceTree),
            files: Arc::new(fs::FsBrowser::new(&r.repo_root)),
        },
        host,
    ))
}
