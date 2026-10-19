//! The start primitive (ADR-0009, layer 2): the events and the session of one
//! node iteration's start, handed back to the caller to append then launch
//! (ADR-0038). No scheduling, no graph traversal.

use crate::application::sandbox::SandboxExec;
use crate::domain::event::{self, EventKind};
use pdo_pipeline::domain::pipeline;

use super::start_parts::{has_node_started_event, node_type_str, resolve_inputs, StartNodeTail};
use super::start_prep::{prepare_working_dir, resolve_harness, staging_set_env};
use super::{PrimitiveOutcome, StartNodeParams, StartNodeResult, StartNodeSpawn};

/// A start refused for `reason`: nothing to append, nothing to launch.
fn rejected(reason: String) -> StartNodeResult {
    StartNodeResult {
        outcome: PrimitiveOutcome::Rejected { reason },
        events: vec![],
        spawn: None,
    }
}

pub fn start_node(params: &StartNodeParams<'_>) -> StartNodeResult {
    let node = match params
        .pipeline
        .nodes
        .iter()
        .find(|n| n.id == params.node_id)
    {
        Some(n) => n,
        None => {
            return StartNodeResult {
                outcome: PrimitiveOutcome::Rejected {
                    reason: format!("node '{}' not found in pipeline", params.node_id),
                },
                events: vec![],
                spawn: None,
            }
        }
    };

    if has_node_started_event(params.run_state, params.node_id, params.iter) {
        return StartNodeResult {
            outcome: PrimitiveOutcome::AlreadyDone,
            events: vec![],
            spawn: None,
        };
    }

    let input_paths = resolve_inputs(params, node);

    // #653 / ADR-0060: where this NodeRun works, read off the document. Unlike
    // `node_spawn`, this path never re-poses a live iteration — the
    // `has_node_started_event` guard above already answered `AlreadyDone` for any
    // iteration that has started — so there is no frozen value to prefer here.
    let has_sub_worktree = node.is_isolated();

    let (working_dir, spawn_base_sha, node_provisioning) =
        match prepare_working_dir(params, node, has_sub_worktree) {
            Ok(prepared) => prepared,
            Err(reason) => return rejected(reason),
        };

    let canonical_path = pipeline::canonical_prompt_path(params.pipeline_path, params.node_id);
    let role_prompt = std::fs::read_to_string(&canonical_path).unwrap_or_default();

    // The manual start/retry endpoints produce the entry-node preamble, so they
    // must read this too — same gating and error posture as the live spawn site.
    let start_prompt_present = if params.pipeline.prompt_required {
        false
    } else {
        match crate::application::node::prompt::read_start_prompt_present(params.artifacts_dir) {
            Ok(present) => present,
            Err(e) => {
                tracing::warn!(
                    "entry-node input read failed (run {} node {} iter {}): {e}; \
                     assuming a prompt is present",
                    params.run_id,
                    params.node_id,
                    params.iter
                );
                true
            }
        }
    };

    // A sandboxed node's session execs into the container, where `localhost` is
    // the container — hence the same resolver as the manager preamble. Defensive:
    // no node preamble prints `AugmentContext.daemon_url` yet.
    let sandboxed = !params.run_state.sandbox.is_off();

    let aug_ctx = crate::application::node::prompt::AugmentContext {
        pipeline: params.pipeline,
        node,
        run_id: params.run_id,
        iter: params.iter,
        artifacts_dir: params.artifacts_dir,
        variables: params.resolved_vars,
        daemon_url: &pdo_sandbox::domain::container::daemon_url(params.daemon_port, sandboxed),
        foreach_context: None,
        source_worktree_dir: has_sub_worktree.then_some(working_dir.as_path()),
        // #654: the same section, from the other isolation. This site only
        // ever builds a preamble for a node that spawns a session, so the
        // two are exhaustive and mutually exclusive.
        shared_worktree_dir: (!has_sub_worktree).then_some(working_dir.as_path()),
        input_images: Vec::new(),
        input_files: Vec::new(),
        start_prompt_present,
        source_iters: crate::domain::input_resolution::resolved_source_iters(
            params.pipeline,
            params.run_state,
            params.node_id,
            params.iter,
        ),
        repeated_iters: crate::domain::input_resolution::resolved_repeated_iters(
            params.pipeline,
            params.run_state,
            params.node_id,
        ),
        absent_sources: crate::domain::input_resolution::absent_sources(
            params.pipeline,
            params.run_state,
            params.node_id,
        ),
        // Absolute snapshot paths: the sub-worktree does not inherit the snapshot
        // files.
        secondary_repos: crate::application::node::prompt::secondary_repo_contexts(
            params.repo_root,
            params.run_id,
            &params.run_state.target_repos,
        ),
        // Constant by construction here: the `Reusable` arm of
        // `ensure_sub_worktree` is unreachable on this path, so no interrupted-op
        // notice is ever routed.
        reused_sub_worktree: false,
        interrupted_git_ops: &[],
        // This path only runs for a never-started iteration, so no partial output
        // can survive; restart-with-artifacts is `node_spawn`'s concern.
        partial_outputs: &[],
        extension_preamble: params.extension.preamble.as_deref(),
        public_url: params
            .extension
            .env
            .get(pdo_kernel::public_url::PUBLIC_URL_ENV)
            .map(String::as_str),
    };

    let full_prompt = crate::application::node::prompt::build_full_prompt(&aug_ctx, &role_prompt);

    // A `script` node (ADR-0017) runs the author's bash instead of Claude: its I/O
    // arrives as env vars (a script can't read the prose preamble) and its declared
    // output dirs must exist before the body's `>` redirect runs. The file `bash`
    // executes must be the RAW body, never the augmented prompt.
    let is_script = node.node_type == pipeline::NodeType::Script;

    // An empty script body would `bash <empty>` → exit 0 → a silent no-op that
    // masquerades as success. `create_run` refuses it at launch; this guards the
    // manual force-spawn / restart door.
    if is_script && role_prompt.trim().is_empty() {
        return StartNodeResult {
            outcome: PrimitiveOutcome::Rejected {
                reason: format!("script node '{}' has an empty body", params.node_id),
            },
            events: vec![],
            spawn: None,
        };
    }

    let spawn_prompt: &str = if is_script {
        &role_prompt
    } else {
        &full_prompt
    };
    let script_env = if is_script {
        crate::application::node::prompt::precreate_output_dirs(&aug_ctx);
        crate::application::node::prompt::build_script_env(&aug_ctx)
    } else {
        Vec::new()
    };
    let (resolved_harness, harness_descriptor) = match resolve_harness(params, node, is_script) {
        Ok(resolved) => resolved,
        Err(reason) => return rejected(reason),
    };
    // Model + effort feed both the tail and the `NodeStarted` payload, which the
    // resume path reads back to re-pose `--effort`.
    // #708 / ADR-0063 §3: fill the resolved harness's staging set into this Run's
    // staging (once per Run, idempotent behind the marker) so a force-spawned node
    // carries the same staged home + env as a scheduled one. `None` home ⇒ skip (a
    // scheduler spawn or the manager fills it). A fill error degrades to no env
    // rather than failing this manual path — the scheduler spawn is the fail-fast.
    let sandbox_set_env = staging_set_env(params, sandboxed, harness_descriptor.as_ref());

    let resolved_model = resolved_harness.as_ref().and_then(|r| r.model.clone());
    let resolved_effort = resolved_harness.as_ref().and_then(|r| r.effort.clone());
    // Pin a session id only for a harness that can honour it (`claude`).
    let session_id: Option<String> = harness_descriptor
        .as_ref()
        .filter(|d| d.pins_session_id())
        .map(|_| uuid::Uuid::new_v4().to_string());
    let tail = if is_script {
        StartNodeTail::Script {
            timeout_secs: pdo_session::domain::launch::SCRIPT_TIMEOUT_SECS,
            env: script_env,
        }
    } else {
        StartNodeTail::Agent {
            harness: Box::new(
                harness_descriptor
                    .clone()
                    .expect("a non-script node resolved a harness descriptor"),
            ),
            model: resolved_model.clone(),
            effort: resolved_effort.clone(),
            session_id: session_id.clone(),
        }
    };

    let mut node_started = event::Event {
        id: None,
        run_id: params.run_id.to_string(),
        ts: pdo_kernel::clock::now_iso(),
        kind: EventKind::NodeStarted,
        node_id: Some(params.node_id.to_string()),
        iter: Some(params.iter),
        payload: Some(serde_json::json!({
            "prompt_preview": full_prompt.chars().take(500).collect::<String>(),
            "node_type": node_type_str(&node.node_type),
            "interactive": node.interactive,
            // #810/ADR-0007: FREEZE the « Orchestrator » toggle beside
            // `interactive`. The two together are the node's **genre** (kind),
            // the axis Stats › Performance filters on; reading the document at
            // stat time would reclassify history after an edit.
            "orchestrator": node.orchestrator,
            // #653/ADR-0060: FREEZE where this NodeRun works. Mirrors the
            // `spawn_node` payload — every later reader asks this event.
            "isolated_worktree": has_sub_worktree,
            "provisioning": node_provisioning,
            "input_paths": input_paths,
            // Launch-time model + effort, **resolved**. Mirrors the `spawn_node`
            // payload.
            "model": resolved_model.as_deref(),
            "effort": resolved_effort.as_deref(),
            // FROZEN so the resume path re-poses what was launched (ADR-0007).
            "harness": resolved_harness.as_ref().map(|r| r.harness.as_str()),
            // Read back by the sweep (transcript resolution) and the resume path.
            // `null` for a script node and for legacy rows.
            "session_id": session_id,
            // #503: the sub-worktree's base commit. Absent for a node with no
            // sub-worktree (a non-isolated agent/script), which never merges back.
            "base_sha": spawn_base_sha,
        })),
    };

    crate::application::node::spawn::freeze_extension_env(&mut node_started, &params.extension);
    let session_name =
        pdo_session::domain::names::node_session_name(params.run_id, params.node_id, params.iter);
    // **Do not add a spawn path that spawns before the caller appends
    // `NodeStarted`** (ADR-0038): the reaper's "absent ⇒ orphan" verdict is only
    // sound because no session can exist before its reservation.
    let sandbox = sandboxed.then(|| {
        SandboxExec::new(
            params.docker_cmd_override,
            params.run_id,
            &session_name,
            &working_dir,
            &sandbox_set_env,
        )
    });
    let spawn = StartNodeSpawn {
        session_name,
        prompt: spawn_prompt.to_string(),
        working_dir,
        run_id: params.run_id.to_string(),
        node_id: params.node_id.to_string(),
        iter: params.iter,
        daemon_port: params.daemon_port,
        tmux_cmd_override: params.tmux_cmd_override.map(str::to_string),
        tail,
        sandbox,
        // Gated by `!is_script`: a script node runs bash, so it carries no hook.
        inject_hook: params.inject_hook && !is_script,
        extension_env: crate::application::node::extension_launch::env_pairs(&params.extension),
    };

    // #588/#810 / ADR-0069 §1: NO `NodeAwaitingUser` at spawn, interactive or
    // not — the contract the reference primitive (`node_spawn::spawn_node`)
    // already honoured, now honoured here too. A fresh interactive node is
    // `running`, with the colour of a working node; the wait is declared by its
    // agent (`pdo wait-user`) or by a refused unreleased completion. Stats reads
    // the same log: a payload-less wait event would be an attente with no cause,
    // and the active duration would subtract time nobody waited.
    let events = vec![node_started];

    StartNodeResult {
        outcome: PrimitiveOutcome::Executed,
        events,
        spawn: Some(spawn),
    }
}
