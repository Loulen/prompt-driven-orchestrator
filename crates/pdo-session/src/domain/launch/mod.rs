//! What a tmux session runs: the launch selector ([`SessionTail`]), the
//! sandbox wrap, and the pure builders of the script handed to
//! `tmux new-session` (ADR-0009 layer 1: no tmux, no filesystem, no clock).

mod quote;
mod script;
mod tails;
mod wrap;

pub use quote::{sh_quote_arg, sh_single_quote};
pub use script::{build_resume_script, build_tmux_script};
pub use tails::{build_script_tail, LIBASSIST_HOOK_SETTINGS_JSON};
pub use wrap::host_daemon_url;

/// Default wall-clock bound for a `script` node's bash body (#248 / ADR-0017).
/// Mirrors the trigger guard's 60 s (`pdo_trigger::domain::guard`). A script has no JSONL, so
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
    /// Agent node / manager. `model` is the per-node model
    /// override (#296); `None` ⇒ account default (byte-identical legacy launch).
    Agent {
        /// The resolved harness descriptor (#550, ADR-0045). Its `launch`
        /// template renders the tail via [`pdo_harness::argv`]; its `env` block
        /// is exported before the tail (that is where `claude`'s CCR suppression
        /// now comes from — AC #4). For infra sessions and the byte-identity gate
        /// this is the `claude` descriptor, whose template reproduces the legacy
        /// tail exactly once its holes are empty.
        harness: &'a pdo_harness::registry::HarnessDescriptor,
        model: Option<&'a str>,
        /// Per-node reasoning-effort override (#424). `None` *or* an empty string
        /// ⇒ no `--effort`, byte-identical tail. A `Script` tail carries no such
        /// field, so the type itself guarantees a `script` node never gets one.
        effort: Option<&'a str>,
        /// #473: the PDO-pinned Claude Code session id (`claude --session-id
        /// <uuid>`). Claude Code names its transcript `<session_id>.jsonl`, so
        /// pinning it lets the liveness sweep resolve a node's transcript by
        /// *identity* instead of by the newest `.jsonl` in a cwd it shares with the
        /// manager and any sibling non-isolated node. `None` *or* an
        /// empty string ⇒ no `--session-id`, byte-identical legacy tail — the state
        /// for infra sessions (`__manager__`) that own no
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
    /// prompt file, no I/O catalogue. The loop is load-bearing — see the `Shell`
    /// arm of [`build_tmux_script`]. Deterministic like
    /// [`SessionTail::Script`], so it **ignores** `tmux_cmd_override` (the test
    /// seam must never swap the real bash for a `sleep`). Still
    /// `wrap_with_env`-wrapped so every respawned `bash -i` inherits
    /// `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1` and a user-typed `claude`
    /// can't SIGKILL live sibling sessions.
    Shell,
}

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
    /// How the session enters its Run's container. The sandbox context builds
    /// the `docker exec` argv; the session only splices its tail after it.
    pub exec: &'a dyn ContainerExec,
    /// The env of the resolved harness's **staging set** (#708, ADR-0063 §1 —
    /// `pi`: `PI_SKIP_VERSION_CHECK=1`, `PI_TELEMETRY=0`), forwarded on the exec as
    /// explicit `-e K=V` **before** the per-node catalogue. The descriptor's own
    /// `env` block is exported host-side and never crosses the exec, so this is the
    /// only way a set's env reaches the container. Empty for `claude` (no env) ⇒ its
    /// exec argv is byte-identical to before.
    pub set_env: &'a [(String, String)],
}

/// The `docker exec` into a Run's sandbox container, as the sandbox context
/// builds it (#407). The session marker it poses **MUST** equal the tmux
/// session name the kill path uses (`PDO_SBX_SESSION`), or the targeted `/proc`
/// kill misses its tree.
pub trait ContainerExec: Send + Sync {
    /// The whole argv, from the `docker` binary to the container name, with
    /// `env` forwarded as valued `-e K=V` pairs.
    fn exec_argv(&self, env: &[(String, String)]) -> Vec<String>;
}
