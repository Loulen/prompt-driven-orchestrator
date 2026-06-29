//! Deep module — the single path through which the daemon touches tmux.
//!
//! Exposes: spawn / capture / kill / list / session_exists / reaper / orphan-sweep / resume.
//! Nothing outside this module should shell out to `tmux`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use tracing::info;

/// Env var that replaces the `claude …` tail in the tmux script.
///
/// Read **once at daemon boot** by [`crate::DaemonConfig::from_env`] and then
/// carried as per-daemon config — never consulted in the spawn hot path. Tests
/// must seed the override through [`crate::DaemonConfig`] / `TestDaemon`, not by
/// mutating this process-global env (which races across cargo's parallel test
/// threads and is `unsafe`/UB-prone under the 2024 edition).
pub const TMUX_CMD_OVERRIDE_ENV: &str = "PDO_TMUX_CMD_OVERRIDE";

/// Compute the per-daemon tmux socket name (`tmux -L <name>`) for a daemon
/// listening on `daemon_port`.
///
/// Each daemon scopes its tmux state to a private socket so that orphan
/// sweeps and `list` calls only see *its own* sessions. Two daemons running
/// on different ports therefore can't observe — or kill — each other's
/// sessions, even when both run as the same user on the same host.
///
/// This eliminates the failure mode where a sub-claude transitively spawns
/// its own `pdo daemon` (e.g. for an end-to-end test from a Tester
/// node): the new daemon's boot-time `sweep_orphans` runs against an empty
/// event log and would otherwise call `tmux kill-session` on every
/// `pdo-*` session it finds on the system-default socket — collapsing
/// the parent daemon's running pipelines.
pub fn tmux_socket_name(daemon_port: u16) -> String {
    format!("pdo-{daemon_port}")
}

/// Build a `Command` for `tmux -L <socket>`. Use this everywhere we shell
/// out — never `Command::new("tmux")` directly.
fn tmux(socket: &str) -> std::process::Command {
    let mut c = std::process::Command::new("tmux");
    c.args(["-L", socket]);
    c
}

/// Enable mouse mode on a tmux session so that wheel events are forwarded
/// as mouse-report escape sequences instead of being silently dropped.
fn enable_mouse(socket: &str, session_name: &str) {
    let _ = tmux(socket)
        .args(["set-option", "-t", session_name, "mouse", "on"])
        .output();
}

/// Env var that overrides the reaper TTL (seconds). Default: 3600 (1 h).
pub const REAPER_TTL_SECS_ENV: &str = "PDO_REAPER_TTL_SECS";

/// Env var that overrides the reaper sweep interval (seconds). Default: 60.
pub const REAPER_INTERVAL_SECS_ENV: &str = "PDO_REAPER_INTERVAL_SECS";

/// Default TTL after node completion before the session is reaped.
pub const DEFAULT_REAPER_TTL: Duration = Duration::from_secs(3600);

/// Default sweep interval for the reaper background task.
pub const DEFAULT_REAPER_INTERVAL: Duration = Duration::from_secs(60);

// ---------------------------------------------------------------------------
// Shell helpers
// ---------------------------------------------------------------------------

fn sh_single_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for ch in s.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

// ---------------------------------------------------------------------------
// Script builder (pub for assertion in layer-3a tests)
// ---------------------------------------------------------------------------

/// Wrap a tail command with PDO env exports and an `exec bash -c` trampoline.
///
/// Both `exec`s collapse the shell so claude becomes the session leader.
///
/// `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1` is exported to suppress the
/// claude-code remote-bridge / CCR feature. Without it, a sub-claude spawned
/// here registers a worker session with api.anthropic.com that gets superseded
/// (HTTP 409 epoch mismatch) the moment any other claude code instance under
/// the same OAuth account makes an API call — at which point the backend
/// pushes `end_session`, claude tears down, opens `/dev/tty` (ENXIO inside the
/// tmux pane), writes `~/.claude.json`, and force-exits via `kill(getpid(),
/// SIGKILL)`. That's the "Tester dies silently 20–60 s in" bug.
fn wrap_with_env(
    run_id: &str,
    node_id: &str,
    iter: i64,
    daemon_port: u16,
    tail_cmd: &str,
) -> String {
    let inner = format!(
        "export PDO_RUN_ID={run_id_q} && \
         export PDO_NODE_ID={node_id_q} && \
         export PDO_NODE_ITER={iter_q} && \
         export PDO_DAEMON_URL={daemon_url_q} && \
         export CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 && \
         {tail_cmd}",
        run_id_q = sh_single_quote(run_id),
        node_id_q = sh_single_quote(node_id),
        iter_q = sh_single_quote(&iter.to_string()),
        daemon_url_q = sh_single_quote(&format!("http://localhost:{daemon_port}")),
    );

    format!("exec bash -c {}", sh_single_quote(&inner))
}

/// Construct the script tmux launches for a node run.
///
/// `tmux_cmd_override` replaces the default `claude …` tail when `Some` — the
/// per-daemon test seam (see [`TMUX_CMD_OVERRIDE_ENV`]). `None` → production
/// claude invocation.
pub fn build_tmux_script(
    run_id: &str,
    node_id: &str,
    iter: i64,
    daemon_port: u16,
    prompt_path: &Path,
    tmux_cmd_override: Option<&str>,
) -> String {
    let tail_cmd = match tmux_cmd_override {
        Some(cmd) => cmd.to_string(),
        None => format!(
            "exec claude --dangerously-skip-permissions \"$(cat {})\"",
            sh_single_quote(&prompt_path.to_string_lossy())
        ),
    };

    wrap_with_env(run_id, node_id, iter, daemon_port, &tail_cmd)
}

/// Build a resume script that uses `claude --continue` in the same working_dir.
///
/// `tmux_cmd_override` replaces the default `claude --continue` tail when
/// `Some` — the per-daemon test seam.
fn build_resume_script(
    run_id: &str,
    node_id: &str,
    iter: i64,
    daemon_port: u16,
    tmux_cmd_override: Option<&str>,
) -> String {
    let tail_cmd = match tmux_cmd_override {
        Some(cmd) => cmd.to_string(),
        None => "exec claude --dangerously-skip-permissions --continue".to_string(),
    };

    wrap_with_env(run_id, node_id, iter, daemon_port, &tail_cmd)
}

// ---------------------------------------------------------------------------
// Core operations
// ---------------------------------------------------------------------------

/// Session naming convention for NodeRuns.
pub fn node_session_name(run_id: &str, node_id: &str, iter: i64) -> String {
    format!("pdo-{run_id}-{node_id}-iter-{iter}")
}

/// Session naming convention for the Pipeline Manager.
pub fn manager_session_name(run_id: &str) -> String {
    format!("pdo-mgr-{run_id}")
}

/// Spawn a detached tmux session for a NodeRun.
///
/// `tmux_cmd_override` (per-daemon config, `AppState.tmux_cmd_override`)
/// replaces the `claude …` tail when `Some` — how tests run a harmless command
/// instead of launching real claude.
// The session identity (name + run/node/iter), working dir, daemon port, and
// command override are all irreducible inputs to a spawn; bundling them into a
// struct would only move the argument list, not shorten it.
#[allow(clippy::too_many_arguments)]
pub fn spawn(
    session_name: &str,
    prompt: &str,
    working_dir: &Path,
    run_id: &str,
    node_id: &str,
    iter: i64,
    daemon_port: u16,
    tmux_cmd_override: Option<&str>,
) -> Result<()> {
    let prompt_dir = working_dir.join(".pdo").join("prompts");
    std::fs::create_dir_all(&prompt_dir)?;
    let prompt_path = prompt_dir.join(format!("{node_id}-iter-{iter}.md"));
    std::fs::write(&prompt_path, prompt)?;

    let script = build_tmux_script(
        run_id,
        node_id,
        iter,
        daemon_port,
        &prompt_path,
        tmux_cmd_override,
    );
    let socket = tmux_socket_name(daemon_port);

    let output = tmux(&socket)
        .args(["new-session", "-d", "-s", session_name, "-c"])
        .arg(working_dir)
        .arg(&script)
        .output()
        .context("failed to run tmux new-session")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("tmux new-session failed: {stderr}");
    }

    enable_mouse(&socket, session_name);

    info!("Spawned tmux session: {session_name}");
    Ok(())
}

/// Resume a dead session via `claude --continue` in the original working_dir.
pub fn resume(
    session_name: &str,
    working_dir: &Path,
    run_id: &str,
    node_id: &str,
    iter: i64,
    daemon_port: u16,
    tmux_cmd_override: Option<&str>,
) -> Result<()> {
    let script = build_resume_script(run_id, node_id, iter, daemon_port, tmux_cmd_override);
    let socket = tmux_socket_name(daemon_port);

    let output = tmux(&socket)
        .args(["new-session", "-d", "-s", session_name, "-c"])
        .arg(working_dir)
        .arg(&script)
        .output()
        .context("failed to run tmux new-session (resume)")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("tmux new-session (resume) failed: {stderr}");
    }

    enable_mouse(&socket, session_name);

    info!("Resumed tmux session: {session_name}");
    Ok(())
}

/// Capture the visible pane content (with ANSI escapes) for a session.
/// Returns `None` if the session doesn't exist or capture fails.
pub fn capture(socket: &str, session_name: &str) -> Option<String> {
    let output = tmux(socket)
        .args(["capture-pane", "-pe", "-S", "-1000", "-t", session_name])
        .output()
        .ok()?;

    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        None
    }
}

/// Send keys to a tmux session. Best-effort — does not fail if the session is absent.
pub fn send_keys(socket: &str, session_name: &str, text: &str) {
    let _ = tmux(socket)
        .args(["send-keys", "-t", session_name, text, "Enter"])
        .output();
}

/// Kill a tmux session. Best-effort — does not fail if the session is absent.
pub fn kill(socket: &str, session_name: &str) {
    let _ = tmux(socket)
        .args(["kill-session", "-t", session_name])
        .output();
}

/// Check whether the tmux server for a given socket is alive (#234).
/// Runs `tmux -L <socket> ls` and returns true if the server responds.
pub fn server_alive(socket: &str) -> bool {
    tmux(socket)
        .arg("ls")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Check whether a tmux session exists.
pub fn session_exists(socket: &str, session_name: &str) -> bool {
    tmux(socket)
        .args(["has-session", "-t", session_name])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// List all tmux sessions whose name starts with `pdo-`, on the given socket.
/// Returns a set of session names.
pub fn list_pdo_sessions(socket: &str) -> HashSet<String> {
    let output = match tmux(socket).args(["ls", "-F", "#{session_name}"]).output() {
        Ok(o) if o.status.success() => o,
        _ => return HashSet::new(),
    };

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| line.starts_with("pdo-"))
        .map(String::from)
        .collect()
}

// ---------------------------------------------------------------------------
// Session name parsing
// ---------------------------------------------------------------------------

/// Parsed components of a `pdo-*` session name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedSession {
    NodeRun {
        run_id: String,
        node_id: String,
        iter: i64,
    },
    Manager {
        run_id: String,
    },
}

/// Parse a session name like `pdo-<run_id>-<node_id>-iter-<N>` or
/// `pdo-mgr-<run_id>`. Returns `None` for unrecognised formats.
pub fn parse_session_name(name: &str) -> Option<ParsedSession> {
    let rest = name.strip_prefix("pdo-")?;

    if let Some(run_id) = rest.strip_prefix("mgr-") {
        if !run_id.is_empty() {
            return Some(ParsedSession::Manager {
                run_id: run_id.to_string(),
            });
        }
        return None;
    }

    // run_id contains dashes (e.g. 20260506-143000-a3f1b2c), so we split on
    // the last "-iter-" to isolate the iter suffix first.
    let iter_sep = rest.rfind("-iter-")?;
    let before_iter = &rest[..iter_sep];
    let iter_str = &rest[iter_sep + 6..];
    let iter: i64 = iter_str.parse().ok()?;

    // run_id format: YYYYMMDD-HHMMSS-<7hex> = 23 chars.
    // After that comes "-" then node_id.
    let bytes = before_iter.as_bytes();
    const RUN_ID_LEN: usize = 23; // 8 + 1 + 6 + 1 + 7
    if bytes.len() <= RUN_ID_LEN
        || bytes[8] != b'-'
        || bytes[15] != b'-'
        || bytes[RUN_ID_LEN] != b'-'
    {
        return None;
    }

    let run_id = &before_iter[..RUN_ID_LEN];
    let node_id = &before_iter[RUN_ID_LEN + 1..];

    if node_id.is_empty() {
        return None;
    }

    Some(ParsedSession::NodeRun {
        run_id: run_id.to_string(),
        node_id: node_id.to_string(),
        iter,
    })
}

// ---------------------------------------------------------------------------
// Reaper / orphan sweep
// ---------------------------------------------------------------------------

/// Information the reaper needs about a NodeRun to decide whether to reap.
pub struct NodeRunInfo {
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub is_archived: bool,
}

/// Read the reaper TTL from the env or use the default.
pub fn reaper_ttl() -> Duration {
    std::env::var(REAPER_TTL_SECS_ENV)
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_REAPER_TTL)
}

/// Read the reaper interval from the env or use the default.
pub fn reaper_interval() -> Duration {
    std::env::var(REAPER_INTERVAL_SECS_ENV)
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_REAPER_INTERVAL)
}

/// Sweep orphan tmux sessions at daemon boot.
///
/// Scans only the daemon's private socket — never the system-default
/// socket — so we can never reach into another daemon's tmux state.
///
/// An orphan is a `pdo-*` session whose corresponding run is:
/// - archived
/// - absent from the event log
/// - a NodeRun that completed more than `ttl` ago
pub fn sweep_orphans<F>(socket: &str, lookup: F, ttl: Duration)
where
    F: Fn(&str, &str, i64) -> Option<NodeRunInfo>,
{
    let sessions = list_pdo_sessions(socket);
    let now = chrono::Utc::now();

    for session_name in &sessions {
        let parsed = match parse_session_name(session_name) {
            Some(p) => p,
            None => {
                info!("Orphan sweep: killing unrecognised session {session_name}");
                kill(socket, session_name);
                continue;
            }
        };

        match parsed {
            ParsedSession::Manager { ref run_id } => {
                // Kill manager sessions for absent/archived runs
                let info = lookup(run_id, "__manager__", 0);
                match info {
                    None => {
                        info!("Orphan sweep: killing manager session for absent run {run_id}");
                        kill(socket, session_name);
                    }
                    Some(info) if info.is_archived => {
                        info!("Orphan sweep: killing manager session for archived run {run_id}");
                        kill(socket, session_name);
                    }
                    _ => {}
                }
            }
            ParsedSession::NodeRun {
                ref run_id,
                ref node_id,
                iter,
            } => {
                let info = lookup(run_id, node_id, iter);
                match info {
                    None => {
                        info!("Orphan sweep: killing session for absent run {run_id}/{node_id}");
                        kill(socket, session_name);
                    }
                    Some(info) if info.is_archived => {
                        info!("Orphan sweep: killing session for archived run {run_id}/{node_id}");
                        kill(socket, session_name);
                    }
                    Some(NodeRunInfo {
                        completed_at: Some(completed),
                        ..
                    }) => {
                        let age = now.signed_duration_since(completed);
                        if age
                            > chrono::Duration::from_std(ttl).unwrap_or(chrono::Duration::hours(1))
                        {
                            info!(
                                "Orphan sweep: killing stale session {session_name} (completed {}s ago)",
                                age.num_seconds()
                            );
                            kill(socket, session_name);
                        }
                    }
                    _ => {} // still running or not yet completed
                }
            }
        }
    }
}

/// Resolve the working_dir for a NodeRun given run context.
pub fn working_dir_for_node(
    repo_root: &Path,
    run_id: &str,
    node_id: &str,
    iter: i64,
    node_type: &str,
) -> PathBuf {
    if node_type == "code-mutating" {
        repo_root
            .join(".pdo")
            .join("runs")
            .join(run_id)
            .join("nodes")
            .join(node_id)
            .join(format!("iter-{iter}"))
    } else {
        repo_root
            .join(".pdo")
            .join("runs")
            .join(run_id)
            .join("worktree")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_node_session() {
        let name = "pdo-20260506-143000-a3f1b2c-solo-iter-1";
        let parsed = parse_session_name(name).unwrap();
        assert_eq!(
            parsed,
            ParsedSession::NodeRun {
                run_id: "20260506-143000-a3f1b2c".into(),
                node_id: "solo".into(),
                iter: 1,
            }
        );
    }

    #[test]
    fn parse_node_session_with_dashed_node_id() {
        let name = "pdo-20260506-143000-a3f1b2c-impl-worker-iter-3";
        let parsed = parse_session_name(name).unwrap();
        assert_eq!(
            parsed,
            ParsedSession::NodeRun {
                run_id: "20260506-143000-a3f1b2c".into(),
                node_id: "impl-worker".into(),
                iter: 3,
            }
        );
    }

    #[test]
    fn parse_manager_session() {
        let name = "pdo-mgr-20260506-143000-a3f1b2c";
        let parsed = parse_session_name(name).unwrap();
        assert_eq!(
            parsed,
            ParsedSession::Manager {
                run_id: "20260506-143000-a3f1b2c".into(),
            }
        );
    }

    #[test]
    fn parse_garbage_returns_none() {
        assert!(parse_session_name("foo-bar").is_none());
        assert!(parse_session_name("pdo-").is_none());
        assert!(parse_session_name("pdo-mgr-").is_none());
    }

    #[test]
    fn build_script_default_and_override() {
        let prompt_path = Path::new("/tmp/test-prompt.md");

        // None → production claude tail.
        let script = build_tmux_script("run-abc", "solo", 1, 5172, prompt_path, None);
        assert!(script.starts_with("exec bash -c "));
        assert!(script.contains("exec claude --dangerously-skip-permissions"));
        assert!(script.contains("PDO_RUN_ID"));
        assert!(script.contains("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1"));

        // Some(..) → override tail, no claude. The override is passed as a
        // parameter (per-daemon config), never read from process-global env.
        let script = build_tmux_script(
            "run-abc",
            "solo",
            1,
            5172,
            prompt_path,
            Some("exec sleep 60"),
        );
        assert!(script.contains("exec sleep 60"));
        assert!(!script.contains("claude"));
    }

    #[test]
    fn reaper_ttl_default_and_from_env() {
        std::env::remove_var(REAPER_TTL_SECS_ENV);
        assert_eq!(reaper_ttl(), Duration::from_secs(3600));

        std::env::set_var(REAPER_TTL_SECS_ENV, "5");
        assert_eq!(reaper_ttl(), Duration::from_secs(5));
        std::env::remove_var(REAPER_TTL_SECS_ENV);
    }

    #[test]
    fn node_session_name_format() {
        assert_eq!(
            node_session_name("20260506-143000-a3f1b2c", "solo", 1),
            "pdo-20260506-143000-a3f1b2c-solo-iter-1"
        );
    }

    #[test]
    fn manager_session_name_format() {
        assert_eq!(
            manager_session_name("20260506-143000-a3f1b2c"),
            "pdo-mgr-20260506-143000-a3f1b2c"
        );
    }
}
