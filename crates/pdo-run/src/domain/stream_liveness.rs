//! The liveness of a connection of the event stream (API 0.23): the daemon
//! pings it, and a connection it heard nothing from (no Pong, no message) for
//! longer than [`StreamLiveness::mute_after`] is mute. A proxy or a tunnel may
//! keep a connection half-open long after its tab lost it: the daemon's
//! writes still succeed, only the silence tells. Nothing here does I/O: the
//! clock is handed in.

use std::time::{Duration, Instant};

/// How often the daemon pings a connection, and how long a silence makes it
/// mute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamLiveness {
    pub ping_every: Duration,
    pub mute_after: Duration,
}

impl Default for StreamLiveness {
    /// A Ping every 2 s, mute after 10 s of silence: a lost tab's Presence
    /// falls 10 to 12 s after it last answered.
    fn default() -> Self {
        Self {
            ping_every: Duration::from_secs(2),
            mute_after: Duration::from_secs(10),
        }
    }
}

/// When a connection was last heard, against its delay of silence.
#[derive(Debug, Clone, Copy)]
pub struct Liveness {
    mute_after: Duration,
    last_heard: Instant,
}

impl Liveness {
    /// A connection that opened at `now`: opening counts as hearing it.
    pub fn new(mute_after: Duration, now: Instant) -> Self {
        Self {
            mute_after,
            last_heard: now,
        }
    }

    /// Something came in from the connection at `now` (a Pong, a message).
    pub fn heard(&mut self, now: Instant) {
        self.last_heard = self.last_heard.max(now);
    }

    /// Whether the connection said nothing for longer than its delay at `now`.
    pub fn is_mute(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.last_heard) > self.mute_after
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEN: Duration = Duration::from_secs(10);

    #[test]
    fn a_connection_silent_beyond_its_delay_is_mute() {
        let opened = Instant::now();
        let liveness = Liveness::new(TEN, opened);
        assert!(!liveness.is_mute(opened));
        assert!(!liveness.is_mute(opened + TEN), "at the delay, not beyond");
        assert!(liveness.is_mute(opened + TEN + Duration::from_millis(1)));
    }

    #[test]
    fn hearing_the_connection_starts_the_delay_again() {
        let opened = Instant::now();
        let mut liveness = Liveness::new(TEN, opened);
        liveness.heard(opened + Duration::from_secs(8));
        assert!(!liveness.is_mute(opened + Duration::from_secs(17)));
        assert!(liveness.is_mute(opened + Duration::from_secs(19)));
    }

    #[test]
    fn a_late_report_of_an_older_hearing_moves_nothing_back() {
        let opened = Instant::now();
        let mut liveness = Liveness::new(TEN, opened);
        liveness.heard(opened + Duration::from_secs(5));
        liveness.heard(opened + Duration::from_secs(1));
        assert!(!liveness.is_mute(opened + Duration::from_secs(14)));
    }

    #[test]
    fn by_default_a_lost_tab_is_mute_within_about_ten_seconds() {
        let liveness = StreamLiveness::default();
        assert_eq!(liveness.mute_after, TEN);
        assert!(liveness.ping_every < liveness.mute_after);
    }
}
