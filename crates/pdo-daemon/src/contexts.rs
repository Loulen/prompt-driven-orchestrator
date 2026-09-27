//! The seam between the legacy daemon and the bounded contexts already
//! extracted from it (ADR-0079).
//!
//! The legacy daemon opens the database and binds the listener; the composition
//! root wires each extracted context over these [`ContextResources`] and hands
//! back the [`Contexts`]. The ports a context needs from what is still legacy
//! (the Runs, the instance settings) are implemented here, and move to their own
//! crates when those contexts are extracted.

use std::path::PathBuf;
use std::sync::{Arc, Weak};

use axum::Router;
use pdo_platform::application::ports::{BoxFuture, Runs, UpdateCheckSetting};
use pdo_platform::domain::install::{InstallMethod, Supervision};
use pdo_platform::domain::service_health::ServiceHealthOverride;
use pdo_platform::Platform;

use crate::{event_log, instance_config, AppState, SessionClaim};

/// What the legacy daemon offers the contexts it hosts.
pub struct ContextResources {
    /// The pool of `pdo.db`.
    pub db: sqlx::SqlitePool,
    /// The daemon's repo root (its cwd).
    pub repo_root: PathBuf,
    /// The port the daemon listens on.
    pub port: u16,
    /// The home the update journal lives under: the per-daemon override, else
    /// `$HOME`; `None` when neither is set.
    pub home_root: Option<PathBuf>,
    pub update_source_url: Option<String>,
    pub update_executor_override: Option<String>,
    pub install_method_override: Option<InstallMethod>,
    pub supervision_override: Option<Supervision>,
    pub relaunch_command: Option<Vec<String>>,
    pub service_health_override: Option<ServiceHealthOverride>,
    /// The Runs, as the platform context sees them.
    pub runs: Arc<dyn Runs>,
    /// The `update_check` instance setting.
    pub update_setting: Arc<dyn UpdateCheckSetting>,
}

/// The extracted contexts, wired: each one's handle, and the routes they serve.
pub struct Contexts {
    pub platform: Arc<Platform>,
    pub router: Router,
}

/// The resources known before the state exists; the ports that reach back into
/// the state are added once it is being built.
pub(crate) struct ContextResourcesSeed {
    pub db: sqlx::SqlitePool,
    pub repo_root: PathBuf,
    pub port: u16,
    pub home_root: Option<PathBuf>,
    pub update_source_url: Option<String>,
    pub update_executor_override: Option<String>,
    pub install_method_override: Option<InstallMethod>,
    pub supervision_override: Option<Supervision>,
    pub relaunch_command: Option<Vec<String>>,
    pub service_health_override: Option<ServiceHealthOverride>,
}

impl ContextResourcesSeed {
    pub(crate) fn into_resources(self, state: Weak<AppState>) -> ContextResources {
        ContextResources {
            update_setting: Arc::new(LegacyUpdateSetting {
                db: self.db.clone(),
            }),
            runs: Arc::new(LegacyRuns { state }),
            db: self.db,
            repo_root: self.repo_root,
            port: self.port,
            home_root: self.home_root,
            update_source_url: self.update_source_url,
            update_executor_override: self.update_executor_override,
            install_method_override: self.install_method_override,
            supervision_override: self.supervision_override,
            relaunch_command: self.relaunch_command,
            service_health_override: self.service_health_override,
        }
    }
}

/// The Runs of the legacy daemon, behind the platform's port.
pub(crate) struct LegacyRuns {
    pub(crate) state: Weak<AppState>,
}

impl LegacyRuns {
    fn state(&self) -> Result<Arc<AppState>, String> {
        self.state
            .upgrade()
            .ok_or_else(|| "the daemon is shutting down".to_string())
    }
}

impl Runs for LegacyRuns {
    fn verify<'a>(
        &'a self,
        run_id: &'a str,
        node_id: &'a str,
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let state = self.state()?;
            let claim = SessionClaim {
                run_id: run_id.to_string(),
                node_id: node_id.to_string(),
            };
            crate::verify_session_claim(&state, &claim)
                .await
                .map(|_| ())
                .map_err(|(_, body)| body["error"].as_str().unwrap_or_default().to_string())
        })
    }

    fn record_command<'a>(
        &'a self,
        run_id: &'a str,
        node_id: &'a str,
        command: &'a str,
        page_mount: serde_json::Value,
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let state = self.state()?;
            crate::append_event(
                &state,
                &event_log::Event {
                    id: None,
                    run_id: run_id.to_string(),
                    ts: pdo_kernel::clock::now_iso(),
                    kind: event_log::EventKind::CommandIssued,
                    node_id: Some(node_id.to_string()),
                    iter: None,
                    payload: Some(serde_json::json!({
                        "command": command,
                        "page_mount": page_mount,
                    })),
                },
            )
            .await
            .map(|_| ())
            .map_err(|e| format!("{e:#}"))
        })
    }

    /// Live Runs (`Running` / `AwaitingUser` / `Paused`) — what the update's
    /// confirm dialog counts.
    fn count_live(&self) -> BoxFuture<'_, usize> {
        Box::pin(async move {
            let Ok(state) = self.state() else {
                return 0;
            };
            let Ok(run_ids) = crate::load_all_run_ids(&state.db).await else {
                return 0;
            };
            let mut live = 0;
            for run_id in run_ids {
                let Ok(events) = crate::load_events(&state.db, &run_id).await else {
                    continue;
                };
                if let Some(run_state) = event_log::project(&events) {
                    if run_state.status.is_live() {
                        live += 1;
                    }
                }
            }
            live
        })
    }
}

/// The `update_check` instance setting, `stored → env → default`, read fresh.
pub(crate) struct LegacyUpdateSetting {
    pub(crate) db: sqlx::SqlitePool,
}

impl UpdateCheckSetting for LegacyUpdateSetting {
    fn enabled(&self) -> BoxFuture<'_, bool> {
        Box::pin(async move {
            let stored = instance_config::get(&self.db)
                .await
                .ok()
                .and_then(|c| c.update_check);
            pdo_platform::domain::update_check::resolve_update_check(
                stored,
                pdo_platform::adapters::env::env_update_check(),
            )
        })
    }
}
