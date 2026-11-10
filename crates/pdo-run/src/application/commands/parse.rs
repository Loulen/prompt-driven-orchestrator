//! The wire shape of a command and its validation: the only way to build a
//! [`RunCommand`]. **Pure** — no store, no filesystem, no clock.

use super::*;

/// The wire shape of a command. `pub(crate)` only because axum's extractor puts
/// this type in the handler's public signature; its fields stay private.
#[derive(Deserialize)]
pub struct RunCommandRequest {
    pub(super) kind: String,
    #[serde(default)]
    pub(super) node_id: Option<String>,
    #[serde(default)]
    pub(super) iter: Option<i64>,
    #[serde(default)]
    pub(super) additional_iter: Option<i64>,
    /// The loop region a `bump_region` / `end_region` / `set_region_max_iter`
    /// targets.
    #[serde(default)]
    pub(super) region_id: Option<String>,
    /// The **absolute** iteration cap of a `set_region_max_iter` — the total laps
    /// the region now allows, not a delta like `additional_iter`.
    #[serde(default)]
    pub(super) max_iter: Option<i64>,
    /// The **absolute** cost cap of a `set_region_max_cost`, in dollars. Outer
    /// `None` = the field is absent (refused); `Some(Null)` = remove the cap.
    /// Kept as a raw JSON value so a non-numeric cap answers a 400 in our
    /// words, not axum's 422.
    #[serde(default, deserialize_with = "super::parse_cost_cap::present_value")]
    pub(super) max_cost_usd: Option<serde_json::Value>,
    /// The **source** of a `force_route`: a node id OR a region id whose `when:`
    /// edges are short-circuited. Separate from `node_id`/`region_id` so it can name
    /// either kind without overloading their meaning.
    #[serde(default)]
    pub(super) from: Option<String>,
    /// The **target** node a `force_route` exits to.
    #[serde(default)]
    pub(super) target: Option<String>,
    /// Per-port overrides for a `start_node`, or the default outputs of a
    /// `skip_node`. Keyed by port name; each value is **inline content** written to
    /// that port's artifact before the node runs, so an operator can drive a node
    /// without its upstream having produced.
    #[serde(default)]
    pub(super) overrides: Option<HashMap<String, String>>,
    #[serde(default)]
    pub(super) path: Option<String>,
    #[serde(default)]
    pub(super) content: Option<String>,
    #[serde(default)]
    pub(super) name: Option<String>,
}

/// A command from the Pipeline Manager, already validated. The only way to build
/// one is [`parse_run_command`], so no arm re-validates presence, applies a
/// default, or re-inspects a path.
///
/// One variant per accepted `kind`, except `bump_region`/`end_region`, which share
/// [`RunCommand::Region`] because they share their whole I/O tail.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum RunCommand {
    MarkNodeDone {
        node_id: String,
        iter: i64,
    },
    ReleaseNodeCompletion {
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
    /// Edit a bounded region's **Plafond de coût** in flight, absolute, in
    /// dollars; `None` removes the cap. No floor: a cap under the spend stops
    /// the loop at its next re-entry, the lap in progress always finishes.
    SetRegionMaxCost {
        region_id: String,
        max_cost_usd: Option<f64>,
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
    /// Give up on a live Run: no more launches, every session stopped, the Run
    /// `halted` « cancelled » (terminal, so retried or reopened).
    CancelRun,
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
pub(super) enum RegionAction {
    Bump { additional_iter: i64 },
    End,
}

/// Rejected at parse time. Every rejection site answers `400` + `Json({"error":
/// …})`, so the status is not carried here — add it the day one stops being a 400.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CommandParseError(pub(super) String);

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
    /// The Operation the command asks the Autorisation d'opération about: a
    /// node command targets its node, every other command the Run.
    pub(super) fn operation(&self, run_id: &str) -> pdo_kernel::Operation {
        use crate::application::operations::{on_node, on_run};
        match self.target_node() {
            Some(node_id) => on_node(self.kind_str(), run_id, node_id),
            None => on_run(self.kind_str(), run_id),
        }
    }

    /// The node a node command targets; `None` for a command on the Run.
    pub(super) fn target_node(&self) -> Option<&str> {
        match self {
            RunCommand::MarkNodeDone { node_id, .. }
            | RunCommand::ReleaseNodeCompletion { node_id, .. }
            | RunCommand::ExtendCycle { node_id, .. }
            | RunCommand::SkipNode { node_id, .. }
            | RunCommand::KillNode { node_id, .. }
            | RunCommand::RestartNode { node_id, .. }
            | RunCommand::RecoverNode { node_id, .. }
            | RunCommand::StartNode { node_id, .. } => Some(node_id),
            _ => None,
        }
    }

    /// The wire `kind` this command came from. Load-bearing: the region arm logs it
    /// after an `.await` (the request is long gone by then), and every
    /// `CommandIssued` payload carries it as `"command"`.
    pub(super) fn kind_str(&self) -> &'static str {
        match self {
            RunCommand::MarkNodeDone { .. } => "mark_node_done",
            RunCommand::ReleaseNodeCompletion { .. } => "release_node_completion",
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
            RunCommand::SetRegionMaxCost { .. } => "set_region_max_cost",
            RunCommand::ForceRoute { .. } => "force_route",
            RunCommand::SkipNode { .. } => "skip_node",
            RunCommand::PauseRun => "pause_run",
            RunCommand::ResumeRun => "resume_run",
            RunCommand::CancelRun => "cancel_run",
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
pub(super) fn parse_run_command(req: RunCommandRequest) -> Result<RunCommand, CommandParseError> {
    pub(super) fn required(
        value: Option<String>,
        field: &str,
        kind: &str,
    ) -> Result<String, CommandParseError> {
        value.ok_or_else(|| CommandParseError(format!("{field} required for {kind}")))
    }
    pub(super) fn positive_iter(value: Option<i64>, kind: &str) -> Result<i64, CommandParseError> {
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
        "release_node_completion" => Ok(RunCommand::ReleaseNodeCompletion {
            node_id: required(req.node_id, "node_id", "release_node_completion")?,
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
        "set_region_max_cost" => Ok(RunCommand::SetRegionMaxCost {
            // `region_id` first, then the cap.
            region_id: required(req.region_id, "region_id", "set_region_max_cost")?,
            max_cost_usd: super::parse_cost_cap::cost_cap(req.max_cost_usd)?,
        }),
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
        "cancel_run" => Ok(RunCommand::CancelRun),
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
