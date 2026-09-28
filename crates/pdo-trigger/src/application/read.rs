//! The reads of the context: the list, one Trigger, its fires, the heartbeat's
//! health. Reads change nothing, so they ask no one.

use serde::Serialize;
use tracing::error;
use ts_rs::TS;

use super::TriggerError;
use crate::domain::schedule::TICK_INTERVAL_SECS;
use crate::domain::trigger::{Trigger, TriggerFire};
use crate::Triggers;

/// A Trigger as returned by `GET /triggers`, wrapping the raw stored Trigger with
/// a resolved `effective_repo` for the "group by project" view. Don't fold it into
/// `trigger.target_repo`: the raw value drives the row badge, the detail panel and
/// the Run-now pre-fill, while this one exists purely so the client can group by a
/// concrete path.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct TriggerListEntry {
    #[serde(flatten)]
    pub trigger: Trigger,
    pub effective_repo: String,
}

/// The heartbeat's liveness: the most recent tick start, the configured
/// interval, and the global pause flag, so a dead/stalled scheduler — or a
/// deliberately-paused one — is observable instead of silently dormant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Health {
    pub last_tick_at: Option<String>,
    pub tick_interval_secs: u64,
    pub paused: bool,
}

impl Triggers {
    pub async fn list(&self) -> Result<Vec<TriggerListEntry>, TriggerError> {
        let triggers = self.store.list().await.map_err(|e| {
            error!("failed to list triggers: {e}");
            TriggerError::Storage("failed to list triggers".to_string())
        })?;
        // Read-side fallback only, and it stays: historical rows legitimately carry a
        // null repo (the write boundary now refuses one), and the "no Unassigned
        // bucket" invariant depends on resolving them here. Do NOT symmetrise it with
        // create/update. The raw `target_repo` is left untouched so the badge / detail
        // / Run-now pre-fill keep showing only what the user actually set.
        let repo_root = self.options.repo_root.to_string_lossy().into_owned();
        Ok(triggers
            .into_iter()
            .map(|t| {
                let effective_repo = t.target_repo.clone().unwrap_or_else(|| repo_root.clone());
                TriggerListEntry {
                    trigger: t,
                    effective_repo,
                }
            })
            .collect())
    }

    pub async fn get(&self, id: &str) -> Result<Trigger, TriggerError> {
        match self.store.get(id).await {
            Ok(Some(trigger)) => Ok(trigger),
            Ok(None) => Err(TriggerError::NotFound("trigger not found".to_string())),
            Err(e) => {
                error!("failed to fetch trigger: {e}");
                Err(TriggerError::Storage("failed to fetch trigger".to_string()))
            }
        }
    }

    pub async fn fires(&self, id: &str) -> Result<Vec<TriggerFire>, TriggerError> {
        self.store.fires(id).await.map_err(|e| {
            error!("failed to list trigger fires: {e}");
            TriggerError::Storage("failed to list trigger fires".to_string())
        })
    }

    pub async fn health(&self) -> Health {
        let ms = self.last_tick_ms();
        let last_tick_at = (ms != 0)
            .then(|| chrono::DateTime::from_timestamp_millis(ms))
            .flatten()
            .map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
        // Fail-open on a read error, matching the tick gate: a paused daemon must
        // never look "dead", and a genuinely-broken store surfaces on the next tick.
        let paused = self.settings.paused().await.unwrap_or(false);
        Health {
            last_tick_at,
            tick_interval_secs: TICK_INTERVAL_SECS,
            paused,
        }
    }
}
