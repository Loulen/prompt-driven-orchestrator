//! The PTY bridge's wire protocol (WS `/sessions/{id}/pty`): the control frames a
//! browser sends on the Text channel, and the operator's extra WebSocket origins.

use std::time::Duration;

use serde::Deserialize;

/// How often the bridge beats an open terminal socket (ADR-0083 §5): well
/// under the 60 s read timeout a reverse proxy applies by default, so a
/// terminal whose agent thinks for minutes is never idle in its eyes; short
/// enough for the browser, which never sees pings, to call a socket that
/// missed three beats dead (its 30 s silence delay).
pub const TERMINAL_BEAT_INTERVAL: Duration = Duration::from_secs(10);

/// The Text frame of each beat, sent after its Ping: what the browser's
/// silence delay reads. Never written to the pane.
pub const HEARTBEAT_FRAME: &str = r#"{"type":"heartbeat"}"#;

/// Parse the raw `PDO_ALLOWED_WS_ORIGINS` value into a normalised, ready-to-
/// compare list. **Pure** (takes the raw string, touches no env), so the truth
/// table is testable without a process-global env (#181).
///
/// Split on `,` → trim → strip **one** trailing `/` → lowercase → drop empties.
/// A browser Origin never carries a path or trailing slash (RFC 6454 §7.1:
/// `scheme "://" host [ ":" port ]`), so stripping a single `/` forgives an
/// operator who pastes `https://pdo.example.tld/` at zero cost. We deliberately
/// do NOT use `trim_end_matches('/')`: eating several slashes would silently
/// "repair" a malformed `…//` entry into something that matches, hiding a typo.
/// No wildcard handling (D3): `*` survives as a literal that can never equal a
/// real Origin, so mis-configuring it opens nothing.
pub fn parse_allowed_origins(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|entry| {
            let entry = entry.trim();
            let entry = entry.strip_suffix('/').unwrap_or(entry);
            entry.to_lowercase()
        })
        .filter(|entry| !entry.is_empty())
        .collect()
}

/// A resize message sent from the xterm.js client.
#[derive(Debug, Deserialize, PartialEq)]
pub struct ResizeMsg {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub cols: u16,
    pub rows: u16,
}

/// Try to decode a text WS frame as a resize message.
pub fn decode_resize(text: &str) -> Option<ResizeMsg> {
    let msg: ResizeMsg = serde_json::from_str(text).ok()?;
    if msg.msg_type == "resize" && msg.cols > 0 && msg.rows > 0 {
        Some(msg)
    } else {
        None
    }
}

/// Whether a text WS frame is the take-control message (#870).
pub fn is_take_control(text: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(text)
        .is_ok_and(|v| v.get("type").and_then(|t| t.as_str()) == Some("take_control"))
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- #564: parse_allowed_origins (pure) ---

    #[test]
    fn parse_allowed_origins_trims_lowercases_and_drops_empties() {
        let parsed = parse_allowed_origins("  https://PDO.Example.com , ,https://Other.tld ");
        assert_eq!(parsed, vec!["https://pdo.example.com", "https://other.tld"]);
    }

    #[test]
    fn parse_allowed_origins_strips_exactly_one_trailing_slash() {
        assert_eq!(
            parse_allowed_origins("https://pdo.example.com/"),
            vec!["https://pdo.example.com"]
        );
        // Only ONE slash is stripped: a malformed `…//` keeps the extra slash and
        // so never matches a real browser Origin, rather than being "repaired".
        assert_eq!(
            parse_allowed_origins("https://pdo.example.com//"),
            vec!["https://pdo.example.com/"]
        );
    }

    #[test]
    fn parse_allowed_origins_empty_or_blank_is_empty_vec() {
        assert!(parse_allowed_origins("").is_empty());
        assert!(parse_allowed_origins("   ").is_empty());
        assert!(parse_allowed_origins(",, ,").is_empty());
    }

    // --- Resize message decoder tests ---

    #[test]
    fn decode_resize_valid() {
        let msg = decode_resize(r#"{"type":"resize","cols":120,"rows":40}"#);
        assert_eq!(
            msg,
            Some(ResizeMsg {
                msg_type: "resize".into(),
                cols: 120,
                rows: 40,
            })
        );
    }

    #[test]
    fn decode_resize_rejects_zero_cols() {
        assert_eq!(
            decode_resize(r#"{"type":"resize","cols":0,"rows":40}"#),
            None
        );
    }

    #[test]
    fn decode_resize_rejects_zero_rows() {
        assert_eq!(
            decode_resize(r#"{"type":"resize","cols":80,"rows":0}"#),
            None
        );
    }

    #[test]
    fn decode_resize_rejects_wrong_type() {
        assert_eq!(
            decode_resize(r#"{"type":"data","cols":80,"rows":24}"#),
            None
        );
    }

    #[test]
    fn decode_resize_rejects_garbage() {
        assert_eq!(decode_resize("not json at all"), None);
    }

    #[test]
    fn take_control_frame_is_recognised() {
        assert!(is_take_control(r#"{"type":"take_control"}"#));
        assert!(!is_take_control(r#"{"type":"resize","cols":80,"rows":24}"#));
        assert!(!is_take_control("take_control"));
        assert!(!is_take_control(r#"{"kind":"take_control"}"#));
    }

    #[test]
    fn decode_resize_rejects_missing_fields() {
        assert_eq!(decode_resize(r#"{"type":"resize"}"#), None);
    }
}
