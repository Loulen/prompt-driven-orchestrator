//! Update the daemon (#699): spawn the detached executor and answer at once with
//! the attempt. Refused when the install method is unknown or an attempt is
//! running; never refused for active Runs (the daemon restarts, tmux sessions
//! survive — `KillMode=process`).

use std::path::PathBuf;
use std::sync::Arc;

use pdo_kernel::authorization::{authorize, Denied};
use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};

use super::update_context::UpdateContext;
use crate::domain::attempt::{apply_gate, AttemptStatus, UpdateAttempt, ATTEMPT_SCHEMA};
use crate::domain::install::{InstallMethod, Supervision};
use crate::domain::release::INSTALLED_VERSION;
use crate::domain::update_plan::{
    default_relaunch, explicit_bind_from_relaunch, render_update_script, UpdatePlan,
};

/// The attempt an apply started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartedUpdate {
    pub attempt_id: String,
    pub method: InstallMethod,
    pub command: &'static str,
    pub supervision: Supervision,
    pub log_path: PathBuf,
    pub started_at: String,
    pub from_version: String,
}

/// Why no attempt started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyError {
    Refused(Denied),
    /// The gate is closed: unknown method, or an attempt already running.
    Blocked {
        reason: Option<String>,
        method: InstallMethod,
    },
    /// No home directory to journal the attempt in.
    NoHome,
    /// The script could not be written or spawned; the record says `failed`.
    Failed {
        error: String,
        attempt_id: String,
    },
}

pub struct ApplyUpdate {
    update: Arc<UpdateContext>,
    host: Arc<dyn ExtensionHost>,
}

impl ApplyUpdate {
    pub fn new(update: Arc<UpdateContext>, host: Arc<dyn ExtensionHost>) -> Self {
        Self { update, host }
    }

    pub async fn execute(&self, actor: &Actor) -> Result<StartedUpdate, ApplyError> {
        let operation = Operation::new(ResourceKind::Update, Verb::Action("apply".into()));
        authorize(self.host.as_ref(), actor, &operation).map_err(ApplyError::Refused)?;
        let _serial = self.update.apply_lock.lock().await;
        let ports = &self.update.ports;
        let journal = &ports.journal;
        let method = ports.installation.method();
        let supervision = ports.installation.supervision();
        let last = journal
            .is_available()
            .then(|| journal.read_attempt())
            .flatten();
        let (ok, reason) = apply_gate(method, last.as_ref(), chrono::Utc::now());
        if !ok {
            return Err(ApplyError::Blocked { reason, method });
        }
        if !journal.is_available() {
            return Err(ApplyError::NoHome);
        }

        let attempt_id = new_attempt_id();
        let options = &self.update.options;
        let exe = ports
            .installation
            .invoked_exe()
            .unwrap_or_else(|| PathBuf::from("pdo"));
        let relaunch = options
            .relaunch_command
            .clone()
            .unwrap_or_else(|| default_relaunch(&exe, options.port));
        let plan = UpdatePlan {
            attempt_id: attempt_id.clone(),
            method,
            supervision,
            exe,
            port: options.port,
            bind: explicit_bind_from_relaunch(&relaunch),
            working_dir: options.working_dir.clone(),
            daemon_pid: std::process::id(),
            relaunch,
            log_path: journal.log_path(&attempt_id),
            attempt_path: journal.attempt_path(),
            from_version: INSTALLED_VERSION.to_string(),
        };
        let mut attempt = UpdateAttempt {
            schema: ATTEMPT_SCHEMA.to_string(),
            attempt_id: attempt_id.clone(),
            status: AttemptStatus::Running,
            started_at: pdo_kernel::clock::now_iso(),
            finished_at: None,
            exit_code: None,
            method,
            command: plan.upgrade_command().to_string(),
            supervision,
            log_path: plan.log_path.clone(),
            from_version: plan.from_version.clone(),
        };
        let spawned = journal
            .write_script(&attempt_id, &render_update_script(&plan))
            .and_then(|script| {
                journal.write_attempt(&attempt)?;
                ports.executor.spawn(&plan, &script)
            });
        match spawned {
            Ok(child) => {
                tracing::info!(
                    "update {attempt_id}: executor spawned (pid {}), method {method:?}, supervision {supervision:?}, log {}",
                    child.pid(),
                    plan.log_path.display()
                );
                self.record_exit_when_done(child, attempt_id.clone());
                Ok(StartedUpdate {
                    attempt_id,
                    method,
                    command: plan.upgrade_command(),
                    supervision,
                    log_path: plan.log_path,
                    started_at: attempt.started_at,
                    from_version: plan.from_version,
                })
            }
            Err(e) => {
                tracing::error!("update {attempt_id}: {e}");
                attempt.status = AttemptStatus::Failed;
                attempt.finished_at = Some(pdo_kernel::clock::now_iso());
                let _ = journal.write_attempt(&attempt);
                journal.append_log(&attempt_id, &format!("== pdo update {attempt_id}: {e}"));
                Err(ApplyError::Failed {
                    error: e,
                    attempt_id,
                })
            }
        }
    }

    /// Reap the executor if the daemon outlives it (fixture executor, failed
    /// upgrade): record the exit code, so a non-zero exit is visible in Settings
    /// even when the script died before its own `record`.
    fn record_exit_when_done(&self, child: Box<dyn super::ports::RunningExecutor>, id: String) {
        let journal = self.update.ports.journal.clone();
        tokio::task::spawn_blocking(move || {
            let Some(code) = child.wait() else {
                return;
            };
            if let Some(mut a) = journal.read_attempt() {
                if a.attempt_id != id {
                    return; // a newer attempt owns the record
                }
                a.finish(code, pdo_kernel::clock::now_iso);
                if let Err(e) = journal.write_attempt(&a) {
                    tracing::warn!("update {id}: cannot record the exit: {e}");
                }
            }
        });
    }
}

/// A fresh attempt id: UTC timestamp + short random suffix, filesystem-safe.
fn new_attempt_id() -> String {
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let suffix: String = uuid::Uuid::new_v4().simple().to_string()[..6].to_string();
    format!("{stamp}-{suffix}")
}
