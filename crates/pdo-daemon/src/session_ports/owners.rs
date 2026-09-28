//! The owners of the live sessions, as the orphan sweep reads them (ADR-0038).

use std::collections::HashMap;

use pdo_session::application::ports::{BoxFuture, SessionOwners};
use pdo_session::domain::reaper::{NodeRunInfo, SweepTtls};

use super::LegacySessionPorts;
use crate::event_log;

impl SessionOwners for LegacySessionPorts {
    fn owners<'a>(
        &'a self,
        owners: &'a [(String, String)],
    ) -> BoxFuture<'a, Result<Vec<Option<NodeRunInfo>>, String>> {
        Box::pin(async move {
            let state = self.state()?;
            // Project ONLY the runs that actually hold a live session, once each.
            // The error is load-bearing and must NOT be swallowed into `None`: a
            // pool timeout would read a live run as "absent" and kill its session.
            // An unknown run_id is no error (zero rows → `project` → `None` →
            // absent → killed, which is the intended verdict).
            let mut projected: HashMap<&str, Option<event_log::RunState>> = HashMap::new();
            let mut facts = Vec::with_capacity(owners.len());
            for (run_id, node_id) in owners {
                if !projected.contains_key(run_id.as_str()) {
                    let events = crate::load_events(&state.db, run_id)
                        .await
                        .map_err(|e| format!("{e:#}"))?;
                    // Keep the `Option`: synthesising an empty `RunState` for a
                    // fragmentary log (events with no `RunStarted`) would turn
                    // "absent" into "keep".
                    projected.insert(run_id, event_log::project(&events));
                }
                facts.push(
                    projected
                        .get(run_id.as_str())
                        .and_then(|s| s.as_ref())
                        .and_then(|rs| reaper_info_for(rs, node_id)),
                );
            }
            Ok(facts)
        })
    }

    /// Both sweep TTLs at their `stored → env → default` precedence (ADR-0015),
    /// the single point every sweep pass goes through. A failed read degrades to
    /// the env/default tiers rather than aborting the pass: a sweep that does not
    /// run reaps nothing at all.
    fn ttls(&self) -> BoxFuture<'_, SweepTtls> {
        Box::pin(async move {
            let cfg = match self.state() {
                Ok(state) => pdo_settings::adapters::sqlite::instance_config::get(&state.db)
                    .await
                    .ok(),
                Err(_) => None,
            };
            SweepTtls {
                node_run: pdo_session::adapters::env::reaper_ttl_with(
                    cfg.as_ref()
                        .and_then(|c| c.reaper_ttl_secs)
                        .map(|n| n as u64),
                ),
                libassist_idle: pdo_session::adapters::env::libassist_idle_ttl_with(
                    cfg.as_ref()
                        .and_then(|c| c.libassist_idle_ttl_secs)
                        .map(|n| n as u64),
                ),
            }
        })
    }
}

fn rfc3339(s: Option<&str>) -> Option<chrono::DateTime<chrono::Utc>> {
    s.and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&chrono::Utc))
}

/// The reaper's facts about one session's owner. `None` = absent from the log,
/// which is the sweep's kill arm.
fn reaper_info_for(run_state: &event_log::RunState, node_id: &str) -> Option<NodeRunInfo> {
    let is_archived = run_state.status == event_log::RunStatus::Archived;
    // The manager and the run shell are not projected nodes, so they resolve
    // against the run itself. Without this branch the sweep kills them as
    // "absent" on every pass. Their arms ignore `completed_at`, so only
    // `is_archived` is load-bearing.
    if node_id == "__manager__" || node_id == "__shell__" {
        return Some(NodeRunInfo {
            completed_at: rfc3339(run_state.completed_at.as_deref()),
            is_archived,
        });
    }
    let node = run_state.nodes.get(node_id)?;
    Some(NodeRunInfo {
        completed_at: rfc3339(node.completed_at.as_deref()),
        is_archived,
    })
}
