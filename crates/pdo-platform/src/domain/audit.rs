//! The out-of-Run audit journal (#507, ADR-0044): the configuration mutations
//! made *outside* a Run — disabling a Trigger by hand, editing its cron, pausing
//! the Triggers, mounting a page from a plain terminal — which the event log
//! cannot hold (every event there belongs to a Run).
//!
//! **Invariant (ADR-0044):** the audit may *under-report* (a mutation succeeded,
//! its row is missing on a full disk or a locked base) but **never**
//! *over-report* (a row for a mutation that did not commit). Hence the order at
//! the seam: read the before → mutate → write the audit **after the commit**,
//! best-effort. A failed write is logged and never fails the mutation.

use serde::Serialize;
use ts_rs::TS;

/// One audit row as `GET /audit` returns it (newest first).
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct AuditEntry {
    pub id: i64,
    pub ts: String,
    pub actor_hint: String,
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub target_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub target_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "unknown")]
    pub before: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "unknown")]
    pub after: Option<serde_json::Value>,
}

/// An audit row to write. Two explicit `before`/`after` columns, not a catch-all
/// `detail`: the feed renders the *before → after*.
#[derive(Debug, Clone, PartialEq)]
pub struct NewAuditEntry {
    pub actor_hint: String,
    pub action: String,
    pub target_kind: Option<String>,
    pub target_id: Option<String>,
    pub before: Option<serde_json::Value>,
    pub after: Option<serde_json::Value>,
}

/// The four optional filters of the feed, and its size. `[from, to)`: the lower
/// bound included, the upper one excluded. No filter is a global feed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditFilter {
    pub from: Option<String>,
    pub to: Option<String>,
    pub target_kind: Option<String>,
    pub target_id: Option<String>,
    pub limit: i64,
}

/// How many rows the feed returns when the caller does not say.
pub const DEFAULT_AUDIT_LIMIT: i64 = 200;

impl Default for AuditFilter {
    fn default() -> Self {
        Self {
            from: None,
            to: None,
            target_kind: None,
            target_id: None,
            limit: DEFAULT_AUDIT_LIMIT,
        }
    }
}

/// The declared origin of a configuration mutation, read from the `X-PDO-Actor`
/// header and stored in the `actor_hint` column.
///
/// **Forgeable** (the daemon binds every interface without auth): a
/// traceability hint, never a gate. Not the Actor of the extension API, which
/// the `identify` middleware establishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorHint {
    Ui,
    Cli,
    Unknown,
}

impl ActorHint {
    /// The hint a header value declares. Absent or unknown is `Unknown`, never an
    /// error: "origin unknown" is acceptable, "no row" is not.
    pub fn from_header(value: Option<&str>) -> Self {
        match value {
            Some("ui") => ActorHint::Ui,
            Some("cli") => ActorHint::Cli,
            _ => ActorHint::Unknown,
        }
    }

    /// The byte-exact `actor_hint` column value.
    pub fn as_hint(self) -> &'static str {
        match self {
            ActorHint::Ui => "ui",
            ActorHint::Cli => "cli",
            ActorHint::Unknown => "unknown",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hint_maps_the_header_and_never_rejects() {
        assert_eq!(ActorHint::from_header(Some("ui")), ActorHint::Ui);
        assert_eq!(ActorHint::from_header(Some("cli")), ActorHint::Cli);
        assert_eq!(ActorHint::from_header(Some("bogus")), ActorHint::Unknown);
        assert_eq!(ActorHint::from_header(None), ActorHint::Unknown);
        assert_eq!(ActorHint::Ui.as_hint(), "ui");
        assert_eq!(ActorHint::Cli.as_hint(), "cli");
        assert_eq!(ActorHint::Unknown.as_hint(), "unknown");
    }

    #[test]
    fn the_default_feed_is_global_and_bounded() {
        let filter = AuditFilter::default();
        assert_eq!(filter.limit, 200);
        assert!(filter.from.is_none() && filter.target_kind.is_none());
    }
}
