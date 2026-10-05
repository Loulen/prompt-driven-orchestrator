//! The session context's Run-side ports (the declared wait's facts, a node's
//! pane, the run shell, the sweep's owners, the status bar's gauges, the
//! templates the library assistant writes, the harness it launches on),
//! implemented over the run context, the platform's service health and the
//! pipeline registry on disk.

mod daemon;
mod owners;
mod runs;

use std::path::PathBuf;
use std::sync::Arc;

use pdo_pipeline::adapters::self_writes::SelfWrites;
use pdo_platform::Platform;
use pdo_run::domain::event::{Event, EventKind};
use pdo_run::domain::projection::project;
use pdo_run::domain::state::RunState;
use pdo_run::Runs;
use pdo_session::application::ports::{
    AssistantHarness, DaemonGauges, NodePanes, NodeWaits, RunLookup, RunProjects, RunShells,
    SessionOwners, Templates,
};
use tokio::sync::broadcast;

use super::Late;

/// Every Run-side port of the session context. The session context is wired
/// before the Runs and the platform, which it reaches through [`Late`].
pub(crate) struct RunSide {
    pub(crate) runs: Late<Runs>,
    pub(crate) platform: Late<Platform>,
    pub(crate) db: sqlx::SqlitePool,
    /// The daemon's repo root, where the templates land.
    pub(crate) repo_root: PathBuf,
    /// The daemon's own writes, which the pipeline watcher does not announce.
    pub(crate) self_writes: SelfWrites,
    /// The raw channel of the kernel's bus, where a template write is announced.
    pub(crate) bus_tx: broadcast::Sender<serde_json::Value>,
    /// The `docker` binary a sandboxed run shell execs into.
    pub(crate) docker_cmd_override: Option<String>,
}

/// The session context's Run-side ports, one handle each.
#[derive(Clone)]
pub struct SessionRunPorts {
    pub waits: Arc<dyn NodeWaits>,
    pub panes: Arc<dyn NodePanes>,
    pub shells: Arc<dyn RunShells>,
    pub owners: Arc<dyn SessionOwners>,
    pub gauges: Arc<dyn DaemonGauges>,
    pub templates: Arc<dyn Templates>,
    pub assistant_harness: Arc<dyn AssistantHarness>,
    pub projects: Arc<dyn RunProjects>,
}

impl SessionRunPorts {
    pub(crate) fn over(side: RunSide) -> Self {
        let ports = Arc::new(side);
        Self {
            waits: ports.clone(),
            panes: ports.clone(),
            shells: ports.clone(),
            owners: ports.clone(),
            gauges: ports.clone(),
            templates: ports.clone(),
            projects: ports.clone(),
            assistant_harness: ports,
        }
    }
}

impl RunSide {
    async fn events(&self, run_id: &str) -> anyhow::Result<Vec<Event>> {
        pdo_run::adapters::sqlite_event_store::load(&self.db, run_id).await
    }

    async fn projected(
        &self,
        run_id: &str,
    ) -> Result<(Arc<Runs>, Vec<Event>, RunState), RunLookup> {
        let runs = self.runs.get().map_err(RunLookup::Storage)?;
        let events = self
            .events(run_id)
            .await
            .map_err(|e| RunLookup::Storage(e.to_string()))?;
        let run_state = project(&events).ok_or(RunLookup::NotFound)?;
        Ok((runs, events, run_state))
    }

    async fn record(
        &self,
        run_id: &str,
        node_id: &str,
        iter: i64,
        kind: EventKind,
        payload: serde_json::Value,
    ) -> Result<(), String> {
        let runs = self.runs.get()?;
        let event = Event {
            id: None,
            run_id: run_id.to_string(),
            ts: pdo_kernel::clock::now_iso(),
            kind,
            node_id: Some(node_id.to_string()),
            iter: Some(iter),
            payload: Some(payload),
        };
        pdo_run::application::events::append_event(&runs, &event)
            .await
            .map(|_| ())
            .map_err(|e| format!("{e:#}"))
    }
}

/// The session context over these ports, wired as the daemon wires it: the
/// declared wait, a node's pane, the status bar's gauges and the sweep, over
/// the run context's real log (stage 3).
#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use pdo_platform::domain::service_health::ServiceHealthOverride;
    use pdo_run::application::complete_node::{
        complete_node_iteration, CompletionAttempt, CompletionSource,
    };
    use pdo_run::application::release_completion::release_node_completion;
    use pdo_run::application::reopen::embed_reopen_for_targeted_command;
    use pdo_run::application::scheduler::advance as run_advance;
    use pdo_run::domain::admission;
    use pdo_run::domain::completion_refusal;
    use pdo_workspace::adapters::git::worktrees::worktree_dir_for_run;
    use tower::util::ServiceExt;

    use crate::contexts::testing::{TestContexts, TestOptions};

    /// A Run's events, its state and their projection.
    mod event_log {
        pub(super) use pdo_run::domain::event::*;
        pub(super) use pdo_run::domain::projection::*;
        pub(super) use pdo_run::domain::state::*;
    }

    /// The session context declares a wait with its own causes; the Run's
    /// projection folds them under the same names.
    #[test]
    fn the_wait_causes_are_the_ones_the_projection_folds() {
        use pdo_session::domain::declared_wait::{CAUSE_COMPLETION_NOT_RELEASED, CAUSE_DECLARED};
        assert_eq!(CAUSE_DECLARED, event_log::AWAITING_CAUSE_DECLARED);
        assert_eq!(
            CAUSE_COMPLETION_NOT_RELEASED,
            event_log::AWAITING_CAUSE_COMPLETION_NOT_RELEASED
        );
    }

    /// A tmux for the unit tests of the run side: every session "exists", nothing
    /// runs. The declared wait asks tmux whether the node's session is up, and the
    /// runs these tests seed have no real session behind them.
    struct AliveTmux;

    impl pdo_session::application::ports::Tmux for AliveTmux {
        fn socket(&self) -> &str {
            "pdo-test-alive"
        }
        fn session_path(&self) -> String {
            String::new()
        }
        fn new_session(
            &self,
            _name: &str,
            _working_dir: &Path,
            _script: &str,
            _what: Option<&str>,
        ) -> anyhow::Result<()> {
            Ok(())
        }
        fn kill(&self, _name: &str) {}
        fn session_exists(&self, _name: &str) -> bool {
            true
        }
        fn server_alive(&self) -> Option<bool> {
            Some(true)
        }
        fn list_pdo_sessions(&self) -> std::collections::HashSet<String> {
            Default::default()
        }
        fn is_attached(&self, _name: &str) -> bool {
            false
        }
        fn capture(&self, _name: &str) -> Option<String> {
            None
        }
        fn send_keys(&self, _name: &str, _text: &str) {}
        fn paste(&self, _name: &str, _text: &str) {}
        fn attach_argv(&self, _name: &str, _ignore_size: bool) -> Vec<String> {
            Vec::new()
        }
        fn attach_command(&self, _name: &str) -> String {
            String::new()
        }
        fn set_client_ignore_size(&self, _client_pid: u32, _ignore_size: bool) -> bool {
            false
        }
    }

    async fn with_alive_sessions() -> TestContexts {
        TestContexts::with(TestOptions {
            tmux: Some(Arc::new(AliveTmux)),
            ..TestOptions::default()
        })
        .await
    }

    /// DOCTRINE INVARIANT (ADR-0012(a)): the reaper sweep kills tmux sessions
    /// but MUST NEVER delete a run's worktree or branches, nor archive the run.
    /// This pins the verdict of the #128 grilling — a future change that makes
    /// the runtime auto-delete will turn this test red.
    #[tokio::test]
    async fn reaper_never_deletes_worktree() {
        let t = TestContexts::new().await;
        let run_id = "doctrine-completed";
        seed_run_with_run_events(&t, run_id, "p", &[event_log::EventKind::RunCompleted]).await;
        mk_run_worktree_dir(t.repo_root(), run_id);
        let worktree = t
            .repo_root()
            .join(".pdo")
            .join("runs")
            .join(run_id)
            .join("worktree");
        assert!(worktree.exists(), "precondition: worktree on disk");

        // Run the reaper with maximum aggression (ttl=0) against the test daemon's
        // tmux socket, where no server runs (no sessions to kill — we only care that
        // the sweep leaves the disk and run status untouched).
        t.contexts
            .session
            .sweep()
            .run_with(pdo_session::domain::reaper::SweepTtls {
                node_run: std::time::Duration::ZERO,
                libassist_idle: std::time::Duration::ZERO,
            })
            .await
            .unwrap();

        assert!(
            worktree.exists(),
            "DOCTRINE REGRESSION: reaper deleted the worktree"
        );
        let events = t.events(run_id).await;
        let run_state = event_log::project(&events).unwrap();
        assert_eq!(
            run_state.status,
            event_log::RunStatus::Completed,
            "DOCTRINE REGRESSION: reaper archived the run on its own"
        );
    }

    #[tokio::test]
    async fn sessions_endpoint_reports_live_count_and_cap() {
        // The status-bar counter reads `GET /sessions`: the live
        // NodeRun-session count and the configured cap. A single running node
        // counts as one live session.
        let t = TestContexts::new().await;
        let run_id = "sessions-run";
        t.append(&event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({ "pipeline_name": "test" })),
        })
        .await;
        t.append(&event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::NodeStarted,
            node_id: Some("worker".into()),
            iter: Some(1),
            payload: None,
        })
        .await;

        let status = t.contexts.session.read_status().execute().await;
        assert_eq!(status.live, 1, "one running node = one live session");
        assert!(status.cap >= 1, "cap should be a positive integer");
        // #485 / ADR-0038: the orphan-sweep gauge rides the same payload. This
        // daemon has never swept, so the timestamp is null and both counters are
        // zero (the moving parts are pinned by
        // `tests/tmux_lifecycle.rs::genuine_orphans_are_still_reaped_and_counted`,
        // the payload over HTTP by `tests/composition_root.rs`).
        assert_eq!(status.last_sweep_at, None);
        assert_eq!((status.killed, status.killed_for_absent_run), (0, 0));
    }

    #[tokio::test]
    async fn sessions_endpoint_reports_cached_service_health() {
        // #156 (D5): `GET /sessions` carries the boot-cached `service` object.
        // Here it is ephemeral (reachable but not installed as a unit) — the
        // exact state that drives the status bar's amber `ephemeral` pill.
        let t = TestContexts::with(TestOptions {
            service_health: Some(ServiceHealthOverride::Ephemeral),
            ..TestOptions::default()
        })
        .await;
        let json = t.contexts.session.read_status().execute().await;
        assert_eq!(
            json.service["supervisor"],
            t.contexts.platform.service_health().supervisor.as_str(),
            "the service supervisor hint is surfaced verbatim from the platform"
        );
        assert_eq!(
            json.service["persistent"], false,
            "persistent:false is the reachable-but-ephemeral signal the pill consumes"
        );
    }

    #[tokio::test]
    async fn sessions_service_persistent_is_null_when_unknown() {
        // The neutral default (non-Linux / no systemd / spawn failure) serializes
        // `persistent: null` — the UI silences on null exactly as it does on true.
        let t = TestContexts::with(TestOptions {
            service_health: Some(ServiceHealthOverride::Unknown),
            ..TestOptions::default()
        })
        .await;
        let json = t.contexts.session.read_status().execute().await;
        assert!(
            json.service["persistent"].is_null(),
            "unknown persistence must serialize as null, not an error or false"
        );
    }

    #[tokio::test]
    async fn sessions_endpoint_excludes_session_holding_node_of_a_terminal_run() {
        // #215 repro at the HTTP boundary: a run fails (fail-fast) while a
        // sibling node is still projected Running. `GET /sessions` must report
        // `live: 0`, not leak a phantom slot. This is the exact symptom the
        // issue filed (`/sessions` returned `live:1` with no live tmux).
        let t = TestContexts::new().await;
        let run_id = "sessions-terminal-run";
        for event in [
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event_log::EventKind::RunStarted,
                node_id: None,
                iter: None,
                payload: Some(serde_json::json!({ "pipeline_name": "test" })),
            },
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event_log::EventKind::NodeStarted,
                node_id: Some("worker".into()),
                iter: Some(1),
                payload: None,
            },
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event_log::EventKind::RunFailed,
                node_id: None,
                iter: None,
                payload: Some(serde_json::json!({ "reason": "fail-fast" })),
            },
        ] {
            t.append(&event).await;
        }

        let status = t.contexts.session.read_status().execute().await;
        assert_eq!(
            status.live, 0,
            "a session-holding node in a terminal run must not count (#215)"
        );
    }

    async fn seed_started_node(t: &TestContexts, run_id: &str, interactive: bool) {
        for event in [
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event_log::EventKind::RunStarted,
                node_id: None,
                iter: None,
                payload: Some(serde_json::json!({
                    "pipeline_name": "declared",
                    "node_defs": [
                        { "id": "griller", "node_type": "agent", "inputs": [], "outputs": [] },
                        { "id": "runner", "node_type": "script", "inputs": [], "outputs": [] },
                        { "id": "later", "node_type": "agent", "inputs": [], "outputs": [] }
                    ]
                })),
            },
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event_log::EventKind::NodeStarted,
                node_id: Some("griller".into()),
                iter: Some(1),
                payload: Some(serde_json::json!({ "interactive": interactive })),
            },
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event_log::EventKind::NodeStarted,
                node_id: Some("runner".into()),
                iter: Some(1),
                payload: None,
            },
        ] {
            t.append(&event).await;
        }
    }

    /// Declare the wait through the session context's use case, over this
    /// crate's Run ports — what `POST …/wait-user` runs.
    async fn post_wait_user(
        t: &TestContexts,
        run_id: &str,
        node_id: &str,
        body: serde_json::Value,
    ) -> Result<
        pdo_session::domain::declared_wait::DeclareOutcome,
        pdo_session::application::declared_wait::DeclareWaitError,
    > {
        t.contexts
            .session
            .declare_wait()
            .execute(
                &pdo_kernel::Actor::LocalOperator,
                pdo_session::application::declared_wait::DeclareWaitCommand {
                    run_id: run_id.into(),
                    node_id: node_id.into(),
                    iter: body["iter"].as_i64().unwrap_or(1),
                    message: body["message"].as_str().map(str::to_string),
                },
            )
            .await
    }

    /// The bytes a browser typed into `session`, as the PTY bridge hands them over.
    async fn note_pty_input(t: &TestContexts, session: &str, bytes: &[u8]) {
        t.contexts
            .session
            .terminals()
            .note_input(&pdo_kernel::Actor::LocalOperator, session, bytes)
            .await;
    }

    fn wait_refusal(
        result: Result<
            pdo_session::domain::declared_wait::DeclareOutcome,
            pdo_session::application::declared_wait::DeclareWaitError,
        >,
    ) -> &'static str {
        match result {
            Err(pdo_session::application::declared_wait::DeclareWaitError::Wait(r)) => r.slug(),
            other => panic!("expected a named refusal, got {other:?}"),
        }
    }

    async fn projected(t: &TestContexts, run_id: &str) -> event_log::RunState {
        event_log::project(&t.events(run_id).await).unwrap()
    }

    #[tokio::test]
    async fn wait_user_declares_the_wait_with_the_message_as_run_reason_and_no_code() {
        let t = with_alive_sessions().await;
        let run_id = "declared-wait-message";
        seed_started_node(&t, run_id, true).await;
        // A fresh interactive node is NOT awaiting (ADR-0069: nothing at spawn).
        let before = projected(&t, run_id).await;
        assert_eq!(before.status, event_log::RunStatus::Running);
        assert_eq!(
            before.nodes["griller"].status,
            event_log::NodeStatus::Running
        );

        let outcome = post_wait_user(
            &t,
            run_id,
            "griller",
            serde_json::json!({ "iter": 1, "message": "Which layout: strip or card?" }),
        )
        .await;
        assert_eq!(
            outcome,
            Ok(pdo_session::domain::declared_wait::DeclareOutcome::Declared)
        );

        let after = projected(&t, run_id).await;
        assert_eq!(after.status, event_log::RunStatus::AwaitingUser);
        assert_eq!(
            after.awaiting_reason.as_deref(),
            Some("Which layout: strip or card?")
        );
        assert!(
            after.awaiting_reason_code.is_none(),
            "a declared wait has no code"
        );
        assert!(after.is_declared_wait());
        let node = &after.nodes["griller"];
        assert_eq!(node.status, event_log::NodeStatus::AwaitingUser);
        let info = node.awaiting.as_ref().expect("awaiting info");
        assert_eq!(info.cause, event_log::AWAITING_CAUSE_DECLARED);
        assert_eq!(
            info.message.as_deref(),
            Some("Which layout: strip or card?")
        );

        // Same message again: a legal no-op, nothing appended.
        let events_before = t.events(run_id).await.len();
        let outcome = post_wait_user(
            &t,
            run_id,
            "griller",
            serde_json::json!({ "iter": 1, "message": "Which layout: strip or card?" }),
        )
        .await;
        assert_eq!(
            outcome,
            Ok(pdo_session::domain::declared_wait::DeclareOutcome::Noop)
        );
        assert_eq!(t.events(run_id).await.len(), events_before);

        // A new message refreshes the banner.
        let outcome = post_wait_user(
            &t,
            run_id,
            "griller",
            serde_json::json!({ "iter": 1, "message": "Dark theme too?" }),
        )
        .await;
        assert_eq!(
            outcome,
            Ok(pdo_session::domain::declared_wait::DeclareOutcome::Refreshed)
        );
        let refreshed = projected(&t, run_id).await;
        assert_eq!(
            refreshed.awaiting_reason.as_deref(),
            Some("Dark theme too?")
        );

        // A message over the cap is truncated by the daemon too.
        let long = "z".repeat(300);
        let _ = post_wait_user(
            &t,
            run_id,
            "griller",
            serde_json::json!({ "iter": 1, "message": long }),
        )
        .await;
        let capped = projected(&t, run_id).await;
        assert_eq!(
            capped.awaiting_reason.as_deref().unwrap().chars().count(),
            pdo_session::domain::declared_wait::MESSAGE_MAX_CHARS
        );
    }

    #[tokio::test]
    async fn wait_user_bare_is_accepted_on_a_non_interactive_node_and_refused_by_name() {
        let t = with_alive_sessions().await;
        let run_id = "declared-wait-refusals";
        seed_started_node(&t, run_id, false).await;

        // Bare, on a plain (non-interactive) agent node: accepted, no message.
        let outcome = post_wait_user(&t, run_id, "griller", serde_json::json!({ "iter": 1 })).await;
        assert_eq!(
            outcome,
            Ok(pdo_session::domain::declared_wait::DeclareOutcome::Declared)
        );
        let rs = projected(&t, run_id).await;
        assert_eq!(rs.status, event_log::RunStatus::AwaitingUser);
        assert!(rs.awaiting_reason.is_none(), "no message ⇒ no prose reason");
        assert!(rs.awaiting_reason_code.is_none());
        assert!(rs.nodes["griller"]
            .awaiting
            .as_ref()
            .unwrap()
            .message
            .is_none());

        // A script node runs no agent: named refusal, still the caller's turn.
        let refusal = post_wait_user(&t, run_id, "runner", serde_json::json!({ "iter": 1 })).await;
        assert_eq!(wait_refusal(refusal), "node_is_script");

        // A defined node that never spawned has no session.
        let refusal = post_wait_user(&t, run_id, "later", serde_json::json!({ "iter": 1 })).await;
        assert_eq!(wait_refusal(refusal), "node_session_not_live");

        // An unknown node.
        let refusal = post_wait_user(&t, run_id, "ghost", serde_json::json!({ "iter": 1 })).await;
        assert_eq!(wait_refusal(refusal), "node_not_found");
    }

    #[tokio::test]
    async fn a_human_enter_after_typed_bytes_lifts_the_wait_but_nothing_else_does() {
        let t = with_alive_sessions().await;
        let run_id = "20260912-100000-a11ce01";
        seed_started_node(&t, run_id, true).await;
        let _ = post_wait_user(
            &t,
            run_id,
            "griller",
            serde_json::json!({ "iter": 1, "message": "q?" }),
        )
        .await;
        let session = pdo_session::domain::names::node_session_name(run_id, "griller", 1);

        // An Enter on an empty line (a click to focus the pane): nothing.
        note_pty_input(&t, &session, b"\r").await;
        assert_eq!(
            projected(&t, run_id).await.status,
            event_log::RunStatus::AwaitingUser
        );
        // Arrows, scroll, mouse: nothing, and they do not arm the rule either.
        note_pty_input(&t, &session, b"\x1b[A\x1b[B\x1b[<65;3;4M").await;
        note_pty_input(&t, &session, b"\r").await;
        assert_eq!(
            projected(&t, run_id).await.status,
            event_log::RunStatus::AwaitingUser
        );
        // Typed bytes, then Enter: the wait lifts, node and run back to running.
        note_pty_input(&t, &session, b"the strip").await;
        note_pty_input(&t, &session, b"\r").await;
        let lifted = projected(&t, run_id).await;
        assert_eq!(lifted.status, event_log::RunStatus::Running);
        assert_eq!(
            lifted.nodes["griller"].status,
            event_log::NodeStatus::Running
        );
        assert!(lifted.nodes["griller"].awaiting.is_none());
        assert!(lifted.awaiting_reason.is_none());
        assert!(t
            .events(run_id)
            .await
            .iter()
            .any(|e| e.kind == event_log::EventKind::NodeResumed));

        // The manager / shell sessions never lift anything.
        let _ = post_wait_user(&t, run_id, "griller", serde_json::json!({ "iter": 1 })).await;
        note_pty_input(
            &t,
            &pdo_session::domain::names::manager_session_name(run_id),
            b"hello\r",
        )
        .await;
        assert_eq!(
            projected(&t, run_id).await.status,
            event_log::RunStatus::AwaitingUser
        );

        // Typed bytes BEFORE a re-declaration do not count: the rule restarts.
        note_pty_input(&t, &session, b"typed before").await;
        let _ = post_wait_user(
            &t,
            run_id,
            "griller",
            serde_json::json!({ "iter": 1, "message": "another?" }),
        )
        .await;
        note_pty_input(&t, &session, b"\r").await;
        assert_eq!(
            projected(&t, run_id).await.status,
            event_log::RunStatus::AwaitingUser
        );
    }

    #[tokio::test]
    async fn a_refused_unreleased_completion_declares_the_wait_and_release_lifts_it() {
        let t = with_alive_sessions().await;
        let run_id = "declared-wait-refused-completion";
        seed_started_node(&t, run_id, true).await;

        let attempt = complete_node_iteration(
            &t.contexts.run,
            run_id.into(),
            "griller".into(),
            1,
            CompletionSource::Explicit,
        )
        .await;
        assert!(matches!(
            attempt,
            CompletionAttempt::Refused {
                refusal: completion_refusal::CompletionRefusal::CompletionNotReleased
            }
        ));
        let rs = projected(&t, run_id).await;
        assert_eq!(rs.status, event_log::RunStatus::AwaitingUser);
        let info = rs.nodes["griller"]
            .awaiting
            .as_ref()
            .expect("declared by the daemon");
        assert_eq!(
            info.cause,
            event_log::AWAITING_CAUSE_COMPLETION_NOT_RELEASED
        );
        assert_eq!(
            rs.awaiting_reason.as_deref(),
            Some(pdo_session::domain::declared_wait::COMPLETION_NOT_RELEASED_MESSAGE)
        );
        assert!(rs.awaiting_reason_code.is_none());

        // A second refusal does not stack a second marker.
        let n = t.events(run_id).await.len();
        complete_node_iteration(
            &t.contexts.run,
            run_id.into(),
            "griller".into(),
            1,
            CompletionSource::StopHook,
        )
        .await;
        assert_eq!(t.events(run_id).await.len(), n);

        // The release (ADR-0068) brings the node back to running — unchanged.
        let response = release_node_completion(&t.contexts.run, run_id, "griller", 1).await;
        assert_eq!(response.status(), StatusCode::OK);
        let released = projected(&t, run_id).await;
        assert_eq!(released.status, event_log::RunStatus::Running);
        assert_eq!(
            released.nodes["griller"].status,
            event_log::NodeStatus::Running
        );
        assert!(released.nodes["griller"].awaiting.is_none());
        assert!(released.nodes["griller"].iterations[0].completion_released);
    }

    #[tokio::test]
    async fn a_children_pending_marker_is_not_lifted_by_enter() {
        let t = with_alive_sessions().await;
        let run_id = "20260912-100000-b1nd1e2";
        seed_started_node(&t, run_id, false).await;
        t.append(&event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::NodeAwaitingUser,
            node_id: Some("griller".into()),
            iter: Some(1),
            payload: Some(serde_json::json!({
                "reason": run_advance::BINDING_MARKER_REASON, "active": 1, "failed": 0
            })),
        })
        .await;
        let rs = projected(&t, run_id).await;
        assert_eq!(
            rs.nodes["griller"].awaiting.as_ref().unwrap().cause,
            run_advance::BINDING_MARKER_REASON
        );
        assert!(rs.awaiting_reason.is_none(), "the marker carries no prose");
        let session = pdo_session::domain::names::node_session_name(run_id, "griller", 1);
        note_pty_input(&t, &session, b"hi\r").await;
        assert_eq!(
            projected(&t, run_id).await.nodes["griller"].status,
            event_log::NodeStatus::AwaitingUser
        );
    }

    #[tokio::test]
    async fn a_declared_wait_is_not_an_incident_for_the_reopen_embedding() {
        let t = with_alive_sessions().await;
        let run_id = "declared-wait-no-reopen";
        seed_started_node(&t, run_id, true).await;
        let _ = post_wait_user(
            &t,
            run_id,
            "griller",
            serde_json::json!({ "iter": 1, "message": "q?" }),
        )
        .await;
        let rs = projected(&t, run_id).await;
        let n = t.events(run_id).await.len();
        let same = embed_reopen_for_targeted_command(&t.contexts.run, run_id, rs)
            .await
            .unwrap();
        assert_eq!(same.status, event_log::RunStatus::AwaitingUser);
        assert_eq!(
            t.events(run_id).await.len(),
            n,
            "no reopen command was appended for a declared wait"
        );
    }

    // Node-completion tail convergence: the *observable* behaviour `complete_node`
    // must preserve across both entrypoints (`node_done` and the `mark_node_done`
    // command arm).
    //
    // The headline invariant is exactly-one `RunCompleted` per run: `append_event`
    // does NOT de-dup it, so a regression that double-routes completion surfaces here.
    // Each test runs in the gate-only harness (no pipeline file), so
    // `handle_node_completion` / `advance_run` no-op and the shared completion gate is
    // exercised in isolation.

    async fn seed_running_run(t: &TestContexts, run_id: &str, node_id: &str) {
        let events = vec![
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event_log::EventKind::RunStarted,
                node_id: None,
                iter: None,
                payload: Some(serde_json::json!({ "pipeline_name": "test-pipe" })),
            },
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event_log::EventKind::NodeStarted,
                node_id: Some(node_id.into()),
                iter: Some(1),
                payload: None,
            },
        ];
        for ev in &events {
            t.append(ev).await;
        }
    }

    /// A node's pane, through the session context's use case over this crate's
    /// Run ports — what `GET …/pane` runs.
    async fn read_pane(
        t: &TestContexts,
        run_id: &str,
        node_id: &str,
        iter: i64,
    ) -> Result<
        (String, pdo_session::domain::pane::Pane),
        pdo_session::application::read_pane::ReadPaneError,
    > {
        t.contexts
            .session
            .read_pane()
            .execute(&pdo_kernel::Actor::LocalOperator, run_id, node_id, iter)
            .await
    }

    #[tokio::test]
    async fn pane_returns_404_for_nonexistent_run() {
        let t = TestContexts::new().await;
        assert_eq!(
            read_pane(&t, "nonexistent", "worker", 1).await,
            Err(pdo_session::application::read_pane::ReadPaneError::Run(
                pdo_session::application::ports::RunLookup::NotFound
            ))
        );
    }

    #[tokio::test]
    async fn pane_returns_404_for_nonexistent_node() {
        let t = TestContexts::new().await;
        seed_running_run(&t, "pane-404-node", "worker").await;
        assert_eq!(
            read_pane(&t, "pane-404-node", "bogus", 1).await,
            Err(pdo_session::application::read_pane::ReadPaneError::NodeNotFound)
        );
    }

    #[tokio::test]
    async fn pane_returns_200_with_placeholder_when_no_tmux() {
        let t = TestContexts::new().await;
        seed_running_run(&t, "pane-no-tmux", "worker").await;
        let (session_name, pane) = read_pane(&t, "pane-no-tmux", "worker", 1).await.unwrap();
        assert_eq!(
            session_name,
            pdo_session::domain::names::node_session_name("pane-no-tmux", "worker", 1)
        );
        assert!(!pane.content.is_empty());
    }

    #[tokio::test]
    async fn pane_non_latest_iter_returns_placeholder() {
        let t = TestContexts::new().await;
        let run_id = "pane-old-iter";
        // Node at iter 3 (simulating cyclic node)
        let events = vec![
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event_log::EventKind::RunStarted,
                node_id: None,
                iter: None,
                payload: Some(serde_json::json!({ "pipeline_name": "test" })),
            },
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event_log::EventKind::NodeStarted,
                node_id: Some("reviewer".into()),
                iter: Some(3),
                payload: None,
            },
        ];
        for ev in &events {
            t.append(ev).await;
        }

        // Request iter=1 (not latest, which is 3)
        let (_, pane) = read_pane(&t, run_id, "reviewer", 1).await.unwrap();
        assert_eq!(pane.content, "Session no longer available");
        assert!(!pane.resumed);
        assert!(pane.stale, "non-latest iter should be marked stale");
    }

    #[tokio::test]
    async fn pane_latest_iter_is_not_stale() {
        let t = TestContexts::new().await;
        seed_running_run(&t, "pane-latest-stale", "worker").await;
        let (_, pane) = read_pane(&t, "pane-latest-stale", "worker", 1)
            .await
            .unwrap();
        assert!(!pane.stale, "latest iter should not be stale");
    }

    // -----------------------------------------------------------------------
    // node_prompt endpoint — Layer 2 contract tests
    // -----------------------------------------------------------------------

    /// Seed a `blocker` run holding enough Running nodes to saturate the
    /// global session cap, so any legitimate spawn in the run under test lands
    /// as an observable `node_waiting` event instead of a real tmux session.
    async fn saturate_session_cap(t: &TestContexts) {
        let run_id = "cap-blocker";
        let run_started = event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({ "pipeline_name": "blocker" })),
        };
        t.append(&run_started).await;
        for i in 0..admission::DEFAULT_SESSION_CAP {
            let started = event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event_log::EventKind::NodeStarted,
                node_id: Some(format!("hog-{i}")),
                iter: Some(1),
                payload: None,
            };
            t.append(&started).await;
        }
    }

    fn seed_event(
        run_id: &str,
        kind: event_log::EventKind,
        node_id: Option<&str>,
        iter: Option<i64>,
        payload: Option<serde_json::Value>,
    ) -> event_log::Event {
        event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind,
            node_id: node_id.map(String::from),
            iter,
            payload,
        }
    }

    #[tokio::test]
    async fn a_historical_node_stale_run_still_projects_and_displays() {
        // AC3. `NodeStale` keeps its `EventKind` variant and its projection
        // arm on purpose: the log is APPEND-ONLY, so removing the variant would
        // fail deserialisation for every historical Run that carries one,
        // `project()` would return None, and those Runs would VANISH from the UI.
        // Nothing emits it any more — but everything can still read it.
        let t = TestContexts::new().await;
        let run_id = "historical-stale";
        for event in [
            seed_event(
                run_id,
                event_log::EventKind::RunStarted,
                None,
                None,
                Some(serde_json::json!({ "pipeline_name": "test" })),
            ),
            seed_event(
                run_id,
                event_log::EventKind::NodeStarted,
                Some("worker"),
                Some(1),
                None,
            ),
            seed_event(
                run_id,
                event_log::EventKind::NodeStale,
                Some("worker"),
                Some(1),
                Some(serde_json::json!({ "reason": "idle_outputs_incomplete" })),
            ),
        ] {
            t.append(&event).await;
        }

        // The projection survives …
        let events = t.events(run_id).await;
        let run_state = event_log::project(&events).expect("a historical stale Run must project");
        assert_eq!(
            run_state.nodes["worker"].status,
            event_log::NodeStatus::Stale
        );

        // … and so does the HTTP view the UI reads.
        let resp = t
            .contexts
            .router
            .clone()
            .layer(axum::Extension(pdo_kernel::Actor::LocalOperator))
            .oneshot(
                Request::builder()
                    .uri(format!("/runs/{run_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "the Run must still be served"
        );
        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            body["nodes"]["worker"]["status"], "stale",
            "a historical stale node must still read `stale` in the UI: {body}"
        );
    }

    // Characterization net for POST /runs/{id}/commands
    //
    // Written BEFORE the dispatcher refactor and green on the unmodified tree.
    // These do not describe a contract anyone designed — they RECORD what HEAD
    // actually answers, so a "pure move" that quietly changes a status, a
    // content-type or a side effect goes red instead of shipping. Every value
    // below was observed, never deduced: the surface asserts no content-type
    // anywhere else in the repo, and the unknown-run behaviour is a three-way
    // split that no test covered.

    async fn seed_run_for_node_control(t: &TestContexts, run_id: &str, pipeline_name: &str) {
        let ev = event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({ "pipeline_name": pipeline_name })),
        };
        t.append(&ev).await;
    }

    async fn seed_node_started(t: &TestContexts, run_id: &str, node_id: &str, iter: i64) {
        let ev = event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::NodeStarted,
            node_id: Some(node_id.into()),
            iter: Some(iter),
            payload: None,
        };
        t.append(&ev).await;
    }

    // spawn_node driven directly through the injectable SpawnDeps
    //
    // Before the carve, `spawn_node` could only be exercised end-to-end through a
    // live daemon (POST /runs, real tmux). With `SpawnDeps::from_state`, its
    // ordering invariants are drivable in-process against fakes: these three
    // tests each pin one exit path (throttle / panic-reap / guard-refuse) without
    // launching a session (`tmux_cmd_override = "exec true"`).

    // The sandbox spawn precondition (block + replay).
    //
    // A sandboxed Run's node session runs `docker exec … pdo-sbx-<run>`, and
    // `create_run`'s detached prep task used to be the ONLY thing that waited for the
    // container: the pipeline watcher called the same advance path with no such wait,
    // so the exec hit a container that did not exist and the session died ~25 s later.
    // The precondition lives in `spawn_node`, which is why these tests drive the spawn
    // and the advance rather than the watcher's mpsc plumbing — the watcher calls
    // `spawn_ready_after_event` == `advance_run`, the entry point exercised here.

    #[tokio::test]
    async fn pane_resurrection_refused_at_session_cap() {
        // #487 §3 — a click on a dead pane must not bypass the session cap
        //. At the cap the resurrection is declined and reserves NOTHING:
        // no NodeStarted, so it never overshoots the bound a spawn would respect.
        let t = TestContexts::new().await;
        let repo_root = t.repo_root();
        saturate_session_cap(&t).await;

        let run_id = "pane-cap";
        seed_run_for_node_control(&t, run_id, "test-pipe").await;
        seed_node_started(&t, run_id, "worker", 1).await;
        // The resurrection branch is only reached when the node's working dir still
        // exists (a non-isolated node's dir is the run's pipeline worktree).
        std::fs::create_dir_all(worktree_dir_for_run(repo_root, run_id)).unwrap();

        let started_before = t
            .events(run_id)
            .await
            .iter()
            .filter(|e| {
                e.kind == event_log::EventKind::NodeStarted
                    && e.node_id.as_deref() == Some("worker")
            })
            .count();

        let (_, pane) = read_pane(&t, run_id, "worker", 1).await.unwrap();
        assert_eq!(
            pane.source,
            pdo_session::domain::pane::PaneSource::Unavailable,
            "a resurrection blocked by the cap serves no live session: {pane:?}"
        );
        assert!(
            pane.content.contains("session cap"),
            "the operator is told why: {pane:?}"
        );

        let started_after = t
            .events(run_id)
            .await
            .iter()
            .filter(|e| {
                e.kind == event_log::EventKind::NodeStarted
                    && e.node_id.as_deref() == Some("worker")
            })
            .count();
        assert_eq!(
            started_after, started_before,
            "a resurrection refused at the cap must reserve nothing (no NodeStarted)"
        );
    }

    /// Seed a run as `RunStarted` followed by `extra` run-level events (e.g.
    /// `RunCompleted`, `RunFailed`, `RunArchived`), with no nodes spawned. Lets
    /// the `/runs/reapable` tests construct a run in any terminal/live status.
    async fn seed_run_with_run_events(
        t: &TestContexts,
        run_id: &str,
        pipeline_name: &str,
        extra: &[event_log::EventKind],
    ) {
        let mut events = vec![event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({ "pipeline_name": pipeline_name })),
        }];
        for kind in extra {
            events.push(event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: pdo_kernel::clock::now_iso(),
                kind: kind.clone(),
                node_id: None,
                iter: None,
                payload: None,
            });
        }
        for ev in &events {
            t.append(ev).await;
        }
    }

    /// Create the on-disk worktree dir the daemon would have forked for a run,
    /// so the `/runs/reapable` `worktree_present` check sees it.
    fn mk_run_worktree_dir(repo_root: &std::path::Path, run_id: &str) {
        std::fs::create_dir_all(
            repo_root
                .join(".pdo")
                .join("runs")
                .join(run_id)
                .join("worktree"),
        )
        .unwrap();
    }
}
