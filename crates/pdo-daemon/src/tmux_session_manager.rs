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

/// Default wall-clock bound for a `script` node's bash body (#248 / ADR-0017).
/// Mirrors the trigger guard's 60 s (`guard_runner`). A script has no JSONL, so
/// the stale-detector can never fire on it — the in-wrapper `timeout` is the
/// *only* thing that bounds a hung script, hence it is mandatory, not optional.
pub const SCRIPT_TIMEOUT_SECS: u64 = 60;

/// What a spawned tmux session runs after the shared `PDO_*` env exports.
///
/// The default is [`SessionTail::Agent`] — launch `claude` with the node's
/// prompt. A [`SessionTail::Script`] node (#248 / ADR-0017) instead runs the
/// author's bash body under a `timeout` and self-signals via `pdo complete` /
/// `pdo fail` — no LLM, no `tmux_cmd_override` (a script *is* deterministic
/// bash, so the test seam must not clobber it).
pub enum SessionTail<'a> {
    /// Agent node / manager / merge-resolver. `model` is the per-node model
    /// override (#296); `None` ⇒ account default (byte-identical legacy launch).
    Agent { model: Option<&'a str> },
    /// Script node (#248). Runs `timeout <secs>s bash <body>` then completes on
    /// exit 0 / fails otherwise. `env` is the `PDO_INPUT_*`/`PDO_OUTPUT_*`/… I/O
    /// catalogue exported before the body (a script can't read the prose
    /// preamble).
    Script {
        timeout_secs: u64,
        env: &'a [(String, String)],
    },
    /// Ad-hoc run shell (#316 / ADR-0021). Runs an interactive `bash -i` inside a
    /// `while true` respawn loop in the run's pipeline worktree — no LLM, no
    /// prompt file, no I/O catalogue. The loop is load-bearing: a bare `bash -i`
    /// exits on EOF (Ctrl-D / `exit` / PTY-bridge teardown) and, as the session's
    /// only window, takes the whole session down with it — the persistence bug
    /// caught in iteration 1's validation. Respawning keeps the pane (hence the
    /// session) alive for its whole lifetime. Deterministic like
    /// [`SessionTail::Script`], so it **ignores** `tmux_cmd_override` (the test
    /// seam must never swap the real bash for a `sleep`). Still
    /// `wrap_with_env`-wrapped so every respawned `bash -i` inherits
    /// `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1` and a user-typed `claude`
    /// can't SIGKILL live sibling sessions.
    Shell,
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
///
/// `extra_env` are additional `export K=V` pairs injected *after* the base four
/// and the CCR suppression, before the tail. Agents pass `&[]`, so the emitted
/// bytes are identical to the legacy command (the #296 byte-identity discipline)
/// — only `script` nodes populate it with the `PDO_INPUT_*`/`PDO_OUTPUT_*`/…
/// catalogue.
fn wrap_with_env(
    run_id: &str,
    node_id: &str,
    iter: i64,
    daemon_port: u16,
    extra_env: &[(String, String)],
    tail_cmd: &str,
) -> String {
    let extra_exports: String = extra_env
        .iter()
        .map(|(k, v)| format!("export {k}={} && ", sh_single_quote(v)))
        .collect();

    let inner = format!(
        "export PDO_RUN_ID={run_id_q} && \
         export PDO_NODE_ID={node_id_q} && \
         export PDO_NODE_ITER={iter_q} && \
         export PDO_DAEMON_URL={daemon_url_q} && \
         export CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 && \
         {extra_exports}{tail_cmd}",
        run_id_q = sh_single_quote(run_id),
        node_id_q = sh_single_quote(node_id),
        iter_q = sh_single_quote(&iter.to_string()),
        daemon_url_q = sh_single_quote(&format!("http://localhost:{daemon_port}")),
    );

    format!("exec bash -c {}", sh_single_quote(&inner))
}

/// Build the `exec claude …` tail for an agent node. `model` `Some(m)` inserts
/// `--model '<m>'`; `None` reproduces the legacy command byte-for-byte.
fn build_agent_tail(prompt_path: &Path, model: Option<&str>) -> String {
    // `Some` ⇒ a single-quoted `--model '<m>' ` with a trailing space;
    // `None` ⇒ empty string, so the command collapses to the exact legacy
    // literal (one space before the `"$(cat …)"`).
    let model_flag = match model {
        Some(m) => format!("--model {} ", sh_single_quote(m)),
        None => String::new(),
    };
    format!(
        "exec claude --dangerously-skip-permissions {}\"$(cat {})\"",
        model_flag,
        sh_single_quote(&prompt_path.to_string_lossy())
    )
}

/// Build the bash tail for a `script` node (#248 / ADR-0017).
///
/// Runs the author's body under `timeout` then self-signals: exit 0 ⇒
/// `pdo complete` (with a `pdo fail` fallback so a post-success output-validation
/// rejection still terminates the node); exit 124 (timeout) or any non-zero ⇒
/// `pdo fail` with a diagnostic reason. **Not** `exec`-ed: unlike the agent tail
/// (`exec claude`), the wrapper must run the bash *and then* run `pdo`, so it is
/// a plain sequence. Ordering `pdo complete` before shell exit makes the node
/// terminal before the session dies (#304).
fn build_script_tail(prompt_path: &Path, timeout_secs: u64) -> String {
    let body = sh_single_quote(&prompt_path.to_string_lossy());
    format!(
        "timeout {timeout_secs}s bash {body} ; ec=$? ; \
         if [ $ec -eq 0 ]; then pdo complete || pdo fail --reason \"output validation failed after script success\" ; \
         elif [ $ec -eq 124 ]; then pdo fail --reason \"script timed out after {timeout_secs}s\" ; \
         else pdo fail --reason \"script exited $ec\" ; fi"
    )
}

/// Construct the script tmux launches for a node run.
///
/// `tmux_cmd_override` replaces the default `claude …` tail when `Some` — the
/// per-daemon test seam (see [`TMUX_CMD_OVERRIDE_ENV`]). `None` → production
/// claude invocation. **Ignored for a [`SessionTail::Script`]** node: a script
/// *is* deterministic bash, so the override must not clobber it (a strictly
/// stronger property — a script node is end-to-end testable in CI with zero
/// stubbing).
///
/// `tail` selects the launch: [`SessionTail::Agent`] with the per-node `model`
/// (#296), or [`SessionTail::Script`] with its `timeout` and I/O env catalogue.
pub fn build_tmux_script(
    run_id: &str,
    node_id: &str,
    iter: i64,
    daemon_port: u16,
    prompt_path: &Path,
    tmux_cmd_override: Option<&str>,
    tail: SessionTail<'_>,
) -> String {
    const NO_ENV: &[(String, String)] = &[];
    let (tail_cmd, extra_env): (String, &[(String, String)]) = match tail {
        SessionTail::Script { timeout_secs, env } => {
            (build_script_tail(prompt_path, timeout_secs), env)
        }
        SessionTail::Shell => {
            // #316: a deterministic interactive bash. Like `Script`, the test
            // seam must not clobber it (`sleep 600` instead of a real shell is
            // useless and untestable), so `tmux_cmd_override` is ignored here.
            //
            // Respawn loop, NOT a bare `exec bash -i` (iteration 1 shipped that
            // and it failed the ADR-0021 #4 persistence check): an interactive
            // bash exits on EOF — a stray Ctrl-D, an explicit `exit`, or the PTY
            // bridge tearing the pane's input down when the modal/tab closes.
            // Being the session's only window, that exit destroys the whole
            // session, losing the long-running command (the `git bisect`) the
            // feature exists to preserve. Keeping the interactive shell inside a
            // `while true` loop makes the pane outlive any single bash: on exit a
            // fresh `bash -i` takes its place in the *same* pane (scrollback
            // preserved), so the session persists for its whole lifetime and is
            // torn down only by cleanup / the reaper. The `sleep 0.2` bounds the
            // loop if bash ever exits instantly (a pathological permanent-EOF
            // stdin) instead of busy-spinning. The env exports from
            // `wrap_with_env` sit before the loop, so every respawned bash
            // inherits `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1`.
            (
                "while true; do bash -i; sleep 0.2; done".to_string(),
                NO_ENV,
            )
        }
        SessionTail::Agent { model } => {
            let cmd = match tmux_cmd_override {
                Some(cmd) => cmd.to_string(),
                None => build_agent_tail(prompt_path, model),
            };
            (cmd, NO_ENV)
        }
    };

    wrap_with_env(run_id, node_id, iter, daemon_port, extra_env, &tail_cmd)
}

/// Build a resume script that uses `claude --continue` in the same working_dir.
///
/// `tmux_cmd_override` replaces the default `claude --continue` tail when
/// `Some` — the per-daemon test seam.
///
/// No `--model` is threaded here (#296): a resumed session keeps the model it
/// was launched with — "Resumed sessions started with `claude --resume`,
/// `--continue`, or the `/resume` picker keep the model they were using when
/// the transcript was saved" (https://code.claude.com/docs/en/model-config).
/// So `--continue` never silently downgrades the per-node model.
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

    wrap_with_env(run_id, node_id, iter, daemon_port, &[], &tail_cmd)
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

/// Session naming convention for an ad-hoc run shell (#316, ADR-0021).
///
/// One fixed name per Run so `POST /sessions/{run_id}/shell` is create-if-absent
/// (a second click re-attaches the same session). Parsed back out by
/// [`parse_session_name`] via the `shell-` prefix branch, mirroring `mgr-`.
pub fn shell_session_name(run_id: &str) -> String {
    format!("pdo-shell-{run_id}")
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
    tail: SessionTail<'_>,
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
        tail,
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

/// Spawn a detached tmux session running an interactive `bash -i` (#316 / ADR-0021).
///
/// Mirror of [`spawn`] minus the prompt file: an ad-hoc shell has no prompt, no
/// node, and no I/O catalogue. The session is env-wrapped (`__shell__`, iter 0)
/// so a user-typed `claude` inherits `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1`,
/// and it **ignores** `tmux_cmd_override` (see [`SessionTail::Shell`]).
pub fn spawn_shell(
    session_name: &str,
    working_dir: &Path,
    run_id: &str,
    daemon_port: u16,
) -> Result<()> {
    // prompt_path is unused for `SessionTail::Shell` (bash has no prompt);
    // pass the working_dir as a harmless placeholder.
    let script = build_tmux_script(
        run_id,
        "__shell__",
        0,
        daemon_port,
        working_dir,
        None,
        SessionTail::Shell,
    );
    let socket = tmux_socket_name(daemon_port);

    let output = tmux(&socket)
        .args(["new-session", "-d", "-s", session_name, "-c"])
        .arg(working_dir)
        .arg(&script)
        .output()
        .context("failed to run tmux new-session (shell)")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("tmux new-session (shell) failed: {stderr}");
    }

    enable_mouse(&socket, session_name);

    info!("Spawned run shell tmux session: {session_name}");
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

/// Check whether a tmux session exists.
pub fn session_exists(socket: &str, session_name: &str) -> bool {
    tmux(socket)
        .args(["has-session", "-t", session_name])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Check whether the tmux *server* for this socket is alive at all (#234).
///
/// `tmux -L <socket> ls` exits non-zero ("no server running on …") once the
/// socket's server is gone. This is the single most discriminating fact when a
/// node's session is found dead: a dead server means the whole socket collapsed
/// and *every* session under it died at once (e.g. an external `kill <pid>` of
/// the server process), not just this one node — see the session-death
/// diagnostics in [`crate::stale_detector::SessionDeathDiagnostics`].
///
/// `Some(true)` = server alive, `Some(false)` = server gone, `None` = the
/// `tmux` probe itself could not be run (so absence is never read as a real
/// "server gone").
pub fn server_alive(socket: &str) -> Option<bool> {
    tmux(socket)
        .args(["ls"])
        .output()
        .map(|o| o.status.success())
        .ok()
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
    /// Ad-hoc run shell `pdo-shell-<run_id>` (#316 / ADR-0021).
    Shell {
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

    // #316: `pdo-shell-<run_id>` — parsed BEFORE the `-iter-` split (a shell name
    // has no `-iter-` suffix, so it would otherwise return None and be killed as
    // "unrecognised" by the orphan sweep).
    if let Some(run_id) = rest.strip_prefix("shell-") {
        if !run_id.is_empty() {
            return Some(ParsedSession::Shell {
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

/// Resolve the reaper TTL, `stored → env → default` (#129, ADR-0015).
///
/// `stored_secs` is the instance-wide setting persisted via the settings page
/// (or `None` when unset). A stored value `>= 1` wins; otherwise the env var
/// [`REAPER_TTL_SECS_ENV`] applies; otherwise [`DEFAULT_REAPER_TTL`]. A stored
/// `0` is ignored (a zero TTL would reap sessions the instant they complete).
///
/// The module stays pure: the caller loads the stored value and passes it in.
/// [`reaper_ttl`] is the `stored = None` shorthand (env-only, unchanged).
///
/// **Load-bearing (ADR-0015):** the reaper reads this **inside its sweep loop**,
/// not once at boot — otherwise a `PUT /settings` is a silent no-op until the
/// daemon restarts.
pub fn reaper_ttl_with(stored_secs: Option<u64>) -> Duration {
    stored_secs
        .filter(|&n| n >= 1)
        .or_else(env_reaper_ttl_secs)
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_REAPER_TTL)
}

/// Read the reaper TTL from the env var alone (`stored = None`).
pub fn reaper_ttl() -> Duration {
    reaper_ttl_with(None)
}

/// The reaper TTL (seconds) contributed by [`REAPER_TTL_SECS_ENV`] alone, or
/// `None` when unset or unparseable.
///
/// Exposed so `GET /settings` can disclose a shadowed env var and compute the
/// winning tier identically to [`reaper_ttl_with`] (#129, ADR-0015).
pub fn env_reaper_ttl_secs() -> Option<u64> {
    std::env::var(REAPER_TTL_SECS_ENV)
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
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
            ParsedSession::Shell { ref run_id } => {
                // #316: mirror the Manager arm — reap iff the run is absent or
                // archived, NEVER on a TTL (an interactive shell must not be
                // yanked from a user who stepped away). The `__shell__` lookup
                // branch supplies the run's archived flag.
                let info = lookup(run_id, "__shell__", 0);
                match info {
                    None => {
                        info!("Orphan sweep: killing shell session for absent run {run_id}");
                        kill(socket, session_name);
                    }
                    Some(info) if info.is_archived => {
                        info!("Orphan sweep: killing shell session for archived run {run_id}");
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
    fn parse_shell_session() {
        // #316: `pdo-shell-<run_id>` parses to a Shell variant, even though the
        // run_id itself contains dashes and no `-iter-` suffix.
        let name = "pdo-shell-20260506-143000-a3f1b2c";
        let parsed = parse_session_name(name).unwrap();
        assert_eq!(
            parsed,
            ParsedSession::Shell {
                run_id: "20260506-143000-a3f1b2c".into(),
            }
        );
    }

    #[test]
    fn parse_garbage_returns_none() {
        assert!(parse_session_name("foo-bar").is_none());
        assert!(parse_session_name("pdo-").is_none());
        assert!(parse_session_name("pdo-mgr-").is_none());
        assert!(parse_session_name("pdo-shell-").is_none());
    }

    #[test]
    fn build_script_default_and_override() {
        let prompt_path = Path::new("/tmp/test-prompt.md");

        // None → production claude tail.
        let script = build_tmux_script(
            "run-abc",
            "solo",
            1,
            5172,
            prompt_path,
            None,
            SessionTail::Agent { model: None },
        );
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
            SessionTail::Agent { model: None },
        );
        assert!(script.contains("exec sleep 60"));
        assert!(!script.contains("claude"));
    }

    #[test]
    fn build_script_omits_model_when_none() {
        // #296: the `None` model path must reproduce the legacy command
        // byte-for-byte — no `--model`, exactly one space before `"$(cat …)"`.
        // This is the byte-identity guard: adding the flag must not perturb the
        // default launch.
        let prompt_path = Path::new("/tmp/test-prompt.md");
        let script = build_tmux_script(
            "run-abc",
            "solo",
            1,
            5172,
            prompt_path,
            None,
            SessionTail::Agent { model: None },
        );
        assert!(
            !script.contains("--model"),
            "no model flag when unset: {script}"
        );
        // The exact legacy tail, single space before the cat substitution.
        assert!(
            script.contains("exec claude --dangerously-skip-permissions \"$(cat "),
            "legacy tail must be byte-identical: {script}"
        );
    }

    #[test]
    fn build_script_inserts_model_when_some() {
        // #296: `Some(model)` inserts a single-quoted `--model '<m>'` between
        // `--dangerously-skip-permissions` and the prompt `cat` substitution.
        //
        // The whole tail is re-wrapped in `bash -c '…'` by `wrap_with_env`, so
        // the single quotes around the model value get rewritten by
        // `sh_single_quote` as `'\''` — i.e. `--model 'opus'` becomes
        // `--model '\''opus'\''` in the final script bytes.
        let prompt_path = Path::new("/tmp/test-prompt.md");
        let script = build_tmux_script(
            "run-abc",
            "solo",
            1,
            5172,
            prompt_path,
            None,
            SessionTail::Agent {
                model: Some("opus"),
            },
        );
        assert!(script.contains("--model"), "model flag present: {script}");
        assert!(
            script.contains(r"--model '\''opus'\''"),
            "model value single-quoted (bash -c escaping): {script}"
        );
        // The flag sits right after the base flag, before the prompt cat.
        assert!(
            script.contains(r"--dangerously-skip-permissions --model '\''opus'\'' "),
            "model flag must sit right after the base flag: {script}"
        );
        let model_at = script.find("--model").unwrap();
        let cat_at = script.find("$(cat").unwrap();
        assert!(
            model_at < cat_at,
            "model flag must precede the prompt cat: {script}"
        );
    }

    #[test]
    fn build_script_tail_runs_bash_and_self_signals() {
        // #248: a script node's tail runs the author's bash under `timeout`,
        // then completes on exit 0 / fails on non-zero or timeout. No claude,
        // and the tail is NOT `exec`-ed (the wrapper must run bash *then* pdo).
        let prompt_path = Path::new("/tmp/body.md");
        let script = build_tmux_script(
            "run-abc",
            "solo",
            1,
            5172,
            prompt_path,
            None,
            SessionTail::Script {
                timeout_secs: 42,
                env: &[],
            },
        );
        assert!(script.starts_with("exec bash -c "));
        assert!(
            !script.contains("claude"),
            "script node launches no claude: {script}"
        );
        assert!(
            script.contains("timeout 42s bash"),
            "runs body under timeout: {script}"
        );
        assert!(
            script.contains("pdo complete"),
            "completes on success: {script}"
        );
        assert!(
            script.contains("pdo fail --reason"),
            "fails otherwise: {script}"
        );
        assert!(
            script.contains("script exited $ec"),
            "reports the exit code: {script}"
        );
        assert!(
            script.contains("script timed out after 42s"),
            "reports timeout: {script}"
        );
        // Base env is still exported.
        assert!(script.contains("PDO_RUN_ID"));
        assert!(script.contains("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1"));
    }

    #[test]
    fn build_script_tail_bypasses_cmd_override() {
        // #248: the test seam (`tmux_cmd_override`) swaps claude for a stub so CI
        // never launches real claude. A script IS deterministic bash, so the
        // override must NOT clobber it — the wrapper is built unconditionally.
        let prompt_path = Path::new("/tmp/body.md");
        let script = build_tmux_script(
            "run-abc",
            "solo",
            1,
            5172,
            prompt_path,
            Some("exec sleep 99"),
            SessionTail::Script {
                timeout_secs: 60,
                env: &[],
            },
        );
        assert!(
            !script.contains("sleep 99"),
            "override ignored for scripts: {script}"
        );
        assert!(
            script.contains("timeout 60s bash"),
            "script tail preserved: {script}"
        );
    }

    #[test]
    fn build_script_tail_injects_env_catalogue() {
        // #248: a script can't read the prose preamble, so its I/O arrives as env
        // vars, exported before the body — after the base four (byte-identity for
        // agents preserved: they pass an empty env).
        let prompt_path = Path::new("/tmp/body.md");
        let env = vec![
            (
                "PDO_INPUT_TASK".to_string(),
                "/art/_input/output.md".to_string(),
            ),
            (
                "PDO_OUTPUT_OUT".to_string(),
                "/art/solo/iter-1/out/output.md".to_string(),
            ),
        ];
        let script = build_tmux_script(
            "run-abc",
            "solo",
            1,
            5172,
            prompt_path,
            None,
            SessionTail::Script {
                timeout_secs: 60,
                env: &env,
            },
        );
        assert!(
            script.contains("export PDO_INPUT_TASK="),
            "input env exported: {script}"
        );
        assert!(
            script.contains("export PDO_OUTPUT_OUT="),
            "output env exported: {script}"
        );
        // The env exports precede the body invocation.
        let env_at = script.find("PDO_OUTPUT_OUT").unwrap();
        let body_at = script.find("timeout 60s bash").unwrap();
        assert!(
            env_at < body_at,
            "env must be exported before the body runs: {script}"
        );
    }

    #[test]
    fn build_script_tail_shell_runs_env_wrapped_bash() {
        // #316: the run shell tail is an interactive bash inside a respawn loop
        // (a bare `exec bash -i` dies on EOF and takes the session with it —
        // iteration 1's persistence bug). Still env-wrapped so every respawned
        // bash inherits CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 and never
        // SIGKILLs live sibling sessions. No claude, no prompt cat.
        let prompt_path = Path::new("/unused");
        let script = build_tmux_script(
            "run-abc",
            "__shell__",
            0,
            5172,
            prompt_path,
            None,
            SessionTail::Shell,
        );
        assert!(script.starts_with("exec bash -c "));
        assert!(
            script.contains("bash -i"),
            "runs interactive bash: {script}"
        );
        assert!(
            script.contains("while true; do bash -i; sleep 0.2; done"),
            "interactive bash is wrapped in a respawn loop so an EOF/exit can't \
             destroy the session (ADR-0021 #4): {script}"
        );
        assert!(
            !script.contains("claude"),
            "shell launches no claude: {script}"
        );
        assert!(script.contains("PDO_RUN_ID"));
        assert!(
            script.contains("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1"),
            "env-safety export present: {script}"
        );
    }

    #[test]
    fn build_script_tail_shell_bypasses_cmd_override() {
        // #316: like a script node, the shell IS deterministic bash — the test
        // seam (`tmux_cmd_override`) must NOT swap it for a `sleep`.
        let prompt_path = Path::new("/unused");
        let script = build_tmux_script(
            "run-abc",
            "__shell__",
            0,
            5172,
            prompt_path,
            Some("exec sleep 600"),
            SessionTail::Shell,
        );
        assert!(
            !script.contains("sleep 600"),
            "override ignored for the run shell: {script}"
        );
        assert!(
            script.contains("while true; do bash -i; sleep 0.2; done"),
            "shell tail (respawn loop) preserved: {script}"
        );
    }

    #[test]
    fn shell_session_name_format() {
        assert_eq!(
            shell_session_name("20260506-143000-a3f1b2c"),
            "pdo-shell-20260506-143000-a3f1b2c"
        );
    }

    #[test]
    fn reaper_ttl_default_and_from_env() {
        // Single test on purpose: `REAPER_TTL_SECS_ENV` is process-global, so a
        // second test mutating it concurrently would flake. The stored-precedence
        // assertions (#129, ADR-0015) therefore live here too.
        std::env::remove_var(REAPER_TTL_SECS_ENV);
        assert_eq!(reaper_ttl(), Duration::from_secs(3600));

        std::env::set_var(REAPER_TTL_SECS_ENV, "5");
        assert_eq!(reaper_ttl(), Duration::from_secs(5));

        // --- stored → env → default precedence (#129, ADR-0015) ---
        // Stored wins over env.
        assert_eq!(reaper_ttl_with(Some(120)), Duration::from_secs(120));
        // A zero stored value is ignored → falls through to env.
        assert_eq!(reaper_ttl_with(Some(0)), Duration::from_secs(5));
        // No stored value → env applies.
        assert_eq!(reaper_ttl_with(None), Duration::from_secs(5));
        // No stored and no env → default; stored still wins when env is unset.
        std::env::remove_var(REAPER_TTL_SECS_ENV);
        assert_eq!(reaper_ttl_with(None), DEFAULT_REAPER_TTL);
        assert_eq!(reaper_ttl_with(Some(90)), Duration::from_secs(90));
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
