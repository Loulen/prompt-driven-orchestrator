//! The Retour automatique part of the executor script ([`super::update_plan`]):
//! waiting for the restarted daemon to answer, (re)starting it on whatever
//! binary is in place, and the `rollback` function that puts the snapshot back.
//! Pure text, golden-tested through [`super::update_plan::render_update_script`].

use super::install::Supervision;
use super::update_plan::{dq, q, service_command, sh_quote, UpdatePlan};
use super::update_snapshot::UpdateSnapshot;

/// `wait_ready <seconds>`: wait for a readiness marker whose daemon is alive;
/// sets `READY` to the version it answers in. `sleep 0.1` ticks.
pub(crate) const WAIT_READY: &str = "wait_ready() {
  READY=
  n=0
  while [ \"$n\" -lt $(($1 * 10)) ]; do
    if [ -f \"$MARKER\" ]; then
      v=$(sed -n 's/.*\"version\":\"\\([^\"]*\\)\".*/\\1/p' \"$MARKER\")
      p=$(sed -n 's/.*\"pid\":\\([0-9][0-9]*\\).*/\\1/p' \"$MARKER\")
      if [ -n \"$v\" ] && [ -n \"$p\" ] && kill -0 \"$p\" 2>/dev/null; then READY=$v; return 0; fi
    fi
    sleep 0.1
    n=$((n+1))
  done
  return 1
}
";

/// How the update restarts a launchd job: the plist `service refresh` may
/// have rewritten is read again only when the job is booted out then in
/// again; `kickstart -k` would restart it on the plist loaded before.
pub(crate) const RELOAD_PLIST: bool = true;

/// The lines that (re)start the daemon on whatever binary is in place, from
/// the daemon's working dir; `$rc` is the restart's exit code. Unsupervised,
/// `NEW_PID` is the relaunched daemon. Under launchd, `reload_plist` boots
/// the job out and in again (the update), otherwise `kickstart -k` (the
/// rollback, whose `service install` has just bootstrapped the plist).
pub(crate) fn start_daemon(plan: &UpdatePlan, reload_plist: bool) -> String {
    let mut s = String::new();
    s.push_str("rm -f \"$MARKER\"\n");
    match plan.supervision {
        Supervision::Systemd => {
            s.push_str("say restart \"systemctl --user restart pdo\"\n");
            s.push_str("systemctl --user restart pdo\nrc=$?\n");
        }
        Supervision::Launchd if !reload_plist => {
            s.push_str("say restart \"launchctl kickstart -k gui/$(id -u)/com.pdo.daemon\"\n");
            s.push_str("launchctl kickstart -k \"gui/$(id -u)/com.pdo.daemon\"\nrc=$?\n");
        }
        Supervision::Launchd => {
            s.push_str(
                "say restart \"launchctl bootout + bootstrap gui/$(id -u)/com.pdo.daemon\"\n",
            );
            s.push_str("launchctl bootout \"gui/$(id -u)/com.pdo.daemon\" 2>/dev/null\n");
            s.push_str(
                "launchctl bootstrap \"gui/$(id -u)\" \"$HOME/Library/LaunchAgents/com.pdo.daemon.plist\"\nrc=$?\n",
            );
        }
        Supervision::None => {
            let argv: Vec<String> = plan.relaunch.iter().map(|a| sh_quote(a)).collect();
            s.push_str(&format!(
                "say restart \"{}\"\n",
                dq(&plan.relaunch.join(" "))
            ));
            // `setsid` when present (util-linux); otherwise a subshell in the
            // background is enough for a daemon whose parent — this script — ends.
            s.push_str(&format!(
                "if command -v setsid >/dev/null 2>&1; then setsid {argv} </dev/null >>{log} 2>&1 & NEW_PID=$!; else {argv} </dev/null >>{log} 2>&1 & NEW_PID=$!; fi\nrc=0\n",
                argv = argv.join(" "),
                log = q(&plan.log_path),
            ));
        }
    }
    s
}

/// Stop whatever daemon the marker names, and wait until it is gone.
const STOP_MARKED: &str = "if [ -f \"$MARKER\" ]; then stop_pid \"$(sed -n 's/.*\"pid\":\\([0-9][0-9]*\\).*/\\1/p' \"$MARKER\")\"; fi\n";

/// The lines that stop the daemon the update started, and wait until it is
/// gone, before files are copied back over it. `systemctl stop` is
/// synchronous and cancels the restart policy; launchd's `KeepAlive` would
/// relaunch a job merely killed, so the job is booted out (the restore
/// bootstraps it again through `service install`).
fn stop_new_daemon(plan: &UpdatePlan) -> String {
    match plan.supervision {
        Supervision::Systemd => format!("systemctl --user stop pdo\n{STOP_MARKED}"),
        Supervision::Launchd => {
            format!("launchctl bootout \"gui/$(id -u)/com.pdo.daemon\" 2>/dev/null\n{STOP_MARKED}")
        }
        Supervision::None => format!("stop_pid \"${{NEW_PID:-}}\"\n{STOP_MARKED}"),
    }
}

/// `stop_pid <pid>`: SIGTERM, wait up to 10 s, then SIGKILL.
pub(crate) const STOP_PID: &str = "stop_pid() {
  [ -n \"$1\" ] || return 0
  kill -TERM \"$1\" 2>/dev/null
  i=0
  while kill -0 \"$1\" 2>/dev/null && [ \"$i\" -lt 100 ]; do sleep 0.1; i=$((i+1)); done
  if kill -0 \"$1\" 2>/dev/null; then say stop \"pid $1 still alive, SIGKILL\"; kill -KILL \"$1\" 2>/dev/null; sleep 0.5; fi
}
";

/// The `rollback <reason>` function: stop the new daemon, copy the binary and
/// the databases back (dropping their WAL and shared memory), let the restored
/// binary write its own service unit when supervised, restart, wait for the
/// daemon to answer, record `failed` + rolled back, exit 1.
pub(crate) fn rollback_function(plan: &UpdatePlan, snapshot: &UpdateSnapshot) -> String {
    let mut s = String::from("rollback() {\n");
    s.push_str(&format!(
        "  say rollback \"binary {}, {}\"\n",
        dq(&plan.from_version),
        if snapshot.databases.len() == 1 {
            "1 database".to_string()
        } else {
            format!("{} databases", snapshot.databases.len())
        }
    ));
    for line in stop_new_daemon(plan).lines() {
        s.push_str(&format!("  {line}\n"));
    }
    s.push_str(&format!(
        "  cp -f {} {} || say rollback \"cannot put the binary back\"\n",
        q(&snapshot.binary.saved),
        q(&snapshot.binary.original)
    ));
    for db in &snapshot.databases {
        let original = db.original.display().to_string();
        s.push_str(&format!(
            "  cp -f {} {} && rm -f {} {} || say rollback \"cannot put back {}\"\n",
            q(&db.saved),
            sh_quote(&original),
            sh_quote(&format!("{original}-wal")),
            sh_quote(&format!("{original}-shm")),
            dq(&original),
        ));
    }
    if plan.supervision != Supervision::None {
        // The unit the new binary wrote goes too: the restored binary writes
        // its own (and loads it again under launchd).
        s.push_str(&format!(
            "  cd {} && {} || say rollback \"service install failed\"\n",
            q(&plan.working_dir),
            service_command(plan, "install")
        ));
    }
    for line in start_daemon(plan, !RELOAD_PLIST).lines() {
        s.push_str(&format!("  {line}\n"));
    }
    s.push_str("  if [ \"$rc\" -eq 0 ] && wait_ready \"$TIMEOUT\"; then\n");
    s.push_str("    say verify \"daemon answers v$READY\"\n");
    s.push_str(
        "    record failed 1 \"$1 The previous version and its data were restored.\" true\n",
    );
    s.push_str("  else\n");
    s.push_str("    say verify \"no answer in $TIMEOUT s after the rollback\"\n");
    s.push_str(
        "    record failed 1 \"$1 The previous version and its data were restored, but the daemon did not answer: restart it by hand.\" true\n",
    );
    s.push_str("  fi\n");
    s.push_str("  say result \"failed (rolled back)\"\n");
    s.push_str("  exit 1\n}\n");
    s
}
