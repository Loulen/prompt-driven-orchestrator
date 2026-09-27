//! Persistent-service health for the status bar (ADR-0019): will a daemon come
//! back after a reboot?

use serde::Serialize;
use ts_rs::TS;

/// Computed **once at daemon boot** and cached, so the polled `GET /sessions`
/// pays zero subprocess cost per request. Serialized as the `service` object on
/// `/sessions`.
///
/// **Accepted staleness:** install happens out-of-process, so a daemon already
/// running when the service is installed keeps a stale value until it restarts.
/// The normal flow is install-then-run (ADR-0019).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct ServiceHealth {
    /// How THIS process was launched, best-effort from env markers:
    /// `"systemd"` | `"launchd"` | `"none"`. A diagnostic hint, not a guarantee
    /// (the markers are inheritable).
    pub supervisor: String,
    /// Will a daemon come back after reboot? `Some(true)` when an enabled `pdo`
    /// unit is present, `Some(false)` when reachable-but-ephemeral, `None`
    /// (serialized `null`) when unknown/unsupported (non-Linux, no systemd,
    /// spawn failure, or timeout). **Never** surfaced as an error — the UI
    /// silences on `true`/`null` and shows an amber `ephemeral` pill only on
    /// `false`.
    pub persistent: Option<bool>,
}

/// Forced service-health observation, armed by `PDO_SERVICE_HEALTH`. Makes the
/// `ephemeral` status-bar branch demonstrable in a real browser on a box where an
/// enabled `pdo.service` already exists, without touching the real unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceHealthOverride {
    Persistent,
    Ephemeral,
    Unknown,
}

impl ServiceHealthOverride {
    /// The override a `PDO_SERVICE_HEALTH` value names; invalid values are ignored
    /// rather than fatal.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "persistent" => Some(Self::Persistent),
            "ephemeral" => Some(Self::Ephemeral),
            "unknown" => Some(Self::Unknown),
            _ => None,
        }
    }

    /// The persistence bit this override forces.
    pub fn persistent(self) -> Option<bool> {
        match self {
            Self::Persistent => Some(true),
            Self::Ephemeral => Some(false),
            Self::Unknown => None,
        }
    }
}

/// The health a daemon reports: the supervisor it detected, and the persistence
/// bit — forced by an override, or observed.
pub fn service_health(
    supervisor: &str,
    forced: Option<ServiceHealthOverride>,
    observed: impl FnOnce() -> Option<bool>,
) -> ServiceHealth {
    let persistent = match forced {
        Some(o) => o.persistent(),
        None => observed(),
    };
    ServiceHealth {
        supervisor: supervisor.to_string(),
        persistent,
    }
}

/// Best-effort detection of how this process was launched, from the env markers
/// a supervisor sets (`XPC_SERVICE_NAME`; `INVOCATION_ID`, `NOTIFY_SOCKET`,
/// `JOURNAL_STREAM`). A hint: the markers are inheritable.
pub fn detect_supervisor(launchd_marker: bool, systemd_marker: bool) -> &'static str {
    if launchd_marker {
        "launchd"
    } else if systemd_marker {
        "systemd"
    } else {
        "none"
    }
}

/// The persistence bit `systemctl --user is-enabled` reports: `enabled` (or
/// `enabled-runtime`) comes back after reboot, any other state does not, an empty
/// answer is unknown.
pub fn persistent_from_is_enabled(stdout: &str) -> Option<bool> {
    match stdout.trim() {
        "enabled" | "enabled-runtime" => Some(true),
        s if !s.is_empty() => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_health_override_maps_to_persistence_bit() {
        assert_eq!(
            ServiceHealthOverride::parse("ephemeral"),
            Some(ServiceHealthOverride::Ephemeral)
        );
        assert_eq!(
            ServiceHealthOverride::parse(" Persistent "),
            Some(ServiceHealthOverride::Persistent)
        );
        assert_eq!(ServiceHealthOverride::parse("bogus"), None);
        let forced = service_health("none", Some(ServiceHealthOverride::Ephemeral), || {
            panic!("an override never observes")
        });
        assert_eq!(forced.persistent, Some(false));
        let unknown = service_health("none", Some(ServiceHealthOverride::Unknown), || Some(true));
        assert_eq!(unknown.persistent, None);
        let observed = service_health("systemd", None, || Some(true));
        assert_eq!(observed.persistent, Some(true));
        assert_eq!(observed.supervisor, "systemd");
    }

    #[test]
    fn supervisor_and_is_enabled_readings() {
        assert_eq!(detect_supervisor(true, true), "launchd");
        assert_eq!(detect_supervisor(false, true), "systemd");
        assert_eq!(detect_supervisor(false, false), "none");
        assert_eq!(persistent_from_is_enabled("enabled\n"), Some(true));
        assert_eq!(persistent_from_is_enabled("enabled-runtime"), Some(true));
        assert_eq!(persistent_from_is_enabled("disabled"), Some(false));
        assert_eq!(persistent_from_is_enabled(""), None);
    }
}
