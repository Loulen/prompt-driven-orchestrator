//! The detached update executor (#699): the script runs in a **new session**
//! (`setsid`), stdio bound to the attempt's log, so the daemon it replaces can
//! die under it.

use std::path::Path;

use crate::application::ports::{RunningExecutor, UpdateExecutor};
use crate::domain::update_plan::UpdatePlan;

/// Env seam: a command run INSTEAD of `sh <script>`; it receives the script path as
/// its single argument and the plan as `PDO_UPDATE_*` variables. Tests point it at a
/// fixture that logs what it was asked and exits.
pub const EXECUTOR_OVERRIDE_ENV: &str = "PDO_UPDATE_EXECUTOR";

/// Env seam: force the detected installation method (`homebrew` | `script` |
/// `unknown`). A test binary is neither brewed nor scripted; the FP container is.
pub const INSTALL_METHOD_OVERRIDE_ENV: &str = "PDO_INSTALL_METHOD";

/// Runs the plan with `sh`, or with the override command a test sets.
pub struct DetachedExecutor {
    override_cmd: Option<String>,
}

impl DetachedExecutor {
    pub fn new(override_cmd: Option<String>) -> Self {
        Self { override_cmd }
    }
}

struct Child(std::process::Child);

impl RunningExecutor for Child {
    fn pid(&self) -> u32 {
        self.0.id()
    }

    fn wait(mut self: Box<Self>) -> Option<i32> {
        let status = self.0.wait().ok()?;
        Some(status.code().unwrap_or(-1))
    }
}

impl UpdateExecutor for DetachedExecutor {
    fn spawn(&self, plan: &UpdatePlan, script: &Path) -> Result<Box<dyn RunningExecutor>, String> {
        spawn_detached(plan, script, self.override_cmd.as_deref())
            .map(|child| Box::new(Child(child)) as Box<dyn RunningExecutor>)
            .map_err(|e| format!("cannot spawn the update executor: {e}"))
    }
}

/// Spawn the executor **detached**: new session (`setsid`), stdin from `/dev/null`,
/// stdout+stderr appended to the log. `override_cmd` (the [`EXECUTOR_OVERRIDE_ENV`]
/// seam) replaces `sh <script>` by `<override_cmd> <script>`; the plan travels as
/// env variables either way so a fixture can log it without parsing shell.
///
/// The child is not waited on by the caller — the daemon may be gone before it
/// ends. It IS reaped when the daemon survives (fixture executor, failed upgrade):
/// [`spawn_detached`] returns the child, and the caller waits on it off-thread to
/// record the exit code.
fn spawn_detached(
    plan: &UpdatePlan,
    script: &Path,
    override_cmd: Option<&str>,
) -> std::io::Result<std::process::Child> {
    use std::os::unix::process::CommandExt;
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&plan.log_path)?;
    let log_err = log.try_clone()?;
    let mut cmd = match override_cmd {
        Some(o) if !o.trim().is_empty() => {
            let mut c = std::process::Command::new("sh");
            c.arg("-c")
                .arg(format!("exec {} \"$@\"", o.trim()))
                .arg("pdo-update-executor")
                .arg(script);
            c
        }
        _ => {
            let mut c = std::process::Command::new("sh");
            c.arg(script);
            c
        }
    };
    cmd.current_dir(&plan.working_dir)
        .env("PDO_UPDATE_ATTEMPT_ID", &plan.attempt_id)
        .env("PDO_UPDATE_COMMAND", plan.upgrade_command())
        .env("PDO_UPDATE_METHOD", plan.method.as_str())
        .env("PDO_UPDATE_SUPERVISION", plan.supervision.as_str())
        .env("PDO_UPDATE_EXE", &plan.exe)
        .env("PDO_UPDATE_PORT", plan.port.to_string())
        .env("PDO_UPDATE_DAEMON_PID", plan.daemon_pid.to_string())
        .env("PDO_UPDATE_RELAUNCH", plan.relaunch.join(" "))
        .env("PDO_UPDATE_LOG", &plan.log_path)
        .env("PDO_UPDATE_ATTEMPT_FILE", &plan.attempt_path)
        .env("PDO_UPDATE_SCRIPT", script)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::from(log))
        .stderr(std::process::Stdio::from(log_err));
    // SAFETY: `setsid` is async-signal-safe and touches no memory shared with the
    // parent; the closure does nothing else.
    unsafe {
        cmd.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    cmd.spawn()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::install::{InstallMethod, Supervision};
    use std::path::PathBuf;

    fn plan(dir: &Path) -> UpdatePlan {
        UpdatePlan {
            attempt_id: "a1".into(),
            method: InstallMethod::Script,
            supervision: Supervision::None,
            exe: PathBuf::from("/usr/bin/pdo"),
            port: 5172,
            bind: None,
            working_dir: dir.to_path_buf(),
            daemon_pid: 1,
            relaunch: vec!["pdo".into(), "daemon".into()],
            log_path: dir.join("a1.log"),
            attempt_path: dir.join("last-attempt.json"),
            from_version: "1.0.0".into(),
        }
    }

    #[test]
    fn the_override_receives_the_script_and_the_plan_and_its_output_lands_in_the_log() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("a1.sh");
        std::fs::write(&script, "#!/bin/sh\n").unwrap();
        let executor = DetachedExecutor::new(Some(
            "sh -c 'echo \"$PDO_UPDATE_METHOD $1\"; exit 3' fixture".into(),
        ));
        let child = executor.spawn(&plan(dir.path()), &script).unwrap();
        assert!(child.pid() > 0);
        assert_eq!(child.wait(), Some(3));
        let log = std::fs::read_to_string(dir.path().join("a1.log")).unwrap();
        assert_eq!(log.trim(), format!("script {}", script.display()));
    }

    #[test]
    fn without_override_the_script_runs_under_sh() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("a1.sh");
        std::fs::write(&script, "echo ran; exit 0\n").unwrap();
        let child = DetachedExecutor::new(None)
            .spawn(&plan(dir.path()), &script)
            .unwrap();
        assert_eq!(child.wait(), Some(0));
        let log = std::fs::read_to_string(dir.path().join("a1.log")).unwrap();
        assert_eq!(log, "ran\n");
    }
}
