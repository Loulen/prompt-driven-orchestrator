//! Re-attach an interrupted node's session in place (ADR-0049 §3).

use tracing::warn;

use crate::application::admission_count::count_global_live_sessions_excluding;
use crate::application::events::append_event;
use crate::application::node::launch::find_launch_effort;
use crate::application::node::launch::find_launch_harness;
use crate::application::node::launch::find_launch_node_started_payload;
use crate::application::node::launch::find_launch_session_id;
use crate::application::node::launch::find_node_type;
use crate::application::node::launch::node_run_isolation;
use crate::application::sandbox as run_sandbox;
use crate::application::settings::stored_autocomplete_turn_end;
use crate::application::settings::stored_session_cap;
use crate::domain::admission;
use crate::domain::{event, state};
use crate::Runs;
use pdo_session::domain::pane::ReattachOutcome;

/// Re-attach a node's dead-but-non-terminal session in place (ADR-0049 §3(a)).
///
/// Pure of the *response* shape: it performs the resume mechanics and returns a
/// [`ReattachOutcome`] the caller renders. The caller must have already ruled out a
/// live/terminal iteration — this is only reached for the latest, non-terminal,
/// non-pending one.
#[allow(clippy::too_many_arguments)]
pub async fn reattach_node_session(
    state: &Runs,
    events: &[event::Event],
    run_state: &state::RunState,
    repo_root: &std::path::Path,
    run_id: &str,
    node_id: &str,
    iter: i64,
    session_name: &str,
) -> ReattachOutcome {
    let node_type = find_node_type(run_state, node_id).unwrap_or("agent");
    // #248 / ADR-0017: never resume a `script` node via `claude --continue`.
    if node_type == "script" {
        return ReattachOutcome::ScriptNode;
    }

    // #653: the FROZEN isolation of this iteration — re-attaching has to open the
    // directory the session actually runs in, not the one the document now names.
    let working_dir = pdo_session::domain::names::working_dir_for_node(
        repo_root,
        run_id,
        node_id,
        iter,
        node_run_isolation(events, run_state, node_id, iter),
    );
    if !working_dir.exists() {
        return ReattachOutcome::WorkingDirMissing;
    }

    // A resume restores the model but loses the effort level, so re-pose the level
    // the node was LAUNCHED with, and resume by the pinned session id.
    let launch_effort = find_launch_effort(events, node_id, iter);
    let launch_session_id = find_launch_session_id(events, node_id, iter);
    // Re-pose the harness FROZEN at spawn, resolved against the embedded floor merged
    // with the disk descriptor tier. A name that WAS frozen but no longer resolves
    // REFUSES — never a silent `claude` relaunch, which would run a different agent in
    // this node's worktree. A row with NO frozen harness keeps the `claude` floor.
    let launch_harness = find_launch_harness(events, node_id, iter);
    let harness_home_root = run_sandbox::sandbox_home_roots(state)
        .map(|(home, _)| home)
        .unwrap_or_default();
    let descriptor = match launch_harness.as_deref() {
        Some(name) => {
            match pdo_harness::adapters::descriptor_file::load(&harness_home_root).resolve(name) {
                Some(d) => d,
                None => {
                    return ReattachOutcome::FrozenHarnessGone {
                        harness: name.to_string(),
                    }
                }
            }
        }
        None => pdo_harness::registry::claude(),
    };
    // A harness with no resume tail cannot be re-attached (ADR-0045).
    if !descriptor.can_resume() {
        return ReattachOutcome::CannotResume;
    }
    // Re-resolve turn-end auto-completion FRESH.
    let inject_hook = stored_autocomplete_turn_end(state).await;

    // A resurrection IS a (re)spawn: pass the admission gate and leave a
    // `NodeStarted` trace, holding the lock across check-and-reserve then dropping it
    // before the slower tmux relaunch, exactly as `spawn_node` orders it.
    let admission_guard = state.admission_lock.lock().await;
    let cap = crate::adapters::env::configured_cap_with(stored_session_cap(state).await);
    let live = count_global_live_sessions_excluding(
        state,
        Some(admission::SlotExclusion {
            run_id,
            node_id,
            iter,
        }),
    )
    .await;
    if !admission::can_admit(live, cap) {
        drop(admission_guard);
        return ReattachOutcome::CapReached { live, cap };
    }
    let resurrection_started = event::Event {
        id: None,
        run_id: run_id.to_string(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event::EventKind::NodeStarted,
        node_id: Some(node_id.to_string()),
        iter: Some(iter),
        payload: find_launch_node_started_payload(events, node_id, iter),
    };
    if let Err(e) = append_event(state, &resurrection_started).await {
        // The guard also enforces run-liveness on this append; a refusal means the
        // event log has no room for this session, so do not launch it.
        drop(admission_guard);
        return ReattachOutcome::TraceRefused {
            error: e.to_string(),
        };
    }
    drop(admission_guard);

    // #708: the frozen harness's staging set is already on disk (filled at the first
    // spawn, marker present) — `stage_harness_set` returns its env idempotently, which
    // the re-entered container session carries. Best-effort: never block a recovery.
    let set_env = if run_state.sandbox.is_off() {
        Vec::new()
    } else {
        run_sandbox::stage_harness_set(state, run_state, &descriptor.name, repo_root)
            .unwrap_or_else(|e| {
                warn!("reattach run {run_id} node {node_id}: staging `{}` set failed (best-effort): {e:#}", descriptor.name);
                Vec::new()
            })
    };
    // A resumed sandboxed session re-enters its container.
    let sandbox = (!run_state.sandbox.is_off()).then(|| {
        crate::application::sandbox::SandboxExec::new(
            state.docker_cmd_override.as_deref(),
            run_id,
            session_name,
            &working_dir,
            &set_env,
        )
    });

    // API 0.8: the resumed agent re-carries what the extensions give its launch.
    let extension = crate::application::node::extension_launch::contribution(
        state,
        run_state,
        node_id,
        iter,
        pdo_extension_api::NodeKind::Agent,
    )
    .await;
    if let Err(e) = pdo_session::application::launch::resume(
        state.tmux(),
        session_name,
        &working_dir,
        run_id,
        node_id,
        iter,
        state.port,
        &descriptor,
        launch_effort.as_deref(),
        launch_session_id.as_deref(),
        state.tmux_cmd_override.as_deref(),
        sandbox
            .as_ref()
            .map(crate::application::sandbox::SandboxExec::wrap)
            .as_ref(),
        inject_hook,
        &crate::application::node::extension_launch::env_pairs(&extension),
    ) {
        return ReattachOutcome::ResumeFailed {
            error: e.to_string(),
        };
    }
    ReattachOutcome::Resumed
}
