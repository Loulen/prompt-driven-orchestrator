//! The daemon's journal of its WebSockets (ADR-0083): one line per opening
//! (path, time to the `101`), per closing (who closed, the code, clean or not,
//! the lifetime) and per refused upgrade (status, cause, path, Origin). Enough
//! to tell a slow opening or a hostile page from the log alone; the plain HTTP
//! requests are not journaled.
//!
//! Each opening and closing also moves the count of open WebSockets the
//! telemetry reads (`pdo_kernel::live_connections`).
//!
//! Shared by every upgrade of the daemon: `/ws` (the run context), the
//! terminals' PTY, and the admission of the composition root.

use std::time::Duration;

use axum::http::{HeaderMap, StatusCode};
use tracing::{info, warn};

use super::origin::origin_for_log;

/// The close code of a frame that carried none (RFC 6455 §7.1.5).
pub const NO_STATUS: u16 = 1005;

/// How a WebSocket of the daemon ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Closing {
    /// The client sent a Close frame (its code, [`NO_STATUS`] without one);
    /// the daemon answered it.
    ByClient(u16),
    /// The daemon closed it (its side ended: the PTY, the bus).
    ByDaemon,
    /// The connection dropped without a Close frame (the browser's 1006).
    Dropped,
}

impl Closing {
    fn describe(self) -> String {
        match self {
            Closing::ByClient(code) => format!("by the client, code {code}, clean"),
            Closing::ByDaemon => "by the daemon, clean".to_string(),
            Closing::Dropped => "dropped without a Close frame (1006), unclean".to_string(),
        }
    }
}

/// An upgrade answered `101`: `to_upgrade` from the request to the upgrade.
pub fn opened(path: &str, to_upgrade: Duration) {
    pdo_kernel::live_connections::opened(path);
    info!(
        "WebSocket opened: {path} ({} ms to the 101)",
        to_upgrade.as_millis()
    );
}

/// A WebSocket ended, `lifetime` after it opened.
pub fn closed(path: &str, closing: Closing, lifetime: Duration) {
    pdo_kernel::live_connections::closed(path);
    info!(
        "WebSocket closed: {path}, {}, after {:.1} s",
        closing.describe(),
        lifetime.as_secs_f64()
    );
}

/// An upgrade refused with `status` for `cause`, before any `101`.
pub fn refused(path: &str, status: StatusCode, cause: &str, headers: &HeaderMap) {
    warn!(
        "WebSocket upgrade refused: {path}, {} ({cause}), Origin {}",
        status.as_u16(),
        origin_for_log(headers)
    );
}

/// Whether `headers` ask for a WebSocket upgrade.
pub fn is_upgrade(headers: &HeaderMap) -> bool {
    headers
        .get(axum::http::header::UPGRADE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("websocket"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn a_closing_says_who_closed_and_whether_it_was_clean() {
        assert_eq!(
            Closing::ByClient(1000).describe(),
            "by the client, code 1000, clean"
        );
        assert_eq!(Closing::ByDaemon.describe(), "by the daemon, clean");
        assert!(Closing::Dropped.describe().contains("1006"));
    }

    #[test]
    fn an_upgrade_is_told_by_its_header_whatever_its_case() {
        let mut headers = HeaderMap::new();
        assert!(!is_upgrade(&headers));
        headers.insert("upgrade", HeaderValue::from_static("WebSocket"));
        assert!(is_upgrade(&headers));
        headers.insert("upgrade", HeaderValue::from_static("h2c"));
        assert!(!is_upgrade(&headers));
    }
}
