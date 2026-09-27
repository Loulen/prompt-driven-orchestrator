//! The session knobs read from the environment: the `env` tier of their
//! `stored → env → default` resolution (ADR-0015), read fresh at every call so
//! a `PUT /settings` takes effect without a restart.

use std::time::Duration;

use pdo_kernel::env::{
    DEFAULT_HARNESS_ENV, DEFAULT_MODEL_ENV, LIBASSIST_IDLE_TTL_SECS_ENV, REAPER_INTERVAL_SECS_ENV,
    REAPER_TTL_SECS_ENV,
};

use crate::domain::settings::{
    DEFAULT_LIBASSIST_IDLE_TTL, DEFAULT_REAPER_INTERVAL, DEFAULT_REAPER_TTL,
};

/// Env var that EXTENDS the WebSocket Origin allowlist with operator-supplied
/// origins (#564). Comma-separated exact origins (`scheme://host[:port]`), read
/// once at boot by the daemon's configuration — never in the hot path
/// (#181/#407). Additive: the four localhost/127.0.0.1 defaults always stay.
pub const ALLOWED_WS_ORIGINS_ENV: &str = "PDO_ALLOWED_WS_ORIGINS";

/// Resolve the reaper TTL, `stored → env → default` (#129, ADR-0015).
///
/// `stored_secs` is the instance-wide setting persisted via the settings page
/// (or `None` when unset). A stored value `>= 1` wins; otherwise the env var
/// [`REAPER_TTL_SECS_ENV`] applies; otherwise [`DEFAULT_REAPER_TTL`]. A stored
/// `0` is ignored (a zero TTL would reap sessions the instant they complete).
///
/// The caller loads the stored value and passes it in.
/// [`reaper_ttl`] is the `stored = None` shorthand (env-only, unchanged).
///
/// **Load-bearing (ADR-0015):** the reaper reads this **inside its sweep loop**,
/// not once at boot — otherwise a `PUT /settings` is a silent no-op until the
/// daemon restarts.
pub fn reaper_ttl_with(stored_secs: Option<u64>) -> Duration {
    stored_secs
        .filter(|&n| n >= 1)
        .or_else(env_reaper_ttl_secs)
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_REAPER_TTL)
}

/// Read the reaper TTL from the env var alone (`stored = None`).
pub fn reaper_ttl() -> Duration {
    reaper_ttl_with(None)
}

/// The reaper TTL (seconds) contributed by [`REAPER_TTL_SECS_ENV`] alone, or
/// `None` when unset or unparseable.
///
/// Exposed so `GET /settings` can disclose a shadowed env var and compute the
/// winning tier identically to [`reaper_ttl_with`] (#129, ADR-0015).
pub fn env_reaper_ttl_secs() -> Option<u64> {
    std::env::var(REAPER_TTL_SECS_ENV)
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
}

/// Resolve the library assistant's idle TTL, `stored → env → default` (#594,
/// ADR-0051) — the same three tiers, the same `>= 1` floor and the same
/// read-it-inside-the-loop discipline as [`reaper_ttl_with`].
///
/// Kept as its own knob rather than folded into the reaper TTL: they bound
/// different things (see [`DEFAULT_LIBASSIST_IDLE_TTL`]) and differ by more than
/// an order of magnitude, so one value could not serve both.
pub fn libassist_idle_ttl_with(stored_secs: Option<u64>) -> Duration {
    stored_secs
        .filter(|&n| n >= 1)
        .or_else(env_libassist_idle_ttl_secs)
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_LIBASSIST_IDLE_TTL)
}

/// Read the library assistant's idle TTL from the env alone (`stored = None`).
pub fn libassist_idle_ttl() -> Duration {
    libassist_idle_ttl_with(None)
}

/// The idle TTL (seconds) contributed by [`LIBASSIST_IDLE_TTL_SECS_ENV`] alone,
/// or `None` when unset or unparseable. Exposed so `GET /settings` can disclose a
/// shadowed env var and compute the winning tier identically to
/// [`libassist_idle_ttl_with`].
pub fn env_libassist_idle_ttl_secs() -> Option<u64> {
    std::env::var(LIBASSIST_IDLE_TTL_SECS_ENV)
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
}

/// The `default_model` contributed by [`DEFAULT_MODEL_ENV`] alone, or `None`
/// when the var is unset or empty.
///
/// Unlike the numeric knobs, a String has no `parse()` that rejects the empty
/// value for free — so the empty string is filtered explicitly. Without it,
/// `PDO_DEFAULT_MODEL=""` would resolve to `Some("")` and emit `--model ''`.
/// Exposed so `GET /settings` can disclose a shadowed env var and compute the
/// winning tier identically to [`default_model_with`].
pub fn env_default_model() -> Option<String> {
    std::env::var(DEFAULT_MODEL_ENV)
        .ok()
        .filter(|s| !s.is_empty())
}

/// Resolve the instance-wide default model, `stored → env → None` (#347,
/// ADR-0015). A non-empty stored value (the settings page) wins; otherwise the
/// env var; otherwise `None` (the account default — no baked-in const, unlike
/// the reaper TTL).
///
/// **Load-bearing (ADR-0015):** every spawn seam reads this *fresh* (via the
/// instance-config store), so a `PUT /settings` takes effect on the next node
/// without a daemon restart.
pub fn default_model_with(stored: Option<String>) -> Option<String> {
    stored.filter(|s| !s.is_empty()).or_else(env_default_model)
}

/// The default harness contributed by [`DEFAULT_HARNESS_ENV`] alone, or `None`
/// when unset or empty. Exposed so `GET /settings` can disclose a shadowed env
/// var and compute the winning tier identically to [`default_harness_with`].
pub fn env_default_harness() -> Option<String> {
    std::env::var(DEFAULT_HARNESS_ENV)
        .ok()
        .filter(|s| !s.is_empty())
}

/// Resolve the instance-wide default harness, `stored → env → None` (#550,
/// ADR-0046, mirroring [`default_model_with`]). `None` ⇒ the resolver's `claude`
/// floor applies. Read FRESH at every spawn seam so a `PUT /settings` takes
/// effect on the next node without a daemon restart (ADR-0015).
pub fn default_harness_with(stored: Option<String>) -> Option<String> {
    stored
        .filter(|s| !s.is_empty())
        .or_else(env_default_harness)
}

/// Read the reaper interval from the env or use the default.
pub fn reaper_interval() -> Duration {
    std::env::var(REAPER_INTERVAL_SECS_ENV)
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_REAPER_INTERVAL)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_model_env_and_stored_precedence() {
        // Single test on purpose: `DEFAULT_MODEL_ENV` is process-global, so a
        // second test mutating it concurrently would flake (mirrors
        // `reaper_ttl_default_and_from_env`).
        std::env::remove_var(DEFAULT_MODEL_ENV);
        assert_eq!(env_default_model(), None);
        assert_eq!(default_model_with(None), None, "no stored, no env → None");

        std::env::set_var(DEFAULT_MODEL_ENV, "sonnet");
        assert_eq!(env_default_model(), Some("sonnet".to_string()));
        // Stored wins over env.
        assert_eq!(
            default_model_with(Some("opus".to_string())),
            Some("opus".to_string())
        );
        // Empty stored → falls through to env.
        assert_eq!(
            default_model_with(Some(String::new())),
            Some("sonnet".to_string())
        );
        // No stored → env applies.
        assert_eq!(default_model_with(None), Some("sonnet".to_string()));

        // An empty env var is treated as unset (a String has no parse that
        // rejects "" for free).
        std::env::set_var(DEFAULT_MODEL_ENV, "");
        assert_eq!(env_default_model(), None);
        assert_eq!(default_model_with(None), None);

        std::env::remove_var(DEFAULT_MODEL_ENV);
    }

    #[test]
    fn reaper_ttl_default_and_from_env() {
        // Single test on purpose: `REAPER_TTL_SECS_ENV` is process-global, so a
        // second test mutating it concurrently would flake. The stored-precedence
        // assertions (#129, ADR-0015) therefore live here too.
        std::env::remove_var(REAPER_TTL_SECS_ENV);
        assert_eq!(reaper_ttl(), Duration::from_secs(3600));

        std::env::set_var(REAPER_TTL_SECS_ENV, "5");
        assert_eq!(reaper_ttl(), Duration::from_secs(5));

        // Stored wins over env.
        assert_eq!(reaper_ttl_with(Some(120)), Duration::from_secs(120));
        // A zero stored value is ignored → falls through to env.
        assert_eq!(reaper_ttl_with(Some(0)), Duration::from_secs(5));
        // No stored value → env applies.
        assert_eq!(reaper_ttl_with(None), Duration::from_secs(5));
        // No stored and no env → default; stored still wins when env is unset.
        std::env::remove_var(REAPER_TTL_SECS_ENV);
        assert_eq!(reaper_ttl_with(None), DEFAULT_REAPER_TTL);
        assert_eq!(reaper_ttl_with(Some(90)), Duration::from_secs(90));
    }

    #[test]
    fn libassist_idle_ttl_default_and_from_env() {
        // One test, same reason as `reaper_ttl_default_and_from_env`: the env var
        // is process-global, so a second test mutating it would flake.
        std::env::remove_var(LIBASSIST_IDLE_TTL_SECS_ENV);
        assert_eq!(libassist_idle_ttl(), DEFAULT_LIBASSIST_IDLE_TTL);

        std::env::set_var(LIBASSIST_IDLE_TTL_SECS_ENV, "45");
        assert_eq!(libassist_idle_ttl(), Duration::from_secs(45));

        // stored → env → default, and a stored 0 is ignored (a zero TTL would
        // reap the assistant the instant a heartbeat is a hair late).
        assert_eq!(
            libassist_idle_ttl_with(Some(300)),
            Duration::from_secs(300),
            "stored wins over env"
        );
        assert_eq!(libassist_idle_ttl_with(Some(0)), Duration::from_secs(45));
        assert_eq!(libassist_idle_ttl_with(None), Duration::from_secs(45));

        std::env::remove_var(LIBASSIST_IDLE_TTL_SECS_ENV);
        assert_eq!(libassist_idle_ttl_with(None), DEFAULT_LIBASSIST_IDLE_TTL);
        assert_eq!(libassist_idle_ttl_with(Some(90)), Duration::from_secs(90));
    }
}
