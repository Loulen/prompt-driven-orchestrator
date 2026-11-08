//! A running daemon, and the seams its integration tests drive it through
//! instead of racing its background loops.

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use pdo_run::application::boot_recovery;
use pdo_run::application::stale::sweep::run_stale_detection_supervised;
use pdo_run::domain::event::{Event, EventKind};
use tracing::warn;

/// A running daemon: where it listens, its serving task, and the contexts its
/// test seams drive.
pub struct DaemonHandle {
    pub addr: SocketAddr,
    pub task: tokio::task::JoinHandle<Result<()>>,
    pub(super) run: Arc<pdo_run::Runs>,
    pub(super) session: Arc<pdo_session::Session>,
    pub(super) stats: Arc<pdo_stats::Stats>,
    pub(super) platform: Arc<pdo_platform::Platform>,
    pub(super) triggers: Arc<pdo_trigger::Triggers>,
    pub(super) settings: Arc<pdo_settings::Settings>,
}

impl DaemonHandle {
    /// Run a single Trigger scheduler tick synchronously. Lets integration
    /// tests drive firing deterministically instead of waiting for the ~30 s
    /// background interval.
    pub async fn run_trigger_tick(&self) {
        pdo_trigger::adapters::heartbeat::tick_supervised(&self.triggers).await;
    }

    /// Run a single stale-detection sweep synchronously. Lets integration tests
    /// drive liveness detection (dead session -> node Failed) deterministically
    /// instead of waiting for the ~30 s background interval.
    pub async fn run_stale_detection_tick(&self) {
        run_stale_detection_supervised(&self.run).await;
    }

    /// Run the boot-recovery reconciliation pass synchronously. The daemon runs
    /// this once at startup; the seam lets integration tests drive it
    /// deterministically (orphaned Running node -> Failed at boot).
    pub async fn run_boot_recovery_tick(&self) {
        boot_recovery::run_boot_recovery(&self.run).await;
    }

    /// Run the boot price-table refresh synchronously. Production spawns it
    /// DETACHED at startup, and a test must never race a detached task. Honours
    /// all three gates (config flag, cache must already exist, 24 h staleness).
    pub async fn run_price_refresh_tick(&self) {
        self.stats.refresh_prices().execute().await;
    }

    /// Run one periodic version-check pass synchronously (#697): loads the disk
    /// cache if not yet done, then checks ONLY if the setting is on and the cache is
    /// older than the interval — production's own gates, driven deterministically.
    pub async fn run_update_check_tick(&self) {
        self.platform.check_for_update().periodic().await;
    }

    /// Warm the harness-catalogue cache synchronously. Production spawns it
    /// DETACHED at boot, so a test drives it here instead of racing that task.
    pub async fn run_catalogue_probe_tick(&self) {
        self.settings.warm_harness_catalogues().await;
    }

    /// Run a single orphan-sweep (reaper) pass synchronously, instead of racing
    /// the ~60 s background interval. Resolves the TTL exactly like the background
    /// loop (`stored → env → default`).
    pub async fn run_orphan_sweep_tick(&self) {
        if let Err(e) = self.session.sweep().run().await {
            warn!("Reaper sweep (test seam) failed: {e}");
        }
    }

    /// Arm the one-shot stale-sweep poison: the next [`run_stale_detection`]
    /// panics, then disarms itself. Arm it *after* boot, or the immediate startup
    /// sweep consumes it first.
    pub fn arm_stale_panic(&self) {
        self.run.arm_stale_panic();
    }

    /// Shorten the event stream's liveness for the connections opened from
    /// now: a Ping every `ping_every`, mute after `mute_after` of silence.
    pub fn set_stream_liveness(
        &self,
        ping_every: std::time::Duration,
        mute_after: std::time::Duration,
    ) {
        self.run
            .set_stream_liveness(pdo_run::domain::stream_liveness::StreamLiveness {
                ping_every,
                mute_after,
            });
    }

    /// Arm the one-shot spawn poison: the next [`spawn_node`] panics inside its
    /// post-worktree span, then disarms itself. Arm it *after* boot so the run's
    /// entry-node spawn can be targeted.
    pub fn arm_spawn_panic(&self) {
        self.run.arm_spawn_panic();
    }

    /// Arm the terminal-tail gate: the detached tail spawned by the next
    /// `node_done`/`node_fail`/`node_skip` parks at its head until
    /// [`Self::release_node_done_gate`], so a test can drop the client connection
    /// mid-window and prove the advance still happens.
    pub fn arm_node_done_gate(&self) {
        self.run.arm_node_done_gate();
    }

    /// Release the terminal-tail gate: disarm, then wake every tail
    /// parked at the gate. See [`Self::arm_node_done_gate`].
    pub fn release_node_done_gate(&self) {
        self.run.release_node_done_gate();
    }

    /// Shorten the beat of the terminal sockets opened afterwards, instead of
    /// waiting the production 10 s.
    pub fn set_terminal_beat_interval(&self, interval: std::time::Duration) {
        self.session.set_terminal_beat_interval(interval);
    }

    /// Publish `count` throwaway messages on the bus in one synchronous burst:
    /// more than it keeps, so a `/ws` client that could not read them in
    /// between lags, the condition that makes the daemon send `resync`.
    pub fn flood_bus(&self, count: usize) {
        for seq in 0..count {
            self.run
                .bus()
                .publish(serde_json::json!({ "type": "test_flood", "seq": seq }));
        }
    }

    /// Force a Trigger's next fire into the past so the next
    /// [`Self::run_trigger_tick`] treats it as due. Test seam only — production
    /// next-fire times come from the cron schedule.
    pub async fn force_trigger_due(&self, trigger_id: &str) {
        let _ = self
            .triggers
            .store()
            .set_next_fire(trigger_id, Some("2020-01-01T00:00:00.000Z"))
            .await;
    }

    /// Seed a Trigger row whose `target_repo` is NULL — a legacy record.
    ///
    /// Bypassing the handler is the point, not a shortcut: `POST /triggers`
    /// refuses an unnamed repo (ADR-0033), so the API can no longer produce this
    /// shape, yet the read side must keep resolving it and the fire path must
    /// refuse it before running its guard.
    pub async fn seed_legacy_trigger_without_target_repo(
        &self,
        name: &str,
        pipeline_id: &str,
        cron: &str,
        guard_command: Option<&str>,
    ) -> String {
        let trigger = self
            .triggers
            .store()
            .create(pdo_trigger::domain::trigger::NewTrigger {
                name: name.to_string(),
                pipeline_id: pipeline_id.to_string(),
                pipeline_name: pipeline_id.to_string(),
                target_repo: None,
                project_id: None,
                target_repos: None,
                source_branch: None,
                input_template: "seeded input".to_string(),
                variables: serde_json::json!({}),
                cron: cron.to_string(),
                guard_command: guard_command.map(|g| g.to_string()),
                overlap_policy: "skip".to_string(),
                max_concurrent: None,
                sandbox: None,
                harness: None,
                agent_choice: None,
                skills: Vec::new(),
                auto_name: true,
                next_fire_at: Some("2030-01-01T00:00:00.000Z".to_string()),
                last_editor: None,
            })
            .await
            .expect("seeding a legacy trigger row must succeed");
        trigger.id
    }

    /// Seed a `run_started` event of a Run working in `target_repo`, without
    /// creating anything on disk: the Runs a list holds (the CORE-5 measure of
    /// `GET /runs` over 500 Runs).
    #[doc(hidden)]
    pub async fn seed_run_in(&self, run_id: &str, pipeline_name: &str, target_repo: &str) {
        let _ = pdo_run::application::events::append_event(
            &self.run,
            &Event {
                id: None,
                run_id: run_id.to_string(),
                ts: pdo_kernel::clock::now_iso(),
                kind: EventKind::RunStarted,
                node_id: None,
                iter: None,
                payload: Some(serde_json::json!({
                    "pipeline_name": pipeline_name,
                    "input": "seeded input",
                    "target_repo": target_repo,
                })),
            },
        )
        .await;
    }

    /// Seed a `run_started` event of a child Run of `parent_run_id`, created
    /// from its `parent_node_id` session, working in `target_repo`: a tree of
    /// Runs without spawning any session (the tree cost, ADR
    /// draft-cout-de-l-arbre).
    #[doc(hidden)]
    pub async fn seed_child_run_in(
        &self,
        run_id: &str,
        pipeline_name: &str,
        target_repo: &str,
        parent_run_id: &str,
        parent_node_id: &str,
    ) {
        let _ = pdo_run::application::events::append_event(
            &self.run,
            &Event {
                id: None,
                run_id: run_id.to_string(),
                ts: pdo_kernel::clock::now_iso(),
                kind: EventKind::RunStarted,
                node_id: None,
                iter: None,
                payload: Some(serde_json::json!({
                    "pipeline_name": pipeline_name,
                    "input": "seeded input",
                    "target_repo": target_repo,
                    "parent_run_id": parent_run_id,
                    "parent_node_id": parent_node_id,
                })),
            },
        )
        .await;
    }

    /// Seed the archival of `run_id` (`run_archived`), without touching the
    /// disk: an archived Run the reads still hold.
    #[doc(hidden)]
    pub async fn seed_run_archived(&self, run_id: &str) {
        let _ = pdo_run::application::events::append_event(
            &self.run,
            &Event {
                id: None,
                run_id: run_id.to_string(),
                ts: pdo_kernel::clock::now_iso(),
                kind: EventKind::RunArchived,
                node_id: None,
                iter: None,
                payload: None,
            },
        )
        .await;
    }

    /// Seed a `run_started` event whose payload carries **no** `target_repo` — a
    /// legacy Run record. Same rationale as
    /// [`Self::seed_legacy_trigger_without_target_repo`]: `POST /runs` refuses
    /// this shape now, but the read side must keep resolving it forever.
    pub async fn seed_legacy_run_without_target_repo(&self, run_id: &str, pipeline_name: &str) {
        let _ = pdo_run::application::events::append_event(
            &self.run,
            &Event {
                id: None,
                run_id: run_id.to_string(),
                ts: pdo_kernel::clock::now_iso(),
                kind: EventKind::RunStarted,
                node_id: None,
                iter: None,
                payload: Some(serde_json::json!({
                    "pipeline_name": pipeline_name,
                    "input": "seeded input",
                })),
            },
        )
        .await;
    }
}
