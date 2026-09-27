//! The seam between the legacy daemon and the bounded contexts already
//! extracted from it (ADR-0079).
//!
//! The legacy daemon opens the database and binds the listener; the composition
//! root wires each extracted context over these [`ContextResources`] and hands
//! back the [`Contexts`]. The ports a context needs from what is still legacy
//! (the Runs, the templates, the instance settings) are implemented here for
//! `platform`, in `session_ports` for `session`, in `pipeline_ports` for
//! `pipeline` and in `run_ports` for `run`, and move to their own crates when
//! those contexts are extracted.

use std::path::PathBuf;
use std::sync::{Arc, Weak};

use axum::Router;
use pdo_kernel::event_bus::EventBus;
use pdo_pipeline::application::ports::{ActiveRuns, RenameLedger};
use pdo_platform::application::ports::{BoxFuture, Runs, UpdateCheckSetting};
use pdo_platform::domain::install::{InstallMethod, Supervision};
use pdo_platform::domain::service_health::ServiceHealthOverride;
use pdo_platform::Platform;
use pdo_run::application::ports::LiveFacts;

use pdo_session::application::ports::{
    AssistantHarness, DaemonGauges, NodePanes, NodeWaits, RunShells, SessionOwners, Templates,
};
use pdo_session::Session;
use tokio::sync::broadcast;

use crate::pipeline_ports::{LegacyActiveRuns, LegacyRenameLedger};
use crate::run_ports::LegacyLiveFacts;
use crate::session_ports::LegacySessionPorts;
use crate::SelfWrites;
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
    /// Replaces the harness tail of a session (the per-daemon test seam).
    pub tmux_cmd_override: Option<String>,
    /// Extra WebSocket origins, parsed at boot.
    pub allowed_ws_origins: Vec<String>,
    /// The session context's Run-side ports.
    pub session: SessionRunPorts,
    /// What the pipeline context needs from this daemon.
    pub pipeline: PipelineResources,
    /// What the run context needs from this daemon.
    pub run: RunResources,
}

/// The run context's resources: the kernel's bus the daemon opened, and what a
/// Run's detail reads from disk, which the legacy side still implements.
#[derive(Clone)]
pub struct RunResources {
    pub bus: Arc<dyn EventBus>,
    pub live_facts: Arc<dyn LiveFacts>,
}

/// The pipeline context's resources: the ports the Runs and the Stats
/// implement, the skill bank, the live stream and the record of the daemon's
/// own writes the watcher reads.
#[derive(Clone)]
pub struct PipelineResources {
    pub runs: Arc<dyn ActiveRuns>,
    pub renames: Arc<dyn RenameLedger>,
    pub skills: Arc<pdo_skill::Skills>,
    pub events: broadcast::Sender<serde_json::Value>,
    pub self_writes: SelfWrites,
}

/// The ports of the session context that the Runs, the templates and the
/// instance settings implement.
#[derive(Clone)]
pub struct SessionRunPorts {
    pub waits: Arc<dyn NodeWaits>,
    pub panes: Arc<dyn NodePanes>,
    pub shells: Arc<dyn RunShells>,
    pub owners: Arc<dyn SessionOwners>,
    pub gauges: Arc<dyn DaemonGauges>,
    pub templates: Arc<dyn Templates>,
    pub assistant_harness: Arc<dyn AssistantHarness>,
}

impl SessionRunPorts {
    /// Every port over the daemon's state, reached through `state`.
    pub(crate) fn over(state: Weak<AppState>) -> Self {
        let ports = Arc::new(LegacySessionPorts { state });
        Self {
            waits: ports.clone(),
            panes: ports.clone(),
            shells: ports.clone(),
            owners: ports.clone(),
            gauges: ports.clone(),
            templates: ports.clone(),
            assistant_harness: ports,
        }
    }
}

/// The extracted contexts, wired: each one's handle, and the routes they serve.
pub struct Contexts {
    pub platform: Arc<Platform>,
    pub session: Arc<Session>,
    /// The run context. Its routes are merged by the legacy router, which the
    /// daemon's in-crate tests drive as is.
    pub run: Arc<pdo_run::Runs>,
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
    pub tmux_cmd_override: Option<String>,
    pub allowed_ws_origins: Vec<String>,
    pub skills: Arc<pdo_skill::Skills>,
    pub pipeline_events: broadcast::Sender<serde_json::Value>,
    pub self_writes: SelfWrites,
    pub bus: Arc<dyn EventBus>,
}

impl ContextResourcesSeed {
    pub(crate) fn into_resources(self, state: Weak<AppState>) -> ContextResources {
        ContextResources {
            update_setting: Arc::new(LegacyUpdateSetting {
                db: self.db.clone(),
            }),
            runs: Arc::new(LegacyRuns {
                state: state.clone(),
            }),
            run: RunResources {
                bus: self.bus,
                live_facts: Arc::new(LegacyLiveFacts {
                    state: state.clone(),
                }),
            },
            session: SessionRunPorts::over(state),
            pipeline: PipelineResources {
                runs: Arc::new(LegacyActiveRuns {
                    db: self.db.clone(),
                }),
                renames: Arc::new(LegacyRenameLedger {
                    db: self.db.clone(),
                }),
                skills: self.skills,
                events: self.pipeline_events,
                self_writes: self.self_writes,
            },
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
            tmux_cmd_override: self.tmux_cmd_override,
            allowed_ws_origins: self.allowed_ws_origins,
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

/// The session context of a unit-test state: the real tmux adapter on the test
/// daemon's socket (no server runs there), the OS terminal window, and no
/// extension — or the `tmux` a test injects.
#[cfg(test)]
pub(crate) fn test_session(
    state: Weak<AppState>,
    repo_root: PathBuf,
    port: u16,
    tmux: Option<Arc<dyn pdo_session::application::ports::Tmux>>,
) -> Arc<Session> {
    let run = SessionRunPorts::over(state);
    Session::new(
        pdo_session::SessionPorts {
            tmux: tmux.unwrap_or_else(|| {
                Arc::new(pdo_session::adapters::tmux::TmuxCli::for_daemon(port))
            }),
            waits: run.waits,
            panes: run.panes,
            shells: run.shells,
            owners: run.owners,
            gauges: run.gauges,
            templates: run.templates,
            assistant_harness: run.assistant_harness,
            terminal_window: Arc::new(pdo_session::adapters::terminal_window::OsTerminalWindow),
            extensions: Arc::new(pdo_kernel::extension_host::NoExtensions),
        },
        pdo_session::SessionOptions {
            daemon_port: port,
            repo_root,
            tmux_cmd_override: Some("exec true".to_string()),
            allowed_ws_origins: Vec::new(),
        },
    )
}

/// The run context of a unit-test state: the SQLite log of the test
/// database, the state's bus, the legacy live facts, and the session's tmux
/// for the manager probe.
#[cfg(test)]
pub(crate) fn test_run(
    state: Weak<AppState>,
    db: &sqlx::SqlitePool,
    bus_tx: &broadcast::Sender<serde_json::Value>,
    session: &Session,
    port: u16,
) -> Arc<pdo_run::Runs> {
    pdo_run::Runs::new(
        pdo_run::RunsPorts {
            store: Arc::new(
                pdo_run::adapters::sqlite_event_store::SqliteEventStore::new(db.clone()),
            ),
            bus: Arc::new(pdo_kernel::event_bus::BroadcastEventBus::from_sender(
                bus_tx.clone(),
            )),
            live_facts: Arc::new(LegacyLiveFacts { state }),
            managers: Arc::new(pdo_run::adapters::manager_sessions::TmuxManagerSessions(
                session.tmux().clone(),
            )),
        },
        pdo_run::RunsOptions {
            daemon_port: port,
            allowed_ws_origins: Vec::new(),
        },
    )
}

#[cfg(test)]
mod tests {
    use pdo_session::domain::declared_wait::{CAUSE_COMPLETION_NOT_RELEASED, CAUSE_DECLARED};

    use crate::event_log::{AWAITING_CAUSE_COMPLETION_NOT_RELEASED, AWAITING_CAUSE_DECLARED};

    /// The session context declares a wait with its own causes; the Run's
    /// projection folds them under the same names.
    #[test]
    fn the_wait_causes_are_the_ones_the_projection_folds() {
        assert_eq!(CAUSE_DECLARED, AWAITING_CAUSE_DECLARED);
        assert_eq!(
            CAUSE_COMPLETION_NOT_RELEASED,
            AWAITING_CAUSE_COMPLETION_NOT_RELEASED
        );
    }
}
