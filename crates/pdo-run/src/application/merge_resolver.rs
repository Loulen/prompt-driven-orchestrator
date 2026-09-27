//! The merge resolver session (ADR-0006): its spawn on a conflicting
//! merge-back, and its `done`.

use std::sync::Arc;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use tracing::{error, info, warn};

use crate::application::events::append_event;
use crate::application::events::reload_run_state;
use crate::application::events::run_is_forgotten;
use crate::application::paths::effective_repo_root;
use crate::application::sandbox as run_sandbox;
use crate::application::scheduler::advance as run_advance;
use crate::domain::completion_refusal;
use crate::domain::{event, projection, state};
use crate::Runs;
use pdo_workspace::adapters::git::worktrees::{validate_merge_resolution, worktree_dir_for_run};

/// Spawn the automatic merge resolver. **DEAD in production** since ADR-0006:
/// reached only from `DeliveryOutcome::ConflictPendingResolution`, which is built only
/// under `keep_conflict == true`, which no production caller passes.
///
/// Returns a typed refusal rather than a `Response` (ADR-0035 §6): both outcomes —
/// spawned, and spawn-failed — describe a completion that was **not** granted.
pub async fn spawn_merge_resolver(
    state: &Runs,
    run_id: &str,
    conflicting_node_id: &str,
    conflicting_iter: i64,
    worktree_dir: &std::path::Path,
) -> completion_refusal::CompletionRefusal {
    let prompt = load_merge_resolver_prompt(&state.repo_root);
    let session_name =
        pdo_session::domain::names::node_session_name(run_id, MERGE_RESOLVER_NODE_ID, 1);

    // The merge resolver runs inside the Run's container when sandboxed AND on the
    // harness OF THE RUN. Project both from the SAME reload.
    let (sandbox_mode, run_harness, run_agent_choice) = reload_run_state(state, run_id)
        .await
        .map(|(_, s)| (s.sandbox, s.harness, s.agent_choice))
        .unwrap_or_default();

    let resolver_started = event::Event {
        id: None,
        run_id: run_id.to_string(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event::EventKind::MergeResolverStarted,
        node_id: None,
        iter: None,
        payload: Some(serde_json::json!({
            "conflicting_node_id": conflicting_node_id,
            "iter": conflicting_iter,
            "session_name": session_name,
        })),
    };
    let _ = append_event(state, &resolver_started).await;

    // DEAD in production (ADR-0006); the resolver would follow the Run's harness, so
    // an empty set env is acceptable here — it never runs a real pi session.
    let resolver_set_env: Vec<(String, String)> = Vec::new();
    let sandbox = (!sandbox_mode.is_off()).then(|| {
        crate::application::sandbox::SandboxExec::new(
            state.docker_cmd_override.as_deref(),
            run_id,
            &session_name,
            worktree_dir,
            &resolver_set_env,
        )
    });
    // Follows the harness OF THE RUN, exactly like the manager: `Run → instance →
    // floor`, no node tier, no model/effort. An unknown name falls back to `claude`
    // with a warning.
    let config = state.settings.defaults().await;
    let profiles = state.settings.agent_profiles().await;
    let resolver_agent = pdo_harness::agent_choice::resolve_infra(
        run_agent_choice.as_ref(),
        run_harness.as_deref(),
        config.as_ref().and_then(|cfg| cfg.agent_choice.as_ref()),
        config
            .as_ref()
            .and_then(|cfg| cfg.default_harness.as_deref()),
        &profiles,
        pdo_harness::agent_choice::DEFAULT_PROFILE_ID,
    );
    let resolver_harness_name = resolver_agent.combo.harness.clone();
    // Resolve against the DISK TIER, like the manager.
    let resolver_home_root = run_sandbox::sandbox_home_roots(state)
        .map(|(home, _)| home)
        .unwrap_or_default();
    let resolver_harness = pdo_harness::adapters::descriptor_file::load(&resolver_home_root)
        .resolve(&resolver_harness_name)
        .unwrap_or_else(|| {
            warn!(
                "merge resolver for run {run_id}: unknown harness '{resolver_harness_name}' — \
                 launching on the claude floor"
            );
            pdo_harness::registry::claude()
        });
    if let Err(e) = pdo_session::application::launch::spawn(
        state.tmux(),
        &session_name,
        &prompt,
        worktree_dir,
        run_id,
        MERGE_RESOLVER_NODE_ID,
        1,
        state.port,
        state.tmux_cmd_override.as_deref(),
        // No pinned session id: `__merge_resolver__` owns no `NodeStarted` and is
        // neither probed nor resumed. NOT to be confused with a `merge` NODE, which is
        // a regular NodeDef routed through `spawn_node` and DOES carry a pinned id.
        pdo_session::domain::launch::SessionTail::Agent {
            harness: &resolver_harness,
            model: resolver_agent.combo.model.as_deref(),
            effort: resolver_agent.combo.effort.as_deref(),
            session_id: None,
        },
        sandbox
            .as_ref()
            .map(crate::application::sandbox::SandboxExec::wrap)
            .as_ref(),
        // An infra session: its `…/done` is handled by a distinct branch, not
        // `complete_node_iteration`, so it gets no turn-end hook.
        false,
    ) {
        error!("failed to spawn merge resolver tmux session: {e}");
        let fail_event = event::Event {
            id: None,
            run_id: run_id.to_string(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event::EventKind::MergeResolverFailed,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({
                "reason": format!("failed to spawn resolver session: {e}")
            })),
        };
        let _ = append_event(state, &fail_event).await;
        // ADR-0049: park `AwaitingUser`, never `RunFailed`.
        let run_interrupted = event::Event {
            id: None,
            run_id: run_id.to_string(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event::EventKind::RunInterrupted,
            node_id: None,
            iter: None,
            payload: Some(projection::interrupt_payload(
                "merge_resolver_spawn_failed",
                "merge_resolver_spawn_failed: merge resolver spawn failed",
            )),
        };
        let _ = append_event(state, &run_interrupted).await;

        return completion_refusal::CompletionRefusal::MergeResolverFailed {
            reason: format!("failed to spawn resolver session: {e}"),
        };
    }

    info!("Spawned merge resolver for run {run_id} (conflict on {conflicting_node_id})");
    completion_refusal::CompletionRefusal::MergeResolverSpawned {
        node_id: conflicting_node_id.to_string(),
    }
}

pub async fn handle_merge_resolver_done(
    state: &Runs,
    run_id: &str,
    worktree_dir: &std::path::Path,
    pre_run_state: &state::RunState,
) -> Response {
    let problems = match validate_merge_resolution(worktree_dir) {
        Ok(p) => p,
        Err(e) => {
            error!("merge resolution validation error: {e}");
            vec![format!("validation error: {e}")]
        }
    };

    if !problems.is_empty() {
        let reason = problems.join("; ");
        let fail_event = event::Event {
            id: None,
            run_id: run_id.to_string(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event::EventKind::MergeResolverFailed,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({ "reason": reason })),
        };
        let _ = append_event(state, &fail_event).await;

        // ADR-0049: park `AwaitingUser`, never `RunFailed`.
        let run_interrupted = event::Event {
            id: None,
            run_id: run_id.to_string(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event::EventKind::RunInterrupted,
            node_id: None,
            iter: None,
            payload: Some(projection::interrupt_payload(
                "merge_resolution_failed",
                format!("merge_resolution_failed: {reason}"),
            )),
        };
        let _ = append_event(state, &run_interrupted).await;

        warn!("Merge resolver failed for run {run_id}: {reason}");
        // `409` with the slug, never a `200`: the resolution was refused (ADR-0035).
        return crate::adapters::http::verdicts::refusal_response(
            &completion_refusal::CompletionRefusal::MergeResolutionFailed { reason },
        );
    }

    let completed_event = event::Event {
        id: None,
        run_id: run_id.to_string(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event::EventKind::MergeResolverCompleted,
        node_id: None,
        iter: None,
        payload: None,
    };
    let _ = append_event(state, &completed_event).await;

    info!("Merge resolver completed for run {run_id}");

    if let Some(ref mr) = pre_run_state.merge_resolver {
        let original_node_id = &mr.conflicting_node_id;
        let original_iter = mr.iter;

        let node_completed = event::Event {
            id: None,
            run_id: run_id.to_string(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event::EventKind::NodeCompleted,
            node_id: Some(original_node_id.clone()),
            iter: Some(original_iter),
            payload: None,
        };
        if let Err(e) = append_event(state, &node_completed).await {
            error!("failed to append node_completed for resolved node: {e}");
        }

        // `completed_node_id` is the ORIGINAL conflicting node, NOT the
        // `__merge_resolver__` route param.
        run_advance::complete_node(
            state,
            run_id,
            original_node_id,
            run_advance::CompletionOrder::CompletionFirst,
            false,
        )
        .await;
    }

    (StatusCode::OK, "ok").into_response()
}

/// The `__merge_resolver__` half of `POST …/nodes/:id/done` (ADR-0035 §6).
///
/// Order matters: the forgotten-run tombstone check (ADR-0024) must come **before**
/// any side effect, then load + project.
pub async fn merge_resolver_done(state: &Arc<Runs>, run_id: &str) -> Response {
    match run_is_forgotten(state, run_id).await {
        Ok(true) => {
            return crate::adapters::http::verdicts::refusal_response(
                &completion_refusal::CompletionRefusal::RunForgotten {
                    run_id: run_id.to_string(),
                },
            );
        }
        Ok(false) => {}
        Err(e) => {
            return crate::adapters::http::verdicts::refusal_response(
                &completion_refusal::CompletionRefusal::Internal {
                    error: format!("forgotten-run check failed: {e}"),
                },
            );
        }
    }

    let Some((_, pre_run_state)) = reload_run_state(state, run_id).await else {
        return crate::adapters::http::verdicts::refusal_response(
            &completion_refusal::CompletionRefusal::RunNotFound,
        );
    };
    let repo_root = effective_repo_root(state, &pre_run_state);
    let worktree_dir = worktree_dir_for_run(&repo_root, run_id);
    handle_merge_resolver_done(state, run_id, &worktree_dir, &pre_run_state).await
}

pub const MERGE_RESOLVER_NODE_ID: &str = "__merge_resolver__";

pub const FALLBACK_MERGE_RESOLVER_PROMPT: &str = "\
You are the Merge Resolver. A git merge conflict occurred. \
Resolve all conflicts, remove all conflict markers, and commit the merge.";

pub fn load_merge_resolver_prompt(repo_root: &std::path::Path) -> String {
    let path = repo_root.join("prompts/builtin/merge-resolver.md");
    std::fs::read_to_string(&path).unwrap_or_else(|_| FALLBACK_MERGE_RESOLVER_PROMPT.to_string())
}

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;
    use crate::domain::scheduler;
    use crate::domain::scheduler::dispatcher as scheduler_dispatcher;
    use pdo_pipeline::domain::pipeline;
    use pretty_assertions::assert_eq;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    /// A Run's events, its state and their projection, as these tests read them.
    mod event_log {
        pub use crate::domain::event::*;
        pub use crate::domain::projection::*;
        pub use crate::domain::state::*;
    }

    #[test]
    fn builtin_merge_resolver_prompt_loads_from_file() {
        let prompt = load_merge_resolver_prompt(std::path::Path::new("."));
        assert!(
            prompt.contains("Merge Resolver"),
            "prompt should contain 'Merge Resolver', got first 100 chars: {}",
            &prompt[..prompt.len().min(100)]
        );
    }

    #[test]
    fn builtin_merge_resolver_prompt_falls_back_when_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let prompt = load_merge_resolver_prompt(tmp.path());
        assert_eq!(prompt, FALLBACK_MERGE_RESOLVER_PROMPT);
    }

    #[test]
    fn merge_resolver_node_id_is_dunder() {
        assert_eq!(MERGE_RESOLVER_NODE_ID, "__merge_resolver__");
    }
}
