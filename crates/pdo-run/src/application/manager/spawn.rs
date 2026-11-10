//! The Run's Pipeline Manager session: its spawn — the harness of the Run, or
//! the instance's pinned profile — and the pin's resolution.

use pdo_harness::agent_choice::ResolvedCombo;
use pdo_kernel::Actor;
use tracing::{error, info, warn};

use crate::application::events::{emit_run_event, reload_run_state};
use crate::application::node::prompt::{build_manager_prompt, with_first_message, RunNameHint};
use crate::application::paths::effective_repo_root;
use crate::application::sandbox::{sandbox_home_roots, stage_harness_set, SandboxExec};
use crate::domain::event::EventKind;
use crate::Runs;

/// Spawn the Run's Pipeline Manager session, started by `started_by`: the
/// Actor whose gesture starts it, or `None` when it starts with the Run, which
/// then reads as started by the Run's creator (CORE-61). The extensions' launch
/// hooks are asked for it, with that Actor: their variables go into its
/// environment, their text at the top of its prompt. Once up, it is announced
/// started. `first_message`, when a gesture starts it to hand it one (the
/// Review's send, CORE-239), ends its launch prompt.
///
/// **Its only reservation is `RunStarted`** (ADR-0038): the orphan sweep resolves a
/// `pdo-mgr-<run>` session against the *run*, not a node, so a manager spawned for a
/// run whose `RunStarted` never landed is an orphan the sweep will legitimately kill.
/// Keep every spawn site downstream of a successful `RunStarted` append.
pub async fn spawn_manager_session(
    state: &Runs,
    run_id: &str,
    worktree_dir: &std::path::Path,
    name_hint: RunNameHint,
    sandboxed: bool,
    started_by: Option<Actor>,
    first_message: Option<&str>,
) {
    // The URL must be reachable from the side this manager runs on. A sandboxed
    // manager execs into the Run's container, where `localhost` is the container, so a
    // host URL makes every `curl` of its own preamble hit nothing and the manager
    // report the daemon dead.
    let daemon_url = pdo_sandbox::domain::container::daemon_url(state.port, sandboxed);

    let static_prompt = std::fs::read_to_string(
        state
            .repo_root
            .join("prompts")
            .join("builtin")
            .join("manager.md"),
    )
    .unwrap_or_default();

    let full_prompt = build_manager_prompt(run_id, &daemon_url, &static_prompt, name_hint);
    let full_prompt = match first_message {
        Some(message) => with_first_message(&full_prompt, message),
        None => full_prompt,
    };

    let session_name = pdo_session::domain::names::manager_session_name(run_id);
    // The manager follows the harness OF THE RUN — `Run → instance → floor`, no node
    // tier and deliberately no model/effort — so "this Run runs on X" holds with no
    // exception … unless a stored `manager_profile` pin says otherwise (applied
    // just below). An unknown name falls back to `claude` with a warning rather than
    // failing the Run: the first NODE spawn is where an unknown harness fails fast
    // (ADR-0037), and the manager is a best-effort assist that must not wedge the Run.
    let run_state = reload_run_state(state, run_id)
        .await
        .map(|(_, state)| state);
    let started_by =
        started_by.or_else(|| run_state.as_ref().and_then(|run| run.created_by.clone()));
    let extension = match &run_state {
        Some(run) => {
            crate::application::node::extension_launch::manager_contribution(
                state,
                run,
                started_by.clone(),
            )
            .await
        }
        None => Default::default(),
    };
    let full_prompt = extension.above(full_prompt);
    let config = state.settings.defaults().await;
    let profiles = state.settings.agent_profiles().await;
    let manager_agent = pdo_harness::agent_choice::resolve_infra(
        run_state.as_ref().and_then(|run| run.agent_choice.as_ref()),
        run_state.as_ref().and_then(|run| run.harness.as_deref()),
        config.as_ref().and_then(|cfg| cfg.agent_choice.as_ref()),
        config
            .as_ref()
            .and_then(|cfg| cfg.default_harness.as_deref()),
        &profiles,
        pdo_harness::agent_choice::DEFAULT_PROFILE_ID,
    );
    // The stored `manager_profile` pin, applied AFTER « Follow the Run » so a pin
    // fully wins (design §2.2): the profile's harness · model · effort replace the
    // Run-tier combo entirely — a pin that only replaced the floor would almost
    // never fire, since the Run tier always resolves. A pin naming a profile that
    // was deleted (or renamed) AFTER registration warns and falls back to « Follow
    // the Run » — loud, and the stored value is never rewritten (#432 tombstone).
    let stored_pin = config
        .as_ref()
        .and_then(|cfg| cfg.manager_profile.as_deref())
        .map(str::trim)
        .filter(|p| !p.is_empty());
    let found = match stored_pin {
        Some(pin) => state.settings.profile_named(pin).await,
        None => Ok(None),
    };
    let (manager_combo, pin_warning) =
        apply_manager_profile_pin(stored_pin, found, manager_agent.combo.clone());
    if let Some(warning) = &pin_warning {
        warn!("manager for run {run_id}: {warning}");
    }
    let manager_harness_name = manager_combo.harness.clone();
    // Resolve against the DISK TIER, through the same registry the node spawns use,
    // so "this Run runs on X" holds even when X is a user-declared harness.
    let harness_home_root = sandbox_home_roots(state)
        .map(|(home, _)| home)
        .unwrap_or_default();
    let manager_harness = pdo_harness::adapters::descriptor_file::load(&harness_home_root)
        .resolve(&manager_harness_name)
        .unwrap_or_else(|| {
            warn!(
                "manager for run {run_id}: unknown harness '{manager_harness_name}' — \
                 launching the manager on the claude floor (the first node spawn fails fast)"
            );
            pdo_harness::registry::claude()
        });
    // #708: the manager runs the Run's harness inside the container, so it fills that
    // harness's staging set (once per Run — the first node may not have spawned yet)
    // and carries its env on the exec. Best-effort: a fill error must not wedge the
    // best-effort manager assist, so it degrades to no env (the first NODE spawn is
    // the authoritative fail-fast on an unfillable set).
    let manager_set_env: Vec<(String, String)> = match (sandboxed, run_state.as_ref()) {
        (true, Some(rs)) => {
            let trusted = effective_repo_root(state, rs);
            stage_harness_set(state, rs, &manager_harness_name, &trusted)
                .unwrap_or_else(|e| {
                    warn!("manager for run {run_id}: staging `{manager_harness_name}` set failed (best-effort): {e:#}");
                    Vec::new()
                })
        }
        _ => Vec::new(),
    };
    // The manager runs inside the Run's container too when sandboxed.
    let sandbox = sandboxed.then(|| {
        SandboxExec::new(
            state.docker_cmd_override.as_deref(),
            run_id,
            &session_name,
            worktree_dir,
            &manager_set_env,
        )
    });
    // Reservation before spawn (ADR-0038, extended to every path): the durable
    // event lands FIRST, the tmux spawn after. The projection ignores this kind —
    // the wire's `has_manager` stays an observed tmux fact, so a spawn that fails
    // after the reservation reads `false`, not `true`.
    emit_run_event(state, run_id, EventKind::ManagerStarted, None).await;
    if let Err(e) = pdo_session::application::launch::spawn(
        state.tmux(),
        &session_name,
        &full_prompt,
        worktree_dir,
        run_id,
        "__manager__",
        0,
        state.port,
        state.tmux_cmd_override.as_deref(),
        // No pinned session id: the manager owns no `NodeStarted`, the sweep never
        // probes it and it is never resumed. Resolving nodes by their own id is
        // precisely what stops the manager's transcript, which shares this cwd, from
        // being read as a node's.
        pdo_session::domain::launch::SessionTail::Agent {
            harness: &manager_harness,
            model: manager_combo.model.as_deref(),
            effort: manager_combo.effort.as_deref(),
            session_id: None,
        },
        sandbox.as_ref().map(SandboxExec::wrap).as_ref(),
        // The manager is an infra session, not a pipeline node — it never calls
        // `pdo complete`, so it gets no turn-end `Stop` hook.
        false,
        // What the extensions give the session of whoever starts it (API 0.13).
        &crate::application::node::extension_launch::env_pairs(&extension),
    ) {
        error!("failed to spawn manager tmux session: {e}");
    } else {
        info!("Spawned manager session: {session_name}");
        state.sessions().started_named(&session_name, started_by);
    }
}

/// Apply the stored `manager_profile` pin to the manager's « Follow the Run »
/// combo (design §2.2 « Manager profile »). `stored` is the trimmable
/// instance-config value; `found` is the settings' case-insensitive lookup of
/// that profile (its combo) (errors stringified so the helper stays pure and unit-testable).
///
/// Returns the combo to launch with — the named profile's harness · model ·
/// effort when the pin resolves, otherwise the « Follow the Run » combo
/// untouched — plus a warning for a pin that could not fire (profile missing or
/// unreadable): the fallback is loud, and the stored value is never rewritten.
pub fn apply_manager_profile_pin(
    stored: Option<&str>,
    found: Result<Option<ResolvedCombo>, String>,
    follow_run: ResolvedCombo,
) -> (ResolvedCombo, Option<String>) {
    let Some(pin) = stored else {
        return (follow_run, None);
    };
    match found {
        Ok(Some(combo)) => (combo, None),
        Ok(None) => (
            follow_run,
            Some(format!(
                "manager profile `{pin}` names no agent profile — falling back to \
                 « Follow the Run »"
            )),
        ),
        Err(e) => (
            follow_run,
            Some(format!(
                "cannot read agent profile `{pin}` ({e}) — falling back to \
                 « Follow the Run »"
            )),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_pin_leaves_the_follow_the_run_combo_untouched() {
        let follow_run = pdo_harness::agent_choice::ResolvedCombo {
            harness: "codex".into(),
            model: Some("gpt-5".into()),
            effort: Some("high".into()),
        };
        let (combo, warning) = apply_manager_profile_pin(None, Ok(None), follow_run.clone());
        assert_eq!(combo, follow_run);
        assert_eq!(warning, None);
    }

    #[test]
    fn a_resolved_pin_fully_replaces_the_follow_the_run_combo() {
        // Strong pin (design §2.2, decision #3): harness, model AND effort come
        // from the profile — the Run tier never leaks through.
        let follow_run = pdo_harness::agent_choice::ResolvedCombo {
            harness: "codex".into(),
            model: Some("gpt-5".into()),
            effort: Some("high".into()),
        };
        // The profile named `CheapManager`, as the settings' lookup answers it.
        let profile = ResolvedCombo {
            harness: "claude".into(),
            model: Some("haiku".into()),
            effort: None,
        };
        let (combo, warning) =
            apply_manager_profile_pin(Some("CheapManager"), Ok(Some(profile)), follow_run);
        assert_eq!(combo.harness, "claude");
        assert_eq!(combo.model.as_deref(), Some("haiku"));
        assert_eq!(combo.effort, None);
        assert_eq!(warning, None);
    }

    #[test]
    fn a_pin_to_a_missing_profile_falls_back_with_a_warning_naming_it() {
        // #432 tombstone discipline: the profile was deleted after the pin was
        // stored — the spawn falls back to « Follow the Run », loudly, and the
        // stored value is never rewritten (nothing here persists anything).
        let follow_run = pdo_harness::agent_choice::ResolvedCombo {
            harness: "codex".into(),
            model: None,
            effort: None,
        };
        let (combo, warning) =
            apply_manager_profile_pin(Some("ghost"), Ok(None), follow_run.clone());
        assert_eq!(combo, follow_run);
        let warning = warning.expect("a missing pin must warn");
        assert!(warning.contains("`ghost`"), "got: {warning}");
        assert!(warning.contains("Follow the Run"), "got: {warning}");
    }

    #[test]
    fn a_pin_that_cannot_be_looked_up_falls_back_with_a_warning() {
        let follow_run = pdo_harness::agent_choice::ResolvedCombo {
            harness: "claude".into(),
            model: None,
            effort: None,
        };
        let (combo, warning) =
            apply_manager_profile_pin(Some("p"), Err("db locked".into()), follow_run.clone());
        assert_eq!(combo, follow_run);
        let warning = warning.expect("a failed lookup must warn");
        assert!(warning.contains("db locked"), "got: {warning}");
        assert!(warning.contains("Follow the Run"), "got: {warning}");
    }
}
