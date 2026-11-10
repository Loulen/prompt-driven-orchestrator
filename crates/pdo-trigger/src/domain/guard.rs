//! The rules of a Trigger guard (CONTEXT.md → *Trigger*): a single shell
//! command run before each fire, whose exit code decides and whose stdout
//! becomes the Run's input. Running it is an adapter's job
//! (`GuardRunner`); what it may take and what it keeps are decided here.

use std::time::Duration;

/// The default hard timeout for a guard command, so a hung guard never freezes
/// the scheduler.
pub const GUARD_TIMEOUT_SECS: u64 = 60;

/// The longest guard timeout an operator may store, in seconds.
pub const MAX_GUARD_TIMEOUT_SECS: u64 = 600;

/// Cap on the guard output we *capture* for the fire history (#244): each of
/// stdout/stderr is tail-kept to at most this many bytes. This bounds the
/// diagnostic blob stored per skip row; it is **never** applied to the `Pass`
/// path, whose stdout is the actual Run input and must stay byte-for-byte.
pub const GUARD_CAPTURE_LIMIT_BYTES: usize = 16 * 1024;

/// Prefix prepended to a tail-capped stream so a reader knows the head was
/// dropped (errors usually print last, so we keep the tail).
pub const TRUNCATION_MARKER: &str = "…[truncated, showing last 16 KB]\n";

/// Keep the last `limit` *bytes* of `s`, snapped forward to a UTF-8 char
/// boundary so the result is always valid UTF-8. Counts bytes (it's a storage
/// bound), not chars.
pub fn cap_tail(s: &str, limit: usize) -> String {
    if s.len() <= limit {
        return s.to_string();
    }
    let mut start = s.len() - limit;
    while start < s.len() && !s.is_char_boundary(start) {
        start += 1;
    }
    format!("{TRUNCATION_MARKER}{}", &s[start..])
}

/// Resolve the guard timeout, `stored → env → default` (#129, ADR-0015).
///
/// **Unit care:** `stored_secs` is in **seconds** (the settings page unit),
/// while `env_ms`, the value of `PDO_GUARD_TIMEOUT_MS`, is in
/// **milliseconds**. A stored value within `[1, 600]` seconds wins; otherwise
/// the env override (in ms) applies; otherwise [`GUARD_TIMEOUT_SECS`]. An
/// out-of-range stored value is ignored (fail-safe: the `PUT` handler already
/// rejects it with `400`, so this is a defensive floor).
pub fn guard_timeout(stored_secs: Option<u64>, env_ms: Option<u64>) -> Duration {
    stored_secs
        .filter(|&n| (1..=MAX_GUARD_TIMEOUT_SECS).contains(&n))
        .map(Duration::from_secs)
        .or_else(|| env_ms.map(Duration::from_millis))
        .unwrap_or_else(|| Duration::from_secs(GUARD_TIMEOUT_SECS))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guard_timeout_precedence_stored_env_default() {
        // Unit care (#129, ADR-0015): stored is SECONDS, env is MILLISECONDS.
        assert_eq!(
            guard_timeout(None, None),
            Duration::from_secs(GUARD_TIMEOUT_SECS)
        );
        // Env override (ms) applies when no stored value.
        assert_eq!(guard_timeout(None, Some(250)), Duration::from_millis(250));
        // Stored (seconds) wins over env (ms).
        assert_eq!(guard_timeout(Some(30), Some(250)), Duration::from_secs(30));
        // Out-of-range stored is ignored → falls through to env.
        assert_eq!(
            guard_timeout(Some(0), Some(250)),
            Duration::from_millis(250)
        );
        assert_eq!(
            guard_timeout(Some(601), Some(250)),
            Duration::from_millis(250)
        );
        // Boundaries are in range.
        assert_eq!(guard_timeout(Some(1), None), Duration::from_secs(1));
        assert_eq!(guard_timeout(Some(600), None), Duration::from_secs(600));
    }

    #[test]
    fn cap_tail_keeps_the_last_bytes_with_a_marker() {
        assert_eq!(cap_tail("short", 1024), "short");

        let s: String = (0..1000).map(|_| 'a').collect();
        let capped = cap_tail(&s, 100);
        assert!(capped.starts_with(TRUNCATION_MARKER));
        assert_eq!(capped.len(), TRUNCATION_MARKER.len() + 100);
        assert!(capped.ends_with("aaaa"));
    }

    #[test]
    fn cap_tail_snaps_to_a_utf8_char_boundary() {
        // A multi-byte char straddling the cut must not panic and must yield
        // valid UTF-8; we snap forward, so the cap is the *upper* bound.
        let s = "é".repeat(100); // each 'é' is 2 bytes → 200 bytes
        let capped = cap_tail(&s, 51); // 51 lands mid-codepoint
        assert!(capped.starts_with(TRUNCATION_MARKER));
        let payload = &capped[TRUNCATION_MARKER.len()..];
        assert!(payload.chars().all(|c| c == 'é'));
        assert!(payload.len() <= 51);
    }
}
