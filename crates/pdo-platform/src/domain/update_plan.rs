//! The update plan (#699) and the script that carries it out.
//!
//! « Who updates is never who dies » (Tailscale, Caddy, Syncthing): the daemon does
//! NOT run the upgrade itself — its own restart would kill it midway. It writes a
//! small POSIX shell script and spawns it detached, then answers at once with the
//! attempt id. The script does exactly what the user would type (CONTEXT.md § *Mise
//! à jour depuis l'app*: sharp tool, delegate to the installation method):
//!
//! 1. the **installation method's** upgrade command (`brew update && brew upgrade
//!    Loulen/tap/pdo`, the cargo-dist installer re-run, or the command a
//!    distribution's release channel declares, #24);
//! 2. if **supervised**: `pdo service install` (idempotent, port preserved — and,
//!    since #699, writing the STABLE binary path so an existing unit pointing into
//!    a versioned Cellar is repaired), then a service restart;
//! 3. if **not supervised**: stop the daemon by pid and relaunch it with its
//!    original command line (same port), itself detached.
//!
//! Everything here is a pure function of the plan, so the script is golden-tested.

use std::net::IpAddr;
use std::path::PathBuf;

use super::attempt::ATTEMPT_SCHEMA;
use super::install::{InstallMethod, Supervision};

/// Everything the script needs, decided by the daemon at apply time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdatePlan {
    pub attempt_id: String,
    pub method: InstallMethod,
    /// The upgrade command, verbatim: the detected method's, or the one the
    /// release channel declares.
    pub upgrade_command: String,
    pub supervision: Supervision,
    /// The stable path of the daemon binary (what `service install` and the
    /// relaunch invoke): Homebrew's `bin/pdo`, never `Cellar/pdo/<v>/bin/pdo`.
    pub exe: PathBuf,
    pub port: u16,
    /// Explicit bind passed to the daemon, if any. `None` preserves the default.
    pub bind: Option<IpAddr>,
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
    /// The upgrade command, verbatim — the same text the UI shows as the manual
    /// command, so the button runs exactly what the user would type.
    pub fn upgrade_command(&self) -> &str {
        &self.upgrade_command
    }
}

/// The `--bind` a recorded relaunch command carries, if it names one.
pub fn explicit_bind_from_relaunch(relaunch: &[String]) -> Option<IpAddr> {
    relaunch
        .windows(2)
        .find(|pair| pair[0] == "--bind")
        .and_then(|pair| pair[1].parse().ok())
}

/// The command line an unsupervised relaunch runs when the daemon recorded none:
/// `<exe> daemon --port <port>`.
pub fn default_relaunch(exe: &std::path::Path, port: u16) -> Vec<String> {
    vec![
        exe.display().to_string(),
        "daemon".to_string(),
        "--port".to_string(),
        port.to_string(),
    ]
}

/// Single-quote a string for POSIX `sh`: safe against every byte but NUL.
pub fn sh_quote(s: &str) -> String {
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
        method = plan.method.as_str(),
        cmd = json_escape(&json_escape(plan.upgrade_command())),
        sup = plan.supervision.as_str(),
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
        plan.method.as_str(),
        plan.supervision.as_str()
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
            let bind_arg = plan
                .bind
                .map(|ip| format!(" --bind {}", sh_quote(&ip.to_string())))
                .unwrap_or_default();
            s.push_str(&format!(
                "{} service install --port {}{bind_arg}\nrc=$?\n",
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

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(method: InstallMethod, supervision: Supervision) -> UpdatePlan {
        UpdatePlan {
            attempt_id: "20260905-120000-abc123".into(),
            method,
            upgrade_command: method.manual_command().unwrap_or("acme-upgrade").into(),
            supervision,
            exe: PathBuf::from("/home/linuxbrew/.linuxbrew/bin/pdo"),
            port: 5172,
            bind: None,
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
    fn supervised_update_preserves_an_explicit_bind() {
        let mut plan = plan(InstallMethod::Homebrew, Supervision::Systemd);
        plan.bind = Some("127.0.0.1".parse().unwrap());
        let s = render_update_script(&plan);
        assert!(s.contains("service install --port 5172 --bind 127.0.0.1"));
    }

    #[test]
    fn relaunch_bind_is_read_only_when_explicit() {
        let explicit = vec![
            "pdo".into(),
            "daemon".into(),
            "--port".into(),
            "5172".into(),
            "--bind".into(),
            "127.0.0.1".into(),
        ];
        assert_eq!(
            explicit_bind_from_relaunch(&explicit),
            Some("127.0.0.1".parse().unwrap())
        );
        assert_eq!(
            explicit_bind_from_relaunch(&["pdo".into(), "daemon".into()]),
            None
        );
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
    fn a_declared_channel_runs_its_own_command() {
        let s = render_update_script(&plan(InstallMethod::Declared, Supervision::Systemd));
        assert!(s.contains("sh -c acme-upgrade\n"), "{s}");
        assert!(s.contains("== method: declared"));
        assert!(s.contains("\\\"method\\\":\\\"declared\\\""));
        assert!(!s.contains("brew upgrade"), "never the vanilla command");
    }

    #[test]
    fn sh_quote_is_safe() {
        assert_eq!(sh_quote("/usr/bin/pdo"), "/usr/bin/pdo");
        assert_eq!(sh_quote("a b"), "'a b'");
        assert_eq!(sh_quote("it's"), "'it'\\''s'");
        assert_eq!(sh_quote(""), "''");
    }
}
