//! The Origin guard of the daemon's WebSocket upgrades (#564): both
//! `WS /sessions/{id}/pty` (the terminal) and `WS /ws` (the event stream) accept
//! only the daemon's own localhost origins, plus the operator's extra ones.

use axum::http::HeaderMap;

/// Validate the Origin header against the daemon's own address, plus any
/// operator-configured `extra_origins`, to prevent DNS-rebinding / cross-site
/// WebSocket hijacking (CSWSH). Returns `true` if the origin is acceptable.
///
/// Shared by BOTH WebSocket upgrades — `WS /sessions/{id}/pty` (the terminal)
/// and `WS /ws` (the dashboard event stream, #564). Read it with
/// [`parse_allowed_origins`](crate::domain::pty_protocol::parse_allowed_origins):
/// both sides normalise to lowercase and the pair only reads sensibly together.
/// `extra_origins` are the already-parsed,
/// already-lowercased entries of `PDO_ALLOWED_WS_ORIGINS`; they ADD to the
/// four localhost defaults, never replace them (D2).
pub fn check_origin(headers: &HeaderMap, daemon_port: u16, extra_origins: &[String]) -> bool {
    // No Origin header — e.g. same-origin requests, curl, or non-browser
    // clients. Allow these; the browser always sends Origin on WS upgrade.
    let Some(origin_header) = headers.get("origin") else {
        return true;
    };
    let Ok(origin) = origin_header.to_str() else {
        return false;
    };

    let origin_lower = origin.to_lowercase();

    let allowed = [
        format!("http://localhost:{daemon_port}"),
        format!("http://127.0.0.1:{daemon_port}"),
        format!("https://localhost:{daemon_port}"),
        format!("https://127.0.0.1:{daemon_port}"),
    ];

    allowed.contains(&origin_lower) || extra_origins.contains(&origin_lower)
}

/// Render the received Origin header for a rejection log line (#564, D8),
/// truncated so a hostile client can't flood the log with a giant "origin".
/// `<none>` when absent, `<non-utf8>` when the value isn't visible-ASCII — the
/// same two branches [`check_origin`] treats specially.
pub fn origin_for_log(headers: &HeaderMap) -> String {
    const MAX_CHARS: usize = 256;
    match headers.get("origin") {
        None => "<none>".to_string(),
        Some(value) => match value.to_str() {
            Ok(s) if s.chars().count() > MAX_CHARS => {
                let truncated: String = s.chars().take(MAX_CHARS).collect();
                format!("{truncated}…")
            }
            Ok(s) => s.to_string(),
            Err(_) => "<non-utf8>".to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::pty_protocol::parse_allowed_origins;
    use axum::http::HeaderValue;

    // --- Origin check tests ---

    // The 3rd argument is the parsed `PDO_ALLOWED_WS_ORIGINS` list. An empty
    // slice is the unconfigured production/test default — the seven tests below
    // pin the localhost-only behaviour that #564 must leave untouched.

    #[test]
    fn origin_check_allows_localhost_on_correct_port() {
        let mut headers = HeaderMap::new();
        headers.insert("origin", HeaderValue::from_static("http://localhost:5172"));
        assert!(check_origin(&headers, 5172, &[]));
    }

    #[test]
    fn origin_check_allows_127_0_0_1_on_correct_port() {
        let mut headers = HeaderMap::new();
        headers.insert("origin", HeaderValue::from_static("http://127.0.0.1:5172"));
        assert!(check_origin(&headers, 5172, &[]));
    }

    #[test]
    fn origin_check_rejects_wrong_port() {
        let mut headers = HeaderMap::new();
        headers.insert("origin", HeaderValue::from_static("http://localhost:9999"));
        assert!(!check_origin(&headers, 5172, &[]));
    }

    #[test]
    fn origin_check_rejects_external_origin() {
        let mut headers = HeaderMap::new();
        headers.insert("origin", HeaderValue::from_static("http://evil.com"));
        assert!(!check_origin(&headers, 5172, &[]));
    }

    #[test]
    fn origin_check_allows_no_origin_header() {
        let headers = HeaderMap::new();
        assert!(check_origin(&headers, 5172, &[]));
    }

    #[test]
    fn origin_check_allows_https_localhost() {
        let mut headers = HeaderMap::new();
        headers.insert("origin", HeaderValue::from_static("https://localhost:5172"));
        assert!(check_origin(&headers, 5172, &[]));
    }

    #[test]
    fn origin_check_case_insensitive() {
        let mut headers = HeaderMap::new();
        headers.insert("origin", HeaderValue::from_static("HTTP://LOCALHOST:5172"));
        assert!(check_origin(&headers, 5172, &[]));
    }

    // --- #564: configurable allowlist (PDO_ALLOWED_WS_ORIGINS) ---

    #[test]
    fn origin_check_allows_configured_origin() {
        let extra = parse_allowed_origins("https://pdo.example.com");
        let mut headers = HeaderMap::new();
        headers.insert(
            "origin",
            HeaderValue::from_static("https://pdo.example.com"),
        );
        assert!(check_origin(&headers, 5172, &extra));
    }

    #[test]
    fn origin_check_still_allows_localhost_when_allowlist_configured() {
        // The single most important guard (D2): configuring an extra origin must
        // NOT drop the localhost defaults, or Playwright / layer-3 (baseURL
        // 127.0.0.1) and loopback debugging break.
        let extra = parse_allowed_origins("https://pdo.example.com");
        let mut headers = HeaderMap::new();
        headers.insert("origin", HeaderValue::from_static("http://127.0.0.1:5172"));
        assert!(check_origin(&headers, 5172, &extra));
        headers.insert("origin", HeaderValue::from_static("http://localhost:5172"));
        assert!(check_origin(&headers, 5172, &extra));
    }

    #[test]
    fn origin_check_rejects_origin_absent_from_allowlist() {
        let extra = parse_allowed_origins("https://pdo.example.com");
        let mut headers = HeaderMap::new();
        headers.insert(
            "origin",
            HeaderValue::from_static("https://evil.example.org"),
        );
        assert!(!check_origin(&headers, 5172, &extra));
    }

    #[test]
    fn origin_check_configured_match_is_case_insensitive() {
        // Both sides normalise to lowercase, so a shouting browser still matches
        // a lowercase allowlist entry.
        let extra = parse_allowed_origins("https://pdo.example.com");
        let mut headers = HeaderMap::new();
        headers.insert(
            "origin",
            HeaderValue::from_static("HTTPS://PDO.EXAMPLE.COM"),
        );
        assert!(check_origin(&headers, 5172, &extra));
    }

    #[test]
    fn origin_check_configured_origin_is_scheme_and_port_exact() {
        // Exact-match only (D3): same host with a different scheme or an explicit
        // port is a different origin and must be rejected.
        let extra = parse_allowed_origins("https://pdo.example.com");
        let mut headers = HeaderMap::new();
        headers.insert("origin", HeaderValue::from_static("http://pdo.example.com"));
        assert!(!check_origin(&headers, 5172, &extra));
        headers.insert(
            "origin",
            HeaderValue::from_static("https://pdo.example.com:8443"),
        );
        assert!(!check_origin(&headers, 5172, &extra));
    }

    #[test]
    fn origin_check_allows_no_origin_header_even_with_allowlist() {
        // Pins the D5 branch: a configured allowlist does not harden the
        // "no Origin ⇒ allowed" path (curl / CLI / layer-3 stay green).
        let extra = parse_allowed_origins("https://pdo.example.com");
        let headers = HeaderMap::new();
        assert!(check_origin(&headers, 5172, &extra));
    }

    #[test]
    fn origin_check_rejects_non_ascii_origin_header() {
        // A non-visible-ASCII value makes `to_str()` fail — that branch was
        // previously uncovered. Rejected with or without an allowlist.
        let extra = parse_allowed_origins("https://pdo.example.com");
        let mut headers = HeaderMap::new();
        headers.insert("origin", HeaderValue::from_bytes(&[0xff, 0xfe]).unwrap());
        assert!(!check_origin(&headers, 5172, &extra));
        assert!(!check_origin(&headers, 5172, &[]));
    }

    #[test]
    fn parse_allowed_origins_keeps_wildcard_verbatim_so_it_never_matches() {
        // No wildcard support in v1 (D3): '*' is kept as a literal entry that can
        // never equal a real Origin — mis-configuring it silently allows nothing.
        let parsed = parse_allowed_origins("*");
        assert_eq!(parsed, vec!["*"]);
        let mut headers = HeaderMap::new();
        headers.insert("origin", HeaderValue::from_static("http://evil.com"));
        assert!(!check_origin(&headers, 5172, &parsed));
    }
}
