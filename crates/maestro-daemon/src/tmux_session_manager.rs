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
/// Used by integration tests to spawn `sleep 60` instead of claude.
pub const TMUX_CMD_OVERRIDE_ENV: &str = "MAESTRO_TMUX_CMD_OVERRIDE";

/// Env var that overrides the reaper TTL (seconds). Default: 3600 (1 h).
pub const REAPER_TTL_SECS_ENV: &str = "MAESTRO_REAPER_TTL_SECS";

/// Env var that overrides the reaper sweep interval (seconds). Default: 60.
pub const REAPER_INTERVAL_SECS_ENV: &str = "MAESTRO_REAPER_INTERVAL_SECS";

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

/// Wrap a tail command with Maestro env exports and an `exec bash -c` trampoline.
///
/// Both `exec`s collapse the shell so claude becomes the session leader.
fn wrap_with_env(
    run_id: &str,
    node_id: &str,
    iter: i64,
    daemon_port: u16,
    tail_cmd: &str,
) -> String {
    let inner = format!(
        "export MAESTRO_RUN_ID={run_id_q} && \
         export MAESTRO_NODE_ID={node_id_q} && \
         export MAESTRO_NODE_ITER={iter_q} && \
         export MAESTRO_DAEMON_URL={daemon_url_q} && \
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
/// The default tail can be overridden via `TMUX_CMD_OVERRIDE_ENV`.
pub fn build_tmux_script(
    run_id: &str,
    node_id: &str,
    iter: i64,
    daemon_port: u16,
    prompt_path: &Path,
) -> String {
    let tail_cmd = std::env::var(TMUX_CMD_OVERRIDE_ENV).unwrap_or_else(|_| {
        format!(
            "exec claude --dangerously-skip-permissions \"$(cat {})\"",
            sh_single_quote(&prompt_path.to_string_lossy())
        )
    });

    wrap_with_env(run_id, node_id, iter, daemon_port, &tail_cmd)
}

/// Build a resume script that uses `claude --continue` in the same working_dir.
fn build_resume_script(run_id: &str, node_id: &str, iter: i64, daemon_port: u16) -> String {
    let tail_cmd = std::env::var(TMUX_CMD_OVERRIDE_ENV)
        .unwrap_or_else(|_| "exec claude --dangerously-skip-permissions --continue".to_string());

    wrap_with_env(run_id, node_id, iter, daemon_port, &tail_cmd)
}

// ---------------------------------------------------------------------------
// Core operations
// ---------------------------------------------------------------------------

/// Session naming convention for NodeRuns.
pub fn node_session_name(run_id: &str, node_id: &str, iter: i64) -> String {
    format!("maestro-{run_id}-{node_id}-iter-{iter}")
}

/// Session naming convention for the Pipeline Manager.
pub fn manager_session_name(run_id: &str) -> String {
    format!("maestro-mgr-{run_id}")
}

/// Spawn a detached tmux session for a NodeRun.
pub fn spawn(
    session_name: &str,
    prompt: &str,
    working_dir: &Path,
    run_id: &str,
    node_id: &str,
    iter: i64,
    daemon_port: u16,
) -> Result<()> {
    let prompt_dir = working_dir.join(".maestro").join("prompts");
    std::fs::create_dir_all(&prompt_dir)?;
    let prompt_path = prompt_dir.join(format!("{node_id}-iter-{iter}.md"));
    std::fs::write(&prompt_path, prompt)?;

    let script = build_tmux_script(run_id, node_id, iter, daemon_port, &prompt_path);

    let output = std::process::Command::new("tmux")
        .args(["new-session", "-d", "-s", session_name, "-c"])
        .arg(working_dir)
        .arg(&script)
        .output()
        .context("failed to run tmux new-session")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("tmux new-session failed: {stderr}");
    }

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
) -> Result<()> {
    let script = build_resume_script(run_id, node_id, iter, daemon_port);

    let output = std::process::Command::new("tmux")
        .args(["new-session", "-d", "-s", session_name, "-c"])
        .arg(working_dir)
        .arg(&script)
        .output()
        .context("failed to run tmux new-session (resume)")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("tmux new-session (resume) failed: {stderr}");
    }

    info!("Resumed tmux session: {session_name}");
    Ok(())
}

/// Capture the visible pane content (with ANSI escapes) for a session.
/// Returns `None` if the session doesn't exist or capture fails.
pub fn capture(session_name: &str) -> Option<String> {
    let output = std::process::Command::new("tmux")
        .args(["capture-pane", "-pe", "-S", "-1000", "-t", session_name])
        .output()
        .ok()?;

    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        None
    }
}

/// Kill a tmux session. Best-effort — does not fail if the session is absent.
pub fn kill(session_name: &str) {
    let _ = std::process::Command::new("tmux")
        .args(["kill-session", "-t", session_name])
        .output();
}

/// Check whether a tmux session exists.
pub fn session_exists(session_name: &str) -> bool {
    std::process::Command::new("tmux")
        .args(["has-session", "-t", session_name])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// List all tmux sessions whose name starts with `maestro-`.
/// Returns a set of session names.
pub fn list_maestro_sessions() -> HashSet<String> {
    let output = match std::process::Command::new("tmux")
        .args(["ls", "-F", "#{session_name}"])
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => return HashSet::new(),
    };

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| line.starts_with("maestro-"))
        .map(String::from)
        .collect()
}

// ---------------------------------------------------------------------------
// Session name parsing
// ---------------------------------------------------------------------------

/// Parsed components of a `maestro-*` session name.
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

/// Parse a session name like `maestro-<run_id>-<node_id>-iter-<N>` or
/// `maestro-mgr-<run_id>`. Returns `None` for unrecognised formats.
pub fn parse_session_name(name: &str) -> Option<ParsedSession> {
    let rest = name.strip_prefix("maestro-")?;

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
/// An orphan is a `maestro-*` session whose corresponding run is:
/// - archived
/// - absent from the event log
/// - a NodeRun that completed more than `ttl` ago
pub fn sweep_orphans<F>(lookup: F, ttl: Duration)
where
    F: Fn(&str, &str, i64) -> Option<NodeRunInfo>,
{
    let sessions = list_maestro_sessions();
    let now = chrono::Utc::now();

    for session_name in &sessions {
        let parsed = match parse_session_name(session_name) {
            Some(p) => p,
            None => {
                info!("Orphan sweep: killing unrecognised session {session_name}");
                kill(session_name);
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
                        kill(session_name);
                    }
                    Some(info) if info.is_archived => {
                        info!("Orphan sweep: killing manager session for archived run {run_id}");
                        kill(session_name);
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
                        kill(session_name);
                    }
                    Some(info) if info.is_archived => {
                        info!("Orphan sweep: killing session for archived run {run_id}/{node_id}");
                        kill(session_name);
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
                            kill(session_name);
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
            .join(".maestro")
            .join("runs")
            .join(run_id)
            .join("nodes")
            .join(node_id)
            .join(format!("iter-{iter}"))
    } else {
        repo_root
            .join(".maestro")
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
        let name = "maestro-20260506-143000-a3f1b2c-solo-iter-1";
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
        let name = "maestro-20260506-143000-a3f1b2c-impl-worker-iter-3";
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
        let name = "maestro-mgr-20260506-143000-a3f1b2c";
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
        assert!(parse_session_name("maestro-").is_none());
        assert!(parse_session_name("maestro-mgr-").is_none());
    }

    #[test]
    fn build_script_default_and_override() {
        std::env::remove_var(TMUX_CMD_OVERRIDE_ENV);
        let prompt_path = Path::new("/tmp/test-prompt.md");
        let script = build_tmux_script("run-abc", "solo", 1, 5172, prompt_path);
        assert!(script.starts_with("exec bash -c "));
        assert!(script.contains("exec claude --dangerously-skip-permissions"));
        assert!(script.contains("MAESTRO_RUN_ID"));

        std::env::set_var(TMUX_CMD_OVERRIDE_ENV, "exec sleep 60");
        let script = build_tmux_script("run-abc", "solo", 1, 5172, prompt_path);
        assert!(script.contains("exec sleep 60"));
        assert!(!script.contains("claude"));
        std::env::remove_var(TMUX_CMD_OVERRIDE_ENV);
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
            "maestro-20260506-143000-a3f1b2c-solo-iter-1"
        );
    }

    #[test]
    fn manager_session_name_format() {
        assert_eq!(
            manager_session_name("20260506-143000-a3f1b2c"),
            "maestro-mgr-20260506-143000-a3f1b2c"
        );
    }
}
