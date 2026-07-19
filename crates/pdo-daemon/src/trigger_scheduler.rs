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

/// Where a fire evaluation comes from (#341, ADR-0027). `Cron` is the ~30 s
/// scheduler tick; `Manual` is a user clicking "Run now" (`POST
/// /triggers/{id}/fire`). A manual fire is a first-class fire — same guard,
/// same overlap gate, same audit trail — but is *always due* (the user's click
/// is the schedule) and never touches `next_fire_at` (the cron heartbeat owns
/// it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FireSource {
    Cron,
    Manual,
}

impl FireSource {
    /// The `trigger_fires.source` column value for this origin.
    pub fn as_str(self) -> &'static str {
        match self {
            FireSource::Cron => "cron",
            FireSource::Manual => "manual",
        }
    }
}

/// A lifecycle event that decides what happens to a Trigger's `next_fire_at`
/// (#372). Every writer of `next_fire_at` names its transition here and routes
/// through [`recompute_next_fire`], so "who recomputes the next fire, and to
/// what" lives in one exhaustive `match` instead of five scattered sites.
///
/// The five `advance` variants carry the `CronSchedule` **already parsed by the
/// calling site**: no re-parse, and the type makes an advance-without-schedule
/// unrepresentable. (`&Trigger` would risk reading a stale cron — a PATCH's new
/// cron lives in the request, not the stored row; `&str` would re-derive a parse
/// error each route already handles to render its own `400`.)
#[derive(Debug, Clone, Copy)]
pub enum Transition<'a> {
    /// A freshly created Trigger.
    Create(&'a CronSchedule),
    /// A schedule edit (new cron).
    CronEdit(&'a CronSchedule),
    /// A pipeline repoint reviving a dormant Trigger (existing cron).
    Repoint(&'a CronSchedule),
    /// Re-enabling a disabled Trigger. Decision B (#372, ADR-0012): recompute
    /// **forward** from `now`, skipping the missed slot — never a hidden
    /// catch-up fire. This arm is the behaviour change; before #372 the enable
    /// path left `next_fire_at` frozen in the past *by omission*.
    Enable(&'a CronSchedule),
    /// A scheduler tick advancing past the slot it just evaluated.
    CronTick(&'a CronSchedule),
    /// A manual "Run now" (#341, ADR-0027): leave `next_fire_at` intact — the
    /// cron heartbeat owns the schedule, a 14:32 click must not shift 15:00.
    ManualFire,
    /// A dangling pipeline/repo reference: stop firing (clear `next_fire_at`).
    Dangling,
}

/// The single writer-side decision for `next_fire_at` (#372). The return mirrors
/// [`crate::trigger_store::UpdateTrigger::next_fire_at`]
/// (`Option<Option<String>>`): `None` = leave the stored value alone;
/// `Some(None)` = set NULL; `Some(Some(s))` = write `s` (canonical UTC `…Z`).
///
/// The five ADVANCE arms share one body — deliberately. The exhaustive `match`
/// *proves* every transition chose a behaviour explicitly (the issue's ask), and
/// the compiler forces any future transition to choose too. The `Enable` arm
/// advancing (rather than leaving, as it did by omission) is the #372 fix.
pub fn recompute_next_fire(
    now: DateTime<Utc>,
    transition: Transition<'_>,
) -> Option<Option<String>> {
    use Transition::*;
    match transition {
        Create(s) | CronEdit(s) | Repoint(s) | Enable(s) | CronTick(s) => {
            // `Some(None)` when the cron yields no future slot (e.g. Feb 30):
            // that clears `next_fire_at`, so an impossible expression stops
            // firing — identical to the pre-#372 behaviour.
            Some(s.next_fire_utc(now))
        }
        ManualFire => None,
        Dangling => Some(None),
    }
}

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
/// `max_concurrent`). `guard` is the guard result (`None` for a cron-only trigger
/// with no guard command; the guard is run and wired in `lib.rs`).
pub fn plan_tick(
    trigger: &Trigger,
    now: DateTime<Utc>,
    live_run_count: usize,
    guard: Option<GuardResult>,
    prompt_required: bool,
    source: FireSource,
) -> TickPlan {
    let schedule = CronSchedule::parse(&trigger.cron);

    // A broken cron expression: the Trigger stops firing and surfaces an error
    // outcome rather than rotting silently.
    let (schedule, cron_invalid) = match schedule {
        Ok(s) => (Some(s), false),
        Err(_) => (None, true),
    };

    // A manual fire is always due (#341): the user's click *is* the schedule.
    // `decide()`'s silent `!enabled || !due` no-op stays cron-only — the manual
    // route rejects a disabled trigger with an explicit 409 before reaching
    // this path.
    let due = source == FireSource::Manual
        || trigger
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

    // Recompute the next fire forward from `now` (forward-only, no backfill),
    // through the single recompute seam (#372). `schedule` is already parsed
    // above (needed for `cron_invalid`), so the `CronTick` arm reuses it — no
    // re-parse, no second stringification.
    let next_fire_at = schedule
        .as_ref()
        .and_then(|s| recompute_next_fire(now, Transition::CronTick(s)).flatten());

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
                guard_stdout: None,
                guard_stderr: None,
                guard_exit_code: None,
                source: Some(source.as_str().to_string()),
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

    let record = record_for(&decision, source);

    TickPlan {
        decision,
        record,
        next_fire_at,
        cron_invalid: false,
    }
}

/// Map a decision to the audit record (if any) to persist this tick, stamped
/// with its origin (`cron` / `manual`, #341).
fn record_for(decision: &FireDecision, source: FireSource) -> Option<FireRecord> {
    use crate::fire_decision::SkipReason;
    match decision {
        FireDecision::Fire { .. } => Some(FireRecord {
            outcome: "fired".to_string(),
            reason: None,
            // run_id is filled by the caller once the Run is created.
            run_id: None,
            guard_stdout: None,
            guard_stderr: None,
            guard_exit_code: None,
            source: Some(source.as_str().to_string()),
        }),
        FireDecision::Skip { reason: None } => None,
        FireDecision::Skip {
            reason: Some(SkipReason::OverlapPreviousRunLive),
        } => Some(FireRecord {
            outcome: "skipped-overlap".to_string(),
            reason: Some("previous run still active".to_string()),
            run_id: None,
            guard_stdout: None,
            guard_stderr: None,
            guard_exit_code: None,
            source: Some(source.as_str().to_string()),
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
            guard_stdout: None,
            guard_stderr: None,
            guard_exit_code: None,
            source: Some(source.as_str().to_string()),
        }),
        // #244: carry the guard's captured stdout/stderr/exit code onto the audit
        // row so the fire history can explain *why* the guard skipped.
        FireDecision::Skip {
            reason:
                Some(SkipReason::GuardExitNonZero {
                    stdout,
                    stderr,
                    exit_code,
                }),
        } => Some(FireRecord {
            outcome: "guard-exit-nonzero".to_string(),
            reason: Some("guard exited non-zero".to_string()),
            run_id: None,
            guard_stdout: Some(stdout.clone()),
            guard_stderr: Some(stderr.clone()),
            guard_exit_code: *exit_code,
            source: Some(source.as_str().to_string()),
        }),
        FireDecision::Skip {
            reason: Some(SkipReason::GuardError { detail }),
        } => Some(FireRecord {
            outcome: "guard-error".to_string(),
            reason: Some(detail.clone()),
            run_id: None,
            guard_stdout: None,
            guard_stderr: None,
            guard_exit_code: None,
            source: Some(source.as_str().to_string()),
        }),
        FireDecision::Reject { reason } => Some(FireRecord {
            outcome: "error".to_string(),
            reason: Some(reason.clone()),
            run_id: None,
            guard_stdout: None,
            guard_stderr: None,
            guard_exit_code: None,
            source: Some(source.as_str().to_string()),
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
        let plan = plan_tick(&t, now, 0, None, false, FireSource::Cron);
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
        let plan = plan_tick(&t, now, 1, None, false, FireSource::Cron);
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
        let plan = plan_tick(&t, now, 2, None, false, FireSource::Cron);
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
        let plan = plan_tick(&t, now, 1, None, false, FireSource::Cron);
        assert!(matches!(plan.decision, FireDecision::Fire { .. }));
        assert_eq!(plan.record.as_ref().unwrap().outcome, "fired");
    }

    #[test]
    fn not_due_trigger_is_a_silent_noop_with_no_audit_row() {
        let t = trigger("* * * * *", Some("2999-01-01T00:00:00.000Z"));
        let now = at("2026-06-06T10:00:30.000Z");
        let plan = plan_tick(&t, now, 0, None, false, FireSource::Cron);
        assert_eq!(plan.decision, FireDecision::Skip { reason: None });
        assert!(plan.record.is_none());
    }

    #[test]
    fn missed_slots_are_forward_only_no_backfill() {
        // next_fire is far in the past (daemon was down for days); the recompute
        // jumps forward from `now`, never replaying the missed slots.
        let t = trigger("0 * * * *", Some("2026-06-01T09:00:00.000Z"));
        let now = at("2026-06-06T10:30:00.000Z");
        let plan = plan_tick(&t, now, 0, None, false, FireSource::Cron);
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
        let plan = plan_tick(&t, now, 0, None, false, FireSource::Cron);
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
        let plan = plan_tick(&t, now, 0, None, false, FireSource::Cron);
        assert_eq!(plan.decision, FireDecision::Skip { reason: None });
        assert!(plan.record.is_none());
    }

    // --- #341: manual fires (FireSource::Manual) ---

    #[test]
    fn manual_fire_is_due_even_when_next_fire_is_in_the_future() {
        // "Run now" at 14:32 with the next cron slot at 15:00: the manual fire
        // proceeds (the click is the schedule) — no waiting for the slot.
        let t = trigger("* * * * *", Some("2999-01-01T00:00:00.000Z"));
        let now = at("2026-06-06T10:00:30.000Z");
        let plan = plan_tick(&t, now, 0, None, false, FireSource::Manual);
        assert!(matches!(plan.decision, FireDecision::Fire { .. }));
        let record = plan.record.as_ref().unwrap();
        assert_eq!(record.outcome, "fired");
        assert_eq!(record.source.as_deref(), Some("manual"));
    }

    #[test]
    fn manual_fire_still_honours_the_overlap_gate() {
        // A manual fire is a first-class fire: the overlap ceiling applies to
        // it exactly as to a cron fire.
        let t = trigger("* * * * *", Some("2999-01-01T00:00:00.000Z"));
        let now = at("2026-06-06T10:00:30.000Z");
        let plan = plan_tick(&t, now, 1, None, false, FireSource::Manual);
        assert!(matches!(
            plan.decision,
            FireDecision::Skip { reason: Some(_) }
        ));
        let record = plan.record.as_ref().unwrap();
        assert_eq!(record.outcome, "skipped-overlap");
        assert_eq!(record.source.as_deref(), Some("manual"));
    }

    #[test]
    fn manual_fire_still_honours_the_guard() {
        // A guard exiting non-zero skips a manual fire too — same contract as
        // cron, audited with source=manual.
        let mut t = trigger("* * * * *", Some("2999-01-01T00:00:00.000Z"));
        t.guard_command = Some("exit 7".to_string());
        let now = at("2026-06-06T10:00:30.000Z");
        let guard = Some(GuardResult::Skip {
            stdout: String::new(),
            stderr: "no work".to_string(),
            exit_code: Some(7),
        });
        let plan = plan_tick(&t, now, 0, guard, false, FireSource::Manual);
        let record = plan.record.as_ref().unwrap();
        assert_eq!(record.outcome, "guard-exit-nonzero");
        assert_eq!(record.source.as_deref(), Some("manual"));
    }

    #[test]
    fn cron_records_are_stamped_source_cron() {
        let t = trigger("* * * * *", Some("2026-06-06T10:00:00.000Z"));
        let now = at("2026-06-06T10:00:30.000Z");
        let plan = plan_tick(&t, now, 0, None, false, FireSource::Cron);
        assert_eq!(
            plan.record.as_ref().unwrap().source.as_deref(),
            Some("cron")
        );
    }

    #[test]
    fn guard_exit_nonzero_plan_carries_captured_output_onto_the_record() {
        // #244: a guard that exits non-zero produces a `guard-exit-nonzero` audit
        // row carrying the captured stdout/stderr/exit code so the history can
        // explain the skip.
        let mut t = trigger("* * * * *", Some("2026-06-06T10:00:00.000Z"));
        t.guard_command = Some("printf 'out'; echo 'err' >&2; exit 7".to_string());
        let now = at("2026-06-06T10:00:30.000Z");
        let guard = Some(GuardResult::Skip {
            stdout: "checked 0 issues".to_string(),
            stderr: "gh: no work to do".to_string(),
            exit_code: Some(7),
        });
        let plan = plan_tick(&t, now, 0, guard, false, FireSource::Cron);

        assert!(matches!(
            plan.decision,
            FireDecision::Skip { reason: Some(_) }
        ));
        let record = plan.record.as_ref().unwrap();
        assert_eq!(record.outcome, "guard-exit-nonzero");
        assert_eq!(record.guard_stdout.as_deref(), Some("checked 0 issues"));
        assert_eq!(record.guard_stderr.as_deref(), Some("gh: no work to do"));
        assert_eq!(record.guard_exit_code, Some(7));
    }

    #[test]
    fn non_guard_records_leave_guard_output_none() {
        // A plain `fired` record must keep the three guard fields NULL (D2).
        let t = trigger("* * * * *", Some("2026-06-06T10:00:00.000Z"));
        let now = at("2026-06-06T10:00:30.000Z");
        let plan = plan_tick(&t, now, 0, None, false, FireSource::Cron);
        let record = plan.record.as_ref().unwrap();
        assert_eq!(record.outcome, "fired");
        assert!(record.guard_stdout.is_none());
        assert!(record.guard_stderr.is_none());
        assert!(record.guard_exit_code.is_none());
    }

    // --- #372: the single `recompute_next_fire` seam (the transition matrix) ---
    //
    // Each ADVANCE test starts from a `now` already *past* a slot to prove the
    // recompute jumps forward (strictly after `now`), never catching up the
    // missed slot.

    fn daily_nine() -> CronSchedule {
        CronSchedule::parse("0 9 * * *").expect("valid cron")
    }

    #[test]
    fn create_recomputes_forward() {
        let s = daily_nine();
        let now = at("2026-06-06T10:00:30.000Z"); // past today's 09:00
        let out = recompute_next_fire(now, Transition::Create(&s));
        assert_eq!(out, Some(Some("2026-06-07T09:00:00.000Z".to_string())));
        let fwd = out.flatten().unwrap();
        assert!(at(&fwd) > now, "create must recompute strictly forward");
    }

    #[test]
    fn cron_edit_recomputes_forward() {
        let s = daily_nine();
        let now = at("2026-06-06T10:00:30.000Z");
        let out = recompute_next_fire(now, Transition::CronEdit(&s));
        assert_eq!(out, Some(Some("2026-06-07T09:00:00.000Z".to_string())));
    }

    #[test]
    fn repoint_recomputes_forward() {
        let s = daily_nine();
        let now = at("2026-06-06T10:00:30.000Z");
        let out = recompute_next_fire(now, Transition::Repoint(&s));
        assert_eq!(out, Some(Some("2026-06-07T09:00:00.000Z".to_string())));
    }

    /// The load-bearing test for decision B (#372): re-enabling recomputes
    /// forward and skips the missed slot — no catch-up fire.
    #[test]
    fn enable_recomputes_forward_no_catchup() {
        let s = daily_nine();
        // Trigger was disabled around its 09:00 slot; re-enabled at 10:00.
        let now = at("2026-06-06T10:00:30.000Z");
        let out = recompute_next_fire(now, Transition::Enable(&s));
        let fwd = out.expect("advance").expect("a future slot");
        assert!(fwd.ends_with('Z'), "canonical UTC, got {fwd}");
        assert!(
            at(&fwd) > now,
            "enable must recompute strictly forward, never replay the missed slot"
        );
        assert_eq!(fwd, "2026-06-07T09:00:00.000Z");
    }

    #[test]
    fn cron_tick_recomputes_forward() {
        let s = daily_nine();
        let now = at("2026-06-06T10:00:30.000Z");
        let out = recompute_next_fire(now, Transition::CronTick(&s));
        assert_eq!(out, Some(Some("2026-06-07T09:00:00.000Z".to_string())));
    }

    /// An impossible-but-valid expression clears `next_fire_at` on any ADVANCE
    /// arm (`Some(None)`), so the Trigger stops firing — same as pre-#372.
    #[test]
    fn advance_on_impossible_cron_clears() {
        let s = CronSchedule::parse("0 0 30 2 *").expect("parses fine");
        let now = at("2026-06-06T10:00:30.000Z");
        assert_eq!(recompute_next_fire(now, Transition::Create(&s)), Some(None));
        assert_eq!(recompute_next_fire(now, Transition::Enable(&s)), Some(None));
    }

    /// A manual fire leaves `next_fire_at` untouched (ADR-0027): the seam returns
    /// `None` (do not write).
    #[test]
    fn manual_fire_leaves_next_fire_intact() {
        let now = at("2026-06-06T10:00:30.000Z");
        assert_eq!(recompute_next_fire(now, Transition::ManualFire), None);
    }

    /// A dangling reference clears `next_fire_at` (`Some(None)` → NULL): the
    /// Trigger stops firing.
    #[test]
    fn dangling_clears_next_fire() {
        let now = at("2026-06-06T10:00:30.000Z");
        assert_eq!(recompute_next_fire(now, Transition::Dangling), Some(None));
    }
}
