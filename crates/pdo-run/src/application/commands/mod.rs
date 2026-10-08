//! `RunCommand` (`POST /runs/{id}/commands`): the Pipeline Manager's command
//! surface — the parse ([`parse`]), one file per family of commands, and the
//! post-command re-evaluation they drive ([`reeval`], [`reeval_pass`]).
//!
//! Layer 3 in ADR-0009 terms. Its sibling surface is the per-node route family
//! `POST /runs/{id}/nodes/{node_id}/{start,stop,retry}` (the canvas buttons).

use crate::domain::{event, projection, state};
use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use pdo_kernel::Actor;
use serde::Deserialize;
use tracing::{error, info, warn};

mod completion;
mod lifecycle;
mod loops;
mod nodes;
mod parse;
mod recover;
mod reeval;
mod reeval_pass;
mod restart;
mod routing;
mod skip;

pub use parse::RunCommandRequest;
use parse::*;
use reeval::*;
use reeval_pass::*;

use crate::application::complete_node::completion_head_gate;
use crate::application::events::append_event;
use crate::application::events::load_events;
use crate::application::events::load_projected;
use crate::application::events::reload_run_state;
use crate::application::events::run_is_forgotten;
use crate::application::node::output_validation::check_output_validation_with_retry;
use crate::application::node::reap::reap_node_session;
use crate::application::node::spawn::{spawn_node, SpawnContext, SpawnDeps, SpawnOutcome};
use crate::application::paths::effective_repo_root;
use crate::application::paths::resolve_pipeline_path;
use crate::application::paths::resolve_run_pipeline_path;
use crate::application::paths::run_scoped_pipeline_path;
use crate::application::sandbox as run_sandbox;
use crate::application::sandbox_prep::mark_sandbox_prep_ready;
use crate::application::scheduler::advance as run_advance;
use crate::application::scheduler::frontmatter::resolve_completed_frontmatter;
use crate::application::scheduler::frontmatter::resolve_run_variables;
use crate::application::scheduler::frontmatter::resolve_source_frontmatter;
use crate::application::scheduler::interpreter as scheduler_interpreter;
use crate::application::scheduler::interpreter::{ActionOutcome, SpawnDedup};
use crate::application::scheduler::node_completion::retry_waiting_nodes;
use crate::application::start_node::force_spawn_node;
use crate::domain::loop_region;
use crate::domain::restart_verdict;
use crate::domain::scheduler;
use crate::domain::transition_guard;
use crate::Runs;
use pdo_pipeline::domain::pipeline;
use pdo_workspace::adapters::git::worktrees::worktree_dir_for_run;

/// `RunCommand`: the Pipeline Manager (or the operator through the UI) sends
/// one command on a Run.
impl Runs {
    /// The forgotten-run gate first (ADR-0024), then the parse, then the
    /// Autorisation d'opération on what the command targets — a node for the
    /// node commands, the Run for the others — and only then the command.
    pub async fn run_command(
        self: &Arc<Self>,
        actor: &Actor,
        run_id: String,
        request: RunCommandRequest,
    ) -> Response {
        // ADR-0024: a forgotten run accepts no commands. Reject before any arm can
        // append (extend_cycle appends CommandIssued before its own existence
        // check) or trigger side effects.
        match run_is_forgotten(self, &run_id).await {
            Ok(true) => {
                return (
                    StatusCode::GONE,
                    Json(
                        serde_json::json!({ "error": format!("run {run_id} has been forgotten") }),
                    ),
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

        // Validation runs AFTER the 410 gate: a malformed command against a
        // forgotten run answers 410, not 400, and that precedence belongs to
        // ADR-0024.
        let cmd = match parse_run_command(request) {
            Ok(cmd) => cmd,
            Err(e) => return e.into_response(),
        };
        if let Err(denied) = self.authorize(actor, &cmd.operation(&run_id)).await {
            return denied.into_response();
        }

        dispatch(self.clone(), run_id, cmd).await
    }
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
/// worktrees and Run creation, so it takes the whole `Runs`. Dependency
/// injection à la `SpawnDeps` is reserved for leaves (`node_spawn`).
async fn dispatch(state: Arc<Runs>, run_id: String, cmd: RunCommand) -> Response {
    // Read before the match consumes `cmd`: the region arm logs its wire kind after
    // two `.await`s.
    let kind_str = cmd.kind_str();

    match cmd {
        RunCommand::ReleaseNodeCompletion { node_id, iter } => {
            crate::application::release_completion::release_node_completion(
                &state, &run_id, &node_id, iter,
            )
            .await
        }
        RunCommand::MarkNodeDone { node_id, iter } => {
            completion::mark_node_done(state, run_id, node_id, iter).await
        }
        RunCommand::ExtendCycle {
            node_id,
            additional_iter,
        } => loops::extend_cycle(state, run_id, node_id, additional_iter).await,
        RunCommand::Region { region_id, action } => {
            loops::region(state, run_id, region_id, action, kind_str).await
        }
        RunCommand::SetRegionMaxIter {
            region_id,
            max_iter,
        } => loops::set_region_max_iter(state, run_id, region_id, max_iter).await,
        RunCommand::ForceRoute { from, target } => {
            routing::force_route(state, run_id, from, target).await
        }
        RunCommand::SkipNode {
            node_id,
            iter,
            overrides,
        } => skip::skip_node(state, run_id, node_id, iter, overrides).await,
        RunCommand::PauseRun => lifecycle::pause_run(state, run_id).await,
        RunCommand::ResumeRun => lifecycle::resume_run(state, run_id).await,
        RunCommand::ReopenRun => lifecycle::reopen_run(state, run_id).await,
        RunCommand::KillNode { node_id, iter } => {
            nodes::kill_node(state, run_id, node_id, iter).await
        }
        RunCommand::RestartNode { node_id, iter } => {
            restart::restart_node(state, run_id, node_id, iter).await
        }
        RunCommand::RecoverNode { node_id, iter } => {
            recover::recover_node(state, run_id, node_id, iter).await
        }
        RunCommand::StartNode { node_id, overrides } => {
            nodes::start_node(state, run_id, node_id, overrides).await
        }
        RunCommand::InjectArtifact { path, content } => {
            lifecycle::inject_artifact(state, run_id, path, content).await
        }
        RunCommand::RenameRun { name: new_name } => {
            lifecycle::rename_run(state, run_id, new_name).await
        }
        // Archiving a Run and retrying it whole answer as the Run's surfaces
        // do: their answers are forwarded verbatim.
        RunCommand::CleanupRun => crate::application::archive::cleanup_run(&state, &run_id).await,
        RunCommand::RetryAll => crate::application::retry_all::retry_all_run(&state, &run_id).await,
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
    fn parse_accepts_the_fifteen_kinds() {
        let cases: [(serde_json::Value, RunCommand); 15] = [
            (
                serde_json::json!({ "kind": "mark_node_done", "node_id": "n1", "iter": 3 }),
                RunCommand::MarkNodeDone {
                    node_id: "n1".into(),
                    iter: 3,
                },
            ),
            (
                serde_json::json!({ "kind": "release_node_completion", "node_id": "n1", "iter": 3 }),
                RunCommand::ReleaseNodeCompletion {
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
                serde_json::json!({ "kind": "release_node_completion" }),
                "node_id required for release_node_completion",
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
            "release_node_completion",
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
        use pdo_pipeline::domain::pipeline::*;

        let pipeline = PipelineDef {
            name: "test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![NodeDef {
                skills: Vec::new(),
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
                    intermediate: false,
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
                    intermediate: false,
                }],
                interactive: false,
                view: None,
                max_iter: None,
                over: None,
                pin_harness: None,
                harnesses: Default::default(),
                agent_choice: None,
                auto_fail: None,
                orchestrator: false,
            }],
            edges: vec![EdgeDef {
                source: EdgeSource::single("reviewer", "review"),
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
            grid_size: None,
        };

        let refs = extract_variable_refs_from_outgoing_edges(&pipeline, "reviewer");
        assert_eq!(refs, vec!["max_iter_review"]);
    }

    #[test]
    fn extract_var_refs_empty_for_no_vars() {
        use pdo_pipeline::domain::pipeline::*;

        let pipeline = PipelineDef {
            name: "test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![NodeDef {
                skills: Vec::new(),
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
                    intermediate: false,
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
                orchestrator: false,
            }],
            edges: vec![EdgeDef {
                source: EdgeSource::single("a", "out"),
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
            grid_size: None,
        };

        let refs = extract_variable_refs_from_outgoing_edges(&pipeline, "a");
        assert!(refs.is_empty());
    }
}
