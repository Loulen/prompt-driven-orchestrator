//! The Runs' measures (ADR-0090 §3): attached to the Projet, the repository
//! (its remote), git, the pipeline, the harness and the model; the Run and
//! node ids on the traces only; never the Actor.

use std::sync::Arc;

use opentelemetry::Value;
use pdo_run::domain::event::EventKind;
use pdo_telemetry::application::runs::RunRecorder;
use pdo_telemetry::domain::attributes::{keys, metrics};
use serde_json::json;

use crate::fakes::{event, value_of, world, Costs, Facts, World};

/// One Run: launched by `alice`, one node on `model-x` that waits 20 s on a
/// person out of 60 s, then the Run completes.
async fn one_run(w: &World) {
    w.setting.set(true, Some("http://127.0.0.1:4318"));
    w.telemetry.reload().await;
    let mut recorder = RunRecorder::new(w.telemetry.clone(), Arc::new(Facts), Arc::new(Costs));
    let actor = json!({"created_by": "alice", "triggered_by": "alice"});
    for e in [
        event(0, EventKind::RunStarted, None, actor.clone()),
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
        event(31, EventKind::NodeResumed, Some("build"), actor.clone()),
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

#[tokio::test]
async fn the_actor_is_never_exported_and_no_value_is_a_local_path() {
    let w = world();
    one_run(&w).await;
    let spans = w.exporters.spans.get_finished_spans().unwrap();
    let values = w
        .exporters
        .points()
        .into_iter()
        .flat_map(|(_, attributes)| attributes)
        .chain(spans.into_iter().flat_map(|s| s.attributes))
        .map(|kv| kv.value.to_string());
    for value in values {
        assert!(!value.contains("alice"), "the Actor leaked: {value}");
        assert!(!value.starts_with('/'), "a local path leaked: {value}");
    }
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
