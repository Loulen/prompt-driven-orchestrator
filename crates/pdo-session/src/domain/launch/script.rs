//! The script `tmux new-session` runs for a node, a resumed node, a run shell
//! or the library assistant — pure string building.

use std::path::Path;

use super::quote::sh_single_quote;
use super::tails::{build_agent_tail, build_script_tail, forced_ccr_env};
use super::wrap::{extension_exports, wrap_tail_in_docker_exec, wrap_with_env};
use super::{SandboxWrap, SessionTail};

/// Construct the script tmux launches for a node run.
///
/// `tmux_cmd_override` replaces the default `claude …` tail when `Some` — the
/// per-daemon test seam (see [`pdo_kernel::env::TMUX_CMD_OVERRIDE_ENV`]). `None` → production
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
    session_path: &str,
    prompt_path: &Path,
    tmux_cmd_override: Option<&str>,
    tail: SessionTail<'_>,
    sandbox: Option<&SandboxWrap<'_>>,
    settings_path: Option<&Path>,
) -> String {
    build_tmux_script_with(
        run_id,
        node_id,
        iter,
        daemon_port,
        session_path,
        prompt_path,
        tmux_cmd_override,
        tail,
        sandbox,
        settings_path,
        &[],
    )
}

/// [`build_tmux_script`], plus the variables the extensions add to the Node
/// (`extension_env`, API 0.8). They are exported last on the host, forwarded
/// as `-e K=V` on the exec when sandboxed, and **never** overwrite a variable
/// of the core ([`extension_exports`]): an empty list is byte-identical to
/// [`build_tmux_script`].
#[allow(clippy::too_many_arguments)]
pub fn build_tmux_script_with(
    run_id: &str,
    node_id: &str,
    iter: i64,
    daemon_port: u16,
    session_path: &str,
    prompt_path: &Path,
    tmux_cmd_override: Option<&str>,
    tail: SessionTail<'_>,
    sandbox: Option<&SandboxWrap<'_>>,
    settings_path: Option<&Path>,
    extension_env: &[(String, String)],
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
            // seam must not clobber it, so `tmux_cmd_override` is ignored here.
            //
            // Don't collapse this to a bare `exec bash -i`: an interactive bash
            // exits on EOF (a stray Ctrl-D, an explicit `exit`, or the PTY bridge
            // tearing the pane's input down when the modal/tab closes) and, being
            // the session's only window, takes the whole session down with it —
            // losing the long-running command the feature exists to preserve. The
            // `while true` loop respawns bash in the *same* pane (scrollback
            // preserved); `sleep 0.2` keeps it from busy-spinning if bash ever
            // exits instantly on a permanent-EOF stdin.
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
    let set_env = sandbox.map(|wrap| wrap.set_env).unwrap_or_default();
    let node_env: Vec<(String, String)> = extra_env
        .iter()
        .cloned()
        .chain(extension_exports(
            extension_env,
            &[&harness_env, extra_env, set_env],
        ))
        .collect();
    match sandbox {
        Some(wrap) => {
            let docker_tail = wrap_tail_in_docker_exec(wrap, &node_env, &tail_cmd);
            wrap_with_env(
                run_id,
                node_id,
                iter,
                daemon_port,
                session_path,
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
            session_path,
            &harness_env,
            &node_env,
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
/// for a non-isolated node that cwd is the Run worktree, shared with
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
/// `effort` IS re-posed, unlike the model. Measured on claude 2.1.220: `--effort
/// xhigh` then a resume reports `auto (currently high)` — the level is lost, and
/// the transcript stores no `effort` field for anything to read back. It is
/// re-posed from the `NodeStarted` payload (launch-time value, not the current
/// YAML — ADR-0007: an edit has no effect on a live node's current iter). `None`
/// or an empty string ⇒ no `--effort`.
///
/// `settings_path` (#433 / ADR-0043 D7) re-arms the turn-end `Stop` hook so a
/// resurrected session does not silently lose it; `None` ⇒ no `--settings`.
// Every argument is an irreducible input to the resume script (identity, working
// context, launch selectors, sandbox wrap, settings path); bundling them into a
// struct would only move the list, not shorten it — same rationale as
// `resume` / `spawn` / `build_tmux_script`.
#[allow(clippy::too_many_arguments)]
pub fn build_resume_script(
    run_id: &str,
    node_id: &str,
    iter: i64,
    daemon_port: u16,
    session_path: &str,
    descriptor: &pdo_harness::registry::HarnessDescriptor,
    effort: Option<&str>,
    session_id: Option<&str>,
    tmux_cmd_override: Option<&str>,
    sandbox: Option<&SandboxWrap<'_>>,
    settings_path: Option<&Path>,
) -> String {
    build_resume_script_with(
        run_id,
        node_id,
        iter,
        daemon_port,
        session_path,
        descriptor,
        effort,
        session_id,
        tmux_cmd_override,
        sandbox,
        settings_path,
        &[],
    )
}

/// [`build_resume_script`], with the extensions' variables re-posed as at
/// spawn ([`build_tmux_script_with`]).
#[allow(clippy::too_many_arguments)]
pub fn build_resume_script_with(
    run_id: &str,
    node_id: &str,
    iter: i64,
    daemon_port: u16,
    session_path: &str,
    descriptor: &pdo_harness::registry::HarnessDescriptor,
    effort: Option<&str>,
    session_id: Option<&str>,
    tmux_cmd_override: Option<&str>,
    sandbox: Option<&SandboxWrap<'_>>,
    settings_path: Option<&Path>,
    extension_env: &[(String, String)],
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
    let holes = pdo_harness::argv::Holes {
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
        None => pdo_harness::argv::render(&descriptor.resume, &holes),
    };

    // #407: the resume tail is wrapped identically — a resumed sandboxed session
    // re-enters the same container. `--continue` matches its transcript by
    // working-dir path; the container mounts the repo at the same host path, so
    // the path (hence the transcript) still matches.
    let harness_env = descriptor.env.clone();
    let set_env = sandbox.map(|wrap| wrap.set_env).unwrap_or_default();
    let node_env = extension_exports(extension_env, &[&harness_env, set_env]);
    match sandbox {
        Some(wrap) => {
            let docker_tail = wrap_tail_in_docker_exec(wrap, &node_env, &tail_cmd);
            wrap_with_env(
                run_id,
                node_id,
                iter,
                daemon_port,
                session_path,
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
            session_path,
            &harness_env,
            &node_env,
            &tail_cmd,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::launch::ContainerExec;

    /// A fixed, non-empty session `PATH` for the builder goldens (#661).
    /// Keeps them hermetic (no ambient shell probe) while exercising the
    /// ADR-0055 `export PATH=…` the live spawn threads from `harness_probe_path`.
    const TEST_SESSION_PATH: &str = "/home/u/.local/bin:/usr/bin:/bin";

    #[test]
    fn build_script_default_and_override() {
        let prompt_path = Path::new("/tmp/test-prompt.md");

        // None → production claude tail.
        let script = build_tmux_script(
            "run-abc",
            "solo",
            1,
            5172,
            TEST_SESSION_PATH,
            prompt_path,
            None,
            SessionTail::Agent {
                harness: &pdo_harness::registry::claude(),
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
            TEST_SESSION_PATH,
            prompt_path,
            Some("exec sleep 60"),
            SessionTail::Agent {
                harness: &pdo_harness::registry::claude(),
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

    /// #661/ADR-0055 — the fix. The session `PATH` is exported at the **head** of
    /// the wrapper, before every `PDO_*` export and before the tail, so a `bash -c`
    /// tail (neither interactive nor login) resolves the harness in the very `PATH`
    /// the preflight (`binary_available`) found it on — not the tmux server's
    /// inherited service `PATH`. Without it a user-installed `claude` (native
    /// installer → `~/.local/bin`) is `command not found` → exit 127 → the lone
    /// window returns → the session/server dies → the sweep mis-blames tmux.
    #[test]
    fn build_script_exports_session_path_before_everything() {
        let prompt_path = Path::new("/tmp/test-prompt.md");
        let script = build_tmux_script(
            "run-abc",
            "solo",
            1,
            5172,
            "/home/u/.local/bin:/usr/bin:/bin",
            prompt_path,
            None,
            SessionTail::Agent {
                harness: &pdo_harness::registry::claude(),
                model: None,
                effort: None,
                session_id: None,
            },
            None,
            None,
        );
        // Present and single-quoted, carrying the user prefix the service PATH lacks.
        // `wrap_with_env` re-wraps the inner in `bash -c '…'`, so the single quotes
        // around the value are rewritten as `'\''` (the same escaping `--model` uses).
        assert!(
            script.contains(r"export PATH='\''/home/u/.local/bin:/usr/bin:/bin'\'' &&"),
            "session PATH must be exported at the head: {script}"
        );
        // It leads: PATH before the PDO_* exports, and those before the tail — so the
        // tail (and any binary the exports touch) resolves on the exported PATH.
        let path_at = script.find("export PATH=").expect("PATH export present");
        let run_id_at = script
            .find("export PDO_RUN_ID=")
            .expect("PDO_RUN_ID export present");
        let tail_at = script.find("exec claude").expect("claude tail present");
        assert!(
            path_at < run_id_at && run_id_at < tail_at,
            "PATH export must precede the PDO_* exports and the tail: {script}"
        );
    }

    /// #661 — an **empty** session PATH emits NO `export PATH`: never `export
    /// PATH=''`, which would erase the PATH and break even `bash`/`pdo`. Doubles as
    /// the byte-identity escape hatch — the inner still opens on `PDO_RUN_ID`, the
    /// legacy shape.
    #[test]
    fn build_script_omits_path_export_when_session_path_empty() {
        let prompt_path = Path::new("/tmp/test-prompt.md");
        let script = build_tmux_script(
            "run-abc",
            "solo",
            1,
            5172,
            "",
            prompt_path,
            None,
            SessionTail::Agent {
                harness: &pdo_harness::registry::claude(),
                model: None,
                effort: None,
                session_id: None,
            },
            None,
            None,
        );
        assert!(
            !script.contains("export PATH="),
            "empty session PATH must emit no PATH export: {script}"
        );
        assert!(
            script.starts_with("exec bash -c 'export PDO_RUN_ID="),
            "legacy inner shape preserved when no session PATH: {script}"
        );
    }

    /// #661 — a resumed session gets the same leading PATH export. A resurrected
    /// node re-enters the same harness, so it must resolve `claude` where the
    /// preflight did or it dies the identical `command not found` death.
    #[test]
    fn resume_script_exports_session_path_before_everything() {
        let script = build_resume_script(
            "r1",
            "solo",
            1,
            6172,
            "/home/u/.local/bin:/usr/bin:/bin",
            &pdo_harness::registry::claude(),
            None,
            Some("11111111-2222-3333-4444-555555555555"),
            None,
            None,
            None,
        );
        assert!(
            script.contains(r"export PATH='\''/home/u/.local/bin:/usr/bin:/bin'\'' &&"),
            "resume must export the session PATH: {script}"
        );
        let path_at = script
            .find("export PATH=")
            .expect("PATH export present on resume");
        let run_id_at = script
            .find("export PDO_RUN_ID=")
            .expect("PDO_RUN_ID export present");
        assert!(
            path_at < run_id_at,
            "PATH export must lead on resume too: {script}"
        );
        assert!(
            script.contains("--resume"),
            "resume tail still renders: {script}"
        );
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
            TEST_SESSION_PATH,
            prompt_path,
            None,
            SessionTail::Agent {
                harness: &pdo_harness::registry::claude(),
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
            TEST_SESSION_PATH,
            prompt_path,
            None,
            SessionTail::Agent {
                harness: &pdo_harness::registry::claude(),
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
            TEST_SESSION_PATH,
            prompt_path,
            None,
            SessionTail::Agent {
                harness: &pdo_harness::registry::claude(),
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
            TEST_SESSION_PATH,
            Path::new("/tmp/test-prompt.md"),
            None,
            SessionTail::Agent {
                harness: &pdo_harness::registry::claude(),
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
            TEST_SESSION_PATH,
            Path::new("/tmp/test-prompt.md"),
            None,
            SessionTail::Agent {
                harness: &pdo_harness::registry::claude(),
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
            TEST_SESSION_PATH,
            Path::new("/tmp/test-prompt.md"),
            None,
            SessionTail::Agent {
                harness: &pdo_harness::registry::claude(),
                model,
                effort,
                session_id,
            },
            None,
            None,
        )
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
            TEST_SESSION_PATH,
            Path::new("/tmp/test-prompt.md"),
            None,
            SessionTail::Agent {
                harness: &pdo_harness::registry::claude(),
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
        // #473: `None` OR `Some("")` (an infra session — the manager)
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
        let wrap_exec = TestExec::new("run-abc", "pdo-run-abc-solo-iter-1", Path::new("/wd"));
        let wrap = SandboxWrap {
            exec: &wrap_exec,
            set_env: &[],
        };
        let script = build_tmux_script(
            "run-abc",
            "solo",
            1,
            5172,
            TEST_SESSION_PATH,
            Path::new("/tmp/test-prompt.md"),
            None,
            SessionTail::Agent {
                harness: &pdo_harness::registry::claude(),
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
            TEST_SESSION_PATH,
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
            TEST_SESSION_PATH,
            &pdo_harness::registry::claude(),
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
            TEST_SESSION_PATH,
            &pdo_harness::registry::claude(),
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
            TEST_SESSION_PATH,
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
            TEST_SESSION_PATH,
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
            TEST_SESSION_PATH,
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
            TEST_SESSION_PATH,
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
            TEST_SESSION_PATH,
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

    /// The sandbox context's `docker exec` argv (`pdo_sandbox::domain::container::exec_prefix_with_env`),
    /// replicated for the goldens: the three base `-e`, the forwarded env (never
    /// `PDO_DAEMON_URL`), then `--user`, `-w` and the Run's container.
    struct TestExec<'a> {
        run_id: &'a str,
        marker: &'a str,
        workdir: &'a Path,
    }

    impl<'a> TestExec<'a> {
        fn new(run_id: &'a str, marker: &'a str, workdir: &'a Path) -> Self {
            Self {
                run_id,
                marker,
                workdir,
            }
        }
    }

    impl ContainerExec for TestExec<'_> {
        fn exec_argv(&self, env: &[(String, String)]) -> Vec<String> {
            let mut args: Vec<String> = ["docker", "exec", "-i", "-t", "-e"]
                .iter()
                .map(|s| s.to_string())
                .collect();
            args.push(format!("PDO_SBX_SESSION={}", self.marker));
            for a in ["-e", "PDO_NODE_ID", "-e", "PDO_NODE_ITER"] {
                args.push(a.to_string());
            }
            for (k, v) in env.iter().filter(|(k, _)| k != "PDO_DAEMON_URL") {
                args.push("-e".to_string());
                args.push(format!("{k}={v}"));
            }
            args.push("--user".to_string());
            args.push("1000:1000".to_string());
            args.push("-w".to_string());
            args.push(self.workdir.display().to_string());
            args.push(format!("pdo-sbx-{}", self.run_id));
            args
        }
    }

    fn sample_wrap<'a>(exec: &'a TestExec<'a>) -> SandboxWrap<'a> {
        SandboxWrap { exec, set_env: &[] }
    }

    /// #708: a staging set's env (pi's two vars) rides the exec as valued `-e K=V`,
    /// before the catalogue and never as a host-side export; an empty set env leaves
    /// the exec argv byte-identical (claude).
    #[test]
    fn sandbox_wrap_forwards_the_staging_set_env_on_the_exec() {
        let harness = pdo_harness::registry::pi();
        let set_env = vec![
            ("PI_SKIP_VERSION_CHECK".to_string(), "1".to_string()),
            ("PI_TELEMETRY".to_string(), "0".to_string()),
        ];
        let wrap_exec = TestExec::new("run-abc", "pdo-run-abc-solo-iter-1", Path::new("/wd"));
        let wrap = SandboxWrap {
            exec: &wrap_exec,
            set_env: &set_env,
        };
        let tail = SessionTail::Agent {
            harness: &harness,
            model: None,
            effort: None,
            session_id: Some("sid-1"),
        };
        let with = build_tmux_script(
            "run-abc",
            "solo",
            1,
            5172,
            TEST_SESSION_PATH,
            Path::new("/wd/.pdo/prompts/solo.md"),
            None,
            tail,
            Some(&wrap),
            None,
        );
        let exec_at = with.find(" exec -i -t ").expect("a docker exec");
        let container_at = with.find("pdo-sbx-run-abc").expect("the container name");
        let exec_argv = &with[exec_at..container_at];
        assert!(
            exec_argv.contains("-e PI_SKIP_VERSION_CHECK=1 -e PI_TELEMETRY=0 --user 1000:1000"),
            "the set env is valued `-e` pairs on the exec, before --user: {exec_argv}"
        );

        let bare_exec = TestExec::new("run-abc", "pdo-run-abc-solo-iter-1", Path::new("/wd"));
        let bare = sample_wrap(&bare_exec);
        let without = build_tmux_script(
            "run-abc",
            "solo",
            1,
            5172,
            TEST_SESSION_PATH,
            Path::new("/wd/.pdo/prompts/solo.md"),
            None,
            SessionTail::Agent {
                harness: &harness,
                model: None,
                effort: None,
                session_id: Some("sid-1"),
            },
            Some(&bare),
            None,
        );
        let exec_at = without.find(" exec -i -t ").unwrap();
        let container_at = without.find("pdo-sbx-run-abc").unwrap();
        assert!(
            !without[exec_at..container_at].contains("PI_TELEMETRY"),
            "no set env ⇒ nothing added to the exec: {without}"
        );
    }

    #[test]
    fn sandbox_wraps_agent_tail_in_docker_exec() {
        // #407 D5: a sandboxed agent tail runs INSIDE `pdo-sbx-<run>` via a
        // `docker exec … bash -lc '<exec claude …>'`. The host env exports still
        // run (harmless); the marker equals the session name (the kill target).
        let prompt_path = Path::new("/repo/.pdo/runs/r1/worktree/.pdo/prompts/solo-iter-1.md");
        let wt = Path::new("/repo/.pdo/runs/r1/nodes/solo/iter-1");
        let wrap_exec = TestExec::new("r1", "pdo-r1-solo-iter-1", wt);
        let wrap = sample_wrap(&wrap_exec);
        let script = build_tmux_script(
            "r1",
            "solo",
            1,
            6172,
            TEST_SESSION_PATH,
            prompt_path,
            None,
            SessionTail::Agent {
                harness: &pdo_harness::registry::claude(),
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
        let wrap_exec = TestExec::new("r1", "pdo-r1-solo-iter-1", wt);
        let wrap = sample_wrap(&wrap_exec);
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
            TEST_SESSION_PATH,
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
        let wrap_exec = TestExec::new("r1", "pdo-shell-r1", wt);
        let wrap = sample_wrap(&wrap_exec);
        let script = build_tmux_script(
            "r1",
            "__shell__",
            0,
            6172,
            TEST_SESSION_PATH,
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
            TEST_SESSION_PATH,
            prompt_path,
            None,
            SessionTail::Agent {
                harness: &pdo_harness::registry::claude(),
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
        let wrap_exec = TestExec::new("r1", "pdo-r1-solo-iter-1", wt);
        let wrap = sample_wrap(&wrap_exec);
        let wrapped = build_resume_script(
            "r1",
            "solo",
            1,
            6172,
            TEST_SESSION_PATH,
            &pdo_harness::registry::claude(),
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
            TEST_SESSION_PATH,
            &pdo_harness::registry::claude(),
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
            TEST_SESSION_PATH,
            &pdo_harness::registry::claude(),
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
        let wrap_exec = TestExec::new("r1", "pdo-r1-solo-iter-1", wt);
        let wrap = sample_wrap(&wrap_exec);
        let wrapped = build_resume_script(
            "r1",
            "solo",
            1,
            6172,
            TEST_SESSION_PATH,
            &pdo_harness::registry::claude(),
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
            TEST_SESSION_PATH,
            &pdo_harness::registry::copilot(),
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
            TEST_SESSION_PATH,
            &pdo_harness::registry::copilot(),
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
    fn build_resume_script_re_arms_the_pi_turn_end_extension_only_when_given() {
        // #707 / ADR-0043 D7: a resumed pi node re-carries its `agent_settled`
        // extension through the SAME `{settings}` hole (`-e <file>`), and re-poses
        // its effort; with no file (setting off) the `-e` token drops whole — pi is
        // never handed an empty `-e`.
        let sid = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
        let ext = Path::new("/repo/.pdo/prompts/solo-iter-1.turn-end.ts");
        let armed = build_resume_script(
            "r1",
            "solo",
            1,
            6172,
            TEST_SESSION_PATH,
            &pdo_harness::registry::pi(),
            Some("high"),
            Some(sid),
            None,
            None,
            Some(ext),
        );
        assert!(
            armed.contains(&format!(r"--session-id '\''{sid}'\''")),
            "pi resumes by identity with its created-or-resumed flag: {armed}"
        );
        assert!(
            armed.contains(r"-e '\''/repo/.pdo/prompts/solo-iter-1.turn-end.ts'\''"),
            "the extension is re-armed at resume: {armed}"
        );
        assert!(armed.contains(r"--thinking '\''high'\''"), "{armed}");
        assert!(
            !armed.contains("--settings"),
            "never claude's flag: {armed}"
        );

        let off = build_resume_script(
            "r1",
            "solo",
            1,
            6172,
            TEST_SESSION_PATH,
            &pdo_harness::registry::pi(),
            None,
            Some(sid),
            None,
            None,
            None,
        );
        assert!(
            !off.contains("-e "),
            "setting off ⇒ no -e token at all: {off}"
        );
        assert!(!off.contains("turn-end.ts"), "{off}");
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
            TEST_SESSION_PATH,
            &pdo_harness::registry::claude(),
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
                TEST_SESSION_PATH,
                &pdo_harness::registry::claude(),
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
            TEST_SESSION_PATH,
            &pdo_harness::registry::claude(),
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
            TEST_SESSION_PATH,
            &pdo_harness::registry::claude(),
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

    fn ext_env(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    /// API 0.8: an extension's variables reach an agent on the host, after the
    /// core's; a variable of the core (identity, harness env) is never
    /// overwritten.
    #[test]
    fn extension_env_is_exported_to_an_agent_and_never_overwrites_the_core() {
        let script = build_tmux_script_with(
            "r1",
            "solo",
            1,
            6172,
            TEST_SESSION_PATH,
            Path::new("/p.md"),
            None,
            SessionTail::Agent {
                harness: &pdo_harness::registry::claude(),
                model: None,
                effort: None,
                session_id: None,
            },
            None,
            None,
            &ext_env(&[
                ("FIXTURE_HELLO", "monde d'ici"),
                ("PDO_RUN_ID", "hijack"),
                ("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "0"),
                ("PATH", "/evil"),
            ]),
        );
        assert!(
            script.contains("FIXTURE_HELLO="),
            "the extension's variable is exported: {script}"
        );
        assert!(!script.contains("hijack"), "{script}");
        assert!(!script.contains("TRAFFIC=0"), "{script}");
        assert!(!script.contains("/evil"), "{script}");
        let ext_at = script.find("FIXTURE_HELLO").unwrap();
        let tail_at = script.find("exec claude").unwrap();
        assert!(ext_at < tail_at, "exported before the tail: {script}");
    }

    /// An empty extension env is byte-identical to the historical builder.
    #[test]
    fn no_extension_env_is_byte_identical() {
        let tail = || SessionTail::Script {
            timeout_secs: 60,
            env: &[],
        };
        let p = Path::new("/p.md");
        assert_eq!(
            build_tmux_script("r1", "s", 1, 6172, "", p, None, tail(), None, None),
            build_tmux_script_with("r1", "s", 1, 6172, "", p, None, tail(), None, None, &[]),
        );
    }

    /// A script's catalogue is the core's too, and in a sandbox the variables
    /// cross the exec as valued `-e`, never as a host export.
    #[test]
    fn extension_env_reaches_a_sandboxed_script_without_overwriting_its_catalogue() {
        let wt = Path::new("/wd");
        let wrap_exec = TestExec::new("r1", "pdo-r1-s-iter-1", wt);
        let wrap = sample_wrap(&wrap_exec);
        let catalogue = ext_env(&[("PDO_OUTPUT_OUT", "/a/out.md")]);
        let script = build_tmux_script_with(
            "r1",
            "s",
            1,
            6172,
            TEST_SESSION_PATH,
            Path::new("/p.md"),
            None,
            SessionTail::Script {
                timeout_secs: 60,
                env: &catalogue,
            },
            Some(&wrap),
            None,
            &ext_env(&[("FIXTURE_HELLO", "monde"), ("PDO_OUTPUT_OUT", "/x")]),
        );
        assert!(
            script.contains("-e FIXTURE_HELLO=monde"),
            "forwarded on the exec: {script}"
        );
        assert!(!script.contains("export FIXTURE_HELLO"), "{script}");
        assert!(!script.contains("PDO_OUTPUT_OUT=/x"), "{script}");
        assert!(script.contains("PDO_OUTPUT_OUT=/a/out.md"), "{script}");
    }

    /// A resumed node re-carries its extensions' variables.
    #[test]
    fn a_resume_re_poses_the_extension_env() {
        let script = build_resume_script_with(
            "r1",
            "solo",
            1,
            6172,
            TEST_SESSION_PATH,
            &pdo_harness::registry::claude(),
            None,
            Some("sid"),
            None,
            None,
            None,
            &ext_env(&[
                ("CLAUDE_CONFIG_DIR", "/home/u/.pdo/test/alice"),
                ("HOME", "/x"),
            ]),
        );
        assert!(script.contains("export CLAUDE_CONFIG_DIR="), "{script}");
        assert!(!script.contains("HOME=/x"), "{script}");
    }
}
