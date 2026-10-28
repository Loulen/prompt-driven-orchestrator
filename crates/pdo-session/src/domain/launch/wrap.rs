//! The wrappers around a session's tail: the `PDO_*` env exports every session
//! runs under, and the `docker exec` into the Run's container when sandboxed.

use super::quote::{sh_quote_arg, sh_single_quote};
use super::SandboxWrap;

/// The daemon's URL as a process on its own host reaches it — what every
/// session exports as `PDO_DAEMON_URL` host-side. The container side of a
/// sandboxed Run is posed by the sandbox context at `docker create` (#447).
pub fn host_daemon_url(daemon_port: u16) -> String {
    format!("http://localhost:{daemon_port}")
}

/// The variables the core sets in every node session, whatever its tail: the
/// session `PATH` and identity (a Session hébergée's `PDO_SESSION_ID`
/// included), the daemon URL, the sandbox's `HOME` and its session marker. An
/// extension can never overwrite one of them.
pub const CORE_ENV_NAMES: &[&str] = &[
    "PATH",
    "HOME",
    "PDO_RUN_ID",
    "PDO_NODE_ID",
    "PDO_NODE_ITER",
    "PDO_DAEMON_URL",
    "PDO_SBX_SESSION",
    "PDO_SESSION_ID",
];

/// The extensions' variables a session may carry (API 0.8): every pair but
/// those whose name the core sets itself ([`CORE_ENV_NAMES`], and each name
/// of `core`: the harness's env, a script's catalogue, a staging set's env),
/// dropped with a warning. A variable of the core is never overwritten.
pub(super) fn extension_exports(
    extension_env: &[(String, String)],
    core: &[&[(String, String)]],
) -> Vec<(String, String)> {
    extension_env
        .iter()
        .filter(|(name, _)| {
            let taken = CORE_ENV_NAMES.contains(&name.as_str())
                || core.iter().any(|env| env.iter().any(|(k, _)| k == name));
            if taken {
                tracing::warn!("an extension's variable `{name}` is the core's: not overwritten");
            }
            !taken
        })
        .cloned()
        .collect()
}

/// Splice the container-exec argv (built by the sandbox context, see
/// [`super::ContainerExec`]) around a base tail:
/// `docker <exec argv incl. -e K=V> pdo-sbx-<run> bash -lc '<tail>'`.
/// The base tail is single-quoted as one `bash -lc` argument; `wrap_with_env`
/// then single-quotes the whole thing again for its outer `bash -c` (the same
/// double-quoting the `--model` path already relies on).
pub(super) fn wrap_tail_in_docker_exec(
    wrap: &SandboxWrap<'_>,
    extra_env: &[(String, String)],
    base_tail: &str,
) -> String {
    // #708: the staging set's env leads, then the per-node catalogue. Both are
    // valued `-e K=V` on the exec — a host-side export would not cross it.
    let exec_env: Vec<(String, String)> = wrap
        .set_env
        .iter()
        .cloned()
        .chain(extra_env.iter().cloned())
        .collect();
    let mut argv = wrap.exec.exec_argv(&exec_env);
    argv.push("bash".to_string());
    argv.push("-lc".to_string());
    argv.push(base_tail.to_string());
    argv.iter()
        .map(|a| sh_quote_arg(a))
        .collect::<Vec<_>>()
        .join(" ")
}

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
///
/// `session_path` is exported as the session `PATH` **before** every other export
/// (#661 / ADR-0060, prolonging ADR-0055). The preflight (`binary_available`)
/// resolves the harness on
/// [`harness_probe_path`] — the user's *interactive* `PATH` — but a `bash -c` tail
/// is neither interactive nor login, so it would otherwise run in the tmux server's
/// inherited `PATH` (the systemd unit's), where a user-installed `claude` (native
/// installer → `~/.local/bin`) is absent: `exit 127`, the lone window returns, the
/// session (and its server) dies, and the liveness sweep mis-reports `session_died`
/// naming tmux for a `PATH` fault (the mis-attribution ADR-0037 exists to prevent).
/// Exporting the very `PATH` the probe resolved on closes that asymmetry. It runs on
/// the **host** side of a `docker exec` wrapper (#447) so it never crosses into the
/// container, and stays useful there for locating `docker` itself under a user
/// prefix. An **empty** `session_path` emits nothing — never `export PATH=''`, which
/// would erase the `PATH` and break even `bash`/`pdo` (also the byte-identity escape
/// hatch for the legacy-shape goldens).
// Every argument is an irreducible input to the wrapped command (identity, port,
// session PATH, the two env layers, the tail); bundling them into a struct would
// only move the list — same rationale as the spawn/build seams.
#[allow(clippy::too_many_arguments)]
pub fn wrap_with_env(
    run_id: &str,
    node_id: &str,
    iter: i64,
    daemon_port: u16,
    session_path: &str,
    harness_env: &[(String, String)],
    extra_env: &[(String, String)],
    tail_cmd: &str,
) -> String {
    // #661/ADR-0060: the session PATH export leads, so every later export and the
    // tail resolve binaries where the preflight already found the harness. Empty ⇒
    // no export (never `export PATH=''`); `sh_single_quote` survives an exotic dir
    // name carrying a `'`, matching the other base exports.
    let path_export = if session_path.is_empty() {
        String::new()
    } else {
        format!("export PATH={} && ", sh_single_quote(session_path))
    };
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
        "{path_export}\
         export PDO_RUN_ID={run_id_q} && \
         export PDO_NODE_ID={node_id_q} && \
         export PDO_NODE_ITER={iter_q} && \
         export PDO_DAEMON_URL={daemon_url_q} && \
         {harness_exports}{extra_exports}{tail_cmd}",
        run_id_q = sh_single_quote(run_id),
        node_id_q = sh_single_quote(node_id),
        iter_q = sh_single_quote(&iter.to_string()),
        // #447: the HOST-side URL is NOT an oversight — these exports run on the
        // HOST side of the `docker exec` wrapper (see `build_tmux_script`: the
        // wrapped tail is the *argument* of `wrap_with_env`, so the exports execute
        // before the exec and never cross into the container). Resolving to the
        // gateway here would hand the host path a hostname it can't resolve, and
        // would be dead bytes on the sandbox path — the `docker create` already
        // posted the container-side value, which the exec deliberately never
        // clobbers (ADR-0030 §5).
        daemon_url_q = sh_single_quote(&host_daemon_url(daemon_port)),
    );

    format!("exec bash -c {}", sh_single_quote(&inner))
}
