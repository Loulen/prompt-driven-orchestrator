//! The switch: off by default, nothing is built, so nothing can connect; on
//! and off again, and a new collector, without a restart.

use std::sync::atomic::Ordering;

use pdo_run::domain::event::EventKind;
use pdo_telemetry::application::hub::TestExportError;
use pdo_telemetry::application::runs::RunRecorder;
use serde_json::json;

use crate::fakes::{event, world, Costs, Facts};

#[tokio::test]
async fn off_by_default_nothing_is_built_and_the_runs_are_not_measured() {
    let w = world();
    w.telemetry.reload().await;
    assert!(w.telemetry.active().is_none());
    let mut recorder = RunRecorder::new(
        w.telemetry.clone(),
        std::sync::Arc::new(Facts),
        std::sync::Arc::new(Costs),
    );
    recorder
        .on_event(&event(0, EventKind::RunStarted, None, json!({})))
        .await;
    assert_eq!(w.exporters.builds.load(Ordering::SeqCst), 0);
    assert!(!w.telemetry.status().exporting);
    assert!(matches!(
        w.telemetry
            .test_export(&pdo_kernel::Actor::LocalOperator)
            .await
            .unwrap_err(),
        TestExportError::NotExporting
    ));
}

#[tokio::test]
async fn a_switch_on_without_a_collector_builds_nothing() {
    let w = world();
    w.setting.set(true, None);
    w.telemetry.reload().await;
    assert!(w.telemetry.active().is_none());
    assert_eq!(w.exporters.builds.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn the_export_follows_the_setting_without_a_restart() {
    let w = world();
    w.setting.set(true, Some("http://127.0.0.1:4318"));
    w.telemetry.reload().await;
    assert!(w.telemetry.active().is_some());
    assert!(w.telemetry.status().exporting);

    // The same setting again: nothing rebuilt.
    w.telemetry.reload().await;
    assert_eq!(w.exporters.builds.load(Ordering::SeqCst), 1);

    // A new collector: rebuilt toward it.
    w.setting
        .set(true, Some("http://collector.example.org:4318"));
    w.telemetry.reload().await;
    assert_eq!(w.exporters.builds.load(Ordering::SeqCst), 2);
    assert_eq!(
        w.exporters.targets.lock().unwrap()[1].endpoint,
        "http://collector.example.org:4318"
    );

    // Off: nothing exports any more.
    w.setting
        .set(false, Some("http://collector.example.org:4318"));
    w.telemetry.reload().await;
    assert!(w.telemetry.active().is_none());
    assert!(!w.telemetry.status().exporting);
}

#[tokio::test]
async fn a_test_export_sends_a_span_even_with_no_measure_yet() {
    let w = world();
    w.setting.set(true, Some("http://127.0.0.1:4318"));
    w.telemetry.reload().await;
    let status = w
        .telemetry
        .test_export(&pdo_kernel::Actor::LocalOperator)
        .await
        .unwrap();
    assert!(status.exporting);
    let spans = w.exporters.spans.get_finished_spans().unwrap();
    assert!(spans.iter().any(|s| s.name == "pdo.telemetry.test"));
}
