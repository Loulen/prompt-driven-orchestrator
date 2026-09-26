//! The daemon's clock.

/// Now, as the RFC 3339 UTC timestamp with milliseconds every PDO record uses
/// (`2026-09-26T18:09:16.123Z`).
pub fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_rfc3339_utc_with_milliseconds() {
        let now = now_iso();
        let parsed = chrono::DateTime::parse_from_rfc3339(&now).expect("RFC 3339");
        assert_eq!(parsed.offset().local_minus_utc(), 0);
        assert!(now.ends_with('Z'), "{now}");
        // `YYYY-MM-DDTHH:MM:SS.mmmZ`
        assert_eq!(now.len(), 24, "{now}");
    }
}
