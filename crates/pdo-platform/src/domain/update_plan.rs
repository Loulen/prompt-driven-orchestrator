//! The update plan (#699) and the script that carries it out.
//!
//! « Who updates is never who dies » (Tailscale, Caddy, Syncthing): the daemon does
//! NOT run the upgrade itself — its own restart would kill it midway. It writes a
//! small POSIX shell script and spawns it detached, then answers at once with the
//! attempt id. The script does exactly what the user would type (CONTEXT.md § *Mise
//! à jour depuis l'app*: sharp tool, delegate to the installation method), then
//! checks the result and undoes it when it is broken (the **Retour automatique**):
//!
//! 1. the daemon has already taken the **snapshot** (its databases, the binary in
//!    place, [`UpdateSnapshot`]) before spawning the script;
//! 2. the **installation method's** upgrade command (`brew update && brew upgrade
//!    Loulen/tap/pdo`, the cargo-dist installer re-run, or the command a
//!    distribution's release channel declares, #24) — a non-zero exit stops
//!    there, the running daemon untouched, nothing restored;
//! 3. if **supervised**: `pdo service install` (idempotent, port preserved — and,
//!    since #699, writing the STABLE binary path so an existing unit pointing into
//!    a versioned Cellar is repaired), then a service restart;
//!    if **not supervised**: stop the daemon by pid and relaunch it with its
//!    original command line (same port), itself detached;
//! 4. **verify**: wait, at most `verify_timeout_secs`, for the daemon to answer
//!    in a version other than the one it started from (its readiness marker,
//!    [`super::update_snapshot::ReadyMarker`]). Only then is the attempt
//!    `succeeded`, and the snapshot removed;
//! 5. otherwise **roll back**: stop the daemon, copy the binary and the databases
//!    back, restart, wait for it to answer, and record `failed` with the reason.
//!
//! The release channel's identifier reaches the update command through the
//! executor's environment (`PDO_GITHUB_TOKEN`), never through the plan: the
//! script, and so its log, cannot show it.
//!
//! Everything here is a pure function of the plan, so the script is golden-tested.

use std::net::IpAddr;
use std::path::PathBuf;

use super::attempt::ATTEMPT_SCHEMA;
use super::install::{InstallMethod, Supervision};
use super::update_rollback::{rollback_function, start_daemon, STOP_PID, WAIT_READY};
use super::update_snapshot::UpdateSnapshot;

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
    /// The newest version known at apply time, for the log and the record.
    pub to_version: Option<String>,
    /// What the Retour automatique puts back; `None`: nothing was kept, a
    /// broken update is reported, not undone.
    pub snapshot: Option<UpdateSnapshot>,
    /// The readiness marker the restarted daemon writes once it serves.
    pub ready_marker: PathBuf,
    /// How long the script waits for the restarted daemon to answer.
    pub verify_timeout_secs: u64,
    /// The release identifier the executor receives (`PDO_GITHUB_TOKEN`) is
    /// the binary's own: the script drops it from its environment once the
    /// update command has run, so neither the restarted daemon nor the
    /// Retour automatique inherits it. `false`: it is the operator's, which
    /// the daemon was started with and keeps.
    pub release_token_for_command_only: bool,
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

/// `<exe> service install --port <port> [--bind <ip>]`: (re)writes the unit
/// for the binary in place, the port and the bind kept.
pub(crate) fn service_install(plan: &UpdatePlan) -> String {
    let bind_arg = plan
        .bind
        .map(|ip| format!(" --bind {}", sh_quote(&ip.to_string())))
        .unwrap_or_default();
    format!(
        "{} service install --port {}{bind_arg}",
        q(&plan.exe),
        plan.port
    )
}

/// A path, quoted for `sh`.
pub(crate) fn q(path: &std::path::Path) -> String {
    sh_quote(&path.display().to_string())
}

/// A string for a double-quoted `sh` argument: `"`, `\`, `$` and backquotes
/// escaped.
pub(crate) fn dq(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$")
        .replace('`', "\\`")
}

/// The `record` function: rewrites `last-attempt.json` (the daemon wrote it as
/// `running`; the script owns the end state since the daemon dies before it).
/// `record <status> <exit_code> [reason] [rolled_back]`; a reason is plain text
/// the script composes, never with a `"` or a `\`.
fn record_function(plan: &UpdatePlan) -> String {
    let to_version = match &plan.to_version {
        Some(v) => format!("\\\"{}\\\"", dq(&json_escape(v))),
        None => "null".to_string(),
    };
    let json = format!(
        "{{\\\"schema\\\":\\\"{schema}\\\",\\\"attempt_id\\\":\\\"{id}\\\",\\\"status\\\":\\\"$1\\\",\
         \\\"started_at\\\":\\\"$STARTED_AT\\\",\\\"finished_at\\\":\\\"$(date -u +%Y-%m-%dT%H:%M:%SZ)\\\",\
         \\\"exit_code\\\":$2,\\\"method\\\":\\\"{method}\\\",\\\"command\\\":\\\"{cmd}\\\",\
         \\\"supervision\\\":\\\"{sup}\\\",\\\"log_path\\\":\\\"{log}\\\",\\\"from_version\\\":\\\"{from}\\\",\
         \\\"to_version\\\":{to_version},\\\"reason\\\":$r,\\\"rolled_back\\\":${{4:-false}},\
         \\\"verify_timeout_secs\\\":{timeout}}}",
        schema = ATTEMPT_SCHEMA,
        id = dq(&json_escape(&plan.attempt_id)),
        method = plan.method.as_str(),
        cmd = dq(&json_escape(plan.upgrade_command())),
        sup = plan.supervision.as_str(),
        log = dq(&json_escape(&plan.log_path.display().to_string())),
        from = dq(&json_escape(&plan.from_version)),
        timeout = plan.verify_timeout_secs,
    );
    format!(
        "record() {{\n  \
         if [ -n \"${{3:-}}\" ]; then r=\"\\\"$3\\\"\"; else r=null; fi\n  \
         printf '%s\\n' \"{json}\" > \"$ATTEMPT_FILE.tmp\" && mv \"$ATTEMPT_FILE.tmp\" \"$ATTEMPT_FILE\"\n\
         }}\n"
    )
}

/// The shell filter that reads a version out of `--version`: the first word
/// shaped like one (`X.Y.Z…`), since a Distribution appends the core's version
/// (CONTEXT.md § Canal de mise à jour), reduced to the characters a version
/// uses (it goes into the record's JSON). POSIX awk: dash, busybox, BSD.
const VERSION_WORD: &str = "awk '{for(i=1;i<=NF;i++) if($i ~ /^[0-9]+\\.[0-9]+\\.[0-9]+/){print $i; exit}}' | tr -cd 'A-Za-z0-9._+-'";

/// Render the executor script. Pure: identical plans give identical bytes.
///
/// Shape (POSIX `sh`, no bashisms — the FP container's `sh` is dash):
/// * a `PATH` widened with the exe dir, `~/.local/bin` and the Homebrew prefixes,
///   because a daemon started by systemd has a minimal `PATH` without `brew`;
/// * `say <step> <text>` writes one log line, `HH:MM:SS step text`;
/// * `record` rewrites `last-attempt.json` (see [`record_function`]);
/// * the upgrade command — a non-zero exit records `failed` and stops there: the
///   old daemon keeps running, nothing is restarted nor restored;
/// * supervised: `pdo service install --port <port>` from the daemon's cwd, then the
///   supervisor's restart; unsupervised: SIGTERM the pid, wait until it is gone,
///   relaunch the recorded argv in a new session with stdio to the log;
/// * the verification, then the success or the Retour automatique.
pub fn render_update_script(plan: &UpdatePlan) -> String {
    let exe_dir = plan
        .exe
        .parent()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| ".".to_string());
    let exe = q(&plan.exe);

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
    s.push_str(&format!("ATTEMPT_FILE={}\n", q(&plan.attempt_path)));
    s.push_str(&format!("MARKER={}\n", q(&plan.ready_marker)));
    s.push_str(&format!("TIMEOUT={}\n", plan.verify_timeout_secs));
    s.push_str(&format!("FROM={}\n", sh_quote(&plan.from_version)));
    s.push_str("STARTED_AT=$(date -u +%Y-%m-%dT%H:%M:%SZ)\n");
    s.push_str(&record_function(plan));
    s.push_str("say() {\n  printf '%s %-8s %s\\n' \"$(date +%H:%M:%S)\" \"$1\" \"$2\"\n}\n");
    s.push_str(WAIT_READY);
    s.push_str(STOP_PID);
    if let Some(snapshot) = &plan.snapshot {
        s.push_str(&rollback_function(plan, snapshot));
    } else {
        // Nothing kept: a broken update is reported, never undone.
        s.push_str("rollback() {\n  say result \"failed (nothing to restore)\"\n  record failed 1 \"$1\" false\n  exit 1\n}\n");
    }
    s.push_str(&format!(
        "say start \"pdo update {} from v{} · method {} · supervision {}\"\n",
        dq(&plan.attempt_id),
        dq(&plan.from_version),
        plan.method.as_str(),
        plan.supervision.as_str()
    ));
    if let Some(to) = &plan.to_version {
        s.push_str(&format!("say target \"v{}\"\n", dq(to)));
    }
    match &plan.snapshot {
        Some(snapshot) => s.push_str(&format!(
            "say snapshot \"{}\"\n",
            dq(&snapshot.summary(&plan.from_version))
        )),
        None => s.push_str("say snapshot \"none: a broken update cannot be undone\"\n"),
    }
    let clean_snapshot = plan
        .snapshot
        .as_ref()
        .map(|snap| format!("rm -rf {}\n", q(&snap.dir)))
        .unwrap_or_default();

    // 2. The upgrade command.
    s.push_str(&format!("say run \"{}\"\n", dq(plan.upgrade_command())));
    s.push_str(&format!(
        "sh -c {}\nrc=$?\n",
        sh_quote(plan.upgrade_command())
    ));
    if plan.release_token_for_command_only {
        // The release identifier was for the update command alone.
        s.push_str("unset PDO_GITHUB_TOKEN\n");
    }
    s.push_str("if [ \"$rc\" -ne 0 ]; then\n");
    s.push_str(
        "  say result \"failed: the update command exited $rc; the running daemon is left untouched\"\n",
    );
    for line in clean_snapshot.lines() {
        s.push_str(&format!("  {line}\n"));
    }
    s.push_str("  record failed \"$rc\" \"The update command exited with code $rc.\"\n  exit \"$rc\"\nfi\n");
    // The version the new binary says it is.
    s.push_str(&format!(
        "NEW=$({exe} --version 2>/dev/null | {VERSION_WORD})\n"
    ));
    s.push_str("say installed \"${NEW:-unknown}\"\n");
    // How the reason names the new version: the binary's own word, else the
    // newest known release, else no version at all.
    match &plan.to_version {
        Some(to) => s.push_str(&format!("LABEL=\"v${{NEW:-{}}}\"\n", dq(to))),
        None => s.push_str(
            "if [ -n \"$NEW\" ]; then LABEL=\"v$NEW\"; else LABEL=\"The new version\"; fi\n",
        ),
    }

    // 3. The restart.
    s.push_str(&format!(
        "cd {} || rollback \"Cannot enter the daemon's working directory.\"\n",
        q(&plan.working_dir)
    ));
    match plan.supervision {
        Supervision::Systemd | Supervision::Launchd => {
            s.push_str(&format!(
                "say unit \"reinstalling the service unit (stable path {})\"\n",
                dq(&plan.exe.display().to_string())
            ));
            s.push_str(&format!("{}\nrc=$?\n", service_install(plan)));
            s.push_str("if [ \"$rc\" -ne 0 ]; then\n");
            s.push_str("  say unit \"service install failed (exit $rc)\"\n");
            s.push_str("  rollback \"The service install failed (exit $rc).\"\nfi\n");
        }
        Supervision::None => {
            s.push_str(&format!(
                "say stop \"daemon pid {} · tmux sessions survive\"\n",
                plan.daemon_pid
            ));
            s.push_str(&format!("stop_pid {}\n", plan.daemon_pid));
        }
    }
    s.push_str(&start_daemon(plan));
    s.push_str("if [ \"$rc\" -ne 0 ]; then\n");
    s.push_str("  say restart \"failed (exit $rc)\"\n");
    s.push_str("  rollback \"The restart failed (exit $rc).\"\nfi\n");

    // 4. The verification.
    s.push_str("if wait_ready \"$TIMEOUT\"; then\n");
    s.push_str("  if [ \"$READY\" != \"$FROM\" ]; then\n");
    s.push_str("    say verify \"daemon answers v$READY\"\n");
    for line in clean_snapshot.lines() {
        s.push_str(&format!("    {line}\n"));
    }
    s.push_str("    record succeeded 0\n");
    s.push_str("    say result succeeded\n");
    s.push_str("    exit 0\n");
    s.push_str("  fi\n");
    s.push_str("  say verify \"daemon answers v$FROM: the binary did not change\"\n");
    s.push_str("  rollback \"The daemon came back on v$FROM: the binary did not change.\"\n");
    s.push_str("fi\n");
    s.push_str("say verify \"no answer in $TIMEOUT s\"\n");
    s.push_str("rollback \"$LABEL did not answer within $TIMEOUT s after the restart.\"\n");
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::update_snapshot::SavedFile;

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
            to_version: Some("1.61.0".into()),
            snapshot: Some(UpdateSnapshot {
                dir: PathBuf::from("/home/u/.pdo/update/snapshot"),
                binary: SavedFile {
                    original: PathBuf::from("/home/linuxbrew/.linuxbrew/bin/pdo"),
                    saved: PathBuf::from("/home/u/.pdo/update/snapshot/pdo"),
                },
                databases: vec![
                    SavedFile {
                        original: PathBuf::from("/home/u/.pdo/app/.pdo/pdo.db"),
                        saved: PathBuf::from("/home/u/.pdo/update/snapshot/db-0-pdo.db"),
                    },
                    SavedFile {
                        original: PathBuf::from("/home/u/.pdo/extensions/acme/acme.db"),
                        saved: PathBuf::from("/home/u/.pdo/update/snapshot/db-1-acme.db"),
                    },
                ],
            }),
            ready_marker: PathBuf::from("/home/u/.pdo/update/daemon-5172.json"),
            verify_timeout_secs: 90,
            release_token_for_command_only: false,
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
        let restart = s.find("\nsystemctl --user restart pdo\n").unwrap();
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
            !s.contains("stop_pid 4242"),
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
    fn launchd_rollback_boots_the_job_out_before_restoring() {
        let s = render_update_script(&plan(InstallMethod::Homebrew, Supervision::Launchd));
        let body = &s[s.find("rollback() {").unwrap()..];
        let bootout = body
            .find("launchctl bootout \"gui/$(id -u)/com.pdo.daemon\"")
            .expect("KeepAlive would relaunch a job merely killed");
        let wait = body.find("then stop_pid").unwrap();
        let restore = body.find("cp -f").unwrap();
        let install = body.find("service install --port 5172").unwrap();
        assert!(bootout < wait && wait < restore && restore < install);
        assert!(!body.contains("launchctl kill"));
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
        assert!(
            s.contains("stop_pid 4242"),
            "stops the pid and waits for it"
        );
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
        assert!(s.contains("method declared"));
        assert!(s.contains("\\\"method\\\":\\\"declared\\\""));
        assert!(!s.contains("brew upgrade"), "never the vanilla command");
    }

    /// The order the Retour automatique rests on: snapshot reported, command,
    /// restart, verification; success only after it; rollback otherwise.
    #[test]
    fn the_attempt_succeeds_only_once_the_new_version_answers() {
        let s = render_update_script(&plan(InstallMethod::Homebrew, Supervision::Systemd));
        let at = |needle: &str| {
            s.find(needle)
                .unwrap_or_else(|| panic!("missing `{needle}`:\n{s}"))
        };
        let snapshot = at("say snapshot \"2 databases, binary 1.60.0 kept\"");
        let command = at("sh -c 'brew update");
        let restart = at("\nsystemctl --user restart pdo\n");
        let verify = at("if wait_ready \"$TIMEOUT\"; then\n  if [ \"$READY\" != \"$FROM\" ]");
        let succeeded = at("    record succeeded 0\n");
        assert!(snapshot < command && command < restart && restart < verify && verify < succeeded);
        assert!(s.contains("TIMEOUT=90\n"));
        assert!(s.contains("MARKER=/home/u/.pdo/update/daemon-5172.json\n"));
        assert_eq!(
            s.matches("record succeeded").count(),
            1,
            "never succeeded before the verification"
        );
        assert!(
            s[verify..succeeded].contains("rm -rf /home/u/.pdo/update/snapshot\n"),
            "the snapshot is removed after a success"
        );
        assert!(s.contains("LABEL=\"v${NEW:-1.61.0}\"\n"));
        assert!(s.ends_with(
            "say verify \"no answer in $TIMEOUT s\"\n\
             rollback \"$LABEL did not answer within $TIMEOUT s after the restart.\"\n"
        ));
    }

    #[test]
    fn the_rollback_puts_the_binary_and_every_database_back_then_restarts() {
        let s = render_update_script(&plan(InstallMethod::Homebrew, Supervision::Systemd));
        let body = &s[s.find("rollback() {").unwrap()..];
        let body = &body[..body.find("\n}\n").unwrap()];
        let stop = body.find("systemctl --user stop pdo").unwrap();
        let binary = body
            .find("cp -f /home/u/.pdo/update/snapshot/pdo /home/linuxbrew/.linuxbrew/bin/pdo")
            .unwrap();
        let core_db = body
            .find(
                "cp -f /home/u/.pdo/update/snapshot/db-0-pdo.db /home/u/.pdo/app/.pdo/pdo.db \
                 && rm -f /home/u/.pdo/app/.pdo/pdo.db-wal /home/u/.pdo/app/.pdo/pdo.db-shm",
            )
            .unwrap();
        assert!(body.contains("db-1-acme.db /home/u/.pdo/extensions/acme/acme.db"));
        let unit = body
            .find("cd /home/u/.pdo/app && /home/linuxbrew/.linuxbrew/bin/pdo service install --port 5172")
            .expect("the restored binary writes its own unit");
        let restart = body.find("systemctl --user restart pdo").unwrap();
        assert!(stop < binary && binary < core_db && core_db < unit && unit < restart);
        assert!(body.contains(
            "record failed 1 \"$1 The previous version and its data were restored.\" true"
        ));
        assert!(body.contains("exit 1"));
    }

    #[test]
    fn a_failed_update_command_restores_nothing() {
        let s = render_update_script(&plan(InstallMethod::Homebrew, Supervision::None));
        let failed = &s[s.find("if [ \"$rc\" -ne 0 ]; then").unwrap()..];
        let failed = &failed[..failed.find("\nfi\n").unwrap()];
        assert!(
            failed.contains("record failed \"$rc\" \"The update command exited with code $rc.\"")
        );
        assert!(failed.contains("rm -rf /home/u/.pdo/update/snapshot"));
        assert!(!failed.contains("rollback"), "the daemon is untouched");
        assert!(!failed.contains("stop_pid"), "the daemon is untouched");
    }

    #[test]
    fn the_same_version_coming_back_is_rolled_back_too() {
        let s = render_update_script(&plan(InstallMethod::Script, Supervision::None));
        assert!(
            s.contains("rollback \"The daemon came back on v$FROM: the binary did not change.\"")
        );
        assert!(s.contains("& NEW_PID=$!"), "the relaunched pid is kept");
        assert!(
            s.contains("stop_pid \"${NEW_PID:-}\""),
            "and stopped on a rollback"
        );
    }

    #[test]
    fn without_a_snapshot_a_broken_update_is_reported_not_undone() {
        let mut plan = plan(InstallMethod::Homebrew, Supervision::Systemd);
        plan.snapshot = None;
        plan.to_version = None;
        let s = render_update_script(&plan);
        assert!(s.contains("record failed 1 \"$1\" false"));
        // No version known: the reason names none, never « vthe new version ».
        assert!(s.contains("else LABEL=\"The new version\"; fi"));
        assert!(!s.contains("cp -f"));
        assert!(!s.contains("rm -rf"));
    }

    #[test]
    fn a_command_with_shell_characters_lands_verbatim_in_the_record() {
        let mut plan = plan(InstallMethod::Declared, Supervision::None);
        plan.upgrade_command = "acme-up --token \"$TOKEN\" `id` \\x".into();
        let s = render_update_script(&plan);
        assert!(
            s.contains(r#"\"command\":\"acme-up --token \\\"\$TOKEN\\\" \`id\` \\\\x\""#),
            "{s}"
        );
    }

    #[test]
    fn sh_quote_is_safe() {
        assert_eq!(sh_quote("/usr/bin/pdo"), "/usr/bin/pdo");
        assert_eq!(sh_quote("a b"), "'a b'");
        assert_eq!(sh_quote("it's"), "'it'\\''s'");
        assert_eq!(sh_quote(""), "''");
    }

    #[test]
    fn a_binarys_release_identifier_is_dropped_right_after_the_update_command() {
        let mut plan = plan(InstallMethod::Declared, Supervision::None);
        let without = render_update_script(&plan);
        assert!(
            !without.contains("PDO_GITHUB_TOKEN"),
            "the operator's identifier stays with the restarted daemon"
        );
        plan.release_token_for_command_only = true;
        let s = render_update_script(&plan);
        let command = s.find("sh -c acme-upgrade\nrc=$?\n").unwrap();
        let unset = s.find("\nunset PDO_GITHUB_TOKEN\n").unwrap();
        let rc_check = s.find("if [ \"$rc\" -ne 0 ]; then").unwrap();
        let restart = s.find("stop_pid 4242\n").unwrap();
        assert!(command < unset && unset < rc_check && unset < restart);
        assert_eq!(s.matches("PDO_GITHUB_TOKEN").count(), 1, "{s}");
    }

    /// The script reads the new version from the first word shaped like one:
    /// a Distribution prints `<name> <version> (core PDO <core>)`.
    #[test]
    fn the_new_version_is_the_first_version_shaped_word() {
        let s = render_update_script(&plan(InstallMethod::Homebrew, Supervision::Systemd));
        assert!(
            s.contains(&format!(
                "NEW=$(/home/linuxbrew/.linuxbrew/bin/pdo --version 2>/dev/null | {VERSION_WORD})\n"
            )),
            "{s}"
        );
        assert!(s.contains(
            "NEW=$(/home/linuxbrew/.linuxbrew/bin/pdo --version 2>/dev/null | \
             awk '{for(i=1;i<=NF;i++) if($i ~ /^[0-9]+\\.[0-9]+\\.[0-9]+/){print $i; exit}}' | \
             tr -cd 'A-Za-z0-9._+-')\n"
        ));
        assert!(!s.contains("$NF"), "the last word is no longer read:\n{s}");
        assert!(s.contains("say installed \"${NEW:-unknown}\"\n"));
        assert!(s.contains("LABEL=\"v${NEW:-1.61.0}\"\n"));
    }

    /// [`VERSION_WORD`] run for real under `sh`, on what binaries print.
    #[test]
    fn the_version_word_is_read_from_vanilla_and_distribution_outputs() {
        for (printed, expected) in [
            ("acme-pdo 0.3.0 (core PDO 2.89.0)", "0.3.0"),
            ("Acogni Driver 0.89.0 (core PDO 2.89.0)", "0.89.0"),
            ("pdo 2.89.0", "2.89.0"),
            ("pdo 2.90.0-rc.1", "2.90.0-rc.1"),
            ("pdo 2.90.0+build.7", "2.90.0+build.7"),
            ("acme-1.2.3 0.4.0", "0.4.0"),
            ("", ""),
            ("pdo", ""),
        ] {
            let out = std::process::Command::new("sh")
                .arg("-c")
                .arg(format!(
                    "printf '%s\\n' {} | {VERSION_WORD}",
                    sh_quote(printed)
                ))
                .output()
                .unwrap();
            assert!(out.status.success(), "{printed}");
            assert_eq!(String::from_utf8_lossy(&out.stdout), expected, "{printed}");
        }
    }
}
