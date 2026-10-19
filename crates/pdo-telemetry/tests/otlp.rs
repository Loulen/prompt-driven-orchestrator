//! The OTLP exporter against a collector that does not answer: the status
//! says so, and the measures go on.

use std::sync::Arc;
use std::time::Duration;

use pdo_telemetry::adapters::otlp::OtlpExporters;
use pdo_telemetry::{Identity, Telemetry};

use crate::fakes::Setting;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unreachable_collector_is_reported_in_the_status() {
    // A port nothing listens on: bound, then released.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let setting = Arc::new(Setting::default());
    setting.set(true, Some(&format!("http://127.0.0.1:{port}")));
    let telemetry = Telemetry::new(
        setting,
        Arc::new(OtlpExporters::current()),
        Identity {
            instance: "host:1".into(),
            version: "0.0.0".into(),
        },
        Duration::from_secs(3600),
    );
    telemetry.reload().await;
    let status = tokio::time::timeout(
        Duration::from_secs(60),
        telemetry.test_export(&pdo_kernel::Actor::LocalOperator),
    )
    .await
    .expect("a dead collector never hangs the test export")
    .unwrap();
    let last = status.last_export.expect("the failed export is reported");
    assert!(!last.ok);
    assert!(!last.detail.is_empty());
    telemetry.shutdown().await;
}
