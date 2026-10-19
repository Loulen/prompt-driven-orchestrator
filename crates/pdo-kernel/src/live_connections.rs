//! The daemon's open WebSockets, counted by channel: the run events (`/ws`),
//! the terminals, any other upgrade. Every upgrade goes through the session
//! context's WebSocket journal, which keeps these counts; the telemetry reads
//! them. Process-wide, like the sockets themselves.

use std::sync::atomic::{AtomicI64, Ordering};

static EVENTS: AtomicI64 = AtomicI64::new(0);
static TERMINAL: AtomicI64 = AtomicI64::new(0);
static OTHER: AtomicI64 = AtomicI64::new(0);

/// The channel of the upgrade at `path`.
pub fn channel_of(path: &str) -> &'static str {
    if path == "/ws" {
        "events"
    } else if path.ends_with("/pty") {
        "terminal"
    } else {
        "other"
    }
}

fn counter(path: &str) -> &'static AtomicI64 {
    match channel_of(path) {
        "events" => &EVENTS,
        "terminal" => &TERMINAL,
        _ => &OTHER,
    }
}

/// A WebSocket opened at `path`.
pub fn opened(path: &str) {
    counter(path).fetch_add(1, Ordering::Relaxed);
}

/// A WebSocket opened at `path` closed.
pub fn closed(path: &str) {
    counter(path).fetch_sub(1, Ordering::Relaxed);
}

/// The open WebSockets, by channel.
pub fn snapshot() -> [(&'static str, i64); 3] {
    [
        ("events", EVENTS.load(Ordering::Relaxed)),
        ("terminal", TERMINAL.load(Ordering::Relaxed)),
        ("other", OTHER.load(Ordering::Relaxed)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_upgrade_is_counted_on_its_channel_until_it_closes() {
        assert_eq!(channel_of("/ws"), "events");
        assert_eq!(channel_of("/sessions/abc/pty"), "terminal");
        assert_eq!(channel_of("/ext/x/live"), "other");
        let before = snapshot()[1].1;
        opened("/sessions/abc/pty");
        assert_eq!(snapshot()[1].1, before + 1);
        closed("/sessions/abc/pty");
        assert_eq!(snapshot()[1].1, before);
    }
}
