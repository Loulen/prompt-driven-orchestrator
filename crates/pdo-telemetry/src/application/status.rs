//! The outcome of the last export, for the page's « Last export » line, and the
//! log of a collector that stops answering: said once when it fails, once when
//! it answers again, never once per export.

use std::sync::Mutex;

use chrono::{DateTime, Utc};
use serde::Serialize;
use ts_rs::TS;

/// One export, as the page shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct LastExport {
    /// RFC 3339, UTC.
    pub at: String,
    /// `metrics` or `traces`.
    pub signal: String,
    pub ok: bool,
    /// The HTTP status, or why no answer came.
    pub detail: String,
}

/// The status every export writes to.
#[derive(Debug, Default)]
pub struct ExportStatus {
    last: Mutex<Option<LastExport>>,
}

impl ExportStatus {
    /// An export answered `status`.
    pub fn answered(&self, signal: &str, status: u16, at: DateTime<Utc>) {
        let ok = (200..300).contains(&status);
        let detail = match ok {
            true => format!("{status} OK"),
            false => format!("{status}"),
        };
        self.record(signal, ok, detail, at);
    }

    /// An export got no answer.
    pub fn unreachable(&self, signal: &str, error: &str, at: DateTime<Utc>) {
        self.record(signal, false, error.to_string(), at);
    }

    fn record(&self, signal: &str, ok: bool, detail: String, at: DateTime<Utc>) {
        let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
        let was_ok = last.as_ref().map(|l| l.ok);
        match (was_ok, ok) {
            (Some(false), false) => {}
            (_, false) => tracing::warn!(
                signal,
                detail = %detail,
                "telemetry: the collector did not accept the export; the next ones are retried \
                 silently until it does"
            ),
            (Some(false), true) => tracing::info!(signal, "telemetry: the collector answers again"),
            _ => {}
        }
        *last = Some(LastExport {
            at: at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            signal: signal.to_string(),
            ok,
            detail,
        });
    }

    pub fn last(&self) -> Option<LastExport> {
        self.last.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Forget the last export: the target changed.
    pub fn reset(&self) {
        *self.last.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_last_export_says_its_answer() {
        let status = ExportStatus::default();
        assert_eq!(status.last(), None);
        status.answered("metrics", 200, Utc::now());
        let last = status.last().unwrap();
        assert!(last.ok);
        assert_eq!(last.detail, "200 OK");
        status.unreachable("traces", "connection refused", Utc::now());
        let last = status.last().unwrap();
        assert!(!last.ok);
        assert_eq!(last.signal, "traces");
        status.reset();
        assert_eq!(status.last(), None);
    }
}
