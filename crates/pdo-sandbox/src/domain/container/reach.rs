//! Whether a Run's container reaches the daemon: the script of the probe, and
//! what to say when it does not.
//!
//! A node in the container reports back through `PDO_DAEMON_URL`
//! (`http://host.docker.internal:<port>`): `pdo complete`, `pdo fail`, the
//! `Stop` hook. When the daemon listens on an address the container cannot
//! reach — `127.0.0.1` on a Linux Docker engine, whose `host-gateway` is the
//! bridge address — every one of those calls is refused, and the node runs
//! for ever. The bind address alone cannot decide it (Docker Desktop forwards
//! the gateway to the host's loopback; a firewall can drop the bridge), so the
//! container itself is asked, once, before a node is handed to it.

use super::{daemon_url, sh_single_quote, CONTAINER_SIDE_HOST};

/// How long the probe waits for the daemon's port to answer, in seconds.
pub const REACH_TIMEOUT_SECS: u32 = 5;

/// The `bash -lc` script that opens a TCP connection to the daemon's port from
/// inside the container: bash's own `/dev/tcp` (every session already runs on
/// bash), bounded by coreutils' `timeout` when the image has it. Exit 0 ⇒ the
/// daemon answered.
pub fn reach_daemon_script(port: u16) -> String {
    let connect = format!("exec 3<>/dev/tcp/{CONTAINER_SIDE_HOST}/{port}");
    format!(
        "if command -v timeout >/dev/null 2>&1; then exec timeout {REACH_TIMEOUT_SECS} bash -c {}; \
         else {connect}; fi",
        sh_single_quote(&connect)
    )
}

/// Why a sandboxed Run cannot start: its container does not reach the daemon.
/// `detail` is what the probe said (its stderr, or its exit).
pub fn daemon_unreachable_reason(port: u16, detail: &str) -> String {
    format!(
        "the sandbox container cannot reach the PDO daemon at {} ({detail}), so its nodes \
         could never report back (`pdo complete`, `pdo fail`). The daemon must listen on an \
         address the container can reach: every interface (the default — leave PDO_BIND \
         unset, or set it to 0.0.0.0) or the docker bridge address, never 127.0.0.1 alone; \
         and no firewall may drop the bridge's traffic to the host",
        daemon_url(port, true)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_probe_connects_to_the_container_side_url_bounded_by_timeout() {
        let script = reach_daemon_script(6172);
        assert_eq!(
            script,
            "if command -v timeout >/dev/null 2>&1; then exec timeout 5 bash -c \
             'exec 3<>/dev/tcp/host.docker.internal/6172'; \
             else exec 3<>/dev/tcp/host.docker.internal/6172; fi"
        );
    }

    #[test]
    fn the_reason_names_the_url_and_the_remedy() {
        let reason = daemon_unreachable_reason(6172, "Connection refused");
        assert!(reason.contains("http://host.docker.internal:6172 (Connection refused)"));
        assert!(reason.contains("PDO_BIND"));
        assert!(reason.contains("0.0.0.0"));
        assert!(reason.contains("127.0.0.1"));
    }
}
