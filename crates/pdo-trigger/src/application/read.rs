//! The reads of the context: the list, one Trigger, its fires, the heartbeat's
//! health. Reads change nothing: they ask only whether the actor may read the
//! Trigger, in the Projets of its target repository (`read_access.rs`).

use pdo_kernel::attributes::{self, AttributeFilter, AttributedList};
use pdo_kernel::authorization::EntityRead;
use pdo_kernel::{Actor, ResourceKind};
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

/// How many times `health` asks the store again for the pause after an error.
const PAUSED_READ_RETRIES: usize = 2;

/// The answer about a Trigger that does not exist.
fn not_found() -> TriggerError {
    TriggerError::NotFound("trigger not found".to_string())
}

impl Triggers {
    /// Every Trigger `actor` may read carrying the attributes `filter` asks
    /// for, each with its attributes.
    pub async fn list(
        &self,
        actor: &Actor,
        filter: AttributeFilter,
    ) -> Result<Vec<TriggerListEntry>, TriggerError> {
        let mut triggers = self.store.list().await.map_err(|e| {
            error!("failed to list triggers: {e}");
            TriggerError::Storage("failed to list triggers".to_string())
        })?;
        // Read-side fallback only, and it stays: historical rows legitimately carry a
        // null repo (the write boundary now refuses one), and the "no Unassigned
        // bucket" invariant depends on resolving them here. Do NOT symmetrise it with
        // create/update. The raw `target_repo` is left untouched so the badge / detail
        // / Run-now pre-fill keep showing only what the user actually set.
        let repo_root = self.options.repo_root.to_string_lossy().into_owned();
        let mut readable = self.readable(actor);
        let mut kept = Vec::with_capacity(triggers.len());
        for trigger in triggers.drain(..) {
            if readable.keeps(&trigger).await {
                kept.push(trigger);
            }
        }
        let triggers = kept;
        let attributed =
            AttributedList::read(self.extensions(), ResourceKind::Trigger, filter).await;
        Ok(triggers
            .into_iter()
            .filter(|t| attributed.keeps(&t.id))
            .map(|mut t| {
                t.attributes = attributed.of(&t.id);
                let t = self.with_suspension(t);
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
            Ok(None) => Err(not_found()),
            Err(e) => {
                error!("failed to fetch trigger: {e}");
                Err(TriggerError::Storage("failed to fetch trigger".to_string()))
            }
        }
    }

    /// One Trigger `actor` may read, with its attributes.
    pub async fn attributed(&self, actor: &Actor, id: &str) -> Result<Trigger, TriggerError> {
        if self.authorize_read(actor, id).await? == EntityRead::AsUnknown {
            return Err(not_found());
        }
        let mut trigger = self.get(id).await?;
        trigger.attributes =
            attributes::of_entity(self.extensions(), ResourceKind::Trigger, id).await;
        Ok(self.with_suspension(trigger))
    }

    /// The recent fires of a Trigger `actor` may read; none, as for an
    /// unknown Trigger, on a concealed refusal.
    pub async fn fires(&self, actor: &Actor, id: &str) -> Result<Vec<TriggerFire>, TriggerError> {
        if self.authorize_read(actor, id).await? == EntityRead::AsUnknown {
            return Ok(Vec::new());
        }
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
        // A busy store (a write of the same daemon holding the database) is asked
        // again first: one transient refusal must not report a paused instance
        // as running (CORE-232, seen under load).
        let mut paused = self.settings.paused().await;
        for _ in 0..PAUSED_READ_RETRIES {
            if paused.is_ok() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            paused = self.settings.paused().await;
        }
        let paused = paused.unwrap_or(false);
        Health {
            last_tick_at,
            tick_interval_secs: TICK_INTERVAL_SECS,
            paused,
        }
    }
}
