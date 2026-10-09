//! `StartNode`: force-spawn a node out of dependency order — the canvas's
//! Start button and the Manager's `start_node` share [`force_spawn_node`].

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use tracing::{error, info, warn};

use crate::application::admission_count::count_global_live_sessions;
use crate::application::events::append_event;
use crate::application::events::load_events;
use crate::application::node::primitives as node_primitives;
use crate::application::operations::on_node;
use crate::application::paths::effective_repo_root;
use crate::application::paths::resolve_pipeline_path;
use crate::application::paths::run_scoped_pipeline_path;
use crate::application::reopen::embed_reopen_for_targeted_command;
use crate::application::sandbox as run_sandbox;
use crate::application::scheduler::frontmatter::resolve_run_variables;
use crate::application::settings::stored_autocomplete_turn_end;
use crate::application::settings::stored_default_harness;
use crate::application::settings::stored_default_harness_models;
use crate::application::settings::stored_default_model;
use crate::application::settings::stored_session_cap;
use crate::domain::{admission, transition_guard};
use crate::domain::{event, projection, state};
use crate::Runs;
use pdo_kernel::Actor;
use pdo_pipeline::domain::pipeline;
use pdo_workspace::adapters::git::worktrees::worktree_dir_for_run;

/// `StartNode`: the operator force-spawns a node out of dependency order (the
/// canvas's Start button).
#[derive(Debug, Clone)]
pub struct StartNode {
    pub run_id: String,
    pub node_id: String,
}

impl Runs {
    /// REST entry point for the UI **Start** button: force-spawn `node_id` out of
    /// dependency order. A thin wrapper — the logic and guards live in the shared
    /// [`force_spawn_node`], which the `start_node` `/commands` kind also calls, so the
    /// two consumers cannot drift.
    pub async fn start_node(self: &Arc<Self>, actor: &Actor, command: StartNode) -> Response {
        let StartNode { run_id, node_id } = command;
        if let Err(denied) = self
            .authorize(actor, &on_node("start", &run_id, &node_id))
            .await
        {
            return denied.into_response();
        }
        let state = self;

        force_spawn_node(state, &run_id, &node_id, None).await
    }
}

/// Writes each per-port inline-content override to an artifact under the node's
/// iteration dir and returns the port-keyed map `start_node` consumes. This is what
/// lets a `start_node`/`skip_node` supply a **dummy input** without the upstream
/// having produced: the resolver reads these paths in place of the edge-resolved
/// ones. A per-port write failure is skipped with a `warn!` (ADR-0001) rather than
/// failing the whole spawn.
pub fn materialize_override_inputs(
    artifacts_dir: &std::path::Path,
    node_id: &str,
    iter: i64,
    overrides: &HashMap<String, String>,
) -> HashMap<String, std::path::PathBuf> {
    let mut out = HashMap::new();
    for (port, content) in overrides {
        let dir = artifacts_dir
            .join(node_id)
            .join(format!("iter-{iter}"))
            .join("__override__")
            .join(port);
        if let Err(e) = std::fs::create_dir_all(&dir) {
            warn!("failed to create override dir for {node_id}.{port}: {e}");
            continue;
        }
        let path = dir.join("output.md");
        match std::fs::write(&path, content) {
            Ok(()) => {
                out.insert(port.clone(), path);
            }
            Err(e) => warn!("failed to write override artifact for {node_id}.{port}: {e}"),
        }
    }
    out
}

/// Force-spawn a node now, without waiting for its upstream producers. The single
/// implementation behind both consumers: the UI **Start** button and the Pipeline
/// Manager `start_node` `/commands` kind, so the guards live in exactly one place.
///
/// Guards run **before** any side effect, in order:
///  - run must exist (`404`) and the node must not already be `Running` (`409`);
///  - the derived `NodeStarted` is pre-validated through [`transition_guard`], which
///    is what makes the refusal a `409` here instead of an `error!` swallowed by the
///    append loop below. `RunStatus::is_live()` is deliberately *not* the predicate:
///    it admits `Paused`, which the append backstop rejects, and that gap would
///    orphan a session on a paused run. Pre-validating the actual event with the same
///    guard the backstop uses keeps the two in lockstep.
///  - `admission_lock` is held across the spawn + append so the cap check is an
///    atomic check-and-reserve, as `spawn_node` does. Force-spawn fails fast rather
///    than queueing to `waiting` — "start now" must not silently defer.
pub async fn force_spawn_node(
    state: &Arc<Runs>,
    run_id: &str,
    node_id: &str,
    overrides: Option<&HashMap<String, String>>,
) -> Response {
    let events = match load_events(state, run_id).await {
        Ok(e) => e,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
        }
    };
    let run_state = match projection::project(&events) {
        Some(s) => s,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "run not found" })),
            )
                .into_response();
        }
    };

    // A Start on a terminal (or incident-parked) Run re-opens it atomically
    // (ADR-0049). Must run before the guards below, so they see a live Run.
    let run_state = match embed_reopen_for_targeted_command(state, run_id, run_state).await {
        Ok(s) => s,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response()
        }
    };

    if let Some(ns) = run_state.nodes.get(node_id) {
        if ns.status == state::NodeStatus::Running {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({ "error": "node is already running" })),
            )
                .into_response();
        }
    }

    let iter = run_state
        .nodes
        .get(node_id)
        .map(|ns| ns.iter + 1)
        .unwrap_or(1);

    // This path drives `node_primitives::start_node`, not `spawn_node`, so it needs
    // the sandbox precondition stated AGAIN — the same `docker exec` on a container
    // that does not exist yet would die the same way. 409 rather than a silent defer:
    // "start now" must not queue.
    if let Some(reason) = run_state.sandbox_spawn_block() {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "error": reason })),
        )
            .into_response();
    }

    // Pre-validate against the SAME transition guard that backstops `append_event`,
    // BEFORE any tmux session exists, so a refused start leaves no orphan session.
    let started_probe = event::Event {
        id: None,
        run_id: run_id.to_string(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event::EventKind::NodeStarted,
        node_id: Some(node_id.to_string()),
        iter: Some(iter),
        payload: None,
    };
    match transition_guard::validate_transition(Some(&run_state), &started_probe) {
        transition_guard::Verdict::Allow => {}
        transition_guard::Verdict::NoOp { reason } => {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({ "error": reason })),
            )
                .into_response();
        }
        transition_guard::Verdict::Reject { reason } => {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({ "error": reason.to_string() })),
            )
                .into_response();
        }
    }

    let repo_root = effective_repo_root(state, &run_state);
    let pipeline_path = {
        let run_scoped = run_scoped_pipeline_path(&repo_root, run_id);
        if run_scoped.exists() {
            run_scoped
        } else {
            resolve_pipeline_path(&repo_root, &run_state.pipeline_name)
        }
    };
    let Ok(yaml) = std::fs::read_to_string(&pipeline_path) else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "cannot read pipeline" })),
        )
            .into_response();
    };
    let Ok(parse_result) = pipeline::parse_pipeline(&yaml) else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "cannot parse pipeline" })),
        )
            .into_response();
    };
    let pipeline_def = parse_result.pipeline;

    let worktree_dir = worktree_dir_for_run(&repo_root, run_id);
    let artifacts_dir = worktree_dir.join(".pdo").join("artifacts");
    let resolved_vars = resolve_run_variables(&pipeline_def, &events);

    // Materialize any operator-supplied per-port overrides into artifacts, so the
    // node runs on the dummy input(s) even though its upstream never produced.
    let override_paths = overrides
        .map(|ov| materialize_override_inputs(&artifacts_dir, node_id, iter, ov))
        .filter(|m| !m.is_empty());

    // Resolve against the DISK TIER, so a manual Start reaches a user-declared
    // harness exactly like the scheduler's `spawn_node` does. Bound before `params`
    // so the borrow outlives `start_node`. Absent HOME degrades to the embedded floor.
    let harness_home_root = run_sandbox::sandbox_home_roots(state)
        .map(|(home, _)| home)
        .unwrap_or_default();
    let harness_registry = pdo_harness::adapters::descriptor_file::load(&harness_home_root);

    let extension_contribution = match pipeline_def.nodes.iter().find(|n| n.id == node_id) {
        Some(node) => {
            use crate::application::node::extension_launch as ext;
            let kind = ext::node_kind(&node.node_type);
            ext::contribution(state, &run_state, node_id, iter, kind).await
        }
        None => Default::default(),
    };
    let params = node_primitives::StartNodeParams {
        run_id,
        node_id,
        iter,
        overrides: override_paths,
        pipeline: &pipeline_def,
        run_state: &run_state,
        artifacts_dir: &artifacts_dir,
        worktree_dir: &worktree_dir,
        repo_root: &repo_root,
        pipeline_path: &pipeline_path,
        resolved_vars: &resolved_vars,
        daemon_port: state.port,
        tmux_cmd_override: state.tmux_cmd_override.as_deref(),
        docker_cmd_override: state.docker_cmd_override.as_deref(),
        // Resolve fresh so a force-spawn honours the instance default too: wiring
        // only the scheduler seam leaves this path at the account default, a silent
        // bug visible only on a manual force-spawn.
        default_model: stored_default_model(state).await,
        default_harness: stored_default_harness(state).await,
        default_harness_models: stored_default_harness_models(state).await,
        // The Projet tier, resolved fresh so a manual force-spawn honours a Projet
        // setting attached mid-Run. A DB error degrades to a transparent tier.
        project_harness: crate::application::node::project_tiers::project_harness_for_path(
            state,
            &repo_root.to_string_lossy(),
        )
        .await
        .ok()
        .flatten(),
        // Same reasoning as `default_model`: resolve fresh, or a force-spawned node
        // silently misses the `Stop` hook on this seam alone.
        inject_hook: stored_autocomplete_turn_end(state).await,
        harness_registry: Some(&harness_registry),
        // #708: a sandboxed force-spawn fills the resolved harness's staging set at
        // this spawn, exactly like the scheduler. `harness_home_root` is already the
        // host `$HOME` (or the embedded-floor fallback); pass it only when sandboxed.
        sandbox_home_root: (!run_state.sandbox.is_off()).then_some(harness_home_root.as_path()),
        extension: extension_contribution,
    };

    // Hold the lock from the count until the reservation event is appended — exactly
    // as `spawn_node` does — or concurrent force-spawns both claim the same free slot
    // and overshoot the cap toward the tmux-server collapse point.
    let _admission_guard = state.admission_lock.lock().await;
    let cap = crate::adapters::env::configured_cap_with(stored_session_cap(state).await);
    let live = count_global_live_sessions(state).await;
    if !admission::can_admit(live, cap) {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": "session cap reached",
                "live_sessions": live,
                "cap": cap,
            })),
        )
            .into_response();
    }

    let result = node_primitives::start_node(&params);

    for ev in &result.events {
        if let Err(e) = append_event(state, ev).await {
            error!("failed to append event: {e}");
        }
    }

    // Reserve, THEN spawn (ADR-0038). Spawning first leaves a live session with no
    // `NodeStarted` in the log, and the orphan sweep kills exactly that. Appending
    // first also means a rejected append leaves no orphan session; the residual risk
    // (append landed, spawn failed) is what `spawn_node` has always carried.
    if let Some(spawn) = &result.spawn {
        match spawn.execute(state.tmux()) {
            // CORE-61: the session is up, started by the Run's creator.
            Ok(()) => state
                .sessions()
                .started_named(&spawn.session_name, run_state.created_by.clone()),
            Err(e) => error!("failed to spawn tmux session: {e}"),
        }
    }

    match result.outcome {
        node_primitives::PrimitiveOutcome::Executed => {
            info!("force_spawn_node: started {node_id} iter {iter} in run {run_id}");
            (
                StatusCode::OK,
                Json(serde_json::json!({ "ok": true, "iter": iter })),
            )
                .into_response()
        }
        node_primitives::PrimitiveOutcome::AlreadyDone => (
            StatusCode::OK,
            Json(serde_json::json!({ "ok": true, "already_running": true })),
        )
            .into_response(),
        node_primitives::PrimitiveOutcome::Rejected { reason } => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": reason })),
        )
            .into_response(),
    }
}
