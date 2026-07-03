//! Persistent-daemon service-unit generation (#156, ADR-0019).
//!
//! Turns an ephemeral `pdo daemon` (which dies at logout/reboot — the exact v1
//! limitation ADR-0012 flagged, CONTEXT.md:536) into an **installable OS
//! service** that starts at boot and survives logout.
//!
//! Everything here is a **pure function of its inputs** (no I/O, no
//! `std::env`), so the generated unit files are golden-tested at Layer 1
//! (ADR-0004). The impure edges — locating `node`, resolving `$XDG_CONFIG_HOME`,
//! writing the file, running `systemctl` — live in `run_service` (lib.rs) and
//! are exercised via `--dry-run` / an injected command runner (ADR-0004 line 29:
//! host-mutating acts stay out of the automated suite).
//!
//! The Linux systemd unit is a **byte-faithful port of the proven prod recipe**
//! in the `Makefile` (`service-install`), parameterised. Two lines are
//! load-bearing and asserted in the golden tests:
//!   * `KillMode=process` — systemd's default `control-group` SIGKILLs the whole
//!     cgroup on stop/restart, which would nuke the child tmux server holding
//!     every live Claude session (#234). `process` kills only the daemon pid.
//!   * `Environment=PATH=…<node dir>…` — the daemon shells out to
//!     `claude`/`node`/`git`/`tmux`; a bare unit PATH silently breaks spawns.

use std::path::{Path, PathBuf};

/// launchd label / plist basename for the macOS LaunchAgent (#156, D6).
pub const LAUNCHD_LABEL: &str = "com.pdo.daemon";

/// systemd unit name written under `<config_home>/systemd/user/`.
pub const SYSTEMD_UNIT_NAME: &str = "pdo.service";

/// Render the Linux `systemd --user` unit (#156, D3).
///
/// Byte-faithful to the prod recipe (`Makefile` `service-install`),
/// parameterised on the four install-time values. Pure: given identical
/// arguments it always returns the same string.
///
/// * `exe` — the daemon binary the unit launches (`std::env::current_exe()` at
///   install time), so the unit points at *this* binary, not a guessed path.
/// * `port` — `PDO_PORT` for the daemon (ties to the `env = "PDO_PORT"` arg on
///   the `daemon` subcommand).
/// * `working_dir` — the daemon derives `repo_root` from its cwd (not a flag);
///   a unit without `WorkingDirectory` would run from `/` and resolve the wrong
///   repo root. Load-bearing.
/// * `path_env` — the `Environment=PATH=` value (see [`build_path_env`]).
pub fn render_systemd_unit(exe: &Path, port: u16, working_dir: &Path, path_env: &str) -> String {
    format!(
        "[Unit]\n\
         Description=PDO (Prompt-Driven Orchestrator) daemon\n\
         After=network-online.target\n\
         Wants=network-online.target\n\
         \n\
         [Service]\n\
         Type=simple\n\
         WorkingDirectory={working_dir}\n\
         Environment=PDO_PORT={port}\n\
         Environment=PATH={path_env}\n\
         ExecStart={exe} daemon\n\
         Restart=on-failure\n\
         RestartSec=3\n\
         KillMode=process\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n",
        working_dir = working_dir.display(),
        port = port,
        path_env = path_env,
        exe = exe.display(),
    )
}

/// Render the macOS launchd LaunchAgent plist (#156, D6). **Best-effort**:
/// generation is golden-tested (Layer 1) but the real `launchctl` path is
/// untested on the Linux CI box and flagged as such in ADR-0019.
///
/// `AbandonProcessGroup=true` is the `KillMode=process` analog — it keeps the
/// setsid'd child tmux server alive across a stop/restart. `WorkingDirectory`
/// and `EnvironmentVariables/PATH` are load-bearing for the same reasons as the
/// systemd unit (repo-root resolution; finding `node`/`claude`/`git`/`tmux`).
pub fn render_launchd_plist(
    exe: &Path,
    port: u16,
    working_dir: &Path,
    home: &Path,
    path_env: &str,
) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\"\n\
         \x20 \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n\
         <dict>\n\
         \x20 <key>Label</key><string>{label}</string>\n\
         \x20 <key>ProgramArguments</key>\n\
         \x20 <array><string>{exe}</string><string>daemon</string></array>\n\
         \x20 <key>RunAtLoad</key><true/>\n\
         \x20 <key>KeepAlive</key><true/>\n\
         \x20 <!-- KillMode=process analog: do NOT reap the setsid'd child tmux server on stop/restart -->\n\
         \x20 <key>AbandonProcessGroup</key><true/>\n\
         \x20 <key>WorkingDirectory</key><string>{working_dir}</string>\n\
         \x20 <key>EnvironmentVariables</key>\n\
         \x20 <dict>\n\
         \x20 \x20 <key>PDO_PORT</key><string>{port}</string>\n\
         \x20 \x20 <key>PATH</key><string>{path_env}</string>\n\
         \x20 \x20 <key>HOME</key><string>{home}</string>\n\
         \x20 </dict>\n\
         \x20 <key>StandardOutPath</key><string>{home}/Library/Logs/{label}.out.log</string>\n\
         \x20 <key>StandardErrorPath</key><string>{home}/Library/Logs/{label}.err.log</string>\n\
         </dict>\n\
         </plist>\n",
        label = LAUNCHD_LABEL,
        exe = exe.display(),
        working_dir = working_dir.display(),
        port = port,
        path_env = path_env,
        home = home.display(),
    )
}

/// Build the `Environment=PATH=` value for the unit (#156, D3).
///
/// Faithful to the Makefile recipe:
/// `<exe dir>:<node dir>:/usr/local/bin:/usr/bin:/bin`. The exe dir first so the
/// service resolves the very `pdo` it was installed from; the node dir so
/// `claude`/`node` spawns work under the minimal env systemd hands a unit.
///
/// `node_dir` is passed in (resolved impurely by [`resolve_node_dir`]) to keep
/// this pure and golden-testable. When `None` (node not found) the node segment
/// is omitted — `run_service` warns loudly in that case, since a missing node
/// dir is the single most likely silent-spawn-failure on a shipped unit.
pub fn build_path_env(exe: &Path, node_dir: Option<&Path>) -> String {
    let mut parts: Vec<String> = Vec::with_capacity(5);
    if let Some(dir) = exe.parent() {
        parts.push(dir.display().to_string());
    }
    if let Some(dir) = node_dir {
        parts.push(dir.display().to_string());
    }
    parts.push("/usr/local/bin".to_string());
    parts.push("/usr/bin".to_string());
    parts.push("/bin".to_string());
    parts.join(":")
}

/// Path the systemd `--user` unit is written to:
/// `<config_home>/systemd/user/pdo.service` (#156, D4).
///
/// `config_home` is **injected** (resolved by [`resolve_config_home`] in prod)
/// so tests point it at a `TempDir` and never touch the real `~/.config` — the
/// same determinism trick as `resolve_browse_root`.
pub fn systemd_unit_path(config_home: &Path) -> PathBuf {
    config_home
        .join("systemd")
        .join("user")
        .join(SYSTEMD_UNIT_NAME)
}

/// Path the launchd LaunchAgent plist is written to:
/// `<home>/Library/LaunchAgents/com.pdo.daemon.plist` (#156, D6).
pub fn launchd_plist_path(home: &Path) -> PathBuf {
    home.join("Library")
        .join("LaunchAgents")
        .join(format!("{LAUNCHD_LABEL}.plist"))
}

/// Resolve `$XDG_CONFIG_HOME`, falling back to `$HOME/.config` (#156, D4).
///
/// Impure (reads env); the pure [`systemd_unit_path`] takes the result as a
/// parameter so tests stay hermetic. Returns `None` only if neither
/// `XDG_CONFIG_HOME` nor `HOME` is set (a broken environment).
pub fn resolve_config_home() -> Option<PathBuf> {
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            return Some(PathBuf::from(xdg));
        }
    }
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config"))
}

/// Locate the directory containing the `node` executable by scanning `$PATH`
/// (#156, D3) — the dependency-free analog of the Makefile's
/// `dirname $(command -v node)`. Impure (reads `$PATH`, stats files); returns
/// `None` when node is not on `$PATH`, so callers can warn instead of silently
/// shipping a unit whose daemon can't spawn Claude.
pub fn resolve_node_dir() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join("node");
        if candidate.is_file() {
            return Some(dir);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn systemd_unit_is_byte_faithful_to_the_prod_recipe() {
        let exe = Path::new("/home/user/.local/bin/pdo");
        let wd = Path::new("/home/user/.pdo/app");
        let path_env = "/home/user/.local/bin:/opt/node/bin:/usr/local/bin:/usr/bin:/bin";
        let unit = render_systemd_unit(exe, 6160, wd, path_env);

        let expected = "\
[Unit]
Description=PDO (Prompt-Driven Orchestrator) daemon
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
WorkingDirectory=/home/user/.pdo/app
Environment=PDO_PORT=6160
Environment=PATH=/home/user/.local/bin:/opt/node/bin:/usr/local/bin:/usr/bin:/bin
ExecStart=/home/user/.local/bin/pdo daemon
Restart=on-failure
RestartSec=3
KillMode=process

[Install]
WantedBy=default.target
";
        assert_eq!(unit, expected);
    }

    #[test]
    fn systemd_unit_contains_every_load_bearing_line() {
        let unit = render_systemd_unit(
            Path::new("/x/pdo"),
            5172,
            Path::new("/repo"),
            "/x:/opt/node/bin:/usr/bin",
        );
        // KillMode=process — keeps the child tmux server alive across restart (#234).
        assert!(unit.contains("KillMode=process"), "missing KillMode=process");
        // ExecStart points at THIS binary + the `daemon` subcommand.
        assert!(unit.contains("ExecStart=/x/pdo daemon"));
        // WorkingDirectory — daemon derives repo_root from cwd.
        assert!(unit.contains("WorkingDirectory=/repo"));
        // Port + PATH.
        assert!(unit.contains("Environment=PDO_PORT=5172"));
        assert!(unit.contains("Environment=PATH=/x:/opt/node/bin:/usr/bin"));
        // Restart policy + install target.
        assert!(unit.contains("Restart=on-failure"));
        assert!(unit.contains("RestartSec=3"));
        assert!(unit.contains("WantedBy=default.target"));
    }

    #[test]
    fn launchd_plist_contains_every_load_bearing_key() {
        let plist = render_launchd_plist(
            Path::new("/Users/u/.local/bin/pdo"),
            6160,
            Path::new("/Users/u/.pdo/app"),
            Path::new("/Users/u"),
            "/Users/u/.local/bin:/opt/node/bin:/usr/bin",
        );
        assert!(plist.contains("<key>Label</key><string>com.pdo.daemon</string>"));
        // AbandonProcessGroup — the KillMode=process analog (tmux survives).
        assert!(plist.contains("<key>AbandonProcessGroup</key><true/>"));
        assert!(plist.contains("<key>RunAtLoad</key><true/>"));
        assert!(plist.contains("<key>KeepAlive</key><true/>"));
        // ProgramArguments launches this binary's `daemon` subcommand.
        assert!(plist.contains(
            "<array><string>/Users/u/.local/bin/pdo</string><string>daemon</string></array>"
        ));
        // Load-bearing env: PDO_PORT, PATH (incl. node dir), HOME; and WorkingDirectory.
        assert!(plist.contains("<key>PDO_PORT</key><string>6160</string>"));
        assert!(plist.contains(
            "<key>PATH</key><string>/Users/u/.local/bin:/opt/node/bin:/usr/bin</string>"
        ));
        assert!(plist.contains("<key>HOME</key><string>/Users/u</string>"));
        assert!(plist.contains("<key>WorkingDirectory</key><string>/Users/u/.pdo/app</string>"));
        assert!(plist.starts_with("<?xml version=\"1.0\""));
    }

    #[test]
    fn build_path_env_orders_exe_then_node_then_std_dirs() {
        let got = build_path_env(
            Path::new("/home/user/.local/bin/pdo"),
            Some(Path::new("/opt/node/bin")),
        );
        assert_eq!(
            got,
            "/home/user/.local/bin:/opt/node/bin:/usr/local/bin:/usr/bin:/bin"
        );
    }

    #[test]
    fn build_path_env_omits_node_segment_when_absent() {
        let got = build_path_env(Path::new("/home/user/.local/bin/pdo"), None);
        assert_eq!(got, "/home/user/.local/bin:/usr/local/bin:/usr/bin:/bin");
    }

    #[test]
    fn systemd_unit_path_lands_under_systemd_user() {
        let got = systemd_unit_path(Path::new("/home/user/.config"));
        assert_eq!(
            got,
            Path::new("/home/user/.config/systemd/user/pdo.service")
        );
    }

    #[test]
    fn launchd_plist_path_lands_under_launchagents() {
        let got = launchd_plist_path(Path::new("/Users/u"));
        assert_eq!(
            got,
            Path::new("/Users/u/Library/LaunchAgents/com.pdo.daemon.plist")
        );
    }
}
