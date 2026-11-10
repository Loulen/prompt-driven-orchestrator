//! One liveness sweep over every live Run (a transition of the daemon).

use std::path::Path;
use std::sync::Arc;

use tracing::{error, info, warn};

use crate::application::complete_node::complete_node_iteration;
use crate::application::complete_node::CompletionAttempt;
use crate::application::complete_node::CompletionSource;
use crate::application::events::append_event;
use crate::application::events::load_all_run_ids;
use crate::application::events::load_events;
use crate::application::node::launch::find_launch_harness;
use crate::application::node::launch::find_launch_session_id;
use crate::application::node::launch::node_run_isolation;
use crate::application::node::reap::reap_node_session;
use crate::application::paths::effective_repo_root;
use crate::application::paths::resolve_run_pipeline_path;
use crate::application::sandbox as run_sandbox;
use crate::application::scheduler::node_completion::retry_waiting_nodes;
use crate::application::scheduler::stall::reconcile_run_level_stall;
use crate::application::stale::death_probe::gather_session_death_diagnostics;
use crate::application::stale::detector as stale_detector;
use crate::application::stale::restart_race::restarted_since_snapshot;
use crate::application::supervision::run_isolated;
use crate::domain::{projection, state};
use crate::Runs;
use pdo_workspace::adapters::git::worktrees::{sub_worktree_path, worktree_dir_for_run};

/// Run one stale-detection sweep under panic isolation. The ~30 s background loop
/// *and* the [`DaemonHandle::run_stale_detection_tick`] test seam both go through
/// here, so a single stray panic cannot silently disable stall / idle /
/// session-death detection daemon-wide.
pub async fn run_stale_detection_supervised(state: &Arc<Runs>) {
    let sweep_state = state.clone();
    run_isolated("stale detector", async move {
        run_stale_detection(&sweep_state).await
    })
    .await;
}

/// Sweep-time [`stale_detector::NodeProbes`] adapter: binds one running node's tmux
/// and filesystem context so [`stale_detector::assess_node`] can run the whole
/// detection pipeline with real I/O injected. Every method MUST stay a
/// side-effect-free read; the sweep performs the reap/spawn side effects itself.
pub struct SweepNodeProbes<'a> {
    tmux: &'a dyn pdo_session::application::ports::Tmux,
    session_name: &'a str,
    working_dir: &'a Path,
    /// The transcript store root: the sandboxed Run's staged home while it is live,
    /// the host root otherwise. The encoded cwd segment is still derived from
    /// `working_dir`, the single source of truth, never re-encoded here.
    projects_root: &'a Path,
    /// The agent session id this iteration was launched with, from its latest
    /// `NodeStarted` payload. `Some` ⇒ resolve the transcript by identity; `None`
    /// (a legacy row, a script node) ⇒ the newest-mtime fallback.
    session_id: Option<&'a str>,
    /// The node's frozen-at-spawn harness. Transcript resolution dispatches on it,
    /// so this adapter never applies claude's resolution to a foreign harness.
    harness: &'a str,
    pipeline_path: &'a Path,
    artifacts_dir: &'a Path,
    run_id: &'a str,
    node_id: &'a str,
    iter: i64,
    running: &'a [(String, i64)],
}

impl stale_detector::NodeProbes for SweepNodeProbes<'_> {
    fn session_alive(&self) -> bool {
        self.tmux.session_exists(self.session_name)
    }

    fn transcript_tail(&self) -> Option<stale_detector::TranscriptTail> {
        // Dispatch to the resolved harness's own implementation: a data-declared
        // harness resolves `None` and no tail is read.
        pdo_harness::adapters::transcripts::resolve_transcript(
            self.harness,
            self.projects_root,
            self.working_dir,
            self.session_id,
        )
        .as_deref()
        .and_then(stale_detector::read_transcript_tail)
    }

    fn outputs_valid(&self) -> bool {
        stale_detector::validate_outputs(
            self.pipeline_path,
            self.node_id,
            self.iter,
            self.artifacts_dir,
        )
    }

    fn capture_pane(&self) -> Option<String> {
        self.tmux.capture(self.session_name)
    }

    fn session_closed_at(&self) -> Option<std::time::SystemTime> {
        self.tmux.session_closed_at(self.session_name)
    }

    fn session_death_diagnostics(&self) -> stale_detector::SessionDeathDiagnostics {
        gather_session_death_diagnostics(
            self.tmux,
            self.run_id,
            self.node_id,
            self.iter,
            self.running,
        )
    }
}

// Takes `&Arc<Runs>`, not `&Runs`: the turn-end arm calls the shared
// node-completion body, whose detached tail must own a clone of the state that
// outlives this sweep.
pub async fn run_stale_detection(state: &Arc<Runs>) {
    // Record liveness at sweep START: a wedged sweep then freezes the value while an
    // isolated panic has already advanced it, so `/stale/health` answers "is the
    // sweep loop alive?".
    state.last_stale_tick_ms.store(
        chrono::Utc::now().timestamp_millis(),
        std::sync::atomic::Ordering::Relaxed,
    );

    // One-shot poison pill. Checked *after* the heartbeat, so a test can assert
    // liveness advanced through a panicking sweep.
    if state
        .panic_on_stale_sweep
        .swap(false, std::sync::atomic::Ordering::Relaxed)
    {
        panic!("PDO_DEBUG_PANIC_STALE fault injection (#251)");
    }

    let run_ids = match load_all_run_ids(state).await {
        Ok(ids) => ids,
        Err(e) => {
            warn!("Stale detector: failed to load run ids: {e}");
            return;
        }
    };

    let tmux = state.tmux();
    let now = std::time::SystemTime::now();

    // Resolved once for the whole sweep. HOME absent must NOT abort the sweep,
    // unlike `context_from_state` at create — degrade to the host root and log.
    let (home_root, sandbox_root) = match run_sandbox::sandbox_home_roots(state) {
        Ok(r) => r,
        Err(e) => {
            warn!(
                "stale sweep: cannot resolve sandbox home roots, treating all runs as host ({e:#})"
            );
            let home = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default());
            let sandbox = home.join(".pdo").join("sandbox");
            (home, sandbox)
        }
    };

    // Recomputed each sweep, so a node leaving the live set drops out naturally.
    // Published to the `blocked_on_limit` gauge after the loop.
    let mut blocked_on_limit_count: i64 = 0;

    // Resolved ONCE PER SWEEP, not at boot, so ticking the box takes effect within
    // one sweep interval with no restart — and one read per sweep, not one per node.
    // A DB read error degrades the *stored* tier to unset, never to an assumed "on":
    // guessing "on" would take a terminal action nobody asked for.
    let autocomplete_turn_end = stale_detector::autocomplete_turn_end_with(
        state
            .settings
            .defaults()
            .await
            .and_then(|c| c.autocomplete_turn_end),
    );

    for run_id in &run_ids {
        let events = match load_events(state, run_id).await {
            Ok(e) => e,
            Err(_) => continue,
        };
        let run_state = match projection::project(&events) {
            Some(s) => s,
            None => continue,
        };

        if run_state.status != state::RunStatus::Running
            && run_state.status != state::RunStatus::AwaitingUser
        {
            continue;
        }

        let repo_root = effective_repo_root(state, &run_state);
        let worktree_dir = worktree_dir_for_run(&repo_root, run_id);
        let artifacts_dir = worktree_dir.join(".pdo").join("artifacts");
        let pipeline_path = resolve_run_pipeline_path(&repo_root, run_id, &run_state.pipeline_name);

        // The staged home while a sandboxed Run is live, the host root otherwise.
        // The per-node encoded dirname is still derived from the node's
        // `working_dir` inside the probe; this seam only swaps the base.
        let projects_root = pdo_sandbox::adapters::staging::stores::transcripts_root(
            !run_state.sandbox.is_off(),
            run_id,
            &home_root,
            &sandbox_root,
        );
        // `copilot` resolves from the host journal; `pi` from the sandbox-aware store
        // (#708). Picked per node below by the frozen harness
        // (`HarnessStores::root_for`).
        let stores = pdo_sandbox::adapters::staging::stores::HarnessStores::for_run(
            !run_state.sandbox.is_off(),
            run_id,
            &home_root,
            &sandbox_root,
        );

        // The nodes holding a session: the running ones, and the ones whose
        // agent waits in its session for its user, probed for death only.
        let holding = stale_detector::session_holding_nodes(&run_state);
        let running: Vec<(String, i64)> = holding
            .iter()
            .map(|(node_id, iter, _)| (node_id.clone(), *iter))
            .collect();
        for (node_id, iter, awaiting_user) in &holding {
            // #653/ADR-0060: the sweep probes where the NodeRun actually works —
            // its frozen isolation, not a type that no longer implies a directory.
            let working_dir = if node_run_isolation(&events, &run_state, node_id, *iter) {
                sub_worktree_path(&repo_root, run_id, node_id, *iter)
            } else {
                worktree_dir.clone()
            };

            let session_name =
                pdo_session::domain::names::node_session_name(run_id, node_id, *iter);

            // The session id this iteration was launched with, so the probe resolves
            // the transcript by identity. `None` ⇒ legacy newest-mtime pick.
            let launch_session_id = find_launch_session_id(&events, node_id, *iter);

            // The node's FROZEN-at-spawn harness (ADR-0046), never the current YAML.
            // Every capability — transcript resolution, turn-end, usage-limit —
            // dispatches on this name, so the sweep never reads another harness's
            // store with claude's parser.
            let node_harness = find_launch_harness(&events, node_id, *iter)
                .unwrap_or_else(|| pdo_harness::registry::CLAUDE.to_string());

            // The store ROOT is the resolved harness's own; the per-harness
            // `resolve_transcript` joins the right leaf.
            // API 0.8: a node given a `CLAUDE_CONFIG_DIR` writes its transcripts
            // under it — its liveness is read there.
            let claude_root =
                crate::application::node::launch::find_launch_claude_root(&events, node_id, *iter)
                    .unwrap_or_else(|| projects_root.clone());
            let node_store_root: &Path = stores.root_for(&node_harness, &claude_root);

            let probes = SweepNodeProbes {
                tmux,
                session_name: &session_name,
                working_dir: &working_dir,
                projects_root: node_store_root,
                session_id: launch_session_id.as_deref(),
                harness: &node_harness,
                pipeline_path: &pipeline_path,
                artifacts_dir: &artifacts_dir,
                run_id,
                node_id,
                iter: *iter,
                running: &running,
            };

            let assessment = stale_detector::assess_node(
                &probes,
                &events,
                run_id,
                node_id,
                *iter,
                now,
                // A turn that ended on a question to the user never completes it.
                autocomplete_turn_end && !awaiting_user,
                &node_harness,
            );

            if assessment.blocked_on_limit {
                blocked_on_limit_count += 1;
            }
            // A restart of this iteration since the snapshot: the death is its kill (restart_race).
            if assessment.detection == stale_detector::Detection::SessionDied
                && restarted_since_snapshot(state, &events, run_id, node_id, *iter).await
            {
                continue;
            }

            for event in &assessment.events {
                // `append_event` re-validates against the freshly projected state, so
                // a node that terminated organically since this loop's snapshot never
                // receives a late NodeFailed.
                if let Err(e) = append_event(state, event).await {
                    error!("Stale detector: failed to append event: {e}");
                }
            }

            match assessment.detection {
                stale_detector::Detection::SessionDied => {
                    let diag = assessment.session_death_diagnostics.unwrap_or_default();
                    info!(
                        "Stale detector: node {node_id} in run {run_id} — session died \
                         (tmux_server_alive={:?}, correlated_deaths={}, \
                         mem_available_kb={:?}, swap_free_kb={:?})",
                        diag.tmux_server_alive,
                        diag.correlated_deaths,
                        diag.mem_available_kb,
                        diag.swap_free_kb,
                    );
                    // The session is already gone; reaping captures whatever remains
                    // and is a no-op otherwise. Its slot is freed, so re-drive
                    // throttled `waiting` nodes.
                    reap_node_session(
                        state,
                        &repo_root,
                        run_id,
                        node_id,
                        *iter,
                        !run_state.sandbox.is_off(),
                    );
                    retry_waiting_nodes(state).await;
                }
                stale_detector::Detection::TurnEnded => {
                    // #469 §3: complete through the SAME body `POST …/done`
                    // runs, not by appending a terminal event here. On a
                    // isolated node a bare append would record a
                    // `Completed` whose commit stayed on `pdo/sub-…`, leaving the
                    // downstream with nothing — the exact defect of the old
                    // observe-only design's `Act` half.
                    match complete_node_iteration(
                        state,
                        run_id.clone(),
                        node_id.clone(),
                        *iter,
                        CompletionSource::TurnEnded,
                    )
                    .await
                    {
                        // Reads the VARIANT, never the status, so a change to the
                        // wire status cannot silently reroute this.
                        CompletionAttempt::Completed => info!(
                            "Stale detector: node {node_id} in run {run_id} — agent finished its \
                             turn without signalling; auto-completed (#469)"
                        ),
                        noop @ CompletionAttempt::NoOp { .. } => info!(
                            "Stale detector: node {node_id} in run {run_id} — turn ended but \
                             auto-completion was a legal no-op: {}",
                            noop.reason()
                        ),
                        refused @ CompletionAttempt::Refused { .. } => warn!(
                            "Stale detector: node {node_id} in run {run_id} — turn ended but \
                             auto-completion was refused: {}",
                            refused.reason()
                        ),
                    }
                }
                stale_detector::Detection::Ok => {}
            }
        }

        // After node-level detection a run may have no live node and nothing
        // schedulable. Re-reads fresh state so it observes any failure appended above.
        //
        // Safe under a turn-end auto-completion whose detached tail has not yet
        // spawned the successor: `run_stall_reason` returns `None` for "all nodes
        // Completed", and its ready-but-not-driven arm needs
        // `idle_secs >= SPAWN_STALL_GRACE_SECS`, which the `NodeAutoCompleted` just
        // appended resets to ~0.
        reconcile_run_level_stall(state, run_id).await;
    }

    // Recomputed from scratch each sweep, so a node that completed since the last
    // one simply isn't recounted — no stale set to prune.
    state
        .blocked_on_limit
        .store(blocked_on_limit_count, std::sync::atomic::Ordering::Relaxed);
}
