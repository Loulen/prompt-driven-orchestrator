//! The trigger context's instance default, as `GET /settings` discloses it
//! (ADR-0015): the guard timeout, resolved as the heartbeat resolves it. The
//! stored tier is handed in by the settings context, which keeps it.

use pdo_kernel::instance_default::{Disclosed, Number, Source};

use super::env::{env_guard_timeout_ms, guard_timeout_with};
use crate::domain::guard::{GUARD_TIMEOUT_SECS, MAX_GUARD_TIMEOUT_SECS};

/// The guard timeout, in seconds — except its `env`, the raw
/// `PDO_GUARD_TIMEOUT_MS` in milliseconds, the env seam's own unit. A stored
/// value out of `1..=MAX` is disclosed but never wins.
pub fn guard_timeout_secs(stored: Option<i64>) -> Number {
    let env = env_guard_timeout_ms().map(|n| n as i64);
    Disclosed {
        effective: guard_timeout_with(stored.map(|n| n as u64)).as_secs() as i64,
        source: Source::of(
            stored
                .filter(|&n| (1..=MAX_GUARD_TIMEOUT_SECS as i64).contains(&n))
                .is_some(),
            env.is_some(),
        ),
        stored,
        env,
        default: GUARD_TIMEOUT_SECS as i64,
    }
}
