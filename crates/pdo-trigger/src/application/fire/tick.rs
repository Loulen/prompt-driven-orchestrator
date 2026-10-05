//! One heartbeat tick: fire every due Trigger the decision admits, recording
//! every significant outcome and recomputing each next fire.
//!
//! The heartbeat (a primary adapter: the clock) calls it every
//! [`TICK_INTERVAL_SECS`](crate::domain::schedule::TICK_INTERVAL_SECS); a test
//! calls it directly. Deciding that a Trigger is due asks no one; firing it is
//! an Operation whose Acteur is the Trigger.

use std::sync::atomic::Ordering;

use pdo_kernel::applied::{self, OperationApplied};
use pdo_kernel::attributes::EntityRef;
use pdo_kernel::{Actor, ResourceKind, Verb};
use tracing::warn;

use crate::application::{operation, FIRE};
use crate::domain::schedule::{recompute_next_fire, FireSource, Transition};
use crate::domain::trigger::{FireRecord, Trigger};
use crate::Triggers;

impl Triggers {
    pub async fn tick(&self) {
        let _tick = self.tick_lock.lock().await;
        let now = chrono::Utc::now();
        // Record liveness at tick START: a wedged tick then freezes the value, while an
        // isolated panic has already advanced it, so `/triggers/health` answers "is the
        // loop alive?".
        self.last_tick_ms
            .store(now.timestamp_millis(), Ordering::Relaxed);

        // Global kill-switch: skip the whole fire loop while paused. Read FRESH each
        // tick so `POST /triggers/pause` takes effect without a restart. Liveness above
        // still advances — paused ≠ dead. Because `due` is never called while paused,
        // `next_fire_at` stays frozen in the past, so on unpause the next ordinary tick
        // fires each due Trigger once and recomputes forward: a one-shot catch-up with
        // NO dedicated code and NO `Transition` variant. Fail-OPEN on a read error,
        // matching the scheduler's best-effort posture.
        if self.settings.paused().await.unwrap_or(false) {
            return;
        }

        let now_str = now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let due = match self.store.due(&now_str).await {
            Ok(t) => t,
            Err(e) => {
                warn!("trigger scheduler: due_triggers failed: {e}");
                return;
            }
        };

        for trigger in due {
            // TOCTOU close: re-read the kill-switch at the last moment. The tick-start
            // read is stale by now — `due` suspends on a storage round-trip, and `POST
            // /triggers/pause` deliberately does NOT take the tick lock (a lock held
            // across the guard subprocess would make the brake hang for ~guard-timeout,
            // and a brake that hangs is no brake). Without this re-read a tick inside
            // its critical section still fires a Run AFTER the operator paused. Bail the
            // whole tick: `next_fire_at` stays frozen, preserving the one-shot catch-up.
            // Fail-OPEN, same posture as above. "Run now" never reaches this Cron path,
            // so it stays exempt.
            if self.settings.paused().await.unwrap_or(false) {
                return;
            }

            if self.options.panic_on_trigger_name.as_deref() == Some(trigger.name.as_str()) {
                panic!(
                    "PDO_DEBUG_PANIC_TRIGGER fault injection (#222): trigger {}",
                    trigger.name
                );
            }

            // Surface an error outcome and stop firing (clear next_fire) rather than rot
            // silently or auto-delete. The clear goes through the single recompute seam.
            if let Some(reason) = self.dangling_reason(&trigger) {
                let cleared = recompute_next_fire(now, Transition::Dangling).flatten();
                let _ = self
                    .store
                    .set_next_fire(&trigger.id, cleared.as_deref())
                    .await;
                self.record_fire(&trigger.id, &cron_error(reason)).await;
                continue;
            }

            // API 0.8: a suspended Trigger spends its slot without firing, and
            // the history says why. It fires again at its next slot once lifted.
            if let Some(suspension) = self.suspension_of(&trigger) {
                self.skip_slot(&trigger, now, suspended(suspension.reason))
                    .await;
                continue;
            }

            // The fire is the Trigger's own gesture: its Acteur is the Trigger.
            let actor = Actor::Trigger {
                trigger_id: trigger.id.clone(),
            };
            let fire = operation(Verb::Action(FIRE.to_string()), Some(&trigger.id));
            if let Err(denied) = self.authorize(&actor, &fire) {
                self.refused(&trigger, now, denied.to_string()).await;
                continue;
            }

            self.fire_one(&trigger, now, FireSource::Cron).await;
            // A primary adapter of its own: the tick hands its fire to the
            // extensions, as the `identify` middleware does for a request.
            applied::publish(
                self.extensions(),
                vec![OperationApplied::new(
                    actor,
                    fire,
                    Some(EntityRef::new(ResourceKind::Trigger, trigger.id.clone())),
                )],
            );
        }
    }

    /// A cron fire an extension refused: the slot is spent (the schedule moves
    /// forward, as for a skip, so the next tick does not ask again) and the
    /// refusal is recorded in the fire history, where the operator looks.
    async fn refused(&self, trigger: &Trigger, now: chrono::DateTime<chrono::Utc>, reason: String) {
        self.skip_slot(trigger, now, cron_error(reason)).await;
    }

    /// Spend the slot of `trigger` (its schedule moves forward, as for a skip)
    /// and record `record` in its fire history.
    async fn skip_slot(
        &self,
        trigger: &Trigger,
        now: chrono::DateTime<chrono::Utc>,
        record: FireRecord,
    ) {
        let next = crate::domain::cron_schedule::CronSchedule::parse(&trigger.cron)
            .ok()
            .and_then(|s| recompute_next_fire(now, Transition::CronTick(&s)).flatten());
        if let Err(e) = self.store.set_next_fire(&trigger.id, next.as_deref()).await {
            warn!(
                "trigger scheduler: set_next_fire failed for {}: {e}",
                trigger.id
            );
        }
        self.record_fire(&trigger.id, &record).await;
    }
}

/// The `skipped-suspended` outcome of a cron slot a suspended Trigger spent.
fn suspended(reason: String) -> FireRecord {
    FireRecord {
        outcome: SKIPPED_SUSPENDED.to_string(),
        ..cron_error(reason)
    }
}

/// The outcome a suspended Trigger records for each slot it does not fire.
pub const SKIPPED_SUSPENDED: &str = "skipped-suspended";

/// An `error` outcome of a cron evaluation.
fn cron_error(reason: String) -> FireRecord {
    FireRecord {
        outcome: "error".to_string(),
        reason: Some(reason),
        run_id: None,
        guard_stdout: None,
        guard_stderr: None,
        guard_exit_code: None,
        source: Some(FireSource::Cron.as_str().to_string()),
    }
}
