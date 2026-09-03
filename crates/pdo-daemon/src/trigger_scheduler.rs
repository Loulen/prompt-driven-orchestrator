//! The Trigger scheduler: a background task (sibling of the reaper/stale tasks)
//! that ticks every ~30 s and fires due Triggers.
//!
//! The per-tick *decision* is the pure `plan_tick`; the effectful `run_tick`
//! drives the store and `create_run_core`. Run creation is validated by
//! integration tests, not unit tests (CODING_STANDARDS).

use chrono::{DateTime, Utc};

use crate::cron_schedule::CronSchedule;
use crate::fire_decision::{self, FireDecision, FireInputs, GuardResult, OverlapPolicy};
use crate::trigger_store::{FireRecord, Trigger};

/// How often the scheduler wakes up. Cron resolves to the minute; a 30 s tick
/// guarantees every slot is seen.
pub(crate) const TICK_INTERVAL_SECS: u64 = 30;

/// Where a fire evaluation comes from (#341, ADR-0027). A `Manual` fire ("Run
/// now") is a first-class fire — same guard, same overlap gate, same audit trail
/// — but is *always due* (the click is the schedule) and never touches
/// `next_fire_at` (the cron heartbeat owns it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FireSource {
    Cron,
    Manual,
}

impl FireSource {
    /// The `trigger_fires.source` column value.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            FireSource::Cron => "cron",
            FireSource::Manual => "manual",
        }
    }
}

/// A lifecycle event that decides what happens to a Trigger's `next_fire_at`
/// (#372). Every writer names its transition here and routes through
/// [`recompute_next_fire`], so the decision lives in one exhaustive `match`
/// instead of five scattered sites.
///
/// The advance variants carry a `CronSchedule` **already parsed by the calling
/// site**. Don't take `&Trigger` (a PATCH's new cron lives in the request, not
/// the stored row, so it would read a stale cron) nor `&str` (it would re-derive
/// a parse error each route already handles to render its own `400`).
#[derive(Debug, Clone, Copy)]
pub(crate) enum Transition<'a> {
    Create(&'a CronSchedule),
    /// A schedule edit (new cron).
    CronEdit(&'a CronSchedule),
    /// A pipeline repoint reviving a dormant Trigger (existing cron).
    Repoint(&'a CronSchedule),
    /// Re-enabling a disabled Trigger. Decision B (#372, ADR-0012): recompute
    /// **forward** from `now`, skipping the missed slot — never a hidden
    /// catch-up fire. Before #372 this path left `next_fire_at` frozen in the
    /// past *by omission*.
    Enable(&'a CronSchedule),
    /// A scheduler tick advancing past the slot it just evaluated.
    CronTick(&'a CronSchedule),
    /// A manual "Run now" (#341, ADR-0027): leave `next_fire_at` intact — a
    /// 14:32 click must not shift the 15:00 slot.
    ManualFire,
    /// A dangling pipeline/repo reference: stop firing.
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
pub(crate) fn recompute_next_fire(
    now: DateTime<Utc>,
    transition: Transition<'_>,
) -> Option<Option<String>> {
    use Transition::*;
    match transition {
        Create(s) | CronEdit(s) | Repoint(s) | Enable(s) | CronTick(s) => {
            // `Some(None)` when the cron yields no future slot (e.g. Feb 30):
            // clearing `next_fire_at` stops an impossible expression firing.
            Some(s.next_fire_utc(now))
        }
        ManualFire => None,
        Dangling => Some(None),
    }
}

/// The plan for one Trigger on one tick: what to do, what to audit, and the
/// recomputed next fire.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TickPlan {
    pub decision: FireDecision,
    /// The audit record to persist. A not-due / disabled no-op produces `None`.
    pub record: Option<FireRecord>,
    /// The next scheduled fire after `now`. `None` when the cron is unparseable
    /// or yields no future slot — the Trigger then stops firing.
    pub next_fire_at: Option<String>,
    pub cron_invalid: bool,
}

/// `live_run_count` is the number of the Trigger's *own* Runs still live (#239):
/// compared against the overlap ceiling (`skip` ⇒ 1, bounded `allow` ⇒
/// `max_concurrent`). `guard` is the guard result (`None` for a cron-only trigger
/// with no guard command; the guard is run and wired in `lib.rs`).
pub(crate) fn plan_tick(
    trigger: &Trigger,
    now: DateTime<Utc>,
    live_run_count: usize,
    guard: Option<GuardResult>,
    prompt_required: bool,
    source: FireSource,
) -> TickPlan {
    let schedule = CronSchedule::parse(&trigger.cron);

    let (schedule, cron_invalid) = match schedule {
        Ok(s) => (Some(s), false),
        Err(_) => (None, true),
    };

    // A manual fire is always due (#341): the click *is* the schedule. The
    // manual route rejects a disabled trigger with a 409 before reaching here,
    // so `decide()`'s silent `!enabled` no-op stays effectively cron-only.
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

    // Forward-only from `now`, no backfill of missed slots (#372).
    let next_fire_at = schedule
        .as_ref()
        .and_then(|s| recompute_next_fire(now, Transition::CronTick(s)).flatten());

    if cron_invalid {
        // A broken cron has no next_fire, so it never becomes due again — audit
        // the error on this evaluation or it never surfaces at all.
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
        // The store holds a signed `i64`; clamp a stray negative to 0 here
        // (`overlap_ceiling` then clamps a 0 ceiling back up to 1).
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
        // A bounded-`allow` skip reuses the `skipped-overlap` outcome (#239)
        // rather than adding a status-dot the UI would have to learn; the cap
        // lives in the reason instead.
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
            skills: Vec::new(),
            id: "trg-1".to_string(),
            name: "t".to_string(),
            pipeline_id: "p".to_string(),
            pipeline_name: "P".to_string(),
            target_repo: None,
            target_repos: None,
            source_branch: None,
            input_template: "do it".to_string(),
            variables: serde_json::json!({}),
            cron: cron.to_string(),
            guard_command: None,
            overlap_policy: "skip".to_string(),
            max_concurrent: None,
            sandbox: None,
            harness: None,
            agent_choice: None,
            auto_name: true,
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
        // next_fire is days in the past (daemon was down).
        let t = trigger("0 * * * *", Some("2026-06-01T09:00:00.000Z"));
        let now = at("2026-06-06T10:30:00.000Z");
        let plan = plan_tick(&t, now, 0, None, false, FireSource::Cron);
        assert!(matches!(plan.decision, FireDecision::Fire { .. }));
        // The next hourly slot after `now`, not a backfill of June 1.
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

    #[test]
    fn manual_fire_is_due_even_when_next_fire_is_in_the_future() {
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
        let t = trigger("* * * * *", Some("2026-06-06T10:00:00.000Z"));
        let now = at("2026-06-06T10:00:30.000Z");
        let plan = plan_tick(&t, now, 0, None, false, FireSource::Cron);
        let record = plan.record.as_ref().unwrap();
        assert_eq!(record.outcome, "fired");
        assert!(record.guard_stdout.is_none());
        assert!(record.guard_stderr.is_none());
        assert!(record.guard_exit_code.is_none());
    }

    // Each ADVANCE test below starts from a `now` already *past* a slot, to prove
    // the recompute jumps strictly forward and never catches up the missed slot.

    fn daily_nine() -> CronSchedule {
        CronSchedule::parse("0 9 * * *").expect("valid cron")
    }

    #[test]
    fn create_recomputes_forward() {
        let s = daily_nine();
        let now = at("2026-06-06T10:00:30.000Z"); // past today's 09:00 slot
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

    /// The load-bearing test for decision B (#372).
    #[test]
    fn enable_recomputes_forward_no_catchup() {
        let s = daily_nine();
        // Disabled around its 09:00 slot; re-enabled at 10:00.
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
    /// arm, so the Trigger stops firing.
    #[test]
    fn advance_on_impossible_cron_clears() {
        let s = CronSchedule::parse("0 0 30 2 *").expect("parses fine");
        let now = at("2026-06-06T10:00:30.000Z");
        assert_eq!(recompute_next_fire(now, Transition::Create(&s)), Some(None));
        assert_eq!(recompute_next_fire(now, Transition::Enable(&s)), Some(None));
    }

    /// A manual fire leaves `next_fire_at` untouched (ADR-0027).
    #[test]
    fn manual_fire_leaves_next_fire_intact() {
        let now = at("2026-06-06T10:00:30.000Z");
        assert_eq!(recompute_next_fire(now, Transition::ManualFire), None);
    }

    #[test]
    fn dangling_clears_next_fire() {
        let now = at("2026-06-06T10:00:30.000Z");
        assert_eq!(recompute_next_fire(now, Transition::Dangling), Some(None));
    }
}
