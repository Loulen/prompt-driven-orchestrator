//! `POST /runs/{id}/commands` — the Pipeline Manager's command surface (#236).
//!
//! Carved out of `lib.rs` as a pure move: this module holds the HTTP handler,
//! the post-command re-evaluation it drives, and the two pipeline helpers only
//! that re-evaluation uses. Nothing here changed in the move — the wire
//! contract of the thirteen accepted `kind`s is identical, byte for byte.
//!
//! Layer 3 in ADR-0009 terms, and the one the Pipeline Manager drives. Its
//! sibling surface is the per-node route family
//! `POST /runs/{id}/nodes/{node_id}/{start,stop,retry}` (the canvas buttons),
//! which lives in `lib.rs` still.

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Json, Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use tracing::{error, info, warn};

// Everything below is private to the crate root. Rust makes a root item visible
// in the root AND every descendant module, so carving this file out needed no
// widening: the only visibility change in the whole move is `pub(crate)` on
// `run_command` itself, which the router (a root item, i.e. an ANCESTOR) has to
// import by name.
use crate::node_spawn::{spawn_node, SpawnContext, SpawnDeps, SpawnOutcome};
use crate::scheduler_interpreter::{ActionOutcome, SpawnDedup};
use crate::worktree_ops::worktree_dir_for_run;
use crate::{
    append_event, check_output_validation_with_retry, cleanup_run, completion_head_gate,
    create_run_core, effective_repo_root, event_log, force_spawn_node, load_events, loop_region,
    mark_sandbox_prep_ready, pipeline, reload_run_state, resolve_completed_frontmatter,
    resolve_pipeline_path, resolve_run_pipeline_path, resolve_run_variables,
    resolve_source_frontmatter, run_advance, run_is_forgotten, run_scoped_pipeline_path,
    sandbox_run, scheduler, scheduler_interpreter, tmux_session_manager, transition_guard,
    AppState, CreateRunRequest,
};

/// The wire shape of a command. `pub(crate)` only because `run_command` is —
/// axum's extractor puts this type in the handler's public signature, and the
/// router lives in the crate root (an ancestor). Its fields stay private: no
/// caller outside this module builds or reads one.
#[derive(Deserialize)]
pub(crate) struct RunCommandRequest {
    kind: String,
    #[serde(default)]
    node_id: Option<String>,
    #[serde(default)]
    iter: Option<i64>,
    #[serde(default)]
    additional_iter: Option<i64>,
    /// Identifies the loop region a `bump_region` / `end_region` command targets
    /// (ADR-0011 / #152 — the Pipeline Manager routes a region by id).
    #[serde(default)]
    region_id: Option<String>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

/// A command from the Pipeline Manager, already validated (#236). The only way
/// to build one is [`parse_run_command`], so every field below is checked
/// before an arm ever sees it: no arm re-validates presence, applies a default,
/// or re-inspects a path.
///
/// Thirteen accepted `kind`s, twelve variants — `bump_region` and `end_region`
/// share one, because they share their whole I/O tail.
#[derive(Debug, Clone, PartialEq, Eq)]
enum RunCommand {
    MarkNodeDone {
        node_id: String,
        iter: i64,
    },
    ExtendCycle {
        node_id: String,
        additional_iter: i64,
    },
    /// `bump_region` / `end_region`. They differ only in payload and required
    /// fields — everything after that (region lookup in the snapshot, the
    /// `CommandIssued` append, the Halt lift, the re-evaluation) is shared. The
    /// difference lives in [`RegionAction`], which is why `end_region` cannot
    /// carry an `additional_iter` IN THE TYPE.
    Region {
        region_id: String,
        action: RegionAction,
    },
    PauseRun,
    ResumeRun,
    KillNode {
        node_id: String,
        iter: i64,
    },
    RestartNode {
        node_id: String,
        iter: i64,
    },
    /// `req.iter` is dropped at parse time — `force_spawn_node` derives the
    /// iteration itself (#204), and letting the manager pin one would fight
    /// that derivation.
    StartNode {
        node_id: String,
    },
    /// `path` stays a `String`, not a `PathBuf`: it is echoed verbatim into the
    /// `CommandIssued` payload and the `info!`. Already proven relative and
    /// free of `..` components.
    InjectArtifact {
        path: String,
        content: String,
    },
    /// A missing `name` is LEGAL and means the empty string. Do not "fix" it
    /// into a rejection — renaming to `""` clears the name, and the projection
    /// reads it that way.
    RenameRun {
        name: String,
    },
    CleanupRun,
    RetryAll,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RegionAction {
    Bump { additional_iter: i64 },
    End,
}

/// Rejected at parse time. Every one of the fourteen current rejection sites
/// answers `400` + `Json({"error": …})`, so the status is not carried here —
/// add it the day one of them stops being a 400.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CommandParseError(String);

impl IntoResponse for CommandParseError {
    fn into_response(self) -> Response {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": self.0 })),
        )
            .into_response()
    }
}

impl RunCommand {
    /// The wire `kind` this command came from. Load-bearing, not cosmetic: the
    /// region arm interpolates it into a `warn!` and an `info!` that both sit
    /// AFTER an `.await` (so the request is long gone), and every
    /// `CommandIssued` payload carries it as `"command"`.
    fn kind_str(&self) -> &'static str {
        match self {
            RunCommand::MarkNodeDone { .. } => "mark_node_done",
            RunCommand::ExtendCycle { .. } => "extend_cycle",
            RunCommand::Region {
                action: RegionAction::Bump { .. },
                ..
            } => "bump_region",
            RunCommand::Region {
                action: RegionAction::End,
                ..
            } => "end_region",
            RunCommand::PauseRun => "pause_run",
            RunCommand::ResumeRun => "resume_run",
            RunCommand::KillNode { .. } => "kill_node",
            RunCommand::RestartNode { .. } => "restart_node",
            RunCommand::StartNode { .. } => "start_node",
            RunCommand::InjectArtifact { .. } => "inject_artifact",
            RunCommand::RenameRun { .. } => "rename_run",
            RunCommand::CleanupRun => "cleanup_run",
            RunCommand::RetryAll => "retry_all",
        }
    }
}

/// Turn the wire shape into a validated command. **Pure** — no DB, no
/// filesystem, no clock — which is the whole point: every one of the fourteen
/// rejections below used to be reachable only through a live daemon.
///
/// Two orderings are behaviour, not style, and both are pinned by tests:
/// `bump_region` reports a missing `region_id` BEFORE a missing
/// `additional_iter`, and `inject_artifact` reports a missing `content` BEFORE
/// it looks at the path. Swap either and a malformed request changes its
/// answer.
///
/// The `kind` match comes first, so an unknown `kind` with missing fields still
/// answers `"unknown command"` rather than a field complaint.
///
/// Deliberately NOT derived from the wire with `#[serde(tag = "kind")]`: that
/// would turn these fourteen messages into axum's `422` + serde prose in
/// text/plain.
fn parse_run_command(req: RunCommandRequest) -> Result<RunCommand, CommandParseError> {
    fn required(
        value: Option<String>,
        field: &str,
        kind: &str,
    ) -> Result<String, CommandParseError> {
        value.ok_or_else(|| CommandParseError(format!("{field} required for {kind}")))
    }
    fn positive_iter(value: Option<i64>, kind: &str) -> Result<i64, CommandParseError> {
        let n = value
            .ok_or_else(|| CommandParseError(format!("additional_iter required for {kind}")))?;
        if n <= 0 {
            return Err(CommandParseError(
                "additional_iter must be positive".to_string(),
            ));
        }
        Ok(n)
    }

    match req.kind.as_str() {
        "mark_node_done" => Ok(RunCommand::MarkNodeDone {
            node_id: required(req.node_id, "node_id", "mark_node_done")?,
            // Omitted `iter` means 1 — three arms share this default.
            iter: req.iter.unwrap_or(1),
        }),
        "extend_cycle" => {
            let node_id = required(req.node_id, "node_id", "extend_cycle")?;
            Ok(RunCommand::ExtendCycle {
                node_id,
                additional_iter: positive_iter(req.additional_iter, "extend_cycle")?,
            })
        }
        kind @ ("bump_region" | "end_region") => {
            // `region_id` first, for BOTH kinds: `{"kind":"bump_region"}` alone
            // must complain about the region, not the count.
            let region_id = required(req.region_id, "region_id", kind)?;
            let action = if kind == "bump_region" {
                RegionAction::Bump {
                    additional_iter: positive_iter(req.additional_iter, "bump_region")?,
                }
            } else {
                // A stray `additional_iter` on `end_region` is accepted and
                // dropped. Rejecting it would be a regression.
                RegionAction::End
            };
            Ok(RunCommand::Region { region_id, action })
        }
        "pause_run" => Ok(RunCommand::PauseRun),
        "resume_run" => Ok(RunCommand::ResumeRun),
        "kill_node" => Ok(RunCommand::KillNode {
            node_id: required(req.node_id, "node_id", "kill_node")?,
            iter: req.iter.unwrap_or(1),
        }),
        "restart_node" => Ok(RunCommand::RestartNode {
            node_id: required(req.node_id, "node_id", "restart_node")?,
            iter: req.iter.unwrap_or(1),
        }),
        "start_node" => Ok(RunCommand::StartNode {
            node_id: required(req.node_id, "node_id", "start_node")?,
        }),
        "inject_artifact" => {
            let path = required(req.path, "path", "inject_artifact")?;
            // BEFORE the traversal check: a request with a hostile path and no
            // content answers "content required", not "path traversal".
            let content = required(req.content, "content", "inject_artifact")?;
            let requested = std::path::Path::new(&path);
            if requested.is_absolute()
                || requested
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                return Err(CommandParseError("path traversal not allowed".to_string()));
            }
            Ok(RunCommand::InjectArtifact { path, content })
        }
        "rename_run" => Ok(RunCommand::RenameRun {
            name: req.name.unwrap_or_default(),
        }),
        "cleanup_run" => Ok(RunCommand::CleanupRun),
        "retry_all" => Ok(RunCommand::RetryAll),
        other => Err(CommandParseError(format!("unknown command: {other}"))),
    }
}

pub(crate) async fn run_command(
    State(state): State<Arc<AppState>>,
    AxumPath(run_id): AxumPath<String>,
    Json(req): Json<RunCommandRequest>,
) -> Response {
    // #328 / ADR-0024: a forgotten run accepts no commands — reject before any
    // arm can append (extend_cycle appends CommandIssued before its own
    // existence check) or trigger side effects.
    match run_is_forgotten(&state.db, &run_id).await {
        Ok(true) => {
            return (
                StatusCode::GONE,
                Json(serde_json::json!({ "error": format!("run {run_id} has been forgotten") })),
            )
                .into_response();
        }
        Ok(false) => {}
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
        }
    }

    // #236: all presence / default / path-shape validation is pure and lives in
    // `parse_run_command` — AFTER the 410 gate, which must stay first
    // (ADR-0024 / #328). A malformed command against a forgotten run answers
    // 410 today, not 400, and that precedence belongs to ADR-0024.
    let cmd = match parse_run_command(req) {
        Ok(cmd) => cmd,
        Err(e) => return e.into_response(),
    };

    dispatch(state, run_id, cmd).await
}

/// Run one validated command and answer for it.
///
/// **Returns an `axum::Response`, deliberately — not a semantic
/// `CommandOutcome` mapped to HTTP by a central mapper.** That alternative is
/// provably lossy: this surface emits twenty-two distinct (status,
/// content-type, body-shape) triplets, including seven `404 text/plain` "run
/// not found" against one `404` in JSON, a `409 text/plain` against eight in
/// JSON, five success shapes no verdict enum expresses, and the `201 {run_id}`
/// of `create_run_core` forwarded verbatim by `retry_all` — which the frontend
/// reads to navigate to the retried Run. A single mapper must pick one
/// content-type per verdict, and each pick rewrites the other half. Returning
/// the `Response` is lossless by construction: the wire contract cannot see
/// this refactor. See the #236 addendum to ADR-0009.
///
/// `dispatch` is a DRIVER, not a leaf primitive: it reaches twenty-three root
/// items across five subsystems (sqlite, tmux, docker, worktrees, Run
/// creation). The house idiom for a driver is the whole `AppState`
/// (`run_advance`, `scheduler_interpreter`); dependency injection à la
/// `SpawnDeps` is reserved for leaves (`node_spawn`), and a bundle here would
/// carry all of `AppState` while buying no testability.
///
/// Both parameters are taken **by value**, which is what lets every arm move
/// across byte-for-byte: `Arc` because `force_spawn_node` wants `&Arc<AppState>`
/// and the arms already say `&state`; `run_id` because every arm builds events
/// that own it. The handler holds no use for either afterwards, so the moves
/// cost nothing.
async fn dispatch(state: Arc<AppState>, run_id: String, cmd: RunCommand) -> Response {
    // Read before the match consumes `cmd`: the region arm logs its wire kind
    // after two `.await`s, and `&'static str` costs nothing to carry.
    let kind_str = cmd.kind_str();

    match cmd {
        RunCommand::MarkNodeDone { node_id, iter } => {
            let events = match load_events(&state.db, &run_id).await {
                Ok(e) => e,
                Err(e) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}"))
                        .into_response();
                }
            };
            let run_state = event_log::project(&events);

            // Transition guard (#212, #354): validate the completion against the
            // projected state BEFORE any side effect (output validation, append,
            // downstream dispatch). Shared head — same pure decision as
            // `node_done` and `node_skip`. `run_state.as_ref()` may be `None`
            // (unstarted run); the guard maps `None -> Allow`, forwarded verbatim.
            if let Some(resp) = completion_head_gate(
                run_advance::evaluate_completion_head(run_state.as_ref(), &run_id, &node_id, iter),
                "mark_node_done",
                &run_id,
                &node_id,
                iter,
            ) {
                return resp;
            }

            let empty_run_state = event_log::RunState::new(run_id.clone(), String::new());
            let rs_ref = run_state.as_ref().unwrap_or(&empty_run_state);
            let repo_root = effective_repo_root(&state, rs_ref);
            let pipeline_name = run_state
                .as_ref()
                .map(|rs| rs.pipeline_name.as_str())
                .unwrap_or("");
            let pipeline_path = resolve_run_pipeline_path(&repo_root, &run_id, pipeline_name);
            let worktree_dir = worktree_dir_for_run(&repo_root, &run_id);
            let artifacts_dir = worktree_dir.join(".pdo").join("artifacts");
            if let Some(resp) = check_output_validation_with_retry(
                &state,
                &pipeline_path,
                &node_id,
                iter,
                &artifacts_dir,
                &run_id,
                rs_ref,
            )
            .await
            {
                return resp;
            }

            let event = event_log::Event {
                id: None,
                run_id: run_id.clone(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::NodeCompleted,
                node_id: Some(node_id.clone()),
                iter: Some(iter),
                payload: Some(serde_json::json!({ "source": "mark_node_done" })),
            };

            if let Err(e) = append_event(&state, &event).await {
                return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
            }

            let cmd_event = event_log::Event {
                id: None,
                run_id: run_id.clone(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::CommandIssued,
                node_id: Some(node_id.clone()),
                iter: Some(iter),
                payload: Some(serde_json::json!({
                    "command": "mark_node_done",
                    "node_id": node_id,
                    "iter": iter,
                })),
            };
            if let Err(e) = append_event(&state, &cmd_event).await {
                error!("failed to append mark_node_done command event: {e}");
            }

            // Shared post-`NodeCompleted` tail (#275), `SweepFirst`: advance the
            // run + re-drive throttled waiters, THEN fire this node's edges (the
            // interactive node is already gone), then the single completion gate.
            // flag = true: the just-finished node was interactive, so the run can
            // still project `AwaitingUser` at the gate and must still complete —
            // unlike the other sites (flag = false, #235). The `NodeCompleted`
            // (with its `source` payload) + `CommandIssued` appends above are the
            // caller's head.
            run_advance::complete_node(
                &state,
                &run_id,
                &node_id,
                run_advance::CompletionOrder::SweepFirst,
                true,
            )
            .await;

            info!("mark_node_done: node {node_id} in run {run_id}");
            (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
        }
        RunCommand::ExtendCycle {
            node_id,
            additional_iter,
        } => {
            // ADR-0025 / #327: validate the target against the run's pipeline
            // SNAPSHOT before any event is appended — a rejected command must
            // leave no trace in the log. Snapshot-first (`resolve_run_pipeline_path`)
            // so a library edit after launch can't affect an in-flight run.
            let events = match load_events(&state.db, &run_id).await {
                Ok(e) => e,
                Err(e) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}"))
                        .into_response();
                }
            };
            let run_state = match event_log::project(&events) {
                Some(s) => s,
                None => {
                    return (StatusCode::NOT_FOUND, "run not found").into_response();
                }
            };
            let repo_root = effective_repo_root(&state, &run_state);
            let pipeline_path =
                resolve_run_pipeline_path(&repo_root, &run_id, &run_state.pipeline_name);
            if let Some(pipeline) = std::fs::read_to_string(&pipeline_path)
                .ok()
                .and_then(|yaml| pipeline::parse_pipeline(&yaml).ok())
                .map(|p| p.pipeline)
            {
                if !pipeline.nodes.iter().any(|n| n.id == node_id) {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(serde_json::json!({
                            "error": format!("node '{node_id}' not found in pipeline")
                        })),
                    )
                        .into_response();
                }
                if let Some(region) = loop_region::bounded_region_for_member(&pipeline, &node_id) {
                    return (
                        StatusCode::CONFLICT,
                        Json(serde_json::json!({
                            "error": format!(
                                "node '{node_id}' is a member of loop region '{}'; \
                                 use bump_region with region_id '{}'",
                                region.id, region.id
                            )
                        })),
                    )
                        .into_response();
                }
            } else {
                // An unreadable/unparsable snapshot can't be validated against;
                // stay permissive (legacy behavior) rather than reject blind.
                warn!("extend_cycle: pipeline snapshot unreadable for run {run_id}; skipping target validation");
            }

            let cmd_event = event_log::Event {
                id: None,
                run_id: run_id.clone(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::CommandIssued,
                node_id: Some(node_id.clone()),
                iter: None,
                payload: Some(serde_json::json!({
                    "command": "extend_cycle",
                    "node_id": node_id,
                    "additional_iter": additional_iter,
                })),
            };
            if let Err(e) = append_event(&state, &cmd_event).await {
                return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
            }

            if run_state.status == event_log::RunStatus::Halted
                || run_state.status == event_log::RunStatus::Failed
            {
                let resume_event = event_log::Event {
                    id: None,
                    run_id: run_id.clone(),
                    ts: event_log::now_iso(),
                    kind: event_log::EventKind::CommandIssued,
                    node_id: None,
                    iter: None,
                    payload: Some(serde_json::json!({ "command": "resume_run" })),
                };
                if let Err(e) = append_event(&state, &resume_event).await {
                    error!("failed to append resume_run: {e}");
                }
            }

            // Re-evaluate outgoing edges with the extended cycle
            let summary = re_evaluate_after_command(&state, &run_id).await;

            info!("extend_cycle: node {node_id} +{additional_iter} in run {run_id}");
            (StatusCode::OK, Json(summary.into_response_body())).into_response()
        }
        // The Pipeline Manager routes a loop region BY ID (ADR-0011 / #152):
        // `bump_region` runs N more iterations; `end_region` fires its
        // completion. Both append a control-flow `CommandIssued` event and then
        // continue the run (lift an exhausted-unrouted Halt and re-evaluate),
        // so a stalled region is unstuck without restarting the daemon.
        RunCommand::Region { region_id, action } => {
            let payload = match &action {
                RegionAction::Bump { additional_iter } => serde_json::json!({
                    "command": "bump_region",
                    "region_id": region_id,
                    "additional_iter": additional_iter,
                }),
                RegionAction::End => serde_json::json!({
                    "command": "end_region",
                    "region_id": region_id,
                }),
            };

            // ADR-0025 / #327: validate the region against the run's pipeline
            // SNAPSHOT before any event is appended — an unknown region_id must
            // leave no trace in the log.
            let events = match load_events(&state.db, &run_id).await {
                Ok(e) => e,
                Err(e) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}"))
                        .into_response();
                }
            };
            let run_state = match event_log::project(&events) {
                Some(s) => s,
                None => {
                    return (StatusCode::NOT_FOUND, "run not found").into_response();
                }
            };
            let repo_root = effective_repo_root(&state, &run_state);
            let pipeline_path =
                resolve_run_pipeline_path(&repo_root, &run_id, &run_state.pipeline_name);
            if let Some(pipeline) = std::fs::read_to_string(&pipeline_path)
                .ok()
                .and_then(|yaml| pipeline::parse_pipeline(&yaml).ok())
                .map(|p| p.pipeline)
            {
                if !pipeline.loops.iter().any(|r| r.id == region_id) {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(serde_json::json!({
                            "error": format!("region '{region_id}' not found in pipeline")
                        })),
                    )
                        .into_response();
                }
            } else {
                warn!(
                    "{kind_str}: pipeline snapshot unreadable for run {run_id}; skipping region validation"
                );
            }

            let cmd_event = event_log::Event {
                id: None,
                run_id: run_id.clone(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::CommandIssued,
                node_id: None,
                iter: None,
                payload: Some(payload),
            };
            if let Err(e) = append_event(&state, &cmd_event).await {
                return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
            }

            // Continue the run: an exhausted-unrouted region halts the run, so
            // lift the Halt/Failed back to Running before re-evaluating.
            if run_state.status == event_log::RunStatus::Halted
                || run_state.status == event_log::RunStatus::Failed
            {
                let resume_event = event_log::Event {
                    id: None,
                    run_id: run_id.clone(),
                    ts: event_log::now_iso(),
                    kind: event_log::EventKind::CommandIssued,
                    node_id: None,
                    iter: None,
                    payload: Some(serde_json::json!({ "command": "resume_run" })),
                };
                if let Err(e) = append_event(&state, &resume_event).await {
                    error!("failed to append resume_run after region route: {e}");
                }
            }

            let summary = re_evaluate_after_command(&state, &run_id).await;

            info!("{kind_str}: region {region_id} in run {run_id}");
            (StatusCode::OK, Json(summary.into_response_body())).into_response()
        }
        RunCommand::PauseRun => {
            let events = match load_events(&state.db, &run_id).await {
                Ok(e) => e,
                Err(e) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}"))
                        .into_response();
                }
            };
            let run_state = match event_log::project(&events) {
                Some(s) => s,
                None => {
                    return (StatusCode::NOT_FOUND, "run not found").into_response();
                }
            };

            if !matches!(
                run_state.status,
                event_log::RunStatus::Running | event_log::RunStatus::AwaitingUser
            ) {
                return (
                    StatusCode::CONFLICT,
                    Json(serde_json::json!({
                        "error": format!("cannot pause run in {:?} state", run_state.status)
                    })),
                )
                    .into_response();
            }

            let pause_event = event_log::Event {
                id: None,
                run_id: run_id.clone(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::RunPaused,
                node_id: None,
                iter: None,
                payload: None,
            };
            if let Err(e) = append_event(&state, &pause_event).await {
                return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
            }

            info!("pause_run: run {run_id}");
            (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
        }
        RunCommand::ResumeRun => {
            let events = match load_events(&state.db, &run_id).await {
                Ok(e) => e,
                Err(e) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}"))
                        .into_response();
                }
            };
            let run_state = match event_log::project(&events) {
                Some(s) => s,
                None => {
                    return (StatusCode::NOT_FOUND, "run not found").into_response();
                }
            };

            match run_state.status {
                event_log::RunStatus::Paused => {
                    let resume_event = event_log::Event {
                        id: None,
                        run_id: run_id.clone(),
                        ts: event_log::now_iso(),
                        kind: event_log::EventKind::RunResumed,
                        node_id: None,
                        iter: None,
                        payload: None,
                    };
                    if let Err(e) = append_event(&state, &resume_event).await {
                        return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}"))
                            .into_response();
                    }
                }
                event_log::RunStatus::Halted | event_log::RunStatus::Failed => {
                    let cmd_event = event_log::Event {
                        id: None,
                        run_id: run_id.clone(),
                        ts: event_log::now_iso(),
                        kind: event_log::EventKind::CommandIssued,
                        node_id: None,
                        iter: None,
                        payload: Some(serde_json::json!({ "command": "resume_run" })),
                    };
                    if let Err(e) = append_event(&state, &cmd_event).await {
                        return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}"))
                            .into_response();
                    }
                }
                _ => {}
            }

            // #316: a resumable run re-drives the git merge in its pipeline
            // worktree; kill any open shell (best-effort) so its uncommitted
            // edits can't race the merge. 409-refusing would deadlock — a shell
            // only dies on archive, reachable only from a terminal state.
            tmux_session_manager::kill(
                &state.tmux_socket(),
                &tmux_session_manager::shell_session_name(&run_id),
            );

            // #408 D5: resuming a sandboxed Run must re-arm its container before
            // the scheduler `docker exec`s into it. Containers are created without
            // `--restart` and `boot_recovery` skips terminal Runs, so after a host
            // reboot the container is down — reviving a terminal sandboxed Run
            // would otherwise spawn into a dead container ("failed to spawn tmux
            // session"). Resurrect it (via `spawn_blocking`, `ensure_ready` may
            // `docker build`) or fail EXPLICITLY — never a silent host fallback.
            // Mirrors the run-shell guard (#407 D11).
            if !run_state.sandbox.is_off() {
                let prep = match sandbox_run::context_from_state(&state, &run_state).await {
                    Ok(ctx) => tokio::task::spawn_blocking(move || sandbox_run::ensure_ready(&ctx))
                        .await
                        .unwrap_or_else(|je| {
                            Err(anyhow::anyhow!("sandbox ensure_ready panicked: {je}"))
                        }),
                    Err(e) => Err(e),
                };
                if let Err(e) = prep {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({
                            "error": format!("sandbox container unavailable: {e:#}")
                        })),
                    )
                        .into_response();
                }
                // #445: the container is up again — say so in the log, or the spawn
                // precondition would refuse every node the re-evaluation below proposes.
                // Load-bearing for the Run that failed *during* its own prep: its
                // projection is still `pending`, and resuming it is the operator's only
                // recovery path. Emitted only after `ensure_ready` returned `Ok` (so the
                // event never claims a container that isn't there) and only when the Run
                // is actually blocked (so a routine resume adds no no-op event).
                if run_state.sandbox_spawn_block().is_some() {
                    mark_sandbox_prep_ready(&state, &run_id).await;
                }
            }

            let summary = re_evaluate_after_command(&state, &run_id).await;

            info!("resume_run: run {run_id}");
            (StatusCode::OK, Json(summary.into_response_body())).into_response()
        }
        RunCommand::KillNode { node_id, iter } => {
            let session_name = tmux_session_manager::node_session_name(&run_id, &node_id, iter);
            tmux_session_manager::kill(&state.tmux_socket(), &session_name);
            // #407: also kill the process tree inside the container (best-effort,
            // no-op for `off`). The tmux-side `docker exec` client death leaves the
            // reparented container process alive.
            let kill_sandbox = reload_run_state(&state, &run_id)
                .await
                .is_some_and(|(_, s)| !s.sandbox.is_off());
            sandbox_run::kill_session_best_effort(
                state.docker_cmd_override.as_deref().unwrap_or("docker"),
                kill_sandbox,
                &run_id,
                &session_name,
            );

            let fail_event = event_log::Event {
                id: None,
                run_id: run_id.clone(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::NodeFailed,
                node_id: Some(node_id.clone()),
                iter: Some(iter),
                payload: Some(serde_json::json!({
                    "reason": "killed via kill_node command",
                    "source": "kill_node",
                })),
            };
            if let Err(e) = append_event(&state, &fail_event).await {
                return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
            }

            let cmd_event = event_log::Event {
                id: None,
                run_id: run_id.clone(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::CommandIssued,
                node_id: Some(node_id.clone()),
                iter: Some(iter),
                payload: Some(serde_json::json!({
                    "command": "kill_node",
                    "node_id": node_id,
                    "iter": iter,
                })),
            };
            if let Err(e) = append_event(&state, &cmd_event).await {
                error!("failed to append kill_node command event: {e}");
            }

            info!("kill_node: node {node_id} iter {iter} in run {run_id}");
            (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
        }
        RunCommand::RestartNode { node_id, iter } => {
            // Transition guard (#212 / #196): restart_node is mutually
            // exclusive with the scheduler's own re-fire — validate against
            // the projected state BEFORE killing anything, so a stale-view
            // restart of an old iter never races a newer live iteration.
            {
                let events = match load_events(&state.db, &run_id).await {
                    Ok(e) => e,
                    Err(e) => {
                        return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}"))
                            .into_response();
                    }
                };
                let run_state = event_log::project(&events);
                let restart_probe = event_log::Event {
                    id: None,
                    run_id: run_id.clone(),
                    ts: event_log::now_iso(),
                    kind: event_log::EventKind::NodeStarted,
                    node_id: Some(node_id.clone()),
                    iter: Some(iter),
                    payload: None,
                };
                if let transition_guard::Verdict::Reject { reason } =
                    transition_guard::validate_transition(run_state.as_ref(), &restart_probe)
                {
                    warn!("restart_node rejected for {node_id} iter {iter} in {run_id}: {reason}");
                    return (
                        StatusCode::CONFLICT,
                        Json(serde_json::json!({ "error": reason })),
                    )
                        .into_response();
                }
            }

            // Kill existing session
            let session_name = tmux_session_manager::node_session_name(&run_id, &node_id, iter);
            tmux_session_manager::kill(&state.tmux_socket(), &session_name);
            // #407: also kill the in-container process tree before the re-spawn
            // (best-effort, no-op for `off`) so the old session's container
            // process doesn't linger alongside the new one.
            let restart_sandbox = reload_run_state(&state, &run_id)
                .await
                .is_some_and(|(_, s)| !s.sandbox.is_off());
            sandbox_run::kill_session_best_effort(
                state.docker_cmd_override.as_deref().unwrap_or("docker"),
                restart_sandbox,
                &run_id,
                &session_name,
            );

            let cmd_event = event_log::Event {
                id: None,
                run_id: run_id.clone(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::CommandIssued,
                node_id: Some(node_id.clone()),
                iter: Some(iter),
                payload: Some(serde_json::json!({
                    "command": "restart_node",
                    "node_id": node_id,
                    "iter": iter,
                })),
            };
            if let Err(e) = append_event(&state, &cmd_event).await {
                error!("failed to append restart_node command event: {e}");
            }

            // Re-spawn the node
            let events = match load_events(&state.db, &run_id).await {
                Ok(e) => e,
                Err(e) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}"))
                        .into_response();
                }
            };
            let run_state = match event_log::project(&events) {
                Some(s) => s,
                None => {
                    return (StatusCode::NOT_FOUND, "run not found").into_response();
                }
            };

            let repo_root = effective_repo_root(&state, &run_state);
            let pipeline_path = {
                let run_scoped = run_scoped_pipeline_path(&repo_root, &run_id);
                if run_scoped.exists() {
                    run_scoped
                } else {
                    resolve_pipeline_path(&repo_root, &run_state.pipeline_name)
                }
            };
            let Ok(yaml) = std::fs::read_to_string(&pipeline_path) else {
                return (StatusCode::INTERNAL_SERVER_ERROR, "cannot read pipeline").into_response();
            };
            let Ok(parse_result) = pipeline::parse_pipeline(&yaml) else {
                return (StatusCode::INTERNAL_SERVER_ERROR, "cannot parse pipeline")
                    .into_response();
            };

            let pipeline = parse_result.pipeline;
            if let Some(node) = pipeline.nodes.iter().find(|n| n.id == node_id) {
                let worktree_dir = worktree_dir_for_run(&repo_root, &run_id);
                let artifacts_dir = worktree_dir.join(".pdo").join("artifacts");
                let resolved_vars = resolve_run_variables(&pipeline, &events);

                let spawn_ctx = SpawnContext {
                    pipeline: &pipeline,
                    run_id: &run_id,
                    pipeline_path: &pipeline_path,
                    worktree_dir: &worktree_dir,
                    artifacts_dir: &artifacts_dir,
                    resolved_vars: &resolved_vars,
                    repo_root: &repo_root,
                };

                spawn_node(SpawnDeps::from_state(&state), &spawn_ctx, node, iter).await;
            }

            info!("restart_node: node {node_id} iter {iter} in run {run_id}");
            (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
        }
        RunCommand::StartNode { node_id } => {
            // Force-spawn a node out of dependency order (#204). The manager
            // twin of the UI Start button: both funnel through `force_spawn_node`,
            // which derives the iteration and owns the run-status (D4) and
            // admission-cap (D5) guards. The wire `iter` is deliberately dropped
            // at parse time — letting the manager pin an iter would fight that
            // derivation.

            // Audit the manager's intent before acting, mirroring the other
            // command arms' `CommandIssued` parity event.
            let cmd_event = event_log::Event {
                id: None,
                run_id: run_id.clone(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::CommandIssued,
                node_id: Some(node_id.clone()),
                iter: None,
                payload: Some(serde_json::json!({
                    "command": "start_node",
                    "node_id": node_id,
                })),
            };
            if let Err(e) = append_event(&state, &cmd_event).await {
                error!("failed to append start_node command event: {e}");
            }

            force_spawn_node(&state, &run_id, &node_id).await
        }
        RunCommand::InjectArtifact { path, content } => {
            // `path` is already proven relative and `..`-free by the parse.
            let requested = std::path::Path::new(&path);

            let repo_root = match load_events(&state.db, &run_id).await {
                Ok(events) => match event_log::project(&events) {
                    Some(run_state) => effective_repo_root(&state, &run_state),
                    None => state.repo_root.clone(),
                },
                Err(_) => state.repo_root.clone(),
            };
            let worktree_dir = worktree_dir_for_run(&repo_root, &run_id);
            let artifacts_dir = worktree_dir.join(".pdo").join("artifacts");
            let full_path = artifacts_dir.join(requested);

            if let Some(parent) = full_path.parent() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({ "error": format!("failed to create dir: {e}") })),
                    )
                        .into_response();
                }
            }
            if let Err(e) = std::fs::write(&full_path, &content) {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": format!("failed to write artifact: {e}") })),
                )
                    .into_response();
            }

            let cmd_event = event_log::Event {
                id: None,
                run_id: run_id.clone(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::CommandIssued,
                node_id: None,
                iter: None,
                payload: Some(serde_json::json!({
                    "command": "inject_artifact",
                    "path": path,
                })),
            };
            if let Err(e) = append_event(&state, &cmd_event).await {
                return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
            }

            info!("inject_artifact: {path} in run {run_id}");
            (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
        }
        RunCommand::RenameRun { name: new_name } => {
            let rename_event = event_log::Event {
                id: None,
                run_id: run_id.clone(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::RunRenamed,
                node_id: None,
                iter: None,
                payload: Some(serde_json::json!({ "name": new_name })),
            };
            if let Err(e) = append_event(&state, &rename_event).await {
                return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
            }

            info!("rename_run: run {run_id} renamed to {:?}", new_name);
            (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
        }
        RunCommand::CleanupRun => cleanup_run(&state, &run_id).await,
        RunCommand::RetryAll => {
            let events = match load_events(&state.db, &run_id).await {
                Ok(e) => e,
                Err(e) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}"))
                        .into_response();
                }
            };
            let run_state = match event_log::project(&events) {
                Some(s) => s,
                None => {
                    return (StatusCode::NOT_FOUND, "run not found").into_response();
                }
            };

            // NOTE: deliberately NOT `RunStatus::is_terminal()` — this set omits
            // `Archived`. Whether an `Archived` run should be retry-able is an
            // open question (#237 follow-up F2).
            let is_terminal = matches!(
                run_state.status,
                event_log::RunStatus::Completed
                    | event_log::RunStatus::Failed
                    | event_log::RunStatus::Skipped
                    | event_log::RunStatus::Halted
            );
            if !is_terminal {
                return (
                    StatusCode::CONFLICT,
                    Json(serde_json::json!({
                        "error": format!("retry_all requires a terminal state, run is {:?}", run_state.status)
                    })),
                )
                    .into_response();
            }

            let run_started_event = events
                .iter()
                .find(|e| e.kind == event_log::EventKind::RunStarted);
            let run_started_payload = run_started_event.and_then(|e| e.payload.as_ref());

            let pipeline_name = run_state.pipeline_name.clone();
            let input = run_state.input.clone().unwrap_or_default();
            let variables: HashMap<String, serde_yaml::Value> = run_started_payload
                .and_then(|p| p.get("variables"))
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            // #470: read the RESOLVED repo, not the raw field. A retry of a run
            // created before the create boundary was hardened carries
            // `target_repo: null`; forwarding that raw would 400 at the
            // chokepoint — AFTER `cleanup_run` has already archived the original
            // (below), i.e. archived with no replacement. Resolving here lands
            // the retry where the original actually ran, and pins it explicitly.
            let target_repo = Some(
                effective_repo_root(&state, &run_state)
                    .to_string_lossy()
                    .into_owned(),
            );
            let source_branch = run_state.source_branch.clone();

            // Archive the current run (cleanup disk resources, keep events)
            let archive_resp = cleanup_run(&state, &run_id).await;
            let archive_status = archive_resp.into_response().status();
            if !archive_status.is_success() {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": "failed to archive original run" })),
                )
                    .into_response();
            }

            let new_run_req = CreateRunRequest {
                pipeline: pipeline_name,
                input,
                variables,
                // #377: preserve the library pipeline id (when the original run
                // carried one) so the retried run stays in the same "by pipeline"
                // stats bucket rather than falling back to the name.
                pipeline_id: run_state.pipeline_id.clone(),
                target_repo,
                source_branch,
                name: None,
                triggered_by: None,
                // #407/#410: a retry preserves the original Run's isolation mode
                // (immutable per-Run property projected from RunStarted). Wrapped in
                // `Some` so it is treated as EXPLICIT at the chokepoint — the resolver
                // must honour it exactly, never letting a changed instance default
                // silently re-sandbox (or un-sandbox) a retried Run.
                //
                // #432: we HAVE `run_state.sandbox_entries` here and deliberately do NOT
                // forward it. A retry is a NEW Run — new `run_id`, no node has staged
                // anything yet — so there is no coherence to protect, and re-resolving is
                // the only behaviour consistent with ADR-0031 §2 (a profile edited since
                // must take effect). Do not "fix" this by threading the frozen list.
                // Side effect, intended: a profile deleted since makes the retry 400,
                // loudly, instead of quietly running something else.
                sandbox: Some(run_state.sandbox.clone()),
            };
            let new_run_resp = create_run_core(&state, new_run_req, Vec::new()).await;

            info!("retry_all: archived run {run_id}, created new run");
            new_run_resp
        }
    }
}

/// The run reached a terminal state during a post-command re-evaluation
/// (ADR-0025 / #327).
#[derive(Debug, Clone)]
enum ReEvalTerminal {
    Completed,
    Halted(String),
}

/// The real effect of a post-command re-evaluation (ADR-0025 / #327): which
/// nodes were actually spawned, which candidate spawns were skipped (and why),
/// and whether the run went terminal. Command handlers surface this in their
/// response body instead of an unconditional `{ok:true}`.
#[derive(Debug, Default)]
struct ReEvalSummary {
    /// `(node_id, iter)` pairs whose spawn genuinely launched a session.
    spawned: Vec<(String, i64)>,
    /// Human-readable reasons for candidate spawns that did NOT launch
    /// (guard skips, throttling, spawn failures).
    skipped: Vec<String>,
    terminal: Option<ReEvalTerminal>,
}

impl ReEvalSummary {
    fn record_spawn(&mut self, node_id: &str, iter: i64, outcome: SpawnOutcome) {
        match outcome {
            SpawnOutcome::Spawned => self.spawned.push((node_id.to_string(), iter)),
            SpawnOutcome::Throttled => self.skipped.push(format!(
                "node '{node_id}' iter {iter} throttled into waiting (session cap)"
            )),
            // #445: `Deferred` joins the two here rather than getting its own arm —
            // its reason already reads as the operator sentence, and every consumer of
            // `skipped` is a "why did nothing start" message.
            SpawnOutcome::Refused { reason }
            | SpawnOutcome::Deferred { reason }
            | SpawnOutcome::Failed { reason } => self.skipped.push(reason),
        }
    }

    /// The truthful command-response body (ADR-0025). Spawns happened →
    /// `{ok, spawned}`; nothing launched → `{ok, noop, reason}`.
    fn into_response_body(self) -> serde_json::Value {
        if !self.spawned.is_empty() {
            let spawned: Vec<serde_json::Value> = self
                .spawned
                .into_iter()
                .map(|(node_id, iter)| serde_json::json!({ "node_id": node_id, "iter": iter }))
                .collect();
            return serde_json::json!({ "ok": true, "spawned": spawned });
        }
        let reason = match &self.terminal {
            Some(ReEvalTerminal::Completed) => "run completed".to_string(),
            Some(ReEvalTerminal::Halted(msg)) => format!("run halted: {msg}"),
            None => {
                if self.skipped.is_empty() {
                    "no eligible spawn".to_string()
                } else {
                    self.skipped.join("; ")
                }
            }
        };
        serde_json::json!({ "ok": true, "noop": true, "reason": reason })
    }
}

/// Re-evaluate the scheduler after a command (resume_run, extend_cycle).
/// Loads the pipeline and run state, resolves variables (including cycle extensions),
/// then re-evaluates outgoing edges of all completed nodes to find newly ready spawns.
/// Returns what actually happened so command handlers can tell the truth
/// (ADR-0025 / #327).
async fn re_evaluate_after_command(state: &AppState, run_id: &str) -> ReEvalSummary {
    let mut summary = ReEvalSummary::default();
    let events = match load_events(&state.db, run_id).await {
        Ok(e) => e,
        Err(e) => {
            error!("re_evaluate_after_command: failed to load events: {e}");
            summary.skipped.push(format!("failed to load events: {e}"));
            return summary;
        }
    };
    let run_state = match event_log::project(&events) {
        Some(s) => s,
        None => {
            summary.skipped.push("run not found".to_string());
            return summary;
        }
    };

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
        summary.skipped.push("pipeline file unreadable".to_string());
        return summary;
    };
    let Ok(parse_result) = pipeline::parse_pipeline(&yaml) else {
        summary.skipped.push("pipeline failed to parse".to_string());
        return summary;
    };

    let pipeline = parse_result.pipeline;
    let worktree_dir = worktree_dir_for_run(&repo_root, run_id);
    let artifacts_dir = worktree_dir.join(".pdo").join("artifacts");
    let mut resolved_vars = resolve_run_variables(&pipeline, &events);

    // Apply cycle extensions to variables: for each extend_cycle command,
    // find variable references in outgoing edges of the target node and bump them.
    let extensions = event_log::collect_cycle_extensions(&events);
    for (ext_node_id, additional) in &extensions {
        let var_refs = extract_variable_refs_from_outgoing_edges(&pipeline, ext_node_id);
        for var_name in var_refs {
            if let Some(val) = resolved_vars.get_mut(&var_name) {
                if let Some(n) = val.as_i64() {
                    *val = serde_yaml::Value::Number(serde_yaml::Number::from(n + additional));
                }
            }
        }
    }

    // Apply manager loop-region routes (ADR-0011 / #152). A `bump_region` raises
    // the region's effective `max_iter` by the bumped amount; when that cap is a
    // `$var` reference, bumping the variable lifts the `iter >= max` exit guard
    // so the region runs the extra laps after `resume_run`. (A literal cap is the
    // region engine's bound — #148 — and reads the recorded route directly.)
    let region_routes = event_log::collect_region_routes(&events);
    for (region_id, route) in &region_routes {
        if route.bumped_by <= 0 {
            continue;
        }
        if let Some(region) = pipeline.loops.iter().find(|r| &r.id == region_id) {
            if let Some(serde_yaml::Value::String(s)) = &region.max_iter {
                if let Some(var_name) = s.strip_prefix('$') {
                    if let Some(val) = resolved_vars.get_mut(var_name) {
                        if let Some(n) = val.as_i64() {
                            *val = serde_yaml::Value::Number(serde_yaml::Number::from(
                                n + route.bumped_by,
                            ));
                        }
                    }
                }
            }
        }
    }

    // Find completed nodes whose outgoing edges might now fire with updated vars
    let completed_node_ids: Vec<String> = run_state
        .nodes
        .values()
        .filter(|n| n.status == event_log::NodeStatus::Completed)
        .map(|n| n.node_id.clone())
        .collect();

    let spawn_ctx = SpawnContext {
        pipeline: &pipeline,
        run_id,
        pipeline_path: &pipeline_path,
        worktree_dir: &worktree_dir,
        artifacts_dir: &artifacts_dir,
        resolved_vars: &resolved_vars,
        repo_root: &repo_root,
    };

    let frontmatter_by_node = resolve_completed_frontmatter(&pipeline, &run_state, &artifacts_dir);

    for completed_node_id in &completed_node_ids {
        let source_iter = run_state
            .nodes
            .get(completed_node_id)
            .map(|n| n.iter)
            .unwrap_or(1);

        let frontmatter_fields =
            resolve_source_frontmatter(&pipeline, completed_node_id, source_iter, &artifacts_dir);

        let actions = scheduler::evaluate_outgoing_edges_full(
            &pipeline,
            &run_state,
            completed_node_id,
            &resolved_vars,
            &frontmatter_fields,
            &frontmatter_by_node,
        );

        for action in &actions {
            // Re-evaluation applies `GuardSuperfluous` (#212): schedule only
            // MISSING work — never a node with a live iteration, never a
            // completed one — on the pass-1 `run_state` snapshot (INV-2).
            match scheduler_interpreter::interpret(
                state,
                &spawn_ctx,
                &run_state,
                SpawnDedup::GuardSuperfluous,
                source_iter,
                action,
            )
            .await
            {
                ActionOutcome::Spawned {
                    node_id,
                    iter,
                    outcome,
                } => summary.record_spawn(&node_id, iter, outcome),
                ActionOutcome::SpawnSkipped { reason } => {
                    // Skip log stays driver-side (INV-6): pass-1 prefix.
                    info!("re_evaluate_after_command: skip spawn — {reason}");
                    summary.skipped.push(reason);
                }
                ActionOutcome::Progressed => {}
                ActionOutcome::Completed => {
                    summary.terminal = Some(ReEvalTerminal::Completed);
                    return summary;
                }
                ActionOutcome::Halted { message } => {
                    summary.terminal = Some(ReEvalTerminal::Halted(message));
                    return summary;
                }
            }
        }
    }

    // Pass 1 may have appended events; re-project so pass 2 sees fresh state
    // (same race fix as handle_node_completion).
    let Some((fresh_events, fresh_run_state)) = reload_run_state(state, run_id).await else {
        return summary;
    };
    let mut fresh_resolved_vars = resolve_run_variables(&pipeline, &fresh_events);
    for (ext_node_id, additional) in &extensions {
        let var_refs = extract_variable_refs_from_outgoing_edges(&pipeline, ext_node_id);
        for var_name in var_refs {
            if let Some(val) = fresh_resolved_vars.get_mut(&var_name) {
                if let Some(n) = val.as_i64() {
                    *val = serde_yaml::Value::Number(serde_yaml::Number::from(n + additional));
                }
            }
        }
    }
    let fresh_spawn_ctx = SpawnContext {
        pipeline: &pipeline,
        run_id,
        pipeline_path: &pipeline_path,
        worktree_dir: &worktree_dir,
        artifacts_dir: &artifacts_dir,
        resolved_vars: &fresh_resolved_vars,
        repo_root: &repo_root,
    };

    // Check loop body completion for all loop nodes
    for loop_node in pipeline
        .nodes
        .iter()
        .filter(|n| n.node_type == pipeline::NodeType::Loop)
    {
        let loop_actions = scheduler::evaluate_loop_body_completion(
            &pipeline,
            &fresh_run_state,
            &loop_node.id,
            &fresh_resolved_vars,
        );
        // `evaluate_loop_body_completion` only emits Spawn / Loop* today; the
        // total `interpret` subsumes the old `_ => emit_loop_action`
        // fallthrough. GuardSuperfluous on the reloaded snapshot (INV-2);
        // source_iter is irrelevant here (no SwitchRouted) — pass 1 (INV-3).
        for action in &loop_actions {
            match scheduler_interpreter::interpret(
                state,
                &fresh_spawn_ctx,
                &fresh_run_state,
                SpawnDedup::GuardSuperfluous,
                1,
                action,
            )
            .await
            {
                ActionOutcome::Spawned {
                    node_id,
                    iter,
                    outcome,
                } => summary.record_spawn(&node_id, iter, outcome),
                ActionOutcome::SpawnSkipped { reason } => {
                    info!("re_evaluate_after_command(loop): skip spawn — {reason}");
                    summary.skipped.push(reason);
                }
                ActionOutcome::Progressed => {}
                ActionOutcome::Completed => {
                    summary.terminal = Some(ReEvalTerminal::Completed);
                    return summary;
                }
                ActionOutcome::Halted { message } => {
                    summary.terminal = Some(ReEvalTerminal::Halted(message));
                    return summary;
                }
            }
        }
    }

    // Check the barrier of every collection region (ADR-0011 / #269)
    for region in pipeline
        .loops
        .iter()
        .filter(|r| r.kind == pipeline::LoopKind::Collection)
    {
        let collection_actions =
            scheduler::evaluate_collection_barrier(&pipeline, &fresh_run_state, region);
        // `evaluate_collection_barrier` only emits CollectionDone today; the
        // total `interpret` routes it through `emit_collection_action` exactly as
        // the old `_ => emit_collection_action` fallthrough did. GuardSuperfluous
        // on the reloaded snapshot; source_iter irrelevant (no SwitchRouted).
        for action in &collection_actions {
            match scheduler_interpreter::interpret(
                state,
                &fresh_spawn_ctx,
                &fresh_run_state,
                SpawnDedup::GuardSuperfluous,
                1,
                action,
            )
            .await
            {
                ActionOutcome::Spawned {
                    node_id,
                    iter,
                    outcome,
                } => summary.record_spawn(&node_id, iter, outcome),
                ActionOutcome::SpawnSkipped { reason } => {
                    info!("re_evaluate_after_command(collection): skip spawn — {reason}");
                    summary.skipped.push(reason);
                }
                ActionOutcome::Progressed => {}
                ActionOutcome::Completed => {
                    summary.terminal = Some(ReEvalTerminal::Completed);
                    return summary;
                }
                ActionOutcome::Halted { message } => {
                    summary.terminal = Some(ReEvalTerminal::Halted(message));
                    return summary;
                }
            }
        }
    }

    summary
}

/// Extract variable references ($name) from when clauses on Switch output ports
/// reachable from outgoing edges of a node.
fn extract_variable_refs_from_outgoing_edges(
    pipeline: &pipeline::PipelineDef,
    node_id: &str,
) -> Vec<String> {
    let mut refs = Vec::new();
    for edge in &pipeline.edges {
        if edge.source.node != node_id {
            continue;
        }
        let target_node = pipeline.nodes.iter().find(|n| n.id == edge.target.node);
        if let Some(node) = target_node {
            if node.node_type == pipeline::NodeType::Switch {
                for port in &node.outputs {
                    if let Some(ref when) = port.when {
                        collect_yaml_var_refs(when, &mut refs);
                    }
                }
            }
            if let Some(ref max_iter) = node.max_iter {
                collect_yaml_var_refs(max_iter, &mut refs);
            }
        }
    }
    refs.sort();
    refs.dedup();
    refs
}

fn collect_yaml_var_refs(val: &serde_yaml::Value, refs: &mut Vec<String>) {
    match val {
        serde_yaml::Value::String(s) if s.starts_with('$') => {
            refs.push(s[1..].to_string());
        }
        serde_yaml::Value::Mapping(map) => {
            for (_, v) in map {
                collect_yaml_var_refs(v, refs);
            }
        }
        serde_yaml::Value::Sequence(seq) => {
            for v in seq {
                collect_yaml_var_refs(v, refs);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- #236: the pure parse ---
    //
    // Fourteen wire shapes in, fourteen verdicts out, no HTTP and no daemon.
    // Before this slice none of these messages was reachable from a test at
    // all — which is precisely the argument #236 makes. `#[test]`, not
    // `#[tokio::test]`: `CommandParseError` is `PartialEq`, so no response body
    // has to be read back asynchronously to see what was rejected.

    /// Build a wire request from its JSON, exactly as axum's extractor would.
    fn wire(json: serde_json::Value) -> RunCommandRequest {
        serde_json::from_value(json).expect("test fixture must deserialise")
    }

    fn parse(json: serde_json::Value) -> Result<RunCommand, CommandParseError> {
        parse_run_command(wire(json))
    }

    fn reject(json: serde_json::Value) -> String {
        parse(json).expect_err("expected a rejection").0
    }

    #[test]
    fn parse_accepts_the_thirteen_kinds() {
        let cases: [(serde_json::Value, RunCommand); 13] = [
            (
                serde_json::json!({ "kind": "mark_node_done", "node_id": "n1", "iter": 3 }),
                RunCommand::MarkNodeDone {
                    node_id: "n1".into(),
                    iter: 3,
                },
            ),
            (
                serde_json::json!({ "kind": "extend_cycle", "node_id": "n1", "additional_iter": 2 }),
                RunCommand::ExtendCycle {
                    node_id: "n1".into(),
                    additional_iter: 2,
                },
            ),
            (
                serde_json::json!({ "kind": "bump_region", "region_id": "r1", "additional_iter": 2 }),
                RunCommand::Region {
                    region_id: "r1".into(),
                    action: RegionAction::Bump { additional_iter: 2 },
                },
            ),
            (
                serde_json::json!({ "kind": "end_region", "region_id": "r1" }),
                RunCommand::Region {
                    region_id: "r1".into(),
                    action: RegionAction::End,
                },
            ),
            (
                serde_json::json!({ "kind": "pause_run" }),
                RunCommand::PauseRun,
            ),
            (
                serde_json::json!({ "kind": "resume_run" }),
                RunCommand::ResumeRun,
            ),
            (
                serde_json::json!({ "kind": "kill_node", "node_id": "n1", "iter": 7 }),
                RunCommand::KillNode {
                    node_id: "n1".into(),
                    iter: 7,
                },
            ),
            (
                serde_json::json!({ "kind": "restart_node", "node_id": "n1", "iter": 7 }),
                RunCommand::RestartNode {
                    node_id: "n1".into(),
                    iter: 7,
                },
            ),
            (
                serde_json::json!({ "kind": "start_node", "node_id": "n1" }),
                RunCommand::StartNode {
                    node_id: "n1".into(),
                },
            ),
            (
                serde_json::json!({ "kind": "inject_artifact", "path": "a/b.md", "content": "x" }),
                RunCommand::InjectArtifact {
                    path: "a/b.md".into(),
                    content: "x".into(),
                },
            ),
            (
                serde_json::json!({ "kind": "rename_run", "name": "hello" }),
                RunCommand::RenameRun {
                    name: "hello".into(),
                },
            ),
            (
                serde_json::json!({ "kind": "cleanup_run" }),
                RunCommand::CleanupRun,
            ),
            (
                serde_json::json!({ "kind": "retry_all" }),
                RunCommand::RetryAll,
            ),
        ];

        for (json, want) in cases {
            let label = json.to_string();
            assert_eq!(parse(json), Ok(want), "{label}");
        }
    }

    #[test]
    fn parse_rejects_carry_the_exact_wire_message() {
        for (json, want) in [
            (
                serde_json::json!({ "kind": "mark_node_done" }),
                "node_id required for mark_node_done",
            ),
            (
                serde_json::json!({ "kind": "extend_cycle" }),
                "node_id required for extend_cycle",
            ),
            (
                serde_json::json!({ "kind": "extend_cycle", "node_id": "n1" }),
                "additional_iter required for extend_cycle",
            ),
            (
                serde_json::json!({ "kind": "extend_cycle", "node_id": "n1", "additional_iter": 0 }),
                "additional_iter must be positive",
            ),
            (
                serde_json::json!({ "kind": "bump_region" }),
                "region_id required for bump_region",
            ),
            (
                serde_json::json!({ "kind": "end_region" }),
                "region_id required for end_region",
            ),
            (
                serde_json::json!({ "kind": "bump_region", "region_id": "r1" }),
                "additional_iter required for bump_region",
            ),
            (
                serde_json::json!({ "kind": "bump_region", "region_id": "r1", "additional_iter": -1 }),
                "additional_iter must be positive",
            ),
            (
                serde_json::json!({ "kind": "kill_node" }),
                "node_id required for kill_node",
            ),
            (
                serde_json::json!({ "kind": "restart_node" }),
                "node_id required for restart_node",
            ),
            (
                serde_json::json!({ "kind": "start_node" }),
                "node_id required for start_node",
            ),
            (
                serde_json::json!({ "kind": "inject_artifact" }),
                "path required for inject_artifact",
            ),
            (
                serde_json::json!({ "kind": "inject_artifact", "path": "a.md" }),
                "content required for inject_artifact",
            ),
            (
                serde_json::json!({ "kind": "inject_artifact", "path": "../x", "content": "x" }),
                "path traversal not allowed",
            ),
            (
                serde_json::json!({ "kind": "bogus_command" }),
                "unknown command: bogus_command",
            ),
        ] {
            let label = json.to_string();
            assert_eq!(reject(json), want, "{label}");
        }
    }

    #[test]
    fn parse_checks_fields_in_the_order_the_wire_expects() {
        // Two orderings that a rewrite silently inverts. Both are observable:
        // the same malformed request answers a different message.
        assert_eq!(
            reject(serde_json::json!({ "kind": "bump_region" })),
            "region_id required for bump_region",
            "the region comes before the count"
        );
        assert_eq!(
            reject(serde_json::json!({ "kind": "inject_artifact", "path": "../escape" })),
            "content required for inject_artifact",
            "a missing body outranks a hostile path"
        );
        // And the kind match outranks every field check: an unknown kind with
        // nothing else in the body still reports the kind.
        assert_eq!(
            reject(serde_json::json!({ "kind": "ghost" })),
            "unknown command: ghost"
        );
    }

    #[test]
    fn parse_keeps_the_lenient_defaults() {
        // Three leniencies that look like bugs and are not.
        assert_eq!(
            parse(serde_json::json!({ "kind": "mark_node_done", "node_id": "n1" })),
            Ok(RunCommand::MarkNodeDone {
                node_id: "n1".into(),
                iter: 1
            }),
            "an omitted iter means 1, not 'current'"
        );
        assert_eq!(
            parse(serde_json::json!({ "kind": "rename_run" })),
            Ok(RunCommand::RenameRun {
                name: String::new()
            }),
            "an omitted name renames to the empty string"
        );
        assert_eq!(
            parse(
                serde_json::json!({ "kind": "end_region", "region_id": "r1", "additional_iter": 5 })
            ),
            Ok(RunCommand::Region {
                region_id: "r1".into(),
                action: RegionAction::End
            }),
            "a stray additional_iter on end_region is accepted and dropped"
        );
        // `start_node` drops the wire `iter` — the server derives it (#204).
        assert_eq!(
            parse(serde_json::json!({ "kind": "start_node", "node_id": "n1", "iter": 99 })),
            Ok(RunCommand::StartNode {
                node_id: "n1".into()
            })
        );
    }

    #[test]
    fn parse_rejects_every_traversal_shape() {
        for bad in [
            "../../etc/passwd",
            "/absolute/path.md",
            "ok/../../../escape",
        ] {
            assert_eq!(
                reject(serde_json::json!({
                    "kind": "inject_artifact", "path": bad, "content": "x"
                })),
                "path traversal not allowed",
                "{bad}"
            );
        }
    }

    #[test]
    fn kind_str_round_trips_every_variant() {
        // `kind_str` feeds two log lines and is total over `RegionAction`
        // precisely because Bump and End are distinguished in the type.
        for kind in [
            "mark_node_done",
            "extend_cycle",
            "bump_region",
            "end_region",
            "pause_run",
            "resume_run",
            "kill_node",
            "restart_node",
            "start_node",
            "inject_artifact",
            "rename_run",
            "cleanup_run",
            "retry_all",
        ] {
            let cmd = parse(serde_json::json!({
                "kind": kind,
                "node_id": "n1",
                "region_id": "r1",
                "additional_iter": 1,
                "path": "a.md",
                "content": "x",
            }))
            .unwrap_or_else(|e| panic!("{kind} must parse: {}", e.0));
            assert_eq!(cmd.kind_str(), kind);
        }
    }

    #[test]
    fn parse_error_renders_as_400_json() {
        let resp = CommandParseError("boom".to_string()).into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            resp.headers()
                .get(axum::http::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok()),
            Some("application/json")
        );
    }

    #[test]
    fn extract_var_refs_finds_dollar_variables_in_switch_outputs() {
        use crate::pipeline::*;

        let pipeline = PipelineDef {
            name: "test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![NodeDef {
                id: "sw1".into(),
                name: "switch".into(),
                node_type: NodeType::Switch,
                inputs: vec![Port {
                    name: "in".into(),
                    repeated: false,
                    side: None,
                    port_type: PortType::Markdown,
                    frontmatter: None,
                    when: None,
                    description: None,
                }],
                outputs: vec![Port {
                    name: "pass".into(),
                    repeated: false,
                    side: None,
                    port_type: PortType::Markdown,
                    frontmatter: None,
                    when: Some(serde_yaml::from_str("iter: { lt: \"$max_iter_review\" }").unwrap()),
                    description: None,
                }],
                interactive: false,
                view: None,
                max_iter: None,
                over: None,
                model: None,
                effort: None,
            }],
            edges: vec![EdgeDef {
                source: EdgeEndpoint {
                    node: "reviewer".into(),
                    port: "review".into(),
                },
                target: EdgeEndpoint {
                    node: "sw1".into(),
                    port: "in".into(),
                },
                reason: None,
                when: None,
                is_else: false,
                repeated: false,
                ..Default::default()
            }],
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: true,
        };

        let refs = extract_variable_refs_from_outgoing_edges(&pipeline, "reviewer");
        assert_eq!(refs, vec!["max_iter_review"]);
    }

    #[test]
    fn extract_var_refs_empty_for_no_vars() {
        use crate::pipeline::*;

        let pipeline = PipelineDef {
            name: "test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![NodeDef {
                id: "b".into(),
                name: "b".into(),
                node_type: NodeType::DocOnly,
                inputs: vec![Port {
                    name: "in".into(),
                    repeated: false,
                    side: None,
                    port_type: PortType::Markdown,
                    frontmatter: None,
                    when: None,
                    description: None,
                }],
                outputs: vec![],
                interactive: false,
                view: None,
                max_iter: None,
                over: None,
                model: None,
                effort: None,
            }],
            edges: vec![EdgeDef {
                source: EdgeEndpoint {
                    node: "a".into(),
                    port: "out".into(),
                },
                target: EdgeEndpoint {
                    node: "b".into(),
                    port: "in".into(),
                },
                reason: None,
                when: None,
                is_else: false,
                repeated: false,
                ..Default::default()
            }],
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: true,
        };

        let refs = extract_variable_refs_from_outgoing_edges(&pipeline, "a");
        assert!(refs.is_empty());
    }
}
