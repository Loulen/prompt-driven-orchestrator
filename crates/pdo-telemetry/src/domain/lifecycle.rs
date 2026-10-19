//! The Run lifecycle as the telemetry reads it off the event log, one event at
//! a time: a Run launched or ended, a node started, waiting on a person or
//! ended with its three durations (total, active, waiting). The declared wait
//! follows the rule of Stats › Performance (ADR-0069): it opens on a
//! `NodeAwaitingUser` of cause `declared` or `completion_not_released`, closes
//! on the first lift or end of the same execution, and a restart resets it. A
//! lift says who answered, when its event names them.

use std::collections::HashMap;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use pdo_run::domain::declared_wait::{self, awaiting_cause};
use pdo_run::domain::event::{Event, EventKind};
use serde_json::Value;

use super::attributes::actor_in;

/// What the `RunStarted` payload tells of a Run: the raw facts the attributes
/// are resolved from. The repository path is read to name the repository,
/// never sent; who launched the Run is sent only with « include the Actor ».
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunLaunch {
    pub pipeline: Option<String>,
    pub repository: Option<PathBuf>,
    pub source_branch: Option<String>,
    pub fork_sha: Option<String>,
    pub harness: Option<String>,
    /// Who launched the Run, labelled
    /// ([`actor_label`](super::attributes::actor_label)).
    pub created_by: Option<String>,
}

impl RunLaunch {
    pub fn from_payload(payload: Option<&Value>) -> Self {
        let text = |key: &str| {
            payload
                .and_then(|p| p.get(key))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        };
        let chosen_harness = payload
            .and_then(|p| p.get("agent_choice"))
            .and_then(|c| c.get("harness"))
            .and_then(Value::as_str)
            .map(str::to_string);
        Self {
            pipeline: text("pipeline_name"),
            repository: text("target_repo").map(PathBuf::from),
            source_branch: text("source_branch"),
            fork_sha: text("fork_sha"),
            harness: text("harness").or(chosen_harness),
            created_by: actor_in(payload, "created_by"),
        }
    }
}

/// How a Run or a node ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Completed,
    Failed,
    Skipped,
    Halted,
    Stopped,
    Interrupted,
    Stale,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Completed => "completed",
            Outcome::Failed => "failed",
            Outcome::Skipped => "skipped",
            Outcome::Halted => "halted",
            Outcome::Stopped => "stopped",
            Outcome::Interrupted => "interrupted",
            Outcome::Stale => "stale",
        }
    }
}

/// One execution of a node: its id and iteration.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Execution {
    pub run_id: String,
    pub node_id: String,
    pub iter: i64,
}

/// What one event means to the telemetry.
#[derive(Debug, Clone, PartialEq)]
pub enum Observation {
    RunLaunched {
        run_id: String,
        at: DateTime<Utc>,
    },
    NodeStarted {
        execution: Execution,
        at: DateTime<Utc>,
        harness: Option<String>,
        model: Option<String>,
    },
    WaitDeclared {
        execution: Execution,
    },
    /// The declared wait was lifted, by `actor` when the event names them.
    WaitAnswered {
        execution: Execution,
        at: DateTime<Utc>,
        actor: Option<String>,
    },
    NodeEnded {
        execution: Execution,
        at: DateTime<Utc>,
        outcome: Outcome,
        /// Wall-clock, in seconds; `None` when the start was not seen.
        total: Option<f64>,
        /// The declared wait, in seconds.
        waiting: f64,
    },
    RunEnded {
        run_id: String,
        at: DateTime<Utc>,
        outcome: Outcome,
    },
    /// The Run was archived: whatever is still followed of it is dropped, ended
    /// or not.
    RunForgotten {
        run_id: String,
    },
}

#[derive(Debug)]
struct OpenExecution {
    started_at: Option<DateTime<Utc>>,
    wait_opened_at: Option<DateTime<Utc>>,
    waited: f64,
}

/// The executions in flight, fed the events in log order.
#[derive(Debug, Default)]
pub struct Lifecycle {
    open: HashMap<Execution, OpenExecution>,
}

impl Lifecycle {
    pub fn observe(&mut self, event: &Event) -> Option<Observation> {
        let at = parse_ts(&event.ts)?;
        if let Some(outcome) = run_outcome(&event.kind) {
            self.open.retain(|e, _| e.run_id != event.run_id);
            return Some(Observation::RunEnded {
                run_id: event.run_id.clone(),
                at,
                outcome,
            });
        }
        if event.kind == EventKind::RunArchived {
            self.open.retain(|e, _| e.run_id != event.run_id);
            return Some(Observation::RunForgotten {
                run_id: event.run_id.clone(),
            });
        }
        if event.kind == EventKind::RunStarted {
            return Some(Observation::RunLaunched {
                run_id: event.run_id.clone(),
                at,
            });
        }
        let execution = Execution {
            run_id: event.run_id.clone(),
            node_id: event.node_id.clone()?,
            iter: event.iter.unwrap_or(1),
        };
        match &event.kind {
            EventKind::NodeStarted => {
                self.open.insert(
                    execution.clone(),
                    OpenExecution {
                        started_at: Some(at),
                        wait_opened_at: None,
                        waited: 0.0,
                    },
                );
                let text = |key: &str| {
                    event
                        .payload
                        .as_ref()
                        .and_then(|p| p.get(key))
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                };
                Some(Observation::NodeStarted {
                    execution,
                    at,
                    harness: text("harness"),
                    model: text("model"),
                })
            }
            EventKind::NodeAwaitingUser => {
                if !opens_declared_wait(event.payload.as_ref()) {
                    return None;
                }
                let open = self.open.entry(execution.clone()).or_insert(OpenExecution {
                    started_at: None,
                    wait_opened_at: None,
                    waited: 0.0,
                });
                // A refreshed question is the same wait, not a new one.
                if open.wait_opened_at.is_some() {
                    return None;
                }
                open.wait_opened_at = Some(at);
                Some(Observation::WaitDeclared { execution })
            }
            EventKind::NodeResumed | EventKind::NodeCompletionReleased => {
                // A lift with no wait open answered nothing.
                let open = self.open.get_mut(&execution)?;
                open.wait_opened_at?;
                close_wait(open, at);
                Some(Observation::WaitAnswered {
                    execution,
                    at,
                    actor: actor_in(event.payload.as_ref(), "actor"),
                })
            }
            kind => {
                let outcome = node_outcome(kind)?;
                let mut open = self.open.remove(&execution)?;
                close_wait(&mut open, at);
                Some(Observation::NodeEnded {
                    total: open.started_at.map(|s| seconds_between(s, at)),
                    waiting: open.waited,
                    execution,
                    at,
                    outcome,
                })
            }
        }
    }
}

fn close_wait(open: &mut OpenExecution, at: DateTime<Utc>) {
    if let Some(opened) = open.wait_opened_at.take() {
        open.waited += seconds_between(opened, at);
    }
}

/// The active share of a node's wall-clock: what is left once the declared
/// wait is taken out, never below zero.
pub fn active_seconds(total: f64, waiting: f64) -> f64 {
    (total - waiting).max(0.0)
}

fn seconds_between(from: DateTime<Utc>, to: DateTime<Utc>) -> f64 {
    ((to - from).num_milliseconds() as f64 / 1000.0).max(0.0)
}

fn parse_ts(ts: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(ts)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

/// Which causes open a declared wait is the rule of
/// [`pdo_run::domain::declared_wait`], shared with Stats › Performance and the
/// Run projection.
fn opens_declared_wait(payload: Option<&Value>) -> bool {
    awaiting_cause(payload).is_some_and(declared_wait::opens_declared_wait)
}

/// The events that end a Run for good. `RunInterrupted` does not: the Run can
/// still be resumed and end later.
fn run_outcome(kind: &EventKind) -> Option<Outcome> {
    Some(match kind {
        EventKind::RunCompleted => Outcome::Completed,
        EventKind::RunFailed => Outcome::Failed,
        EventKind::RunSkipped => Outcome::Skipped,
        EventKind::RunHalted => Outcome::Halted,
        _ => return None,
    })
}

fn node_outcome(kind: &EventKind) -> Option<Outcome> {
    Some(match kind {
        EventKind::NodeCompleted | EventKind::NodeAutoCompleted => Outcome::Completed,
        EventKind::NodeFailed => Outcome::Failed,
        EventKind::NodeStopped => Outcome::Stopped,
        EventKind::NodeInterrupted => Outcome::Interrupted,
        EventKind::NodeStale => Outcome::Stale,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    fn event(minute: u32, kind: EventKind, node: Option<&str>, payload: Value) -> Event {
        Event {
            id: None,
            run_id: "r1".into(),
            ts: format!("2033-03-10T10:{minute:02}:00Z"),
            kind,
            node_id: node.map(Into::into),
            iter: node.map(|_| 1),
            payload: Some(payload),
        }
    }

    #[test]
    fn a_node_ends_with_its_total_active_and_waiting_durations() {
        let mut life = Lifecycle::default();
        let observed: Vec<Observation> = [
            event(0, EventKind::RunStarted, None, json!({})),
            event(
                1,
                EventKind::NodeStarted,
                Some("a"),
                json!({"harness": "h", "model": "m"}),
            ),
            event(
                3,
                EventKind::NodeAwaitingUser,
                Some("a"),
                json!({"cause": "declared"}),
            ),
            event(
                4,
                EventKind::NodeAwaitingUser,
                Some("a"),
                json!({"cause": "declared"}),
            ),
            event(6, EventKind::NodeResumed, Some("a"), json!({})),
            event(11, EventKind::NodeCompleted, Some("a"), json!({})),
            event(12, EventKind::RunCompleted, None, json!({})),
        ]
        .iter()
        .filter_map(|e| life.observe(e))
        .collect();
        assert_eq!(observed.len(), 6, "{observed:#?}");
        assert!(matches!(
            &observed[3],
            Observation::WaitAnswered { actor: None, .. }
        ));
        let Observation::NodeEnded {
            total,
            waiting,
            outcome,
            ..
        } = &observed[4]
        else {
            panic!("{:?}", observed[4]);
        };
        assert_eq!(*total, Some(600.0));
        assert_eq!(*waiting, 180.0);
        assert_eq!(active_seconds(total.unwrap(), *waiting), 420.0);
        assert_eq!(*outcome, Outcome::Completed);
        assert!(matches!(
            observed[5],
            Observation::RunEnded {
                outcome: Outcome::Completed,
                ..
            }
        ));
    }

    #[test]
    fn an_archived_run_is_forgotten_with_its_executions() {
        let mut life = Lifecycle::default();
        life.observe(&event(1, EventKind::NodeStarted, Some("a"), json!({})));
        assert!(matches!(
            life.observe(&event(2, EventKind::RunArchived, None, json!({}))),
            Some(Observation::RunForgotten { .. })
        ));
        assert_eq!(
            life.observe(&event(3, EventKind::NodeCompleted, Some("a"), json!({}))),
            None,
            "nothing is left open"
        );
    }

    #[test]
    fn an_orchestrator_waiting_on_its_children_is_not_a_declared_wait() {
        let mut life = Lifecycle::default();
        life.observe(&event(1, EventKind::NodeStarted, Some("a"), json!({})));
        let waiting = event(
            2,
            EventKind::NodeAwaitingUser,
            Some("a"),
            json!({"cause": "children_pending"}),
        );
        assert_eq!(life.observe(&waiting), None);
    }

    #[test]
    fn the_launch_payload_names_the_facts() {
        let launch = RunLaunch::from_payload(Some(&json!({
            "pipeline_name": "implement",
            "target_repo": "/home/someone/app",
            "source_branch": "main",
            "fork_sha": "abc",
            "agent_choice": {"harness": "h2"},
            "created_by": {"kind": "identified", "namespace": "test", "id": "someone"},
        })));
        assert_eq!(
            launch,
            RunLaunch {
                pipeline: Some("implement".into()),
                repository: Some(PathBuf::from("/home/someone/app")),
                source_branch: Some("main".into()),
                fork_sha: Some("abc".into()),
                harness: Some("h2".into()),
                created_by: Some("test:someone".into()),
            }
        );
    }

    #[test]
    fn a_lift_says_who_answered_and_only_a_lift_of_an_open_wait_counts() {
        let mut life = Lifecycle::default();
        life.observe(&event(1, EventKind::NodeStarted, Some("a"), json!({})));
        assert_eq!(
            life.observe(&event(2, EventKind::NodeResumed, Some("a"), json!({}))),
            None,
            "no wait was open"
        );
        life.observe(&event(
            3,
            EventKind::NodeAwaitingUser,
            Some("a"),
            json!({"cause": "declared"}),
        ));
        let lifted = life.observe(&event(
            4,
            EventKind::NodeCompletionReleased,
            Some("a"),
            json!({"actor": {"kind": "identified", "namespace": "test", "id": "bob"}}),
        ));
        let Some(Observation::WaitAnswered { actor, .. }) = lifted else {
            panic!("{lifted:?}");
        };
        assert_eq!(actor.as_deref(), Some("test:bob"));
    }
}
