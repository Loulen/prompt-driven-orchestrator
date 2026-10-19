//! The Runs' measures (ADR-0090 §3-4): attached to the Projet, the repository
//! (its remote), git, the pipeline, the harness and the model; the Run and
//! node ids on the traces only; the Actor only with « include the Actor ».

use std::sync::Arc;

use opentelemetry::Value;
use pdo_run::domain::event::EventKind;
use pdo_telemetry::application::runs::RunRecorder;
use pdo_telemetry::domain::attributes::{events, keys, metrics};
use serde_json::json;

use crate::fakes::{event, value_of, world, Costs, Facts, World};

/// One Run: launched by `alice`, one node on `model-x` that waits 20 s on a
/// person out of 60 s, answered by `bob`, then the Run completes.
pub(crate) async fn one_run(w: &World) {
    w.setting.set(true, Some("http://127.0.0.1:4318"));
    w.telemetry.reload().await;
    let mut recorder = RunRecorder::new(w.telemetry.clone(), Arc::new(Facts), Arc::new(Costs));
    let alice = json!({"kind": "identified", "namespace": "test", "id": "alice"});
    let bob = json!({"kind": "identified", "namespace": "test", "id": "bob"});
    for e in [
        event(0, EventKind::RunStarted, None, json!({"created_by": alice})),
        event(
            1,
            EventKind::NodeStarted,
            Some("build"),
            json!({"harness": "node-harness", "model": "model-x", "started_by": "alice"}),
        ),
        event(
            11,
            EventKind::NodeAwaitingUser,
            Some("build"),
            json!({"cause": "declared", "answered_by": "alice"}),
        ),
        event(
            31,
            EventKind::NodeResumed,
            Some("build"),
            json!({"source": "pty_enter", "actor": bob}),
        ),
        event(61, EventKind::NodeCompleted, Some("build"), json!({})),
        event(62, EventKind::RunCompleted, None, json!({})),
    ] {
        recorder.on_event(&e).await;
    }
    // The cost is recorded off the bus's path.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    w.telemetry
        .test_export(&pdo_kernel::Actor::LocalOperator)
        .await
        .unwrap();
}

#[tokio::test]
async fn a_run_and_its_node_are_measured_with_the_projet_the_repository_and_git() {
    let w = world();
    one_run(&w).await;
    let points = w.exporters.points();
    let names: Vec<&str> = points.iter().map(|(n, _)| n.as_str()).collect();
    for expected in [
        metrics::RUNS_STARTED,
        metrics::RUNS_FINISHED,
        metrics::NODE_DURATION,
        metrics::NODE_ACTIVE_DURATION,
        metrics::NODE_WAIT_DURATION,
        metrics::WAITS_DECLARED,
        metrics::COST,
        metrics::TOKENS,
    ] {
        assert!(
            names.contains(&expected),
            "{expected} missing from {names:?}"
        );
    }
    let (_, started) = points
        .iter()
        .find(|(n, _)| n == metrics::RUNS_STARTED)
        .unwrap();
    for (key, want) in [
        (keys::PROJECT, "Core"),
        (keys::REPOSITORY, "https://git.example.org/org/app.git"),
        (keys::BRANCH, "main"),
        (keys::COMMIT, "abc123"),
        (keys::PIPELINE, "implement"),
        (keys::HARNESS, "run-harness"),
    ] {
        assert_eq!(value_of(started, key), Some(&Value::from(want)), "{key}");
    }
    let (_, node) = points
        .iter()
        .find(|(n, _)| n == metrics::NODE_DURATION)
        .unwrap();
    assert_eq!(value_of(node, keys::MODEL), Some(&Value::from("model-x")));
    assert_eq!(
        value_of(node, keys::HARNESS),
        Some(&Value::from("node-harness"))
    );
    let resource = &w.exporters.resources()[0];
    assert_eq!(
        resource.get(&opentelemetry::Key::new(keys::INSTANCE)),
        Some(Value::from("host:5272"))
    );
}

#[tokio::test]
async fn no_metric_carries_a_run_or_node_id_and_the_traces_do() {
    let w = world();
    one_run(&w).await;
    for (name, attributes) in w.exporters.points() {
        for key in [keys::RUN_ID, keys::NODE_ID] {
            assert!(
                value_of(&attributes, key).is_none(),
                "{name} carries {key}: unbounded cardinality on a metric"
            );
        }
    }
    let spans = w.exporters.spans.get_finished_spans().unwrap();
    let run = spans
        .iter()
        .find(|s| s.name == "pdo.run")
        .expect("run span");
    let node = spans
        .iter()
        .find(|s| s.name == "pdo.node")
        .expect("node span");
    assert_eq!(
        value_of(&run.attributes, keys::RUN_ID),
        Some(&Value::from("run-1"))
    );
    assert_eq!(
        value_of(&node.attributes, keys::NODE_ID),
        Some(&Value::from("build"))
    );
    assert_eq!(
        node.parent_span_id,
        run.span_context.span_id(),
        "the node's span is a child of the Run's"
    );
}

/// Every attribute value sent: on the metrics, the spans and their events.
fn sent_values(w: &World) -> Vec<String> {
    let spans = w.exporters.spans.get_finished_spans().unwrap();
    w.exporters
        .points()
        .into_iter()
        .flat_map(|(_, attributes)| attributes)
        .chain(spans.into_iter().flat_map(|s| {
            let events: Vec<_> = s.events.iter().flat_map(|e| e.attributes.clone()).collect();
            s.attributes.into_iter().chain(events)
        }))
        .map(|kv| kv.value.to_string())
        .collect()
}

#[tokio::test]
async fn by_default_the_actor_is_never_exported_and_no_value_is_a_local_path() {
    let w = world();
    one_run(&w).await;
    for value in sent_values(&w) {
        assert!(
            !value.contains("alice") && !value.contains("bob"),
            "the Actor leaked: {value}"
        );
        assert!(!value.starts_with('/'), "a local path leaked: {value}");
    }
}

#[tokio::test]
async fn with_include_the_actor_who_launched_and_who_answered_are_sent() {
    let w = world();
    w.setting.set_include_actor(true);
    one_run(&w).await;
    assert!(w.telemetry.status().include_actor);
    let points = w.exporters.points();
    for metric in [
        metrics::RUNS_STARTED,
        metrics::RUNS_FINISHED,
        metrics::NODE_DURATION,
        metrics::COST,
    ] {
        let (_, attributes) = points.iter().find(|(n, _)| n == metric).unwrap();
        assert_eq!(
            value_of(attributes, keys::ACTOR),
            Some(&Value::from("test:alice")),
            "{metric}"
        );
    }
    let spans = w.exporters.spans.get_finished_spans().unwrap();
    let node = spans.iter().find(|s| s.name == "pdo.node").unwrap();
    assert_eq!(
        value_of(&node.attributes, keys::ACTOR),
        Some(&Value::from("test:alice"))
    );
    let answered = node
        .events
        .iter()
        .find(|e| e.name == events::WAIT_ANSWERED)
        .expect("the answer is on the node's span");
    assert_eq!(
        value_of(&answered.attributes, keys::ACTOR),
        Some(&Value::from("test:bob"))
    );
}

#[tokio::test]
async fn switching_include_the_actor_takes_effect_on_the_next_measure() {
    let w = world();
    w.setting.set(true, Some("http://127.0.0.1:4318"));
    w.setting.set_include_actor(true);
    w.telemetry.reload().await;
    let mut recorder = RunRecorder::new(w.telemetry.clone(), Arc::new(Facts), Arc::new(Costs));
    recorder
        .on_event(&event(0, EventKind::RunStarted, None, json!({})))
        .await;
    w.setting.set_include_actor(false);
    w.telemetry.reload().await;
    recorder
        .on_event(&event(9, EventKind::RunCompleted, None, json!({})))
        .await;
    w.telemetry
        .test_export(&pdo_kernel::Actor::LocalOperator)
        .await
        .unwrap();
    let points = w.exporters.points();
    let actor_of = |metric: &str| {
        let (_, attributes) = points.iter().find(|(n, _)| n == metric).unwrap();
        value_of(attributes, keys::ACTOR).cloned()
    };
    assert_eq!(
        actor_of(metrics::RUNS_STARTED),
        Some(Value::from("test:alice"))
    );
    assert_eq!(actor_of(metrics::RUNS_FINISHED), None);
    assert_eq!(
        w.exporters.builds.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "nothing is rebuilt"
    );
}

#[tokio::test]
async fn the_node_durations_split_the_declared_wait_out() {
    use opentelemetry_sdk::metrics::data::{AggregatedMetrics, MetricData};
    let w = world();
    one_run(&w).await;
    let mut sums = std::collections::HashMap::new();
    for resource in w.exporters.metrics.get_finished_metrics().unwrap() {
        for scope in resource.scope_metrics() {
            for metric in scope.metrics() {
                if let AggregatedMetrics::F64(MetricData::Histogram(h)) = metric.data() {
                    let sum: f64 = h.data_points().map(|p| p.sum()).sum();
                    sums.insert(metric.name().to_string(), sum);
                }
            }
        }
    }
    assert_eq!(sums[metrics::NODE_DURATION], 60.0);
    assert_eq!(sums[metrics::NODE_WAIT_DURATION], 20.0);
    assert_eq!(sums[metrics::NODE_ACTIVE_DURATION], 40.0);
}
