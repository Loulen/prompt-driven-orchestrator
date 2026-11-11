//! The three reads: they fold what the ports answer for the Runs and the
//! Triggers their reader may read, and the Performance memo spares a second
//! fold when nothing moved.

use pdo_run::application::ports::{NodeStart, RunStart};
use pdo_stats::application::performance::{PerformanceError, PerformanceRead};
use pdo_stats::application::read_overview::{ReadError, StatsRead};

use pdo_kernel::Actor;

use crate::fakes::{row, Build, Host, Store, Transcripts};

const LOCAL: Actor = Actor::LocalOperator;

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
        .execute(&LOCAL, &read("day"))
        .await
        .unwrap();
    assert_eq!(overview.buckets, ["2026-07-15", "2026-07-16", "2026-07-17"]);
    // `old` counts under `new`, the fires with it.
    assert_eq!(overview.sessions_by_pipeline[0].id, "new");
    assert_eq!(overview.fires_by_pipeline[0].pipeline_id, "new");
    assert_eq!(overview.fires_by_pipeline[0].count, 3);
    assert_eq!(overview.triggers_created_runs.fired, 3);
    // A read asks the read of each Run and each Trigger, never a mutation.
    let asked = world.host.asked.lock().unwrap().clone();
    assert!(!asked.is_empty());
    assert!(asked.iter().all(|op| op.is_read()), "{asked:?}");

    let raw = world
        .stats
        .read_overview()
        .execute(
            &LOCAL,
            &StatsRead {
                uncombined: true,
                ..read("day")
            },
        )
        .await
        .unwrap();
    assert_eq!(raw.sessions_by_pipeline[0].id, "old");
}

#[tokio::test]
async fn an_unknown_bucket_is_refused_before_any_read() {
    let world = Build::default().world();
    let refused = world.stats.read_cost().execute(&LOCAL, &read("year")).await;
    assert_eq!(
        refused.unwrap_err(),
        ReadError::InvalidBucket("year".into())
    );
    let refused = world.stats.read_overview().execute(&LOCAL, &read("")).await;
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
    let cost = world
        .stats
        .read_cost()
        .execute(&LOCAL, &read("day"))
        .await
        .unwrap();
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

fn child_start(run_id: &str, pipeline: &str, parent: &str, ts: &str) -> RunStart {
    RunStart {
        run_id: run_id.into(),
        ts: ts.into(),
        bucket: None,
        payload: serde_json::json!({
            "pipeline_id": pipeline, "pipeline_name": pipeline, "parent_run_id": parent,
        }),
    }
}

#[tokio::test]
async fn the_rolled_up_cost_recomposes_the_cohort_by_root() {
    let mut build = Build::default();
    // The window's cohort: a root, a child of it started in the window, and a
    // lone Run nobody orchestrated.
    build.events.run_starts = vec![
        run_start("root", "orchestrate"),
        RunStart {
            bucket: Some("2026-07-16".into()),
            ..child_start("child", "implement", "root", "2026-07-16T09:00:00Z")
        },
        run_start("lone", "implement"),
    ];
    // The log's child links: the grandchild started long after the window.
    build.events.child_run_starts = vec![
        child_start("child", "implement", "root", "2026-07-16T09:00:00Z"),
        child_start("grand", "review", "child", "2026-09-01T09:00:00Z"),
    ];
    let world = build.world();
    let read_cost = world.stats.read_cost();

    let plain = read_cost.execute(&LOCAL, &read("day")).await.unwrap();
    let rows: Vec<(&str, Option<u64>)> = plain
        .by_pipeline
        .iter()
        .map(|r| (r.id.as_str(), r.runs))
        .collect();
    assert_eq!(rows, [("implement", Some(2)), ("orchestrate", Some(1))]);
    assert!(plain.by_pipeline.iter().all(|r| r.roll_up.is_none()));

    let rolled = read_cost
        .execute_rolled_up(&LOCAL, &read("day"))
        .await
        .unwrap();
    let rows: Vec<(&str, Option<u64>)> = rolled
        .by_pipeline
        .iter()
        .map(|r| (r.id.as_str(), r.runs))
        .collect();
    // `implement` keeps only the lone Run; the child left with its tree.
    assert_eq!(rows, [("implement", Some(1)), ("orchestrate", Some(1))]);
    let roll_up = rolled.by_pipeline[1].roll_up.as_ref().unwrap();
    assert_eq!(roll_up.children.runs, Some(2));
    assert_eq!(roll_up.levels, 2);
    assert_eq!(roll_up.depths[1].pipelines[0].id, "review");
    // The whole tree lands in its root's period.
    let buckets: Vec<&str> = rolled.by_period.iter().map(|p| p.bucket.as_str()).collect();
    assert_eq!(buckets, ["2026-07-15"]);
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
    performance
        .execute(&LOCAL, &performance_read(false))
        .await
        .unwrap();
    performance
        .execute(&LOCAL, &performance_read(false))
        .await
        .unwrap();
    assert_eq!(
        performance.recomputes(from, to),
        1,
        "the second read hits the memo"
    );
    *world.files.footprint.lock().unwrap() = 7;
    performance
        .execute(&LOCAL, &performance_read(false))
        .await
        .unwrap();
    assert_eq!(
        performance.recomputes(from, to),
        2,
        "a moved footprint folds again"
    );
    performance
        .execute(&LOCAL, &performance_read(true))
        .await
        .unwrap();
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
        .execute(&LOCAL, &performance_read(false))
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

fn run_in(run_id: &str, pipeline: &str, project: &str) -> RunStart {
    RunStart {
        payload: serde_json::json!({
            "pipeline_id": pipeline, "pipeline_name": pipeline, "project_id": project,
        }),
        ..run_start(run_id, pipeline)
    }
}

fn node_start(run: &RunStart) -> NodeStart {
    NodeStart {
        bucket: "2026-07-15".into(),
        run_id: run.run_id.clone(),
        run_ts: run.ts.clone(),
        run_payload: run.payload.clone(),
        event_id: Some(2),
        node_id: Some("worker".into()),
        iter: Some(1),
        payload: serde_json::json!({ "harness": "claude" }),
    }
}

/// A Run in P1, another in P2, a Trigger launching in P1; `host` judges the
/// reads.
fn two_projects(host: std::sync::Arc<Host>) -> crate::fakes::World {
    let mut build = Build {
        host,
        ..Build::default()
    };
    let (r1, r2) = (run_in("r1", "alpha", "p1"), run_in("r2", "beta", "p2"));
    build
        .events
        .counts
        .insert("run_started".into(), vec![("2026-07-15".into(), 2)]);
    build.events.node_starts = vec![node_start(&r1), node_start(&r2)];
    build.events.run_starts = vec![r1, r2];
    build.world()
}

#[tokio::test]
async fn a_reader_of_no_run_counts_nothing() {
    let world = two_projects(Host::reading_only_in(""));
    let overview = world
        .stats
        .read_overview()
        .execute(&LOCAL, &read("day"))
        .await
        .unwrap();
    assert!(overview.runs.iter().all(|b| b.count == 0), "{overview:?}");
    assert!(overview.sessions_by_pipeline.is_empty());
    assert!(overview.fires_by_pipeline.is_empty());
    assert_eq!(overview.triggers_created_runs.fired, 0);
    assert_eq!(overview.triggers_created_runs.enabled_triggers, 0);

    let cost = world
        .stats
        .read_cost()
        .execute(&LOCAL, &read("day"))
        .await
        .unwrap();
    assert!(cost.by_pipeline.is_empty());
    assert!(cost.by_project.is_empty(), "no Projet's name shows");
    let performance = world
        .stats
        .read_performance()
        .execute(&LOCAL, &performance_read(false))
        .await
        .unwrap();
    assert_eq!(performance, world_without_runs().await);
}

/// The Performance of a world with no Run at all.
async fn world_without_runs() -> pdo_stats::domain::performance::StatsPerformance {
    Build::default()
        .world()
        .stats
        .read_performance()
        .execute(&LOCAL, &performance_read(false))
        .await
        .unwrap()
}

#[tokio::test]
async fn a_reader_of_one_project_counts_its_runs_only() {
    let world = two_projects(Host::reading_only_in("p1"));
    let overview = world
        .stats
        .read_overview()
        .execute(&LOCAL, &read("day"))
        .await
        .unwrap();
    let pipelines: Vec<&str> = overview
        .sessions_by_pipeline
        .iter()
        .map(|p| p.id.as_str())
        .collect();
    assert_eq!(pipelines, ["alpha"]);
    // The Trigger launches in P1: its fires count.
    assert_eq!(overview.triggers_created_runs.fired, 3);

    let cost = world
        .stats
        .read_cost()
        .execute(&LOCAL, &read("day"))
        .await
        .unwrap();
    let pipelines: Vec<&str> = cost.by_pipeline.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(pipelines, ["alpha"]);
    assert_eq!(cost.by_project.len(), 1, "the Projet of r1 only");

    // Asked as the list of the Runs asks: `run` × `read` in the Run's Projet.
    let asked = world.host.asked.lock().unwrap().clone();
    assert!(asked
        .iter()
        .any(|op| op.resource == pdo_kernel::ResourceKind::Run
            && op.target.as_deref() == Some("r2")
            && op.projects == ["p2".to_string()]));
}
