//! The Overview and Cost sections over the real event log and the Runs' files
//! (moved from the legacy crate's in-crate tests, #928).

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use crate::common::Harness;

#[tokio::test]
async fn stats_overview_sessions_use_run_cohorts_and_dynamic_harness_hierarchies() {
    let state = Harness::new().await;
    let insert = |run: &str,
                  ts: &str,
                  kind: &str,
                  node: Option<&str>,
                  iter: Option<i64>,
                  payload: Option<serde_json::Value>| {
        let db = state.db.clone();
        let run = run.to_string();
        let ts = ts.to_string();
        let kind = kind.to_string();
        let node = node.map(str::to_string);
        async move {
            sqlx::query(
                "INSERT INTO events (run_id, ts, kind, node_id, iter, payload) \
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(run)
            .bind(ts)
            .bind(kind)
            .bind(node)
            .bind(iter)
            .bind(payload.map(|value| value.to_string()))
            .execute(&db)
            .await
            .unwrap();
        }
    };

    insert(
        "before",
        "2026-07-14T09:00:00Z",
        "run_started",
        None,
        None,
        Some(serde_json::json!({
            "pipeline_id": "p-out",
            "pipeline_name": "Outside",
            "node_defs": [{"id":"worker","name":"Outside worker","node_type":"agent", "isolated_worktree":false}]
        })),
    )
    .await;
    insert(
        "before",
        "2026-07-15T09:00:00Z",
        "node_started",
        Some("worker"),
        Some(1),
        Some(
            serde_json::json!({"node_type":"agent", "isolated_worktree":false,"harness":"claude"}),
        ),
    )
    .await;

    insert(
        "r1",
        "2026-07-15T09:00:00Z",
        "run_started",
        None,
        None,
        Some(serde_json::json!({
            "pipeline_id": "p1",
            "pipeline_name": "Pipeline old",
            "target_repo": "/repos/product",
            "node_defs": [
                {"id":"worker","name":"Worker old","node_type":"agent", "isolated_worktree":false},
                {"id":"build","name":"Build","node_type":"script"}
            ]
        })),
    )
    .await;
    for (harness, node_type, session_id) in [
        (None, "agent", None),
        (Some("copilot"), "agent", Some("copilot-resurrected")),
        (Some("copilot"), "agent", Some("copilot-resurrected")),
        (Some("copilot"), "agent", Some("copilot-restart")),
        (None, "script", None),
    ] {
        insert(
            "r1",
            "2026-07-20T09:00:00Z",
            "node_started",
            Some(if node_type == "script" {
                "build"
            } else {
                "worker"
            }),
            Some(1),
            Some(serde_json::json!({
                "node_type":node_type,
                "harness":harness,
                "session_id":session_id
            })),
        )
        .await;
    }

    insert(
        "r2",
        "2026-07-16T09:00:00Z",
        "run_started",
        None,
        None,
        Some(serde_json::json!({
            "pipeline_id": "p1",
            "pipeline_name": "Pipeline latest",
            "target_repo": "/repos/product",
            "node_defs": [{"id":"worker","name":"Worker latest","node_type":"agent", "isolated_worktree":false}]
        })),
    )
    .await;
    for (iter, session_id) in [(1, "future-loop-1"), (2, "future-loop-2")] {
        insert(
            "r2",
            "2026-07-16T10:00:00Z",
            "node_started",
            Some("worker"),
            Some(iter),
            Some(serde_json::json!({
                "node_type":"agent", "isolated_worktree":false,
                "harness":"future-harness",
                "session_id":session_id
            })),
        )
        .await;
    }

    let response = state
        .router()
        .oneshot(
            Request::builder()
                .uri("/stats/overview?from=2026-07-15T00:00:00Z&to=2026-07-17T00:00:00Z&bucket=day")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();

    assert_eq!(
        body["session_harnesses"],
        serde_json::json!(["claude", "copilot", "future-harness"])
    );
    assert_eq!(
        body["sessions_by_period"],
        serde_json::json!([
            {
                "bucket":"2026-07-15",
                "harnesses":[
                    {"harness":"claude","executions":1},
                    {"harness":"copilot","executions":2}
                ]
            },
            {
                "bucket":"2026-07-16",
                "harnesses":[
                    {"harness":"future-harness","executions":2}
                ]
            }
        ])
    );
    let pipeline = &body["sessions_by_pipeline"][0];
    assert_eq!(pipeline["id"], "p1");
    assert_eq!(pipeline["name"], "Pipeline latest");
    let worker = pipeline["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["id"] == "worker")
        .unwrap();
    assert_eq!(worker["name"], "Worker latest");
    assert_eq!(worker["executions"], 5);
    assert_eq!(
        worker["harnesses"],
        serde_json::json!([
            {"harness":"claude","executions":1},
            {"harness":"copilot","executions":2},
            {"harness":"future-harness","executions":2}
        ])
    );
    assert!(
        pipeline["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|node| node["id"] != "build"),
        "script starts are excluded"
    );
    assert_eq!(pipeline["executions"], 5);
}

#[tokio::test]
async fn stats_cost_returns_harness_coverage_and_nested_contribution_hierarchies() {
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    let mut state = Harness::new().await;
    let home = state.scratch("test-stats-cost");
    let _cleanup = Cleanup(home.clone());
    std::fs::create_dir_all(&home).unwrap();
    state = state.with_home(home.clone());
    let repo = state.repo_root.to_string_lossy().into_owned();
    sqlx::query(
        "INSERT INTO projects (id, name, harness, created_at) VALUES \
         ('prj-product', 'Product', NULL, '2026-07-01T00:00:00Z')",
    )
    .execute(&state.db)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO project_members (path, project_id, created_at) VALUES \
         (?, 'prj-product', '2026-07-01T00:00:00Z')",
    )
    .bind(&repo)
    .execute(&state.db)
    .await
    .unwrap();

    async fn insert_event(
        db: &sqlx::SqlitePool,
        run: &str,
        ts: &str,
        kind: &str,
        node: Option<&str>,
        iter: Option<i64>,
        payload: serde_json::Value,
    ) {
        sqlx::query(
            "INSERT INTO events (run_id, ts, kind, node_id, iter, payload) \
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(run)
        .bind(ts)
        .bind(kind)
        .bind(node)
        .bind(iter)
        .bind(payload.to_string())
        .execute(db)
        .await
        .unwrap();
    }

    let run_ids = ["stats-cost-r1", "stats-cost-r2", "stats-cost-r3"];
    for (index, run_id) in run_ids.iter().enumerate() {
        insert_event(
            &state.db,
            run_id,
            &format!("2026-07-{}T09:00:00Z", 15 + index),
            "run_started",
            None,
            None,
            serde_json::json!({
                "pipeline_id":"p1",
                "pipeline_name":format!("Pipeline {}", index + 1),
                "target_repo":repo,
                "project_id":"prj-product",
                "harness":"claude",
                "node_defs":[{"id":"worker","name":format!("Worker {}", index + 1),"node_type":"agent", "isolated_worktree":true}]
            }),
        )
        .await;
    }
    insert_event(
        &state.db,
        run_ids[0],
        "2026-07-20T09:00:00Z",
        "node_started",
        Some("worker"),
        Some(1),
        serde_json::json!({"node_type":"agent", "isolated_worktree":true,"harness":"claude","session_id":"claude-1"}),
    )
    .await;
    insert_event(
        &state.db,
        run_ids[0],
        "2026-07-20T10:00:00Z",
        "node_started",
        Some("worker"),
        Some(1),
        serde_json::json!({"node_type":"agent", "isolated_worktree":true,"harness":"copilot","session_id":"copilot-1"}),
    )
    .await;
    insert_event(
        &state.db,
        run_ids[0],
        "2026-07-20T11:00:00Z",
        "node_started",
        Some("worker"),
        Some(2),
        serde_json::json!({"node_type":"agent", "isolated_worktree":true,"harness":"future-harness","session_id":"future-1"}),
    )
    .await;
    insert_event(
        &state.db,
        run_ids[1],
        "2026-07-21T09:00:00Z",
        "node_started",
        Some("worker"),
        Some(1),
        serde_json::json!({"node_type":"agent", "isolated_worktree":true,"harness":"future-harness","session_id":"future-2"}),
    )
    .await;
    insert_event(
        &state.db,
        run_ids[2],
        "2026-07-22T09:00:00Z",
        "node_started",
        Some("worker"),
        Some(1),
        serde_json::json!({"node_type":"agent", "isolated_worktree":false}),
    )
    .await;

    let claude_dir = home.join(".claude").join("projects").join(
        pdo_harness::claude_transcript::encode_working_dir(
            &pdo_workspace::domain::worktree_layout::sub_worktree_path(
                &state.repo_root,
                run_ids[0],
                "worker",
                1,
            ),
        ),
    );
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(
        claude_dir.join("claude-1.jsonl"),
        r#"{"type":"assistant","requestId":"r1","message":{"id":"m1","model":"claude-opus-4-8","usage":{"input_tokens":1000000,"output_tokens":0}}}"#,
    )
    .unwrap();
    let legacy_dir = home.join(".claude").join("projects").join(
        pdo_harness::claude_transcript::encode_working_dir(
            &pdo_workspace::domain::worktree_layout::worktree_dir_for_run(
                &state.repo_root,
                run_ids[2],
            ),
        ),
    );
    std::fs::create_dir_all(&legacy_dir).unwrap();
    std::fs::write(
        legacy_dir.join("legacy.jsonl"),
        r#"{"type":"assistant","requestId":"r3","message":{"id":"m3","model":"claude-opus-4-8","usage":{"input_tokens":1000000,"output_tokens":0}}}"#,
    )
    .unwrap();
    let copilot_dir = home
        .join(".copilot")
        .join("session-state")
        .join("copilot-1");
    std::fs::create_dir_all(&copilot_dir).unwrap();
    std::fs::write(
        copilot_dir.join("events.jsonl"),
        r#"{"type":"session.shutdown","data":{"totalNanoAiu":100000000000}}"#,
    )
    .unwrap();

    let response = state
        .router()
        .oneshot(
            Request::builder()
                .uri("/stats/cost?from=2026-07-15T00:00:00Z&to=2026-07-18T00:00:00Z&bucket=day")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();

    assert_eq!(
        body["harnesses"],
        serde_json::json!(["claude", "copilot", "future-harness"])
    );
    assert_eq!(body["total"]["usd"], 11.0);
    assert_eq!(body["total"]["executions"], 3);
    assert_eq!(body["total"]["readable"], 0);
    assert_eq!(body["total"]["unknown"], 3);
    assert!(
        body["total"]["unknown"].as_i64().unwrap() <= body["total"]["executions"].as_i64().unwrap()
    );
    let claude = body["total"]["harnesses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|metric| metric["harness"] == "claude")
        .unwrap();
    assert_eq!(claude["usd"], 10.0);
    // CORE-231: no Run here started a manager or a resolver, so r1 and r2 carry
    // no Infrastructure execution. r1's claude node is readable ($5); r3's
    // legacy node and its ambiguous leftover are not; r2 has no claude at all.
    // #811: `median_usd` travels beside `average_usd` on the same readable
    // population, the unknown Run entering neither.
    assert_eq!(claude["average_usd"], 5.0);
    assert_eq!(claude["median_usd"], 5.0);
    assert_eq!(claude["estimated"], true);
    assert_eq!(claude["executions"], 2);
    assert_eq!(claude["readable"], 1);
    assert_eq!(claude["unknown"], 1);
    let future = body["total"]["harnesses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|metric| metric["harness"] == "future-harness")
        .unwrap();
    assert!(future["usd"].is_null());
    assert_eq!(future["executions"], 2);
    assert_eq!(future["unknown"], 2);
    assert!(future["missing_reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|reason| reason == "harness has no cost source"));
    let copilot = body["total"]["harnesses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|metric| metric["harness"] == "copilot")
        .unwrap();
    assert_eq!(copilot["estimated"], false);
    assert_eq!(copilot["executions"], 1);
    assert_eq!(copilot["readable"], 1);
    assert_eq!(copilot["unknown"], 0);
    assert_eq!(copilot["average_usd"], 1.0);
    assert_eq!(copilot["median_usd"], 1.0);
    for metric in body["total"]["harnesses"].as_array().unwrap() {
        assert!(metric["unknown"].as_i64().unwrap() <= metric["executions"].as_i64().unwrap());
    }

    assert_eq!(body["by_period"][0]["bucket"], "2026-07-15");
    assert_eq!(body["by_period"][0]["usd"], 6.0);
    let pipeline = &body["by_pipeline"][0];
    assert_eq!(pipeline["id"], "p1");
    assert_eq!(pipeline["name"], "Pipeline 3");
    assert_eq!(pipeline["executions"], 3);
    assert_eq!(pipeline["readable"], 0);
    assert_eq!(pipeline["unknown"], 3);
    let worker = pipeline["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "worker")
        .unwrap();
    assert_eq!(worker["name"], "Worker 3");
    assert_eq!(worker["executions"], 5);
    assert_eq!(worker["readable"], 2);
    assert_eq!(worker["unknown"], 3);
    // #811, mixed readable/unreadable: the three unreadable executions enter
    // neither figure, and the two readable ones leave a median (the midpoint
    // of a pair, so it coincides with the average here — same population,
    // two readings).
    assert!(!worker["median_usd"].is_null(), "got: {worker:#?}");
    assert_eq!(worker["median_usd"], worker["average_usd"]);
    let infrastructure = pipeline["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == "Infrastructure")
        .unwrap();
    // Only r3's leftover transcript is an Infrastructure session (a manager
    // that predates `manager_started`); r1 and r2 started none.
    assert_eq!(infrastructure["executions"], 1);
    assert_eq!(infrastructure["unknown"], 1);
    let unassigned = pipeline["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == "Unassigned")
        .unwrap();
    assert_eq!(unassigned["usd"], 5.0);
    assert_eq!(unassigned["readable"], 0);
    assert_eq!(body["by_project"][0]["id"], "prj-product");
    assert_eq!(body["by_project"][0]["name"], "Product");
    assert_eq!(body["by_project"][0]["executions"], 3);
    assert_eq!(body["by_project"][0]["readable"], 0);
    assert_eq!(body["by_project"][0]["unknown"], 3);
    assert_eq!(body["by_project"][0]["pipelines"][0]["id"], "p1");
    assert!(!body["resolved"].as_array().unwrap().is_empty());
}

/// `GET /stats/overview` and `GET /stats/cost` (#810) — the SAME cohort
/// switch, so the three sections never describe different Runs. Overview
/// keeps its Errors series and shows **zero**, rather than hiding the card.
#[tokio::test]
async fn stats_overview_and_cost_share_the_completed_only_cohort() {
    let state = Harness::new().await;
    let repo = state.repo_root.to_string_lossy().into_owned();

    async fn insert_event(
        db: &sqlx::SqlitePool,
        run: &str,
        ts: &str,
        kind: &str,
        node: Option<&str>,
        iter: Option<i64>,
        payload: serde_json::Value,
    ) {
        sqlx::query(
            "INSERT INTO events (run_id, ts, kind, node_id, iter, payload) \
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(run)
        .bind(ts)
        .bind(kind)
        .bind(node)
        .bind(iter)
        .bind(payload.to_string())
        .execute(db)
        .await
        .unwrap();
    }

    for (run, pipeline, terminal) in [
        ("cohort-done", "cdone", "run_completed"),
        ("cohort-failed", "cfail", "run_failed"),
    ] {
        insert_event(
            &state.db,
            run,
            "2033-04-02T09:00:00Z",
            "run_started",
            None,
            None,
            serde_json::json!({
                "pipeline_id":pipeline,"pipeline_name":pipeline,"target_repo":repo,"harness":"claude",
                "node_defs":[{"id":"worker","name":"Worker","node_type":"agent","isolated_worktree":true}]
            }),
        )
        .await;
        insert_event(
            &state.db,
            run,
            "2033-04-02T09:05:00Z",
            "node_started",
            Some("worker"),
            Some(1),
            serde_json::json!({"node_type":"agent","isolated_worktree":true,"harness":"claude","session_id":run}),
        )
        .await;
        insert_event(
            &state.db,
            run,
            "2033-04-02T09:30:00Z",
            terminal,
            None,
            None,
            serde_json::json!({}),
        )
        .await;
    }

    async fn call(state: &Harness, uri: &str) -> serde_json::Value {
        let response = state
            .router()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{uri}");
        serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap()
    }

    fn total(series: &serde_json::Value) -> i64 {
        series
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["count"].as_i64().unwrap())
            .sum()
    }

    let window = "from=2033-04-02T00:00:00Z&to=2033-04-03T00:00:00Z&bucket=day";
    let whole = call(&state, &format!("/stats/overview?{window}")).await;
    assert_eq!(total(&whole["runs"]), 2);
    assert_eq!(total(&whole["errors"]), 1);
    assert_eq!(total(&whole["sessions"]), 2);

    let narrowed = call(
        &state,
        &format!("/stats/overview?{window}&completed_only=true"),
    )
    .await;
    assert_eq!(total(&narrowed["runs"]), 1);
    assert_eq!(
        total(&narrowed["errors"]),
        0,
        "zero errors, card kept — nothing is masked: {narrowed}"
    );
    assert_eq!(total(&narrowed["sessions"]), 1);
    assert_eq!(
        narrowed["sessions_by_pipeline"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["cdone"],
    );

    let cost = call(&state, &format!("/stats/cost?{window}")).await;
    assert_eq!(cost["by_pipeline"].as_array().unwrap().len(), 2);
    let narrowed_cost = call(&state, &format!("/stats/cost?{window}&completed_only=true")).await;
    assert_eq!(
        narrowed_cost["by_pipeline"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["cdone"],
    );
}

// --- The Overview's counts over the real event log (the legacy crate's
// `stats` unit tests over SQLite, #928) ---

use pdo_stats::application::read_overview::StatsRead;
use pdo_stats::domain::overview::{BucketCount, StatsOverview};

/// The Overview of `[FROM, to)`, by day, every Run, absorptions applied.
async fn overview(h: &Harness, to: &str) -> StatsOverview {
    h.stats
        .read_overview()
        .execute(&StatsRead {
            from: FROM.to_string(),
            to: to.to_string(),
            bucket: "day".to_string(),
            completed_only: false,
            uncombined: false,
        })
        .await
        .unwrap()
}

/// Insert a `run_started` + a terminal event for a run. `target_repo` is an
/// `Option` because a Run recorded before #470 legitimately has none, and the
/// "by project" bucket must still place it (ADR-0033).
async fn seed_run(
    db: &sqlx::SqlitePool,
    run_id: &str,
    pipeline_name: &str,
    target_repo: Option<&str>,
    day: &str,
    terminal: &str,
) {
    let mut payload_json = serde_json::json!({ "pipeline_name": pipeline_name });
    if let Some(repo) = target_repo {
        payload_json["target_repo"] = serde_json::json!(repo);
    }
    let payload = payload_json.to_string();
    sqlx::query("INSERT INTO events (run_id, ts, kind, payload) VALUES (?, ?, 'run_started', ?)")
        .bind(run_id)
        .bind(format!("{day}T09:00:00.000Z"))
        .bind(&payload)
        .execute(db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO events (run_id, ts, kind, payload) VALUES (?, ?, ?, NULL)")
        .bind(run_id)
        .bind(format!("{day}T09:05:00.000Z"))
        .bind(terminal)
        .execute(db)
        .await
        .unwrap();
}

async fn seed_session(db: &sqlx::SqlitePool, run_id: &str, day: &str) {
    sqlx::query(
        "INSERT INTO events (run_id, ts, kind, node_id, iter) VALUES (?, ?, 'node_started', 'doer', 0)",
    )
    .bind(run_id)
    .bind(format!("{day}T09:01:00.000Z"))
    .execute(db)
    .await
    .unwrap();
}

/// The FP-377 oracle fixture (6 runs across three days).
async fn seed_oracle(db: &sqlx::SqlitePool) {
    seed_run(
        db,
        "r1",
        "alpha",
        Some("/proj/A"),
        "2026-07-15",
        "run_completed",
    )
    .await;
    seed_run(
        db,
        "r2",
        "alpha",
        Some("/proj/A"),
        "2026-07-15",
        "run_failed",
    )
    .await;
    seed_run(
        db,
        "r3",
        "beta",
        Some("/proj/B"),
        "2026-07-16",
        "run_completed",
    )
    .await;
    seed_run(
        db,
        "r4",
        "beta",
        Some("/proj/B"),
        "2026-07-16",
        "run_completed",
    )
    .await;
    seed_run(
        db,
        "r5",
        "alpha",
        Some("/proj/A"),
        "2026-07-17",
        "run_skipped",
    )
    .await;
    seed_run(
        db,
        "r6",
        "beta",
        Some("/proj/B"),
        "2026-07-17",
        "run_failed",
    )
    .await;
    seed_session(db, "r1", "2026-07-15").await;
    seed_session(db, "r3", "2026-07-16").await;
    seed_session(db, "r4", "2026-07-16").await;
}

const FROM: &str = "2026-07-15T00:00:00Z";
const TO: &str = "2026-07-18T00:00:00Z";

#[tokio::test]
async fn overview_runs_errors_sessions_per_day() {
    let h = Harness::new().await;
    let db = h.db.clone();
    seed_oracle(&db).await;
    let ov = overview(&h, TO).await;

    assert_eq!(
        ov.runs,
        vec![
            BucketCount {
                bucket: "2026-07-15".into(),
                count: 2
            },
            BucketCount {
                bucket: "2026-07-16".into(),
                count: 2
            },
            BucketCount {
                bucket: "2026-07-17".into(),
                count: 2
            },
        ]
    );
    // Errors = run_failed only; run_skipped (r5) is NOT an error.
    assert_eq!(
        ov.errors,
        vec![
            BucketCount {
                bucket: "2026-07-15".into(),
                count: 1
            },
            BucketCount {
                bucket: "2026-07-17".into(),
                count: 1
            },
        ]
    );
    let total_errors: i64 = ov.errors.iter().map(|b| b.count).sum();
    assert_eq!(total_errors, 2, "run_skipped must not inflate errors");
    let total_sessions: i64 = ov.sessions.iter().map(|b| b.count).sum();
    assert_eq!(total_sessions, 3);
    assert_eq!(ov.buckets, vec!["2026-07-15", "2026-07-16", "2026-07-17"]);
}

#[tokio::test]
async fn overview_period_bounds_are_half_open() {
    let h = Harness::new().await;
    let db = h.db.clone();
    seed_oracle(&db).await;
    // A window that ends exactly at the 17th 00:00 excludes the 17th's runs.
    let ov = overview(&h, "2026-07-17T00:00:00Z").await;
    let total_runs: i64 = ov.runs.iter().map(|b| b.count).sum();
    assert_eq!(total_runs, 4, "half-open [from, to): the 17th is excluded");
}

#[tokio::test]
async fn overview_counts_a_legacy_null_target_run() {
    // Companion to the above at the SQL layer: a null-target Run is an
    // ordinary Run everywhere on the read side.
    let h = Harness::new().await;
    let db = h.db.clone();
    seed_run(&db, "legacy", "alpha", None, "2026-07-15", "run_completed").await;
    let ov = overview(&h, TO).await;
    assert_eq!(ov.runs.len(), 1);
    assert_eq!(ov.runs[0].count, 1);
}
