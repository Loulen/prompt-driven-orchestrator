//! The Performance section over the real event log and transcripts (moved
//! from the legacy crate's in-crate tests, #928).

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use crate::common::Harness;

/// `GET /stats/performance` — the whole shape in one cohort: success-only
/// filtering (a loop lap, a stopped-then-restarted attempt, a script node, a
/// failed Run, a failed merge-resolver attempt all excluded/retained per the
/// issue's rules), stable Pipeline/Node identity with "latest name wins",
/// Pipeline aggregation pooling from raw main sessions (not from the Nodes'
/// own means), the whole-cohort `total`, Claude subagent grouping (declared
/// name + the "Unidentified subagent" fallback for an opaque id), a harness
/// with no context-usage source still keeping its Duration, and both
/// Infrastructure roles (Pipeline Manager, Merge resolver).
#[tokio::test]
async fn stats_performance_returns_pipeline_node_subagent_and_infrastructure_metrics() {
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    let mut state = Harness::new().await;
    let home = state.scratch("test-stats-performance");
    let _cleanup = Cleanup(home.clone());
    std::fs::create_dir_all(&home).unwrap();
    state = state.with_home(home.clone());
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

    // --- run_started: two runs of pipeline p1 (latest name "Loop V2" wins),
    // one run of pipeline p2, one run of p1 that will fail outright (so its
    // Pipeline Manager attempt is excluded, never even "expected").
    insert_event(
        &state.db,
        "perf-r1",
        "2026-07-15T09:00:00Z",
        "run_started",
        None,
        None,
        serde_json::json!({
            "pipeline_id":"p1","pipeline_name":"Loop V1","target_repo":repo,"harness":"claude",
            "node_defs":[
                {"id":"worker","name":"Worker V1","node_type":"agent", "isolated_worktree":true},
                {"id":"build","name":"Build","node_type":"script"}
            ]
        }),
    )
    .await;
    insert_event(
        &state.db,
        "perf-r2",
        "2026-07-16T09:00:00Z",
        "run_started",
        None,
        None,
        serde_json::json!({
            "pipeline_id":"p1","pipeline_name":"Loop V2","target_repo":repo,"harness":"copilot",
            "node_defs":[{"id":"worker","name":"Worker V2","node_type":"agent", "isolated_worktree":true}]
        }),
    )
    .await;
    insert_event(
        &state.db,
        "perf-r3",
        "2026-07-17T09:00:00Z",
        "run_started",
        None,
        None,
        serde_json::json!({
            "pipeline_id":"p2","pipeline_name":"Other pipeline","target_repo":repo,"harness":"claude",
            "node_defs":[{"id":"solo","name":"Solo","node_type":"agent", "isolated_worktree":true}]
        }),
    )
    .await;
    insert_event(
        &state.db,
        "perf-r4",
        "2026-07-18T09:00:00Z",
        "run_started",
        None,
        None,
        serde_json::json!({
            "pipeline_id":"p1","pipeline_name":"Loop V2","target_repo":repo,"harness":"claude",
            "node_defs":[]
        }),
    )
    .await;

    // --- perf-r1: worker's loop lap (iter 1, 2), a stopped attempt on iter 3
    // that is then restarted and succeeds, a fourth iter on a harness with no
    // context source, plus a script node that must never appear at all.
    insert_event(
        &state.db, "perf-r1", "2026-07-15T09:10:00Z", "node_started",
        Some("worker"), Some(1),
        serde_json::json!({"node_type":"agent", "isolated_worktree":true,"harness":"claude","session_id":"claude-1"}),
    ).await;
    insert_event(
        &state.db,
        "perf-r1",
        "2026-07-15T09:20:00Z",
        "node_completed",
        Some("worker"),
        Some(1),
        serde_json::json!({}),
    )
    .await;
    insert_event(
        &state.db, "perf-r1", "2026-07-15T09:30:00Z", "node_started",
        Some("worker"), Some(2),
        serde_json::json!({"node_type":"agent", "isolated_worktree":true,"harness":"claude","session_id":"claude-2"}),
    ).await;
    insert_event(
        &state.db,
        "perf-r1",
        "2026-07-15T09:35:00Z",
        "node_completed",
        Some("worker"),
        Some(2),
        serde_json::json!({}),
    )
    .await;
    insert_event(
        &state.db, "perf-r1", "2026-07-15T09:40:00Z", "node_started",
        Some("worker"), Some(3),
        serde_json::json!({"node_type":"agent", "isolated_worktree":true,"harness":"claude","session_id":"claude-3"}),
    ).await;
    insert_event(
        &state.db,
        "perf-r1",
        "2026-07-15T09:41:00Z",
        "node_stopped",
        Some("worker"),
        Some(3),
        serde_json::json!({}),
    )
    .await;
    insert_event(
        &state.db, "perf-r1", "2026-07-15T09:45:00Z", "node_started",
        Some("worker"), Some(3),
        serde_json::json!({"node_type":"agent", "isolated_worktree":true,"harness":"claude","session_id":"claude-3b"}),
    ).await;
    insert_event(
        &state.db,
        "perf-r1",
        "2026-07-15T09:50:00Z",
        "node_completed",
        Some("worker"),
        Some(3),
        serde_json::json!({}),
    )
    .await;
    insert_event(
        &state.db, "perf-r1", "2026-07-15T10:00:00Z", "node_started",
        Some("worker"), Some(4),
        serde_json::json!({"node_type":"agent", "isolated_worktree":true,"harness":"future-harness","session_id":"future-1"}),
    ).await;
    insert_event(
        &state.db,
        "perf-r1",
        "2026-07-15T10:05:00Z",
        "node_completed",
        Some("worker"),
        Some(4),
        serde_json::json!({}),
    )
    .await;
    insert_event(
        &state.db,
        "perf-r1",
        "2026-07-15T09:10:00Z",
        "node_started",
        Some("build"),
        Some(1),
        serde_json::json!({"node_type":"script","harness":"claude","session_id":"build-1"}),
    )
    .await;
    insert_event(
        &state.db,
        "perf-r1",
        "2026-07-15T09:11:00Z",
        "node_completed",
        Some("build"),
        Some(1),
        serde_json::json!({}),
    )
    .await;

    // Pipeline Manager: one successful Run.
    insert_event(
        &state.db,
        "perf-r1",
        "2026-07-15T09:55:00Z",
        "run_completed",
        None,
        None,
        serde_json::json!({}),
    )
    .await;
    // Merge resolver: one failed attempt (excluded), then one successful
    // pairing scoped to worker's iter-3 restart (its own worktree).
    insert_event(
        &state.db,
        "perf-r1",
        "2026-07-15T09:51:00Z",
        "merge_resolver_started",
        None,
        None,
        serde_json::json!({}),
    )
    .await;
    insert_event(
        &state.db,
        "perf-r1",
        "2026-07-15T09:51:30Z",
        "merge_resolver_failed",
        None,
        None,
        serde_json::json!({}),
    )
    .await;
    insert_event(
        &state.db,
        "perf-r1",
        "2026-07-15T09:52:00Z",
        "merge_resolver_started",
        None,
        None,
        serde_json::json!({"conflicting_node_id":"worker","iter":3}),
    )
    .await;
    insert_event(
        &state.db,
        "perf-r1",
        "2026-07-15T09:53:00Z",
        "merge_resolver_completed",
        None,
        None,
        serde_json::json!({}),
    )
    .await;

    // --- perf-r2: worker's one Copilot execution.
    insert_event(
        &state.db, "perf-r2", "2026-07-16T09:10:00Z", "node_started",
        Some("worker"), Some(1),
        serde_json::json!({"node_type":"agent", "isolated_worktree":true,"harness":"copilot","session_id":"copilot-1"}),
    ).await;
    insert_event(
        &state.db,
        "perf-r2",
        "2026-07-16T09:20:00Z",
        "node_completed",
        Some("worker"),
        Some(1),
        serde_json::json!({}),
    )
    .await;

    // --- perf-r3: p2's solo node, one Claude execution. Also a second
    // successful Run, pooled into the same Pipeline Manager role.
    insert_event(
        &state.db, "perf-r3", "2026-07-17T09:10:00Z", "node_started",
        Some("solo"), Some(1),
        serde_json::json!({"node_type":"agent", "isolated_worktree":true,"harness":"claude","session_id":"claude-solo"}),
    ).await;
    insert_event(
        &state.db,
        "perf-r3",
        "2026-07-17T09:16:00Z",
        "node_completed",
        Some("solo"),
        Some(1),
        serde_json::json!({}),
    )
    .await;
    insert_event(
        &state.db,
        "perf-r3",
        "2026-07-17T09:20:00Z",
        "run_completed",
        None,
        None,
        serde_json::json!({}),
    )
    .await;

    // --- perf-r4: the whole Run fails outright — never an "expected"
    // Pipeline Manager attempt at all.
    insert_event(
        &state.db,
        "perf-r4",
        "2026-07-18T09:05:00Z",
        "run_failed",
        None,
        None,
        serde_json::json!({}),
    )
    .await;

    // --- Claude transcripts (main sessions + one Node's two subagents).
    let claude_root = home.join(".claude").join("projects");
    let write_claude_main = |run_id: &str, node_id: &str, iter: i64, session: &str, tokens: u64| {
        let dir = claude_root.join(pdo_harness::claude_transcript::encode_working_dir(
            &pdo_workspace::domain::worktree_layout::sub_worktree_path(
                &state.repo_root,
                run_id,
                node_id,
                iter,
            ),
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(format!("{session}.jsonl")),
            format!(
                r#"{{"type":"assistant","message":{{"model":"claude-opus-4-8","usage":{{"input_tokens":{tokens},"output_tokens":0}}}}}}"#
            ),
        )
        .unwrap();
        dir
    };
    let worker1_dir = write_claude_main("perf-r1", "worker", 1, "claude-1", 20000);
    write_claude_main("perf-r1", "worker", 2, "claude-2", 25000);
    let worker3_dir = write_claude_main("perf-r1", "worker", 3, "claude-3b", 30000);
    write_claude_main("perf-r3", "solo", 1, "claude-solo", 45000);

    // --- Infrastructure subagents (#585 user story #36): a Pipeline
    // Manager session left in the Run's own top-level worktree (never
    // colliding with an isolated Node's disjoint sub-worktree) and a
    // Merge resolver session left in the conflicting Node's own
    // iter-3 worktree, alongside (and excluded from) that Node's own
    // "claude-3b" session — resolved by exclusion, never by path alone.
    let pm_project_dir = claude_root.join(pdo_harness::claude_transcript::encode_working_dir(
        &pdo_workspace::domain::worktree_layout::worktree_dir_for_run(&state.repo_root, "perf-r1"),
    ));
    std::fs::create_dir_all(&pm_project_dir).unwrap();
    std::fs::write(
        pm_project_dir.join("pm-session.jsonl"),
        r#"{"type":"assistant","message":{"model":"claude-opus-4-8","usage":{"input_tokens":12000,"output_tokens":0}}}"#,
    )
    .unwrap();
    let pm_subagents_dir = pm_project_dir.join("pm-session").join("subagents");
    std::fs::create_dir_all(&pm_subagents_dir).unwrap();
    std::fs::write(
        pm_subagents_dir.join("reviewer.jsonl"),
        "{\"type\":\"assistant\",\"timestamp\":\"2026-07-15T09:50:10Z\",\"message\":{\"model\":\"claude-opus-4-8\",\"usage\":{\"input_tokens\":8000,\"output_tokens\":0}}}\n\
         {\"type\":\"user\",\"timestamp\":\"2026-07-15T09:50:40Z\"}\n",
    )
    .unwrap();

    std::fs::write(
        worker3_dir.join("mr-session.jsonl"),
        r#"{"type":"assistant","message":{"model":"claude-opus-4-8","usage":{"input_tokens":9000,"output_tokens":0}}}"#,
    )
    .unwrap();
    let mr_subagents_dir = worker3_dir.join("mr-session").join("subagents");
    std::fs::create_dir_all(&mr_subagents_dir).unwrap();
    std::fs::write(
        mr_subagents_dir.join("patch-reviewer.jsonl"),
        "{\"type\":\"assistant\",\"timestamp\":\"2026-07-15T09:52:10Z\",\"message\":{\"model\":\"claude-opus-4-8\",\"usage\":{\"input_tokens\":7000,\"output_tokens\":0}}}\n\
         {\"type\":\"user\",\"timestamp\":\"2026-07-15T09:52:40Z\"}\n",
    )
    .unwrap();

    let subagents_dir = worker1_dir.join("claude-1").join("subagents");
    std::fs::create_dir_all(&subagents_dir).unwrap();
    std::fs::write(
        subagents_dir.join("explore.jsonl"),
        "{\"type\":\"assistant\",\"timestamp\":\"2026-07-15T09:10:00Z\",\"message\":{\"model\":\"claude-opus-4-8\",\"usage\":{\"input_tokens\":55000,\"output_tokens\":0}}}\n\
         {\"type\":\"user\",\"timestamp\":\"2026-07-15T09:15:00Z\"}\n",
    )
    .unwrap();
    std::fs::write(
        subagents_dir.join("3fa85f64-5717-4562-b3fc-2c963f66afa6.jsonl"),
        "{\"type\":\"assistant\",\"message\":{\"model\":\"claude-opus-4-8\",\"usage\":{\"input_tokens\":30000,\"output_tokens\":0}}}\n",
    )
    .unwrap();

    // --- Copilot journal for the one Copilot execution.
    let copilot_dir = home
        .join(".copilot")
        .join("session-state")
        .join("copilot-1");
    std::fs::create_dir_all(&copilot_dir).unwrap();
    std::fs::write(
        copilot_dir.join("events.jsonl"),
        r#"{"type":"session.shutdown","data":{"usage":{"inputTokens":40000,"outputTokens":0}}}"#,
    )
    .unwrap();

    let response = state
        .router()
        .oneshot(
            Request::builder()
                .uri("/stats/performance?from=2026-07-15T00:00:00Z&to=2026-07-19T00:00:00Z")
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

    // --- Pipeline identity + latest-name-wins.
    let by_pipeline = body["by_pipeline"].as_array().unwrap();
    assert_eq!(by_pipeline.len(), 2, "got: {by_pipeline:#?}");
    let p1 = by_pipeline.iter().find(|p| p["id"] == "p1").unwrap();
    assert_eq!(p1["name"], "Loop V2");
    let p2 = by_pipeline.iter().find(|p| p["id"] == "p2").unwrap();
    assert_eq!(p2["name"], "Other pipeline");

    // --- Node identity + latest-name-wins + success-only filtering: 3
    // observations (loop lap + restart), never 4 (stopped attempt excluded)
    // or 5 (script node excluded).
    let nodes = p1["nodes"].as_array().unwrap();
    assert_eq!(
        nodes.len(),
        1,
        "the script node must never appear: {nodes:#?}"
    );
    let worker = &nodes[0];
    assert_eq!(worker["id"], "worker");
    assert_eq!(worker["name"], "Worker V2");
    let worker_harnesses = worker["harnesses"].as_array().unwrap();
    let worker_claude = worker_harnesses
        .iter()
        .find(|h| h["harness"] == "claude")
        .unwrap();
    assert_eq!(worker_claude["context"]["measured"], 3);
    assert_eq!(worker_claude["context"]["expected"], 3);
    assert_eq!(worker_claude["context"]["stats"]["min"], 20000.0);
    assert_eq!(worker_claude["context"]["stats"]["max"], 30000.0);
    assert_eq!(worker_claude["context"]["stats"]["mean"], 25000.0);
    assert_eq!(worker_claude["context"]["stats"]["median"], 25000.0);
    // #811 — the Tukey fences the « Fenced » zoom level draws its whiskers
    // at, on every `by_pipeline` distribution. This cohort holds no outlier,
    // so they land on min/max: the same picture as « Full », which is the
    // honest answer, not a missing field.
    assert_eq!(worker_claude["context"]["stats"]["fence_low"], 20000.0);
    assert_eq!(worker_claude["context"]["stats"]["fence_high"], 30000.0);
    assert_eq!(worker_claude["duration"]["measured"], 3);
    assert_eq!(worker_claude["duration"]["stats"]["min"], 300000.0);
    assert_eq!(worker_claude["duration"]["stats"]["max"], 600000.0);
    assert_eq!(worker_claude["duration"]["stats"]["mean"], 400000.0);
    let worker_copilot = worker_harnesses
        .iter()
        .find(|h| h["harness"] == "copilot")
        .unwrap();
    assert_eq!(worker_copilot["context"]["stats"]["mean"], 40000.0);
    assert_eq!(worker_copilot["duration"]["stats"]["mean"], 600000.0);
    let worker_future = worker_harnesses
        .iter()
        .find(|h| h["harness"] == "future-harness")
        .unwrap();
    assert!(
        worker_future["context"]["stats"].is_null(),
        "a harness with no context source must never fake a value: {worker_future}"
    );
    assert_eq!(
        worker_future["context"]["measured"], 0,
        "coverage survives a fully absent metric: {worker_future}"
    );
    assert_eq!(worker_future["context"]["expected"], 1);
    assert_eq!(
        worker_future["context"]["missing_reasons"],
        serde_json::json!(["harness has no context-usage source"])
    );
    assert_eq!(worker_future["duration"]["stats"]["mean"], 300000.0);
    assert_eq!(worker_future["duration"]["measured"], 1);
    assert_eq!(worker_future["duration"]["expected"], 1);

    // --- Subagent grouping: declared name + "Unidentified subagent" fallback,
    // never touching the parent Node's own distribution above.
    let subagents = worker["subagents"].as_array().unwrap();
    assert_eq!(subagents.len(), 2, "got: {subagents:#?}");
    let explore = subagents.iter().find(|s| s["name"] == "explore").unwrap();
    let explore_claude = explore["harnesses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["harness"] == "claude")
        .unwrap();
    assert_eq!(explore_claude["context"]["stats"]["mean"], 55000.0);
    assert_eq!(explore_claude["duration"]["stats"]["mean"], 300000.0);
    let unidentified = subagents
        .iter()
        .find(|s| s["name"] == "Unidentified subagent")
        .unwrap();
    let unidentified_claude = unidentified["harnesses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["harness"] == "claude")
        .unwrap();
    assert_eq!(unidentified_claude["context"]["stats"]["mean"], 30000.0);
    assert!(
        unidentified_claude["duration"]["stats"].is_null(),
        "no reliable bounds in this subagent transcript: {unidentified_claude}"
    );
    assert_eq!(unidentified_claude["duration"]["measured"], 0);
    assert_eq!(
        unidentified_claude["duration"]["missing_reasons"],
        serde_json::json!(["no reliable start/end bounds in subagent transcript"])
    );

    // --- Pipeline aggregation pools from raw main sessions, matching its own
    // single Node here exactly (p1 has only one Node).
    let p1_claude = p1["harnesses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["harness"] == "claude")
        .unwrap();
    assert_eq!(p1_claude["context"]["stats"]["mean"], 25000.0);

    // --- p2's solo Node.
    let p2_worker = &p2["nodes"].as_array().unwrap()[0];
    assert_eq!(p2_worker["id"], "solo");
    let p2_claude = p2_worker["harnesses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["harness"] == "claude")
        .unwrap();
    assert_eq!(p2_claude["context"]["stats"]["mean"], 45000.0);

    // --- `total`: every main-session Claude observation across BOTH
    // pipelines pooled together (never Infrastructure, never subagents).
    let total_claude = body["total"]["harnesses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["harness"] == "claude")
        .unwrap();
    assert_eq!(total_claude["context"]["measured"], 4);
    assert_eq!(total_claude["context"]["stats"]["min"], 20000.0);
    assert_eq!(total_claude["context"]["stats"]["max"], 45000.0);
    assert_eq!(total_claude["context"]["stats"]["mean"], 30000.0);
    assert_eq!(total_claude["context"]["stats"]["median"], 27500.0);

    // --- Infrastructure: Pipeline Manager pools BOTH successful Runs (never
    // the failed one); Merge resolver keeps only its successful pairing.
    // Both roles now discover their own residual Claude session — and its
    // subagents — by exclusion from the directory their turns are known to
    // land in (#585 user story #36), never guessing at the ambiguous ones.
    let infra = body["infrastructure"].as_array().unwrap();
    assert_eq!(infra.len(), 2, "got: {infra:#?}");
    let pipeline_manager = infra
        .iter()
        .find(|r| r["id"] == "pipeline-manager")
        .unwrap();
    assert_eq!(pipeline_manager["name"], "Pipeline Manager");
    let pm_claude = pipeline_manager["harnesses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["harness"] == "claude")
        .unwrap();
    // Perf-r1's Run resolves a session (measured); perf-r3's Run leaves no
    // attributable session in its own top-level worktree (absent, but
    // coverage survives: expected still counts it).
    assert_eq!(pm_claude["context"]["measured"], 1);
    assert_eq!(pm_claude["context"]["expected"], 2);
    assert_eq!(pm_claude["context"]["stats"]["mean"], 12000.0);
    assert_eq!(
        pm_claude["context"]["missing_reasons"],
        serde_json::json!(["no attributable session for this infrastructure role"])
    );
    assert_eq!(pm_claude["duration"]["measured"], 2);
    assert_eq!(pm_claude["duration"]["expected"], 2);
    assert_eq!(pm_claude["duration"]["stats"]["min"], 1200000.0);
    assert_eq!(pm_claude["duration"]["stats"]["max"], 3300000.0);
    // #811 — fences on the `infrastructure` tree too, including the
    // one-observation Context distribution, where they collapse onto the
    // single value (never a null, never a theoretical bound).
    assert_eq!(pm_claude["duration"]["stats"]["fence_low"], 1200000.0);
    assert_eq!(pm_claude["duration"]["stats"]["fence_high"], 3300000.0);
    assert_eq!(pm_claude["context"]["stats"]["fence_low"], 12000.0);
    assert_eq!(pm_claude["context"]["stats"]["fence_high"], 12000.0);
    let pm_subagents = pipeline_manager["subagents"].as_array().unwrap();
    assert_eq!(pm_subagents.len(), 1, "got: {pm_subagents:#?}");
    let pm_reviewer_claude = pm_subagents[0]["harnesses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["harness"] == "claude")
        .unwrap();
    assert_eq!(pm_subagents[0]["name"], "reviewer");
    assert_eq!(pm_reviewer_claude["context"]["stats"]["mean"], 8000.0);
    assert_eq!(pm_reviewer_claude["duration"]["stats"]["mean"], 30000.0);

    let merge_resolver = infra.iter().find(|r| r["id"] == "merge-resolver").unwrap();
    assert_eq!(merge_resolver["name"], "Merge resolver");
    let mr_claude = merge_resolver["harnesses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["harness"] == "claude")
        .unwrap();
    assert_eq!(mr_claude["context"]["measured"], 1);
    assert_eq!(mr_claude["context"]["stats"]["mean"], 9000.0);
    assert_eq!(mr_claude["duration"]["measured"], 1);
    assert_eq!(mr_claude["duration"]["stats"]["mean"], 60000.0);
    let mr_subagents = merge_resolver["subagents"].as_array().unwrap();
    assert_eq!(mr_subagents.len(), 1, "got: {mr_subagents:#?}");
    let mr_patch_claude = mr_subagents[0]["harnesses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["harness"] == "claude")
        .unwrap();
    assert_eq!(mr_subagents[0]["name"], "patch-reviewer");
    assert_eq!(mr_patch_claude["context"]["stats"]["mean"], 7000.0);
    assert_eq!(mr_patch_claude["duration"]["stats"]["mean"], 30000.0);

    // --- infrastructure_total: both Infrastructure roles' raw observations
    // pooled together — NOT the mean of Pipeline Manager's mean and Merge
    // resolver's mean (#585 review follow-up, blocker 3). Context pools
    // Pipeline Manager's one measured peak (12000) with Merge resolver's
    // one measured peak (9000): 2 measured out of 3 expected occurrences
    // (Pipeline Manager's own expected=2 + Merge resolver's own expected=1).
    // Duration pools all three occurrences directly, since every occurrence
    // is paired Run/resolver timestamps with no absence possible here.
    let infra_total_claude = body["infrastructure_total"]["harnesses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["harness"] == "claude")
        .unwrap();
    assert_eq!(infra_total_claude["context"]["expected"], 3);
    assert_eq!(infra_total_claude["context"]["measured"], 2);
    assert_eq!(infra_total_claude["context"]["stats"]["mean"], 10500.0);
    assert_eq!(infra_total_claude["duration"]["expected"], 3);
    assert_eq!(infra_total_claude["duration"]["measured"], 3);
    assert_eq!(infra_total_claude["duration"]["stats"]["mean"], 1520000.0);
}

/// `GET /stats/performance` (#585, user story #36) — an Infrastructure
/// role's own session is never guessed at by path or timing alone: a
/// non-isolated Claude Node sharing the Pipeline Manager's top-level
/// worktree, with no recorded `session_id`, makes that directory's
/// residual session ambiguous — Context (and its subagents) stays a
/// motivated absence rather than an unsafe guess, while Duration (paired
/// from `RunStarted`/`RunCompleted` timestamps, no session lookup
/// involved) is unaffected.
#[tokio::test]
async fn stats_performance_infrastructure_context_is_ambiguous_when_a_shared_session_has_no_recorded_identity(
) {
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    let mut state = Harness::new().await;
    let home = state.scratch("test-stats-performance-ambiguous");
    let _cleanup = Cleanup(home.clone());
    std::fs::create_dir_all(&home).unwrap();
    state = state.with_home(home.clone());
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

    insert_event(
        &state.db,
        "perf-amb",
        "2026-08-01T09:00:00Z",
        "run_started",
        None,
        None,
        serde_json::json!({
            "pipeline_id":"amb","pipeline_name":"Ambiguous","target_repo":repo,"harness":"claude",
            "node_defs":[{"id":"docs","name":"Docs","node_type":"agent", "isolated_worktree":false}]
        }),
    )
    .await;
    // A non-isolated (so it shares the Run's top-level worktree)
    // Claude Node with no recorded `session_id` — the directory can no
    // longer be trusted to hold only the Pipeline Manager's own session.
    insert_event(
        &state.db,
        "perf-amb",
        "2026-08-01T09:10:00Z",
        "node_started",
        Some("docs"),
        Some(1),
        serde_json::json!({"node_type":"agent", "isolated_worktree":false,"harness":"claude"}),
    )
    .await;
    insert_event(
        &state.db,
        "perf-amb",
        "2026-08-01T09:15:00Z",
        "node_completed",
        Some("docs"),
        Some(1),
        serde_json::json!({}),
    )
    .await;
    insert_event(
        &state.db,
        "perf-amb",
        "2026-08-01T09:20:00Z",
        "run_completed",
        None,
        None,
        serde_json::json!({}),
    )
    .await;

    // A residual session does exist in the shared directory — but it must
    // never be attributed, since the co-resident Node's own identity is
    // unknown.
    let claude_root = home.join(".claude").join("projects");
    let pm_project_dir = claude_root.join(pdo_harness::claude_transcript::encode_working_dir(
        &pdo_workspace::domain::worktree_layout::worktree_dir_for_run(&state.repo_root, "perf-amb"),
    ));
    std::fs::create_dir_all(&pm_project_dir).unwrap();
    std::fs::write(
        pm_project_dir.join("pm-session.jsonl"),
        r#"{"type":"assistant","message":{"model":"claude-opus-4-8","usage":{"input_tokens":12000,"output_tokens":0}}}"#,
    )
    .unwrap();

    let response = state
        .router()
        .oneshot(
            Request::builder()
                .uri("/stats/performance?from=2026-08-01T00:00:00Z&to=2026-08-02T00:00:00Z")
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

    let infra = body["infrastructure"].as_array().unwrap();
    let pipeline_manager = infra
        .iter()
        .find(|r| r["id"] == "pipeline-manager")
        .unwrap();
    let pm_claude = pipeline_manager["harnesses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["harness"] == "claude")
        .unwrap();
    assert_eq!(pm_claude["context"]["measured"], 0);
    assert_eq!(pm_claude["context"]["expected"], 1);
    assert!(pm_claude["context"]["stats"].is_null());
    assert_eq!(
        pm_claude["context"]["missing_reasons"],
        serde_json::json!(["ambiguous session identity in a directory shared with a Node"])
    );
    assert!(
        pipeline_manager["subagents"].as_array().unwrap().is_empty(),
        "an ambiguous session must never contribute subagents either: {pipeline_manager}"
    );
    // Duration is untouched by the ambiguity — it never looked at a
    // session file at all.
    assert_eq!(pm_claude["duration"]["measured"], 1);
    assert_eq!(pm_claude["duration"]["stats"]["mean"], 1200000.0);
}

/// #585 review follow-up (blocker 4): Copilot's journal has no declared
/// subagent concept, and its store is flat — this must be a **motivated**
/// absence (the dispatch answers empty), not a coincidence of never having
/// the right directory to look in. Proven by planting a directory that
/// *would* read as subagents under Claude's convention right next to the
/// Copilot session's own journal, and asserting it never surfaces: the
/// Copilot execution's own Context is still read correctly (the absence is
/// specific to subagent discovery, not a wholesale failure to read Copilot
/// at all).
#[tokio::test]
async fn stats_performance_copilot_subagents_are_a_motivated_absence_not_silently_missing() {
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    let mut state = Harness::new().await;
    let home = state.scratch("test-stats-performance-copilot-subagents");
    let _cleanup = Cleanup(home.clone());
    std::fs::create_dir_all(&home).unwrap();
    state = state.with_home(home.clone());
    let repo = state.repo_root.to_string_lossy().into_owned();

    sqlx::query(
        "INSERT INTO events (run_id, ts, kind, node_id, iter, payload) VALUES \
         ('perf-cs1', '2026-08-01T09:00:00Z', 'run_started', NULL, NULL, ?)",
    )
    .bind(
        serde_json::json!({
            "pipeline_id":"p1","pipeline_name":"Loop","target_repo":repo,"harness":"copilot",
            "node_defs":[{"id":"worker","name":"Worker","node_type":"agent", "isolated_worktree":true}]
        })
        .to_string(),
    )
    .execute(&state.db)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO events (run_id, ts, kind, node_id, iter, payload) VALUES \
         ('perf-cs1', '2026-08-01T09:10:00Z', 'node_started', 'worker', 1, ?)",
    )
    .bind(
        serde_json::json!({"node_type":"agent", "isolated_worktree":true,"harness":"copilot","session_id":"copilot-cs1"})
            .to_string(),
    )
    .execute(&state.db)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO events (run_id, ts, kind, node_id, iter, payload) VALUES \
         ('perf-cs1', '2026-08-01T09:20:00Z', 'node_completed', 'worker', 1, '{}')",
    )
    .execute(&state.db)
    .await
    .unwrap();

    let copilot_session_dir = home
        .join(".copilot")
        .join("session-state")
        .join("copilot-cs1");
    std::fs::create_dir_all(&copilot_session_dir).unwrap();
    std::fs::write(
        copilot_session_dir.join("events.jsonl"),
        r#"{"type":"session.shutdown","data":{"usage":{"inputTokens":12000,"outputTokens":0}}}"#,
    )
    .unwrap();
    // Plant a directory that reads as subagents under Claude's own
    // convention, right next to the Copilot session's journal.
    let lookalike_subagents = copilot_session_dir.join("subagents");
    std::fs::create_dir_all(&lookalike_subagents).unwrap();
    std::fs::write(
        lookalike_subagents.join("reviewer.jsonl"),
        "{\"type\":\"assistant\",\"message\":{\"model\":\"claude-opus-4-8\",\"usage\":{\"input_tokens\":999,\"output_tokens\":0}}}\n",
    )
    .unwrap();

    let response = state
        .router()
        .oneshot(
            Request::builder()
                .uri("/stats/performance?from=2026-08-01T00:00:00Z&to=2026-08-02T00:00:00Z")
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

    let by_pipeline = body["by_pipeline"].as_array().unwrap();
    let p1 = by_pipeline.iter().find(|p| p["id"] == "p1").unwrap();
    let worker = &p1["nodes"].as_array().unwrap()[0];
    let worker_copilot = worker["harnesses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["harness"] == "copilot")
        .unwrap();
    // The motivated absence is scoped to subagents — Context itself still
    // reads correctly for the Copilot execution.
    assert_eq!(worker_copilot["context"]["measured"], 1);
    assert_eq!(worker_copilot["context"]["stats"]["mean"], 12000.0);
    assert!(
        worker["subagents"].as_array().unwrap().is_empty(),
        "a look-alike subagents/ directory must never surface for copilot: {worker}"
    );
}

/// `GET /stats/performance` (#585 review follow-up, blocker 2) — the
/// result is cached in memory: a repeat request for the identical `[from,
/// to)` window, with nothing in the cohort's events or the underlying
/// Claude transcript changed, is served from the memo — [`fold_performance`]
/// (the transcript-reading heavy fold) never runs a second time — while
/// still returning the exact same payload a fresh computation would.
#[tokio::test]
async fn stats_performance_result_is_memoized_and_reused_when_nothing_changed() {
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    let mut state = Harness::new().await;
    let home = state.scratch("test-stats-performance-cache");
    let _cleanup = Cleanup(home.clone());
    std::fs::create_dir_all(&home).unwrap();
    state = state.with_home(home.clone());
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

    insert_event(
        &state.db,
        "cache-r1",
        "2031-01-15T09:00:00Z",
        "run_started",
        None,
        None,
        serde_json::json!({
            "pipeline_id":"cp1","pipeline_name":"Cache Pipeline","target_repo":repo,"harness":"claude",
            "node_defs":[{"id":"worker","name":"Worker","node_type":"agent", "isolated_worktree":true}]
        }),
    )
    .await;
    insert_event(
        &state.db, "cache-r1", "2031-01-15T09:10:00Z", "node_started",
        Some("worker"), Some(1),
        serde_json::json!({"node_type":"agent", "isolated_worktree":true,"harness":"claude","session_id":"cache-1"}),
    )
    .await;
    insert_event(
        &state.db,
        "cache-r1",
        "2031-01-15T09:20:00Z",
        "node_completed",
        Some("worker"),
        Some(1),
        serde_json::json!({}),
    )
    .await;

    let claude_root = home.join(".claude").join("projects");
    let dir = claude_root.join(pdo_harness::claude_transcript::encode_working_dir(
        &pdo_workspace::domain::worktree_layout::sub_worktree_path(
            &state.repo_root,
            "cache-r1",
            "worker",
            1,
        ),
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("cache-1.jsonl"),
        r#"{"type":"assistant","message":{"model":"claude-opus-4-8","usage":{"input_tokens":10000,"output_tokens":0}}}"#,
    )
    .unwrap();

    let from = "2031-01-15T00:00:00Z";
    let to = "2031-01-16T00:00:00Z";
    let uri = format!("/stats/performance?from={from}&to={to}");

    async fn call(state: &Harness, uri: &str) -> serde_json::Value {
        let response = state
            .router()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap()
    }

    let first = call(&state, &uri).await;
    let after_first = state.recomputes(from, to);
    assert_eq!(
        after_first, 1,
        "first request must run the heavy fold exactly once"
    );

    let second = call(&state, &uri).await;
    let after_second = state.recomputes(from, to);
    assert_eq!(
        after_second, after_first,
        "an identical repeat request must be served from the memo, not recomputed"
    );
    assert_eq!(
        first, second,
        "a memo hit must return byte-for-byte the same payload"
    );
    let refreshed = call(&state, &format!("{uri}&refresh=true")).await;
    assert_eq!(
        state.recomputes(from, to),
        after_second + 1,
        "an explicit user refresh must bypass the memo"
    );
    assert_eq!(first, refreshed);
    let worker_claude = first["by_pipeline"][0]["nodes"][0]["harnesses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["harness"] == "claude")
        .unwrap();
    assert_eq!(worker_claude["context"]["stats"]["mean"], 10000.0);
}

/// `GET /stats/performance` (#585 review follow-up, blocker 2) — the memo
/// key covers both halves the issue calls out: cohort **event** state (a
/// newly appended Run changes the answer on the very next identical
/// request) and **transcript/journal source** state (rewriting a session's
/// own file — same events, same session id, new content/mtime — does too).
/// Neither can silently serve a stale answer.
#[tokio::test]
async fn stats_performance_cache_is_invalidated_by_new_events_and_by_transcript_changes() {
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    let mut state = Harness::new().await;
    let home = state.scratch("test-stats-performance-cache-invalidation");
    let _cleanup = Cleanup(home.clone());
    std::fs::create_dir_all(&home).unwrap();
    state = state.with_home(home.clone());
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

    insert_event(
        &state.db,
        "cache-i1",
        "2031-02-15T09:00:00Z",
        "run_started",
        None,
        None,
        serde_json::json!({
            "pipeline_id":"cip1","pipeline_name":"Cache Invalidation Pipeline","target_repo":repo,"harness":"claude",
            "node_defs":[{"id":"worker","name":"Worker","node_type":"agent", "isolated_worktree":true}]
        }),
    )
    .await;
    insert_event(
        &state.db, "cache-i1", "2031-02-15T09:10:00Z", "node_started",
        Some("worker"), Some(1),
        serde_json::json!({"node_type":"agent", "isolated_worktree":true,"harness":"claude","session_id":"cache-i-1"}),
    )
    .await;
    insert_event(
        &state.db,
        "cache-i1",
        "2031-02-15T09:20:00Z",
        "node_completed",
        Some("worker"),
        Some(1),
        serde_json::json!({}),
    )
    .await;

    let claude_root = home.join(".claude").join("projects");
    let dir = claude_root.join(pdo_harness::claude_transcript::encode_working_dir(
        &pdo_workspace::domain::worktree_layout::sub_worktree_path(
            &state.repo_root,
            "cache-i1",
            "worker",
            1,
        ),
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("cache-i-1.jsonl"),
        r#"{"type":"assistant","message":{"model":"claude-opus-4-8","usage":{"input_tokens":10000,"output_tokens":0}}}"#,
    )
    .unwrap();

    let from = "2031-02-15T00:00:00Z";
    let to = "2031-02-17T00:00:00Z";
    let uri = format!("/stats/performance?from={from}&to={to}");

    async fn call(state: &Harness, uri: &str) -> serde_json::Value {
        let response = state
            .router()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap()
    }

    let first = call(&state, &uri).await;
    assert_eq!(state.recomputes(from, to), 1);
    let worker_claude_first = first["by_pipeline"][0]["nodes"][0]["harnesses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["harness"] == "claude")
        .unwrap()
        .clone();
    assert_eq!(worker_claude_first["context"]["measured"], 1);
    assert_eq!(worker_claude_first["context"]["stats"]["mean"], 10000.0);

    // --- (a) cohort event state: a second Run appears in the same window.
    insert_event(
        &state.db,
        "cache-i2",
        "2031-02-16T09:00:00Z",
        "run_started",
        None,
        None,
        serde_json::json!({
            "pipeline_id":"cip1","pipeline_name":"Cache Invalidation Pipeline","target_repo":repo,"harness":"claude",
            "node_defs":[{"id":"worker","name":"Worker","node_type":"agent", "isolated_worktree":true}]
        }),
    )
    .await;
    insert_event(
        &state.db, "cache-i2", "2031-02-16T09:10:00Z", "node_started",
        Some("worker"), Some(1),
        serde_json::json!({"node_type":"agent", "isolated_worktree":true,"harness":"claude","session_id":"cache-i-2"}),
    )
    .await;
    insert_event(
        &state.db,
        "cache-i2",
        "2031-02-16T09:20:00Z",
        "node_completed",
        Some("worker"),
        Some(1),
        serde_json::json!({}),
    )
    .await;
    let dir2 = claude_root.join(pdo_harness::claude_transcript::encode_working_dir(
        &pdo_workspace::domain::worktree_layout::sub_worktree_path(
            &state.repo_root,
            "cache-i2",
            "worker",
            1,
        ),
    ));
    std::fs::create_dir_all(&dir2).unwrap();
    std::fs::write(
        dir2.join("cache-i-2.jsonl"),
        r#"{"type":"assistant","message":{"model":"claude-opus-4-8","usage":{"input_tokens":20000,"output_tokens":0}}}"#,
    )
    .unwrap();

    let after_new_run = call(&state, &uri).await;
    assert_eq!(
        state.recomputes(from, to),
        2,
        "a new Run in the same window must force a recompute"
    );
    let worker_claude_after_new_run = after_new_run["by_pipeline"][0]["nodes"][0]["harnesses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["harness"] == "claude")
        .unwrap();
    assert_eq!(
        worker_claude_after_new_run["context"]["measured"], 2,
        "the new Run's own Context observation must now be pooled: {after_new_run:#?}"
    );

    // --- (b) transcript/journal source state: rewrite the FIRST Run's own
    // session file in place (no new event at all) with a different token
    // count.
    std::fs::write(
        dir.join("cache-i-1.jsonl"),
        r#"{"type":"assistant","message":{"model":"claude-opus-4-8","usage":{"input_tokens":99000,"output_tokens":0}}}"#,
    )
    .unwrap();

    let after_transcript_rewrite = call(&state, &uri).await;
    assert_eq!(
        state.recomputes(from, to),
        3,
        "rewriting a session's own transcript, with no new event, must still force a recompute"
    );
    let worker_claude_after_rewrite = after_transcript_rewrite["by_pipeline"][0]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == "worker")
        .unwrap()["harnesses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["harness"] == "claude")
        .unwrap()
        .clone();
    // Node-level rows sum over both Runs' own Worker iter-1 sessions; only
    // cache-i1's was rewritten (10000 -> 99000), cache-i2's (20000) is
    // unaffected — the mean must reflect the rewrite, not the stale value.
    assert_eq!(worker_claude_after_rewrite["context"]["measured"], 2);
    assert_eq!(
        worker_claude_after_rewrite["context"]["stats"]["mean"],
        (99000.0 + 20000.0) / 2.0
    );
}

/// `GET /stats/performance` (#737, ADR-0065) — the « By model » axis: the
/// peak of each session file is attributed to the model of THAT file (a
/// subagent has its own bucket), a mute source falls back to the requested
/// model × effort (marked `requested`), a harness with an observed source
/// (`copilot`) buckets its journal's model + effort, and the `by_pipeline`
/// Node leaf carries its model × effort couples — the drill "Node → modèle
/// × effort". The existing memo fingerprints cover the axis: a subagent
/// file's rewrite forces a recompute, an unchanged state is a memo hit.
#[tokio::test]
async fn stats_performance_by_model_ventilates_each_session_file_to_its_own_model() {
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    let mut state = Harness::new().await;
    let home = state.scratch("test-stats-performance-by-model");
    let _cleanup = Cleanup(home.clone());
    std::fs::create_dir_all(&home).unwrap();
    state = state.with_home(home.clone());
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

    // perf-m1: a claude Node run twice — iter 1 whose session file names
    // its model (opus) and whose subagent runs on another one (sonnet);
    // iter 2 whose file names no model at all (mute source), falling back
    // to the requested model × effort.
    insert_event(
        &state.db,
        "perf-m1",
        "2026-08-01T09:00:00Z",
        "run_started",
        None,
        None,
        serde_json::json!({
            "pipeline_id":"mp1","pipeline_name":"Model loop","target_repo":repo,"harness":"claude",
            "node_defs":[{"id":"worker","name":"Worker","node_type":"agent", "isolated_worktree":true}]
        }),
    )
    .await;
    insert_event(
        &state.db, "perf-m1", "2026-08-01T09:10:00Z", "node_started",
        Some("worker"), Some(1),
        serde_json::json!({"node_type":"agent", "isolated_worktree":true,"harness":"claude","session_id":"m1-iter1"}),
    ).await;
    insert_event(
        &state.db,
        "perf-m1",
        "2026-08-01T09:20:00Z",
        "node_completed",
        Some("worker"),
        Some(1),
        serde_json::json!({}),
    )
    .await;
    insert_event(
        &state.db, "perf-m1", "2026-08-01T09:30:00Z", "node_started",
        Some("worker"), Some(2),
        serde_json::json!({"node_type":"agent", "isolated_worktree":true,"harness":"claude","session_id":"m1-iter2","model":"sonnet","effort":"high"}),
    ).await;
    insert_event(
        &state.db,
        "perf-m1",
        "2026-08-01T09:35:00Z",
        "node_completed",
        Some("worker"),
        Some(2),
        serde_json::json!({}),
    )
    .await;
    // perf-m2: one copilot execution — its journal names the model AND the
    // effort at the usage points, so both are observed.
    insert_event(
        &state.db,
        "perf-m2",
        "2026-08-02T09:00:00Z",
        "run_started",
        None,
        None,
        serde_json::json!({
            "pipeline_id":"mp2","pipeline_name":"Other pipeline","target_repo":repo,"harness":"copilot",
            "node_defs":[{"id":"solo","name":"Solo","node_type":"agent", "isolated_worktree":true}]
        }),
    )
    .await;
    insert_event(
        &state.db, "perf-m2", "2026-08-02T09:10:00Z", "node_started",
        Some("solo"), Some(1),
        serde_json::json!({"node_type":"agent", "isolated_worktree":true,"harness":"copilot","session_id":"m-copilot-1"}),
    ).await;
    insert_event(
        &state.db,
        "perf-m2",
        "2026-08-02T09:20:00Z",
        "node_completed",
        Some("solo"),
        Some(1),
        serde_json::json!({}),
    )
    .await;

    // --- claude transcripts: iter 1's main file (opus) + subagent file
    // (sonnet — its own bucket), iter 2's mute file (no model line).
    let claude_root = home.join(".claude").join("projects");
    let dir1 = claude_root.join(pdo_harness::claude_transcript::encode_working_dir(
        &pdo_workspace::domain::worktree_layout::sub_worktree_path(
            &state.repo_root,
            "perf-m1",
            "worker",
            1,
        ),
    ));
    std::fs::create_dir_all(&dir1).unwrap();
    std::fs::write(
        dir1.join("m1-iter1.jsonl"),
        r#"{"type":"assistant","message":{"model":"claude-opus-4-8","usage":{"input_tokens":20000,"output_tokens":0}}}"#,
    )
    .unwrap();
    let subagents_dir = dir1.join("m1-iter1").join("subagents");
    std::fs::create_dir_all(&subagents_dir).unwrap();
    std::fs::write(
        subagents_dir.join("explore.jsonl"),
        "{\"type\":\"assistant\",\"timestamp\":\"2026-08-01T09:10:00Z\",\"message\":{\"model\":\"claude-sonnet-4-5\",\"usage\":{\"input_tokens\":55000,\"output_tokens\":0}}}\n\
         {\"type\":\"user\",\"timestamp\":\"2026-08-01T09:15:00Z\"}\n",
    )
    .unwrap();
    let dir2 = claude_root.join(pdo_harness::claude_transcript::encode_working_dir(
        &pdo_workspace::domain::worktree_layout::sub_worktree_path(
            &state.repo_root,
            "perf-m1",
            "worker",
            2,
        ),
    ));
    std::fs::create_dir_all(&dir2).unwrap();
    // A file that reads but says nothing: no model line, no readable peak.
    std::fs::write(dir2.join("m1-iter2.jsonl"), "{not json\n").unwrap();

    // --- copilot journal: the model and effort at the usage point, plus
    // the peak reading the shutdown carries.
    let copilot_dir = home
        .join(".copilot")
        .join("session-state")
        .join("m-copilot-1");
    std::fs::create_dir_all(&copilot_dir).unwrap();
    std::fs::write(
        copilot_dir.join("events.jsonl"),
        r#"{"type":"session.start","data":{"selectedModel":"gpt-5","reasoningEffort":"medium"}}
{"type":"session.usage_checkpoint","data":{"totalNanoAiu":5000000000,"modelCacheState":[{"modelId":"gpt-5"}]}}
{"type":"session.shutdown","data":{"usage":{"inputTokens":40000,"outputTokens":0}}}"#,
    )
    .unwrap();

    let from = "2026-08-01T00:00:00Z";
    let to = "2026-08-03T00:00:00Z";
    let uri = format!("/stats/performance?from={from}&to={to}");
    async fn call(state: &Harness, uri: &str) -> serde_json::Value {
        let response = state
            .router()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap()
    }

    let body = call(&state, &uri).await;
    let by_model = body["by_model"].as_array().unwrap();
    let ids: Vec<&str> = by_model
        .iter()
        .filter_map(|row| row["id"].as_str())
        .collect();
    assert_eq!(
        ids,
        vec!["claude-opus-4-8", "claude-sonnet-4-5", "gpt-5", "sonnet"],
        "one verbatim row per model id, harness-agnostic: {by_model:#?}"
    );

    let find_model = |body: &serde_json::Value, id: &str| {
        body["by_model"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == id)
            .unwrap()
            .clone()
    };
    let node_of =
        |model: &serde_json::Value| model["efforts"][0]["pipelines"][0]["nodes"][0].clone();

    // Observed main-session model: the Node's peak + the execution's
    // wall-clock, effort « not set » (no effort in the startup event).
    let opus = find_model(&body, "claude-opus-4-8");
    assert_eq!(opus["provenance"], "observed");
    assert_eq!(opus["efforts"][0]["effort"], serde_json::Value::Null);
    assert_eq!(opus["efforts"][0]["name"], "not set");
    assert_eq!(opus["efforts"][0]["pipelines"][0]["id"], "mp1");
    let opus_node = node_of(&opus);
    assert_eq!(opus_node["id"], "worker");
    assert_eq!(opus_node["harnesses"][0]["harness"], "claude");
    assert_eq!(opus_node["harnesses"][0]["context"]["measured"], 1);
    assert_eq!(
        opus_node["harnesses"][0]["context"]["stats"]["max"],
        20000.0
    );
    assert_eq!(
        opus_node["harnesses"][0]["duration"]["stats"]["max"],
        600000.0
    );
    // #811 — fences on the `by_model` tree as well; one observation puts
    // both of them on it.
    assert_eq!(
        opus_node["harnesses"][0]["context"]["stats"]["fence_low"],
        20000.0
    );
    assert_eq!(
        opus_node["harnesses"][0]["context"]["stats"]["fence_high"],
        20000.0
    );
    // The by_model Node row carries no couples — the path IS the drill, so
    // the empty field is omitted on the wire (never an empty array).
    assert!(opus_node.get("models").is_none());

    // The subagent file's own model: its own bucket, its own peak and its
    // own transcript time span as duration.
    let sonnet_observed = find_model(&body, "claude-sonnet-4-5");
    assert_eq!(sonnet_observed["provenance"], "observed");
    let sonnet_node = node_of(&sonnet_observed);
    assert_eq!(sonnet_node["id"], "worker");
    assert_eq!(
        sonnet_node["harnesses"][0]["context"]["stats"]["max"],
        55000.0
    );
    assert_eq!(
        sonnet_node["harnesses"][0]["duration"]["stats"]["max"],
        300000.0
    );

    // A mute source falls back to the requested model × effort, marked.
    let sonnet_requested = find_model(&body, "sonnet");
    assert_eq!(sonnet_requested["provenance"], "requested");
    assert_eq!(sonnet_requested["efforts"][0]["effort"], "high");
    assert_eq!(sonnet_requested["efforts"][0]["provenance"], "requested");
    let requested_node = node_of(&sonnet_requested);
    assert_eq!(requested_node["id"], "worker");
    assert_eq!(requested_node["harnesses"][0]["context"]["measured"], 0);
    assert_eq!(
        requested_node["harnesses"][0]["context"]["missing_reasons"],
        serde_json::json!(["no readable context usage in transcript"])
    );
    assert_eq!(
        requested_node["harnesses"][0]["duration"]["stats"]["max"],
        300000.0
    );

    // copilot: model AND effort observed from the journal; the context peak
    // rides the same journal.
    let gpt5 = find_model(&body, "gpt-5");
    assert_eq!(gpt5["provenance"], "observed");
    assert_eq!(gpt5["efforts"][0]["effort"], "medium");
    assert_eq!(gpt5["efforts"][0]["provenance"], "observed");
    let gpt5_node = node_of(&gpt5);
    assert_eq!(gpt5_node["id"], "solo");
    assert_eq!(
        gpt5_node["harnesses"][0]["context"]["stats"]["max"],
        40000.0
    );
    assert_eq!(
        gpt5_node["harnesses"][0]["duration"]["stats"]["max"],
        600000.0
    );

    // --- by_pipeline is untouched at the top, but its Node leaf now ends
    // with the model × effort couples.
    let worker = body["by_pipeline"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == "mp1")
        .unwrap()["nodes"]
        .as_array()
        .unwrap()[0]
        .clone();
    assert_eq!(worker["harnesses"][0]["context"]["measured"], 1);
    assert_eq!(
        worker["harnesses"][0]["context"]["missing_reasons"],
        serde_json::json!(["no readable context usage in transcript"])
    );
    let couples = worker["models"].as_array().unwrap();
    assert_eq!(couples.len(), 3, "got: {couples:#?}");
    let couple_keys: Vec<String> = couples
        .iter()
        .map(|pair| {
            format!(
                "{}·{}",
                pair["model"].as_str().unwrap(),
                pair["effort"].as_str().unwrap_or("not set")
            )
        })
        .collect();
    assert_eq!(
        couple_keys,
        vec![
            "claude-opus-4-8·not set",
            "claude-sonnet-4-5·not set",
            "sonnet·high"
        ]
    );
    let opus_couple = &couples[0];
    assert_eq!(opus_couple["model_provenance"], "observed");
    assert_eq!(opus_couple["effort_provenance"], serde_json::Value::Null);
    assert_eq!(
        opus_couple["harnesses"][0]["context"]["stats"]["max"],
        20000.0
    );
    let sonnet_high_couple = &couples[2];
    assert_eq!(sonnet_high_couple["model_provenance"], "requested");
    assert_eq!(sonnet_high_couple["effort_provenance"], "requested");
    let sonnet_sub_couple = &couples[1];
    assert_eq!(sonnet_sub_couple["model_provenance"], "observed");
    assert_eq!(
        sonnet_sub_couple["harnesses"][0]["context"]["stats"]["max"],
        55000.0
    );

    // The whole-cohort total pools the main-session observations only —
    // never a subagent, never Infrastructure. Untouched by the axis.
    let total_claude = body["total"]["harnesses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["harness"] == "claude")
        .unwrap();
    assert_eq!(total_claude["context"]["measured"], 1);

    // --- the memo key covers the axis: unchanged state is a hit, a
    // subagent file's rewrite (no event at all) forces a recompute that
    // moves ITS model's bucket.
    let _ = call(&state, &uri).await;
    assert_eq!(
        state.recomputes(from, to),
        1,
        "an unchanged state must be a memo hit"
    );
    std::fs::write(
        subagents_dir.join("explore.jsonl"),
        "{\"type\":\"assistant\",\"timestamp\":\"2026-08-01T09:10:00Z\",\"message\":{\"model\":\"claude-sonnet-4-5\",\"usage\":{\"input_tokens\":66000,\"output_tokens\":0}}}\n\
         {\"type\":\"user\",\"timestamp\":\"2026-08-01T09:15:00Z\"}\n",
    )
    .unwrap();
    let after_rewrite = call(&state, &uri).await;
    assert_eq!(
        state.recomputes(from, to),
        2,
        "rewriting a subagent file, with no new event, must force a recompute"
    );
    let rewritten = find_model(&after_rewrite, "claude-sonnet-4-5");
    assert_eq!(
        node_of(&rewritten)["harnesses"][0]["context"]["stats"]["max"],
        66000.0
    );
}

/// An empty cohort (no Runs in the window) is not an error — an empty
/// Performance payload, distinct from the source-wide-unreadable case below.
/// #792: the Steering metric at the derivation seam — a forged event log
/// plus session files in temporary roots for the three instrumented
/// harnesses, read back through `GET /stats/performance`.
#[tokio::test]
async fn stats_performance_steering_counts_typed_turns_after_launch_per_harness() {
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    let mut state = Harness::new().await;
    let home = state.scratch("test-stats-performance-steering");
    let _cleanup = Cleanup(home.clone());
    std::fs::create_dir_all(&home).unwrap();
    state = state.with_home(home.clone());
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

    let run = "steer-r1";
    insert_event(
        &state.db,
        run,
        "2026-08-01T09:00:00Z",
        "run_started",
        None,
        None,
        serde_json::json!({
            "pipeline_id":"sp","pipeline_name":"Steered pipeline","target_repo":repo,"harness":"claude",
            "node_defs":[
                {"id":"coder","name":"Coder","node_type":"agent","isolated_worktree":true},
                {"id":"pilot","name":"Pilot","node_type":"agent","isolated_worktree":true},
                {"id":"pi-node","name":"Pi node","node_type":"agent","isolated_worktree":true},
                {"id":"ship","name":"Ship","node_type":"agent","isolated_worktree":true}
            ]
        }),
    )
    .await;
    let mut minute = 10;
    let mut events = Vec::new();
    for (node, iter, harness, session) in [
        ("coder", 1, "claude", "c1"),
        ("coder", 2, "claude", "c2"),
        ("coder", 3, "claude", "c3"),
        ("pilot", 1, "copilot", "cp1"),
        ("pi-node", 1, "pi", "pi1"),
        ("ship", 1, "opencode", "oc1"),
    ] {
        let start = format!("2026-08-01T09:{minute:02}:00Z");
        let end = format!("2026-08-01T09:{:02}:00Z", minute + 1);
        minute += 2;
        let payload = serde_json::json!({
            "node_type":"agent","isolated_worktree":true,"harness":harness,"session_id":session
        });
        events.push((node, iter, start, end, payload));
    }
    for (node, iter, start, end, payload) in &events {
        insert_event(
            &state.db,
            run,
            start,
            "node_started",
            Some(node),
            Some(*iter),
            payload.clone(),
        )
        .await;
        insert_event(
            &state.db,
            run,
            end,
            "node_completed",
            Some(node),
            Some(*iter),
            serde_json::json!({}),
        )
        .await;
    }
    insert_event(
        &state.db,
        run,
        "2026-08-01T09:40:00Z",
        "run_completed",
        None,
        None,
        serde_json::json!({}),
    )
    .await;

    // --- claude: c1 = launch + 2 human turns + 1 runtime turn + a tool
    // result, a skill load (isMeta) and a compaction summary — count 2. Its
    // subagent file carries human-looking turns that must never count. c2 =
    // launch only — count 0 (readable, autonomous). c3 = assistant lines only
    // — unreadable, never 0.
    let claude_root = home.join(".claude").join("projects");
    let assistant = r#"{"type":"assistant","message":{"model":"claude-opus-4-8","usage":{"input_tokens":1000,"output_tokens":0}}}"#;
    let user = |text: &str| {
        format!(
            r#"{{"type":"user","message":{{"role":"user","content":{}}}}}"#,
            serde_json::Value::from(text)
        )
    };
    let claude_dir = |iter: i64| {
        let dir = claude_root.join(pdo_harness::claude_transcript::encode_working_dir(
            &pdo_workspace::domain::worktree_layout::sub_worktree_path(
                &state.repo_root,
                run,
                "coder",
                iter,
            ),
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    };
    let c1_dir = claude_dir(1);
    std::fs::write(
        c1_dir.join("c1.jsonl"),
        [
            user("launch prompt").as_str(),
            assistant,
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t","content":"x"}]},"toolUseResult":{"stdout":"x"}}"#,
            r#"{"type":"user","isMeta":true,"message":{"role":"user","content":[{"type":"text","text":"Base directory for this skill: /s"}]}}"#,
            user("first steer").as_str(),
            assistant,
            user("[pdo-runtime] your output is missing frontmatter").as_str(),
            r#"{"type":"user","isCompactSummary":true,"message":{"role":"user","content":"This session is being continued"}}"#,
            user("second steer").as_str(),
            assistant,
        ]
        .join("\n"),
    )
    .unwrap();
    let sub_dir = c1_dir.join("c1").join("subagents");
    std::fs::create_dir_all(&sub_dir).unwrap();
    std::fs::write(
        sub_dir.join("explore.jsonl"),
        [
            user("subagent prompt").as_str(),
            assistant,
            user("looks like a steer but nobody typed it").as_str(),
        ]
        .join("\n"),
    )
    .unwrap();
    std::fs::write(
        claude_dir(2).join("c2.jsonl"),
        [user("launch prompt").as_str(), assistant].join("\n"),
    )
    .unwrap();
    std::fs::write(claude_dir(3).join("c3.jsonl"), assistant).unwrap();

    // --- copilot: launch + runtime + one human turn — count 1.
    let copilot_dir = home.join(".copilot").join("session-state").join("cp1");
    std::fs::create_dir_all(&copilot_dir).unwrap();
    std::fs::write(
        copilot_dir.join("events.jsonl"),
        [
            r#"{"type":"session.start","data":{"selectedModel":"gpt-5.6-sol","reasoningEffort":"medium"}}"#,
            r#"{"type":"user.message","data":{"content":"launch prompt"}}"#,
            r#"{"type":"user.message","data":{"content":"[pdo-runtime] retry"}}"#,
            r#"{"type":"user.message","data":{"content":"human steer"}}"#,
            r#"{"type":"session.shutdown","data":{"usage":{"inputTokens":4000,"outputTokens":0},"totalNanoAiu":1000000000}}"#,
        ]
        .join("\n"),
    )
    .unwrap();

    // --- pi: launch + two human turns — count 2.
    let pi_dir = home.join(".pi").join("agent").join("sessions").join(
        pdo_harness::pi_session::session_dir_name(
            &pdo_workspace::domain::worktree_layout::sub_worktree_path(
                &state.repo_root,
                run,
                "pi-node",
                1,
            ),
        ),
    );
    std::fs::create_dir_all(&pi_dir).unwrap();
    std::fs::write(
        pi_dir.join("2026-08-01T09-18-00-000Z_pi1.jsonl"),
        [
            r#"{"type":"session","id":"pi1"}"#,
            r#"{"type":"message","id":"a","message":{"role":"user","content":[{"type":"text","text":"launch prompt"}]}}"#,
            r#"{"type":"message","id":"b","message":{"role":"assistant","model":"claude-sonnet-4-5","provider":"anthropic","content":[{"type":"text","text":"ok"}],"usage":{"totalTokens":3000,"cost":{"total":0.01}}}}"#,
            r#"{"type":"message","id":"c","message":{"role":"user","content":"steer one"}}"#,
            r#"{"type":"message","id":"d","message":{"role":"user","content":"steer two"}}"#,
        ]
        .join("\n"),
    )
    .unwrap();

    let response = state
        .router()
        .oneshot(
            Request::builder()
                .uri("/stats/performance?from=2026-08-01T00:00:00Z&to=2026-08-02T00:00:00Z")
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

    let harness = |row: &serde_json::Value, name: &str| -> serde_json::Value {
        row["harnesses"]
            .as_array()
            .unwrap()
            .iter()
            .find(|h| h["harness"] == name)
            .unwrap_or_else(|| panic!("no {name} in {row:#?}"))
            .clone()
    };
    let pipeline = &body["by_pipeline"][0];
    assert_eq!(pipeline["id"], "sp");
    let node = |id: &str| -> serde_json::Value {
        pipeline["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["id"] == id)
            .unwrap()
            .clone()
    };

    // claude: two readable counts (2, 0) out of three executions, one steered.
    let coder = harness(&node("coder"), "claude");
    assert_eq!(coder["steering"]["measured"], 2, "{coder:#?}");
    assert_eq!(coder["steering"]["expected"], 3);
    assert_eq!(coder["steering"]["stats"]["max"], 2.0);
    assert_eq!(coder["steering"]["stats"]["min"], 0.0);
    assert_eq!(coder["steering"]["stats"]["mean"], 1.0);
    assert_eq!(
        coder["steering"]["missing_reasons"],
        serde_json::json!(["no readable human turn in transcript"])
    );
    assert_eq!(
        coder["steered"],
        serde_json::json!({"steered":1,"readable":2})
    );
    // The additive contract: the two existing metrics are untouched.
    assert_eq!(coder["context"]["measured"], 3);
    assert_eq!(coder["duration"]["measured"], 3);

    // The subagent is never steered: a declared absence, no expected observation.
    let explore = harness(&node("coder")["subagents"][0], "claude");
    assert_eq!(explore["steering"]["expected"], 0);
    assert_eq!(explore["steering"]["measured"], 0);
    assert_eq!(
        explore["steering"]["missing_reasons"],
        serde_json::json!(["subagents are never steered"])
    );
    assert_eq!(
        explore["steered"],
        serde_json::json!({"steered":0,"readable":0})
    );

    // copilot / pi: their own formats.
    let pilot = harness(&node("pilot"), "copilot");
    assert_eq!(pilot["steering"]["stats"]["max"], 1.0, "{pilot:#?}");
    assert_eq!(
        pilot["steered"],
        serde_json::json!({"steered":1,"readable":1})
    );
    let pi_node = harness(&node("pi-node"), "pi");
    assert_eq!(pi_node["steering"]["stats"]["max"], 2.0, "{pi_node:#?}");
    assert_eq!(
        pi_node["steered"],
        serde_json::json!({"steered":1,"readable":1})
    );

    // opencode: unavailable with a named reason, never 0.
    let ship = harness(&node("ship"), "opencode");
    assert_eq!(ship["steering"]["measured"], 0);
    assert_eq!(ship["steering"]["expected"], 1);
    assert!(ship["steering"]["stats"].is_null());
    assert_eq!(
        ship["steering"]["missing_reasons"],
        serde_json::json!(["session not resolvable on opencode"])
    );
    assert_eq!(
        ship["steered"],
        serde_json::json!({"steered":0,"readable":0})
    );

    // The cohort total pools the raw counts: 2, 0 (claude) — per harness.
    let total_claude = harness(&body["total"], "claude");
    assert_eq!(
        total_claude["steered"],
        serde_json::json!({"steered":1,"readable":2})
    );

    // Infrastructure: « runtime messages only », never 0.
    let pm = &body["infrastructure"][0];
    assert_eq!(pm["id"], "pipeline-manager");
    let pm_claude = harness(pm, "claude");
    assert_eq!(pm_claude["steering"]["expected"], 1);
    assert_eq!(pm_claude["steering"]["measured"], 0);
    assert_eq!(
        pm_claude["steering"]["missing_reasons"],
        serde_json::json!(["runtime messages only"])
    );
    let infra_total = harness(&body["infrastructure_total"], "claude");
    assert_eq!(
        infra_total["steered"],
        serde_json::json!({"steered":0,"readable":0})
    );

    // The « By model » axis carries the metric too, down to the Node row,
    // and the Node's own model × effort couples on the by_pipeline side.
    let by_model = body["by_model"].as_array().unwrap();
    let opus = by_model
        .iter()
        .find(|m| m["id"] == "claude-opus-4-8")
        .unwrap_or_else(|| panic!("{by_model:#?}"));
    let opus_claude = harness(opus, "claude");
    assert_eq!(
        opus_claude["steered"],
        serde_json::json!({"steered":1,"readable":2})
    );
    let opus_node = &opus["efforts"][0]["pipelines"][0]["nodes"][0];
    assert_eq!(opus_node["id"], "coder");
    assert_eq!(harness(opus_node, "claude")["steering"]["measured"], 2);
    let couples = node("coder")["models"].as_array().unwrap().clone();
    let opus_couple = couples
        .iter()
        .find(|c| c["model"] == "claude-opus-4-8")
        .unwrap();
    assert_eq!(
        harness(opus_couple, "claude")["steered"],
        serde_json::json!({"steered":1,"readable":2})
    );
    let sonnet = by_model
        .iter()
        .find(|m| m["id"] == "claude-sonnet-4-5")
        .unwrap();
    assert_eq!(
        harness(sonnet, "pi")["steered"],
        serde_json::json!({"steered":1,"readable":1})
    );
}

/// `GET /stats/performance` (#810, story #808) — the **durée active** and
/// the **genre de nœud**, read at the HTTP seam over a forged event log.
///
/// One Run exercises every wait rule at once: a `declared` wait lifted by a
/// `NodeResumed`, a `completion_not_released` wait lifted by a
/// `NodeCompletionReleased`, a `children_pending` marker that opens NOTHING
/// (an orchestrator on `pdo run wait` is working), a `NodeAwaitingUser`
/// with no cause at all (ignored — defensive, for pre-ADR-0069 history), a
/// wait closed by the very terminal that records the observation, and the
/// Infrastructure row subtracting the sum of its Nodes' waits. The same
/// payloads carry the genre: frozen on `NodeStarted`, or read off the Run's
/// pipeline snapshot when the startup event predates the freeze.
#[tokio::test]
async fn stats_performance_subtracts_declared_waits_and_carries_the_node_kind() {
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

    // `orch`'s snapshot says orchestrator; its `node_started` will NOT, so
    // the fold must fall back to the snapshot (pre-#810 history).
    insert_event(
        &state.db,
        "wait-r1",
        "2033-03-10T10:00:00Z",
        "run_started",
        None,
        None,
        serde_json::json!({
            "pipeline_id":"wp1","pipeline_name":"Wait Pipeline","target_repo":repo,"harness":"claude",
            "node_defs":[
                {"id":"chat","name":"chat","node_type":"agent","isolated_worktree":true},
                {"id":"orch","name":"orch","node_type":"agent","isolated_worktree":true,"orchestrator":true},
                {"id":"plain","name":"plain","node_type":"agent","isolated_worktree":true},
                {"id":"late","name":"late","node_type":"agent","isolated_worktree":true}
            ]
        }),
    )
    .await;

    // `chat` — interactive, two waits of two different causes, both lifted.
    // 10 min wall-clock, 3 min waited → 7 min active.
    for (ts, kind, payload) in [
        (
            "2033-03-10T10:00:00Z",
            "node_started",
            serde_json::json!({"node_type":"agent","isolated_worktree":true,"harness":"claude","interactive":true,"orchestrator":false,"model":"claude-opus-5"}),
        ),
        (
            "2033-03-10T10:01:00Z",
            "node_awaiting_user",
            serde_json::json!({"cause":"declared","message":"Strip or card?"}),
        ),
        (
            "2033-03-10T10:03:00Z",
            "node_resumed",
            serde_json::json!({}),
        ),
        (
            "2033-03-10T10:04:00Z",
            "node_awaiting_user",
            serde_json::json!({"cause":"completion_not_released"}),
        ),
        (
            "2033-03-10T10:05:00Z",
            "node_completion_released",
            serde_json::json!({}),
        ),
        (
            "2033-03-10T10:10:00Z",
            "node_completed",
            serde_json::json!({}),
        ),
    ] {
        insert_event(
            &state.db,
            "wait-r1",
            ts,
            kind,
            Some("chat"),
            Some(1),
            payload,
        )
        .await;
    }

    // `orch` — the binding marker (`reason: children_pending`) opens no wait.
    // 6 min wall-clock, 6 min active. No `orchestrator` in the payload.
    for (ts, kind, payload) in [
        (
            "2033-03-10T10:00:00Z",
            "node_started",
            serde_json::json!({"node_type":"agent","isolated_worktree":true,"harness":"claude"}),
        ),
        (
            "2033-03-10T10:02:00Z",
            "node_awaiting_user",
            serde_json::json!({"reason":"children_pending","active":["c1"],"failed":[]}),
        ),
        (
            "2033-03-10T10:06:00Z",
            "node_completed",
            serde_json::json!({}),
        ),
    ] {
        insert_event(
            &state.db,
            "wait-r1",
            ts,
            kind,
            Some("orch"),
            Some(1),
            payload,
        )
        .await;
    }

    // `plain` — a `NodeAwaitingUser` with no cause at all is ignored.
    // 4 min wall-clock, 4 min active.
    for (ts, kind, payload) in [
        (
            "2033-03-10T10:00:00Z",
            "node_started",
            serde_json::json!({"node_type":"agent","isolated_worktree":true,"harness":"claude"}),
        ),
        (
            "2033-03-10T10:01:00Z",
            "node_awaiting_user",
            serde_json::json!({}),
        ),
        (
            "2033-03-10T10:04:00Z",
            "node_completed",
            serde_json::json!({}),
        ),
    ] {
        insert_event(
            &state.db,
            "wait-r1",
            ts,
            kind,
            Some("plain"),
            Some(1),
            payload,
        )
        .await;
    }

    // `late` — still awaiting when it completes: the terminal closes the
    // wait. 12 min wall-clock, 10 min waited → 2 min active.
    for (ts, kind, payload) in [
        (
            "2033-03-10T10:00:00Z",
            "node_started",
            serde_json::json!({"node_type":"agent","isolated_worktree":true,"harness":"claude"}),
        ),
        (
            "2033-03-10T10:02:00Z",
            "node_awaiting_user",
            serde_json::json!({"cause":"declared"}),
        ),
        (
            "2033-03-10T10:12:00Z",
            "node_completed",
            serde_json::json!({}),
        ),
    ] {
        insert_event(
            &state.db,
            "wait-r1",
            ts,
            kind,
            Some("late"),
            Some(1),
            payload,
        )
        .await;
    }

    // Infrastructure: 20 min of Run, 3 + 10 min of declared wait → 7 min.
    insert_event(
        &state.db,
        "wait-r1",
        "2033-03-10T10:20:00Z",
        "run_completed",
        None,
        None,
        serde_json::json!({}),
    )
    .await;

    // A second Run of the same pipeline that FAILS: its successful node is
    // an observation for the default cohort and vanishes under
    // `completed_only`.
    insert_event(
        &state.db,
        "wait-r2",
        "2033-03-10T11:00:00Z",
        "run_started",
        None,
        None,
        serde_json::json!({
            "pipeline_id":"wp2","pipeline_name":"Doomed Pipeline","target_repo":repo,"harness":"claude",
            "node_defs":[{"id":"doomed","name":"doomed","node_type":"agent","isolated_worktree":true}]
        }),
    )
    .await;
    insert_event(
        &state.db,
        "wait-r2",
        "2033-03-10T11:00:00Z",
        "node_started",
        Some("doomed"),
        Some(1),
        serde_json::json!({"node_type":"agent","isolated_worktree":true,"harness":"claude"}),
    )
    .await;
    insert_event(
        &state.db,
        "wait-r2",
        "2033-03-10T11:05:00Z",
        "node_completed",
        Some("doomed"),
        Some(1),
        serde_json::json!({}),
    )
    .await;
    insert_event(
        &state.db,
        "wait-r2",
        "2033-03-10T11:10:00Z",
        "run_failed",
        None,
        None,
        serde_json::json!({}),
    )
    .await;

    async fn call(state: &Harness, uri: &str) -> serde_json::Value {
        let response = state
            .router()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap()
    }

    fn node<'a>(body: &'a serde_json::Value, pipeline: &str, id: &str) -> &'a serde_json::Value {
        body["by_pipeline"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["id"] == pipeline)
            .unwrap_or_else(|| panic!("pipeline {pipeline} missing from {body}"))["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["id"] == id)
            .unwrap_or_else(|| panic!("node {id} missing"))
    }

    fn claude_metric<'a>(entity: &'a serde_json::Value, metric: &str) -> &'a serde_json::Value {
        &entity["harnesses"]
            .as_array()
            .unwrap()
            .iter()
            .find(|h| h["harness"] == "claude")
            .unwrap()[metric]
    }

    let from = "2033-03-10T00:00:00Z";
    let to = "2033-03-11T00:00:00Z";
    let body = call(&state, &format!("/stats/performance?from={from}&to={to}")).await;

    const MIN: f64 = 60_000.0;
    for (id, wall, active, waiting) in [
        ("chat", 10.0, 7.0, 3.0),
        ("orch", 6.0, 6.0, 0.0),
        ("plain", 4.0, 4.0, 0.0),
        ("late", 12.0, 2.0, 10.0),
    ] {
        let row = node(&body, "wp1", id);
        assert_eq!(
            claude_metric(row, "duration")["stats"]["median"],
            wall * MIN,
            "{id} wall-clock"
        );
        assert_eq!(
            claude_metric(row, "active_duration")["stats"]["median"],
            active * MIN,
            "{id} active duration"
        );
        // #819 — the third reading: the declared wait alone. A node nobody
        // waited on measures a real `0`, never an absence.
        assert_eq!(
            claude_metric(row, "wait_duration")["stats"]["median"],
            waiting * MIN,
            "{id} waiting"
        );
        assert_eq!(
            active * MIN + waiting * MIN,
            wall * MIN,
            "{id}: Total = Active + Waiting"
        );
        // Coverage never disagrees between the three readings: the mode
        // switch must not change `n=`.
        for metric in ["active_duration", "wait_duration"] {
            assert_eq!(
                claude_metric(row, "duration")["measured"],
                claude_metric(row, metric)["measured"],
                "{id} {metric} coverage"
            );
        }
    }

    // #819 — the wait coverage the « i » reads: the five Node executions of
    // the default cohort (four here plus the failed Run's `doomed`), two of
    // which declared a wait. Infrastructure is not an execution.
    assert_eq!(body["executions"], 5);
    assert_eq!(body["waited_executions"], 2);

    // The genre travels on every Node row: frozen in the payload for
    // `chat`, read off the Run's snapshot for `orch`.
    assert_eq!(node(&body, "wp1", "chat")["interactive"], true);
    assert_eq!(node(&body, "wp1", "chat")["orchestrator"], false);
    assert_eq!(node(&body, "wp1", "orch")["orchestrator"], true);
    assert_eq!(node(&body, "wp1", "orch")["interactive"], false);
    assert_eq!(node(&body, "wp1", "plain")["interactive"], false);
    assert_eq!(node(&body, "wp1", "plain")["orchestrator"], false);
    // A Pipeline row is not a Node: it carries no kind at all.
    assert!(
        body["by_pipeline"][0].get("interactive").is_none(),
        "a Pipeline row carries no kind: {}",
        body["by_pipeline"][0]
    );

    // The « By model » axis carries the same genre on its Node leaves.
    let model_node = &body["by_model"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["id"] == "claude-opus-5")
        .unwrap()["efforts"][0]["pipelines"][0]["nodes"][0];
    assert_eq!(model_node["id"], "chat");
    assert_eq!(model_node["interactive"], true);
    assert_eq!(
        claude_metric(model_node, "active_duration")["stats"]["median"],
        7.0 * MIN
    );

    // Infrastructure: the Run's own span minus the sum of its Nodes' waits.
    let manager = body["infrastructure"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == "pipeline-manager")
        .unwrap();
    assert_eq!(
        claude_metric(manager, "duration")["stats"]["median"],
        20.0 * MIN
    );
    assert_eq!(
        claude_metric(manager, "active_duration")["stats"]["median"],
        7.0 * MIN
    );
    // #819 — and its Waiting is that same sum, so the three modes reconcile
    // at the Infrastructure level too.
    assert_eq!(
        claude_metric(manager, "wait_duration")["stats"]["median"],
        13.0 * MIN
    );
    assert!(
        body["by_pipeline"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["id"] == "wp2"),
        "the failed Run's successful execution counts in the default cohort"
    );

    // --- « Runs terminés seulement » --------------------------------------
    let before = state.recomputes(from, to);
    let narrowed = call(
        &state,
        &format!("/stats/performance?from={from}&to={to}&completed_only=true"),
    )
    .await;
    assert_eq!(
        state.recomputes(from, to),
        before + 1,
        "the cohort switch is part of the memo key: it must not serve the other cohort"
    );
    assert!(
        !narrowed["by_pipeline"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["id"] == "wp2"),
        "a Run without a run_completed leaves the cohort whole: {narrowed}"
    );
    assert_eq!(
        claude_metric(node(&narrowed, "wp1", "chat"), "active_duration")["stats"]["median"],
        7.0 * MIN,
        "the completed Run is untouched by the cohort switch"
    );
    // The coverage follows the cohort it describes: `doomed` left with its
    // Run, so the denominator drops to the four executions on screen.
    assert_eq!(narrowed["executions"], 4);
    assert_eq!(narrowed["waited_executions"], 2);
    // A repeat of the narrowed request is served from ITS memo entry.
    let repeat = call(
        &state,
        &format!("/stats/performance?from={from}&to={to}&completed_only=true"),
    )
    .await;
    assert_eq!(
        state.recomputes(from, to),
        before + 1,
        "each cohort keeps its own memo entry"
    );
    assert_eq!(narrowed, repeat);
}

#[tokio::test]
async fn stats_performance_empty_cohort_is_not_an_error() {
    let state = Harness::new().await;
    let response = state
        .router()
        .oneshot(
            Request::builder()
                .uri("/stats/performance?from=2026-01-01T00:00:00Z&to=2026-01-02T00:00:00Z")
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
    assert_eq!(body["harnesses"], serde_json::json!([]));
    assert_eq!(body["by_pipeline"], serde_json::json!([]));
    assert_eq!(body["infrastructure"], serde_json::json!([]));
    assert_eq!(body["total"]["harnesses"], serde_json::json!([]));
}

/// A harness's whole transcript source existing but unreadable (permissions,
/// a corrupted mount) is a visible endpoint error — distinct from a single
/// unreadable session, which just degrades to a local absence reason.
#[tokio::test]
async fn stats_performance_source_wide_unreadable_is_a_visible_error() {
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::set_permissions(
                &self.0,
                std::os::unix::fs::PermissionsExt::from_mode(0o755),
            );
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    let mut state = Harness::new().await;
    let home = state.scratch("test-stats-performance-unreadable");
    std::fs::create_dir_all(&home).unwrap();
    state = state.with_home(home.clone());
    let repo = state.repo_root.to_string_lossy().into_owned();

    sqlx::query(
        "INSERT INTO events (run_id, ts, kind, node_id, iter, payload) VALUES \
         ('perf-unreadable', '2026-07-15T09:00:00Z', 'run_started', NULL, NULL, ?)",
    )
    .bind(
        serde_json::json!({
            "pipeline_id":"p1","pipeline_name":"Loop","target_repo":repo,"harness":"claude",
            "node_defs":[{"id":"worker","name":"Worker","node_type":"agent", "isolated_worktree":true}]
        })
        .to_string(),
    )
    .execute(&state.db)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO events (run_id, ts, kind, node_id, iter, payload) VALUES \
         ('perf-unreadable', '2026-07-15T09:10:00Z', 'node_started', 'worker', 1, ?)",
    )
    .bind(
        serde_json::json!({"node_type":"agent", "isolated_worktree":true,"harness":"claude","session_id":"claude-1"})
            .to_string(),
    )
    .execute(&state.db)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO events (run_id, ts, kind, node_id, iter, payload) VALUES \
         ('perf-unreadable', '2026-07-15T09:20:00Z', 'node_completed', 'worker', 1, '{}')",
    )
    .execute(&state.db)
    .await
    .unwrap();

    let claude_root = home.join(".claude").join("projects");
    std::fs::create_dir_all(&claude_root).unwrap();
    std::fs::set_permissions(
        &claude_root,
        std::os::unix::fs::PermissionsExt::from_mode(0o000),
    )
    .unwrap();
    let _cleanup = Cleanup(claude_root.clone());

    let response = state
        .router()
        .oneshot(
            Request::builder()
                .uri("/stats/performance?from=2026-07-15T00:00:00Z&to=2026-07-16T00:00:00Z")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    let message = body["error"].as_str().unwrap_or_default();
    assert!(
        message.contains("Claude"),
        "the error must name the harness/source: {message}"
    );
}
