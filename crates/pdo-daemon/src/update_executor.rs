//! The detached update executor (#699, story #695).
//!
//! « Who updates is never who dies » (Tailscale, Caddy, Syncthing): the daemon does
//! NOT run the upgrade itself — its own restart would kill it midway. It writes a
//! small POSIX shell script under `<home>/.pdo/update/` and spawns it in a **new
//! session** (`setsid`), stdio bound to a log file, then answers at once with the
//! attempt id. The script does exactly what the user would type (CONTEXT.md § *Mise
//! à jour depuis l'app*: sharp tool, delegate to the installation method):
//!
//! 1. the **installation method's** upgrade command (`brew update && brew upgrade
//!    Loulen/tap/pdo`, or the cargo-dist installer re-run);
//! 2. if **supervised**: `pdo service install` (idempotent, port preserved — and,
//!    since #699, writing the STABLE binary path so an existing unit pointing into
//!    a versioned Cellar is repaired), then a service restart;
//! 3. if **not supervised**: stop the daemon by pid and relaunch it with its
//!    original command line (same port), itself detached.
//!
//! The observable success is the new version in the status bar after reconnection;
//! this module only guarantees the **journal** (the log, plus the attempt record
//! `last-attempt.json` with status / dates / exit code) so a failed update is
//! diagnosable in Settings afterwards.
//!
//! Everything but [`spawn_detached`] is a pure function of its inputs, so the script
//! is golden-tested at layer 1; layer 3 runs a real daemon with a **fixture
//! executor** (`PDO_UPDATE_EXECUTOR`) that receives the plan through env variables.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::update_check::{InstallMethod, Supervision};

/// Env seam: a command run INSTEAD of `sh <script>`; it receives the script path as
/// its single argument and the plan as `PDO_UPDATE_*` variables. Tests point it at a
/// fixture that logs what it was asked and exits.
pub const EXECUTOR_OVERRIDE_ENV: &str = "PDO_UPDATE_EXECUTOR";

/// Env seam: force the detected installation method (`homebrew` | `script` |
/// `unknown`). A test binary is neither brewed nor scripted; the FP container is.
pub const INSTALL_METHOD_OVERRIDE_ENV: &str = "PDO_INSTALL_METHOD";

/// Schema tag of `last-attempt.json`.
pub(crate) const ATTEMPT_SCHEMA: &str = "update-attempt-v1";

/// Outcome of an attempt, as the record on disk reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AttemptStatus {
    /// Spawned; the daemon (or the script) has not recorded an end yet.
    Running,
    Succeeded,
    Failed,
}

/// The last update attempt — what Settings › Version & update shows afterwards,
/// including in failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateAttempt {
    #[serde(default)]
    pub schema: String,
    pub attempt_id: String,
    pub status: AttemptStatus,
    /// RFC3339 UTC.
    pub started_at: String,
    #[serde(default)]
    pub finished_at: Option<String>,
    #[serde(default)]
    pub exit_code: Option<i32>,
    pub method: InstallMethod,
    /// The upgrade command the executor ran (verbatim).
    pub command: String,
    pub supervision: Supervision,
    pub log_path: PathBuf,
    /// Version the daemon ran when the attempt started — so the UI can tell « the
    /// daemon came back on the same version » from a real update.
    #[serde(default)]
    pub from_version: String,
}

/// Everything the script needs, decided by the daemon at apply time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdatePlan {
    pub attempt_id: String,
    pub method: InstallMethod,
    pub supervision: Supervision,
    /// The stable path of the daemon binary (what `service install` and the
    /// relaunch invoke): Homebrew's `bin/pdo`, never `Cellar/pdo/<v>/bin/pdo`.
    pub exe: PathBuf,
    pub port: u16,
    /// The daemon's cwd: `service install` derives `WorkingDirectory` from it and
    /// the relaunch must resolve the same repo root.
    pub working_dir: PathBuf,
    /// The running daemon's pid, stopped by the script before an unsupervised
    /// relaunch.
    pub daemon_pid: u32,
    /// The command line the unsupervised relaunch runs, argv form. Defaults to
    /// `<exe> daemon --port <port>` when the daemon recorded nothing else.
    pub relaunch: Vec<String>,
    pub log_path: PathBuf,
    pub attempt_path: PathBuf,
    pub from_version: String,
}

impl UpdatePlan {
    /// The method's upgrade command, verbatim — the same text the UI shows as the
    /// manual command, so the button runs exactly what the user would type.
    pub fn upgrade_command(&self) -> &'static str {
        self.method.manual_command()
    }
}

/// `<home>/.pdo/update/`.
pub(crate) fn update_dir(home_root: &Path) -> PathBuf {
    home_root.join(".pdo").join("update")
}

/// `<home>/.pdo/update/last-attempt.json`.
pub(crate) fn attempt_path(home_root: &Path) -> PathBuf {
    update_dir(home_root).join("last-attempt.json")
}

/// `<home>/.pdo/update/<attempt_id>.log`.
pub(crate) fn log_path(home_root: &Path, attempt_id: &str) -> PathBuf {
    update_dir(home_root).join(format!("{attempt_id}.log"))
}

/// `<home>/.pdo/update/<attempt_id>.sh`.
pub(crate) fn script_path(home_root: &Path, attempt_id: &str) -> PathBuf {
    update_dir(home_root).join(format!("{attempt_id}.sh"))
}

/// A fresh attempt id: UTC timestamp + short random suffix, filesystem-safe.
pub(crate) fn new_attempt_id() -> String {
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let suffix: String = uuid::Uuid::new_v4().simple().to_string()[..6].to_string();
    format!("{stamp}-{suffix}")
}

/// Read the last attempt record, `None` when absent, unreadable or of a foreign
/// schema (never an error: a corrupt record is "no attempt yet").
pub(crate) fn read_attempt(path: &Path) -> Option<UpdateAttempt> {
    let text = std::fs::read_to_string(path).ok()?;
    let a: UpdateAttempt = serde_json::from_str(&text).ok()?;
    (a.schema == ATTEMPT_SCHEMA).then_some(a)
}

/// Write the record atomically (temp + rename), creating the directory.
pub(crate) fn write_attempt(path: &Path, attempt: &UpdateAttempt) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("json.tmp");
    let text = serde_json::to_string_pretty(attempt).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

/// Single-quote a string for POSIX `sh`: safe against every byte but NUL.
pub(crate) fn sh_quote(s: &str) -> String {
    if !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_./:=+@%".contains(&b))
    {
        return s.to_string();
    }
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Escape a string for inclusion inside a JSON string literal written by the script.
fn json_escape(s: &str) -> String {
    serde_json::to_string(s)
        .map(|q| q[1..q.len() - 1].to_string())
        .unwrap_or_default()
}

/// Render the executor script. Pure: identical plans give identical bytes.
///
/// Shape (POSIX `sh`, no bashisms — the FP container's `sh` is dash):
/// * a `PATH` widened with the exe dir, `~/.local/bin` and the Homebrew prefixes,
///   because a daemon started by systemd has a minimal `PATH` without `brew`;
/// * `record <status> <exit_code>` rewrites `last-attempt.json` (the daemon wrote it
///   as `running`; the script owns the end state since the daemon dies before it);
/// * the upgrade command — a non-zero exit records `failed` and stops there: the
///   old daemon keeps running, nothing is restarted onto a half-installed binary;
/// * supervised: `pdo service install --port <port>` from the daemon's cwd, then the
///   supervisor's restart; unsupervised: SIGTERM the pid, wait until it is gone,
///   relaunch the recorded argv in a new session with stdio to the log.
pub fn render_update_script(plan: &UpdatePlan) -> String {
    let exe_dir = plan
        .exe
        .parent()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| ".".to_string());
    let attempt_json_head = format!(
        "{{\\\"schema\\\":\\\"{schema}\\\",\\\"attempt_id\\\":\\\"{id}\\\",\\\"status\\\":\\\"$1\\\",\
         \\\"started_at\\\":\\\"{started}\\\",\\\"finished_at\\\":\\\"$(date -u +%Y-%m-%dT%H:%M:%SZ)\\\",\
         \\\"exit_code\\\":$2,\\\"method\\\":\\\"{method}\\\",\\\"command\\\":\\\"{cmd}\\\",\
         \\\"supervision\\\":\\\"{sup}\\\",\\\"log_path\\\":\\\"{log}\\\",\\\"from_version\\\":\\\"{from}\\\"}}",
        schema = ATTEMPT_SCHEMA,
        id = json_escape(&plan.attempt_id),
        started = "$STARTED_AT",
        method = serde_json::to_value(plan.method)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default(),
        cmd = json_escape(&json_escape(plan.upgrade_command())),
        sup = serde_json::to_value(plan.supervision)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default(),
        log = json_escape(&json_escape(&plan.log_path.display().to_string())),
        from = json_escape(&plan.from_version),
    );

    let mut s = String::new();
    s.push_str("#!/bin/sh\n");
    s.push_str(&format!(
        "# pdo update executor — attempt {} (generated by pdo {}; safe to delete afterwards)\n",
        plan.attempt_id,
        env!("CARGO_PKG_VERSION")
    ));
    s.push_str("set -u\n");
    s.push_str(&format!(
        "export PATH={exe_dir}:$HOME/.local/bin:/home/linuxbrew/.linuxbrew/bin:/opt/homebrew/bin:/usr/local/bin:$PATH\n",
        exe_dir = sh_quote(&exe_dir)
    ));
    s.push_str(&format!(
        "ATTEMPT_FILE={}\n",
        sh_quote(&plan.attempt_path.display().to_string())
    ));
    s.push_str("STARTED_AT=$(date -u +%Y-%m-%dT%H:%M:%SZ)\n");
    s.push_str("record() {\n");
    s.push_str(&format!(
        "  printf '%s\\n' \"{attempt_json_head}\" > \"$ATTEMPT_FILE.tmp\" && mv \"$ATTEMPT_FILE.tmp\" \"$ATTEMPT_FILE\"\n"
    ));
    s.push_str("}\n");
    s.push_str(&format!(
        "echo \"== pdo update {} started $STARTED_AT (from v{})\"\n",
        plan.attempt_id, plan.from_version
    ));
    s.push_str(&format!(
        "echo \"== method: {} · supervision: {}\"\n",
        serde_json::to_value(plan.method)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default(),
        serde_json::to_value(plan.supervision)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default()
    ));
    s.push_str(&format!(
        "echo \"== upgrade: {}\"\n",
        plan.upgrade_command().replace('"', "\\\"")
    ));
    s.push_str(&format!(
        "sh -c {}\nrc=$?\n",
        sh_quote(plan.upgrade_command())
    ));
    s.push_str("if [ \"$rc\" -ne 0 ]; then\n");
    s.push_str(
        "  echo \"== upgrade command failed (exit $rc); the running daemon is left untouched\"\n",
    );
    s.push_str("  record failed \"$rc\"\n  exit \"$rc\"\nfi\n");
    s.push_str(&format!(
        "echo \"== installed: $({} --version 2>/dev/null || echo unknown)\"\n",
        sh_quote(&plan.exe.display().to_string())
    ));

    match plan.supervision {
        Supervision::Systemd | Supervision::Launchd => {
            s.push_str(&format!(
                "cd {} || {{ echo \"== cannot cd to the daemon's working dir\"; record failed 1; exit 1; }}\n",
                sh_quote(&plan.working_dir.display().to_string())
            ));
            s.push_str(&format!(
                "echo \"== reinstalling the service unit (stable path {})\"\n",
                plan.exe.display()
            ));
            s.push_str(&format!(
                "{} service install --port {}\nrc=$?\n",
                sh_quote(&plan.exe.display().to_string()),
                plan.port
            ));
            s.push_str("if [ \"$rc\" -ne 0 ]; then\n");
            s.push_str("  echo \"== service install failed (exit $rc)\"\n  record failed \"$rc\"\n  exit \"$rc\"\nfi\n");
            match plan.supervision {
                Supervision::Systemd => {
                    s.push_str("echo \"== restarting: systemctl --user restart pdo\"\n");
                    s.push_str("record succeeded 0\n");
                    s.push_str("systemctl --user restart pdo\nrc=$?\n");
                }
                _ => {
                    s.push_str(
                        "echo \"== restarting: launchctl kickstart -k gui/$(id -u)/com.pdo.daemon\"\n",
                    );
                    s.push_str("record succeeded 0\n");
                    s.push_str("launchctl kickstart -k \"gui/$(id -u)/com.pdo.daemon\"\nrc=$?\n");
                }
            }
            s.push_str("if [ \"$rc\" -ne 0 ]; then\n");
            s.push_str("  echo \"== restart failed (exit $rc)\"\n  record failed \"$rc\"\n  exit \"$rc\"\nfi\n");
            s.push_str("echo \"== done; the service comes back on the new binary\"\n");
        }
        Supervision::None => {
            s.push_str(&format!(
                "cd {} || {{ echo \"== cannot cd to the daemon's working dir\"; record failed 1; exit 1; }}\n",
                sh_quote(&plan.working_dir.display().to_string())
            ));
            s.push_str(&format!(
                "echo \"== stopping the daemon (pid {}) — tmux sessions survive\"\n",
                plan.daemon_pid
            ));
            s.push_str(&format!("kill -TERM {} 2>/dev/null\n", plan.daemon_pid));
            s.push_str("i=0\n");
            s.push_str(&format!(
                "while kill -0 {} 2>/dev/null && [ \"$i\" -lt 100 ]; do sleep 0.1; i=$((i+1)); done\n",
                plan.daemon_pid
            ));
            s.push_str(&format!(
                "if kill -0 {pid} 2>/dev/null; then echo \"== daemon still alive, SIGKILL\"; kill -KILL {pid} 2>/dev/null; sleep 0.5; fi\n",
                pid = plan.daemon_pid
            ));
            let argv: Vec<String> = plan.relaunch.iter().map(|a| sh_quote(a)).collect();
            s.push_str(&format!(
                "echo \"== relaunching: {}\"\n",
                plan.relaunch.join(" ").replace('"', "\\\"")
            ));
            s.push_str("record succeeded 0\n");
            // `setsid` when present (util-linux); otherwise a subshell in the
            // background is enough for a daemon whose parent — this script — ends.
            s.push_str(&format!(
                "if command -v setsid >/dev/null 2>&1; then setsid {argv} </dev/null >>{log} 2>&1 & else ({argv} </dev/null >>{log} 2>&1 &) fi\n",
                argv = argv.join(" "),
                log = sh_quote(&plan.log_path.display().to_string())
            ));
            s.push_str("echo \"== done; the daemon comes back on the new binary\"\n");
        }
    }
    s.push_str("exit 0\n");
    s
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
pub(crate) fn spawn_detached(
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
        .env(
            "PDO_UPDATE_METHOD",
            serde_json::to_value(plan.method)
                .ok()
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_default(),
        )
        .env(
            "PDO_UPDATE_SUPERVISION",
            serde_json::to_value(plan.supervision)
                .ok()
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_default(),
        )
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

    fn plan(method: InstallMethod, supervision: Supervision) -> UpdatePlan {
        UpdatePlan {
            attempt_id: "20260905-120000-abc123".into(),
            method,
            supervision,
            exe: PathBuf::from("/home/linuxbrew/.linuxbrew/bin/pdo"),
            port: 5172,
            working_dir: PathBuf::from("/home/u/.pdo/app"),
            daemon_pid: 4242,
            relaunch: vec![
                "/home/linuxbrew/.linuxbrew/bin/pdo".into(),
                "daemon".into(),
                "--port".into(),
                "5172".into(),
            ],
            log_path: PathBuf::from("/home/u/.pdo/update/20260905-120000-abc123.log"),
            attempt_path: PathBuf::from("/home/u/.pdo/update/last-attempt.json"),
            from_version: "1.60.0".into(),
        }
    }

    #[test]
    fn homebrew_supervised_script_upgrades_reinstalls_the_unit_then_restarts() {
        let s = render_update_script(&plan(InstallMethod::Homebrew, Supervision::Systemd));
        assert!(s.starts_with("#!/bin/sh\n"));
        assert!(s.contains("sh -c 'brew update && brew upgrade Loulen/tap/pdo'"));
        let up = s.find("brew upgrade").unwrap();
        let install = s
            .find("/home/linuxbrew/.linuxbrew/bin/pdo service install --port 5172")
            .expect("idempotent service install through the STABLE path");
        let restart = s.find("systemctl --user restart pdo").unwrap();
        assert!(
            up < install && install < restart,
            "order: upgrade → install → restart"
        );
        assert!(
            s.contains("cd /home/u/.pdo/app"),
            "install from the daemon's cwd"
        );
        assert!(!s.contains("Cellar"), "never the versioned target");
        assert!(
            !s.contains("kill -TERM"),
            "supervised: the supervisor restarts"
        );
        assert!(
            s.contains("record failed \"$rc\""),
            "a failed upgrade is recorded"
        );
        assert!(s.contains("update-attempt-v1"));
        assert!(s.contains("/home/linuxbrew/.linuxbrew/bin:/opt/homebrew/bin"));
    }

    #[test]
    fn launchd_uses_kickstart() {
        let s = render_update_script(&plan(InstallMethod::Homebrew, Supervision::Launchd));
        assert!(s.contains("launchctl kickstart -k \"gui/$(id -u)/com.pdo.daemon\""));
        assert!(!s.contains("systemctl"));
    }

    #[test]
    fn unsupervised_script_stops_the_pid_and_relaunches_the_recorded_argv() {
        let s = render_update_script(&plan(InstallMethod::Script, Supervision::None));
        assert!(s.contains("pdo-daemon-installer.sh | sh"));
        assert!(s.contains("kill -TERM 4242"));
        assert!(s.contains("kill -0 4242"), "waits for the pid to be gone");
        assert!(
            s.contains("setsid /home/linuxbrew/.linuxbrew/bin/pdo daemon --port 5172"),
            "same arguments (port):\n{s}"
        );
        assert!(
            !s.contains("service install"),
            "no unit when not supervised"
        );
        assert!(!s.contains("systemctl"));
    }

    #[test]
    fn sh_quote_is_safe() {
        assert_eq!(sh_quote("/usr/bin/pdo"), "/usr/bin/pdo");
        assert_eq!(sh_quote("a b"), "'a b'");
        assert_eq!(sh_quote("it's"), "'it'\\''s'");
        assert_eq!(sh_quote(""), "''");
    }

    #[test]
    fn attempt_record_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = attempt_path(dir.path());
        assert!(read_attempt(&path).is_none());
        let a = UpdateAttempt {
            schema: ATTEMPT_SCHEMA.into(),
            attempt_id: "x".into(),
            status: AttemptStatus::Running,
            started_at: "2026-09-05T12:00:00Z".into(),
            finished_at: None,
            exit_code: None,
            method: InstallMethod::Homebrew,
            command: "brew update && brew upgrade Loulen/tap/pdo".into(),
            supervision: Supervision::None,
            log_path: log_path(dir.path(), "x"),
            from_version: "1.60.0".into(),
        };
        write_attempt(&path, &a).unwrap();
        assert_eq!(read_attempt(&path), Some(a));
        std::fs::write(&path, r#"{"schema":"other"}"#).unwrap();
        assert!(read_attempt(&path).is_none());
    }

    /// The script's `record` line must produce JSON the daemon reads back: run the
    /// function under a real `sh` against a tempdir.
    #[test]
    fn record_function_writes_a_readable_attempt() {
        let dir = tempfile::tempdir().unwrap();
        let mut p = plan(InstallMethod::Homebrew, Supervision::None);
        p.attempt_path = attempt_path(dir.path());
        p.log_path = log_path(dir.path(), &p.attempt_id);
        std::fs::create_dir_all(update_dir(dir.path())).unwrap();
        let script = render_update_script(&p);
        // Keep only the preamble + `record`, then call it.
        let head: String = script
            .lines()
            .take_while(|l| !l.starts_with("echo \"== pdo update"))
            .map(|l| format!("{l}\n"))
            .collect();
        let probe = format!("{head}record failed 7\n");
        let out = std::process::Command::new("sh")
            .arg("-c")
            .arg(&probe)
            .env("HOME", dir.path())
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let a = read_attempt(&p.attempt_path).expect("readable JSON");
        assert_eq!(a.status, AttemptStatus::Failed);
        assert_eq!(a.exit_code, Some(7));
        assert_eq!(a.attempt_id, p.attempt_id);
        assert_eq!(a.command, "brew update && brew upgrade Loulen/tap/pdo");
        assert_eq!(a.method, InstallMethod::Homebrew);
        assert_eq!(a.log_path, p.log_path);
        assert!(a.finished_at.is_some());
    }
}
