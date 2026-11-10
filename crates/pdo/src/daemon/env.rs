//! The environment of `pdo daemon`, read once at boot, and its command line.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};

use pdo_platform::domain::service_health::ServiceHealthOverride;

use super::config::{DaemonConfig, ExtensionHostHandle};

/// Resolve the daemon's cleanup posture from the two env signals production
/// carries it on — **pure**, so the truth table is testable without touching
/// process-global env.
///
/// - `PDO_NODE_ID` present at all ⇒ nested. A sub-claude context exports it via
///   `wrap_with_env`, and its mere presence is the signal (an empty value still
///   means "we are inside a NodeRun").
/// - `PDO_DAEMON_NO_CLEANUP` ⇒ nested when set to anything but `""` or `"0"`, the
///   same truthiness the `PDO_DEBUG_PANIC_*` knobs use.
pub(crate) fn nested_daemon_from(node_id: Option<&str>, no_cleanup: Option<&str>) -> bool {
    node_id.is_some() || no_cleanup.is_some_and(|v| !v.is_empty() && v != "0")
}

impl DaemonConfig {
    /// Build config from the environment — the production / CLI daemon path.
    ///
    /// Reads every env seam exactly once, here at boot, keeping those reads out
    /// of the hot paths.
    pub fn from_env() -> Self {
        Self {
            tmux_cmd_override: std::env::var(pdo_kernel::env::TMUX_CMD_OVERRIDE_ENV).ok(),
            panic_on_trigger_name: std::env::var("PDO_DEBUG_PANIC_TRIGGER").ok(),
            panic_on_stale_sweep: std::env::var("PDO_DEBUG_PANIC_STALE")
                .map(|v| !v.is_empty() && v != "0")
                .unwrap_or(false),
            panic_on_spawn: std::env::var("PDO_DEBUG_PANIC_SPAWN")
                .map(|v| !v.is_empty() && v != "0")
                .unwrap_or(false),
            // Invalid values are ignored rather than fatal.
            service_health_override: std::env::var("PDO_SERVICE_HEALTH")
                .ok()
                .and_then(|v| ServiceHealthOverride::parse(&v)),
            docker_cmd_override: std::env::var(pdo_kernel::env::DOCKER_CMD_OVERRIDE_ENV).ok(),
            sandbox_home_override: std::env::var("PDO_SANDBOX_HOME_OVERRIDE")
                .ok()
                .filter(|s| !s.is_empty())
                .map(PathBuf::from),
            price_source_url: std::env::var("PDO_PRICE_SOURCE_URL")
                .ok()
                .filter(|s| !s.is_empty()),
            // The crate's ONE opt-OUT env, deliberately: a feature that must work
            // out of the box cannot be armed by an env var, and the real opt-in is
            // the first click (ADR-0034 — the boot pass only ever REFRESHES an
            // existing cache, it never creates one).
            price_refresh_at_boot: std::env::var("PDO_PRICE_SYNC")
                .map(|v| !v.is_empty() && v != "0" && v != "off")
                .unwrap_or(true),
            update_source_url: std::env::var("PDO_UPDATE_SOURCE_URL")
                .ok()
                .filter(|s| !s.is_empty()),
            // Not an env var: the channel belongs to the binary, which declares it
            // at compile time through `Config::with_release_channel`.
            release_channel: None,
            // Declared at compile time too, through `Config::with_brand`.
            brand: crate::Brand::default(),
            // Core daemon behaviour; the `update_check` SETTING is the user's
            // off switch, not an env var. Only the layer-3 literals opt out.
            run_update_check_loop: true,
            update_executor_override: std::env::var(
                pdo_platform::adapters::detached_executor::EXECUTOR_OVERRIDE_ENV,
            )
            .ok()
            .filter(|s| !s.trim().is_empty()),
            install_method_override: std::env::var(
                pdo_platform::adapters::detached_executor::INSTALL_METHOD_OVERRIDE_ENV,
            )
            .ok()
            .and_then(|v| pdo_platform::domain::install::InstallMethod::parse(&v)),
            supervision_override: None,
            relaunch_command: None,
            // No env seam: the loop is core daemon behaviour; only the layer-3
            // test literals opt out.
            run_trigger_scheduler_loop: true,
            // The ONE place either env var is read; `serve_with_config` consumes
            // the resolved boolean, so tests pick the posture per daemon.
            nested_daemon: nested_daemon_from(
                std::env::var("PDO_NODE_ID").ok().as_deref(),
                std::env::var("PDO_DAEMON_NO_CLEANUP").ok().as_deref(),
            ),
            // The ONE read of PDO_ALLOWED_WS_ORIGINS. Unset or blank yields an
            // empty list, i.e. localhost-only.
            allowed_ws_origins: std::env::var(pdo_session::adapters::env::ALLOWED_WS_ORIGINS_ENV)
                .map(|v| pdo_session::domain::pty_protocol::parse_allowed_origins(&v))
                .unwrap_or_default(),

            router_layer: None,
            extension_host: ExtensionHostHandle::default(),
        }
    }
}

/// `<exe> daemon --port <port>`, plus `--bind` only when the operator gave one.
fn daemon_relaunch_command(exe: &Path, bind: Option<IpAddr>, port: u16) -> Vec<String> {
    let mut command = vec![
        exe.display().to_string(),
        "daemon".to_string(),
        "--port".to_string(),
        port.to_string(),
    ];
    if let Some(bind) = bind {
        command.push("--bind".to_string());
        command.push(bind.to_string());
    }
    command
}

/// The address `pdo daemon` listens on: `--bind` (every interface by default)
/// and `--port`.
pub fn daemon_bind_addr(bind: Option<IpAddr>, port: u16) -> SocketAddr {
    SocketAddr::new(bind.unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED)), port)
}

/// The configuration of `pdo daemon`: the environment, read once, plus the
/// command line an unsupervised update relaunches.
pub fn daemon_cli_config(bind: Option<IpAddr>, port: u16) -> DaemonConfig {
    let mut config = DaemonConfig::from_env();
    // #699: the command line an unsupervised update relaunches — the STABLE binary
    // path (Homebrew's `bin/pdo`, not the Cellar target `brew upgrade` deletes) and
    // the same port, whether it came from `--port` or `PDO_PORT`.
    config.relaunch_command = pdo_platform::adapters::host_installation::invoked_exe_path()
        .map(|exe| daemon_relaunch_command(&exe, bind, port));
    config
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daemon_relaunch_preserves_only_an_explicit_bind() {
        let exe = Path::new("/usr/local/bin/pdo");
        assert_eq!(
            daemon_relaunch_command(exe, None, 5172),
            vec!["/usr/local/bin/pdo", "daemon", "--port", "5172"]
        );
        assert_eq!(
            daemon_relaunch_command(exe, Some("127.0.0.1".parse().unwrap()), 5172),
            vec![
                "/usr/local/bin/pdo",
                "daemon",
                "--port",
                "5172",
                "--bind",
                "127.0.0.1"
            ]
        );
    }

    // The nested-daemon (no-cleanup) posture resolver
    //
    // The truth table used to be inlined in `serve_with_config` as two
    // `std::env::var` reads, which made it untestable except by mutating
    // process-global env — the very thing that raced the `pty_bridge` tests into
    // a red CI. Pure fn, so each case is a plain assertion.

    #[test]
    fn nested_daemon_is_off_when_neither_signal_is_present() {
        assert!(!nested_daemon_from(None, None));
    }

    #[test]
    fn a_sub_claude_context_is_nested_on_the_mere_presence_of_a_node_id() {
        // `wrap_with_env` exports PDO_NODE_ID; presence is the signal, so even an
        // empty value means "inside a NodeRun" (the pre-existing `.is_ok()` shape).
        assert!(nested_daemon_from(Some("implementer"), None));
        assert!(nested_daemon_from(Some(""), None));
    }

    #[test]
    fn the_operator_knob_is_truthy_but_not_merely_set() {
        assert!(nested_daemon_from(None, Some("1")));
        assert!(nested_daemon_from(None, Some("yes")));
        // Same truthiness as the `PDO_DEBUG_PANIC_*` knobs: "" and "0" are OFF, so
        // `PDO_DAEMON_NO_CLEANUP=0` disarms rather than arms.
        assert!(!nested_daemon_from(None, Some("")));
        assert!(!nested_daemon_from(None, Some("0")));
    }

    #[test]
    fn either_signal_alone_is_enough() {
        assert!(nested_daemon_from(Some("worker"), Some("0")));
        assert!(nested_daemon_from(None, Some("1")));
    }
}
