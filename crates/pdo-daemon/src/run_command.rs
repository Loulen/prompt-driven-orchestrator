//! `POST /runs/{id}/commands` — the Pipeline Manager's command surface: the HTTP
//! handler, the post-command re-evaluation it drives, and the pipeline helpers
//! only that re-evaluation uses.
//!
//! Layer 3 in ADR-0009 terms. Its sibling surface is the per-node route family
//! `POST /runs/{id}/nodes/{node_id}/{start,stop,retry}` (the canvas buttons),
//! which still lives in `lib.rs`.

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Json, Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use tracing::{error, info, warn};

use crate::node_spawn::{spawn_node, SpawnContext, SpawnDeps, SpawnOutcome};
use crate::scheduler_interpreter::{ActionOutcome, SpawnDedup};
use crate::worktree_ops::worktree_dir_for_run;
use crate::{
    append_event, check_output_validation_with_retry, cleanup_run, completion_head_gate,
    completion_refusal, create_run_core, effective_repo_root, event_log, force_spawn_node,
    load_events, load_projected, loop_region, mark_sandbox_prep_ready, pipeline, reap_node_session,
    reload_run_state, resolve_completed_frontmatter, resolve_pipeline_path,
    resolve_run_pipeline_path, resolve_run_variables, resolve_source_frontmatter, restart_verdict,
    retry_waiting_nodes, run_advance, run_is_forgotten, run_scoped_pipeline_path, sandbox_run,
    scheduler, scheduler_interpreter, tmux_session_manager, transition_guard, AppState,
    CreateRunRequest, TargetRepoInput,
};

/// The wire shape of a command. `pub(crate)` only because axum's extractor puts
/// this type in the handler's public signature; its fields stay private.
#[derive(Deserialize)]
pub(crate) struct RunCommandRequest {
    kind: String,
    #[serde(default)]
    node_id: Option<String>,
    #[serde(default)]
    iter: Option<i64>,
    #[serde(default)]
    additional_iter: Option<i64>,
    /// The loop region a `bump_region` / `end_region` / `set_region_max_iter`
    /// targets.
    #[serde(default)]
    region_id: Option<String>,
    /// The **absolute** iteration cap of a `set_region_max_iter` — the total laps
    /// the region now allows, not a delta like `additional_iter`.
    #[serde(default)]
    max_iter: Option<i64>,
    /// The **source** of a `force_route`: a node id OR a region id whose `when:`
    /// edges are short-circuited. Separate from `node_id`/`region_id` so it can name
    /// either kind without overloading their meaning.
    #[serde(default)]
    from: Option<String>,
    /// The **target** node a `force_route` exits to.
    #[serde(default)]
    target: Option<String>,
    /// Per-port overrides for a `start_node`, or the default outputs of a
    /// `skip_node`. Keyed by port name; each value is **inline content** written to
    /// that port's artifact before the node runs, so an operator can drive a node
    /// without its upstream having produced.
    #[serde(default)]
    overrides: Option<HashMap<String, String>>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

/// A command from the Pipeline Manager, already validated. The only way to build
/// one is [`parse_run_command`], so no arm re-validates presence, applies a
/// default, or re-inspects a path.
///
/// One variant per accepted `kind`, except `bump_region`/`end_region`, which share
/// [`RunCommand::Region`] because they share their whole I/O tail.
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
    /// `bump_region` / `end_region`. They differ only in payload; everything after
    /// (region lookup, `CommandIssued` append, Halt lift, re-evaluation) is shared.
    /// The difference lives in [`RegionAction`], so `end_region` cannot carry an
    /// `additional_iter` IN THE TYPE.
    Region {
        region_id: String,
        action: RegionAction,
    },
    /// Raise a bounded region's iteration cap **in flight**, absolute, no restart:
    /// the scheduler reads the folded override in place of the declared `max_iter`,
    /// uniformly for a literal and a `$var` cap.
    SetRegionMaxIter {
        region_id: String,
        max_iter: i64,
    },
    /// An explicit exit from a node or region to a target, short-circuiting the
    /// source's `when:` edges — the lever for a run wedged `unrouted`. `target` may
    /// be the `End` node, which completes the run. Folded from the log, so a reopen
    /// does not re-decide it.
    ForceRoute {
        from: String,
        target: String,
    },
    /// Skip a node **locally**: mark it satisfied with an empty (or overridden)
    /// output WITHOUT terminating the run. Distinct from `pdo skip` (`RunSkipped`, a
    /// run-level no-op) — downstream advances, and the skipped node counts as
    /// satisfied for re-projection, so a reopen never re-spawns it.
    SkipNode {
        node_id: String,
        iter: i64,
        overrides: Option<HashMap<String, String>>,
    },
    PauseRun,
    ResumeRun,
    /// The global re-open (ADR-0049): lifts ANY terminal Run — and an
    /// incident-parked `AwaitingUser` — back to `Running` by a re-projection that
    /// freezes the satisfied `(node, iter)` (the scheduler's dedup refuses to
    /// re-spawn them) and re-drives only the unsatisfied work. Distinct from
    /// [`RunCommand::RetryAll`], which archives and forks a NEW run.
    ReopenRun,
    KillNode {
        node_id: String,
        iter: i64,
    },
    RestartNode {
        node_id: String,
        iter: i64,
    },
    /// Recover an `Interrupted` node (ADR-0049 §3). The mechanism is chosen off the
    /// node's **frozen** harness: re-attach in place when it `can_resume()`
    /// (ADR-0045), else fall back automatically to restart-with-artifacts.
    RecoverNode {
        node_id: String,
        iter: i64,
    },
    /// `req.iter` is dropped at parse time: `force_spawn_node` derives the iteration
    /// itself, and a manager-pinned one would fight that derivation.
    StartNode {
        node_id: String,
        overrides: Option<HashMap<String, String>>,
    },
    /// `path` stays a `String`, not a `PathBuf`: it is echoed verbatim into the
    /// `CommandIssued` payload. Already proven relative and free of `..`.
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

/// Rejected at parse time. Every rejection site answers `400` + `Json({"error":
/// …})`, so the status is not carried here — add it the day one stops being a 400.
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
    /// The wire `kind` this command came from. Load-bearing: the region arm logs it
    /// after an `.await` (the request is long gone by then), and every
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
            RunCommand::SetRegionMaxIter { .. } => "set_region_max_iter",
            RunCommand::ForceRoute { .. } => "force_route",
            RunCommand::SkipNode { .. } => "skip_node",
            RunCommand::PauseRun => "pause_run",
            RunCommand::ResumeRun => "resume_run",
            RunCommand::ReopenRun => "reopen_run",
            RunCommand::KillNode { .. } => "kill_node",
            RunCommand::RestartNode { .. } => "restart_node",
            RunCommand::RecoverNode { .. } => "recover_node",
            RunCommand::StartNode { .. } => "start_node",
            RunCommand::InjectArtifact { .. } => "inject_artifact",
            RunCommand::RenameRun { .. } => "rename_run",
            RunCommand::CleanupRun => "cleanup_run",
            RunCommand::RetryAll => "retry_all",
        }
    }
}

/// Turn the wire shape into a validated command. **Pure** — no DB, no filesystem,
/// no clock — so every rejection below is testable without a live daemon.
///
/// Two orderings are behaviour, not style, and both are pinned by tests:
/// `bump_region` reports a missing `region_id` BEFORE a missing `additional_iter`,
/// and `inject_artifact` reports a missing `content` BEFORE it looks at the path.
/// Swap either and a malformed request changes its answer.
///
/// The `kind` match comes first, so an unknown `kind` with missing fields still
/// answers `"unknown command"` rather than a field complaint.
///
/// Deliberately NOT `#[serde(tag = "kind")]`: that would turn these messages into
/// axum's `422` + serde prose in text/plain.
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
            // `region_id` first, for BOTH kinds: `{"kind":"bump_region"}` alone must
            // complain about the region, not the count.
            let region_id = required(req.region_id, "region_id", kind)?;
            let action = if kind == "bump_region" {
                RegionAction::Bump {
                    additional_iter: positive_iter(req.additional_iter, "bump_region")?,
                }
            } else {
                // A stray `additional_iter` here is accepted and dropped; rejecting
                // it would be a regression.
                RegionAction::End
            };
            Ok(RunCommand::Region { region_id, action })
        }
        "set_region_max_iter" => {
            // A non-positive cap is rejected before any event: a region is never
            // made zero-lap by a stray command.
            let region_id = required(req.region_id, "region_id", "set_region_max_iter")?;
            let max_iter = req.max_iter.ok_or_else(|| {
                CommandParseError("max_iter required for set_region_max_iter".into())
            })?;
            if max_iter <= 0 {
                return Err(CommandParseError("max_iter must be positive".to_string()));
            }
            Ok(RunCommand::SetRegionMaxIter {
                region_id,
                max_iter,
            })
        }
        "force_route" => {
            // `from` (a node OR region id) before `target`: `{"kind":"force_route"}`
            // alone must complain about the source, not the destination.
            let from = required(req.from, "from", "force_route")?;
            let target = required(req.target, "target", "force_route")?;
            Ok(RunCommand::ForceRoute { from, target })
        }
        "skip_node" => Ok(RunCommand::SkipNode {
            node_id: required(req.node_id, "node_id", "skip_node")?,
            iter: req.iter.unwrap_or(1),
            overrides: req.overrides,
        }),
        "pause_run" => Ok(RunCommand::PauseRun),
        "resume_run" => Ok(RunCommand::ResumeRun),
        "reopen_run" => Ok(RunCommand::ReopenRun),
        "kill_node" => Ok(RunCommand::KillNode {
            node_id: required(req.node_id, "node_id", "kill_node")?,
            iter: req.iter.unwrap_or(1),
        }),
        "restart_node" => Ok(RunCommand::RestartNode {
            node_id: required(req.node_id, "node_id", "restart_node")?,
            iter: req.iter.unwrap_or(1),
        }),
        "recover_node" => Ok(RunCommand::RecoverNode {
            node_id: required(req.node_id, "node_id", "recover_node")?,
            iter: req.iter.unwrap_or(1),
        }),
        "start_node" => Ok(RunCommand::StartNode {
            node_id: required(req.node_id, "node_id", "start_node")?,
            overrides: req.overrides,
        }),
        "inject_artifact" => {
            let path = required(req.path, "path", "inject_artifact")?;
            // Before the traversal check: a hostile path with no content answers
            // "content required", not "path traversal".
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

/// A `recover_node` refusal in the ADR-0035 §3 shape, plus the chosen `mechanism`.
/// `409`, never a `2xx` that would pretend a re-attach happened.
fn recover_conflict(
    mechanism: crate::recovery::RecoveryMechanism,
    slug: &str,
    message: &str,
) -> Response {
    (
        StatusCode::CONFLICT,
        Json(serde_json::json!({
            "error": slug,
            "recoverable": true,
            "mechanism": mechanism.as_str(),
            "message": message,
        })),
    )
        .into_response()
}

/// Render a re-attach [`crate::ReattachOutcome`] as the `recover_node` response.
/// Only reached on the [`crate::recovery::RecoveryMechanism::Reattach`] branch —
/// the restart-with-artifacts branch forwards `restart_node`'s own response.
fn recover_response(
    mechanism: crate::recovery::RecoveryMechanism,
    run_id: &str,
    node_id: &str,
    iter: i64,
    outcome: crate::ReattachOutcome,
) -> Response {
    use crate::ReattachOutcome;
    match outcome {
        ReattachOutcome::Resumed => {
            info!("recover_node: re-attached {node_id} iter {iter} in run {run_id}");
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "ok": true,
                    "mechanism": mechanism.as_str(),
                    "reattached": [{ "node_id": node_id, "iter": iter }],
                })),
            )
                .into_response()
        }
        // A re-attach IS a (re)spawn, so the cap can queue it. A `2xx` and not a
        // no-op: the caller must not re-issue.
        ReattachOutcome::CapReached { live, cap } => (
            StatusCode::OK,
            Json(serde_json::json!({
                "ok": true,
                "waiting": true,
                "mechanism": mechanism.as_str(),
                "reason": format!(
                    "session cap reached ({live}/{cap}): the re-attach of {node_id} iter {iter} \
                     is queued — do not re-issue"
                ),
            })),
        )
            .into_response(),
        // Honest `409`s, never a silent success. `CannotResume` is unreachable here
        // (this branch was chosen because the harness CAN resume) but is mapped for
        // exhaustiveness.
        ReattachOutcome::ScriptNode => recover_conflict(
            mechanism,
            "script_not_resumable",
            "a script node runs deterministic bash, not an LLM session — it cannot be \
             re-attached (#248, ADR-0017)",
        ),
        ReattachOutcome::WorkingDirMissing => recover_conflict(
            mechanism,
            "worktree_missing",
            "the node's working directory is gone — nothing to re-enter",
        ),
        ReattachOutcome::CannotResume => recover_conflict(
            mechanism,
            "harness_cannot_resume",
            "the frozen harness declares no resume tail (ADR-0045)",
        ),
        // The mechanism choice above already refuses a gone frozen harness, so this
        // is normally unreachable — mapped for exhaustiveness.
        ReattachOutcome::FrozenHarnessGone { harness } => recover_conflict(
            mechanism,
            "frozen_harness_gone",
            &format!(
                "this node's frozen harness '{harness}' no longer resolves; PDO will not \
                 relaunch claude in its place"
            ),
        ),
        ReattachOutcome::TraceRefused { error } => {
            recover_conflict(mechanism, "reattach_refused", &error)
        }
        ReattachOutcome::ResumeFailed { error } => {
            recover_conflict(mechanism, "reattach_failed", &error)
        }
    }
}

pub(crate) async fn run_command(
    State(state): State<Arc<AppState>>,
    AxumPath(run_id): AxumPath<String>,
    Json(req): Json<RunCommandRequest>,
) -> Response {
    // ADR-0024: a forgotten run accepts no commands. Reject before any arm can
    // append (extend_cycle appends CommandIssued before its own existence check) or
    // trigger side effects.
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
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("error: {e}") })),
            )
                .into_response();
        }
    }

    // Validation runs AFTER the 410 gate: a malformed command against a forgotten
    // run answers 410, not 400, and that precedence belongs to ADR-0024.
    let cmd = match parse_run_command(req) {
        Ok(cmd) => cmd,
        Err(e) => return e.into_response(),
    };

    dispatch(state, run_id, cmd).await
}

/// Run one validated command and answer for it.
///
/// **Returns an `axum::Response` deliberately, not a semantic `CommandOutcome`
/// mapped to HTTP by a central mapper.** This surface emits twenty-two distinct
/// (status, content-type, body-shape) triplets — including `404`s in both
/// text/plain and JSON, and the `201 {run_id}` of `create_run_core` forwarded
/// verbatim by `retry_all`, which the frontend reads to navigate to the retried
/// Run. A single mapper must pick one content-type per verdict, and each pick
/// rewrites the other half. See the ADR-0009 addendum.
///
/// `dispatch` is a DRIVER, not a leaf: it reaches across sqlite, tmux, docker,
/// worktrees and Run creation, so it takes the whole `AppState`. Dependency
/// injection à la `SpawnDeps` is reserved for leaves (`node_spawn`).
async fn dispatch(state: Arc<AppState>, run_id: String, cmd: RunCommand) -> Response {
    // Read before the match consumes `cmd`: the region arm logs its wire kind after
    // two `.await`s.
    let kind_str = cmd.kind_str();

    match cmd {
        RunCommand::MarkNodeDone { node_id, iter } => {
            // NOT `load_projected`, which would 404 on an unstarted run: this arm
            // needs the `Option<RunState>` itself, so `None` maps to `Allow` and
            // falls back on a synthetic empty `RunState`.
            let events = match load_events(&state.db, &run_id).await {
                Ok(e) => e,
                Err(e) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({ "error": format!("error: {e}") })),
                    )
                        .into_response();
                }
            };
            let run_state = event_log::project(&events);

            // ADR-0049: completing a node on a terminal (or incident-parked) Run
            // embeds the re-open, so the completion lands on a live Run instead of
            // the guard's "resume the run first" 409.
            let run_state = match run_state {
                Some(rs) => {
                    match crate::embed_reopen_for_targeted_command(&state, &run_id, rs).await {
                        Ok(s) => Some(s),
                        Err(e) => {
                            return (
                                StatusCode::INTERNAL_SERVER_ERROR,
                                Json(serde_json::json!({ "error": format!("error: {e}") })),
                            )
                                .into_response();
                        }
                    }
                }
                None => None,
            };

            // Validate against the projected state BEFORE any side effect (output
            // validation, append, downstream dispatch) — the same pure decision as
            // `node_done` and `node_skip`. The typed stop keeps a legal duplicate a
            // `200` while a reject becomes `409 {"error":"completion_rejected",…}`.
            // The invariant cannot live on `CompletionAttempt`: this arm never
            // builds one, and it is the whole UI path.
            if let Some(stop) = completion_head_gate(
                run_advance::evaluate_completion_head(run_state.as_ref(), &run_id, &node_id, iter),
                "mark_node_done",
                &run_id,
                &node_id,
                iter,
            ) {
                return stop.into_response();
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

            // #654 / ADR-0060: the manual completion delivers like every other
            // one. Pre-#654 this arm ran neither the merge-back nor any
            // worktree check, so an interactive node marked complete from the UI
            // left its sub-worktree stranded and its shared-worktree edits
            // uncommitted — the asymmetry ADR-0035 recorded as an accepted limit
            // and this ticket removes. Same single operation, same events, before
            // the terminal append and therefore before the downstream spawn.
            if let Some(refusal) = crate::deliver_node_run(
                &state,
                &events,
                rs_ref,
                &repo_root,
                &worktree_dir,
                &run_id,
                &node_id,
                iter,
            )
            .await
            {
                return completion_refusal::refusal_response(&refusal);
            }

            // The shared chokepoint (#490). Both surfaces project the refusal
            // through the same single function, which is what makes "a refusal is
            // never a 2xx" cover `POST /commands` too.
            if let Some(refusal) = check_output_validation_with_retry(
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
                return completion_refusal::refusal_response(&refusal);
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
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": format!("error: {e}") })),
                )
                    .into_response();
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

            // Shared post-`NodeCompleted` tail, `SweepFirst`: advance the run +
            // re-drive throttled waiters, THEN fire this node's edges, then the
            // single completion gate. flag = true because the just-finished node was
            // interactive, so the run can still project `AwaitingUser` at the gate
            // and must still complete — unlike the other sites (flag = false).
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
            // Validate against the run's pipeline SNAPSHOT before any event is
            // appended: a rejected command must leave no trace in the log, and a
            // library edit after launch must not affect an in-flight run.
            let (_, run_state) = match load_projected(&state, &run_id).await {
                Ok(v) => v,
                Err(resp) => return *resp,
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
                // An unreadable snapshot can't be validated against; stay permissive
                // rather than reject blind.
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
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": format!("error: {e}") })),
                )
                    .into_response();
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

            let summary = re_evaluate_after_command(&state, &run_id).await;

            info!("extend_cycle: node {node_id} +{additional_iter} in run {run_id}");
            (StatusCode::OK, Json(summary.into_response_body())).into_response()
        }
        // The Pipeline Manager routes a loop region BY ID: `bump_region` runs N more
        // iterations, `end_region` fires its completion. Both append a control-flow
        // `CommandIssued` then continue the run (lift an exhausted-unrouted Halt and
        // re-evaluate), so a stalled region is unstuck without a daemon restart.
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

            // Validate against the run's pipeline SNAPSHOT before any event is
            // appended: an unknown region_id must leave no trace in the log.
            let (_, run_state) = match load_projected(&state, &run_id).await {
                Ok(v) => v,
                Err(resp) => return *resp,
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
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": format!("error: {e}") })),
                )
                    .into_response();
            }

            // An exhausted-unrouted region parks the run, so lift it back to
            // `Running` before re-evaluating. An interactive `AwaitingUser` (no
            // incident reason) is left alone: routing a region never overrides a
            // node's genuine user wait.
            let needs_reopen = matches!(
                run_state.status,
                event_log::RunStatus::Halted | event_log::RunStatus::Failed
            ) || (run_state.status == event_log::RunStatus::AwaitingUser
                && run_state.awaiting_reason.is_some());
            if needs_reopen {
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
        // Raise a bounded region's cap in flight. Same shape as the region routing
        // arm: validate against the snapshot, append the `CommandIssued` (folded into
        // `region_max_iter_overrides`), lift a parked run, re-evaluate.
        RunCommand::SetRegionMaxIter {
            region_id,
            max_iter,
        } => {
            let (_, run_state) = match load_projected(&state, &run_id).await {
                Ok(v) => v,
                Err(resp) => return *resp,
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
                warn!("set_region_max_iter: pipeline snapshot unreadable for run {run_id}; skipping region validation");
            }

            let cmd_event = event_log::Event {
                id: None,
                run_id: run_id.clone(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::CommandIssued,
                node_id: None,
                iter: None,
                payload: Some(serde_json::json!({
                    "command": "set_region_max_iter",
                    "region_id": region_id,
                    "max_iter": max_iter,
                })),
            };
            if let Err(e) = append_event(&state, &cmd_event).await {
                return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
            }

            // Lift a parked run so the raised cap re-drives the region now. An
            // interactive `AwaitingUser` is left alone.
            let needs_reopen = matches!(
                run_state.status,
                event_log::RunStatus::Halted | event_log::RunStatus::Failed
            ) || (run_state.status == event_log::RunStatus::AwaitingUser
                && run_state.awaiting_reason.is_some());
            if needs_reopen {
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
                    error!("failed to append resume_run after set_region_max_iter: {e}");
                }
            }

            let summary = re_evaluate_after_command(&state, &run_id).await;
            info!("set_region_max_iter: region {region_id} -> {max_iter} in run {run_id}");
            (StatusCode::OK, Json(summary.into_response_body())).into_response()
        }
        // Force an explicit exit from a node or region, short-circuiting the
        // source's `when:` edges. Validate BOTH endpoints against the snapshot (a bad
        // route must leave no trace), append the `CommandIssued` (folded into
        // `forced_routes`), lift a parked run, re-evaluate.
        RunCommand::ForceRoute { from, target } => {
            let (_, run_state) = match load_projected(&state, &run_id).await {
                Ok(v) => v,
                Err(resp) => return *resp,
            };
            let repo_root = effective_repo_root(&state, &run_state);
            let pipeline_path =
                resolve_run_pipeline_path(&repo_root, &run_id, &run_state.pipeline_name);
            if let Some(pipeline) = std::fs::read_to_string(&pipeline_path)
                .ok()
                .and_then(|yaml| pipeline::parse_pipeline(&yaml).ok())
                .map(|p| p.pipeline)
            {
                let from_ok = pipeline.nodes.iter().any(|n| n.id == from)
                    || pipeline.loops.iter().any(|r| r.id == from);
                if !from_ok {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(serde_json::json!({
                            "error": format!("force_route source '{from}' is neither a node nor a region in the pipeline")
                        })),
                    )
                        .into_response();
                }
                if !pipeline.nodes.iter().any(|n| n.id == target) {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(serde_json::json!({
                            "error": format!("force_route target '{target}' is not a node in the pipeline")
                        })),
                    )
                        .into_response();
                }
            } else {
                warn!("force_route: pipeline snapshot unreadable for run {run_id}; skipping endpoint validation");
            }

            let cmd_event = event_log::Event {
                id: None,
                run_id: run_id.clone(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::CommandIssued,
                node_id: None,
                iter: None,
                payload: Some(serde_json::json!({
                    "command": "force_route",
                    "from": from,
                    "target": target,
                })),
            };
            if let Err(e) = append_event(&state, &cmd_event).await {
                return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
            }

            let needs_reopen = matches!(
                run_state.status,
                event_log::RunStatus::Halted | event_log::RunStatus::Failed
            ) || (run_state.status == event_log::RunStatus::AwaitingUser
                && run_state.awaiting_reason.is_some());
            if needs_reopen {
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
                    error!("failed to append resume_run after force_route: {e}");
                }
            }

            let summary = re_evaluate_after_command(&state, &run_id).await;
            info!("force_route: {from} -> {target} in run {run_id}");
            (StatusCode::OK, Json(summary.into_response_body())).into_response()
        }
        // Skip a node LOCALLY: mark it satisfied with an empty (or overridden)
        // output, run continues. Unlike `node_skip`, NO `RunSkipped` is appended;
        // downstream advances on that output and the node counts as satisfied, so a
        // reopen never re-spawns it.
        RunCommand::SkipNode {
            node_id,
            iter,
            overrides,
        } => {
            let events = match load_events(&state.db, &run_id).await {
                Ok(e) => e,
                Err(e) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}"))
                        .into_response();
                }
            };
            let run_state = event_log::project(&events);

            // Embed the re-open: skipping on a terminal / incident-parked run lifts
            // it first, so the completion gate does not refuse with "resume first".
            let run_state = match run_state {
                Some(rs) => {
                    match crate::embed_reopen_for_targeted_command(&state, &run_id, rs).await {
                        Ok(s) => Some(s),
                        Err(e) => {
                            return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}"))
                                .into_response();
                        }
                    }
                }
                None => None,
            };

            // Same head-gate as `mark_node_done`: a duplicate skip is rejected or
            // no-op'd, never double-appended.
            if let Some(stop) = completion_head_gate(
                run_advance::evaluate_skip_completion_head(
                    run_state.as_ref(),
                    &run_id,
                    &node_id,
                    iter,
                ),
                "skip_node",
                &run_id,
                &node_id,
                iter,
            ) {
                return stop.into_response();
            }

            let empty_run_state = event_log::RunState::new(run_id.clone(), String::new());
            let rs_ref = run_state.as_ref().unwrap_or(&empty_run_state);
            let repo_root = effective_repo_root(&state, rs_ref);
            let pipeline_path =
                resolve_run_pipeline_path(&repo_root, &run_id, &rs_ref.pipeline_name);
            let worktree_dir = worktree_dir_for_run(&repo_root, &run_id);
            let artifacts_dir = worktree_dir.join(".pdo").join("artifacts");

            // Write the skipped node's outputs, so the downstream resolver finds a
            // real (if empty) artifact instead of a missing file that would read as
            // "not produced". An unreadable snapshot hides the node's ports; deposit
            // a single default `output`.
            let overrides = overrides.unwrap_or_default();
            let write_result = match std::fs::read_to_string(&pipeline_path)
                .ok()
                .and_then(|yaml| pipeline::parse_pipeline(&yaml).ok())
                .map(|p| p.pipeline)
            {
                Some(p) => crate::node_primitives::write_skip_outputs(
                    &p,
                    &node_id,
                    iter,
                    &overrides,
                    &artifacts_dir,
                ),
                None => {
                    let one: HashMap<String, String> = [(
                        "output".to_string(),
                        overrides.get("output").cloned().unwrap_or_default(),
                    )]
                    .into_iter()
                    .collect();
                    crate::node_primitives::inject_outputs(
                        &crate::node_primitives::InjectOutputsParams {
                            node_id: &node_id,
                            iter,
                            artifacts: &one,
                            artifacts_dir: &artifacts_dir,
                        },
                    );
                    Ok(vec!["output".to_string()])
                }
            };
            if let Err(reason) = write_result {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": reason })),
                )
                    .into_response();
            }

            // Deliberately NO output validation and NO sub-worktree merge (the whole
            // point is an empty/dummy output), and — unlike `node_skip` — NO
            // `RunSkipped`: the run stays live.
            let node_completed = event_log::Event {
                id: None,
                run_id: run_id.clone(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::NodeCompleted,
                node_id: Some(node_id.clone()),
                iter: Some(iter),
                payload: Some(serde_json::json!({
                    "source": "skip_node",
                    "skipped": true,
                    "reason": "skipped locally by operator",
                    "override_ports": overrides.keys().cloned().collect::<Vec<_>>(),
                })),
            };
            if let Err(e) = append_event(&state, &node_completed).await {
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
                    "command": "skip_node",
                    "node_id": node_id,
                    "iter": iter,
                })),
            };
            if let Err(e) = append_event(&state, &cmd_event).await {
                error!("failed to append skip_node command event: {e}");
            }

            // Same post-completion tail as `mark_node_done`, flag=false (a skipped
            // node is never interactive).
            run_advance::complete_node(
                &state,
                &run_id,
                &node_id,
                run_advance::CompletionOrder::SweepFirst,
                false,
            )
            .await;

            info!("skip_node: node {node_id} iter {iter} skipped locally in run {run_id}");
            (
                StatusCode::OK,
                Json(serde_json::json!({ "ok": true, "skipped": true })),
            )
                .into_response()
        }
        RunCommand::PauseRun => {
            let (_, run_state) = match load_projected(&state, &run_id).await {
                Ok(v) => v,
                Err(resp) => return *resp,
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
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": format!("error: {e}") })),
                )
                    .into_response();
            }

            info!("pause_run: run {run_id}");
            (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
        }
        RunCommand::ResumeRun => {
            let (_, run_state) = match load_projected(&state, &run_id).await {
                Ok(v) => v,
                Err(resp) => return *resp,
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
                        return (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(serde_json::json!({ "error": format!("error: {e}") })),
                        )
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
                        return (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(serde_json::json!({ "error": format!("error: {e}") })),
                        )
                            .into_response();
                    }
                }
                _ => {}
            }

            // A resumable run re-drives the git merge in its pipeline worktree; kill
            // any open shell so its uncommitted edits can't race it. Refusing with a
            // 409 would deadlock: a shell only dies on archive, itself reachable only
            // from a terminal state.
            tmux_session_manager::kill(
                &state.tmux_socket(),
                &tmux_session_manager::shell_session_name(&run_id),
            );

            // Re-arm a sandboxed Run's container before the scheduler `docker exec`s
            // into it. Containers are created without `--restart` and `boot_recovery`
            // skips terminal Runs, so after a host reboot the container is down and
            // reviving the Run would spawn into a dead one. Resurrect it (via
            // `spawn_blocking`, `ensure_ready` may `docker build`) or fail
            // EXPLICITLY — never a silent host fallback.
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
                // Record that the container is up again, or the spawn precondition
                // refuses every node the re-evaluation below proposes. Load-bearing
                // for a Run that failed *during* its own prep: its projection is
                // still `pending` and resuming is the operator's only recovery path.
                // Emitted only after `ensure_ready` returned `Ok`, and only when the
                // Run is actually blocked, so a routine resume adds no no-op event.
                if run_state.sandbox_spawn_block().is_some() {
                    mark_sandbox_prep_ready(&state, &run_id).await;
                }
            }

            let summary = re_evaluate_after_command(&state, &run_id).await;

            info!("resume_run: run {run_id}");
            (StatusCode::OK, Json(summary.into_response_body())).into_response()
        }
        RunCommand::ReopenRun => {
            let (_, run_state) = match load_projected(&state, &run_id).await {
                Ok(v) => v,
                Err(resp) => return *resp,
            };

            // Re-openable = any terminal Run, or an incident-parked `AwaitingUser`.
            // An interactive `AwaitingUser` and a cleanly `Running`/`Paused` Run are
            // refused loudly: reopen never overrides a node's user wait, and a live
            // Run needs no re-projection.
            let reopenable = run_state.status.is_terminal()
                && run_state.status != event_log::RunStatus::Archived
                || (run_state.status == event_log::RunStatus::AwaitingUser
                    && run_state.awaiting_reason.is_some());
            if !reopenable {
                return (
                    StatusCode::CONFLICT,
                    Json(serde_json::json!({
                        "error": "not_reopenable",
                        "message": format!(
                            "run {run_id} is {:?}: reopen_run only re-opens a terminal \
                             or incident-parked run",
                            run_state.status
                        ),
                    })),
                )
                    .into_response();
            }

            // The projection lifts the Run to `Running`, freezes satisfied
            // `(node, iter)` and drops interrupted nodes so they re-drive. The
            // terminal label stays in the log.
            let cmd_event = event_log::Event {
                id: None,
                run_id: run_id.clone(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::CommandIssued,
                node_id: None,
                iter: None,
                payload: Some(serde_json::json!({ "command": "reopen_run" })),
            };
            if let Err(e) = append_event(&state, &cmd_event).await {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": format!("error: {e}") })),
                )
                    .into_response();
            }

            // Kill any open shell before the re-drive re-arms the merge.
            tmux_session_manager::kill(
                &state.tmux_socket(),
                &tmux_session_manager::shell_session_name(&run_id),
            );

            // Re-arm a sandboxed Run's container before the scheduler `docker exec`s
            // into it (same guard as `resume_run`).
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
                if run_state.sandbox_spawn_block().is_some() {
                    mark_sandbox_prep_ready(&state, &run_id).await;
                }
            }

            let summary = re_evaluate_after_command(&state, &run_id).await;

            info!("reopen_run: run {run_id}");
            (StatusCode::OK, Json(summary.into_response_body())).into_response()
        }
        RunCommand::KillNode { node_id, iter } => {
            // READ-ONLY, and BEFORE any append: one projection yields both the
            // `repo_root` the snapshot goes under (a Run may target a repo other than
            // the daemon's) and the sandbox flag the in-container kill needs. Both
            // must come from the SAME projection. `reload_run_state`, not
            // `load_projected`: both of its failure modes mean the same thing here —
            // no overridden repo, no container to kill — so neither may become a 4xx.
            let (repo_root, kill_sandbox) = reload_run_state(&state, &run_id)
                .await
                .map(|(_, s)| (effective_repo_root(&state, &s), !s.sandbox.is_off()))
                .unwrap_or_else(|| (state.repo_root.clone(), false));

            // THE TERMINAL EVENT FIRST, THE REAP SECOND. Reaping first opens a
            // "dead session / projection still Running" window that `GET …/pane`
            // answers by relaunching the harness; and on an append error the node
            // would stay `Running` forever with its session already killed and no
            // audit event. Appending first makes a 500 mean "nothing happened,
            // retry". Accepted risk: a few ms where the node is `Failed` with a live
            // session, which `GET …/pane` honestly reports as `live`.
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
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": format!("error: {e}") })),
                )
                    .into_response();
            }

            // The helper already contains the tmux kill AND the best-effort session
            // kill, preceded by the pane capture; adding either alongside would double
            // the `docker exec`. Called unconditionally, including when the append
            // above was no-op'd by the transition guard, so a second `kill_node` stays
            // an idempotent cleanup — and the first snapshot stays immutable, since a
            // dead session makes `capture` return `None`.
            reap_node_session(&state, &repo_root, &run_id, &node_id, iter, kill_sandbox);

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

            // Killing a node FREES an admission slot, so re-drive the nodes throttled
            // into `waiting`. `retry_waiting_nodes` has no timer of its own — every
            // caller is event-driven — so a `restart_node` that answered
            // `waiting:true` would otherwise starve forever.
            retry_waiting_nodes(&state).await;

            info!("kill_node: node {node_id} iter {iter} in run {run_id}");
            (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
        }
        RunCommand::RestartNode { node_id, iter } => {
            // #489 / ADR-0037 — EVERY KNOWABLE CAUSE IS TESTED BEFORE THE KILL, AND
            // THE `SpawnOutcome` IS READ.
            //
            // Pre-#489 this arm killed the tmux session, appended its
            // `CommandIssued`, THEN discovered the Run / the pipeline / the node, and
            // finally dropped `spawn_node`'s return without so much as a `let _ =`.
            // Every one of the five `SpawnOutcome`s answered `200 {"ok":true}` — and
            // on an isolated node the spawn failed 100% of the time
            // (`git worktree add -b` on a branch that already exists, exit 255),
            // which is the whole of #489: session dead, zero events, node still
            // projected `Running`, and 30 s later the liveness sweep inventing
            // `session_died` — a false cause that sent operators after tmux for a git
            // bug.
            //
            // ADR-0025 §2's "validate before writing" now extends to the KILL, not
            // just the append. The order below is the contract.

            // PRE-KILL PROBE 1: the transition guard. NOT `load_projected`, same
            // reason as `mark_node_done`:
            // `validate_transition` takes an `Option<RunState>` and maps
            // `None -> Allow` deliberately, so an unstarted run must reach the guard
            // rather than be 404'd here.
            let events = match load_events(&state.db, &run_id).await {
                Ok(e) => e,
                Err(e) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({ "error": format!("error: {e}") })),
                    )
                        .into_response();
                }
            };
            let projected = event_log::project(&events);
            // A restart on a terminal (or incident-parked) Run re-opens it atomically
            // before the guard below sees it — no "resume then restart" race.
            let projected = match projected {
                Some(rs) => {
                    match crate::embed_reopen_for_targeted_command(&state, &run_id, rs).await {
                        Ok(s) => Some(s),
                        Err(e) => {
                            return (
                                StatusCode::INTERNAL_SERVER_ERROR,
                                Json(serde_json::json!({ "error": format!("error: {e}") })),
                            )
                                .into_response();
                        }
                    }
                }
                None => None,
            };
            let restart_probe = event_log::Event {
                id: None,
                run_id: run_id.clone(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::NodeStarted,
                node_id: Some(node_id.clone()),
                iter: Some(iter),
                payload: None,
            };
            // The synthetic `NodeStarted` probe is the RIGHT probe here, unlike on
            // `node_retry`: `validate_start` answers `Allow` on `live_iter == iter`,
            // and `restart_node` alone may re-spawn a live iteration. Do not narrow it
            // to `run_accepts_lifecycle`.
            match transition_guard::validate_transition(projected.as_ref(), &restart_probe) {
                transition_guard::Verdict::Allow => {}
                transition_guard::Verdict::Reject { reason } => {
                    // ONE slug, the guard's prose in `message`. Three of the guard's
                    // reasons land here and are deliberately NOT discriminated: see
                    // `RestartRefusal::RestartRejected`.
                    let refusal = restart_verdict::RestartRefusal::RestartRejected {
                        // Forward the typed cause as prose; the slug stays
                        // `restart_refused`, still not discriminated.
                        message: reason.to_string(),
                        session_killed: false,
                    };
                    warn!(
                        "restart_node rejected for {node_id} iter {iter} in {run_id}: {}",
                        refusal.reason()
                    );
                    return restart_verdict::restart_response(
                        &restart_verdict::RestartVerdict::Refused(refusal),
                    );
                }
                // Defensive parity with `force_spawn_node`, which treats the two
                // identically. `validate_start` never returns `NoOp` today — it is
                // `Allow` or `Reject` — so this arm is unreachable in production and
                // no layer-3 test claims to reach it.
                transition_guard::Verdict::NoOp { reason } => {
                    info!("restart_node no-op for {node_id} iter {iter} in {run_id}: {reason}");
                    return restart_verdict::restart_response(
                        &restart_verdict::RestartVerdict::NoOp { reason },
                    );
                }
            }

            // PRE-KILL PROBE 2: does the Run exist at all? `404` WITHOUT a trace —
            // this used to be answered after the kill and the `CommandIssued` append.
            let Some(run_state) = projected else {
                return (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({ "error": "run not found" })),
                )
                    .into_response();
            };

            // PRE-KILL PROBE 3: the Run's pipeline SNAPSHOT, not the library
            // (ADR-0025 §2) — the same snapshot-first helper `extend_cycle` uses.
            let repo_root = effective_repo_root(&state, &run_state);
            let pipeline_path =
                resolve_run_pipeline_path(&repo_root, &run_id, &run_state.pipeline_name);
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
            let pipeline = parse_result.pipeline;

            // PRE-KILL PROBE 4: is the target in that pipeline? Without the `else`,
            // an unknown `node_id` answered `200 {"ok":true}` after killing a session
            // and appending an audit event for work that never happened.
            let Some(node) = pipeline.nodes.iter().find(|n| n.id == node_id) else {
                let refusal = restart_verdict::RestartRefusal::NodeNotFound {
                    node_id: node_id.clone(),
                };
                warn!("restart_node refused in {run_id}: {}", refusal.reason());
                return restart_verdict::restart_response(
                    &restart_verdict::RestartVerdict::Refused(refusal),
                );
            };

            // PRE-KILL PROBE 5: is the sandbox container up? `sandbox_spawn_block()`
            // is pure, so evaluating it here costs nothing and turns a post-kill `200`
            // lie into a pre-kill `409`. Read off the SAME projection as above.
            if let Some(reason) = run_state.sandbox_spawn_block() {
                let refusal = restart_verdict::RestartRefusal::SandboxPrepNotReady {
                    message: reason,
                    session_killed: false,
                };
                info!("restart_node deferred in {run_id}: {}", refusal.reason());
                return restart_verdict::restart_response(
                    &restart_verdict::RestartVerdict::Refused(refusal),
                );
            }

            // ── PRE-KILL PROBE 6: is the sub-worktree someone else's? (#489-B) ────
            //
            // A pure `git` read. The cost is accepted on ADR-0037 §3's terms: nothing
            // knowable is paid for with a kill. `Absent` / `Reusable` / `Recyclable`
            // all proceed — `ensure_sub_worktree` handles each, and never destroys
            // work in flight.
            // #653/ADR-0060: read the isolation FROZEN on this iteration, not the
            // document's — the re-spawn below will land in the frozen directory,
            // so probing the other one would classify a worktree nobody is about
            // to use (and skip the one that matters).
            let owns_sub_worktree = crate::merge_action::frozen_isolation(&events, &node_id, iter)
                .unwrap_or(node.is_isolated());
            if owns_sub_worktree {
                let sub_wt_dir =
                    crate::worktree_ops::sub_worktree_path(&repo_root, &run_id, &node_id, iter);
                let sub_branch = crate::worktree_ops::sub_worktree_branch(&run_id, &node_id, iter);
                let pipeline_branch = format!("pdo/run-{run_id}");
                if let crate::worktree_ops::SubWorktreeState::Occupied { detail } =
                    crate::worktree_ops::classify_sub_worktree(
                        &repo_root,
                        &sub_wt_dir,
                        &sub_branch,
                        &pipeline_branch,
                    )
                {
                    let refusal =
                        restart_verdict::RestartRefusal::SubWorktreeOccupied { message: detail };
                    warn!("restart_node refused in {run_id}: {}", refusal.reason());
                    return restart_verdict::restart_response(
                        &restart_verdict::RestartVerdict::Refused(refusal),
                    );
                }
            }

            // FROM HERE ON THERE ARE SIDE EFFECTS.

            let session_name = tmux_session_manager::node_session_name(&run_id, &node_id, iter);
            // Deliberately a BARE kill, not `reap_node_session`: the helper's pane
            // snapshot would never be served, since `GET …/pane` only serves one for a
            // TERMINAL iteration and a restart leaves the node non-terminal.
            tmux_session_manager::kill(&state.tmux_socket(), &session_name);
            // Also kill the in-container process tree before the re-spawn, or the old
            // session's process lingers alongside the new one. `sandbox` is immutable
            // over a Run's life, so the pre-kill projection above answers it — no
            // post-kill `reload_run_state` needed.
            sandbox_run::kill_session_best_effort(
                state.docker_cmd_override.as_deref().unwrap_or("docker"),
                !run_state.sandbox.is_off(),
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

            // Read the `SpawnOutcome`. It is not `#[must_use]` and must not become
            // one (fire-and-forget schedulers drop it on purpose), but THIS caller has
            // a client waiting on an answer.
            let outcome = spawn_node(SpawnDeps::from_state(&state), &spawn_ctx, node, iter).await;
            let verdict = match outcome {
                SpawnOutcome::Spawned {
                    reused_sub_worktree,
                    base_sha,
                    interrupted_git_ops,
                } => restart_verdict::RestartVerdict::Spawned {
                    node_id: node_id.clone(),
                    iter,
                    reused_sub_worktree,
                    base_sha,
                    interrupted_git_ops,
                },
                // ADR-0037 §2: a `2xx`, and NOT a `noop` — a `NodeWaiting` was
                // appended and `retry_waiting_nodes` genuinely picks the node back up.
                SpawnOutcome::Throttled => restart_verdict::RestartVerdict::Waiting {
                    reason: format!(
                        "node {node_id} iter {iter} is queued behind the session cap: it will \
                         spawn when a slot frees — do not re-issue"
                    ),
                },
                // Both of these are races: the head probes above evaluated the same
                // predicates and passed, then `spawn_node` re-evaluated them against
                // a fresher projection. The session is already dead, hence
                // `session_killed`.
                SpawnOutcome::Deferred { reason } => restart_verdict::RestartVerdict::Refused(
                    restart_verdict::RestartRefusal::SandboxPrepNotReady {
                        message: reason,
                        session_killed: true,
                    },
                ),
                SpawnOutcome::Refused { reason } => restart_verdict::RestartVerdict::Refused(
                    restart_verdict::RestartRefusal::RestartRejected {
                        message: reason,
                        session_killed: true,
                    },
                ),
                // A panne, not a verdict → `500`. `run_failed` is re-PROJECTED, never
                // guessed: the producers of `Failed` disagree about what they append,
                // and a `500` routes the CLI toward `pdo fail` — catastrophic advice if
                // `RunFailed` is already on the log.
                SpawnOutcome::Failed { reason } => {
                    let run_failed = reload_run_state(&state, &run_id)
                        .await
                        .is_some_and(|(_, s)| s.status == event_log::RunStatus::Failed);
                    restart_verdict::RestartVerdict::Broken {
                        message: reason,
                        run_failed,
                    }
                }
            };

            info!("restart_node: node {node_id} iter {iter} in run {run_id} -> {verdict:?}");
            restart_verdict::restart_response(&verdict)
        }
        RunCommand::RecoverNode { node_id, iter } => {
            // ADR-0049 §3: the mechanism is chosen off the node's FROZEN harness,
            // never the YAML's — re-attach in place when it `can_resume()`, else fall
            // back AUTOMATICALLY to restart-with-artifacts. The fallback is not a
            // second human decision.
            let events = match load_events(&state.db, &run_id).await {
                Ok(e) => e,
                Err(e) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}"))
                        .into_response();
                }
            };
            let Some(run_state) = event_log::project(&events) else {
                return (StatusCode::NOT_FOUND, "run not found").into_response();
            };
            // A name that WAS frozen but no longer resolves REFUSES — never a silent
            // `claude` relaunch in this node's worktree. A row with NO frozen harness
            // keeps the `claude` floor, which `can_resume`.
            let harness_home_root = sandbox_run::sandbox_home_roots(&state)
                .map(|(home, _)| home)
                .unwrap_or_default();
            let descriptor = match crate::find_launch_harness(&events, &node_id, iter).as_deref() {
                Some(name) => {
                    match crate::harness_registry::HarnessRegistry::load(&harness_home_root)
                        .resolve(name)
                    {
                        Some(d) => d,
                        None => {
                            return recover_conflict(
                                crate::recovery::RecoveryMechanism::Reattach,
                                "frozen_harness_gone",
                                &format!(
                                    "this node's frozen harness '{name}' no longer resolves \
                                     (embedded name dropped or disk descriptor removed); PDO will \
                                     not relaunch claude in its place — restore its descriptor, or \
                                     retry the node to start fresh"
                                ),
                            );
                        }
                    }
                }
                None => crate::harness_registry::claude(),
            };
            let mechanism = crate::recovery::choose_recovery(descriptor.can_resume());

            // Audit the intent, naming the mechanism so the automatic fallback is
            // legible in the log (parity with the other arms' `CommandIssued`).
            let cmd_event = event_log::Event {
                id: None,
                run_id: run_id.clone(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::CommandIssued,
                node_id: Some(node_id.clone()),
                iter: Some(iter),
                payload: Some(serde_json::json!({
                    "command": "recover_node",
                    "node_id": node_id,
                    "iter": iter,
                    "mechanism": mechanism.as_str(),
                })),
            };
            if let Err(e) = append_event(&state, &cmd_event).await {
                error!("failed to append recover_node command event: {e}");
            }

            match mechanism {
                crate::recovery::RecoveryMechanism::RestartWithArtifacts => {
                    // The automatic fallback: a fresh agent handed the partial
                    // artifacts as input, reusing the sub-worktree in place so the
                    // partial work is never overwritten. Boxed because `dispatch`
                    // recurses here.
                    info!(
                        "recover_node: {node_id} iter {iter} in {run_id} -> \
                         restart_with_artifacts (harness cannot resume)"
                    );
                    Box::pin(dispatch(
                        state,
                        run_id,
                        RunCommand::RestartNode { node_id, iter },
                    ))
                    .await
                }
                crate::recovery::RecoveryMechanism::Reattach => {
                    // The optimal path: re-attach the SAME session in place, without
                    // re-driving the run. Re-open a parked/terminal Run first, or the
                    // resurrection `NodeStarted` append is refused.
                    let run_state =
                        match crate::embed_reopen_for_targeted_command(&state, &run_id, run_state)
                            .await
                        {
                            Ok(s) => s,
                            Err(e) => {
                                return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}"))
                                    .into_response();
                            }
                        };
                    // Re-read the log after the possible reopen so the resurrection
                    // trace is faithful to the freshest state.
                    let events = match load_events(&state.db, &run_id).await {
                        Ok(e) => e,
                        Err(e) => {
                            return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}"))
                                .into_response();
                        }
                    };
                    let repo_root = effective_repo_root(&state, &run_state);
                    let session_name =
                        tmux_session_manager::node_session_name(&run_id, &node_id, iter);
                    let outcome = crate::reattach_node_session(
                        &state,
                        &events,
                        &run_state,
                        &repo_root,
                        &run_id,
                        &node_id,
                        iter,
                        &session_name,
                    )
                    .await;
                    recover_response(mechanism, &run_id, &node_id, iter, outcome)
                }
            }
        }
        RunCommand::StartNode { node_id, overrides } => {
            // Force-spawn a node out of dependency order. The manager twin of the UI
            // Start button: both funnel through `force_spawn_node`, which derives the
            // iteration and owns the run-status and admission-cap guards. The wire
            // `iter` is dropped at parse time — a manager-pinned iter would fight that
            // derivation.

            // Audit the manager's intent before acting. Only the override port names
            // are logged, not their content, which lands in artifacts.
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
                    "override_ports": overrides
                        .as_ref()
                        .map(|o| o.keys().cloned().collect::<Vec<_>>()),
                })),
            };
            if let Err(e) = append_event(&state, &cmd_event).await {
                error!("failed to append start_node command event: {e}");
            }

            force_spawn_node(&state, &run_id, &node_id, overrides.as_ref()).await
        }
        RunCommand::InjectArtifact { path, content } => {
            // `path` is already proven relative and `..`-free by the parse.
            let requested = std::path::Path::new(&path);

            // NOT `load_projected`, which would turn this arm's 200 into a 404: it is
            // designed to write the artifact a Run has not produced yet, so a missing
            // projection and a DB error both degrade to the daemon's own repo root and
            // the write proceeds.
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
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": format!("error: {e}") })),
                )
                    .into_response();
            }

            // Injecting the artifact a parked node was waiting on embeds the re-open,
            // so the freshly-provided output unblocks the downstream in one
            // round-trip. A live Run is left to its own tick.
            if let Some((_, run_state)) = reload_run_state(&state, &run_id).await {
                let needs_reopen = (run_state.status.is_terminal()
                    && run_state.status != event_log::RunStatus::Archived)
                    || (run_state.status == event_log::RunStatus::AwaitingUser
                        && run_state.awaiting_reason.is_some());
                if needs_reopen {
                    if let Err(e) =
                        crate::embed_reopen_for_targeted_command(&state, &run_id, run_state).await
                    {
                        return (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(serde_json::json!({ "error": format!("error: {e}") })),
                        )
                            .into_response();
                    }
                    re_evaluate_after_command(&state, &run_id).await;
                }
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
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": format!("error: {e}") })),
                )
                    .into_response();
            }

            info!("rename_run: run {run_id} renamed to {:?}", new_name);
            (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
        }
        RunCommand::CleanupRun => cleanup_run(&state, &run_id).await,
        RunCommand::RetryAll => {
            let (events, run_state) = match load_projected(&state, &run_id).await {
                Ok(v) => v,
                Err(resp) => return *resp,
            };

            // Deliberately NOT `RunStatus::is_terminal()`: this set omits `Archived`,
            // whose retry-ability is still an open question.
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
            // Read the RESOLVED repo, not the raw field: an old run carries
            // `target_repo: null`, and forwarding that raw would 400 at the chokepoint
            // AFTER `cleanup_run` archived the original — archived with no replacement.
            let target_repo = Some(
                effective_repo_root(&state, &run_state)
                    .to_string_lossy()
                    .into_owned(),
            );
            let source_branch = run_state.source_branch.clone();
            // Preserve the original Run's read-only secondaries so the retry runs in
            // the same multi-repo context. The primary must sit at [0]; the create
            // chokepoint re-freezes each secondary's SHA against its recorded base
            // branch.
            let target_repos: Vec<TargetRepoInput> = if run_state.target_repos.is_empty() {
                Vec::new()
            } else {
                let mut v = vec![TargetRepoInput {
                    repo: target_repo.clone().unwrap_or_default(),
                    base_branch: source_branch.clone(),
                    // The primary is always writable (ADR-0047).
                    read_only: false,
                }];
                v.extend(run_state.target_repos.iter().map(|p| TargetRepoInput {
                    repo: p.repo.clone(),
                    base_branch: p.base_branch.clone(),
                    // Preserve the read-only opt-in across a retry (ADR-0047).
                    read_only: p.read_only,
                }));
                v
            };

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
                // Preserve the library pipeline id so the retried run stays in the
                // same "by pipeline" stats bucket rather than falling back to the name.
                pipeline_id: run_state.pipeline_id.clone(),
                target_repo,
                target_repos,
                source_branch,
                name: None,
                triggered_by: None,
                // A retry preserves the original Run's isolation mode. Wrapped in
                // `Some` so the chokepoint treats it as EXPLICIT: a changed instance
                // default must never silently re-sandbox (or un-sandbox) a retried Run.
                //
                // `run_state.sandbox_entries` is deliberately NOT forwarded. A retry is
                // a NEW Run with nothing staged, so there is no coherence to protect,
                // and re-resolving is what ADR-0031 §2 requires (a profile edited since
                // must take effect). Do not "fix" this by threading the frozen list: a
                // profile deleted since makes the retry 400 loudly, which is intended.
                sandbox: Some(run_state.sandbox.clone()),
                // A retry must reproduce the original's harness, or an A/B comparison
                // silently reverts to the instance default. `None` (the Run named no
                // harness) forwards as `None`. Unlike `sandbox` this needs no
                // `Some`-wrapping: the create chokepoint freezes `req.harness` verbatim.
                harness: run_state.harness.clone(),
                // Reproduce the original Run's `AgentChoice`, like `harness`: `None`
                // forwards as `None`, so the retry re-resolves through the legacy
                // `harness` tier exactly as the original did.
                agent_choice: run_state.agent_choice.clone(),
                // A retry sets `name: None` and re-derives the name. `Some(true)`
                // reproduces that regardless of the instance default, so a changed
                // `default_auto_name` cannot silently alter how a retried Run is named.
                auto_name: Some(true),
                // Reproduce the original Run's `auto_fail`, like `harness`: `None`
                // forwards as `None` and re-resolves through project / instance.
                auto_fail: run_state.auto_fail,
                // Preserve the explicit Run tier. Instance and Project are resolved
                // afresh for this new Run, while its per-Run override remains stable.
                provisioning: run_state
                    .provisioning_rules
                    .iter()
                    .find(|scoped| scoped.scope == crate::provisioning::ProvisioningScope::Run)
                    .map(|scoped| scoped.rules.clone())
                    .unwrap_or_default(),
            };
            let new_run_resp = create_run_core(&state, new_run_req, Vec::new()).await;

            info!("retry_all: archived run {run_id}, created new run");
            new_run_resp
        }
    }
}

/// The run reached a terminal state during a post-command re-evaluation.
#[derive(Debug, Clone)]
enum ReEvalTerminal {
    Completed,
    Halted(String),
    /// An `unrouted` convergence parked the run `AwaitingUser` — not terminal, but
    /// the re-evaluation had nothing left to dispatch this pass.
    Interrupted(String),
}

/// The real effect of a post-command re-evaluation: which nodes were spawned,
/// which candidate spawns were skipped and why, and whether the run went terminal.
/// Handlers surface this instead of an unconditional `{ok:true}`.
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
            SpawnOutcome::Spawned { .. } => self.spawned.push((node_id.to_string(), iter)),
            SpawnOutcome::Throttled => self.skipped.push(format!(
                "node '{node_id}' iter {iter} throttled into waiting (session cap)"
            )),
            // `Deferred` joins the two rather than getting its own arm: its reason
            // already reads as the operator sentence, and every consumer of `skipped`
            // is a "why did nothing start" message.
            SpawnOutcome::Refused { reason }
            | SpawnOutcome::Deferred { reason }
            | SpawnOutcome::Failed { reason } => self.skipped.push(reason),
        }
    }

    /// The truthful command-response body: spawns happened → `{ok, spawned}`,
    /// nothing launched → `{ok, noop, reason}`.
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
            Some(ReEvalTerminal::Interrupted(msg)) => {
                format!("run interrupted (awaiting user): {msg}")
            }
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

/// Post-command re-evaluation that also re-drives the admission queue when the
/// command drove the run to a **terminal** state.
///
/// A run going terminal stops counting its still-session-holding nodes against the
/// global session cap, so a slot frees — and the site that frees a slot must
/// re-drive the queue, because `retry_waiting_nodes` has no timer of its own and a
/// throttled node in another run would starve. The sweep is global and idempotent.
async fn re_evaluate_after_command(state: &AppState, run_id: &str) -> ReEvalSummary {
    let summary = re_evaluate_after_command_inner(state, run_id).await;
    if summary.terminal.is_some() {
        retry_waiting_nodes(state).await;
    }
    summary
}

/// The re-evaluation proper: load the pipeline and run state, resolve variables
/// (including cycle extensions), then re-evaluate the outgoing edges of every
/// completed node. Returns what actually happened, so handlers can tell the truth.
async fn re_evaluate_after_command_inner(state: &AppState, run_id: &str) -> ReEvalSummary {
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

    // For each extend_cycle command, bump the variables referenced by the target
    // node's outgoing edges.
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

    // A `bump_region` raises the region's effective `max_iter`. When that cap is a
    // `$var`, bumping the variable is what lifts the `iter >= max` exit guard so the
    // region runs the extra laps; a literal cap is the region engine's own bound and
    // reads the recorded route directly.
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

    // Settled-complete nodes whose edges might now fire with updated vars. A
    // `Skipped` node counts too: its edges re-evaluate exactly like a `Completed`
    // node's.
    let completed_node_ids: Vec<String> = run_state
        .nodes
        .values()
        .filter(|n| n.status.is_settled_complete())
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
            // `GuardSuperfluous`: schedule only MISSING work — never a node with a
            // live iteration, never a completed one — on the pass-1 snapshot (INV-2).
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
                ActionOutcome::Interrupted { message } => {
                    summary.terminal = Some(ReEvalTerminal::Interrupted(message));
                    return summary;
                }
            }
        }
    }

    // Pass 1 may have appended events; re-project so pass 2 sees fresh state.
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
        // GuardSuperfluous on the reloaded snapshot (INV-2); source_iter is
        // irrelevant here, since no SwitchRouted is emitted (INV-3).
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
                ActionOutcome::Interrupted { message } => {
                    summary.terminal = Some(ReEvalTerminal::Interrupted(message));
                    return summary;
                }
            }
        }
    }

    for region in pipeline
        .loops
        .iter()
        .filter(|r| r.kind == pipeline::LoopKind::Collection)
    {
        let collection_actions =
            scheduler::evaluate_collection_barrier(&pipeline, &fresh_run_state, region);
        // GuardSuperfluous on the reloaded snapshot; source_iter irrelevant, since
        // no SwitchRouted is emitted.
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
                ActionOutcome::Interrupted { message } => {
                    summary.terminal = Some(ReEvalTerminal::Interrupted(message));
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

    // The pure parse: wire shapes in, verdicts out, no HTTP and no daemon.
    // `#[test]`, not `#[tokio::test]`: `CommandParseError` is `PartialEq`, so no
    // response body has to be read back asynchronously to see what was rejected.

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
    fn set_region_max_iter_parses_region_and_cap() {
        assert_eq!(
            parse(
                serde_json::json!({ "kind": "set_region_max_iter", "region_id": "R", "max_iter": 8 })
            ),
            Ok(RunCommand::SetRegionMaxIter {
                region_id: "R".into(),
                max_iter: 8
            })
        );
    }

    #[test]
    fn set_region_max_iter_reports_region_before_cap() {
        // `{"kind":"set_region_max_iter"}` alone complains about the region first.
        assert_eq!(
            reject(serde_json::json!({ "kind": "set_region_max_iter" })),
            "region_id required for set_region_max_iter"
        );
        assert_eq!(
            reject(serde_json::json!({ "kind": "set_region_max_iter", "region_id": "R" })),
            "max_iter required for set_region_max_iter"
        );
        assert_eq!(
            reject(
                serde_json::json!({ "kind": "set_region_max_iter", "region_id": "R", "max_iter": 0 })
            ),
            "max_iter must be positive"
        );
    }

    #[test]
    fn force_route_parses_from_and_target() {
        assert_eq!(
            parse(serde_json::json!({ "kind": "force_route", "from": "rev", "target": "end" })),
            Ok(RunCommand::ForceRoute {
                from: "rev".into(),
                target: "end".into()
            })
        );
    }

    #[test]
    fn force_route_reports_source_before_target() {
        assert_eq!(
            reject(serde_json::json!({ "kind": "force_route" })),
            "from required for force_route"
        );
        assert_eq!(
            reject(serde_json::json!({ "kind": "force_route", "from": "rev" })),
            "target required for force_route"
        );
    }

    #[test]
    fn skip_node_parses_with_default_iter_and_overrides() {
        assert_eq!(
            parse(serde_json::json!({ "kind": "skip_node", "node_id": "n1" })),
            Ok(RunCommand::SkipNode {
                node_id: "n1".into(),
                iter: 1,
                overrides: None
            })
        );
        let with_ov = parse(serde_json::json!({
            "kind": "skip_node",
            "node_id": "n1",
            "iter": 2,
            "overrides": { "out": "hello" }
        }))
        .expect("valid");
        match with_ov {
            RunCommand::SkipNode {
                node_id,
                iter,
                overrides: Some(ov),
            } => {
                assert_eq!(node_id, "n1");
                assert_eq!(iter, 2);
                assert_eq!(ov.get("out"), Some(&"hello".to_string()));
            }
            other => panic!("unexpected parse: {other:?}"),
        }
    }

    #[test]
    fn skip_node_requires_a_node_id() {
        assert_eq!(
            reject(serde_json::json!({ "kind": "skip_node" })),
            "node_id required for skip_node"
        );
    }

    #[test]
    fn start_node_carries_overrides() {
        let cmd = parse(serde_json::json!({
            "kind": "start_node",
            "node_id": "n1",
            "overrides": { "task": "dummy" }
        }))
        .expect("valid");
        match cmd {
            RunCommand::StartNode {
                node_id,
                overrides: Some(ov),
            } => {
                assert_eq!(node_id, "n1");
                assert_eq!(ov.get("task"), Some(&"dummy".to_string()));
            }
            other => panic!("unexpected parse: {other:?}"),
        }
    }

    #[test]
    fn parse_accepts_the_fourteen_kinds() {
        let cases: [(serde_json::Value, RunCommand); 14] = [
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
                serde_json::json!({ "kind": "recover_node", "node_id": "n1", "iter": 7 }),
                RunCommand::RecoverNode {
                    node_id: "n1".into(),
                    iter: 7,
                },
            ),
            (
                serde_json::json!({ "kind": "start_node", "node_id": "n1" }),
                RunCommand::StartNode {
                    node_id: "n1".into(),
                    overrides: None,
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
        // `start_node` drops the wire `iter` — the server derives it.
        assert_eq!(
            parse(serde_json::json!({ "kind": "start_node", "node_id": "n1", "iter": 99 })),
            Ok(RunCommand::StartNode {
                node_id: "n1".into(),
                overrides: None,
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
        // `kind_str` is total over `RegionAction` precisely because Bump and End are
        // distinguished in the type.
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

    // `into_response_body` is what makes a command answer "here is what actually
    // happened" instead of a blind `{ok:true}`. Promised to the Pipeline Manager in
    // its prompt preamble.

    /// A plain `Spawned`, for tests that only care about the BUCKET `record_spawn`
    /// files an outcome in. The variant's sub-worktree fields are deliberately
    /// ignored by the summary — that detail belongs to `restart_verdict`.
    fn spawned_sample() -> SpawnOutcome {
        SpawnOutcome::Spawned {
            reused_sub_worktree: false,
            base_sha: None,
            interrupted_git_ops: Vec::new(),
        }
    }

    fn summary_with(spawned: &[(&str, i64)], skipped: &[&str]) -> ReEvalSummary {
        let mut s = ReEvalSummary::default();
        for (node_id, iter) in spawned {
            s.record_spawn(node_id, *iter, spawned_sample());
        }
        for reason in skipped {
            s.skipped.push((*reason).to_string());
        }
        s
    }

    #[test]
    fn response_body_reports_spawns_when_anything_launched() {
        let body = summary_with(&[("worker", 2), ("scribe", 1)], &[]).into_response_body();
        assert_eq!(
            body,
            serde_json::json!({
                "ok": true,
                "spawned": [
                    { "node_id": "worker", "iter": 2 },
                    { "node_id": "scribe", "iter": 1 },
                ]
            })
        );
    }

    #[test]
    fn response_body_prefers_spawns_over_a_terminal_verdict() {
        // A run that both spawned and went terminal reports the spawns: the
        // manager needs to know work started, and the terminal state is visible
        // in the projection anyway.
        let mut s = summary_with(&[("worker", 1)], &["something was skipped"]);
        s.terminal = Some(ReEvalTerminal::Completed);
        let body = s.into_response_body();
        assert_eq!(body["spawned"][0]["node_id"], "worker");
        assert!(body.get("noop").is_none(), "got {body}");
    }

    #[test]
    fn response_body_explains_every_flavour_of_nothing_happened() {
        for (summary, want_reason) in [
            (summary_with(&[], &[]), "no eligible spawn"),
            (
                summary_with(&[], &["worker iter 2 throttled", "scribe refused"]),
                "worker iter 2 throttled; scribe refused",
            ),
            (
                {
                    let mut s = summary_with(&[], &["ignored once terminal"]);
                    s.terminal = Some(ReEvalTerminal::Completed);
                    s
                },
                "run completed",
            ),
            (
                {
                    let mut s = summary_with(&[], &[]);
                    s.terminal = Some(ReEvalTerminal::Halted("region exhausted".into()));
                    s
                },
                "run halted: region exhausted",
            ),
        ] {
            let body = summary.into_response_body();
            assert_eq!(
                body,
                serde_json::json!({ "ok": true, "noop": true, "reason": want_reason })
            );
        }
    }

    #[test]
    fn record_spawn_maps_every_outcome_to_the_right_bucket() {
        // The three non-`Spawned` outcomes all read as "why nothing started", which
        // is why `Deferred` joins `Refused`/`Failed` rather than getting its own arm.
        let mut s = ReEvalSummary::default();
        s.record_spawn("a", 1, spawned_sample());
        s.record_spawn("b", 4, SpawnOutcome::Throttled);
        s.record_spawn(
            "c",
            1,
            SpawnOutcome::Refused {
                reason: "refused c".into(),
            },
        );
        s.record_spawn(
            "d",
            1,
            SpawnOutcome::Deferred {
                reason: "deferred d".into(),
            },
        );
        s.record_spawn(
            "e",
            1,
            SpawnOutcome::Failed {
                reason: "failed e".into(),
            },
        );

        assert_eq!(s.spawned, vec![("a".to_string(), 1)]);
        assert_eq!(
            s.skipped,
            vec![
                "node 'b' iter 4 throttled into waiting (session cap)".to_string(),
                "refused c".to_string(),
                "deferred d".to_string(),
                "failed e".to_string(),
            ]
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
                isolated_worktree: None,
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
                    instructions: None,
                    required: false,
                }],
                outputs: vec![Port {
                    name: "pass".into(),
                    repeated: false,
                    side: None,
                    port_type: PortType::Markdown,
                    frontmatter: None,
                    when: Some(serde_yaml::from_str("iter: { lt: \"$max_iter_review\" }").unwrap()),
                    description: None,
                    instructions: None,
                    required: false,
                }],
                interactive: false,
                view: None,
                max_iter: None,
                over: None,
                pin_harness: None,
                harnesses: Default::default(),
                agent_choice: None,
                auto_fail: None,
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
                isolated_worktree: None,
                id: "b".into(),
                name: "b".into(),
                node_type: NodeType::Agent,
                inputs: vec![Port {
                    name: "in".into(),
                    repeated: false,
                    side: None,
                    port_type: PortType::Markdown,
                    frontmatter: None,
                    when: None,
                    description: None,
                    instructions: None,
                    required: false,
                }],
                outputs: vec![],
                interactive: false,
                view: None,
                max_iter: None,
                over: None,
                pin_harness: None,
                harnesses: Default::default(),
                agent_choice: None,
                auto_fail: None,
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
