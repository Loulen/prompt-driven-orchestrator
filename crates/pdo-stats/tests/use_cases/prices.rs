//! The price sync and its boot refresh.

use pdo_kernel::{Actor, ResourceKind, Verb};
use pdo_stats::application::prices::sync::SyncError;

use crate::fakes::{Build, Host, PriceHome, Source};

const SOURCE: &str = r#"{"anthropic":{"models":{
    "claude-new-1": { "id": "claude-new-1", "cost": { "input": 3, "output": 4 } }
}}}"#;

#[tokio::test]
async fn a_refused_sync_never_reaches_the_source() {
    let world = Build {
        host: Host::refusing(),
        source: Source::answering(Ok(SOURCE)),
        ..Build::default()
    }
    .world();
    let refused = world
        .stats
        .sync_prices()
        .execute(&Actor::LocalOperator)
        .await;
    assert!(matches!(refused, Err(SyncError::Refused(_))), "{refused:?}");
    assert_eq!(
        world.source.hits.load(std::sync::atomic::Ordering::SeqCst),
        0
    );
    let asked = world.host.asked.lock().unwrap().clone();
    assert_eq!(asked[0].resource, ResourceKind::PriceTable);
    assert_eq!(asked[0].verb, Verb::Action("sync".into()));
}

#[tokio::test]
async fn a_sync_writes_the_fetched_tier_and_tells_every_client() {
    let world = Build {
        source: Source::answering(Ok(SOURCE)),
        ..Build::default()
    }
    .world();
    let report = world
        .stats
        .sync_prices()
        .execute(&Actor::LocalOperator)
        .await
        .unwrap();
    assert_eq!(report["ok"], true);
    assert_eq!(report["rows"], 1);
    assert_eq!(report["added"], serde_json::json!(["claude-new-1"]));
    assert_eq!(report["source"], "https://prices.test/api.json");
    assert_eq!(world.prices.written.lock().unwrap().len(), 1);
    assert_eq!(*world.announced.0.lock().unwrap(), [1]);
}

#[tokio::test]
async fn an_unusable_source_writes_nothing() {
    for body in [Err("connection refused"), Ok("{}")] {
        let world = Build {
            source: Source::answering(body),
            ..Build::default()
        }
        .world();
        let failed = world
            .stats
            .sync_prices()
            .execute(&Actor::LocalOperator)
            .await;
        assert!(matches!(failed, Err(SyncError::Source(_))), "{failed:?}");
        assert!(world.prices.written.lock().unwrap().is_empty());
        assert!(world.announced.0.lock().unwrap().is_empty());
    }
}

#[tokio::test]
async fn a_sync_without_a_home_is_a_hard_error() {
    let world = Build {
        prices: PriceHome::at(None, false),
        ..Build::default()
    }
    .world();
    let failed = world
        .stats
        .sync_prices()
        .execute(&Actor::LocalOperator)
        .await;
    assert!(matches!(failed, Err(SyncError::Failed(_))), "{failed:?}");
}

#[tokio::test]
async fn the_boot_refresh_never_seeds_and_asks_nothing() {
    // Armed, but the user never synced: no egress.
    let world = Build {
        host: Host::refusing(),
        source: Source::answering(Ok(SOURCE)),
        refresh_at_boot: true,
        ..Build::default()
    }
    .world();
    world.stats.refresh_prices().execute().await;
    assert_eq!(
        world.source.hits.load(std::sync::atomic::Ordering::SeqCst),
        0
    );

    // An existing tier with no readable vintage is refreshed, and a refusing
    // extension does not stop a transition of the daemon.
    let world = Build {
        host: Host::refusing(),
        prices: PriceHome::at(Some("/home"), true),
        source: Source::answering(Ok(SOURCE)),
        refresh_at_boot: true,
        ..Build::default()
    }
    .world();
    world.stats.refresh_prices().execute().await;
    assert_eq!(
        world.source.hits.load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    assert_eq!(world.prices.written.lock().unwrap().len(), 1);
    assert!(world.host.asked.lock().unwrap().is_empty());

    // Disarmed: nothing at all.
    let world = Build {
        prices: PriceHome::at(Some("/home"), true),
        source: Source::answering(Ok(SOURCE)),
        ..Build::default()
    }
    .world();
    world.stats.refresh_prices().execute().await;
    assert_eq!(
        world.source.hits.load(std::sync::atomic::Ordering::SeqCst),
        0
    );
}
