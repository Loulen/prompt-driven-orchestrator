//! `/metrics` (ADR-0090 §2): its own switch, off by default; on, the same
//! measures as the OTLP export, read in the Prometheus text format; the
//! Actor there only with « include the Actor ».

use std::sync::atomic::Ordering;

use pdo_telemetry::application::hub::TestExportError;
use pdo_telemetry::domain::attributes::metrics;

use crate::fakes::world;
use crate::runs::one_run;

/// The Prometheus name of an OpenTelemetry one.
fn prometheus_name(name: &str) -> String {
    name.replace('.', "_")
}

#[tokio::test]
async fn off_by_default_there_is_nothing_to_scrape() {
    let w = world();
    w.telemetry.reload().await;
    assert!(w.telemetry.scrape().is_none());
    assert!(!w.telemetry.status().metrics_endpoint);
}

#[tokio::test]
async fn on_alone_it_measures_without_exporting_anything() {
    let w = world();
    w.setting.set_prometheus(true);
    w.telemetry.reload().await;
    let status = w.telemetry.status();
    assert!(status.metrics_endpoint);
    assert!(!status.exporting, "the OTLP export is still off");
    assert_eq!(w.exporters.builds.load(Ordering::SeqCst), 0);
    assert!(matches!(
        w.telemetry
            .test_export(&pdo_kernel::Actor::LocalOperator)
            .await
            .unwrap_err(),
        TestExportError::NotExporting
    ));
    let text = w.telemetry.scrape().unwrap().unwrap();
    assert!(text.contains("target_info"), "{text}");
    assert!(text.contains("service_instance_id=\"host:5272\""), "{text}");

    w.setting.set_prometheus(false);
    w.telemetry.reload().await;
    assert!(w.telemetry.scrape().is_none());
    assert!(w.telemetry.active().is_none());
}

#[tokio::test]
async fn it_reads_the_same_measures_and_attributes_as_the_otlp_export() {
    let w = world();
    w.setting.set_prometheus(true);
    one_run(&w).await;
    let text = w.telemetry.scrape().unwrap().unwrap();
    let exported: std::collections::BTreeSet<String> = w
        .exporters
        .points()
        .into_iter()
        .map(|(name, _)| prometheus_name(&name))
        .collect();
    assert!(exported.contains("pdo_runs_started"));
    for name in &exported {
        assert!(text.contains(name.as_str()), "{name} missing from:\n{text}");
    }
    assert!(text.contains(&prometheus_name(metrics::NODE_DURATION)));
    for label in [
        "pdo_project=\"Core\"",
        "pdo_repository=\"https://git.example.org/org/app.git\"",
        "pdo_branch=\"main\"",
        "pdo_commit=\"abc123\"",
        "pdo_pipeline=\"implement\"",
        "pdo_model=\"model-x\"",
    ] {
        assert!(text.contains(label), "{label} missing from:\n{text}");
    }
    assert!(!text.contains("pdo_actor"), "the Actor is off by default");
    assert!(!text.contains("pdo_run_id"), "no Run id on a metric");
}

#[tokio::test]
async fn with_include_the_actor_the_scrape_names_who_launched() {
    let w = world();
    w.setting.set_prometheus(true);
    w.setting.set_include_actor(true);
    one_run(&w).await;
    let text = w.telemetry.scrape().unwrap().unwrap();
    assert!(text.contains("pdo_actor=\"test:alice\""), "{text}");
}

#[tokio::test]
async fn switching_it_never_restarts_the_otlp_export() {
    let w = world();
    w.setting.set(true, Some("http://127.0.0.1:4318"));
    w.telemetry.reload().await;
    let before = w.telemetry.active().unwrap();
    w.setting.set_prometheus(true);
    w.telemetry.reload().await;
    assert!(w.telemetry.scrape().is_some());
    w.setting.set_prometheus(false);
    w.telemetry.reload().await;
    assert!(w.telemetry.scrape().is_none());
    assert_eq!(w.exporters.builds.load(Ordering::SeqCst), 1);
    assert!(
        std::sync::Arc::ptr_eq(&before, &w.telemetry.active().unwrap()),
        "the same providers go on"
    );
    assert!(w.telemetry.status().exporting);
}
