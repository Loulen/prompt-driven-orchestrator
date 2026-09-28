//! Firing a Trigger: the shared spine of the heartbeat's tick ([`tick`]) and
//! of "Run now" ([`run_now`]) — overlap, guard, decision, next fire, the Run,
//! the fire's record.
//!
//! A fire is an Operation (`trigger` × `fire` on the Trigger): "Run now" asks
//! with the Acteur of the request, a cron fire with the Trigger itself
//! (`Acteur::Trigger`). Deciding *when* a Trigger is due is the heartbeat's
//! and the pure [`plan_tick`]'s; it asks no one.

pub mod run_now;
pub mod targets;
pub mod tick;

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use tracing::warn;

use super::ports::RunFromTrigger;
use crate::domain::fire_decision::{overlap_ceiling, FireDecision, OverlapPolicy};
use crate::domain::schedule::{recompute_next_fire, FireSource, Transition};
use crate::domain::tick_plan::plan_tick;
use crate::domain::trigger::{FireRecord, Trigger};
use crate::Triggers;

impl Triggers {
    /// Evaluate and (maybe) fire one Trigger. On `Cron` the next fire is
    /// recomputed forward; on `Manual` `next_fire_at` is left untouched, because the
    /// cron heartbeat owns the schedule and a 14:32 click must not shift the 15:00
    /// slot. Call it under [`Triggers::tick_lock`].
    ///
    /// `None` means the evaluation was a silent no-op.
    pub(crate) async fn fire_one(
        &self,
        trigger: &Trigger,
        now: DateTime<Utc>,
        source: FireSource,
    ) -> Option<FireRecord> {
        let live_count = self.runs.live_count(&trigger.id).await;
        let prompt_required = self.prompt_required(trigger);

        // Only run the guard when overlap wouldn't already skip — never spend a guard
        // run on a tick we'd skip anyway. Route it through the same `overlap_ceiling`
        // the decision core uses, or the guard-gate drifts from the fire decision.
        let overlap = if trigger.overlap_policy == "allow" {
            OverlapPolicy::Allow
        } else {
            OverlapPolicy::Skip
        };
        let overlap_skips =
            overlap_ceiling(overlap, trigger.max_concurrent.map(|m| m.max(0) as usize))
                .is_some_and(|ceiling| live_count >= ceiling);
        let guard = match (&trigger.guard_command, overlap_skips) {
            (Some(cmd), false) if !cmd.trim().is_empty() => {
                // Guaranteed present: `dangling_reason` refused the fire upstream
                // otherwise. No fallback to the daemon root.
                let cwd = PathBuf::from(trigger.target_repo.as_deref().unwrap_or_default());
                // Resolve the timeout fresh each tick so a settings change takes effect
                // without a restart.
                let timeout = self.settings.guard_timeout().await;
                Some(self.guards.run(cmd, &cwd, timeout).await)
            }
            _ => None,
        };

        let plan = plan_tick(trigger, now, live_count, guard, prompt_required, source);

        // Route the manual case through the recompute seam too, so the leave-vs-advance
        // rule stays in the one exhaustive match rather than an ad-hoc branch.
        let next_write: Option<Option<String>> = match source {
            FireSource::Cron => Some(plan.next_fire_at.clone()),
            FireSource::Manual => recompute_next_fire(now, Transition::ManualFire),
        };
        if let Some(next) = next_write {
            if let Err(e) = self.store.set_next_fire(&trigger.id, next.as_deref()).await {
                warn!(
                    "trigger scheduler: set_next_fire failed for {}: {e}",
                    trigger.id
                );
            }
        }

        match plan.decision {
            FireDecision::Fire { input } => {
                let record = self.start_run(trigger, input, source).await;
                self.record_fire(&trigger.id, &record).await;
                Some(record)
            }
            FireDecision::Skip { .. } | FireDecision::Reject { .. } => {
                let record = plan.record?;
                self.record_fire(&trigger.id, &record).await;
                Some(record)
            }
        }
    }

    /// Start the Run a `Fire` decision asked for, and say how it went.
    async fn start_run(&self, trigger: &Trigger, input: String, source: FireSource) -> FireRecord {
        // #804 / ADR-0070 §1: refresh the remotes BEFORE the cut, so a Trigger whose
        // source is a tracking ref fires on the server's state rather than on whatever
        // the last manual fetch happened to leave behind. Here, in the `Fire` arm
        // only: a tick we are about to skip must not touch the network. Never a
        // fast-forward — nobody is around to arbitrate one.
        //
        // It holds the (serialized) tick for up to the fetch timeout, exactly as the
        // guard subprocess already does and for the same reason: a fire is
        // read-decide-act, and the act needs the decision's inputs to still be true.
        // `next_fire_at` was written before, so a slow fetch delays THIS fire without
        // ever shifting the schedule.
        let source_fetch_error = self
            .repositories
            .refresh(&targets::fetch_targets(
                trigger.target_repo.as_deref(),
                trigger.target_repos.as_deref(),
            ))
            .await;
        let started = self
            .runs
            .start(RunFromTrigger {
                trigger: trigger.clone(),
                input,
                source_fetch_error,
            })
            .await;
        let (outcome, reason, run_id) = match started {
            Ok(run_id) => ("fired", None, Some(run_id)),
            Err(reason) => ("error", Some(reason), None),
        };
        FireRecord {
            outcome: outcome.to_string(),
            reason,
            run_id,
            guard_stdout: None,
            guard_stderr: None,
            guard_exit_code: None,
            source: Some(source.as_str().to_string()),
        }
    }

    /// Persist a fire's record and announce `trigger_fired`, so the UI refreshes
    /// live.
    pub(crate) async fn record_fire(&self, trigger_id: &str, record: &FireRecord) {
        if let Err(e) = self.store.record_fire(trigger_id, record).await {
            warn!("trigger scheduler: record_fire failed for {trigger_id}: {e}");
        }
        self.announce(serde_json::json!({
            "type": "trigger_fired",
            "trigger_id": trigger_id,
            "outcome": record.outcome,
            "run_id": record.run_id,
        }));
    }

    /// The target pipeline's `prompt_required` flag; `true` for a missing or
    /// unparseable pipeline, so a dangling reference rejects rather than fires blind.
    pub(crate) fn prompt_required(&self, trigger: &Trigger) -> bool {
        match self.pipelines.resolve(&trigger.pipeline_id) {
            Some(Ok(facts)) => facts.prompt_required,
            _ => true,
        }
    }

    /// Why a Trigger's external references forbid a fire, if they do — so the
    /// heartbeat surfaces an error outcome and stops firing rather than rotting
    /// silently.
    pub(crate) fn dangling_reason(&self, trigger: &Trigger) -> Option<String> {
        if self.pipelines.resolve(&trigger.pipeline_id).is_none() {
            return Some(format!("pipeline not found: {}", trigger.pipeline_id));
        }
        // A null/blank target repo is a dangling reference, not a fallback. Refuse
        // HERE rather than at the Run's create chokepoint, for three things a
        // chokepoint 400 cannot buy: the guard command never runs (guards do `git
        // pull` / `gh issue list`, which would execute in the unnamed daemon repo), the
        // `Dangling` transition NULLs `next_fire_at` so the Trigger goes dormant
        // instead of emitting one red row per tick forever, and `POST
        // /triggers/:id/fire` answers 409 with a banner instead of a `200
        // {fired:false}` buried in the fire history.
        self.repositories
            .required(trigger.target_repo.as_deref())
            .err()
    }
}
