//! The three reads: they ask nothing, fold what the ports answer, and the
//! Performance memo spares a second fold when nothing moved.

use pdo_run::application::ports::{NodeStart, RunStart};
use pdo_stats::application::performance::{PerformanceError, PerformanceRead};
use pdo_stats::application::read_overview::{ReadError, StatsRead};

use crate::fakes::{row, Build, Host, Store, Transcripts};

fn read(bucket: &str) -> StatsRead {
    StatsRead {
        from: "2026-07-15T00:00:00Z".into(),
        to: "2026-07-18T00:00:00Z".into(),
        bucket: bucket.into(),
        completed_only: false,
        uncombined: false,
    }
}

fn run_start(run_id: &str, pipeline: &str) -> RunStart {
    RunStart {
        run_id: run_id.into(),
        ts: "2026-07-15T09:00:00Z".into(),
        bucket: Some("2026-07-15".into()),
        payload: serde_json::json!({ "pipeline_id": pipeline, "pipeline_name": pipeline }),
    }
}

#[tokio::test]
async fn the_overview_folds_the_counts_the_fires_and_the_sessions_under_the_absorptions() {
    let mut build = Build {
        host: Host::refusing(),
        store: Store::holding(vec![row("pipeline", "", "old", "new")]),
        ..Build::default()
    };
    build.events.counts.insert(
        "run_started".into(),
        vec![("2026-07-15".into(), 2), ("2026-07-16".into(), 1)],
    );
    build
        .events
        .counts
        .insert("node_started".into(), vec![("2026-07-17".into(), 1)]);
    build.events.node_starts = vec![NodeStart {
        bucket: "2026-07-15".into(),
        run_id: "r1".into(),
        run_ts: "2026-07-15T09:00:00Z".into(),
        run_payload: serde_json::json!({ "pipeline_id": "old", "pipeline_name": "Old" }),
        event_id: Some(2),
        node_id: Some("worker".into()),
        iter: Some(1),
        payload: serde_json::json!({ "harness": "claude" }),
    }];
    let world = build.world();
    let overview = world
        .stats
        .read_overview()
        .execute(&read("day"))
        .await
        .unwrap();
    assert_eq!(overview.buckets, ["2026-07-15", "2026-07-16", "2026-07-17"]);
    // `old` counts under `new`, the fires with it.
    assert_eq!(overview.sessions_by_pipeline[0].id, "new");
    assert_eq!(overview.fires_by_pipeline[0].pipeline_id, "new");
    assert_eq!(overview.fires_by_pipeline[0].count, 3);
    assert_eq!(overview.triggers_created_runs.fired, 3);
    // A read asks nothing, even of an extension that refuses everything.
    assert!(world.host.asked.lock().unwrap().is_empty());

    let raw = world
        .stats
        .read_overview()
        .execute(&StatsRead {
            uncombined: true,
            ..read("day")
        })
        .await
        .unwrap();
    assert_eq!(raw.sessions_by_pipeline[0].id, "old");
}

#[tokio::test]
async fn an_unknown_bucket_is_refused_before_any_read() {
    let world = Build::default().world();
    let refused = world.stats.read_cost().execute(&read("year")).await;
    assert_eq!(
        refused.unwrap_err(),
        ReadError::InvalidBucket("year".into())
    );
    let refused = world.stats.read_overview().execute(&read("")).await;
    assert_eq!(
        refused.unwrap_err(),
        ReadError::InvalidBucket(String::new())
    );
}

#[tokio::test]
async fn the_cost_folds_every_run_of_the_cohort_with_the_price_table() {
    let mut build = Build::default();
    build.events.run_starts = vec![run_start("r1", "p"), run_start("r2", "q")];
    let world = build.world();
    let cost = world.stats.read_cost().execute(&read("day")).await.unwrap();
    // A Run with no readable cost still counts in its Pipeline's row.
    let rows: Vec<(&str, Option<u64>)> = cost
        .by_pipeline
        .iter()
        .map(|row| (row.id.as_str(), row.runs))
        .collect();
    assert_eq!(rows, [("p", Some(1)), ("q", Some(1))]);
    assert!(
        !cost.resolved.is_empty(),
        "the price table in force rides along"
    );
    assert_eq!(
        world.events.reads.load(std::sync::atomic::Ordering::SeqCst),
        2
    );
}

fn performance_read(refresh: bool) -> PerformanceRead {
    PerformanceRead {
        from: "2026-07-15T00:00:00Z".into(),
        to: "2026-07-18T00:00:00Z".into(),
        refresh,
        completed_only: false,
        uncombined: false,
    }
}

#[tokio::test]
async fn the_performance_memo_folds_again_only_when_a_footprint_moves() {
    let mut build = Build::default();
    build.events.run_starts = vec![run_start("r1", "p")];
    let world = build.world();
    let performance = world.stats.read_performance();
    let (from, to) = ("2026-07-15T00:00:00Z", "2026-07-18T00:00:00Z");
    performance.execute(&performance_read(false)).await.unwrap();
    performance.execute(&performance_read(false)).await.unwrap();
    assert_eq!(
        performance.recomputes(from, to),
        1,
        "the second read hits the memo"
    );
    *world.files.footprint.lock().unwrap() = 7;
    performance.execute(&performance_read(false)).await.unwrap();
    assert_eq!(
        performance.recomputes(from, to),
        2,
        "a moved footprint folds again"
    );
    performance.execute(&performance_read(true)).await.unwrap();
    assert_eq!(
        performance.recomputes(from, to),
        3,
        "a refresh always folds"
    );
}

#[tokio::test]
async fn an_unreadable_transcript_source_fails_the_whole_read() {
    let mut build = Build {
        transcripts: Transcripts { unreadable: true },
        ..Build::default()
    };
    build.events.run_starts = vec![run_start("r1", "p")];
    build.events.events.insert(
        "r1".into(),
        vec![
            event(
                "r1",
                "2026-07-15T09:00:00Z",
                pdo_run::domain::event::EventKind::NodeStarted,
            ),
            event(
                "r1",
                "2026-07-15T09:10:00Z",
                pdo_run::domain::event::EventKind::NodeCompleted,
            ),
        ],
    );
    let world = build.world();
    let failed = world
        .stats
        .read_performance()
        .execute(&performance_read(false))
        .await;
    match failed {
        Err(PerformanceError::SourceUnreadable(message)) => {
            assert!(message.contains("permission denied"), "{message}")
        }
        other => panic!("{other:?}"),
    }
}

fn event(
    run_id: &str,
    ts: &str,
    kind: pdo_run::domain::event::EventKind,
) -> pdo_run::domain::event::Event {
    pdo_run::domain::event::Event {
        id: None,
        run_id: run_id.into(),
        ts: ts.into(),
        kind,
        node_id: Some("worker".into()),
        iter: Some(1),
        payload: Some(
            serde_json::json!({ "node_type": "agent", "harness": "claude", "session_id": "s1" }),
        ),
    }
}
