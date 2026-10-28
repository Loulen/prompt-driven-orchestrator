//! The ports of the Sessions hébergées (ADR-0091) that read the other
//! contexts: the primary repository of a Projet (workspace), the harness an
//! agent runs on and the Cap de sessions concurrentes (the Runs').

use std::path::PathBuf;

use pdo_harness::registry::HarnessDescriptor;
use pdo_run::Runs;
use pdo_session::application::hosted::{
    AdmissionSlot, CapReached, HostedHarness, HostedPlaces, SessionAdmission,
};
use pdo_session::application::ports::BoxFuture;

use crate::ports::Late;

/// What the Sessions hébergées read of the daemon.
pub(crate) struct HostedSide {
    pub(crate) runs: Late<Runs>,
    pub(crate) db: sqlx::SqlitePool,
    /// The daemon user's home (the test seam's in the tests).
    pub(crate) home: Option<PathBuf>,
}

impl HostedPlaces for HostedSide {
    fn home(&self) -> Option<PathBuf> {
        self.home.clone()
    }

    fn project_repo<'a>(&'a self, project: &'a str) -> BoxFuture<'a, Option<PathBuf>> {
        Box::pin(async move {
            let project = pdo_workspace::adapters::sqlite::get(&self.db, project)
                .await
                .ok()
                .flatten()?;
            project.default_primary().map(PathBuf::from)
        })
    }
}

impl HostedHarness for HostedSide {
    /// The harness named, else the instance's default one (the `claude`
    /// floor), resolved against the descriptor file like the library
    /// assistant's.
    fn resolve<'a>(&'a self, name: Option<&'a str>) -> BoxFuture<'a, Option<HarnessDescriptor>> {
        Box::pin(async move {
            let Ok(runs) = self.runs.get() else {
                return name.is_none().then(pdo_harness::registry::claude);
            };
            let home_root = pdo_run::application::sandbox::sandbox_home_roots(&runs)
                .map(|(home, _)| home)
                .unwrap_or_default();
            let registry = pdo_harness::adapters::descriptor_file::load(&home_root);
            match name.filter(|n| !n.trim().is_empty()) {
                Some(name) => registry.resolve(name),
                None => {
                    let default_harness =
                        pdo_run::application::settings::stored_default_harness(&runs).await;
                    let name = pdo_harness::resolver::resolve_infra_harness(
                        None,
                        default_harness.as_deref(),
                    );
                    Some(
                        registry
                            .resolve(&name)
                            .unwrap_or_else(pdo_harness::registry::claude),
                    )
                }
            }
        })
    }
}

impl SessionAdmission for HostedSide {
    /// The Runs' admission lock, held from the count until the session is
    /// recorded: the Nodes and the Sessions hébergées share the Cap.
    fn admit(&self) -> BoxFuture<'_, Result<AdmissionSlot, CapReached>> {
        Box::pin(async move {
            let Ok(runs) = self.runs.get() else {
                return Err(CapReached { live: 0, cap: 0 });
            };
            let guard = runs.admission_lock_owned().lock_owned().await;
            let stored = pdo_run::application::settings::stored_session_cap(&runs).await;
            let cap = pdo_run::adapters::env::configured_cap_with(stored);
            let live =
                pdo_run::application::admission_count::count_global_live_sessions(&runs).await;
            if pdo_run::domain::admission::can_admit(live, cap) {
                Ok(AdmissionSlot::holding(guard))
            } else {
                Err(CapReached { live, cap })
            }
        })
    }

    /// A freed slot wakes the Nodes waiting for one, as stopping a Node does.
    fn released(&self) {
        let Ok(runs) = self.runs.get() else {
            return;
        };
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                pdo_run::application::scheduler::node_completion::retry_waiting_nodes(&runs).await
            });
        }
    }
}
