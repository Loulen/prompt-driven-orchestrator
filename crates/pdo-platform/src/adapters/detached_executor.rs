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
    use crate::domain::install::{InstallMethod, Supervision, SCRIPT_COMMAND};
    use std::path::PathBuf;

    fn plan(dir: &Path) -> UpdatePlan {
        UpdatePlan {
            attempt_id: "a1".into(),
            method: InstallMethod::Script,
            upgrade_command: SCRIPT_COMMAND.into(),
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
            to_version: None,
            snapshot: None,
            ready_marker: dir.join("daemon-5172.json"),
            verify_timeout_secs: 90,
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

    // ---- The Retour automatique, run for real under `sh` -------------------
    //
    // The generated script against fake binaries: `--version` prints the
    // version, `daemon` writes the readiness marker then sleeps (or, broken,
    // exits at once). The update command copies the new binary in place and
    // « migrates » the database, as a new version would.

    use crate::adapters::fs_update_journal::FsUpdateJournal;
    use crate::application::ports::UpdateJournal;
    use crate::domain::attempt::AttemptStatus;
    use crate::domain::install::InstallMethod as Method;
    use crate::domain::update_plan::render_update_script;
    use crate::domain::update_snapshot::{SavedFile, UpdateSnapshot};
    use std::time::{Duration, Instant};

    fn fake_binary(path: &Path, version: &str, marker: &Path, starts: bool) {
        use std::os::unix::fs::PermissionsExt;
        let daemon = if starts {
            format!(
                "printf '{{\"version\":\"{version}\",\"pid\":%s}}\\n' $$ > {}; exec sleep 30",
                marker.display()
            )
        } else {
            "echo 'cannot start' >&2; exit 1".to_string()
        };
        std::fs::write(
            path,
            format!(
                "#!/bin/sh\ncase \"$1\" in\n  --version) echo \"pdo {version}\" ;;\n  daemon) {daemon} ;;\nesac\n"
            ),
        )
        .unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn marker_pid(marker: &Path) -> Option<(String, u32)> {
        let text = std::fs::read_to_string(marker).ok()?;
        let v: serde_json::Value = serde_json::from_str(text.trim()).ok()?;
        Some((
            v["version"].as_str()?.to_string(),
            v["pid"].as_u64()? as u32,
        ))
    }

    fn alive(pid: u32) -> bool {
        // SAFETY: signal 0 only checks the pid exists.
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }

    struct Rollback {
        _tmp: tempfile::TempDir,
        journal: FsUpdateJournal,
        exe: PathBuf,
        db: PathBuf,
        marker: PathBuf,
        snapshot_dir: PathBuf,
        old_pid: u32,
        exit: Option<i32>,
    }

    impl Drop for Rollback {
        fn drop(&mut self) {
            if let Some((_, pid)) = marker_pid(&self.marker) {
                unsafe { libc::kill(pid as i32, libc::SIGKILL) };
            }
            unsafe { libc::kill(self.old_pid as i32, libc::SIGKILL) };
        }
    }

    /// Start the « old » daemon (1.0.0), take the snapshot the daemon would,
    /// then run the executor whose update command is `upgrade` (`$NEW` is the
    /// new binary, 2.0.0, that starts or not).
    fn run_update(new_starts: bool, upgrade: &str) -> Rollback {
        let tmp = tempfile::tempdir().unwrap();
        let journal = FsUpdateJournal::new(Some(tmp.path().join("home")));
        let marker = journal.ready_marker_path(5172);
        std::fs::create_dir_all(marker.parent().unwrap()).unwrap();
        let exe = tmp.path().join("bin/pdo");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        fake_binary(&exe, "1.0.0", &marker, true);
        let new = tmp.path().join("new-pdo");
        fake_binary(&new, "2.0.0", &marker, new_starts);
        let db = tmp.path().join("repo/.pdo/pdo.db");
        std::fs::create_dir_all(db.parent().unwrap()).unwrap();
        std::fs::write(&db, "old data").unwrap();
        std::fs::write(db.with_file_name("pdo.db-wal"), "old wal").unwrap();

        // The old daemon, reaped by a thread so a stopped one is gone, not a zombie.
        let mut old = std::process::Command::new(&exe)
            .arg("daemon")
            .spawn()
            .unwrap();
        let old_pid = old.id();
        std::thread::spawn(move || old.wait());
        let deadline = Instant::now() + Duration::from_secs(5);
        while marker_pid(&marker).is_none() {
            assert!(Instant::now() < deadline, "the old daemon never started");
            std::thread::sleep(Duration::from_millis(20));
        }

        // The snapshot, as the daemon takes it before the spawn.
        let snapshot_dir = journal.snapshot_dir();
        std::fs::create_dir_all(&snapshot_dir).unwrap();
        std::fs::copy(&exe, snapshot_dir.join("pdo")).unwrap();
        std::fs::copy(&db, snapshot_dir.join("db-0-pdo.db")).unwrap();
        let snapshot = UpdateSnapshot {
            dir: snapshot_dir.clone(),
            binary: SavedFile {
                original: exe.clone(),
                saved: snapshot_dir.join("pdo"),
            },
            databases: vec![SavedFile {
                original: db.clone(),
                saved: snapshot_dir.join("db-0-pdo.db"),
            }],
        };
        let upgrade = upgrade
            .replace("$NEW", &new.display().to_string())
            .replace("$EXE", &exe.display().to_string())
            .replace("$DB", &db.display().to_string());
        let plan = UpdatePlan {
            attempt_id: "a1".into(),
            method: Method::Declared,
            upgrade_command: upgrade,
            supervision: Supervision::None,
            exe: exe.clone(),
            port: 5172,
            bind: None,
            working_dir: tmp.path().to_path_buf(),
            daemon_pid: old_pid,
            relaunch: vec![exe.display().to_string(), "daemon".into()],
            log_path: journal.log_path("a1"),
            attempt_path: journal.attempt_path(),
            from_version: "1.0.0".into(),
            to_version: Some("2.0.0".into()),
            snapshot: Some(snapshot),
            ready_marker: marker.clone(),
            verify_timeout_secs: 2,
        };
        let script = journal
            .write_script("a1", &render_update_script(&plan))
            .unwrap();
        let exit = DetachedExecutor::new(None)
            .spawn(&plan, &script)
            .unwrap()
            .wait();
        Rollback {
            _tmp: tmp,
            journal,
            exe,
            db,
            marker,
            snapshot_dir,
            old_pid,
            exit,
        }
    }

    #[test]
    fn a_new_version_that_answers_succeeds_and_drops_the_snapshot() {
        let r = run_update(true, "cp $NEW $EXE && printf migrated > $DB");
        let log = r.journal.read_log("a1").unwrap().unwrap_or_default();
        assert_eq!(r.exit, Some(0), "{log}");
        let a = r.journal.read_attempt().unwrap();
        assert_eq!(a.status, AttemptStatus::Succeeded, "{log}");
        assert!(!a.rolled_back);
        assert_eq!(a.reason, None);
        let (version, pid) = marker_pid(&r.marker).unwrap();
        assert_eq!(version, "2.0.0");
        assert!(alive(pid));
        assert!(!r.snapshot_dir.exists(), "one snapshot, removed on success");
        assert_eq!(std::fs::read_to_string(&r.db).unwrap(), "migrated");
        assert!(log.contains("verify   daemon answers v2.0.0"), "{log}");
    }

    #[test]
    fn a_new_version_that_does_not_start_is_rolled_back_with_its_data() {
        let r = run_update(false, "cp $NEW $EXE && printf migrated > $DB");
        let log = r.journal.read_log("a1").unwrap().unwrap_or_default();
        assert_eq!(r.exit, Some(1), "{log}");
        let a = r.journal.read_attempt().unwrap();
        assert_eq!(a.status, AttemptStatus::Failed, "{log}");
        assert!(a.rolled_back);
        assert_eq!(
            a.reason.as_deref(),
            Some(
                "v2.0.0 did not answer within 2 s after the restart. \
                 The previous version and its data were restored."
            )
        );
        // The previous binary and its data are back, and answer.
        assert!(std::fs::read_to_string(&r.exe)
            .unwrap()
            .contains("pdo 1.0.0"));
        assert_eq!(std::fs::read_to_string(&r.db).unwrap(), "old data");
        assert!(
            !r.db.with_file_name("pdo.db-wal").exists(),
            "the WAL of the broken version is dropped"
        );
        let (version, pid) = marker_pid(&r.marker).unwrap();
        assert_eq!(version, "1.0.0");
        assert!(alive(pid));
        assert!(!alive(r.old_pid), "the first daemon was stopped");
        for step in [
            "snapshot 1 database, binary 1.0.0 kept",
            "verify   no answer in 2 s",
            "rollback binary 1.0.0, 1 database",
            "verify   daemon answers v1.0.0",
            "result   failed (rolled back)",
        ] {
            assert!(log.contains(step), "missing `{step}`:\n{log}");
        }
    }

    #[test]
    fn a_failed_update_command_leaves_the_daemon_running_and_restores_nothing() {
        let r = run_update(true, "echo 'no network' >&2; exit 3");
        let log = r.journal.read_log("a1").unwrap().unwrap_or_default();
        assert_eq!(r.exit, Some(3), "{log}");
        let a = r.journal.read_attempt().unwrap();
        assert_eq!(a.status, AttemptStatus::Failed);
        assert!(!a.rolled_back);
        assert_eq!(
            a.reason.as_deref(),
            Some("The update command exited with code 3.")
        );
        assert!(alive(r.old_pid), "the running daemon is untouched");
        assert_eq!(marker_pid(&r.marker).unwrap().1, r.old_pid);
        assert!(log.contains("no network"));
        assert!(!log.contains("rollback"), "{log}");
        assert!(!r.snapshot_dir.exists());
    }
}
