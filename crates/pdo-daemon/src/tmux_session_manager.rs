//! Deep module — the single path through which the daemon touches tmux.
//!
//! Exposes: spawn / capture / kill / list / session_exists / reaper / orphan-sweep / resume.
//! Nothing outside this module should shell out to `tmux`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use tracing::{info, warn};

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
/// node): the new daemon's boot-time orphan sweep runs against an empty
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
    Agent {
        /// The resolved harness descriptor (#550, ADR-0045). Its `launch`
        /// template renders the tail via [`crate::harness_argv`]; its `env` block
        /// is exported before the tail (that is where `claude`'s CCR suppression
        /// now comes from — AC #4). For infra sessions and the byte-identity gate
        /// this is the `claude` descriptor, whose template reproduces the legacy
        /// tail exactly once its holes are empty.
        harness: &'a crate::harness_registry::HarnessDescriptor,
        model: Option<&'a str>,
        /// Per-node reasoning-effort override (#424). `None` *or* an empty string
        /// ⇒ no `--effort`, byte-identical tail. A `Script` tail carries no such
        /// field, so the type itself guarantees a `script` node never gets one.
        effort: Option<&'a str>,
        /// #473: the PDO-pinned Claude Code session id (`claude --session-id
        /// <uuid>`). Claude Code names its transcript `<session_id>.jsonl`, so
        /// pinning it lets the liveness sweep resolve a node's transcript by
        /// *identity* instead of by the newest `.jsonl` in a cwd it shares with the
        /// manager and any sibling non-`code-mutating`/`merge` node. `None` *or* an
        /// empty string ⇒ no `--session-id`, byte-identical legacy tail — the state
        /// for infra sessions (`__manager__` / `__merge_resolver__`) that own no
        /// `NodeStarted`, are never probed by the sweep and are never resumed.
        session_id: Option<&'a str>,
    },
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

/// Single-quote a shell word only when it contains a character that isn't safe
/// bare. Keeps the emitted `docker exec …` argv readable (and its golden stable):
/// simple tokens (`docker`, `exec`, `-i`, `PDO_OUTPUT_out=/a/b`, `1000:1000`) stay
/// bare; anything with a space / quote / `$` / glob char is quoted. Used only to
/// splice the `docker exec` argv into the `bash -c` tail string.
fn sh_quote_arg(s: &str) -> String {
    let safe = !s.is_empty()
        && s.bytes().all(|b| {
            b.is_ascii_alphanumeric()
                || matches!(
                    b,
                    b'_' | b'-' | b'.' | b'/' | b'=' | b':' | b',' | b'@' | b'+'
                )
        });
    if safe {
        s.to_string()
    } else {
        sh_single_quote(s)
    }
}

// ---------------------------------------------------------------------------
// Sandbox wrapping (#407)
// ---------------------------------------------------------------------------

/// Threaded into [`build_tmux_script`] / [`build_resume_script`] when a Run is
/// sandboxed (`sandbox != off`, #407). When present, the node's tail is wrapped in
/// a `docker exec … pdo-sbx-<run_id> bash -lc '<tail>'` so it runs **inside** the
/// Run's long-lived container instead of on the host.
///
/// The base `PDO_*` env exports still run on the *host* side of the wrapper
/// (harmless), so `off` byte-identity is preserved when this is `None`.
/// `PDO_DAEMON_URL=localhost` is therefore **not** re-forwarded into the container
/// (the create posted `host.docker.internal`); the dynamic per-node catalogue (a
/// `script` node's `PDO_INPUT_*`/`PDO_OUTPUT_*`/…) is forwarded as explicit
/// `-e K=V` on the exec, since a bare host export wouldn't cross the exec.
pub struct SandboxWrap<'a> {
    /// The `docker` binary to invoke (per-daemon override → `"docker"`).
    pub docker_bin: &'a str,
    pub uid: u32,
    pub gid: u32,
    /// The session marker: **MUST** equal the tmux session name the kill path uses
    /// (`PDO_SBX_SESSION`), or the targeted `/proc` kill misses its tree.
    pub marker: &'a str,
    /// The node's working dir → `docker exec -w <workdir>` (the load-bearing cwd
    /// inside the container).
    pub workdir: &'a Path,
}

/// Splice the container-exec prefix (from [`crate::sandbox_container`]) around a
/// base tail: `docker <exec argv incl. -e K=V> pdo-sbx-<run> bash -lc '<tail>'`.
/// The base tail is single-quoted as one `bash -lc` argument; `wrap_with_env`
/// then single-quotes the whole thing again for its outer `bash -c` (the same
/// double-quoting the `--model` path already relies on).
fn wrap_tail_in_docker_exec(
    run_id: &str,
    wrap: &SandboxWrap<'_>,
    extra_env: &[(String, String)],
    base_tail: &str,
) -> String {
    let mut argv = vec![wrap.docker_bin.to_string()];
    argv.extend(crate::sandbox_container::exec_prefix_with_env(
        run_id,
        wrap.uid,
        wrap.gid,
        wrap.workdir,
        wrap.marker,
        extra_env,
    ));
    argv.push("bash".to_string());
    argv.push("-lc".to_string());
    argv.push(base_tail.to_string());
    argv.iter()
        .map(|a| sh_quote_arg(a))
        .collect::<Vec<_>>()
        .join(" ")
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
/// `harness_env` are the harness descriptor's env pairs (`claude`'s CCR
/// suppression — AC #4), exported *after* the base four and *before* `extra_env`.
/// For the `claude` descriptor this is exactly `[(CCR, "1")]`, so the emitted
/// bytes match the legacy hard-coded `export CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1`
/// — the value is rendered by [`sh_quote_arg`] (bare-if-safe), which leaves `1`
/// unquoted, preserving byte-identity. `script` / `shell` tails pass the same
/// forced `[(CCR, "1")]` (the wrapper still poses it — a hand-typed `claude` in a
/// run shell depends on it, CONTEXT.md §*Shell de run*).
///
/// `extra_env` are additional `export K=V` pairs injected after `harness_env`,
/// before the tail. Agents pass `&[]`, so the emitted bytes are identical to the
/// legacy command (the #296 byte-identity discipline) — only `script` nodes
/// populate it with the `PDO_INPUT_*`/`PDO_OUTPUT_*`/… catalogue.
fn wrap_with_env(
    run_id: &str,
    node_id: &str,
    iter: i64,
    daemon_port: u16,
    harness_env: &[(String, String)],
    extra_env: &[(String, String)],
    tail_cmd: &str,
) -> String {
    // #550/AC #4: the harness env (CCR for `claude`) — `sh_quote_arg` keeps a safe
    // value like `1` bare, so `export CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1`
    // is byte-identical to the legacy hard-coded export.
    let harness_exports: String = harness_env
        .iter()
        .map(|(k, v)| format!("export {k}={} && ", sh_quote_arg(v)))
        .collect();
    let extra_exports: String = extra_env
        .iter()
        .map(|(k, v)| format!("export {k}={} && ", sh_single_quote(v)))
        .collect();

    let inner = format!(
        "export PDO_RUN_ID={run_id_q} && \
         export PDO_NODE_ID={node_id_q} && \
         export PDO_NODE_ITER={iter_q} && \
         export PDO_DAEMON_URL={daemon_url_q} && \
         {harness_exports}{extra_exports}{tail_cmd}",
        run_id_q = sh_single_quote(run_id),
        node_id_q = sh_single_quote(node_id),
        iter_q = sh_single_quote(&iter.to_string()),
        // #447: `sandboxed = false` is NOT an oversight — these exports run on the
        // HOST side of the `docker exec` wrapper (see `build_tmux_script`: the
        // wrapped tail is the *argument* of `wrap_with_env`, so the exports execute
        // before the exec and never cross into the container). Resolving to the
        // gateway here would hand the host path a hostname it can't resolve, and
        // would be dead bytes on the sandbox path — the `docker create` already
        // posted the container-side value, which the exec deliberately never
        // clobbers (ADR-0030 §5). Routed through the resolver anyway so the literal
        // lives in exactly one module.
        daemon_url_q = sh_single_quote(&crate::sandbox_container::daemon_url(daemon_port, false)),
    );

    format!("exec bash -c {}", sh_single_quote(&inner))
}

/// The settings injected via `claude --settings` to arm a turn-end `Stop` hook
/// (#433, ADR-0043). On every turn end the hook runs `pdo complete --auto` and
/// then `exit 0` **unconditionally**: a `Stop` hook only forces the turn to
/// continue on `exit 2` / `{"decision":"block"}`, so the `; exit 0` swallows the
/// recoverable `exit 3` of a still-missing output (ADR-0035 — nothing recorded)
/// and the hook can never loop or complete prematurely. `--settings` MERGES
/// additively over `~/.claude` (confirmed against the pinned CLI: "load
/// additional settings"), so this clobbers no user hook. `pdo` is on the session
/// PATH (host and container) and the hook inherits the `PDO_*`/`PDO_DAEMON_URL`
/// exports `wrap_with_env` set, so no env has to be threaded into the JSON.
pub(crate) const STOP_HOOK_SETTINGS_JSON: &str = r#"{"hooks":{"Stop":[{"matcher":"","hooks":[{"type":"command","command":"pdo complete --auto; exit 0"}]}]}}"#;

/// Build the agent tail by rendering the harness descriptor's launch template
/// through [`crate::harness_argv`] (#550, ADR-0045). The caller shell-quotes each
/// hole value here — where the shell semantics live — and the renderer only
/// substitutes and drops empty-hole tokens. For the `claude` descriptor with all
/// holes empty this reproduces the legacy `build_agent_tail` **byte for byte**
/// (the #550 gate, pinned by the goldens in `harness_argv`): a non-empty `model`
/// inserts `--model '<m>'`, `effort` inserts `--effort '<lvl>'` after it, a `Some`
/// `settings_path` inserts `--settings '<file>'` (#433), and a non-empty
/// `session_id` inserts `--session-id '<uuid>'` last (#473). `None` *or* an empty
/// string on any of them drops its token (the `Some("")` last-resort guard of
/// #347: a stray `""` never reaches the tail as `--model ''`).
fn build_agent_tail(
    descriptor: &crate::harness_registry::HarnessDescriptor,
    prompt_path: &Path,
    model: Option<&str>,
    effort: Option<&str>,
    settings_path: Option<&Path>,
    session_id: Option<&str>,
) -> String {
    let quote_opt = |v: Option<&str>| {
        v.filter(|s| !s.is_empty())
            .map(sh_single_quote)
            .unwrap_or_default()
    };
    let holes = crate::harness_argv::Holes {
        prompt: format!(
            "\"$(cat {})\"",
            sh_single_quote(&prompt_path.to_string_lossy())
        ),
        model: quote_opt(model),
        effort: quote_opt(effort),
        settings: settings_path
            .map(|p| sh_single_quote(&p.to_string_lossy()))
            .unwrap_or_default(),
        session_id: quote_opt(session_id),
        // The launch template carries no `{resume}` hole.
        resume: String::new(),
    };
    crate::harness_argv::render(&descriptor.launch, &holes)
}

/// The `claude` CCR suppression, forced by the wrapper for `script` / `shell`
/// tails (AC #4): those run bash, not an agent, so they carry no descriptor — but
/// a hand-typed `claude` in a run shell still depends on it (CONTEXT.md §*Shell de
/// run*). An **agent** tail gets this from its descriptor's `env` instead.
fn forced_ccr_env() -> Vec<(String, String)> {
    vec![(
        "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC".to_string(),
        "1".to_string(),
    )]
}

/// Build the bash tail for a `script` node (#248 / ADR-0017).
///
/// Runs the author's body under `timeout` then self-signals: exit 0 ⇒
/// `pdo complete`; exit 124 (timeout) or any non-zero ⇒ `pdo fail` with a
/// diagnostic reason. **Not** `exec`-ed: unlike the agent tail (`exec claude`), the
/// wrapper must run the bash *and then* run `pdo`, so it is a plain sequence.
/// Ordering `pdo complete` before shell exit makes the node terminal before the
/// session dies (#304).
///
/// **The `pdo complete` arm branches on exit code `4`** (#490, ADR-0035 §4). Before
/// #490 it was a bare `pdo complete || pdo fail --reason "…"`, and that `||` was
/// dead code: every completion refusal answered `200`, so `pdo complete` exited `0`
/// and the fallback never fired. Making refusals non-2xx woke it up — and a
/// terminal refusal (`4`) would then append a **second** `NodeFailed` **and** a
/// second `RunFailed`, the latter unguarded and carrying a false reason ("after
/// script success", on a script whose output validation had just failed). So the
/// fallback fires only on a code that is neither `0` (granted or legal duplicate)
/// nor `4` (already ruled — the daemon recorded the failure itself). A `3`
/// (refused, still your turn) cannot reach a script node: the fail-fast branch
/// intercepts before the interactive retry loop.
fn build_script_tail(prompt_path: &Path, timeout_secs: u64) -> String {
    let body = sh_single_quote(&prompt_path.to_string_lossy());
    format!(
        "timeout {timeout_secs}s bash {body} ; ec=$? ; \
         if [ $ec -eq 0 ]; then pdo complete ; cc=$? ; \
         if [ $cc -ne 0 ] && [ $cc -ne 4 ]; then pdo fail --reason \"pdo complete refused with exit $cc after script success\" ; fi ; \
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
// Every argument is an irreducible input to the script the session runs (identity,
// working context, launch selector, sandbox wrap); a struct would only move the
// list, not shorten it — same rationale as `spawn`.
#[allow(clippy::too_many_arguments)]
pub fn build_tmux_script(
    run_id: &str,
    node_id: &str,
    iter: i64,
    daemon_port: u16,
    prompt_path: &Path,
    tmux_cmd_override: Option<&str>,
    tail: SessionTail<'_>,
    sandbox: Option<&SandboxWrap<'_>>,
    settings_path: Option<&Path>,
) -> String {
    const NO_ENV: &[(String, String)] = &[];
    // #550/AC #4: `harness_env` sources the CCR export — from the harness
    // descriptor for an agent tail, forced by the wrapper for `script` / `shell`.
    // (Type inferred, not annotated: an explicit 3-tuple type trips clippy's
    // `type_complexity`, which CI denies.)
    let (tail_cmd, extra_env, harness_env) = match tail {
        SessionTail::Script { timeout_secs, env } => (
            build_script_tail(prompt_path, timeout_secs),
            env,
            forced_ccr_env(),
        ),
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
                forced_ccr_env(),
            )
        }
        SessionTail::Agent {
            harness,
            model,
            effort,
            session_id,
        } => {
            // #433: `settings_path` (the turn-end `Stop` hook) is honoured ONLY on
            // the agent tail — a `Script`/`Shell` tail runs bash, never `claude`,
            // so it structurally cannot carry `--settings`. #473: `session_id` pins
            // the transcript identity, threaded through the enum variant. #550: the
            // harness descriptor's `launch` template renders the tail and its `env`
            // block carries the CCR suppression (AC #4).
            let cmd = match tmux_cmd_override {
                Some(cmd) => cmd.to_string(),
                None => build_agent_tail(
                    harness,
                    prompt_path,
                    model,
                    effort,
                    settings_path,
                    session_id,
                ),
            };
            (cmd, NO_ENV, harness.env.clone())
        }
    };

    // #407: when sandboxed, the node's tail runs INSIDE the container via a
    // `docker exec … bash -lc '<tail>'`. The per-node catalogue is forwarded on
    // the exec as explicit `-e K=V` (never PDO_DAEMON_URL); the host-side base env
    // exports still run (harmless) so `PDO_DAEMON_URL=localhost` is not carried
    // in. `wrap_with_env` then gets an EMPTY `extra_env` — the catalogue crossed
    // the exec, not the host export — keeping the host wrapper minimal.
    match sandbox {
        Some(wrap) => {
            let docker_tail = wrap_tail_in_docker_exec(run_id, wrap, extra_env, &tail_cmd);
            wrap_with_env(
                run_id,
                node_id,
                iter,
                daemon_port,
                &harness_env,
                NO_ENV,
                &docker_tail,
            )
        }
        None => wrap_with_env(
            run_id,
            node_id,
            iter,
            daemon_port,
            &harness_env,
            extra_env,
            &tail_cmd,
        ),
    }
}

/// Build a resume script that re-enters the node's saved conversation in the same
/// working_dir.
///
/// `tmux_cmd_override` replaces the default resume tail when `Some` — the
/// per-daemon test seam.
///
/// **#473 — resume by session identity.** When `session_id` is a non-empty pinned
/// id (recorded on `NodeStarted` at spawn), the tail is `claude --resume <uuid>`,
/// which re-enters *this node's* transcript. The pre-#473 tail was a bare
/// `--continue`, which re-enters "the most recent conversation of the cwd" — and
/// for a non-`code-mutating`/`merge` node that cwd is the Run worktree, shared with
/// the manager's `claude` and any sibling non-CM node, so a resumed node could pick
/// up the *manager's* (or a sibling's) conversation. `session_id` = `None` or empty
/// (a pre-#473 row with no recorded id) falls back to that bare `--continue`,
/// byte-identical to the legacy tail — no migration.
///
/// No `--model` is threaded here (#296): a resumed session keeps the model it
/// was launched with — "Resumed sessions started with `claude --resume`,
/// `--continue`, or the `/resume` picker keep the model they were using when
/// the transcript was saved" (https://code.claude.com/docs/en/model-config).
/// So resuming never silently downgrades the per-node model.
///
/// and nothing else. Measured on claude 2.1.220: `--effort xhigh` then a resume
/// reports `auto (currently high)` — the level is lost, and the transcript stores
/// no `effort` field for anything to read back. So the level is re-posed from the
/// `NodeStarted` payload (launch-time value, not the current YAML — ADR-0007: an
/// edit has no effect on a live node's current iter). `None` or an empty string ⇒
/// no `--effort`.
///
/// `settings_path` (#433 / ADR-0043 D7) re-arms the turn-end `Stop` hook so a
/// resurrected session does not silently lose it; `None` ⇒ no `--settings`.
// Every argument is an irreducible input to the resume script (identity, working
// context, launch selectors, sandbox wrap, settings path); bundling them into a
// struct would only move the list, not shorten it — same rationale as
// `resume` / `spawn` / `build_tmux_script`.
#[allow(clippy::too_many_arguments)]
fn build_resume_script(
    run_id: &str,
    node_id: &str,
    iter: i64,
    daemon_port: u16,
    descriptor: &crate::harness_registry::HarnessDescriptor,
    effort: Option<&str>,
    session_id: Option<&str>,
    tmux_cmd_override: Option<&str>,
    sandbox: Option<&SandboxWrap<'_>>,
    settings_path: Option<&Path>,
) -> String {
    // #473/#614: the resume *selector* is the one thing the pure hole-drop rule
    // cannot express (emit the blind verb precisely *when* the id is empty), so it
    // is computed here — but the VERBS are now the descriptor's property, not
    // constants (#614). `<resume_by_id> '<uuid>'` targets THIS node's transcript by
    // identity; a row with no recorded id (pre-#473, or a harness like `opencode`
    // that can't pin one) falls back to the harness's blind verb. A harness that
    // declares neither (an empty `resume_by_id`/`resume_blind` for the case at
    // hand) renders no resume flag: `copilot` resumes by identity or not at all,
    // never a blind continue (AC).
    let resume_selector = match session_id {
        Some(s) if !s.is_empty() && !descriptor.resume_by_id.is_empty() => {
            format!("{} {}", descriptor.resume_by_id, sh_single_quote(s))
        }
        _ => descriptor.resume_blind.clone(),
    };
    let quote_opt = |v: Option<&str>| {
        v.filter(|s| !s.is_empty())
            .map(sh_single_quote)
            .unwrap_or_default()
    };
    let holes = crate::harness_argv::Holes {
        resume: resume_selector,
        // #424: a resume restores the model but loses the effort, so re-pose it.
        effort: quote_opt(effort),
        // #433 / ADR-0043 (D7): re-arm the `Stop` hook on resume.
        settings: settings_path
            .map(|p| sh_single_quote(&p.to_string_lossy()))
            .unwrap_or_default(),
        ..Default::default()
    };
    let tail_cmd = match tmux_cmd_override {
        Some(cmd) => cmd.to_string(),
        None => crate::harness_argv::render(&descriptor.resume, &holes),
    };

    // #407: the resume tail is wrapped identically — a resumed sandboxed session
    // re-enters the same container. `--continue` matches its transcript by
    // working-dir path; the container mounts the repo at the same host path, so
    // the path (hence the transcript) still matches.
    let harness_env = descriptor.env.clone();
    match sandbox {
        Some(wrap) => {
            let docker_tail = wrap_tail_in_docker_exec(run_id, wrap, &[], &tail_cmd);
            wrap_with_env(
                run_id,
                node_id,
                iter,
                daemon_port,
                &harness_env,
                &[],
                &docker_tail,
            )
        }
        None => wrap_with_env(
            run_id,
            node_id,
            iter,
            daemon_port,
            &harness_env,
            &[],
            &tail_cmd,
        ),
    }
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

/// Session naming convention for a library pipeline authoring assistant
/// (#302 / ADR-0048).
///
/// One fixed name per pipeline id so `POST /sessions/{id}/libassist` is
/// create-if-absent (a second open re-attaches the same session). Parsed back out
/// by [`parse_session_name`] via the `libassist-` prefix branch, mirroring
/// `shell-` / `mgr-` — otherwise the orphan sweep would read it as an
/// unrecognised name and kill it on the next pass.
pub fn libassist_session_name(pipeline_id: &str) -> String {
    format!("pdo-libassist-{pipeline_id}")
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
    sandbox: Option<&SandboxWrap<'_>>,
    inject_hook: bool,
) -> Result<()> {
    let prompt_dir = working_dir.join(".pdo").join("prompts");
    std::fs::create_dir_all(&prompt_dir)?;
    let prompt_path = prompt_dir.join(format!("{node_id}-iter-{iter}.md"));
    std::fs::write(&prompt_path, prompt)?;

    // #433 / ADR-0043: when turn-end auto-completion is enabled, drop a settings
    // file beside the prompt and reference it via `claude --settings`, so a `Stop`
    // hook runs `pdo complete --auto` at every turn end. Same lifecycle as the
    // prompt (gitignored under `.pdo/`, resolves identically host and container).
    // Callers pass `false` for `script`/manager/merge-resolver sessions; the tail
    // selector in `build_tmux_script` is the belt-and-suspenders guard.
    //
    // #613/ADR-0051 (correctif 8): write the claude-format settings file ONLY for a
    // harness that actually has a `{settings}` hole to fill. A node on a harness
    // with none (`opencode`) would never reference the file — writing it beside the
    // prompt was the one place "absence is supplied, not said" leaked. Now the
    // absence is honoured: no hole, no file.
    let harness_takes_settings =
        matches!(&tail, SessionTail::Agent { harness, .. } if harness.has_settings_hole());
    let settings_path = if inject_hook && harness_takes_settings {
        let p = prompt_dir.join(format!("{node_id}-iter-{iter}.settings.json"));
        std::fs::write(&p, STOP_HOOK_SETTINGS_JSON)?;
        Some(p)
    } else {
        None
    };

    let script = build_tmux_script(
        run_id,
        node_id,
        iter,
        daemon_port,
        &prompt_path,
        tmux_cmd_override,
        tail,
        sandbox,
        settings_path.as_deref(),
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
    sandbox: Option<&SandboxWrap<'_>>,
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
        sandbox,
        // An ad-hoc shell runs `bash -i`, never `claude` — no `Stop` hook (#433).
        None,
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

/// Spawn a detached tmux session running the library pipeline authoring assistant
/// (#302 / ADR-0048) — a `claude` REPL whose cwd is the library pipelines
/// directory.
///
/// Mirror of [`spawn`]'s agent launch (an `Agent` tail, not the `bash -i` of
/// [`spawn_shell`]), but keyed on a pipeline id instead of a Run: it drives no
/// Run and emits no `run_command`; its whole effect is writing `<id>.yaml`
/// (+ `<id>.prompts/`) in `working_dir` via the library endpoints. Like the
/// manager it launches `claude "$(cat <primer>)"`; `claude` does **not** exit on
/// EOF (unlike `bash -i`), so the session survives a PTY-bridge/tab close — the
/// assistant is reaped **explicitly** on tab-leave (`DELETE /sessions/{id}/libassist`,
/// ADR-0048), never by the EOF that would end a run shell. The primer is written
/// to `prompt_path`, kept **out** of `working_dir` so the user-facing pipelines
/// directory stays clean. Honours `tmux_cmd_override` (the test seam) exactly as
/// the agent tail does. Never sandboxed: authoring is design-time work on the host.
// Every argument is an irreducible input to the spawn (identity, working context,
// primer path, launch selector); a struct would only move the list — same
// rationale as `spawn` / `spawn_shell` / `build_tmux_script`.
#[allow(clippy::too_many_arguments)]
pub fn spawn_libassist(
    session_name: &str,
    pipeline_id: &str,
    prompt: &str,
    working_dir: &Path,
    prompt_path: &Path,
    daemon_port: u16,
    harness: &crate::harness_registry::HarnessDescriptor,
    tmux_cmd_override: Option<&str>,
) -> Result<()> {
    if let Some(parent) = prompt_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(prompt_path, prompt)?;

    let script = build_tmux_script(
        // The env-wrap poses `PDO_RUN_ID=<pipeline_id>` / `PDO_NODE_ID=__libassist__`
        // (harmless — the assistant never self-signals via `pdo complete`); the
        // load-bearing export is `PDO_DAEMON_URL`, so a `curl` of the library
        // endpoints in the assistant's own preamble resolves.
        pipeline_id,
        "__libassist__",
        0,
        daemon_port,
        prompt_path,
        tmux_cmd_override,
        SessionTail::Agent {
            harness,
            model: None,
            effort: None,
            session_id: None,
        },
        None, // never sandboxed
        None, // no turn-end Stop hook — the assistant never calls `pdo complete`
    );
    let socket = tmux_socket_name(daemon_port);

    let output = tmux(&socket)
        .args(["new-session", "-d", "-s", session_name, "-c"])
        .arg(working_dir)
        .arg(&script)
        .output()
        .context("failed to run tmux new-session (libassist)")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("tmux new-session (libassist) failed: {stderr}");
    }

    enable_mouse(&socket, session_name);

    info!("Spawned library assistant tmux session: {session_name}");
    Ok(())
}

/// Resume a dead session in the original working_dir.
///
/// `effort` is the level the node was **launched** with, read back from its
/// `NodeStarted` event (#424) — a resume restores the model but not the effort,
/// so the flag has to be re-posed. Deliberately not re-resolved from the current
/// YAML: ADR-0007 makes a live node's current iteration immutable to edits.
///
/// `session_id` is the node's pinned Claude Code session id (#473), also read back
/// from `NodeStarted`. When present the resume is `--resume <uuid>` (this node's
/// own transcript); when absent (a pre-#473 row) it degrades to a bare
/// `--continue`.
#[allow(clippy::too_many_arguments)]
pub fn resume(
    session_name: &str,
    working_dir: &Path,
    run_id: &str,
    node_id: &str,
    iter: i64,
    daemon_port: u16,
    descriptor: &crate::harness_registry::HarnessDescriptor,
    effort: Option<&str>,
    session_id: Option<&str>,
    tmux_cmd_override: Option<&str>,
    sandbox: Option<&SandboxWrap<'_>>,
    inject_hook: bool,
) -> Result<()> {
    // #433 / ADR-0043 (D7): a resumed session must re-carry the `Stop` hook.
    // Re-write the settings file (idempotent; same path `spawn` used) and
    // reference it. `create_dir_all` covers the rare case where the prompt dir was
    // pruned since spawn. `false` (setting off, or a `script` node) ⇒ no file, and
    // `build_resume_script` emits a byte-identical `--continue` tail.
    //
    // #613/ADR-0051 (correctif 8): as at spawn, only a harness with a `{settings}`
    // hole gets the file — a resumed `opencode` node writes none.
    let settings_path = if inject_hook && descriptor.has_settings_hole() {
        let prompt_dir = working_dir.join(".pdo").join("prompts");
        std::fs::create_dir_all(&prompt_dir)?;
        let p = prompt_dir.join(format!("{node_id}-iter-{iter}.settings.json"));
        std::fs::write(&p, STOP_HOOK_SETTINGS_JSON)?;
        Some(p)
    } else {
        None
    };
    let script = build_resume_script(
        run_id,
        node_id,
        iter,
        daemon_port,
        descriptor,
        effort,
        session_id,
        tmux_cmd_override,
        sandbox,
        settings_path.as_deref(),
    );
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
    /// Library pipeline authoring assistant `pdo-libassist-<pipeline_id>`
    /// (#302 / ADR-0048). Owns no Run — reaped explicitly on tab-leave, kept by
    /// the orphan sweep.
    LibAssist {
        pipeline_id: String,
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

    // #302: `pdo-libassist-<pipeline_id>` — parsed BEFORE the `-iter-` split, like
    // `shell-` / `mgr-`. A pipeline id has no `-iter-` suffix, so without this
    // branch the name would fall through to the split, return None, and be killed
    // as "unrecognised" by the orphan sweep on the next pass.
    if let Some(pipeline_id) = rest.strip_prefix("libassist-") {
        if !pipeline_id.is_empty() {
            return Some(ParsedSession::LibAssist {
                pipeline_id: pipeline_id.to_string(),
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
///
/// A *facts* type on purpose (#485, ADR-0038): this deep module has no
/// dependency on [`crate::event_log`], so the caller projects the run and hands
/// the two facts down. Keeping the coupling this way round is what lets
/// [`decide_sweep`] stay pure and unit-testable without a database.
#[derive(Debug, Clone, PartialEq)]
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

/// Env var that supplies the instance-wide `default_model` at bootstrap (#347,
/// ADR-0015). No baked-in default: `None` ⇒ the account default (no `--model`),
/// byte-identical to the legacy launch.
pub const DEFAULT_MODEL_ENV: &str = "PDO_DEFAULT_MODEL";

/// The `default_model` contributed by [`DEFAULT_MODEL_ENV`] alone, or `None`
/// when the var is unset or empty.
///
/// Unlike the numeric knobs, a String has no `parse()` that rejects the empty
/// value for free — so the empty string is filtered explicitly. Without it,
/// `PDO_DEFAULT_MODEL=""` would resolve to `Some("")` and emit `--model ''`.
/// Exposed so `GET /settings` can disclose a shadowed env var and compute the
/// winning tier identically to [`default_model_with`].
pub fn env_default_model() -> Option<String> {
    std::env::var(DEFAULT_MODEL_ENV)
        .ok()
        .filter(|s| !s.is_empty())
}

/// Resolve the instance-wide default model, `stored → env → None` (#347,
/// ADR-0015). A non-empty stored value (the settings page) wins; otherwise the
/// env var; otherwise `None` (the account default — no baked-in const, unlike
/// the reaper TTL).
///
/// **Load-bearing (ADR-0015):** every spawn seam reads this *fresh* (via the
/// instance-config store), so a `PUT /settings` takes effect on the next node
/// without a daemon restart.
pub fn default_model_with(stored: Option<String>) -> Option<String> {
    stored.filter(|s| !s.is_empty()).or_else(env_default_model)
}

/// Env var supplying the instance-wide default **harness** (#550, ADR-0046).
/// `None`/empty ⇒ fall through to the `claude` floor, byte-identical launch.
pub const DEFAULT_HARNESS_ENV: &str = "PDO_DEFAULT_HARNESS";

/// The default harness contributed by [`DEFAULT_HARNESS_ENV`] alone, or `None`
/// when unset or empty. Exposed so `GET /settings` can disclose a shadowed env
/// var and compute the winning tier identically to [`default_harness_with`].
pub fn env_default_harness() -> Option<String> {
    std::env::var(DEFAULT_HARNESS_ENV)
        .ok()
        .filter(|s| !s.is_empty())
}

/// Resolve the instance-wide default harness, `stored → env → None` (#550,
/// ADR-0046, mirroring [`default_model_with`]). `None` ⇒ the resolver's `claude`
/// floor applies. Read FRESH at every spawn seam so a `PUT /settings` takes
/// effect on the next node without a daemon restart (ADR-0015).
pub fn default_harness_with(stored: Option<String>) -> Option<String> {
    stored
        .filter(|s| !s.is_empty())
        .or_else(env_default_harness)
}

/// The `PATH` the daemon searches for harness binaries (ADR-0055).
///
/// A daemon launched as a service inherits the **unit's** `PATH`, which misses the
/// entries a package manager (Homebrew, nvm, a user prefix) adds only to an
/// **interactive** shell — so a harness the user installed and can run by hand is
/// invisible to the service, and the spawn fails saying an installed binary does
/// not exist. This resolves the binary in the `PATH` the user has *when they type
/// the command*: the interactive shell's `PATH`, unioned with the process `PATH`
/// so nothing the service already saw is lost. Measured (ADR-0055): a *login*
/// shell does not suffice — package managers add their paths from the
/// **interactive** rc files — so the probe sources an interactive shell (`-i`).
///
/// Resolved **once** and cached for the daemon's lifetime: the cost of sourcing
/// the user's shell config is paid at the first probe, not per spawn, and a `PATH`
/// the user changes afterwards is seen only on the next daemon start (ADR-0055
/// limits, same freshness contract as the model catalogue, ADR-0053). The env
/// override `PDO_HARNESS_PROBE_PATH` short-circuits the shell probe (ops / tests).
pub fn harness_probe_path() -> String {
    static PROBE_PATH: OnceLock<String> = OnceLock::new();
    PROBE_PATH.get_or_init(resolve_harness_probe_path).clone()
}

/// Compute the probe `PATH` (uncached): the interactive shell's `PATH` unioned
/// with the process `PATH`. See [`harness_probe_path`].
fn resolve_harness_probe_path() -> String {
    // An explicit override wins outright — the deterministic seam a test or an
    // operator uses instead of the ambient shell.
    if let Some(p) = std::env::var_os("PDO_HARNESS_PROBE_PATH") {
        if !p.is_empty() {
            return p.to_string_lossy().into_owned();
        }
    }
    let process_path = std::env::var_os("PATH")
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    match interactive_path_via_shell() {
        Some(interactive) => union_paths(&interactive, &process_path),
        None => process_path,
    }
}

/// The user's `PATH` as an **interactive** shell reports it, or `None` when the
/// probe fails (no `$SHELL`, the shell errors, empty output). Runs `$SHELL -i -c`
/// so the interactive rc files — where version and package managers add their
/// paths — are sourced (a *login* shell, `-l`, is not enough; ADR-0055).
fn interactive_path_via_shell() -> Option<String> {
    let shell = std::env::var_os("SHELL").filter(|s| !s.is_empty())?;
    // `printf` with no trailing newline, stdout only — job-control chatter an
    // interactive shell may emit goes to stderr and is ignored.
    let output = std::process::Command::new(&shell)
        .arg("-i")
        .arg("-c")
        .arg(r#"printf '%s' "$PATH""#)
        .output()
        .ok()?;
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!path.is_empty()).then_some(path)
}

/// Union two `PATH` strings, `first` taking precedence, dropping duplicates and
/// empty entries while preserving order. Pure — the testable core of the ADR-0055
/// merge.
fn union_paths(first: &str, second: &str) -> String {
    let mut seen = std::collections::HashSet::new();
    let joined: Vec<String> = std::env::split_paths(first)
        .chain(std::env::split_paths(second))
        .filter(|p| !p.as_os_str().is_empty())
        .filter(|p| seen.insert(p.clone()))
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    joined.join(":")
}

/// Whether `binary` resolves in `path`. Pure — the testable core of the probe.
fn path_contains_binary(path: &str, binary: &str) -> bool {
    std::env::split_paths(path).any(|dir| dir.join(binary).is_file())
}

/// Whether `binary` resolves on the harness probe `PATH` — the fail-fast spawn
/// check (#550, AC #10). A name with a `/` is checked directly; a bare name is
/// searched across [`harness_probe_path`] (the user's interactive `PATH`, ADR-0055
/// — **not** the service's inherited one). **Never executes** the binary (a probe
/// run could hang a resident harness), and lives here (not in the pure
/// `harness_registry`) because it reads the environment. A missing binary makes
/// the spawn fail *before* any session or start event exists (ADR-0037); the
/// caller's diagnostic names [`harness_probe_path`] so "not found" cannot read as
/// "not installed".
pub fn binary_available(binary: &str) -> bool {
    if binary.is_empty() {
        return false;
    }
    if binary.contains('/') {
        return Path::new(binary).is_file();
    }
    path_contains_binary(&harness_probe_path(), binary)
}

/// How long a catalogue / version probe is allowed to run before it is killed and
/// treated as "no answer" (#616). `--help` / `--version` exit immediately on every
/// harness in play; the cap is a defence against a binary that blocks (a broken
/// install, a prompt on stdin), so a probe can never wedge the boot task or the
/// `/settings` response.
const CATALOGUE_PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// The version string of `binary`, read by running `<binary> --version` on the
/// harness probe `PATH` (#616, ADR-0053 §3). `None` when the binary can't be run,
/// times out, or prints nothing. This is the **freshness key** of the catalogue
/// cache: a changed version invalidates the cached catalogue and re-probes, so an
/// auto-updating binary is followed without a daemon restart.
///
/// UNLIKE [`binary_available`], this **executes** the binary — deliberately, and
/// only ever off the resident hot path: at daemon boot, and on a throttled version
/// re-check behind the `/settings` fetch. `--version` is non-interactive and exits
/// at once; the timeout is the backstop.
pub(crate) fn probe_version(binary: &str) -> Option<String> {
    probe_version_on(binary, &harness_probe_path())
}

/// The offered catalogue of `binary`, read by running `<binary> --help` on the
/// harness probe `PATH` and parsing it (#616, ADR-0053 §1). A binary that can't be
/// run, times out, or enumerates nothing yields [`harness_catalogue::Catalogue::default`]
/// — the free-text fallback. Executes the binary; see [`probe_version`] for why
/// that is safe here and not in [`binary_available`].
pub(crate) fn probe_catalogue(binary: &str) -> crate::harness_catalogue::Catalogue {
    probe_catalogue_on(binary, &harness_probe_path())
}

/// [`probe_version`] with an explicit `PATH` — the testable core (a test points it
/// at a tempdir holding a fake binary, no ambient shell).
pub(crate) fn probe_version_on(binary: &str, path: &str) -> Option<String> {
    let out = run_probe(binary, &["--version"], path)?;
    out.lines()
        .next()
        .map(|l| l.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// [`probe_catalogue`] with an explicit `PATH` — the testable core.
pub(crate) fn probe_catalogue_on(binary: &str, path: &str) -> crate::harness_catalogue::Catalogue {
    match run_probe(binary, &["--help"], path) {
        Some(help) => crate::harness_catalogue::parse_help(&help),
        None => crate::harness_catalogue::Catalogue::default(),
    }
}

/// Run `<binary> <args>` with `PATH=path`, capturing stdout+stderr (a CLI may print
/// its help to either), bounded by [`CATALOGUE_PROBE_TIMEOUT`]. Returns `None` when
/// the binary can't be spawned, times out, or exits without output. Pure w.r.t. the
/// daemon's environment — the `PATH` is injected, so this is unit-testable against a
/// fake binary.
///
/// The timeout is a poll-and-kill loop rather than a blocking `output()` so a
/// wedged child (blocked on stdin, a broken install) is reaped instead of hanging
/// the caller's `spawn_blocking` worker forever. `--help`/`--version` output fits a
/// pipe buffer, so reading after exit never blocks.
fn run_probe(binary: &str, args: &[&str], path: &str) -> Option<String> {
    if binary.is_empty() {
        return None;
    }
    let mut child = std::process::Command::new(binary)
        .args(args)
        .env("PATH", path)
        // A probe must never inherit an interactive stdin; if a binary asks, it
        // gets EOF and exits rather than blocking.
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .ok()?;

    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if start.elapsed() > CATALOGUE_PROBE_TIMEOUT {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(_) => return None,
        }
    }
    let output = child.wait_with_output().ok()?;
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    if !output.stderr.is_empty() {
        text.push('\n');
        text.push_str(&String::from_utf8_lossy(&output.stderr));
    }
    (!text.trim().is_empty()).then_some(text)
}

/// Resolve the model a work node launches with: the node's own `model:`
/// override wins, else the instance `default_effective` (#296/#347). An empty
/// string on *either* side collapses to the next tier — "" means "unset"
/// everywhere, so a hand-authored `model: ""` in YAML falls through to the
/// default instead of reaching the tail as an empty `--model`.
///
/// This is the single precedence point both spawn seams (`spawn_node` and
/// `start_node`) call, so the `node → instance` merge is defined and tested in
/// exactly one place.
pub fn resolve_node_model<'a>(
    node_model: Option<&'a str>,
    default_effective: Option<&'a str>,
) -> Option<&'a str> {
    node_model
        .filter(|s| !s.is_empty())
        .or(default_effective.filter(|s| !s.is_empty()))
}

/// Resolve the effort a work node launches with (#424): the node's own `effort:`,
/// with an empty string collapsing to `None` — "" means "unset" everywhere, so a
/// hand-authored `effort: ""` in YAML falls through instead of reaching the tail
/// as an empty `--effort`.
///
/// One tier today, on purpose: there is **no** instance-wide `default_effort`
/// (that is slice B of #424). This is the seam that gains the second tier when it
/// lands — keep it as the single precedence point both spawn seams call, exactly
/// like [`resolve_node_model`].
pub fn resolve_node_effort(node_effort: Option<&str>) -> Option<&str> {
    node_effort.filter(|s| !s.is_empty())
}

/// Read the reaper interval from the env or use the default.
pub fn reaper_interval() -> Duration {
    std::env::var(REAPER_INTERVAL_SECS_ENV)
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_REAPER_INTERVAL)
}

// The orphan sweep runs **at daemon boot AND on the periodic reaper loop**
// (`DEFAULT_REAPER_INTERVAL`). That distinction is load-bearing (#485,
// ADR-0038): only the periodic pass runs against live spawns, so only it can
// race one. The boot pass cannot — it runs before the router is built, so no
// request and no scheduler tick is concurrent with it. This module's
// doc-comment used to say "at daemon boot" and nothing else, and that single
// omission is what made the race invisible to inspection for the reaper's whole
// life: the one dangerous caller was documented as not existing.

/// One live `pdo-*` session, plus everything the reaper knows about its owner.
///
/// **The inventory is an input, never something the sweep fetches for itself
/// (#485, ADR-0038).** The pre-#485 `sweep_orphans` called
/// [`list_pdo_sessions`] from its own body, so the *order* of its two
/// observations — the tmux inventory and the event-log read — could not be
/// expressed, let alone guaranteed, by the caller. It read the log first, so a
/// session born between the two was live in tmux and missing from a snapshot
/// that predated it: judged absent, killed ~150 ms after its own spawn. Keying
/// the input *per session* makes the correct order the path of least
/// resistance — `info` cannot be filled without already holding the names.
#[derive(Debug, Clone)]
pub struct SweepInput {
    pub session_name: String,
    /// `None` = [`parse_session_name`] refused the name (unconditional-kill arm).
    pub parsed: Option<ParsedSession>,
    /// Its owner's facts, resolved **after** the inventory. `None` = absent from
    /// the event log.
    pub info: Option<NodeRunInfo>,
}

/// What the sweep decided about one live session. One per input, `Keep`
/// included, so a test can assert the *absence* of a kill as strongly as its
/// presence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SweepDecision {
    pub session_name: String,
    pub verdict: SweepVerdict,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SweepVerdict {
    Keep,
    Kill(KillReason),
}

/// One variant per log line the sweep emitted before #485.
///
/// Flat on purpose: two of the messages are irregular and a composed
/// `{kind} × {cause}` formatter would smooth them over silently. The NodeRun
/// arms say `killing session for absent run …` with **no** "node" word (unlike
/// manager/shell), and the stale arm keys off the session name rather than
/// `run_id`/`node_id`. `journalctl | grep "Orphan sweep: killing session for
/// absent run"` is how #485 was diagnosed in the first place, so these strings
/// are a contract — `kill_reason_messages_are_verbatim` pins all eight.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KillReason {
    UnrecognisedName {
        session_name: String,
    },
    ManagerRunAbsent {
        run_id: String,
    },
    ManagerRunArchived {
        run_id: String,
    },
    ShellRunAbsent {
        run_id: String,
    },
    ShellRunArchived {
        run_id: String,
    },
    NodeRunAbsent {
        run_id: String,
        node_id: String,
    },
    NodeRunArchived {
        run_id: String,
        node_id: String,
    },
    /// The **only** TTL arm. Neither Manager (#458) nor Shell (#316) has one —
    /// do not "unify" them.
    NodeRunStale {
        session_name: String,
        age_secs: i64,
    },
}

impl std::fmt::Display for KillReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KillReason::UnrecognisedName { session_name } => {
                write!(
                    f,
                    "Orphan sweep: killing unrecognised session {session_name}"
                )
            }
            KillReason::ManagerRunAbsent { run_id } => {
                write!(
                    f,
                    "Orphan sweep: killing manager session for absent run {run_id}"
                )
            }
            KillReason::ManagerRunArchived { run_id } => {
                write!(
                    f,
                    "Orphan sweep: killing manager session for archived run {run_id}"
                )
            }
            KillReason::ShellRunAbsent { run_id } => {
                write!(
                    f,
                    "Orphan sweep: killing shell session for absent run {run_id}"
                )
            }
            KillReason::ShellRunArchived { run_id } => {
                write!(
                    f,
                    "Orphan sweep: killing shell session for archived run {run_id}"
                )
            }
            KillReason::NodeRunAbsent { run_id, node_id } => {
                write!(
                    f,
                    "Orphan sweep: killing session for absent run {run_id}/{node_id}"
                )
            }
            KillReason::NodeRunArchived { run_id, node_id } => {
                write!(
                    f,
                    "Orphan sweep: killing session for archived run {run_id}/{node_id}"
                )
            }
            KillReason::NodeRunStale {
                session_name,
                age_secs,
            } => write!(
                f,
                "Orphan sweep: killing stale session {session_name} (completed {age_secs}s ago)"
            ),
        }
    }
}

impl KillReason {
    /// Whether this kill is an *absence* verdict (including an unparseable
    /// name) rather than routine housekeeping.
    ///
    /// After #485 an absence verdict on a **live** session is a "can no longer
    /// happen": it means either a genuinely leaked session or a reservation that
    /// never landed. Housekeeping (archived run, TTL-expired node) is nominal
    /// and happens every 60 s. The split drives both the log level in
    /// [`apply_sweep`] and the `killed_for_absent_run` gauge on `GET /sessions`.
    pub fn is_absence(&self) -> bool {
        matches!(
            self,
            KillReason::UnrecognisedName { .. }
                | KillReason::ManagerRunAbsent { .. }
                | KillReason::ShellRunAbsent { .. }
                | KillReason::NodeRunAbsent { .. }
        )
    }
}

/// What one sweep pass actually killed, for the `reaper` gauge on
/// `GET /sessions` (#485). Strictly the **pass's own** tally — this type stays
/// per-pass and stateless; the caller is what accumulates it into the two
/// since-boot counters it publishes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SweepTally {
    pub killed: i64,
    pub killed_for_absent_run: i64,
}

/// Decide, for every live session, whether it is an orphan — **pure**.
///
/// ADR-0009 layer 1: no tmux, no DB, no clock. `now` and `ttl` are injected so
/// the TTL arm is deterministic in a test, and every input yields a decision
/// (`Keep` included) so a test can pin "this session survives" as hard as
/// "this one dies".
///
/// An orphan is a `pdo-*` session whose owner is:
/// - archived,
/// - absent from the event log,
/// - or a NodeRun that completed more than `ttl` ago (NodeRun only — #316/#458).
///
/// **Correctness precondition, and this function cannot enforce it (#485,
/// ADR-0038):** `info` must have been resolved from a log read that happened
/// *after* the inventory that produced `session_name`. The log only grows, so an
/// absence observed after the inventory implies absence *at* the inventory —
/// which is what makes the `Absent` verdicts sound. The reverse order has no
/// symmetric proof.
pub fn decide_sweep(
    inputs: &[SweepInput],
    ttl: Duration,
    now: chrono::DateTime<chrono::Utc>,
) -> Vec<SweepDecision> {
    inputs
        .iter()
        .map(|input| SweepDecision {
            session_name: input.session_name.clone(),
            verdict: decide_one(input, ttl, now),
        })
        .collect()
}

fn decide_one(
    input: &SweepInput,
    ttl: Duration,
    now: chrono::DateTime<chrono::Utc>,
) -> SweepVerdict {
    let parsed = match &input.parsed {
        Some(p) => p,
        None => {
            return SweepVerdict::Kill(KillReason::UnrecognisedName {
                session_name: input.session_name.clone(),
            })
        }
    };

    match parsed {
        ParsedSession::Manager { run_id } => match &input.info {
            // Kill manager sessions for absent/archived runs
            None => SweepVerdict::Kill(KillReason::ManagerRunAbsent {
                run_id: run_id.clone(),
            }),
            Some(info) if info.is_archived => SweepVerdict::Kill(KillReason::ManagerRunArchived {
                run_id: run_id.clone(),
            }),
            // No TTL arm (#458): a manager outlives its run's completion.
            _ => SweepVerdict::Keep,
        },
        ParsedSession::Shell { run_id } => match &input.info {
            // #316: mirror the Manager arm — reap iff the run is absent or
            // archived, NEVER on a TTL (an interactive shell must not be
            // yanked from a user who stepped away). The `__shell__` lookup
            // branch supplies the run's archived flag.
            None => SweepVerdict::Kill(KillReason::ShellRunAbsent {
                run_id: run_id.clone(),
            }),
            Some(info) if info.is_archived => SweepVerdict::Kill(KillReason::ShellRunArchived {
                run_id: run_id.clone(),
            }),
            _ => SweepVerdict::Keep,
        },
        // #302 / ADR-0048: a library authoring assistant has no owning Run, so
        // there is nothing to key an absence/archived verdict on. It is reaped
        // **explicitly** on tab-leave (`DELETE /sessions/<id>/libassist`), never by
        // the sweep, and has no TTL (an interactive REPL must not be yanked from a
        // user who stepped away — same reasoning as the shell/manager arms). Always
        // keep: a reopen re-attaches this same session, a leave kills it. `info` is
        // always `None` for this variant (the sweep caller does no run lookup).
        ParsedSession::LibAssist { .. } => SweepVerdict::Keep,
        ParsedSession::NodeRun {
            run_id,
            node_id,
            iter: _,
        } => match &input.info {
            None => SweepVerdict::Kill(KillReason::NodeRunAbsent {
                run_id: run_id.clone(),
                node_id: node_id.clone(),
            }),
            Some(info) if info.is_archived => SweepVerdict::Kill(KillReason::NodeRunArchived {
                run_id: run_id.clone(),
                node_id: node_id.clone(),
            }),
            Some(NodeRunInfo {
                completed_at: Some(completed),
                ..
            }) => {
                let age = now.signed_duration_since(*completed);
                if age > chrono::Duration::from_std(ttl).unwrap_or(chrono::Duration::hours(1)) {
                    SweepVerdict::Kill(KillReason::NodeRunStale {
                        session_name: input.session_name.clone(),
                        age_secs: age.num_seconds(),
                    })
                } else {
                    SweepVerdict::Keep
                }
            }
            _ => SweepVerdict::Keep, // still running or not yet completed
        },
    }
}

/// Execute the decisions: log the reason verbatim, then `tmux kill-session`.
/// The edge half of the sweep (ADR-0009 layer 1 boundary).
///
/// Scans only the daemon's private socket — never the system-default socket —
/// so we can never reach into another daemon's tmux state.
///
/// Log levels split routine housekeeping from "can no longer happen" (#485):
/// *archived* / *stale* stay `info!` (nominal, every 60 s — promoting them
/// would train the operator to ignore a permanent warning stream), while
/// *absent* / *unrecognised* are `warn!` so `journalctl -p warning` surfaces
/// them without a full-log grep. Precedent: `boot_recovery` warns for the same
/// act (the daemon finding an orphaned node).
pub fn apply_sweep(socket: &str, decisions: &[SweepDecision]) -> SweepTally {
    let mut tally = SweepTally::default();

    for decision in decisions {
        let reason = match &decision.verdict {
            SweepVerdict::Keep => continue,
            SweepVerdict::Kill(reason) => reason,
        };
        if reason.is_absence() {
            warn!("{reason}");
            tally.killed_for_absent_run += 1;
        } else {
            info!("{reason}");
        }
        tally.killed += 1;
        kill(socket, &decision.session_name);
    }

    tally
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
    fn parse_libassist_session() {
        // #302: `pdo-libassist-<pipeline_id>` parses to a LibAssist variant. The
        // pipeline id is a file-stem slug (no `-iter-` suffix, may contain dashes).
        let name = "pdo-libassist-feature-with-review";
        let parsed = parse_session_name(name).unwrap();
        assert_eq!(
            parsed,
            ParsedSession::LibAssist {
                pipeline_id: "feature-with-review".into(),
            }
        );
    }

    #[test]
    fn parse_garbage_returns_none() {
        assert!(parse_session_name("foo-bar").is_none());
        assert!(parse_session_name("pdo-").is_none());
        assert!(parse_session_name("pdo-mgr-").is_none());
        assert!(parse_session_name("pdo-shell-").is_none());
        assert!(parse_session_name("pdo-libassist-").is_none());
    }

    /// Formatters and parser pinned as a **round trip**, not independently
    /// against literals (which is all `parse_*_session` / the naming tests do).
    /// Without this, a brand-new session kind whose formatter lands with no
    /// matching parser branch is silently reaped within 60 s, leaving one `info!`
    /// line as the only trace. This turns that into a red test.
    #[test]
    fn session_name_formatters_round_trip_through_the_parser() {
        let run_id = "20260731-153057-553fcb3";

        assert_eq!(
            parse_session_name(&node_session_name(run_id, "impl-worker", 7)),
            Some(ParsedSession::NodeRun {
                run_id: run_id.into(),
                node_id: "impl-worker".into(),
                iter: 7,
            })
        );
        assert_eq!(
            parse_session_name(&manager_session_name(run_id)),
            Some(ParsedSession::Manager {
                run_id: run_id.into()
            })
        );
        assert_eq!(
            parse_session_name(&shell_session_name(run_id)),
            Some(ParsedSession::Shell {
                run_id: run_id.into()
            })
        );
        // #302: the libassist formatter must round-trip too — else the sweep reaps
        // it as unrecognised within 60 s (the exact failure this test exists for).
        assert_eq!(
            parse_session_name(&libassist_session_name("feature-with-review")),
            Some(ParsedSession::LibAssist {
                pipeline_id: "feature-with-review".into()
            })
        );
    }

    // -----------------------------------------------------------------------
    // Orphan sweep — `decide_sweep` (#485, ADR-0038)
    //
    // Layer 1 (ADR-0009): no tmux, no DB, no clock. `sweep_orphans` had zero
    // unit tests before #485 precisely because it fetched the inventory and the
    // clock itself; making both inputs is what puts the sweep's decision rule
    // under CI on every machine, with no `tmux_available()` skip to hide behind.
    // -----------------------------------------------------------------------

    const RID: &str = "20260731-153057-553fcb3";

    fn at(iso: &str) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::parse_from_rfc3339(iso)
            .unwrap()
            .with_timezone(&chrono::Utc)
    }

    /// One input, already resolved: the session name plus its owner's facts.
    fn input(session_name: &str, info: Option<NodeRunInfo>) -> SweepInput {
        SweepInput {
            session_name: session_name.to_string(),
            parsed: parse_session_name(session_name),
            info,
        }
    }

    fn live() -> Option<NodeRunInfo> {
        Some(NodeRunInfo {
            completed_at: None,
            is_archived: false,
        })
    }

    fn verdict(inputs: &[SweepInput], now: chrono::DateTime<chrono::Utc>) -> SweepVerdict {
        let decisions = decide_sweep(inputs, Duration::from_secs(3600), now);
        assert_eq!(decisions.len(), inputs.len(), "one decision per input");
        decisions[0].verdict.clone()
    }

    /// **The test #485 asks for.** A node whose reservation IS visible in the log
    /// survives — reproduced with no dependency on timing at all, because the
    /// snapshot is a parameter.
    #[test]
    fn young_session_of_a_live_node_survives() {
        let name = node_session_name(RID, "alpha", 1);
        assert_eq!(
            verdict(&[input(&name, live())], at("2026-07-31T15:32:19Z")),
            SweepVerdict::Keep
        );
    }

    /// **The twin, and it is non-negotiable.** Without it, a "fix" that simply
    /// neutralises the sweep passes the test above and lets sessions pile up
    /// toward the ~30-session tmux collapse point (#77/#78).
    #[test]
    fn genuinely_absent_node_is_still_killed() {
        let name = node_session_name(RID, "alpha", 1);
        assert_eq!(
            verdict(&[input(&name, None)], at("2026-07-31T15:32:19Z")),
            SweepVerdict::Kill(KillReason::NodeRunAbsent {
                run_id: RID.into(),
                node_id: "alpha".into(),
            })
        );
    }

    #[test]
    fn archived_run_is_killed_on_every_arm() {
        let archived = || {
            Some(NodeRunInfo {
                completed_at: None,
                is_archived: true,
            })
        };
        let now = at("2026-07-31T15:32:19Z");

        let node = node_session_name(RID, "alpha", 1);
        assert_eq!(
            verdict(&[input(&node, archived())], now),
            SweepVerdict::Kill(KillReason::NodeRunArchived {
                run_id: RID.into(),
                node_id: "alpha".into(),
            })
        );
        assert_eq!(
            verdict(&[input(&manager_session_name(RID), archived())], now),
            SweepVerdict::Kill(KillReason::ManagerRunArchived { run_id: RID.into() })
        );
        assert_eq!(
            verdict(&[input(&shell_session_name(RID), archived())], now),
            SweepVerdict::Kill(KillReason::ShellRunArchived { run_id: RID.into() })
        );
    }

    #[test]
    fn absent_run_is_killed_on_manager_and_shell_arms_too() {
        let now = at("2026-07-31T15:32:19Z");
        assert_eq!(
            verdict(&[input(&manager_session_name(RID), None)], now),
            SweepVerdict::Kill(KillReason::ManagerRunAbsent { run_id: RID.into() })
        );
        assert_eq!(
            verdict(&[input(&shell_session_name(RID), None)], now),
            SweepVerdict::Kill(KillReason::ShellRunAbsent { run_id: RID.into() })
        );
    }

    /// #302 / ADR-0048: a library assistant is **always kept** by the sweep,
    /// whatever `info` says. It owns no Run, so there is no absence/archived/TTL
    /// verdict that could apply — it is reaped only by the explicit
    /// `DELETE /sessions/<id>/libassist` on tab-leave. Pinned as hard as a kill so
    /// a future refactor cannot silently start reaping it (the exact bug the
    /// `libassist-` parse branch exists to prevent).
    #[test]
    fn library_assistant_is_always_kept() {
        let now = at("2026-07-31T15:32:19Z");
        let name = libassist_session_name("feature-with-review");
        // No owner (info = None) — the shell/manager arms would kill on this.
        assert_eq!(verdict(&[input(&name, None)], now), SweepVerdict::Keep);
        // Even a (nonsensical) archived owner does not flip it.
        assert_eq!(
            verdict(
                &[input(
                    &name,
                    Some(NodeRunInfo {
                        completed_at: Some(at("2000-01-01T00:00:00Z")),
                        is_archived: true,
                    })
                )],
                now
            ),
            SweepVerdict::Keep
        );
    }

    /// An unparseable name is killed without consulting `info` at all — the same
    /// unconditional arm as before #485. A private socket makes this the right
    /// default (#86); the round-trip test above is what guards against our *own*
    /// formatters drifting into it.
    #[test]
    fn unrecognised_name_is_killed_without_a_lookup() {
        let decisions = decide_sweep(
            &[SweepInput {
                session_name: "pdo-ceci-nest-pas-un-nom".into(),
                parsed: None,
                info: live(),
            }],
            Duration::from_secs(3600),
            at("2026-07-31T15:32:19Z"),
        );
        assert_eq!(
            decisions[0].verdict,
            SweepVerdict::Kill(KillReason::UnrecognisedName {
                session_name: "pdo-ceci-nest-pas-un-nom".into(),
            })
        );
    }

    /// The TTL comparison is `>`, strictly — `age == ttl` keeps. Deterministic
    /// only because `now` is injected.
    #[test]
    fn ttl_boundary_is_strict() {
        let name = node_session_name(RID, "alpha", 1);
        let completed = at("2026-07-31T12:00:00Z");
        let done = |completed| {
            Some(NodeRunInfo {
                completed_at: Some(completed),
                is_archived: false,
            })
        };

        // age == ttl (3600 s) → Keep.
        assert_eq!(
            verdict(&[input(&name, done(completed))], at("2026-07-31T13:00:00Z")),
            SweepVerdict::Keep
        );
        // age == ttl + 1 s → Kill.
        assert_eq!(
            verdict(&[input(&name, done(completed))], at("2026-07-31T13:00:01Z")),
            SweepVerdict::Kill(KillReason::NodeRunStale {
                session_name: name.clone(),
                age_secs: 3601,
            })
        );
    }

    /// An out-of-range `ttl` falls back to one hour, not to "kill everything"
    /// (`chrono::Duration::from_std(..).unwrap_or(hours(1))`).
    #[test]
    fn ttl_out_of_chrono_range_falls_back_to_one_hour() {
        let name = node_session_name(RID, "alpha", 1);
        let info = Some(NodeRunInfo {
            completed_at: Some(at("2026-07-31T12:00:00Z")),
            is_archived: false,
        });
        let absurd = Duration::from_secs(u64::MAX);

        // 30 min old, 1 h fallback → Keep.
        let keep = decide_sweep(
            &[input(&name, info.clone())],
            absurd,
            at("2026-07-31T12:30:00Z"),
        );
        assert_eq!(keep[0].verdict, SweepVerdict::Keep);

        // 2 h old → Kill, so the fallback is 1 h and not "never".
        let kill = decide_sweep(&[input(&name, info)], absurd, at("2026-07-31T14:00:00Z"));
        assert!(matches!(
            kill[0].verdict,
            SweepVerdict::Kill(KillReason::NodeRunStale { .. })
        ));
    }

    /// Manager (#458) and Shell (#316) have **no** TTL arm: a long-completed run
    /// keeps both. Do not "unify" the three arms.
    #[test]
    fn manager_and_shell_have_no_ttl_arm() {
        let long_done = || {
            Some(NodeRunInfo {
                completed_at: Some(at("2020-01-01T00:00:00Z")),
                is_archived: false,
            })
        };
        let now = at("2026-07-31T15:32:19Z");

        assert_eq!(
            verdict(&[input(&manager_session_name(RID), long_done())], now),
            SweepVerdict::Keep
        );
        assert_eq!(
            verdict(&[input(&shell_session_name(RID), long_done())], now),
            SweepVerdict::Keep
        );
    }

    /// The eight kill messages, byte for byte. The whole #485 investigation was a
    /// `journalctl | grep "Orphan sweep: killing session for absent run"`, so
    /// these strings are a contract, not cosmetics. Note the two irregularities a
    /// composed formatter would smooth over: the NodeRun arms have no "node" word,
    /// and the stale arm keys off the session name.
    #[test]
    fn kill_reason_messages_are_verbatim() {
        assert_eq!(
            KillReason::UnrecognisedName {
                session_name: "pdo-weird".into()
            }
            .to_string(),
            "Orphan sweep: killing unrecognised session pdo-weird"
        );
        assert_eq!(
            KillReason::ManagerRunAbsent { run_id: RID.into() }.to_string(),
            "Orphan sweep: killing manager session for absent run 20260731-153057-553fcb3"
        );
        assert_eq!(
            KillReason::ManagerRunArchived { run_id: RID.into() }.to_string(),
            "Orphan sweep: killing manager session for archived run 20260731-153057-553fcb3"
        );
        assert_eq!(
            KillReason::ShellRunAbsent { run_id: RID.into() }.to_string(),
            "Orphan sweep: killing shell session for absent run 20260731-153057-553fcb3"
        );
        assert_eq!(
            KillReason::ShellRunArchived { run_id: RID.into() }.to_string(),
            "Orphan sweep: killing shell session for archived run 20260731-153057-553fcb3"
        );
        assert_eq!(
            KillReason::NodeRunAbsent {
                run_id: RID.into(),
                node_id: "alpha".into()
            }
            .to_string(),
            "Orphan sweep: killing session for absent run 20260731-153057-553fcb3/alpha"
        );
        assert_eq!(
            KillReason::NodeRunArchived {
                run_id: RID.into(),
                node_id: "alpha".into()
            }
            .to_string(),
            "Orphan sweep: killing session for archived run 20260731-153057-553fcb3/alpha"
        );
        assert_eq!(
            KillReason::NodeRunStale {
                session_name: "pdo-20260731-153057-553fcb3-alpha-iter-1".into(),
                age_secs: 7200,
            }
            .to_string(),
            "Orphan sweep: killing stale session pdo-20260731-153057-553fcb3-alpha-iter-1 \
             (completed 7200s ago)"
        );
    }

    /// The level split feeding both `apply_sweep`'s `warn!`/`info!` choice and the
    /// `killed_for_absent_run` gauge: absence (incl. an unparseable name) is the
    /// "can no longer happen" class; archived/stale is nominal housekeeping.
    #[test]
    fn only_absence_verdicts_are_flagged_as_abnormal() {
        for reason in [
            KillReason::UnrecognisedName {
                session_name: "x".into(),
            },
            KillReason::ManagerRunAbsent { run_id: RID.into() },
            KillReason::ShellRunAbsent { run_id: RID.into() },
            KillReason::NodeRunAbsent {
                run_id: RID.into(),
                node_id: "alpha".into(),
            },
        ] {
            assert!(reason.is_absence(), "{reason} should be an absence verdict");
        }
        for reason in [
            KillReason::ManagerRunArchived { run_id: RID.into() },
            KillReason::ShellRunArchived { run_id: RID.into() },
            KillReason::NodeRunArchived {
                run_id: RID.into(),
                node_id: "alpha".into(),
            },
            KillReason::NodeRunStale {
                session_name: "x".into(),
                age_secs: 1,
            },
        ] {
            assert!(!reason.is_absence(), "{reason} is routine housekeeping");
        }
    }

    /// `decide_sweep` yields one decision per input, in input order, mixing
    /// verdicts — the property that lets a caller tally kills and a test assert an
    /// absence of kills.
    #[test]
    fn decide_sweep_returns_one_decision_per_input_in_order() {
        let live_node = node_session_name(RID, "alpha", 1);
        let dead_node = node_session_name(RID, "beta", 1);
        let decisions = decide_sweep(
            &[
                input(&live_node, live()),
                input(&dead_node, None),
                input(&manager_session_name(RID), live()),
            ],
            Duration::from_secs(3600),
            at("2026-07-31T15:32:19Z"),
        );

        assert_eq!(
            decisions
                .iter()
                .map(|d| d.session_name.as_str())
                .collect::<Vec<_>>(),
            vec![
                live_node.as_str(),
                dead_node.as_str(),
                &manager_session_name(RID)
            ]
        );
        assert_eq!(decisions[0].verdict, SweepVerdict::Keep);
        assert!(matches!(decisions[1].verdict, SweepVerdict::Kill(_)));
        assert_eq!(decisions[2].verdict, SweepVerdict::Keep);
    }

    /// #550/AC #10: the fail-fast PATH probe. A bare name absent from the probe
    /// `PATH` is unavailable; an empty name is never available; a slash-path is
    /// checked as a file directly. `sh` is on every CI box (and the process `PATH`
    /// is unioned into the probe path, ADR-0055), so it stands in for "installed".
    #[test]
    fn binary_available_probes_path_without_executing() {
        assert!(!binary_available(""), "empty name is never available");
        assert!(
            !binary_available("pdo-definitely-not-a-real-binary-xyz"),
            "a name absent from PATH is unavailable"
        );
        assert!(
            binary_available("sh"),
            "a ubiquitous binary resolves on PATH"
        );
        assert!(
            !binary_available("/no/such/absolute/path/binary"),
            "a missing slash-path is unavailable"
        );
    }

    /// #616: write an executable fake binary into `dir` that echoes `stdout` on
    /// `--help` and `version` on `--version`. Returns the dir's `PATH` string.
    #[cfg(unix)]
    fn fake_harness_binary(dir: &std::path::Path, name: &str, help: &str, version: &str) -> String {
        use std::os::unix::fs::PermissionsExt;
        // Self-contained: `printf` is a `/bin/sh` builtin, so the script runs even
        // though `run_probe` sets `PATH` to just this dir (no `cat`/`echo` binary).
        // That restricted PATH mirrors nothing in production — there the probe PATH
        // is a full union (ADR-0055) — it only keeps the fixture honest.
        let script = format!(
            "#!/bin/sh\ncase \"$1\" in\n  --version) printf '%s\\n' '{version}';;\n  --help) printf '%s' '{help}';;\nesac\n"
        );
        let bin = dir.join(name);
        std::fs::write(&bin, script).unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        dir.to_string_lossy().into_owned()
    }

    /// #616, ADR-0053 §1/§3: running the resolved binary reads its version and parses
    /// its offered catalogue. Executed against a fake binary on an injected `PATH`,
    /// so the test is deterministic and touches no real harness.
    #[cfg(unix)]
    #[test]
    fn probe_reads_version_and_catalogue_from_the_binary() {
        let dir = tempfile::tempdir().unwrap();
        let help =
            "  --model <m>  [gpt-5|gpt-5-codex|o4-mini]\n  --effort <e>  One of: low, medium, high";
        let path = fake_harness_binary(dir.path(), "fake-harness", help, "fake-harness 1.402");

        assert_eq!(
            probe_version_on("fake-harness", &path).as_deref(),
            Some("fake-harness 1.402")
        );
        let cat = probe_catalogue_on("fake-harness", &path);
        assert_eq!(cat.models, vec!["gpt-5", "gpt-5-codex", "o4-mini"]);
        assert_eq!(cat.efforts, vec!["low", "medium", "high"]);
    }

    /// #616: a binary that can't be resolved on the injected `PATH` yields no
    /// version and an empty catalogue — the free-text fallback, never a panic.
    #[test]
    fn probe_of_an_absent_binary_is_empty_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_string_lossy().into_owned();
        assert_eq!(probe_version_on("no-such-harness", &path), None);
        assert_eq!(
            probe_catalogue_on("no-such-harness", &path),
            crate::harness_catalogue::Catalogue::default()
        );
    }

    /// ADR-0055: the pure core of the probe `PATH` search — a directory that holds
    /// the binary makes it resolvable; one that does not, does not.
    #[test]
    fn path_contains_binary_searches_each_entry() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("my-harness");
        std::fs::write(&bin, b"#!/bin/sh\n").unwrap();
        let path = format!("/nonexistent-a:{}:/nonexistent-b", dir.path().display());
        assert!(path_contains_binary(&path, "my-harness"));
        assert!(!path_contains_binary(&path, "absent-harness"));
        assert!(
            !path_contains_binary("/nonexistent-a:/nonexistent-b", "my-harness"),
            "no entry holds it"
        );
    }

    /// ADR-0055: the user's interactive `PATH` is unioned with the process one,
    /// first wins, duplicates and empty entries dropped, order preserved — so the
    /// package-manager prefix the service never inherited becomes searchable
    /// without losing any entry the service already had.
    #[test]
    fn union_paths_merges_first_precedence_dedup_order_preserved() {
        assert_eq!(
            union_paths("/opt/homebrew/bin:/usr/bin", "/usr/bin:/bin"),
            "/opt/homebrew/bin:/usr/bin:/bin"
        );
        // Empty operands collapse cleanly.
        assert_eq!(union_paths("", "/usr/bin:/bin"), "/usr/bin:/bin");
        assert_eq!(union_paths("/usr/bin", ""), "/usr/bin");
        assert_eq!(union_paths("", ""), "");
        // A stray empty entry ("::") never becomes a "search the cwd" hole.
        assert_eq!(union_paths("/a::/b", "/b"), "/a:/b");
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
            SessionTail::Agent {
                harness: &crate::harness_registry::claude(),
                model: None,
                effort: None,
                session_id: None,
            },
            None,
            None,
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
            SessionTail::Agent {
                harness: &crate::harness_registry::claude(),
                model: None,
                effort: None,
                session_id: None,
            },
            None,
            None,
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
            SessionTail::Agent {
                harness: &crate::harness_registry::claude(),
                model: None,
                effort: None,
                session_id: None,
            },
            None,
            None,
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
                harness: &crate::harness_registry::claude(),
                model: Some("opus"),
                effort: None,
                session_id: None,
            },
            None,
            None,
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
    fn build_script_omits_model_when_empty() {
        // #347: an empty-string model must behave exactly like `None` — no
        // `--model` flag, byte-identical to the legacy launch. This is the
        // last-resort crash guard: `Some("")` would otherwise emit `--model ''`
        // and make `claude` exit non-zero at launch.
        let prompt_path = Path::new("/tmp/test-prompt.md");
        let script = build_tmux_script(
            "run-abc",
            "solo",
            1,
            5172,
            prompt_path,
            None,
            SessionTail::Agent {
                harness: &crate::harness_registry::claude(),
                model: Some(""),
                effort: None,
                session_id: None,
            },
            None,
            None,
        );
        assert!(
            !script.contains("--model"),
            "empty model must emit no flag (identical to None): {script}"
        );
        assert!(
            script.contains("exec claude --dangerously-skip-permissions \"$(cat "),
            "legacy tail must be byte-identical for an empty model: {script}"
        );
    }

    /// #424 helper: build an agent tail with an arbitrary model/effort pair. No
    /// pinned session id (#473) — the model/effort byte-identity tests below assert
    /// on the legacy tail, so this helper keeps `session_id: None`.
    fn agent_script(model: Option<&str>, effort: Option<&str>) -> String {
        build_tmux_script(
            "run-abc",
            "solo",
            1,
            5172,
            Path::new("/tmp/test-prompt.md"),
            None,
            SessionTail::Agent {
                harness: &crate::harness_registry::claude(),
                model,
                effort,
                session_id: None,
            },
            None,
            None,
        )
    }

    #[test]
    fn build_script_omits_effort_when_none() {
        // #424: THE byte-identity gate. A node with no effort must reproduce the
        // legacy command byte-for-byte — no `--effort`, exactly one space before
        // `"$(cat …)"`. Adding the flag must not perturb the default launch.
        let script = agent_script(None, None);
        assert!(
            !script.contains("--effort"),
            "no effort flag when unset: {script}"
        );
        assert!(
            script.contains("exec claude --dangerously-skip-permissions \"$(cat "),
            "legacy tail must be byte-identical: {script}"
        );
    }

    #[test]
    fn build_script_omits_effort_when_empty() {
        // #424: `Some("")` must behave exactly like `None`. Last-resort guard,
        // mirroring the model's (#347): every upstream tier already collapses ""
        // (`resolve_node_effort`), but a missed source would otherwise emit
        // `--effort ''`, which `claude` answers with a stderr warning and a
        // *silent* fall back to the default level — the worst failure mode.
        let script = agent_script(None, Some(""));
        assert!(
            !script.contains("--effort"),
            "empty effort must emit no flag (identical to None): {script}"
        );
        assert!(
            script.contains("exec claude --dangerously-skip-permissions \"$(cat "),
            "legacy tail must be byte-identical for an empty effort: {script}"
        );
    }

    #[test]
    fn build_script_inserts_effort_after_model() {
        // #424: `--effort` sits AFTER `--model` — the model test above pins the
        // substring `--dangerously-skip-permissions --model '<m>' `, so inserting
        // before it would break that assertion for nothing. Single-quoted like the
        // model value, hence `'\''` once `wrap_with_env` re-wraps the tail in
        // `bash -c '…'`.
        let script = agent_script(Some("opus"), Some("low"));
        assert!(
            script.contains(r"--model '\''opus'\'' --effort '\''low'\'' "),
            "effort must follow the model, both single-quoted: {script}"
        );
        let model_at = script.find("--model").unwrap();
        let effort_at = script.find("--effort").unwrap();
        let cat_at = script.find("$(cat").unwrap();
        assert!(
            model_at < effort_at && effort_at < cat_at,
            "order must be --model, --effort, then the prompt cat: {script}"
        );
    }

    #[test]
    fn build_script_effort_without_model_still_hugs_the_base_flag() {
        // #424: the two fragments are independent — an effort-only node emits the
        // effort flag directly after `--dangerously-skip-permissions`, with no
        // double space where the absent model fragment would have been.
        let script = agent_script(None, Some("xhigh"));
        assert!(
            script.contains(r#"--dangerously-skip-permissions --effort '\''xhigh'\'' "$(cat "#),
            "effort-only tail must leave no gap: {script}"
        );
        assert!(!script.contains("--model"), "{script}");
    }

    /// #433 helper: build an agent tail with a `Some` settings path, no
    /// model/effort, no sandbox.
    fn agent_script_with_settings(settings: &Path) -> String {
        build_tmux_script(
            "run-abc",
            "solo",
            1,
            5172,
            Path::new("/tmp/test-prompt.md"),
            None,
            SessionTail::Agent {
                harness: &crate::harness_registry::claude(),
                model: None,
                effort: None,
                session_id: None,
            },
            None,
            Some(settings),
        )
    }

    /// #473 helper: build an agent tail with model/effort/session_id.
    fn agent_script_with_session(
        model: Option<&str>,
        effort: Option<&str>,
        session_id: Option<&str>,
    ) -> String {
        build_tmux_script(
            "run-abc",
            "solo",
            1,
            5172,
            Path::new("/tmp/test-prompt.md"),
            None,
            SessionTail::Agent {
                harness: &crate::harness_registry::claude(),
                model,
                effort,
                session_id,
            },
            None,
            None,
        )
    }

    #[test]
    fn stop_hook_settings_json_is_valid_and_wraps_with_exit_zero() {
        // #433 / ADR-0043: the injected settings must be valid JSON that arms a
        // `Stop` hook whose command swallows any non-zero exit with `; exit 0`
        // (so the recoverable `exit 3` of a missing output can never force-loop
        // the turn or complete prematurely).
        let v: serde_json::Value =
            serde_json::from_str(STOP_HOOK_SETTINGS_JSON).expect("settings JSON must parse");
        let cmd = v["hooks"]["Stop"][0]["hooks"][0]["command"]
            .as_str()
            .expect("the Stop hook must carry a command");
        assert_eq!(cmd, "pdo complete --auto; exit 0");
        assert_eq!(
            v["hooks"]["Stop"][0]["hooks"][0]["type"].as_str(),
            Some("command")
        );
    }

    /// Drive the real `spawn` with a benign tail and report whether it dropped the
    /// turn-end settings file beside the prompt. Kills the ephemeral tmux server on
    /// its own socket afterwards. `port` isolates the socket from sibling tests.
    fn spawn_and_check_settings_file(
        port: u16,
        harness: &crate::harness_registry::HarnessDescriptor,
    ) -> bool {
        let wd = tempfile::tempdir().unwrap();
        let session = node_session_name("run-c8", "n", 1);
        // The tail runs `true` (exits at once); we only assert on the file the write
        // gate controls, which is written before tmux is ever touched.
        let _ = spawn(
            &session,
            "prompt body",
            wd.path(),
            "run-c8",
            "n",
            1,
            port,
            Some("true"),
            SessionTail::Agent {
                harness,
                model: None,
                effort: None,
                session_id: None,
            },
            None,
            true, // inject_hook ON — the setting is enabled
        );
        let settings = wd
            .path()
            .join(".pdo")
            .join("prompts")
            .join("n-iter-1.settings.json");
        let present = settings.is_file();
        // Tear down the ephemeral server (ignore errors — the `true` tail may have
        // already ended the only session, leaving no server to kill).
        let _ = std::process::Command::new("tmux")
            .args(["-L", &tmux_socket_name(port), "kill-server"])
            .output();
        present
    }

    #[test]
    fn spawn_writes_the_settings_file_for_a_harness_with_a_settings_hole() {
        // #613 (correctif 8) control: `claude` HAS a `{settings}` hole, so with the
        // setting on PDO writes the Stop-hook file beside the prompt, as always.
        assert!(
            spawn_and_check_settings_file(58231, &crate::harness_registry::claude()),
            "claude must still get its turn-end settings file"
        );
    }

    #[test]
    fn spawn_writes_no_settings_file_for_a_harness_without_a_settings_hole() {
        // #613 (correctif 8): `opencode` has NO `{settings}` hole, so even with
        // turn-end auto-completion enabled PDO writes it no claude-format settings
        // file — the absence is honoured, not supplied. This was the one place the
        // discipline was broken.
        assert!(
            !spawn_and_check_settings_file(58232, &crate::harness_registry::opencode()),
            "opencode must get no settings file — it has no settings hole"
        );
    }

    #[test]
    fn build_script_omits_settings_when_none() {
        // Off path: no settings ⇒ no `--settings`, byte-identical legacy tail.
        let script = agent_script(None, None);
        assert!(
            !script.contains("--settings"),
            "no settings flag when unset: {script}"
        );
        assert!(
            script.contains("exec claude --dangerously-skip-permissions \"$(cat "),
            "legacy tail must stay byte-identical when the hook is off: {script}"
        );
    }

    #[test]
    fn build_script_inserts_settings_after_effort_before_cat() {
        // #433: `--settings '<file>'` sits AFTER `--model`/`--effort` and before the
        // prompt `cat` — the position that keeps every model/effort substring test
        // green. Single-quoted, hence `'\''` after `wrap_with_env` re-wraps in
        // `bash -c '…'`.
        let script = build_tmux_script(
            "run-abc",
            "solo",
            1,
            5172,
            Path::new("/tmp/test-prompt.md"),
            None,
            SessionTail::Agent {
                harness: &crate::harness_registry::claude(),
                model: Some("opus"),
                effort: Some("low"),
                session_id: None,
            },
            None,
            Some(Path::new("/wd/.pdo/prompts/solo-iter-1.settings.json")),
        );
        assert!(
            script.contains(
                r"--effort '\''low'\'' --settings '\''/wd/.pdo/prompts/solo-iter-1.settings.json'\'' "
            ),
            "settings must follow effort, single-quoted: {script}"
        );
        let effort_at = script.find("--effort").unwrap();
        let settings_at = script.find("--settings").unwrap();
        let cat_at = script.find("$(cat").unwrap();
        assert!(
            effort_at < settings_at && settings_at < cat_at,
            "order must be --effort, --settings, then the prompt cat: {script}"
        );
    }

    #[test]
    fn build_script_omits_session_id_when_none_or_empty() {
        // #473: `None` OR `Some("")` (an infra session — manager / merge resolver)
        // emits no `--session-id`, byte-identical to the legacy tail. This is what
        // keeps the pre-#473 launch bytes for the sessions the sweep never probes.
        for sid in [None, Some("")] {
            let script = agent_script_with_session(None, None, sid);
            assert!(
                !script.contains("--session-id"),
                "no session-id flag when unset ({sid:?}): {script}"
            );
            assert!(
                script.contains("exec claude --dangerously-skip-permissions \"$(cat "),
                "legacy tail must be byte-identical without a pinned id ({sid:?}): {script}"
            );
        }
    }

    #[test]
    fn build_script_inserts_session_id_after_model_and_effort() {
        // #473: a pinned id emits `--session-id '<uuid>'` LAST — after model/effort,
        // right before the prompt cat — so Claude Code names its transcript
        // `<uuid>.jsonl` and the sweep resolves it by identity. Single-quoted, hence
        // `'\''` once `wrap_with_env` re-wraps the tail in `bash -c '…'`.
        let sid = "11111111-2222-3333-4444-555555555555";
        let script = agent_script_with_session(Some("opus"), Some("low"), Some(sid));
        assert!(
            script.contains(&format!(
                r#"--effort '\''low'\'' --session-id '\''{sid}'\'' "$(cat "#
            )),
            "session-id must follow model/effort and precede the prompt cat: {script}"
        );
        let effort_at = script.find("--effort").unwrap();
        let session_at = script.find("--session-id").unwrap();
        let cat_at = script.find("$(cat").unwrap();
        assert!(
            effort_at < session_at && session_at < cat_at,
            "order must be --model, --effort, --session-id, then the prompt cat: {script}"
        );
    }

    #[test]
    fn build_script_settings_hugs_the_base_flag_without_model_or_effort() {
        // The hook can be armed on a node with no model and no effort — the
        // fragments are independent, so `--settings` lands right after the base
        // flag with no double space.
        let script = agent_script_with_settings(Path::new("/wd/s.json"));
        assert!(
            script.contains(
                r#"--dangerously-skip-permissions --settings '\''/wd/s.json'\'' "$(cat "#
            ),
            "settings-only tail must leave no gap: {script}"
        );
        assert!(
            !script.contains("--model") && !script.contains("--effort"),
            "{script}"
        );
    }

    #[test]
    fn build_script_settings_survives_the_docker_exec_wrap() {
        // #433 + #407: a sandboxed agent tail carries `--settings` INSIDE the
        // `docker exec … bash -lc '<tail>'`, at the same path (the container mounts
        // the repo at the identical host path).
        let wrap = SandboxWrap {
            docker_bin: "docker",
            uid: 1000,
            gid: 1000,
            marker: "pdo-run-abc-solo-iter-1",
            workdir: Path::new("/wd"),
        };
        let script = build_tmux_script(
            "run-abc",
            "solo",
            1,
            5172,
            Path::new("/tmp/test-prompt.md"),
            None,
            SessionTail::Agent {
                harness: &crate::harness_registry::claude(),
                model: None,
                effort: None,
                session_id: None,
            },
            Some(&wrap),
            Some(Path::new("/wd/.pdo/prompts/solo-iter-1.settings.json")),
        );
        assert!(
            script.contains("--settings")
                && script.contains("/wd/.pdo/prompts/solo-iter-1.settings.json"),
            "the sandboxed tail must still reference the settings file: {script}"
        );
        assert!(
            script.contains("docker") && script.contains("bash"),
            "{script}"
        );
    }

    #[test]
    fn script_tail_never_receives_settings() {
        // #433 immunity: a `script` node runs bash, never `claude` — even if a
        // settings path is threaded (it never is in production), the Script arm
        // ignores it, so no `--settings` and no `claude` reach the tail.
        let script = build_tmux_script(
            "run-abc",
            "solo",
            1,
            5172,
            Path::new("/tmp/body.sh"),
            None,
            SessionTail::Script {
                timeout_secs: 60,
                env: &[],
            },
            None,
            Some(Path::new("/wd/.pdo/prompts/solo-iter-1.settings.json")),
        );
        assert!(
            !script.contains("--settings"),
            "a script tail must never carry --settings: {script}"
        );
        assert!(!script.contains("claude"), "{script}");
    }

    #[test]
    fn build_resume_script_omits_settings_when_none() {
        // D7 off path: a resume with no hook is byte-identical to the legacy
        // `--continue` tail.
        let script = build_resume_script(
            "r1",
            "solo",
            1,
            6172,
            &crate::harness_registry::claude(),
            None,
            None,
            None,
            None,
            None,
        );
        assert!(!script.contains("--settings"), "{script}");
        assert!(
            script.contains("exec claude --dangerously-skip-permissions --continue"),
            "legacy continue tail must be byte-identical: {script}"
        );
    }

    #[test]
    fn build_resume_script_reinjects_settings_on_resume() {
        // D7: a resumed session must re-carry the `Stop` hook, or it is lost at
        // resurrection. `--settings` follows `--effort` on the `--continue` tail.
        let script = build_resume_script(
            "r1",
            "solo",
            1,
            6172,
            &crate::harness_registry::claude(),
            Some("low"),
            None,
            None,
            None,
            Some(Path::new("/wd/.pdo/prompts/solo-iter-1.settings.json")),
        );
        assert!(
            script.contains(
                r"--continue --effort '\''low'\'' --settings '\''/wd/.pdo/prompts/solo-iter-1.settings.json'\''"
            ),
            "resume tail must re-inject --settings after --effort: {script}"
        );
    }

    #[test]
    fn build_script_session_id_without_model_or_effort_hugs_the_base_flag() {
        // #473: an id-only node (the common case — no model/effort override) emits
        // the session-id flag directly after `--dangerously-skip-permissions`, no
        // gap where the absent model/effort fragments would have been.
        let sid = "11111111-2222-3333-4444-555555555555";
        let script = agent_script_with_session(None, None, Some(sid));
        assert!(
            script.contains(&format!(
                r#"--dangerously-skip-permissions --session-id '\''{sid}'\'' "$(cat "#
            )),
            "id-only tail must leave no gap: {script}"
        );
        assert!(!script.contains("--model"), "{script}");
        assert!(!script.contains("--effort"), "{script}");
    }

    #[test]
    fn resolve_node_effort_collapses_empty() {
        // #424: pure, single-tier — there is no instance-wide `default_effort`
        // (slice B). An empty string means "unset" everywhere, so a hand-authored
        // `effort: ""` falls through to `None` instead of reaching the tail.
        assert_eq!(
            resolve_node_effort(Some("low")),
            Some("low"),
            "a set level passes through"
        );
        assert_eq!(
            resolve_node_effort(Some("turbo")),
            Some("turbo"),
            "an unknown level passes through too — the wire is open (ADR-0001)"
        );
        assert_eq!(
            resolve_node_effort(Some("")),
            None,
            "empty string collapses to unset"
        );
        assert_eq!(resolve_node_effort(None), None, "nothing set → None");
    }

    #[test]
    fn default_model_env_and_stored_precedence() {
        // Single test on purpose: `DEFAULT_MODEL_ENV` is process-global, so a
        // second test mutating it concurrently would flake (mirrors
        // `reaper_ttl_default_and_from_env`).
        std::env::remove_var(DEFAULT_MODEL_ENV);
        assert_eq!(env_default_model(), None);
        assert_eq!(default_model_with(None), None, "no stored, no env → None");

        std::env::set_var(DEFAULT_MODEL_ENV, "sonnet");
        assert_eq!(env_default_model(), Some("sonnet".to_string()));
        // Stored wins over env.
        assert_eq!(
            default_model_with(Some("opus".to_string())),
            Some("opus".to_string())
        );
        // Empty stored → falls through to env.
        assert_eq!(
            default_model_with(Some(String::new())),
            Some("sonnet".to_string())
        );
        // No stored → env applies.
        assert_eq!(default_model_with(None), Some("sonnet".to_string()));

        // An empty env var is treated as unset (a String has no parse that
        // rejects "" for free).
        std::env::set_var(DEFAULT_MODEL_ENV, "");
        assert_eq!(env_default_model(), None);
        assert_eq!(default_model_with(None), None);

        std::env::remove_var(DEFAULT_MODEL_ENV);
    }

    #[test]
    fn resolve_node_model_precedence() {
        // Pure (no env): the node override wins; an empty string on either side
        // collapses to the next tier; both empty/absent → None (account default).
        assert_eq!(
            resolve_node_model(Some("haiku"), Some("opus")),
            Some("haiku"),
            "node override wins over the instance default"
        );
        assert_eq!(
            resolve_node_model(None, Some("opus")),
            Some("opus"),
            "no node override → instance default"
        );
        assert_eq!(
            resolve_node_model(Some(""), Some("opus")),
            Some("opus"),
            "empty node override falls through to the default"
        );
        assert_eq!(
            resolve_node_model(Some("haiku"), None),
            Some("haiku"),
            "node override with no default"
        );
        assert_eq!(resolve_node_model(None, None), None, "nothing set → None");
        assert_eq!(
            resolve_node_model(Some(""), Some("")),
            None,
            "empty on both sides → None (byte-identity floor)"
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
            None,
            None,
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

    /// #490 / ADR-0035 §4 — the tail must NOT double a failure the daemon already
    /// recorded.
    ///
    /// The pre-#490 tail was `pdo complete || pdo fail --reason "…"`. That `||` was
    /// dead code (every refusal answered `200`, so `pdo complete` exited `0`) and
    /// waking it up would have appended a second `NodeFailed` **and** a second
    /// `RunFailed` — the latter unguarded, carrying "after script success" as the
    /// reason for a script whose output validation had just failed.
    ///
    /// Asserted by SUBSTRING because the sibling test is too: it passes whatever the
    /// tail does, so it is blind to the very bug it looks like it covers.
    #[test]
    fn build_script_tail_does_not_double_fail_on_a_refused_completion() {
        let script = build_script_tail(Path::new("/tmp/body.md"), 60);
        assert!(
            !script.contains("pdo complete || pdo fail"),
            "the bare `||` doubles a failure the daemon already recorded: {script}"
        );
        assert!(
            script.contains("-ne 4"),
            "the tail must discriminate exit code 4 (refused, already ruled): {script}"
        );
        assert!(
            !script.contains("output validation failed after script success"),
            "that reason was false — the daemon's own reason is the truthful one: {script}"
        );
    }

    /// The full arm matrix of the tail, so a future edit cannot quietly drop one.
    /// `ec` is the author's bash exit code, `cc` is `pdo complete`'s.
    #[test]
    fn build_script_tail_covers_every_arm() {
        let script = build_script_tail(Path::new("/tmp/body.md"), 60);
        for needle in [
            // ec = 0 → try to complete
            "if [ $ec -eq 0 ]; then pdo complete",
            // cc = 0 (granted or legal duplicate) and cc = 4 (already ruled) → do
            // nothing. Anything else → signal the failure ourselves.
            "if [ $cc -ne 0 ] && [ $cc -ne 4 ]",
            "pdo complete refused with exit $cc after script success",
            // ec = 124 → the `timeout` verdict
            "elif [ $ec -eq 124 ]; then pdo fail --reason \"script timed out after 60s\"",
            // any other ec → the author's own failure
            "else pdo fail --reason \"script exited $ec\"",
        ] {
            assert!(
                script.contains(needle),
                "missing arm {needle:?} in: {script}"
            );
        }
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
            None,
            None,
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
            None,
            None,
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
            None,
            None,
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
            None,
            None,
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

    // -- #407 sandbox wrapping goldens --------------------------------------

    fn sample_wrap<'a>(marker: &'a str, workdir: &'a Path) -> SandboxWrap<'a> {
        SandboxWrap {
            docker_bin: "docker",
            uid: 1000,
            gid: 1000,
            marker,
            workdir,
        }
    }

    #[test]
    fn sandbox_wraps_agent_tail_in_docker_exec() {
        // #407 D5: a sandboxed agent tail runs INSIDE `pdo-sbx-<run>` via a
        // `docker exec … bash -lc '<exec claude …>'`. The host env exports still
        // run (harmless); the marker equals the session name (the kill target).
        let prompt_path = Path::new("/repo/.pdo/runs/r1/worktree/.pdo/prompts/solo-iter-1.md");
        let wt = Path::new("/repo/.pdo/runs/r1/nodes/solo/iter-1");
        let wrap = sample_wrap("pdo-r1-solo-iter-1", wt);
        let script = build_tmux_script(
            "r1",
            "solo",
            1,
            6172,
            prompt_path,
            None,
            SessionTail::Agent {
                harness: &crate::harness_registry::claude(),
                model: None,
                effort: None,
                session_id: None,
            },
            Some(&wrap),
            None,
        );
        // Host wrapper preserved.
        assert!(script.starts_with("exec bash -c "), "{script}");
        assert!(script.contains("export PDO_RUN_ID="), "{script}");
        // The container-exec prefix, in canonical order.
        assert!(
            script.contains(
                "docker exec -i -t -e PDO_SBX_SESSION=pdo-r1-solo-iter-1 \
                 -e PDO_NODE_ID -e PDO_NODE_ITER --user 1000:1000 \
                 -w /repo/.pdo/runs/r1/nodes/solo/iter-1 pdo-sbx-r1 bash -lc"
            ),
            "docker exec prefix missing/wrong: {script}"
        );
        // The claude tail runs inside the container.
        assert!(
            script.contains("exec claude --dangerously-skip-permissions"),
            "{script}"
        );
        // PDO_DAEMON_URL appears ONCE (the host export), never re-forwarded on the
        // exec (would clobber the host-gateway URL posted at create).
        assert_eq!(
            script.matches("PDO_DAEMON_URL").count(),
            1,
            "PDO_DAEMON_URL must not be re-forwarded into the container: {script}"
        );
    }

    #[test]
    fn sandbox_wraps_script_tail_with_env_catalogue() {
        // #407 D6: a `script` node's dynamic catalogue crosses the exec as
        // explicit `-e K=V` (a bare host export wouldn't cross), NOT as a host
        // export — so each catalogue key appears exactly once (on the exec).
        let prompt_path = Path::new("/repo/.pdo/runs/r1/worktree/.pdo/prompts/solo-iter-1.md");
        let wt = Path::new("/repo/.pdo/runs/r1/worktree");
        let wrap = sample_wrap("pdo-r1-solo-iter-1", wt);
        let env = vec![
            (
                "PDO_ARTIFACTS_DIR".to_string(),
                "/repo/.pdo/runs/r1/worktree/.pdo/artifacts".to_string(),
            ),
            (
                "PDO_OUTPUT_out".to_string(),
                "/repo/.pdo/runs/r1/worktree/.pdo/artifacts/solo/iter-1/out/output.md".to_string(),
            ),
            ("PDO_VAR_x".to_string(), "hello".to_string()),
        ];
        let script = build_tmux_script(
            "r1",
            "solo",
            1,
            6172,
            prompt_path,
            None,
            SessionTail::Script {
                timeout_secs: 60,
                env: &env,
            },
            Some(&wrap),
            None,
        );
        assert!(
            script.contains("-e PDO_ARTIFACTS_DIR=/repo/.pdo/runs/r1/worktree/.pdo/artifacts"),
            "{script}"
        );
        assert!(
            script.contains(
                "-e PDO_OUTPUT_out=/repo/.pdo/runs/r1/worktree/.pdo/artifacts/solo/iter-1/out/output.md"
            ),
            "{script}"
        );
        assert!(script.contains("-e PDO_VAR_x=hello"), "{script}");
        // The catalogue is NOT host-exported (only the base four are): each key
        // appears exactly once, on the exec.
        assert_eq!(
            script.matches("PDO_ARTIFACTS_DIR").count(),
            1,
            "catalogue must cross the exec once, not also host-exported: {script}"
        );
        // The body runs under timeout, self-signalling — inside the container.
        assert!(script.contains("timeout 60s bash"), "{script}");
        assert!(script.contains("pdo complete"), "{script}");
        assert_eq!(script.matches("PDO_DAEMON_URL").count(), 1, "{script}");
    }

    #[test]
    fn sandbox_wraps_shell_tail() {
        // #407: the run shell family is wrapped too — the respawn loop runs in the
        // container.
        let wt = Path::new("/repo/.pdo/runs/r1/worktree");
        let wrap = sample_wrap("pdo-shell-r1", wt);
        let script = build_tmux_script(
            "r1",
            "__shell__",
            0,
            6172,
            wt,
            None,
            SessionTail::Shell,
            Some(&wrap),
            None,
        );
        assert!(
            script.contains("-e PDO_SBX_SESSION=pdo-shell-r1"),
            "shell marker = shell session name: {script}"
        );
        assert!(
            script.contains("while true; do bash -i; sleep 0.2; done"),
            "shell respawn loop preserved inside the container: {script}"
        );
    }

    #[test]
    fn sandbox_none_is_byte_identical() {
        // #407 invariant: with no `SandboxWrap`, the emitted bytes are exactly the
        // legacy (host) command — no docker anywhere on the `off` parcours.
        let prompt_path = Path::new("/tmp/p.md");
        let with_none = build_tmux_script(
            "r",
            "n",
            1,
            5172,
            prompt_path,
            None,
            SessionTail::Agent {
                harness: &crate::harness_registry::claude(),
                model: None,
                effort: None,
                session_id: None,
            },
            None,
            None,
        );
        assert!(
            !with_none.contains("docker"),
            "off path must not mention docker: {with_none}"
        );
        assert!(!with_none.contains("pdo-sbx-"), "{with_none}");
        assert!(
            with_none.contains("exec claude --dangerously-skip-permissions \"$(cat "),
            "legacy tail preserved byte-for-byte: {with_none}"
        );
    }

    #[test]
    fn build_resume_script_wraps_continue_in_docker_exec() {
        // #407: the 6th tail path (the resume) is wrapped identically. No pinned id
        // here (#473) ⇒ the legacy `--continue` fallback.
        let wt = Path::new("/repo/.pdo/runs/r1/nodes/solo/iter-1");
        let wrap = sample_wrap("pdo-r1-solo-iter-1", wt);
        let wrapped = build_resume_script(
            "r1",
            "solo",
            1,
            6172,
            &crate::harness_registry::claude(),
            None,
            None,
            None,
            Some(&wrap),
            None,
        );
        assert!(
            wrapped.contains("docker exec -i -t -e PDO_SBX_SESSION=pdo-r1-solo-iter-1"),
            "{wrapped}"
        );
        assert!(wrapped.contains("pdo-sbx-r1 bash -lc"), "{wrapped}");
        assert!(wrapped.contains("--continue"), "{wrapped}");
        // off path unchanged.
        let off = build_resume_script(
            "r1",
            "solo",
            1,
            6172,
            &crate::harness_registry::claude(),
            None,
            None,
            None,
            None,
            None,
        );
        assert!(!off.contains("docker"), "{off}");
        assert!(
            off.contains("exec claude --dangerously-skip-permissions --continue"),
            "{off}"
        );
    }

    #[test]
    fn build_resume_script_resumes_by_session_id_when_pinned() {
        // #473: a node with a pinned session id resumes by IDENTITY — `--resume
        // <uuid>`, which re-enters this node's own transcript, never "the newest
        // conversation of the cwd" (which is the manager's or a sibling's when the
        // cwd is the shared Run worktree). The uuid is single-quoted, hence `'\''`
        // once `wrap_with_env` re-wraps the tail in `bash -c '…'`.
        let sid = "11111111-2222-3333-4444-555555555555";
        let off = build_resume_script(
            "r1",
            "solo",
            1,
            6172,
            &crate::harness_registry::claude(),
            None,
            Some(sid),
            None,
            None,
            None,
        );
        assert!(
            off.contains(&format!(r"--resume '\''{sid}'\''")),
            "resume must target the pinned session id: {off}"
        );
        assert!(
            !off.contains("--continue"),
            "a pinned id must NOT fall back to positional --continue: {off}"
        );
        // Sandboxed: same identity resume, wrapped into the container. The docker
        // exec re-quotes the tail (`sh_quote_arg` around the outer `bash -c`), so
        // the single-quote escaping is doubled — assert on the flag + the verbatim
        // uuid, not on the exact quoting.
        let wt = Path::new("/repo/.pdo/runs/r1/worktree");
        let wrap = sample_wrap("pdo-r1-solo-iter-1", wt);
        let wrapped = build_resume_script(
            "r1",
            "solo",
            1,
            6172,
            &crate::harness_registry::claude(),
            Some("low"),
            Some(sid),
            None,
            Some(&wrap),
            None,
        );
        assert!(wrapped.contains("pdo-sbx-r1 bash -lc"), "{wrapped}");
        assert!(wrapped.contains("--resume"), "{wrapped}");
        assert!(
            wrapped.contains(sid),
            "the resumed uuid must survive the wrap: {wrapped}"
        );
        // Effort is still re-posed after the resume flag (#424).
        assert!(wrapped.contains("--effort"), "{wrapped}");
    }

    #[test]
    fn build_resume_script_takes_the_resume_verb_from_the_descriptor() {
        // #614: the resume verb is the descriptor's property, not a seam constant.
        // copilot resumes by identity with ITS `--resume`, and — because its
        // `resume_blind` is empty — a resume with no id renders NO resume flag,
        // never a blind `--continue`.
        let sid = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
        let by_id = build_resume_script(
            "r1",
            "solo",
            1,
            6172,
            &crate::harness_registry::copilot(),
            None,
            Some(sid),
            None,
            None,
            None,
        );
        assert!(
            by_id.contains(&format!(r"--resume '\''{sid}'\''")),
            "copilot resumes by identity with its own --resume verb: {by_id}"
        );
        assert!(
            !by_id.contains("--continue"),
            "copilot never blind-continues: {by_id}"
        );

        let no_id = build_resume_script(
            "r1",
            "solo",
            1,
            6172,
            &crate::harness_registry::copilot(),
            None,
            None,
            None,
            None,
            None,
        );
        assert!(
            !no_id.contains("--resume") && !no_id.contains("--continue"),
            "with no identity copilot renders no resume flag at all (AC): {no_id}"
        );
    }

    #[test]
    fn build_resume_script_falls_back_to_continue_for_empty_session_id() {
        // #473: an empty pinned id behaves exactly like `None` (a pre-#473 row) —
        // the legacy positional `--continue`, byte-identical.
        let script = build_resume_script(
            "r1",
            "solo",
            1,
            6172,
            &crate::harness_registry::claude(),
            None,
            Some(""),
            None,
            None,
            None,
        );
        assert!(!script.contains("--resume"), "{script}");
        assert!(
            script.contains(r"exec claude --dangerously-skip-permissions --continue'"),
            "empty id must degrade to the legacy --continue tail: {script}"
        );
    }

    #[test]
    fn build_resume_script_omits_effort_when_none_or_empty() {
        // #424: a node launched without an effort resumes on a BARE `--continue`,
        // byte-identical to the legacy tail. `Some("")` behaves like `None` — the
        // same last-resort guard as the spawn tail.
        for effort in [None, Some("")] {
            let script = build_resume_script(
                "r1",
                "solo",
                1,
                6172,
                &crate::harness_registry::claude(),
                effort,
                None,
                None,
                None,
                None,
            );
            assert!(
                !script.contains("--effort"),
                "no effort flag when unset ({effort:?}): {script}"
            );
            // Nothing trails `--continue`: the tail ends there (before the closing
            // quote of the `bash -c '…'` wrapper).
            assert!(
                script.contains(r"exec claude --dangerously-skip-permissions --continue'"),
                "legacy resume tail must be byte-identical ({effort:?}): {script}"
            );
        }
    }

    #[test]
    fn build_resume_script_reposes_effort_when_some() {
        // #424 (slice C): `--continue` restores the MODEL from the transcript
        // (documented guarantee) but loses the EFFORT — measured on claude 2.1.220:
        // `--effort xhigh` then `--continue` reports `auto (currently high)`, and
        // the transcript stores no effort field to read back. So the level is
        // re-posed here, from the `NodeStarted` payload. Still no `--model`: that
        // one really is restored.
        let script = build_resume_script(
            "r1",
            "solo",
            1,
            6172,
            &crate::harness_registry::claude(),
            Some("low"),
            None,
            None,
            None,
            None,
        );
        assert!(
            script.contains(r"--continue --effort '\''low'\''"),
            "effort must be re-posed right after --continue: {script}"
        );
        assert!(
            !script.contains("--model"),
            "the model is restored by --continue and must NOT be re-posed: {script}"
        );
    }

    #[test]
    fn build_resume_script_effort_ignored_under_cmd_override() {
        // The test seam REPLACES the whole tail — it does not wrap it. Pinned here
        // so nobody writes a layer-3 assertion on the resumed tail and believes it
        // (`TestDaemon::spawn` sets the override by default): such a test is
        // structurally blind to the flag.
        let script = build_resume_script(
            "r1",
            "solo",
            1,
            6172,
            &crate::harness_registry::claude(),
            Some("low"),
            Some("11111111-2222-3333-4444-555555555555"),
            Some("exec sleep 60"),
            None,
            None,
        );
        assert!(script.contains("exec sleep 60"), "{script}");
        assert!(!script.contains("--effort"), "{script}");
        // The override REPLACES the whole tail — neither the effort nor the #473
        // session-id resume flag survives it.
        assert!(!script.contains("--resume"), "{script}");
        assert!(!script.contains("claude"), "{script}");
    }
}
