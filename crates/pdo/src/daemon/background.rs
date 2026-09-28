//! The daemon's background work: the periodic loops (the reaper, the stale
//! detection, the Triggers' heartbeat, the relay of a Run's pipeline edits),
//! each panic-isolated, and the detached boot tasks (the price refresh, the
//! version check, the harness catalogues' warm-up).

use std::sync::Arc;
use std::time::Duration;

use pdo_pipeline::adapters::watcher::RunPipelineModified;
use pdo_run::application::pipeline_modified::relay_pipeline_modifications;
use pdo_run::application::stale::detector::STALE_TICK_INTERVAL_SECS;
use pdo_run::application::stale::sweep::run_stale_detection_supervised;
use pdo_run::application::supervision::run_isolated;
use pdo_run::Runs;
use pdo_session::Session;
use tokio::task::JoinHandle;
use tokio::time;
use tracing::warn;

use super::config::DaemonConfig;

/// The loops of one daemon, kept alive as long as it serves.
pub(super) struct Loops {
    _reaper: Option<JoinHandle<()>>,
    _stale: Option<JoinHandle<()>>,
    _trigger: Option<JoinHandle<()>>,
    _run_modified: Option<JoinHandle<()>>,
}

impl Loops {
    /// A nested daemon runs none of the cleanup loops (the reaper, the stale
    /// detection), nor the Triggers' heartbeat.
    pub(super) fn start(
        run: &Arc<Runs>,
        session: &Arc<Session>,
        triggers: &Arc<pdo_trigger::Triggers>,
        config: &DaemonConfig,
    ) -> Self {
        let nested = config.nested_daemon;
        Self {
            _reaper: (!nested).then(|| reaper(session.clone())),
            _stale: (!nested).then(|| stale_detection(run.clone())),
            // The Triggers' heartbeat (#927): fires due Triggers on a ~30s tick.
            _trigger: (!nested && config.run_trigger_scheduler_loop)
                .then(|| pdo_trigger::adapters::heartbeat::spawn(triggers.clone())),
            _run_modified: None,
        }
    }

    /// Process the run-scoped pipeline modifications the watcher reports.
    pub(super) fn relay_pipeline_modifications(
        mut self,
        run: &Arc<Runs>,
        modified: tokio::sync::mpsc::UnboundedReceiver<RunPipelineModified>,
    ) -> Self {
        let run = run.clone();
        self._run_modified = Some(tokio::spawn(async move {
            relay_pipeline_modifications(run, modified).await;
        }));
        self
    }
}

/// The orphan sweep, every reaper interval.
fn reaper(session: Arc<Session>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let interval = pdo_session::adapters::env::reaper_interval();
        let mut tick = time::interval(interval);
        loop {
            tick.tick().await;
            // The sweep resolves its TTLs at every pass, not once at boot: a
            // `PUT /settings` must take effect on the next sweep without a
            // restart (ADR-0015, `stored → env → default`).
            // Panic-isolated: a panic in one tick must not silently kill the
            // loop and leave sessions un-reaped for the daemon's life.
            let session = session.clone();
            run_isolated("reaper", async move {
                if let Err(e) = session.sweep().run().await {
                    warn!("Reaper sweep failed: {e}");
                }
            })
            .await;
        }
    })
}

/// The stale detection (dead sessions + idle agents).
fn stale_detection(run: Arc<Runs>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = time::interval(Duration::from_secs(STALE_TICK_INTERVAL_SECS));
        loop {
            tick.tick().await;
            run_stale_detection_supervised(&run).await;
        }
    })
}

/// The boot tasks that must stay DETACHED and be posted AFTER
/// `tokio::spawn(axum::serve(...))`, never `.await`ed before the router: a
/// `.await` here delays the first `accept()` behind a DNS timeout, and
/// `tests/smoke.sh` polls `/runs` for 30 s so it would go green while masking
/// exactly that.
pub(super) fn start_detached(
    stats: &Arc<pdo_stats::Stats>,
    platform: &Arc<pdo_platform::Platform>,
    settings: &Arc<pdo_settings::Settings>,
    run_update_check_loop: bool,
) {
    let stats = stats.clone();
    tokio::spawn(async move {
        run_isolated("price refresh", async move {
            stats.refresh_prices().execute().await;
        })
        .await;
    });

    // Version check (#697): load the on-disk cache so `GET /update` answers from
    // it immediately, then boot check + 6 h loop — detached, like the price
    // refresh, so a slow GitHub never delays the first `accept()`.
    platform.start_update_checks(run_update_check_loop);

    // Warm the harness-catalogue cache at boot, detached for the same reason as
    // the price refresh: probing every harness's `--help`/`--version` must not
    // delay the first `accept()`.
    let settings = settings.clone();
    tokio::spawn(async move {
        run_isolated("harness catalogue probe", async move {
            settings.warm_harness_catalogues().await;
        })
        .await;
    });
}
