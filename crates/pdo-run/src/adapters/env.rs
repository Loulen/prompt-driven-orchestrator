//! The run context's environment variables: the session cap, the attachment
//! budget of a `POST /runs`, the instance tier of `auto_fail` and the manager's
//! auto-start.

use pdo_kernel::instance_default::truthy;

use crate::domain::admission::{parse_cap, resolve_cap, SESSION_CAP_ENV};

/// The session cap contributed by [`SESSION_CAP_ENV`] alone (ignoring any stored
/// value), or `None` when unset, unparseable, or zero (a 0 cap would deadlock).
///
/// Exposed so `GET /settings` can disclose a shadowed env var and compute the
/// winning tier identically to [`configured_cap_with`] (#129, ADR-0015).
pub fn env_cap() -> Option<usize> {
    std::env::var(SESSION_CAP_ENV)
        .ok()
        .as_deref()
        .and_then(parse_cap)
}

/// The configured global session cap, `stored → env → default`. The caller
/// loads the stored value (from `instance_config`) and passes it in.
pub fn configured_cap_with(stored: Option<usize>) -> usize {
    resolve_cap(stored, env_cap())
}

/// The configured global session cap from the env var alone (`stored = None`).
pub fn configured_cap() -> usize {
    configured_cap_with(None)
}

/// Built-in per-run attachment budget (images + files, bytes counted on the
/// multipart parts) when neither the stored setting nor
/// [`MAX_ATTACHMENTS_MB_ENV`] speaks: 50 MB. The modal reads the SAME value
/// from `GET /settings` so its client-side gate and the daemon's agree (#779).
pub const MAX_ATTACHMENTS_MB_DEFAULT: i64 = 50;
/// Env seam of the attachment budget, read only inside
/// [`resolve_max_attachments_mb`].
pub const MAX_ATTACHMENTS_MB_ENV: &str = "PDO_MAX_ATTACHMENTS_MB";

/// The attachment budget in MB, `stored → env → default(50)`. A stored `0` or
/// negative is impossible (`PUT /settings` refuses `< 1`), an unparsable env
/// falls through to the default.
pub fn resolve_max_attachments_mb(stored: Option<i64>) -> i64 {
    if let Some(n) = stored.filter(|&n| n >= 1) {
        return n;
    }
    if let Some(n) = env_max_attachments_mb() {
        return n;
    }
    MAX_ATTACHMENTS_MB_DEFAULT
}

pub fn env_max_attachments_mb() -> Option<i64> {
    std::env::var(MAX_ATTACHMENTS_MB_ENV)
        .ok()
        .and_then(|s| s.trim().parse::<i64>().ok())
        .filter(|&n| n >= 1)
}

/// Env seam for the instance-wide `auto_fail` default (ADR-0049 / ADR-0015).
/// Read only inside [`resolve_instance_auto_fail`], the `stored → env → default`
/// precedence — never elsewhere. Any of `1`/`true`/`yes`/`on` (case-insensitive)
/// is truthy; anything else (incl. unset) is falsey.
pub const AUTO_FAIL_ENV: &str = "PDO_AUTO_FAIL";

/// The `auto_fail` default contributed by [`AUTO_FAIL_ENV`] alone, or `None`
/// when unset.
pub fn env_auto_fail() -> Option<bool> {
    std::env::var(AUTO_FAIL_ENV).ok().map(|s| truthy(&s))
}

/// The instance (global) tier of `auto_fail`, `stored → env → default(false)`
/// (ADR-0049). `stored` is the `auto_fail` column value (`Some(0/1)` a stored
/// decision, `None` unset). Reads the env only when `stored` is `None`, so a
/// stored `0` beats a `PDO_AUTO_FAIL=1`.
pub fn resolve_instance_auto_fail(stored: Option<i64>) -> bool {
    match stored {
        Some(v) => v != 0,
        None => env_auto_fail().unwrap_or(false),
    }
}

/// Env seam for the manager auto-start flag (manager on demand / ADR-0015).
/// Same truthy vocabulary as [`AUTO_FAIL_ENV`].
pub const MANAGER_ENABLED_ENV: &str = "PDO_MANAGER_ENABLED";

/// Built-in default for the manager auto-start flag: **off** — a Run starts
/// managerless and the user starts one from the Manager tab when they want it.
pub const MANAGER_ENABLED_DEFAULT: bool = false;

/// The manager auto-start flag contributed by [`MANAGER_ENABLED_ENV`] alone, or
/// `None` when unset.
pub fn env_manager_enabled() -> Option<bool> {
    std::env::var(MANAGER_ENABLED_ENV).ok().map(|s| truthy(&s))
}

/// The manager auto-start flag, `stored → env → default(false)`. `stored` is
/// the `manager_enabled` column value (`Some(0/1)`: a stored decision, `None`
/// unset); a stored `0` beats a `PDO_MANAGER_ENABLED=1`.
pub fn resolve_manager_enabled(stored: Option<i64>) -> bool {
    match stored {
        Some(v) => v != 0,
        None => env_manager_enabled().unwrap_or(MANAGER_ENABLED_DEFAULT),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::admission::DEFAULT_SESSION_CAP;

    #[test]
    fn configured_cap_reads_env_then_falls_back_to_default() {
        // Kept as a single test to avoid a parallel-execution env-var race:
        // `SESSION_CAP_ENV` is process-global, so two tests mutating it
        // concurrently would flake. The stored-precedence assertions (#129,
        // ADR-0015) therefore live here too.
        let saved = std::env::var(SESSION_CAP_ENV).ok();

        std::env::remove_var(SESSION_CAP_ENV);
        assert_eq!(configured_cap(), DEFAULT_SESSION_CAP);

        std::env::set_var(SESSION_CAP_ENV, "3");
        assert_eq!(configured_cap(), 3);

        // Garbage and zero are ignored (a 0 cap would deadlock every Run).
        std::env::set_var(SESSION_CAP_ENV, "not-a-number");
        assert_eq!(configured_cap(), DEFAULT_SESSION_CAP);
        std::env::set_var(SESSION_CAP_ENV, "0");
        assert_eq!(configured_cap(), DEFAULT_SESSION_CAP);

        // --- stored → env → default precedence (#129, ADR-0015) ---
        std::env::set_var(SESSION_CAP_ENV, "9");
        // Stored wins over env.
        assert_eq!(configured_cap_with(Some(30)), 30);
        // A zero/invalid stored value is ignored → falls through to env.
        assert_eq!(configured_cap_with(Some(0)), 9);
        // No stored value → env applies (identical to `configured_cap()`).
        assert_eq!(configured_cap_with(None), 9);
        // No stored and no env → default; stored still wins when the env is unset.
        std::env::remove_var(SESSION_CAP_ENV);
        assert_eq!(configured_cap_with(None), DEFAULT_SESSION_CAP);
        assert_eq!(configured_cap_with(Some(5)), 5);

        match saved {
            Some(v) => std::env::set_var(SESSION_CAP_ENV, v),
            None => std::env::remove_var(SESSION_CAP_ENV),
        }
    }
}
