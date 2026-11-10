//! When a Trigger fires next: the heartbeat's interval, where a fire comes
//! from, and the one decision every writer of `next_fire_at` routes through.

use chrono::{DateTime, Utc};

use super::cron_schedule::CronSchedule;

/// How often the scheduler wakes up. Cron resolves to the minute; a 30 s tick
/// guarantees every slot is seen.
pub const TICK_INTERVAL_SECS: u64 = 30;

/// Where a fire evaluation comes from (#341, ADR-0027). A `Manual` fire ("Run
/// now") is a first-class fire — same guard, same overlap gate, same audit trail
/// — but is *always due* (the click is the schedule) and never touches
/// `next_fire_at` (the cron heartbeat owns it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FireSource {
    Cron,
    Manual,
}

impl FireSource {
    /// The `trigger_fires.source` column value.
    pub fn as_str(self) -> &'static str {
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
pub enum Transition<'a> {
    Create(&'a CronSchedule),
    /// A schedule edit (new cron).
    CronEdit(&'a CronSchedule),
    /// A pipeline repoint reviving a dormant Trigger (existing cron).
    Repoint(&'a CronSchedule),
    /// A "Run now" on a dormant Trigger whose references resolve again (the
    /// pipeline or repository came back): its schedule resumes forward from now.
    Revive(&'a CronSchedule),
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
/// [`super::trigger::UpdateTrigger::next_fire_at`]
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
        Create(s) | CronEdit(s) | Repoint(s) | Revive(s) | Enable(s) | CronTick(s) => {
            // `Some(None)` when the cron yields no future slot (e.g. Feb 30):
            // clearing `next_fire_at` stops an impossible expression firing.
            Some(s.next_fire_utc(now))
        }
        ManualFire => None,
        Dangling => Some(None),
    }
}
