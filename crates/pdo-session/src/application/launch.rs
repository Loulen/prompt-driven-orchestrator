//! Launching a session: write what it reads (prompt, turn-end hook, primer),
//! build its script, and create it through the [`Tmux`] port.

use std::path::Path;

use anyhow::Result;
use tracing::info;

use crate::application::ports::Tmux;
use crate::domain::launch::LIBASSIST_HOOK_SETTINGS_JSON;
use crate::domain::launch::{
    build_resume_script_with, build_tmux_script, build_tmux_script_with, SandboxWrap, SessionTail,
};
use crate::domain::libassist::LIBASSIST_ENV_ID;

/// Spawn a detached tmux session for a NodeRun.
///
/// `tmux_cmd_override` (per-daemon config)
/// replaces the `claude …` tail when `Some` — how tests run a harmless command
/// instead of launching real claude.
// The session identity (name + run/node/iter), working dir, daemon port, and
// command override are all irreducible inputs to a spawn; bundling them into a
// struct would only move the argument list, not shorten it.
#[allow(clippy::too_many_arguments)]
pub fn spawn(
    tmux: &dyn Tmux,
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
    extension_env: &[(String, String)],
) -> Result<()> {
    let prompt_dir = working_dir.join(".pdo").join("prompts");
    std::fs::create_dir_all(&prompt_dir)?;
    let prompt_path = prompt_dir.join(format!("{node_id}-iter-{iter}.md"));
    std::fs::write(&prompt_path, prompt)?;

    // #433 / ADR-0043: when turn-end auto-completion is enabled, drop a settings
    // file beside the prompt and reference it via `claude --settings`, so a `Stop`
    // hook runs `pdo complete --auto` at every turn end. Same lifecycle as the
    // prompt (gitignored under `.pdo/`, resolves identically host and container).
    // Callers pass `false` for `script`/manager sessions; the tail
    // selector in `build_tmux_script` is the belt-and-suspenders guard.
    //
    // #613/ADR-0051 (correctif 8): write the claude-format settings file ONLY for a
    // harness that actually has a `{settings}` hole to fill. A node on a harness
    // with none (`opencode`) would never reference the file — writing it beside the
    // prompt was the one place "absence is supplied, not said" leaked. Now the
    // absence is honoured: no hole, no file.
    //
    // #705/#707: the hole is not enough on its own — WHAT goes in it is the
    // harness's. `pi` carries a `{settings}` hole too, but fills it with
    // `-e <extension>`: handed the claude-format hook JSON it would load a JSON file
    // as an extension and refuse to start. So the file written is the one the
    // harness's declared end-of-turn substrate takes
    // (`pdo_harness::probes::turn_end_injection`): the `Stop`-hook JSON for `claude` (and
    // any data-declared harness), the `agent_settled` extension for `pi`, nothing for
    // a harness without a substrate. One switch (`inject_hook`, i.e.
    // `autocomplete_turn_end`) governs all of them; off ⇒ no file, and the token
    // (`--settings` / `-e`) drops at render.
    let settings_path = match &tail {
        SessionTail::Agent { harness, .. } if inject_hook && harness.has_settings_hole() => {
            match pdo_harness::probes::turn_end_injection(&harness.name) {
                Some(injection) => {
                    let p = prompt_dir.join(format!("{node_id}-iter-{iter}{}", injection.suffix));
                    std::fs::write(&p, injection.body)?;
                    Some(p)
                }
                None => None,
            }
        }
        _ => None,
    };

    // #661/ADR-0055: resolve the session PATH at the impure spawn edge (cached
    // `OnceLock`, so this is a cheap clone after the first probe) and thread it into
    // the pure builder — the goldens stay hermetic, the live session inherits the
    // very PATH the preflight resolved the harness on.
    let session_path = tmux.session_path();
    let script = build_tmux_script_with(
        run_id,
        node_id,
        iter,
        daemon_port,
        &session_path,
        &prompt_path,
        tmux_cmd_override,
        tail,
        sandbox,
        settings_path.as_deref(),
        extension_env,
    );
    tmux.new_session(session_name, working_dir, &script, None)?;

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
    tmux: &dyn Tmux,
    session_name: &str,
    working_dir: &Path,
    run_id: &str,
    daemon_port: u16,
    sandbox: Option<&SandboxWrap<'_>>,
) -> Result<()> {
    // prompt_path is unused for `SessionTail::Shell` (bash has no prompt);
    // pass the working_dir as a harmless placeholder.
    // #661/ADR-0055: a run shell exists to hand-type `claude`/`git`/`pdo`
    // (CONTEXT.md §*Shell de run*), so it needs the interactive PATH just as much
    // as an agent node — otherwise a typed `claude` hits the same `command not
    // found` the service PATH produces.
    let session_path = tmux.session_path();
    let script = build_tmux_script(
        run_id,
        "__shell__",
        0,
        daemon_port,
        &session_path,
        working_dir,
        None,
        SessionTail::Shell,
        sandbox,
        // An ad-hoc shell runs `bash -i`, never `claude` — no `Stop` hook (#433).
        None,
    );
    tmux.new_session(session_name, working_dir, &script, Some("shell"))?;

    info!("Spawned run shell tmux session: {session_name}");
    Ok(())
}

/// Spawn a detached tmux session running the library pipeline authoring assistant
/// (#302 / ADR-0048, reshaped by #594 / ADR-0051) — a `claude` REPL whose cwd is
/// the repo's pipelines directory.
///
/// Mirror of [`spawn`]'s agent launch, but owning no Run. `claude` does **not**
/// exit on EOF (unlike `bash -i`), so the session survives a PTY-bridge/tab close
/// — which is why it needs the sweep as a backstop (`decide_one`'s `LibAssist`
/// arm) on top of the explicit `DELETE`.
///
/// One session serves every template; the open one arrives per message via the
/// `UserPromptSubmit` hook this function arms ([`LIBASSIST_HOOK_SETTINGS_JSON`],
/// written as a sibling `settings.json` of `prompt_path`). The primer is written
/// to `prompt_path`, kept **out** of `working_dir` so the user-facing pipelines
/// directory stays clean. Never sandboxed: authoring is design-time work on the
/// host.
// Every argument is an irreducible input to the spawn (identity, working context,
// primer path, launch selector); a struct would only move the list — same
// rationale as `spawn` / `spawn_shell` / `build_tmux_script`.
#[allow(clippy::too_many_arguments)]
pub fn spawn_libassist(
    tmux: &dyn Tmux,
    session_name: &str,
    prompt: &str,
    working_dir: &Path,
    prompt_path: &Path,
    daemon_port: u16,
    harness: &pdo_harness::registry::HarnessDescriptor,
    tmux_cmd_override: Option<&str>,
) -> Result<()> {
    if let Some(parent) = prompt_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(prompt_path, prompt)?;

    // The per-message focus hook, beside the primer and with the same lifecycle.
    // Not the `Stop` hook of #433: the assistant never calls `pdo complete`.
    //
    // #705: written only for a harness whose `{settings}` hole takes the claude-format
    // file. `pi` has the hole but fills it with `-e <extension>`; this JSON would be
    // loaded as an extension and break the launch. `None` ⇒ the `{settings}` token
    // drops at render and the primer's fetch-the-focus instruction is the mechanism
    // (the caller has already said so once, ADR-0051 §3).
    let settings_path = pdo_harness::probes::settings_hole_takes_claude_file(&harness.name)
        .then(|| prompt_path.with_file_name("settings.json"));
    if let Some(p) = &settings_path {
        std::fs::write(p, LIBASSIST_HOOK_SETTINGS_JSON)?;
    }

    // #661/ADR-0055: the assistant is a `claude` REPL, so it needs the same
    // interactive PATH the preflight resolves the binary on.
    let session_path = tmux.session_path();
    let script = build_tmux_script(
        LIBASSIST_ENV_ID,
        LIBASSIST_ENV_ID,
        0,
        daemon_port,
        &session_path,
        prompt_path,
        tmux_cmd_override,
        SessionTail::Agent {
            harness,
            model: None,
            effort: None,
            session_id: None,
        },
        None, // never sandboxed
        settings_path.as_deref(),
    );
    tmux.new_session(session_name, working_dir, &script, Some("libassist"))?;

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
    tmux: &dyn Tmux,
    session_name: &str,
    working_dir: &Path,
    run_id: &str,
    node_id: &str,
    iter: i64,
    daemon_port: u16,
    descriptor: &pdo_harness::registry::HarnessDescriptor,
    effort: Option<&str>,
    session_id: Option<&str>,
    tmux_cmd_override: Option<&str>,
    sandbox: Option<&SandboxWrap<'_>>,
    inject_hook: bool,
    extension_env: &[(String, String)],
) -> Result<()> {
    // #433 / ADR-0043 (D7): a resumed session must re-carry the `Stop` hook.
    // Re-write the settings file (idempotent; same path `spawn` used) and
    // reference it. `create_dir_all` covers the rare case where the prompt dir was
    // pruned since spawn. `false` (setting off, or a `script` node) ⇒ no file, and
    // `build_resume_script` emits a byte-identical `--continue` tail.
    //
    // #613/ADR-0051 (correctif 8): as at spawn, only a harness with a `{settings}`
    // hole gets a file — a resumed `opencode` node writes none. #705/#707: and the
    // file is the one its substrate takes (`pdo_harness::probes::turn_end_injection`) —
    // a resumed `pi` node re-arms its `agent_settled` extension (ADR-0043 D7: the
    // primary substrate must survive a resume), never the claude JSON.
    let settings_path = if inject_hook && descriptor.has_settings_hole() {
        match pdo_harness::probes::turn_end_injection(&descriptor.name) {
            Some(injection) => {
                let prompt_dir = working_dir.join(".pdo").join("prompts");
                std::fs::create_dir_all(&prompt_dir)?;
                let p = prompt_dir.join(format!("{node_id}-iter-{iter}{}", injection.suffix));
                std::fs::write(&p, injection.body)?;
                Some(p)
            }
            None => None,
        }
    } else {
        None
    };
    // #661/ADR-0055: a resumed session re-enters the same harness — thread the same
    // interactive PATH the preflight resolves on, so a resurrected node does not hit
    // the `command not found` a bare service PATH would produce.
    let session_path = tmux.session_path();
    let script = build_resume_script_with(
        run_id,
        node_id,
        iter,
        daemon_port,
        &session_path,
        descriptor,
        effort,
        session_id,
        tmux_cmd_override,
        sandbox,
        settings_path.as_deref(),
        extension_env,
    );
    tmux.new_session(session_name, working_dir, &script, Some("resume"))?;

    info!("Resumed tmux session: {session_name}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::sync::Mutex;

    use crate::domain::names::node_session_name;

    /// A tmux that records the sessions it is asked to create and runs nothing.
    #[derive(Default)]
    struct Recorder {
        created: Mutex<Vec<(String, String)>>,
    }

    impl Tmux for Recorder {
        fn socket(&self) -> &str {
            "pdo-test"
        }
        fn session_path(&self) -> String {
            "/usr/bin:/bin".into()
        }
        fn new_session(
            &self,
            name: &str,
            _working_dir: &Path,
            script: &str,
            _what: Option<&str>,
        ) -> anyhow::Result<()> {
            self.created
                .lock()
                .unwrap()
                .push((name.to_string(), script.to_string()));
            Ok(())
        }
        fn kill(&self, _name: &str) {}
        fn session_exists(&self, _name: &str) -> bool {
            false
        }
        fn server_alive(&self) -> Option<bool> {
            None
        }
        fn list_pdo_sessions(&self) -> HashSet<String> {
            HashSet::new()
        }
        fn is_attached(&self, _name: &str) -> bool {
            false
        }
        fn capture(&self, _name: &str) -> Option<String> {
            None
        }
        fn send_keys(&self, _name: &str, _text: &str) {}
        fn paste(&self, _name: &str, _text: &str) {}
        fn paste_without_enter(&self, _name: &str, _text: &str) -> bool {
            false
        }
        fn attach_argv(&self, _name: &str, _ignore_size: bool) -> Vec<String> {
            Vec::new()
        }
        fn attach_command(&self, _name: &str) -> String {
            String::new()
        }
        fn set_client_ignore_size(&self, _client_pid: u32, _ignore_size: bool) -> bool {
            false
        }
    }

    #[test]
    fn spawn_writes_the_settings_file_for_a_harness_with_a_settings_hole() {
        // #613 (correctif 8) control: `claude` HAS a `{settings}` hole, so with the
        // setting on PDO writes the Stop-hook file beside the prompt, as always.
        assert!(
            spawn_and_check_settings_file(&pdo_harness::registry::claude()),
            "claude must still get its turn-end settings file"
        );
    }

    #[test]
    fn spawn_writes_the_pi_turn_end_extension_not_the_claude_json() {
        // #707: `pi` has a `{settings}` hole that takes an extension. With the
        // setting on, PDO writes `<node>-iter-<n>.turn-end.ts` (the `agent_settled`
        // extension) beside the prompt — and never the claude Stop-hook JSON, which
        // pi would try to load as an extension.
        assert_eq!(
            spawn_turn_end_files(&pdo_harness::registry::pi(), true),
            (false, true),
            "pi gets its extension and never the claude JSON"
        );
        // The control: claude gets its JSON and never pi's extension.
        assert_eq!(
            spawn_turn_end_files(&pdo_harness::registry::claude(), true),
            (true, false)
        );
    }

    #[test]
    fn spawn_writes_no_pi_extension_when_the_setting_is_off() {
        // #707 AC: unchecked ⇒ no file at all, so the `-e` token drops from the argv
        // (the `harness_argv` golden pins the argv side).
        assert_eq!(
            spawn_turn_end_files(&pdo_harness::registry::pi(), false),
            (false, false),
            "setting off ⇒ nothing written"
        );
    }

    #[test]
    fn spawn_writes_no_settings_file_for_a_harness_without_a_settings_hole() {
        // #613 (correctif 8): `opencode` has NO `{settings}` hole, so even with
        // turn-end auto-completion enabled PDO writes it no claude-format settings
        // file — the absence is honoured, not supplied. This was the one place the
        // discipline was broken.
        assert!(
            !spawn_and_check_settings_file(&pdo_harness::registry::opencode()),
            "opencode must get no settings file — it has no settings hole"
        );
    }

    /// Drive `spawn` with a benign tail and report whether it dropped the claude
    /// turn-end settings file beside the prompt.
    fn spawn_and_check_settings_file(harness: &pdo_harness::registry::HarnessDescriptor) -> bool {
        spawn_turn_end_files(harness, true).0
    }

    /// The general form (#707): drive `spawn` with `inject_hook` and report which
    /// turn-end file landed beside the prompt — `(claude settings JSON, pi
    /// extension)`. Exactly one may be true for an instrumented harness, none for a
    /// harness without a hole or with the setting off.
    fn spawn_turn_end_files(
        harness: &pdo_harness::registry::HarnessDescriptor,
        inject_hook: bool,
    ) -> (bool, bool) {
        let wd = tempfile::tempdir().unwrap();
        let tmux = Recorder::default();
        let session = node_session_name("run-c8", "n", 1);
        spawn(
            &tmux,
            &session,
            "prompt body",
            wd.path(),
            "run-c8",
            "n",
            1,
            6172,
            Some("true"),
            SessionTail::Agent {
                harness,
                model: None,
                effort: None,
                session_id: None,
            },
            None,
            inject_hook,
            &[],
        )
        .unwrap();
        assert_eq!(tmux.created.lock().unwrap()[0].0, session);
        let prompts = wd.path().join(".pdo").join("prompts");
        assert_eq!(
            std::fs::read_to_string(prompts.join("n-iter-1.md")).unwrap(),
            "prompt body"
        );
        let settings_json = prompts.join("n-iter-1.settings.json");
        let turn_end_ts = prompts.join("n-iter-1.turn-end.ts");
        let found = (settings_json.is_file(), turn_end_ts.is_file());
        if found.1 {
            // The extension body is pi's, byte for byte.
            assert_eq!(
                std::fs::read_to_string(&turn_end_ts).unwrap(),
                pdo_harness::pi_session::TURN_END_EXTENSION_TS
            );
        }
        if found.0 {
            assert_eq!(
                std::fs::read_to_string(&settings_json).unwrap(),
                pdo_harness::probes::STOP_HOOK_SETTINGS_JSON
            );
        }
        found
    }

    #[test]
    fn the_library_assistant_gets_its_primer_and_focus_hook_beside_each_other() {
        let dir = tempfile::tempdir().unwrap();
        let tmux = Recorder::default();
        let primer = dir.path().join(".libassist").join("assistant.md");
        spawn_libassist(
            &tmux,
            "pdo-libassist-shared",
            "primer",
            dir.path(),
            &primer,
            6172,
            &pdo_harness::registry::claude(),
            Some("true"),
        )
        .unwrap();
        assert_eq!(std::fs::read_to_string(&primer).unwrap(), "primer");
        assert_eq!(
            std::fs::read_to_string(primer.with_file_name("settings.json")).unwrap(),
            LIBASSIST_HOOK_SETTINGS_JSON
        );
        let (_, script) = tmux.created.lock().unwrap()[0].clone();
        assert!(
            script.contains("export PDO_RUN_ID='\\''__libassist__'\\''"),
            "{script}"
        );
    }
}
