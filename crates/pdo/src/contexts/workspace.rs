//! The `workspace` context: the Projets, the provisioning rules and the recent
//! repositories over SQLite, git through its CLI, the source tree and the file
//! browser of the daemon's repo root, the Projets' announcements on the bus
//! and their names for the extensions, and the pipelines' validations and
//! Triggers a deleted Projet leaves.

use std::sync::Arc;

use pdo_kernel::extension_host::ExtensionHost;
use pdo_pipeline::Pipelines;
use pdo_workspace::adapters::{fs, git, sqlite};
use pdo_workspace::application::{Workspace, WorkspacePorts};

use super::ContextResources;
use crate::ports::pipeline_projects::ForgetDeletedProjects;
use crate::ports::project_events::BusProjectEvents;
use crate::ports::Late;

pub(super) fn workspace(
    r: &ContextResources,
    pipelines: &Late<Pipelines>,
    host: Arc<dyn ExtensionHost>,
) -> Arc<Workspace> {
    let projects = Arc::new(sqlite::SqliteProjects::new(r.db.clone()));
    Arc::new(Workspace::new(
        WorkspacePorts {
            projects: projects.clone(),
            provisioning: Arc::new(sqlite::SqliteProvisioningRules::new(r.db.clone())),
            recent: Arc::new(sqlite::SqliteRecentRepositories::new(r.db.clone())),
            events: Arc::new(BusProjectEvents {
                bus: r.bus_tx.clone(),
                names: r.project_names.clone(),
                projects,
                changing: Default::default(),
            }),
            deletions: Arc::new(ForgetDeletedProjects {
                pipelines: pipelines.clone(),
                db: r.db.clone(),
            }),
            git: Arc::new(git::GitCli),
            source_tree: Arc::new(fs::FsSourceTree),
            files: Arc::new(fs::FsBrowser::new(&r.repo_root)),
        },
        host,
    ))
}
