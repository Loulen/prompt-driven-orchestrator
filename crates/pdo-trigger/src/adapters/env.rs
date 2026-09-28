//! The environment the trigger context reads.

use std::time::Duration;

use crate::domain::guard::guard_timeout;

/// Test seam: override the guard timeout, in **milliseconds** (the stored and
/// default values are seconds).
pub const GUARD_TIMEOUT_MS_OVERRIDE_ENV: &str = "PDO_GUARD_TIMEOUT_MS";

/// Injected into the guard process so a guard can reference the target repo
/// without hardcoding a path.
pub const TARGET_REPO_ENV: &str = "PDO_TARGET_REPO";

/// The guard timeout (**milliseconds**) contributed by
/// [`GUARD_TIMEOUT_MS_OVERRIDE_ENV`] alone, read fresh.
///
/// Exposed so `GET /settings` can disclose a shadowed env var and compute the
/// winning tier identically to [`guard_timeout_with`] (#129, ADR-0015).
pub fn env_guard_timeout_ms() -> Option<u64> {
    std::env::var(GUARD_TIMEOUT_MS_OVERRIDE_ENV)
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
}

/// The guard timeout, `stored → env → default`, with the env read now: a
/// stored change or a new env value takes effect on the next tick.
pub fn guard_timeout_with(stored_secs: Option<u64>) -> Duration {
    guard_timeout(stored_secs, env_guard_timeout_ms())
}
