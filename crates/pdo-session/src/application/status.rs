//! The status bar's session gauges (`GET /sessions`): live node sessions, the
//! cap, the daemon version, the persistent-service health and the reaper's
//! since-boot counters.

use std::sync::Arc;

use crate::application::ports::DaemonGauges;
use crate::application::sweep::ReaperGauge;

/// What `GET /sessions` answers.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionsStatus {
    pub live: usize,
    pub cap: usize,
    pub service: serde_json::Value,
    /// RFC 3339 (ms) of the last sweep, `None` before the first.
    pub last_sweep_at: Option<String>,
    pub killed: i64,
    pub killed_for_absent_run: i64,
}

/// Manager sessions are excluded by construction (they are not nodes), and the
/// service health is observed at boot so this polled read stays subprocess-free.
///
/// The reaper counters are **cumulative since boot**, so "the reaper killed
/// something" survives long enough to be read. The gauge deliberately does NOT
/// answer "who killed *my* node": it is instance-wide.
pub struct ReadStatus {
    gauges: Arc<dyn DaemonGauges>,
    reaper: Arc<ReaperGauge>,
}

impl ReadStatus {
    pub fn new(gauges: Arc<dyn DaemonGauges>, reaper: Arc<ReaperGauge>) -> Self {
        Self { gauges, reaper }
    }

    pub async fn execute(&self) -> SessionsStatus {
        let live = self.gauges.live_sessions().await;
        let cap = self.gauges.session_cap().await;
        let (sweep_ms, killed, killed_for_absent_run) = self.reaper.read();
        let last_sweep_at = (sweep_ms != 0)
            .then(|| chrono::DateTime::from_timestamp_millis(sweep_ms))
            .flatten()
            .map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
        SessionsStatus {
            live,
            cap,
            service: self.gauges.service_health(),
            last_sweep_at,
            killed,
            killed_for_absent_run,
        }
    }
}
