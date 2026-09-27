//! The run context's environment variables.

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
