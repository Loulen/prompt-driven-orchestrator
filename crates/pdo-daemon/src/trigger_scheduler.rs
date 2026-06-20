//! The Trigger scheduler: a background task (sibling of the reaper/stale tasks)
//! that ticks every ~30 s and fires due Triggers.
//!
//! The per-tick *decision* is factored into the pure `plan_tick`, which folds
//! `cron_schedule` + `fire_decision` together and recomputes the next fire.
//! The effectful `run_tick` drives the store and `create_run_core`. Side
//! effects (Run creation) are validated by integration tests, not unit tests
//! (CODING_STANDARDS); the planning logic is unit-tested here.

use chrono::{DateTime, Utc};

use crate::cron_schedule::CronSchedule;
use crate::fire_decision::{self, FireDecision, FireInputs, GuardResult, OverlapPolicy};
use crate::trigger_store::{FireRecord, Trigger};

/// How often the scheduler wakes up. Cron resolves to the minute; a 30 s tick
/// guarantees every slot is seen.
pub const TICK_INTERVAL_SECS: u64 = 30;

/// The plan for one Trigger on one tick: what to do, what to audit, and the
/// recomputed next fire.
#[derive(Debug, Clone, PartialEq)]
pub struct TickPlan {
    pub decision: FireDecision,
    /// The audit record to persist, if this tick was significant. A not-due /
    /// disabled no-op produces `None`.
    pub record: Option<FireRecord>,
    /// The next scheduled fire after `now`. `None` when the cron expression is
    /// unparseable or yields no future slot (the Trigger then stops firing and
    /// shows an error outcome — *Sharp tool*).
    pub next_fire_at: Option<String>,
    /// Whether the cron expression failed to parse (drives an error outcome).
    pub cron_invalid: bool,
}

/// Decide what to do for one Trigger at `now`, given the observable world.
///
/// `live_run_count` is the number of the Trigger's *own* Runs still live (#239):
/// compared against the overlap ceiling (`skip` ⇒ 1, bounded `allow` ⇒
/// `max_concurrent`). `guard` is the guard result (always `None` in the cron-only
/// slice).
pub fn plan_tick(
    trigger: &Trigger,
    now: DateTime<Utc>,
    live_run_count: usize,
    guard: Option<GuardResult>,
    prompt_required: bool,
) -> TickPlan {
    let schedule = CronSchedule::parse(&trigger.cron);

    // A broken cron expression: the Trigger stops firing and surfaces an error
    // outcome rather than rotting silently.
    let (schedule, cron_invalid) = match schedule {
        Ok(s) => (Some(s), false),
        Err(_) => (None, true),
    };

    let due = trigger
        .next_fire_at
        .as_deref()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|nf| nf.with_timezone(&Utc) <= now)
        .unwrap_or(false);

    let overlap = if trigger.overlap_policy == "allow" {
        OverlapPolicy::Allow
    } else {
        OverlapPolicy::Skip
    };

    // Recompute the next fire forward from `now` (forward-only, no backfill).
    let next_fire_at = schedule.as_ref().and_then(|s| {
        s.next_fire_after(now)
            .map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
    });

    if cron_invalid {
        // Only audit an error once we'd otherwise have acted (it's due-ish);
        // but a broken cron has no next_fire, so it never becomes due again.
        // Surface the error outcome on this evaluation.
        return TickPlan {
            decision: FireDecision::Reject {
                reason: format!("invalid cron expression: {}", trigger.cron),
            },
            record: Some(FireRecord {
                outcome: "error".to_string(),
                reason: Some(format!("invalid cron expression: {}", trigger.cron)),
                run_id: None,
            }),
            next_fire_at: None,
            cron_invalid: true,
        };
    }

    let decision = fire_decision::decide(&FireInputs {
        enabled: trigger.enabled,
        due,
        overlap,
        live_run_count,
        // Store holds `Option<i64>`; convert to the decision core's `usize` at
        // this one boundary (clamp a stray negative to 0, then `overlap_ceiling`
        // clamps a 0 ceiling up to 1 defensively).
        max_concurrent: trigger.max_concurrent.map(|m| m.max(0) as usize),
        guard,
        input_template: &trigger.input_template,
        prompt_required,
    });

    let record = record_for(&decision);

    TickPlan {
        decision,
        record,
        next_fire_at,
        cron_invalid: false,
    }
}

/// Map a decision to the audit record (if any) to persist this tick.
fn record_for(decision: &FireDecision) -> Option<FireRecord> {
    use crate::fire_decision::SkipReason;
    match decision {
        FireDecision::Fire { .. } => Some(FireRecord {
            outcome: "fired".to_string(),
            reason: None,
            // run_id is filled by the caller once the Run is created.
            run_id: None,
        }),
        FireDecision::Skip { reason: None } => None,
        FireDecision::Skip {
            reason: Some(SkipReason::OverlapPreviousRunLive),
        } => Some(FireRecord {
            outcome: "skipped-overlap".to_string(),
            reason: Some("previous run still active".to_string()),
            run_id: None,
        }),
        // A bounded-`allow` skip keeps the `skipped-overlap` outcome (#239) — no
        // new status-dot to teach the UI — but carries the cap in its reason so
        // the history panel answers "why" precisely.
        FireDecision::Skip {
            reason: Some(SkipReason::OverlapMaxConcurrentReached { live, max }),
        } => Some(FireRecord {
            outcome: "skipped-overlap".to_string(),
            reason: Some(format!("max concurrent runs reached ({live}/{max})")),
            run_id: None,
        }),
        FireDecision::Skip {
            reason: Some(SkipReason::GuardExitNonZero),
        } => Some(FireRecord {
            outcome: "guard-exit-nonzero".to_string(),
            reason: Some("guard exited non-zero".to_string()),
            run_id: None,
        }),
        FireDecision::Skip {
            reason: Some(SkipReason::GuardError { detail }),
        } => Some(FireRecord {
            outcome: "guard-error".to_string(),
            reason: Some(detail.clone()),
            run_id: None,
        }),
        FireDecision::Reject { reason } => Some(FireRecord {
            outcome: "error".to_string(),
            reason: Some(reason.clone()),
            run_id: None,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trigger_store::Trigger;

    fn trigger(cron: &str, next_fire_at: Option<&str>) -> Trigger {
        Trigger {
            id: "trg-1".to_string(),
            name: "t".to_string(),
            pipeline_id: "p".to_string(),
            pipeline_name: "P".to_string(),
            target_repo: None,
            source_branch: None,
            input_template: "do it".to_string(),
            variables: serde_json::json!({}),
            cron: cron.to_string(),
            guard_command: None,
            overlap_policy: "skip".to_string(),
            max_concurrent: None,
            enabled: true,
            next_fire_at: next_fire_at.map(str::to_string),
            last_fired_at: None,
            last_outcome: None,
        }
    }

    fn at(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn due_cron_only_trigger_plans_a_fire_and_recomputes_next() {
        let t = trigger("* * * * *", Some("2026-06-06T10:00:00.000Z"));
        let now = at("2026-06-06T10:00:30.000Z");
        let plan = plan_tick(&t, now, 0, None, false);
        assert_eq!(
            plan.decision,
            FireDecision::Fire {
                input: "do it".to_string()
            }
        );
        assert_eq!(plan.record.as_ref().unwrap().outcome, "fired");
        // Next fire is strictly after now, at the next whole minute.
        assert_eq!(
            plan.next_fire_at.as_deref(),
            Some("2026-06-06T10:01:00.000Z")
        );
    }

    #[test]
    fn overlap_skip_while_own_run_is_live_records_skip_and_still_recomputes() {
        let t = trigger("* * * * *", Some("2026-06-06T10:00:00.000Z"));
        let now = at("2026-06-06T10:00:30.000Z");
        let plan = plan_tick(&t, now, 1, None, false);
        assert!(matches!(
            plan.decision,
            FireDecision::Skip { reason: Some(_) }
        ));
        assert_eq!(plan.record.as_ref().unwrap().outcome, "skipped-overlap");
        // Even when skipped, the schedule advances so we don't re-evaluate the
        // same slot forever.
        assert_eq!(
            plan.next_fire_at.as_deref(),
            Some("2026-06-06T10:01:00.000Z")
        );
    }

    #[test]
    fn bounded_allow_skip_at_cap_records_skipped_overlap_with_count() {
        // #239: an `allow` Trigger at its `max_concurrent` cap skips, audited as
        // `skipped-overlap` with the cap in the reason, and the schedule still
        // advances.
        let mut t = trigger("* * * * *", Some("2026-06-06T10:00:00.000Z"));
        t.overlap_policy = "allow".to_string();
        t.max_concurrent = Some(2);
        let now = at("2026-06-06T10:00:30.000Z");
        let plan = plan_tick(&t, now, 2, None, false);
        assert!(matches!(
            plan.decision,
            FireDecision::Skip { reason: Some(_) }
        ));
        let record = plan.record.as_ref().unwrap();
        assert_eq!(record.outcome, "skipped-overlap");
        assert!(
            record.reason.as_deref().unwrap().contains("(2/2)"),
            "reason must carry the cap: {:?}",
            record.reason
        );
        assert_eq!(
            plan.next_fire_at.as_deref(),
            Some("2026-06-06T10:01:00.000Z")
        );
    }

    #[test]
    fn bounded_allow_below_cap_fires() {
        let mut t = trigger("* * * * *", Some("2026-06-06T10:00:00.000Z"));
        t.overlap_policy = "allow".to_string();
        t.max_concurrent = Some(2);
        let now = at("2026-06-06T10:00:30.000Z");
        let plan = plan_tick(&t, now, 1, None, false);
        assert!(matches!(plan.decision, FireDecision::Fire { .. }));
        assert_eq!(plan.record.as_ref().unwrap().outcome, "fired");
    }

    #[test]
    fn not_due_trigger_is_a_silent_noop_with_no_audit_row() {
        let t = trigger("* * * * *", Some("2999-01-01T00:00:00.000Z"));
        let now = at("2026-06-06T10:00:30.000Z");
        let plan = plan_tick(&t, now, 0, None, false);
        assert_eq!(plan.decision, FireDecision::Skip { reason: None });
        assert!(plan.record.is_none());
    }

    #[test]
    fn missed_slots_are_forward_only_no_backfill() {
        // next_fire is far in the past (daemon was down for days); the recompute
        // jumps forward from `now`, never replaying the missed slots.
        let t = trigger("0 * * * *", Some("2026-06-01T09:00:00.000Z"));
        let now = at("2026-06-06T10:30:00.000Z");
        let plan = plan_tick(&t, now, 0, None, false);
        assert!(matches!(plan.decision, FireDecision::Fire { .. }));
        // The single next fire is the *next* hourly slot after now, not a
        // backfill of June 1.
        assert_eq!(
            plan.next_fire_at.as_deref(),
            Some("2026-06-06T11:00:00.000Z")
        );
    }

    #[test]
    fn invalid_cron_yields_error_outcome_and_stops_firing() {
        let t = trigger("not a cron", Some("2026-06-06T10:00:00.000Z"));
        let now = at("2026-06-06T10:00:30.000Z");
        let plan = plan_tick(&t, now, 0, None, false);
        assert!(matches!(plan.decision, FireDecision::Reject { .. }));
        assert_eq!(plan.record.as_ref().unwrap().outcome, "error");
        // No next fire: the broken trigger stops firing until edited.
        assert!(plan.next_fire_at.is_none());
        assert!(plan.cron_invalid);
    }

    #[test]
    fn disabled_trigger_is_a_noop_even_when_due() {
        let mut t = trigger("* * * * *", Some("2020-01-01T00:00:00.000Z"));
        t.enabled = false;
        let now = at("2026-06-06T10:00:30.000Z");
        let plan = plan_tick(&t, now, 0, None, false);
        assert_eq!(plan.decision, FireDecision::Skip { reason: None });
        assert!(plan.record.is_none());
    }
}
