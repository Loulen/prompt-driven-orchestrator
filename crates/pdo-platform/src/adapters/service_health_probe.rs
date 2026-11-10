//! Observe the persistent-service health once, at daemon boot (ADR-0019).

use std::path::PathBuf;
use std::time::Duration;

use crate::domain::service_health::{
    detect_supervisor, persistent_from_is_enabled, service_health, ServiceHealth,
    ServiceHealthOverride,
};
use crate::domain::service_unit::{launchd_plist_path, SYSTEMD_UNIT_NAME};

/// The health of this daemon: the supervisor its env markers name, and whether
/// an enabled `pdo` unit brings a daemon back after reboot — unless `forced`.
pub fn observe(forced: Option<ServiceHealthOverride>) -> ServiceHealth {
    let supervisor = detect_supervisor(
        std::env::var_os("XPC_SERVICE_NAME").is_some(),
        std::env::var_os("INVOCATION_ID").is_some()
            || std::env::var_os("NOTIFY_SOCKET").is_some()
            || std::env::var_os("JOURNAL_STREAM").is_some(),
    );
    service_health(supervisor, forced, detect_persistent)
}

/// Is an enabled `pdo` service unit present? `None` when unknown/unsupported;
/// never an error.
fn detect_persistent() -> Option<bool> {
    if cfg!(target_os = "macos") {
        let home = std::env::var_os("HOME").map(PathBuf::from)?;
        return Some(launchd_plist_path(&home).exists());
    }
    detect_systemd_enabled()
}

/// `systemctl --user is-enabled pdo.service`, timeout-guarded.
///
/// Runs on a detached thread so a wedged systemctl can't block boot past ~1s; on
/// timeout / spawn failure it degrades to `None`, never an error.
fn detect_systemd_enabled() -> Option<bool> {
    use std::sync::mpsc;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let out = std::process::Command::new("systemctl")
            .args(["--user", "is-enabled", SYSTEMD_UNIT_NAME])
            .output();
        let _ = tx.send(out);
    });
    match rx.recv_timeout(Duration::from_secs(1)) {
        Ok(Ok(output)) => persistent_from_is_enabled(&String::from_utf8_lossy(&output.stdout)),
        // Systemctl absent (spawn error) or wedged (timeout) → unknown.
        Ok(Err(_)) | Err(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_override_is_reported_as_forced() {
        let health = observe(Some(ServiceHealthOverride::Persistent));
        assert_eq!(health.persistent, Some(true));
        assert!(["systemd", "launchd", "none"].contains(&health.supervisor.as_str()));
    }
}
