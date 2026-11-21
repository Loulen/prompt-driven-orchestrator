//! When an Agent de support counts as idle, and what the daemon says of it.
//!
//! Idle means no input from its person: no keystroke in its terminal, no
//! question, no « Keep ». Once the instance's idle timeout runs out, the
//! daemon asks « Still there? » in every tab of the Actor; « Keep » starts the
//! clock over; nobody answering within [`STILL_THERE_GRACE`], the daemon
//! closes the agent, with no tab open as well.

use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Serialize;
use ts_rs::TS;

/// How long « Still there? » waits for an answer before the agent closes.
pub const STILL_THERE_GRACE: Duration = Duration::from_secs(60);

/// The idleness of one agent: when its person last gave it input, and whether
/// « Still there? » is asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Idleness {
    /// Its person's last input; its opening, or the moment the daemon first
    /// saw it (an agent that outlived a daemon restart).
    pub last_input: DateTime<Utc>,
    /// When « Still there? » was asked, while nobody answered.
    pub asked_at: Option<DateTime<Utc>>,
}

/// What the clock of one agent asks for, at one moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdleStep {
    /// Nothing to do.
    Wait,
    /// Ask « Still there? » now: without an answer, the agent closes at
    /// `closes_at`.
    Ask { closes_at: DateTime<Utc> },
    /// Nobody answered: close the agent.
    Close,
}

impl Idleness {
    /// An agent given input at `at`.
    pub fn new(at: DateTime<Utc>) -> Self {
        Self {
            last_input: at,
            asked_at: None,
        }
    }

    /// Its person gave input at `at`: the clock starts over. `true` when this
    /// answered « Still there? ».
    pub fn input(&mut self, at: DateTime<Utc>) -> bool {
        self.last_input = self.last_input.max(at);
        self.asked_at.take().is_some()
    }

    /// When the agent closes, while « Still there? » is asked.
    pub fn closes_at(&self) -> Option<DateTime<Utc>> {
        self.asked_at.map(|asked| asked + grace())
    }

    /// How many seconds are left at `now` before it closes, while « Still
    /// there? » is asked; `0` once the moment passed.
    pub fn closes_in_secs(&self, now: DateTime<Utc>) -> Option<u32> {
        self.closes_at()
            .map(|at| u32::try_from((at - now).num_seconds().max(0)).unwrap_or(u32::MAX))
    }

    /// The step at `now`, `timeout` being the instance's idle timeout: ask
    /// once the timeout ran out since the last input, close once the question
    /// went unanswered for [`STILL_THERE_GRACE`]. Asking is recorded.
    pub fn step(&mut self, now: DateTime<Utc>, timeout: Duration) -> IdleStep {
        match self.asked_at {
            Some(asked) if now >= asked + grace() => IdleStep::Close,
            Some(_) => IdleStep::Wait,
            None if now >= self.last_input + chrono_duration(timeout) => {
                self.asked_at = Some(now);
                IdleStep::Ask {
                    closes_at: now + grace(),
                }
            }
            None => IdleStep::Wait,
        }
    }
}

fn grace() -> chrono::Duration {
    chrono_duration(STILL_THERE_GRACE)
}

fn chrono_duration(duration: Duration) -> chrono::Duration {
    chrono::Duration::from_std(duration).unwrap_or(chrono::Duration::MAX)
}

/// What changed of an Agent de support, as `/ws` tells every tab of its Actor
/// (`{"type": "support_agent_changed", "support_session": …, "support": …}`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
#[serde(tag = "change", rename_all = "snake_case")]
pub enum SupportChange {
    /// It was opened (or replaced).
    Opened,
    /// No input for `idle_timeout_secs`: « Still there? », and without an
    /// answer it closes at `closes_at` (RFC 3339), `closes_in_secs` from now
    /// (what a countdown reads, whatever the reader's clock says).
    Idle {
        closes_at: String,
        closes_in_secs: u32,
        idle_timeout_secs: u32,
    },
    /// Its person answered « Still there? » (Keep, or any input).
    Active,
    /// It wrote in its terminal, at `last_output_at` (RFC 3339): a new answer
    /// for a tab that does not show it.
    Answered { last_output_at: String },
    /// It ended: closed for idleness (`idle`), or by its person or its harness.
    Closed { idle: bool },
}

/// Whether `bytes` are only what a terminal sends on its own, never its
/// person: focus reports (`ESC [ I`, `ESC [ O`), cursor position reports
/// (`ESC [ <row> ; <col> R`) and device attributes (`ESC [ ? … c`,
/// `ESC [ > … c`). The harness asks for them; they are no input. Nothing at
/// all is no input either.
pub fn is_terminal_reply(bytes: &[u8]) -> bool {
    let mut rest = bytes;
    while !rest.is_empty() {
        let Some(body) = rest.strip_prefix(b"\x1b[") else {
            return false;
        };
        let Some(end) = body.iter().position(|b| b.is_ascii_alphabetic()) else {
            return false;
        };
        let (params, last) = (&body[..end], body[end]);
        let digits = |p: &[u8]| p.iter().all(|b| b.is_ascii_digit() || *b == b';');
        let reply = match last {
            b'I' | b'O' => params.is_empty(),
            b'R' => !params.is_empty() && digits(params),
            b'c' => matches!(params.first(), Some(b'?' | b'>')) && digits(&params[1..]),
            _ => false,
        };
        if !reply {
            return false;
        }
        rest = &body[end + 1..];
    }
    true
}

/// The `type` of the `/ws` message that carries a [`SupportChange`].
pub const SUPPORT_AGENT_CHANGED: &str = "support_agent_changed";

/// A moment as the records write it: RFC 3339, UTC, milliseconds.
pub fn rfc3339(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TIMEOUT: Duration = Duration::from_secs(1800);

    fn at(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000 + secs, 0).unwrap()
    }

    #[test]
    fn it_asks_once_the_timeout_ran_out_since_the_last_input() {
        let mut idle = Idleness::new(at(0));
        assert_eq!(idle.step(at(1799), TIMEOUT), IdleStep::Wait);
        assert_eq!(
            idle.step(at(1800), TIMEOUT),
            IdleStep::Ask {
                closes_at: at(1860)
            }
        );
        assert_eq!(idle.closes_at(), Some(at(1860)));
        assert_eq!(idle.closes_in_secs(at(1800)), Some(60));
        assert_eq!(idle.closes_in_secs(at(1845)), Some(15));
        assert_eq!(idle.closes_in_secs(at(1900)), Some(0));
        // Asked once: the next ticks wait for the answer.
        assert_eq!(idle.step(at(1830), TIMEOUT), IdleStep::Wait);
    }

    #[test]
    fn an_input_starts_the_clock_over() {
        let mut idle = Idleness::new(at(0));
        assert!(!idle.input(at(1000)));
        assert_eq!(idle.step(at(1800), TIMEOUT), IdleStep::Wait);
        assert!(matches!(idle.step(at(2800), TIMEOUT), IdleStep::Ask { .. }));
    }

    #[test]
    fn keep_answers_the_question_and_starts_the_clock_over() {
        let mut idle = Idleness::new(at(0));
        idle.step(at(1800), TIMEOUT);
        assert!(idle.input(at(1820)), "it answered « Still there? »");
        assert_eq!(idle.closes_at(), None);
        assert_eq!(idle.step(at(1900), TIMEOUT), IdleStep::Wait);
        assert!(matches!(idle.step(at(3620), TIMEOUT), IdleStep::Ask { .. }));
    }

    #[test]
    fn unanswered_it_closes_60_s_after_asking() {
        let mut idle = Idleness::new(at(0));
        idle.step(at(1800), TIMEOUT);
        assert_eq!(idle.step(at(1859), TIMEOUT), IdleStep::Wait);
        assert_eq!(idle.step(at(1860), TIMEOUT), IdleStep::Close);
    }

    #[test]
    fn an_input_from_the_past_never_moves_the_clock_back() {
        let mut idle = Idleness::new(at(100));
        idle.input(at(50));
        assert_eq!(idle.last_input, at(100));
    }

    #[test]
    fn what_the_terminal_sends_on_its_own_is_no_input() {
        for reply in [
            &b"\x1b[I"[..],
            b"\x1b[O",
            b"\x1b[12;40R",
            b"\x1b[?62;22c",
            b"\x1b[>0;276;0c",
            b"\x1b[O\x1b[I",
            b"",
        ] {
            assert!(is_terminal_reply(reply), "{reply:?}");
        }
        for input in [
            &b"a"[..],
            b"\r",
            b"\x1b[A",
            b"\x1b[<64;10;5M",
            b"\x1b[Ihello",
            b"\x1b",
        ] {
            assert!(!is_terminal_reply(input), "{input:?}");
        }
    }

    #[test]
    fn a_change_reads_as_its_kind_and_its_fields() {
        let idle = SupportChange::Idle {
            closes_at: rfc3339(at(60)),
            closes_in_secs: 60,
            idle_timeout_secs: 1800,
        };
        assert_eq!(
            serde_json::to_value(&idle).unwrap(),
            serde_json::json!({
                "change": "idle",
                "closes_at": "2027-01-15T08:01:00.000Z",
                "closes_in_secs": 60,
                "idle_timeout_secs": 1800,
            })
        );
        assert_eq!(
            serde_json::to_value(SupportChange::Closed { idle: true }).unwrap(),
            serde_json::json!({ "change": "closed", "idle": true })
        );
    }
}
