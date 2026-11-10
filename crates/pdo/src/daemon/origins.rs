//! The logs of the boot: the URL to open, and the operator's WebSocket origins.

use std::net::SocketAddr;

use tracing::{info, warn};

/// The URL to advertise for a listener bound at `bound_addr` (#541).
///
/// An unspecified bind (`0.0.0.0` / `::`) is a *listen* address, not a
/// *connect* address: browsers do reach it, but they then send
/// `Origin: http://0.0.0.0:<port>`, which the WS guard refuses. Map it to
/// `localhost`, the one host every default allowlist entry accepts. A concrete
/// bind (`127.0.0.1`, a LAN IP) is advertised as-is.
pub(crate) fn advertised_url(bound_addr: SocketAddr) -> String {
    let ip = bound_addr.ip();
    if ip.is_unspecified() {
        format!("http://localhost:{}", bound_addr.port())
    } else {
        format!("http://{bound_addr}")
    }
}

/// Only speak up when the operator configured extras — silence is the
/// localhost-only default. A malformed entry is KEPT, never fail-fast: a
/// remote client must not be able to crash the boot. It is flagged because a
/// typo here is the top cause of a dead terminal, and the 403 is otherwise mute.
pub(super) fn log_allowed_ws_origins(allowed_ws_origins: &[String]) {
    if allowed_ws_origins.is_empty() {
        return;
    }
    info!(
        "WS Origin allowlist extended with {} operator origin(s): {} \
         (added on top of the localhost defaults)",
        allowed_ws_origins.len(),
        allowed_ws_origins.join(", ")
    );
    for entry in allowed_ws_origins {
        // An origin is `scheme://host[:port]` with no path. The trailing slash
        // of a bare origin is already stripped by `parse_allowed_origins`.
        let after_scheme = entry
            .strip_prefix("https://")
            .or_else(|| entry.strip_prefix("http://"));
        let looks_valid = match after_scheme {
            Some(rest) => !rest.is_empty() && !rest.contains('/') && !entry.contains('*'),
            None => false,
        };
        if !looks_valid {
            warn!(
                "Suspicious PDO_ALLOWED_WS_ORIGINS entry {entry:?}: expected an \
                 exact origin like \"https://pdo.example.tld\" (scheme + host, no \
                 path, no wildcard). Kept as-is; it will simply never match."
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #541 — the startup banner must hand the user a URL the WS Origin guard
    /// accepts. `0.0.0.0` (and `::`) are listen-only and map to `localhost`;
    /// a concrete bind is advertised verbatim.
    #[test]
    fn advertised_url_maps_unspecified_bind_to_localhost() {
        let v4: SocketAddr = "0.0.0.0:5172".parse().unwrap();
        assert_eq!(advertised_url(v4), "http://localhost:5172");
        let v6: SocketAddr = "[::]:5172".parse().unwrap();
        assert_eq!(advertised_url(v6), "http://localhost:5172");
    }

    #[test]
    fn advertised_url_keeps_concrete_bind() {
        let lo: SocketAddr = "127.0.0.1:0".parse().unwrap();
        assert_eq!(advertised_url(lo), "http://127.0.0.1:0");
        let lan: SocketAddr = "192.168.1.20:5172".parse().unwrap();
        assert_eq!(advertised_url(lan), "http://192.168.1.20:5172");
    }
}
