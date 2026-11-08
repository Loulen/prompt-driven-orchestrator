//! The tails a session runs after its env exports: the harness agent, a
//! `script` node's bash body, and the library assistant's focus hook.

use std::path::Path;

use super::quote::sh_single_quote;

/// The settings injected via `--settings` to arm the library assistant's
/// `UserPromptSubmit` hook (#594, ADR-0051 §3).
///
/// One shared assistant serves every template, so which one is open cannot be
/// frozen into the primer at spawn time — it has to arrive with *each* message.
/// This hook fetches the daemon's current focus and prints it; on `exit 0` the
/// hook's stdout is appended to that turn's context, so the assistant re-learns
/// the open pipeline before it reads a single file, without depending on the
/// model remembering to ask.
///
/// Three properties of the command are load-bearing:
/// - **`exit 0`, always.** A `UserPromptSubmit` hook that exits `2` *rejects the
///   user's message and erases it*. `curl -sf … || true` swallows a daemon that
///   is down or a focus that is unset, and the trailing `exit 0` swallows
///   anything else — a silent no-injection is the correct failure mode, a
///   swallowed prompt is not.
/// - **No `matcher`.** Unlike `Stop`, this event has no matcher field.
/// - **`?format=text`.** The endpoint renders the focus as one plain sentence, so
///   the hook needs no `jq` on the session PATH.
///
/// `PDO_DAEMON_URL` is exported into the session by [`wrap_with_env`], so no env
/// has to be threaded into the JSON. `PDO_TOKEN`, when the session has one, goes
/// as `Authorization: Bearer`, like every CLI call (extension API 0.8). The 10 s timeout sits well under the 30 s
/// the hook contract allows.
///
/// **This only applies on a harness whose launch template has a `{settings}`
/// hole that takes the claude-format file** — the registry's `claude` does;
/// `opencode` has no hole and `pi`'s hole takes an extension, not this JSON
/// (`pdo_harness::probes::settings_hole_takes_claude_file`), and there the token is
/// dropped silently. On those the primer's equivalent instruction
/// (fetch the focus before acting) is the only mechanism: degraded, deliberately,
/// rather than absent (ADR-0051 §3).
pub const LIBASSIST_HOOK_SETTINGS_JSON: &str = r#"{"hooks":{"UserPromptSubmit":[{"hooks":[{"type":"command","command":"curl -sf ${PDO_TOKEN:+-H \"Authorization: Bearer $PDO_TOKEN\"} \"$PDO_DAEMON_URL/sessions/libassist/focus?format=text\" || true; exit 0","timeout":10}]}]}}"#;

/// Build the agent tail by rendering the harness descriptor's launch template
/// through [`pdo_harness::argv`] (#550, ADR-0045). The caller shell-quotes each
/// hole value here — where the shell semantics live — and the renderer only
/// substitutes and drops empty-hole tokens. For the `claude` descriptor with all
/// holes empty this reproduces the legacy `build_agent_tail` **byte for byte**
/// (the #550 gate, pinned by the goldens in `harness_argv`): a non-empty `model`
/// inserts `--model '<m>'`, `effort` inserts `--effort '<lvl>'` after it, a `Some`
/// `settings_path` inserts `--settings '<file>'` (#433), and a non-empty
/// `session_id` inserts `--session-id '<uuid>'` last (#473). `None` *or* an empty
/// string on any of them drops its token (the `Some("")` last-resort guard of
/// #347: a stray `""` never reaches the tail as `--model ''`).
pub(super) fn build_agent_tail(
    descriptor: &pdo_harness::registry::HarnessDescriptor,
    prompt_path: Option<&Path>,
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
    let holes = pdo_harness::argv::Holes {
        // No prompt file (a Session hébergée with nothing to read): the
        // `{prompt}` token drops, the agent opens on an empty input.
        prompt: prompt_path
            .map(|path| format!("\"$(cat {})\"", sh_single_quote(&path.to_string_lossy())))
            .unwrap_or_default(),
        model: quote_opt(model),
        effort: quote_opt(effort),
        settings: settings_path
            .map(|p| sh_single_quote(&p.to_string_lossy()))
            .unwrap_or_default(),
        session_id: quote_opt(session_id),
        // The launch template carries no `{resume}` hole.
        resume: String::new(),
    };
    pdo_harness::argv::render(&descriptor.launch, &holes)
}

/// The `{resume}` hole of a harness's resume template: `<resume_by_id>
/// '<id>'` re-enters the conversation `session_id` by its identity; with no
/// id (or a harness that cannot re-enter by one), the harness's blind verb,
/// which may be empty.
pub(super) fn resume_selector(
    descriptor: &pdo_harness::registry::HarnessDescriptor,
    session_id: Option<&str>,
) -> String {
    match session_id {
        Some(s) if !s.is_empty() && !descriptor.resume_by_id.is_empty() => {
            format!("{} {}", descriptor.resume_by_id, sh_single_quote(s))
        }
        _ => descriptor.resume_blind.clone(),
    }
}

/// The tail that re-enters the conversation `session_id` by its identity,
/// through the harness's resume template, with the `model` and `effort`
/// re-posed when the template has their holes.
pub(super) fn build_resume_tail(
    descriptor: &pdo_harness::registry::HarnessDescriptor,
    model: Option<&str>,
    effort: Option<&str>,
    session_id: &str,
) -> String {
    let quote_opt = |v: Option<&str>| {
        v.filter(|s| !s.is_empty())
            .map(sh_single_quote)
            .unwrap_or_default()
    };
    let holes = pdo_harness::argv::Holes {
        resume: resume_selector(descriptor, Some(session_id)),
        model: quote_opt(model),
        effort: quote_opt(effort),
        ..Default::default()
    };
    pdo_harness::argv::render(&descriptor.resume, &holes)
}

/// The `claude` CCR suppression, forced by the wrapper for `script` / `shell`
/// tails (AC #4): those run bash, not an agent, so they carry no descriptor — but
/// a hand-typed `claude` in a run shell still depends on it (CONTEXT.md §*Shell de
/// run*). An **agent** tail gets this from its descriptor's `env` instead.
pub(super) fn forced_ccr_env() -> Vec<(String, String)> {
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
/// **The `pdo complete` arm branches on exit code `4`** (#490, ADR-0035 §4). Don't
/// simplify it to `pdo complete || pdo fail`: on a terminal refusal (`4`) the
/// daemon has already recorded the failure, so the fallback would append a second
/// `NodeFailed` **and** an unguarded second `RunFailed` carrying a false reason
/// ("after script success", on a script whose output validation just failed). The
/// fallback therefore fires only on a code that is neither `0` (granted or legal
/// duplicate) nor `4` (already ruled).
///
/// **One `3` is legal too: `children_pending`** (ADR-0064). A script that
/// created child runs is a *parent node*, held by them: the daemon parked the
/// node (`AwaitingUser`, binding marker) and its watcher completes it once every
/// child is terminal. Failing the node there would lie, so the tail reads the
/// refusal's slug off `pdo complete`'s output (still echoed to the pane) and
/// leaves a held node alone. Any OTHER `3` keeps failing: the script cannot take
/// another turn (output refusals fail-fast to `4` for a script anyway).
pub fn build_script_tail(prompt_path: &Path, timeout_secs: u64) -> String {
    let body = sh_single_quote(&prompt_path.to_string_lossy());
    format!(
        "timeout {timeout_secs}s bash {body} ; ec=$? ; \
         if [ $ec -eq 0 ]; then co=$(pdo complete 2>&1) ; cc=$? ; printf '%s\\n' \"$co\" ; \
         case \"$cc:$co\" in 0:*|4:*|3:*children_pending*) ;; \
         *) pdo fail --reason \"pdo complete refused with exit $cc after script success\" ;; esac ; \
         elif [ $ec -eq 124 ]; then pdo fail --reason \"script timed out after {timeout_secs}s\" ; \
         else pdo fail --reason \"script exited $ec\" ; fi"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The hook is armed on the assistant's tail (#594) — the one thing that makes
    /// the per-message focus injection happen without the model's cooperation. It
    /// must name `UserPromptSubmit`, carry no `matcher` (this event has none), and
    /// end in `exit 0` (a `UserPromptSubmit` exiting 2 erases the user's message).
    #[test]
    fn libassist_hook_settings_are_a_non_blocking_user_prompt_submit_hook() {
        let v: serde_json::Value = serde_json::from_str(LIBASSIST_HOOK_SETTINGS_JSON)
            .expect("the settings file must be valid JSON — claude parses it at launch");
        let entry = &v["hooks"]["UserPromptSubmit"][0];
        assert!(
            entry["matcher"].is_null(),
            "UserPromptSubmit takes no matcher"
        );
        let cmd = entry["hooks"][0]["command"].as_str().unwrap();
        assert!(
            cmd.trim_end().ends_with("exit 0"),
            "the hook must never block a prompt: {cmd}"
        );
        assert!(
            cmd.contains("/sessions/libassist/focus"),
            "the hook fetches the daemon's focus: {cmd}"
        );
        assert!(
            cmd.contains(r#"${PDO_TOKEN:+-H "Authorization: Bearer $PDO_TOKEN"}"#),
            "the hook sends the session's token, like the CLI: {cmd}"
        );
    }
}
