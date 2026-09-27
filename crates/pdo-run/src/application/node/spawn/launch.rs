//! The launch of a reserved NodeRun: its tmux session (the agent's or the
//! script's tail, in the Run's container when sandboxed), and the benign loser
//! of a duplicate told apart from a genuine failure.

use pdo_session::domain::launch::{SessionTail, SCRIPT_TIMEOUT_SECS};
use pdo_session::domain::names::node_session_name;
use tracing::warn;

use super::interrupts::*;
use super::warnings::*;
use super::*;
use crate::application::sandbox::SandboxExec;
use crate::application::settings::stored_autocomplete_turn_end;
use pdo_pipeline::domain::pipeline;

use super::Plan;

#[allow(clippy::too_many_arguments)]
pub(super) async fn launch(
    deps: SpawnDeps<'_>,
    spawn_ctx: &SpawnContext<'_>,
    node: &pipeline::NodeDef,
    iter: i64,
    plan: &Plan,
    harness_descriptor: Option<&pdo_harness::registry::HarnessDescriptor>,
    set_env: &[(String, String)],
    full_prompt: String,
    script_env: Vec<(String, String)>,
) -> Result<(), SpawnOutcome> {
    let run_id = spawn_ctx.run_id;
    let Plan {
        role_prompt,
        run_sandboxed,
        wt,
        resolved_harness,
        resolved_model,
        resolved_effort,
        session_id,
        ..
    } = plan;
    let run_sandboxed = *run_sandboxed;
    let working_dir = &wt.working_dir;
    let orphan_to_reap = &wt.orphan_to_reap;
    let session_name = node_session_name(run_id, &node.id, iter);
    let is_script = node.node_type == pipeline::NodeType::Script;
    let tail = if is_script {
        SessionTail::Script {
            timeout_secs: SCRIPT_TIMEOUT_SECS,
            env: &script_env,
        }
    } else {
        // Resolved above the panic span so the `NodeStarted` payload could record
        // the same values the flags carry (#347/#424/#473/#550). A non-`script`
        // node always has a resolved descriptor (the fail-fast returned otherwise).
        let descriptor = harness_descriptor
            .as_ref()
            .expect("a non-script node resolved a harness descriptor");
        SessionTail::Agent {
            harness: descriptor,
            model: resolved_model.as_deref(),
            effort: resolved_effort.as_deref(),
            session_id: session_id.as_deref(),
        }
    };
    // A script node executes the RAW bash body (`role_prompt`), never the
    // augmented prompt — the preamble is prose an agent reads, not runnable bash.
    let spawn_prompt: &str = if is_script { role_prompt } else { &full_prompt };
    // #407: when sandboxed, the tail runs in `docker exec … pdo-sbx-<run>`.
    let sandbox = run_sandboxed.then(|| {
        SandboxExec::new(
            deps.docker_cmd_override,
            run_id,
            &session_name,
            working_dir,
            set_env,
        )
    });
    // #433 / ADR-0043: arm the turn-end `Stop` hook only when the operator has
    // opted into turn-end auto-completion (the SAME setting as the daemon sweep,
    // read FRESH — parity with model/effort) and never for a `script` node (bash
    // tail, no `claude`). Resolved here, at the spawn edge, so a `PUT /settings`
    // takes effect on the next node with no daemon restart.
    let inject_hook = !is_script && stored_autocomplete_turn_end(deps.runs).await;
    // #613/ADR-0051 (AC #7): the setting is on but this harness has no end-of-turn
    // substrate ⇒ it will not auto-complete. Say the absence once rather than let
    // the setting be a silent no-op. `claude` (and a `script` node, which never
    // arms the hook) say nothing.
    if inject_hook {
        if let Some(r) = &resolved_harness {
            warn_turn_end_unsupported_once(run_id, &r.harness);
        }
    }
    if let Err(e) = pdo_session::application::launch::spawn(
        deps.tmux,
        &session_name,
        spawn_prompt,
        working_dir,
        run_id,
        &node.id,
        iter,
        deps.port,
        deps.tmux_cmd_override,
        tail,
        sandbox.as_ref().map(SandboxExec::wrap).as_ref(),
        inject_hook,
    ) {
        // #785 / ADR-0050 §2: `duplicate session` while the session of this very
        // `(run, node, iter)` is alive means a WINNER already runs this iteration —
        // this spawn is the benign loser, never a failure. No `NodeInterrupted`, no
        // `RunInterrupted`: parking the run `spawn_aborted` over a healthy winner
        // is exactly what the ADR forbids (and what made "Reopen" fail ten times
        // in a row on the pipeline `grill-with-docs`). The redundant `NodeStarted`
        // this loser appended is harmless to the projection: `upsert_iteration`
        // keeps ONE iteration row per `(node, iter)`, so it re-stamps the winner's
        // live row rather than opening a phantom one. Nothing is reaped either —
        // the sub-worktree, if any, is the winner's.
        let msg = format!("{e:#}");
        if msg.contains("duplicate session") && deps.tmux.session_exists(&session_name) {
            let reason = format!(
                "node {} iter {iter}: tmux reported duplicate session {session_name} \
                 and it is alive — losing spawn is a benign no-op (ADR-0050 §2)",
                node.id
            );
            warn!("Run {run_id}: {reason}");
            return Err(SpawnOutcome::Refused { reason });
        }
        // #508: the tmux spawn itself failed *after* `NodeStarted` is durable. It
        // used to be swallowed (`error!` + fall through), leaving the node
        // projected `Running` with NO session — the liveness sweep then rewrote it
        // `Failed` ~30s later with a false `session_died` cause, and `restart_node`
        // answered a lying `200 {spawned}`. Fail loud right here instead: append
        // `NodeFailed` (legal now that the iteration is `Running`) → reap only what
        // this spawn created → `RunFailed`, then return `Failed`. This lands BEFORE
        // the `Spawned` return below. (ADR-0037 §1/§3.)
        let reason = format!(
            "failed to spawn tmux session {session_name} for node {}: {e}",
            node.id
        );
        interrupt_spawn_after_start(
            deps,
            spawn_ctx.repo_root,
            run_id,
            &node.id,
            iter,
            orphan_to_reap.as_ref(),
            &reason,
        )
        .await;
        return Err(SpawnOutcome::Failed { reason });
    }

    // #588 / ADR-0069: NO `NodeAwaitingUser` at spawn, interactive or not. A
    // fresh node has the colour of a working node; the wait is declared by its
    Ok(())
}
