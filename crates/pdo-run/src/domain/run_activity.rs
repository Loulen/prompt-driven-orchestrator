//! The **Activité d'un Run** (CONTEXT.md): the moments something happened in
//! it, as the Résumé de Run keeps them for the date filter of the lists. A Run
//! active on Monday then on Wednesday is active on both days, and not on
//! Tuesday.
//!
//! The moments are kept as spans: events less than [`ACTIVITY_GAP`] apart
//! join one span. A span carries instants, not days: the day of an instant is
//! the reader's (its time zone), never the daemon's.

use chrono::{DateTime, Duration, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::domain::event::{Event, EventKind};

/// Two events closer than this belong to the same span of activity.
pub const ACTIVITY_GAP: Duration = Duration::hours(1);

/// A span of activity of a Run: from its first event to its last, both RFC
/// 3339 instants in UTC. A lone event is a span of one instant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ActivitySpan {
    pub from: String,
    pub to: String,
}

/// Whether `kind` is something happening in the Run. The bookkeeping around
/// it is not: renaming or archiving it, the daemon noticing a node went quiet
/// or stuck on a usage limit, a lint of its pipeline.
fn is_activity(kind: &EventKind) -> bool {
    !matches!(
        kind,
        EventKind::RunRenamed
            | EventKind::RunArchived
            | EventKind::NodeStale
            | EventKind::NodeBlockedOnLimit
            | EventKind::PipelineLint
    )
}

/// The spans of activity of the Run whose log is `events`, oldest first. An
/// event whose `ts` is not an RFC 3339 instant is left out.
pub fn activity_of(events: &[Event]) -> Vec<ActivitySpan> {
    let mut instants: Vec<DateTime<Utc>> = events
        .iter()
        .filter(|e| is_activity(&e.kind))
        .filter_map(|e| DateTime::parse_from_rfc3339(&e.ts).ok())
        .map(|ts| ts.with_timezone(&Utc))
        .collect();
    instants.sort();
    let mut spans: Vec<(DateTime<Utc>, DateTime<Utc>)> = Vec::new();
    for instant in instants {
        match spans.last_mut() {
            Some((_, to)) if instant - *to < ACTIVITY_GAP => *to = instant,
            _ => spans.push((instant, instant)),
        }
    }
    spans
        .into_iter()
        .map(|(from, to)| ActivitySpan {
            from: instant(from),
            to: instant(to),
        })
        .collect()
}

fn instant(ts: DateTime<Utc>) -> String {
    ts.to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn at(kind: EventKind, ts: &str) -> Event {
        Event {
            id: None,
            run_id: "r".into(),
            ts: ts.into(),
            kind,
            node_id: None,
            iter: None,
            payload: None,
        }
    }

    fn span(from: &str, to: &str) -> ActivitySpan {
        ActivitySpan {
            from: from.into(),
            to: to.into(),
        }
    }

    #[test]
    fn a_log_without_events_has_no_activity() {
        assert_eq!(activity_of(&[]), Vec::new());
    }

    #[test]
    fn events_close_together_make_one_span() {
        let events = [
            at(EventKind::RunStarted, "2026-10-12T09:00:00Z"),
            at(EventKind::NodeStarted, "2026-10-12T09:00:01.250Z"),
            at(EventKind::NodeCompleted, "2026-10-12T09:59:00Z"),
        ];
        assert_eq!(
            activity_of(&events),
            vec![span("2026-10-12T09:00:00.000Z", "2026-10-12T09:59:00.000Z")]
        );
    }

    #[test]
    fn a_run_active_monday_and_wednesday_has_two_spans() {
        let events = [
            at(EventKind::RunStarted, "2026-10-12T09:00:00Z"),
            at(EventKind::NodeAwaitingUser, "2026-10-12T09:10:00Z"),
            at(EventKind::NodeResumed, "2026-10-14T16:00:00+02:00"),
            at(EventKind::RunCompleted, "2026-10-14T14:30:00Z"),
        ];
        assert_eq!(
            activity_of(&events),
            vec![
                span("2026-10-12T09:00:00.000Z", "2026-10-12T09:10:00.000Z"),
                span("2026-10-14T14:00:00.000Z", "2026-10-14T14:30:00.000Z"),
            ]
        );
    }

    #[test]
    fn bookkeeping_is_not_activity() {
        let events = [
            at(EventKind::RunStarted, "2026-10-12T09:00:00Z"),
            at(EventKind::RunCompleted, "2026-10-12T09:30:00Z"),
            at(EventKind::NodeStale, "2026-10-13T09:00:00Z"),
            at(EventKind::PipelineLint, "2026-10-13T09:00:00Z"),
            at(EventKind::NodeBlockedOnLimit, "2026-10-13T10:00:00Z"),
            at(EventKind::RunRenamed, "2026-10-14T09:00:00Z"),
            at(EventKind::RunArchived, "2026-10-15T09:00:00Z"),
        ];
        assert_eq!(
            activity_of(&events),
            vec![span("2026-10-12T09:00:00.000Z", "2026-10-12T09:30:00.000Z")]
        );
    }

    #[test]
    fn events_out_of_order_or_unreadable_still_give_the_spans() {
        let events = [
            at(EventKind::NodeCompleted, "2026-10-14T10:00:00Z"),
            at(EventKind::RunStarted, "2026-10-12T09:00:00Z"),
            at(EventKind::NodeStarted, "not a date"),
        ];
        assert_eq!(
            activity_of(&events),
            vec![
                span("2026-10-12T09:00:00.000Z", "2026-10-12T09:00:00.000Z"),
                span("2026-10-14T10:00:00.000Z", "2026-10-14T10:00:00.000Z"),
            ]
        );
    }
}
