//! The status bar's session gauges.

use std::sync::Arc;
use std::time::Duration;

use pdo_session::application::assistant::FocusCell;
use pdo_session::application::status::ReadStatus;
use pdo_session::application::sweep::{ReaperGauge, SweepOrphans};
use pdo_session::domain::reaper::SweepTtls;

use crate::fakes::{FakeTmux, Gauges, Owners};

#[tokio::test]
async fn the_status_carries_the_gauges_and_the_reaper_counters_since_boot() {
    let gauge = Arc::new(ReaperGauge::default());
    let status = ReadStatus::new(Arc::new(Gauges { live: 2, cap: 4 }), gauge.clone());
    let before = status.execute().await;
    assert_eq!((before.live, before.cap), (2, 4));
    assert_eq!(before.service["supervisor"], "systemd");
    assert_eq!(before.last_sweep_at, None, "never swept");
    assert_eq!((before.killed, before.killed_for_absent_run), (0, 0));

    // One pass kills an unparseable session: an absence verdict.
    let tmux = FakeTmux::with_sessions(&["pdo-garbage"]);
    let owners = Arc::new(Owners {
        facts: Default::default(),
        unreadable: false,
        ttls: SweepTtls {
            node_run: Duration::from_secs(3600),
            libassist_idle: Duration::from_secs(120),
        },
        asked: Default::default(),
    });
    SweepOrphans::new(tmux, owners, Arc::new(FocusCell::default()), gauge)
        .run()
        .await
        .unwrap();
    let after = status.execute().await;
    assert!(after.last_sweep_at.is_some());
    assert_eq!((after.killed, after.killed_for_absent_run), (1, 1));
}
