//! The Runs' measures and traces, read off the event bus: Runs launched,
//! ended, failed; each node's durations; the declared waits; the cost and
//! tokens of an ended Run; a trace per Run with a span per node execution.
//! With « include the Actor », who launched the Run goes on its measures and
//! its spans, and who answered a node's wait on the node's span.
//! Nothing here touches a Run: a collector that does not answer, or an
//! export that is off, leaves the Runs exactly as they were.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;

use chrono::{DateTime, Utc};
use opentelemetry::trace::{SpanKind, Status, TraceContextExt as _, Tracer as _};
use opentelemetry::{Context, KeyValue};
use pdo_run::domain::event::Event;
use serde_json::Value;
use tokio::sync::broadcast::{self, error::RecvError};

use super::hub::{Active, Telemetry};
use super::instruments::key_values;
use super::ports::{RunCosts, RunFactsSource};
use crate::domain::attributes::{events, keys, RunFacts};
use crate::domain::lifecycle::{active_seconds, Execution, Lifecycle, Observation, Outcome};

struct RunTrack {
    facts: RunFacts,
    span: Option<Context>,
    nodes: HashMap<Execution, NodeTrack>,
}

struct NodeTrack {
    harness: Option<String>,
    model: Option<String>,
    span: Option<Context>,
}

/// Records the Runs' measures as their events go by.
pub struct RunRecorder {
    telemetry: Arc<Telemetry>,
    facts: Arc<dyn RunFactsSource>,
    costs: Arc<dyn RunCosts>,
    lifecycle: Lifecycle,
    runs: HashMap<String, RunTrack>,
}

impl RunRecorder {
    pub fn new(
        telemetry: Arc<Telemetry>,
        facts: Arc<dyn RunFactsSource>,
        costs: Arc<dyn RunCosts>,
    ) -> Self {
        Self {
            telemetry,
            facts,
            costs,
            lifecycle: Lifecycle::default(),
            runs: HashMap::new(),
        }
    }

    /// Read the bus until it closes. A lag loses measures, never a Run.
    pub async fn consume(mut self, mut bus: broadcast::Receiver<Value>) {
        loop {
            match bus.recv().await {
                Ok(message) => {
                    if let Some(event) = event_of(&message) {
                        self.on_event(&event).await;
                    }
                }
                Err(RecvError::Lagged(missed)) => {
                    tracing::debug!(missed, "telemetry: the bus outran the recorder");
                }
                Err(RecvError::Closed) => return,
            }
        }
    }

    pub async fn on_event(&mut self, event: &Event) {
        let Some(observation) = self.lifecycle.observe(event) else {
            return;
        };
        // Off, the lifecycle still follows the executions (an export switched
        // on mid-node measures the whole node), but nothing is resolved.
        if let Observation::RunForgotten { run_id } = &observation {
            self.runs.remove(run_id);
            return;
        }
        let Some(active) = self.telemetry.active() else {
            if let Observation::RunEnded { run_id, .. } = &observation {
                self.runs.remove(run_id);
            }
            return;
        };
        // Read at each measure: switching it takes effect on the next one.
        let actor = self.telemetry.include_actor();
        match observation {
            Observation::RunLaunched { run_id, at } => {
                let facts = self.facts.facts(&run_id).await;
                let attributes = key_values(&facts.attributes(actor));
                active.instruments.runs_started.add(1, &attributes);
                let span = start_span(&active, "pdo.run", at, &run_id, None, &attributes, None);
                self.runs.insert(
                    run_id,
                    RunTrack {
                        facts,
                        span: Some(span),
                        nodes: HashMap::new(),
                    },
                );
            }
            Observation::NodeStarted {
                execution,
                at,
                harness,
                model,
            } => {
                let run = self.track(&execution.run_id).await;
                let attributes = key_values(&run.facts.node_attributes(
                    harness.as_deref(),
                    model.as_deref(),
                    actor,
                ));
                let span = start_span(
                    &active,
                    "pdo.node",
                    at,
                    &execution.run_id,
                    Some(&execution),
                    &attributes,
                    run.span.as_ref(),
                );
                run.nodes.insert(
                    execution,
                    NodeTrack {
                        harness,
                        model,
                        span: Some(span),
                    },
                );
            }
            Observation::WaitDeclared { execution } => {
                let attributes = self.node_attributes(&execution, actor).await;
                active.instruments.waits_declared.add(1, &attributes);
            }
            Observation::WaitAnswered {
                execution,
                at,
                actor: answered_by,
            } => {
                let span = self
                    .runs
                    .get(&execution.run_id)
                    .and_then(|run| run.nodes.get(&execution))
                    .and_then(|node| node.span.as_ref());
                if let (Some(span), Some(answered_by)) = (span, answered_by.filter(|_| actor)) {
                    span.span().add_event_with_timestamp(
                        events::WAIT_ANSWERED,
                        SystemTime::from(at),
                        vec![KeyValue::new(keys::ACTOR, answered_by)],
                    );
                }
            }
            Observation::NodeEnded {
                execution,
                at,
                outcome,
                total,
                waiting,
            } => {
                let mut attributes = self.node_attributes(&execution, actor).await;
                attributes.push(KeyValue::new(keys::OUTCOME, outcome.as_str()));
                let instruments = &active.instruments;
                if let Some(total) = total {
                    instruments.node_duration.record(total, &attributes);
                    instruments
                        .node_active_duration
                        .record(active_seconds(total, waiting), &attributes);
                }
                instruments.node_wait_duration.record(waiting, &attributes);
                let node = self
                    .runs
                    .get_mut(&execution.run_id)
                    .and_then(|run| run.nodes.remove(&execution));
                if let Some(span) = node.and_then(|n| n.span) {
                    end_span(&span, at, outcome);
                }
            }
            Observation::RunForgotten { .. } => {}
            Observation::RunEnded {
                run_id,
                at,
                outcome,
            } => {
                let run = self.track(&run_id).await;
                let facts = run.facts.clone();
                let span = run.span.take();
                self.runs.remove(&run_id);
                let mut attributes = key_values(&facts.attributes(actor));
                if outcome == Outcome::Failed {
                    active.instruments.runs_failed.add(1, &attributes);
                }
                attributes.push(KeyValue::new(keys::OUTCOME, outcome.as_str()));
                active.instruments.runs_finished.add(1, &attributes);
                if let Some(span) = span {
                    end_span(&span, at, outcome);
                }
                // The cost is read off the transcripts on disk: never on the
                // bus's path.
                let costs = self.costs.clone();
                tokio::spawn(record_cost(active, costs, run_id, facts, actor));
            }
        }
    }

    /// The Run's track, resolved now when its launch went by before the export
    /// was on (or before the daemon started).
    async fn track(&mut self, run_id: &str) -> &mut RunTrack {
        if !self.runs.contains_key(run_id) {
            let facts = self.facts.facts(run_id).await;
            self.runs.insert(
                run_id.to_string(),
                RunTrack {
                    facts,
                    span: None,
                    nodes: HashMap::new(),
                },
            );
        }
        self.runs.get_mut(run_id).expect("inserted above")
    }

    async fn node_attributes(&mut self, execution: &Execution, actor: bool) -> Vec<KeyValue> {
        let run = self.track(&execution.run_id).await;
        let node = run.nodes.get(execution);
        key_values(&run.facts.node_attributes(
            node.and_then(|n| n.harness.as_deref()),
            node.and_then(|n| n.model.as_deref()),
            actor,
        ))
    }
}

async fn record_cost(
    active: Arc<Active>,
    costs: Arc<dyn RunCosts>,
    run_id: String,
    facts: RunFacts,
    actor: bool,
) {
    for slice in costs.slices(&run_id).await {
        let attributes =
            key_values(&facts.node_attributes(Some(&slice.harness), slice.model.as_deref(), actor));
        if let Some(usd) = slice.usd {
            active.instruments.cost.add(usd, &attributes);
        }
        for (kind, count) in slice.tokens.iter().flat_map(|t| t.by_type()) {
            let mut with_type = attributes.clone();
            with_type.push(KeyValue::new(keys::TOKEN_TYPE, kind));
            active.instruments.tokens.add(count, &with_type);
        }
    }
}

fn start_span(
    active: &Active,
    name: &'static str,
    at: DateTime<Utc>,
    run_id: &str,
    execution: Option<&Execution>,
    attributes: &[KeyValue],
    parent: Option<&Context>,
) -> Context {
    let mut all = attributes.to_vec();
    all.push(KeyValue::new(keys::RUN_ID, run_id.to_string()));
    if let Some(execution) = execution {
        all.push(KeyValue::new(keys::NODE_ID, execution.node_id.clone()));
        all.push(KeyValue::new(keys::ITERATION, execution.iter));
    }
    let builder = active
        .tracer
        .span_builder(name)
        .with_kind(SpanKind::Internal)
        .with_start_time(SystemTime::from(at))
        .with_attributes(all);
    let span = match parent {
        Some(parent) => builder.start_with_context(&active.tracer, parent),
        None => builder.start_with_context(&active.tracer, &Context::new()),
    };
    Context::new().with_span(span)
}

fn end_span(span: &Context, at: DateTime<Utc>, outcome: Outcome) {
    let span = span.span();
    span.set_attribute(KeyValue::new(keys::OUTCOME, outcome.as_str()));
    if outcome == Outcome::Failed {
        span.set_status(Status::error("failed"));
    }
    span.end_with_timestamp(SystemTime::from(at));
}

/// The Run event a bus message announces, if it is one.
fn event_of(message: &Value) -> Option<Event> {
    (message.get("type")?.as_str()? == "event")
        .then(|| serde_json::from_value(message.get("event")?.clone()).ok())
        .flatten()
}
