//! "Run now" (`POST /triggers/{id}/fire`, ADR-0027): `trigger` × `fire`, asked
//! with the Acteur of the request.
//!
//! A first-class fire: same guard, same overlap gate, same fire record
//! (`source = "manual"`), same `triggered_by` provenance — but `next_fire_at` is
//! left untouched, because the cron heartbeat owns the schedule. One exception:
//! a Trigger a dangling reference put to sleep (no next fire) whose references
//! resolve again resumes its schedule forward from the click.

use pdo_kernel::{Actor, Verb};
use tracing::error;

use crate::application::{TriggerError, FIRE};
use crate::domain::cron_schedule::CronSchedule;
use crate::domain::schedule::{recompute_next_fire, FireSource, Transition};
use crate::domain::trigger::FireRecord;
use crate::Triggers;

impl Triggers {
    /// Contract (ADR-0025): refused → 403; unknown → 404; disabled, suspended
    /// (API 0.8) or broken reference → 409, before any effect and with no fire record; otherwise the
    /// record of the evaluation — a guard/overlap skip is an honest answer, not
    /// an error. `None` is a silent no-op.
    pub async fn run_now(
        &self,
        actor: &Actor,
        id: &str,
    ) -> Result<Option<FireRecord>, TriggerError> {
        let fire = self.on_trigger(Verb::Action(FIRE.to_string()), id).await;
        self.authorize(actor, &fire)?;
        let trigger = match self.store.get(id).await {
            Ok(Some(t)) => t,
            Ok(None) => return Err(TriggerError::NotFound(format!("trigger not found: {id}"))),
            Err(e) => {
                error!("failed to load trigger {id}: {e}");
                return Err(TriggerError::Storage("failed to load trigger".to_string()));
            }
        };

        // Disabled ⇒ explicit 409 before any effect. A human click on a paused trigger
        // deserves a truthful refusal, not a silent skip (ADR-0025); `decide()`'s silent
        // no-op stays cron-only.
        if !trigger.enabled {
            return Err(TriggerError::Conflict(format!(
                "trigger '{}' is disabled — enable it first",
                trigger.name
            )));
        }

        // API 0.8: a suspended Trigger does not fire, and the click says why.
        // Named as the user knows it: the message lands as is in a toast.
        if let Some(suspension) = self.suspension_of(&trigger) {
            return Err(TriggerError::Conflict(format!(
                "trigger '{}' is suspended: {}",
                trigger.name, suspension.reason
            )));
        }

        // Refuse explicitly rather than record an error outcome — the cron path keeps
        // its own audited treatment.
        if let Some(reason) = self.dangling_reason(&trigger) {
            return Err(TriggerError::Conflict(format!(
                "broken reference: {reason}"
            )));
        }

        // Serialize with the cron tick, or a manual fire and a tick race the same
        // overlap window.
        let _tick = self.tick_lock.lock().await;
        let now = chrono::Utc::now();
        // A dangling reference put the Trigger to sleep (no next fire); its
        // references resolve again, so the click that proves it also resumes its
        // schedule — never a Trigger that reads ON and silently never fires.
        if trigger.next_fire_at.is_none() {
            if let Ok(schedule) = CronSchedule::parse(&trigger.cron) {
                if let Some(next) = recompute_next_fire(now, Transition::Revive(&schedule)) {
                    if let Err(e) = self.store.set_next_fire(&trigger.id, next.as_deref()).await {
                        error!("failed to revive trigger {id}: {e}");
                    }
                }
            }
        }
        Ok(self.fire_one(&trigger, now, FireSource::Manual).await)
    }
}
