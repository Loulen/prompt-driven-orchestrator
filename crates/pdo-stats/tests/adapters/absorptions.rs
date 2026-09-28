//! The absorptions over the real store and event log: Combine, Uncombine, the
//! rename-born absorption, and how the three sections read them (moved from
//! the legacy crate's in-crate tests, #928).

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use crate::common::Harness;

/// #890 / ADR-0077 — the HTTP contract of an absorption: posed through the
/// API, it folds Pipeline A into Pipeline B in Sessions, Cost (all three
/// trees), Performance (both trees) and the Triggers fires; `uncombined=true`
/// reads the raw rows; the Performance memo recomputes on every write; and
/// removing the last member brings the original rows back.
#[tokio::test]
async fn stats_absorption_folds_pipelines_in_every_tab_and_undoes_cleanly() {
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    let mut state = Harness::new().await;
    let home = state.scratch("test-stats-absorption");
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

    // Two versions of one Pipeline: A (`alpha-old`, older) and B
    // (`alpha-new`). Both have a `worker` Node (same id → one Node row under
    // the absorbent); B's worker ran on a model nobody prices (a lower
    // bound, `partial`), and B has a Node on a harness with no cost source.
    let runs = [
        (
            "abs-a1",
            "alpha-old",
            "Alpha",
            "2032-03-15",
            "claude-opus-4-8",
            10,
        ),
        (
            "abs-b1",
            "alpha-new",
            "Alpha",
            "2032-03-16",
            "claude-unpriced-9",
            20,
        ),
    ];
    for (run, pipeline_id, name, day, model, minutes) in runs {
        insert_event(
            &state.db,
            run,
            &format!("{day}T09:00:00Z"),
            "run_started",
            None,
            None,
            serde_json::json!({
                "pipeline_id": pipeline_id,
                "pipeline_name": name,
                "target_repo": repo,
                "harness": "claude",
                "node_defs": [
                    {"id":"worker","name":"Worker","node_type":"agent","isolated_worktree":true},
                    {"id":"extra","name":"Extra","node_type":"agent","isolated_worktree":true}
                ]
            }),
        )
        .await;
        insert_event(
            &state.db,
            run,
            &format!("{day}T09:01:00Z"),
            "node_started",
            Some("worker"),
            Some(1),
            serde_json::json!({"node_type":"agent","isolated_worktree":true,"harness":"claude","session_id":format!("{run}-s")}),
        )
        .await;
        insert_event(
            &state.db,
            run,
            &format!("{day}T09:{:02}:00Z", 1 + minutes),
            "node_completed",
            Some("worker"),
            Some(1),
            serde_json::json!({}),
        )
        .await;
        insert_event(
            &state.db,
            run,
            &format!("{day}T09:40:00Z"),
            "run_completed",
            None,
            None,
            serde_json::json!({}),
        )
        .await;
        let dir = home.join(".claude").join("projects").join(
            pdo_harness::claude_transcript::encode_working_dir(
                &pdo_workspace::domain::worktree_layout::sub_worktree_path(
                    &state.repo_root,
                    run,
                    "worker",
                    1,
                ),
            ),
        );
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(format!("{run}-s.jsonl")),
            format!(
                r#"{{"type":"assistant","requestId":"{run}","message":{{"id":"{run}-m","model":"{model}","usage":{{"input_tokens":1000000,"output_tokens":0}}}}}}"#
            ),
        )
        .unwrap();
    }
    insert_event(
        &state.db,
        "abs-b1",
        "2032-03-16T09:30:00Z",
        "node_started",
        Some("extra"),
        Some(1),
        serde_json::json!({"node_type":"agent","isolated_worktree":true,"harness":"future-harness","session_id":"abs-b1-extra"}),
    )
    .await;
    // Trigger fires: 2 for A, 1 for B.
    for (id, pipeline) in [("abs-ta", "alpha-old"), ("abs-tb", "alpha-new")] {
        sqlx::query(
            "INSERT INTO triggers (id, name, pipeline_id, cron, enabled, created_at) \
             VALUES (?, ?, ?, '0 2 * * *', 1, '2032-03-01T00:00:00Z')",
        )
        .bind(id)
        .bind(id)
        .bind(pipeline)
        .execute(&state.db)
        .await
        .unwrap();
    }
    for (trigger, ts) in [
        ("abs-ta", "2032-03-15T02:00:00Z"),
        ("abs-ta", "2032-03-16T02:00:00Z"),
        ("abs-tb", "2032-03-16T03:00:00Z"),
    ] {
        sqlx::query("INSERT INTO trigger_fires (trigger_id, ts, outcome) VALUES (?, ?, 'fired')")
            .bind(trigger)
            .bind(ts)
            .execute(&state.db)
            .await
            .unwrap();
    }

    async fn send(
        state: &Harness,
        method: &str,
        uri: &str,
        body: Option<serde_json::Value>,
    ) -> (StatusCode, serde_json::Value) {
        let mut request = Request::builder().method(method).uri(uri);
        if body.is_some() {
            request = request.header("content-type", "application/json");
        }
        let response = state
            .router()
            .oneshot(
                request
                    .body(body.map_or_else(Body::empty, |b| Body::from(b.to_string())))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
        )
    }
    let window = "from=2032-03-15T00:00:00Z&to=2032-03-17T00:00:00Z";
    let (from, to) = ("2032-03-15T00:00:00Z", "2032-03-17T00:00:00Z");
    let overview_uri = format!("/stats/overview?{window}&bucket=day");
    let cost_uri = format!("/stats/cost?{window}&bucket=day");
    let performance_uri = format!("/stats/performance?{window}");
    let ids = |rows: &serde_json::Value| -> Vec<String> {
        let mut ids: Vec<String> = rows
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["id"].as_str().unwrap().to_string())
            .collect();
        ids.sort();
        ids
    };
    let find = |rows: &serde_json::Value, id: &str| -> serde_json::Value {
        rows.as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == id)
            .cloned()
            .unwrap_or_else(|| panic!("no row {id} in {rows:#}"))
    };

    // --- Before: two Pipeline rows everywhere, and the raw costs to add up.
    let (_, overview) = send(&state, "GET", &overview_uri, None).await;
    assert_eq!(
        ids(&overview["sessions_by_pipeline"]),
        vec!["alpha-new", "alpha-old"]
    );
    let (_, raw_cost) = send(&state, "GET", &cost_uri, None).await;
    let usd_old = find(&raw_cost["by_pipeline"], "alpha-old")["usd"]
        .as_f64()
        .unwrap();
    let usd_new = find(&raw_cost["by_pipeline"], "alpha-new")["usd"]
        .as_f64()
        .unwrap_or(0.0);
    assert!(!find(&raw_cost["by_pipeline"], "alpha-old")["partial"]
        .as_bool()
        .unwrap());
    let (_, performance_before) = send(&state, "GET", &performance_uri, None).await;
    assert_eq!(
        ids(&performance_before["by_pipeline"]),
        vec!["alpha-new", "alpha-old"]
    );
    let recomputes = state.recomputes(from, to);

    // --- Combine: B absorbs A.
    let (status, list) = send(
        &state,
        "POST",
        "/stats/absorptions",
        Some(serde_json::json!({
            "dimension": "pipeline",
            "absorbent": {"key": "alpha-new", "name": "Alpha"},
            "members": [{"key": "alpha-old", "name": "Alpha"}],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(list["absorptions"][0]["absorbent"]["key"], "alpha-new");
    assert_eq!(list["absorptions"][0]["members"][0]["key"], "alpha-old");
    assert_eq!(list["absorptions"][0]["members"][0]["origin"], "manual");

    // Sessions: one row, cumulated executions, one `worker` Node, members.
    let (_, overview) = send(&state, "GET", &overview_uri, None).await;
    assert_eq!(ids(&overview["sessions_by_pipeline"]), vec!["alpha-new"]);
    let row = find(&overview["sessions_by_pipeline"], "alpha-new");
    assert_eq!(row["name"], "Alpha");
    assert_eq!(row["executions"], 3);
    assert_eq!(row["runs"], 2);
    assert_eq!(row["last_run"], "2032-03-16T09:00:00Z");
    assert_eq!(find(&row["nodes"], "worker")["executions"], 2);
    assert_eq!(row["absorbed"][0]["key"], "alpha-old");
    assert_eq!(row["absorbed"][0]["runs"], 1);
    assert_eq!(row["absorbed"][0]["executions"], 1);
    assert_eq!(row["absorbed"][0]["last_run"], "2032-03-15T09:00:00Z");
    // Triggers: the fires follow the absorption.
    assert_eq!(
        overview["fires_by_pipeline"],
        serde_json::json!([{"pipeline_id": "alpha-new", "name": "Alpha", "count": 3}])
    );

    // Cost: one row per tree, the costs added up, the honesty marks kept.
    let (_, cost) = send(&state, "GET", &cost_uri, None).await;
    assert_eq!(ids(&cost["by_pipeline"]), vec!["alpha-new"]);
    let pipeline = find(&cost["by_pipeline"], "alpha-new");
    assert_eq!(pipeline["executions"], 2);
    assert_eq!(pipeline["usd"].as_f64().unwrap(), usd_old + usd_new);
    assert_eq!(pipeline["partial"], true, "B's lower bound shows through");
    assert_eq!(pipeline["absorbed"][0]["key"], "alpha-old");
    assert_eq!(find(&pipeline["nodes"], "worker")["executions"], 2);
    let infrastructure: Vec<_> = pipeline["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|node| node["name"] == "Infrastructure")
        .collect();
    assert_eq!(
        infrastructure.len(),
        1,
        "one Infrastructure row per absorbent"
    );
    let extra = find(&pipeline["nodes"], "extra");
    assert!(
        extra["usd"].is_null(),
        "an unknown cost stays unknown: {extra}"
    );
    let project_pipelines = &cost["by_project"][0]["pipelines"];
    assert_eq!(ids(project_pipelines), vec!["alpha-new"]);
    for model in cost["by_model"].as_array().unwrap() {
        for effort in model["efforts"].as_array().unwrap() {
            assert_eq!(ids(&effort["pipelines"]), vec!["alpha-new"]);
        }
    }

    // Performance: one row per tree, the medians recomputed over both Runs.
    let (_, performance) = send(&state, "GET", &performance_uri, None).await;
    assert_eq!(
        state.recomputes(from, to),
        recomputes + 1,
        "a Combine must recompute the Performance memo"
    );
    assert_eq!(ids(&performance["by_pipeline"]), vec!["alpha-new"]);
    let pipeline = find(&performance["by_pipeline"], "alpha-new");
    assert_eq!(pipeline["absorbed"][0]["key"], "alpha-old");
    let claude = pipeline["harnesses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["harness"] == "claude")
        .unwrap()
        .clone();
    assert_eq!(claude["duration"]["measured"], 2);
    assert_eq!(claude["duration"]["stats"]["median"], 900000.0);
    assert_eq!(
        find(&pipeline["nodes"], "worker")["harnesses"][0]["duration"]["measured"],
        2
    );
    for model in performance["by_model"].as_array().unwrap() {
        for effort in model["efforts"].as_array().unwrap() {
            assert_eq!(ids(&effort["pipelines"]), vec!["alpha-new"]);
        }
    }

    // Uncombined: the raw rows, absorption kept.
    let (_, overview) = send(
        &state,
        "GET",
        &format!("{overview_uri}&uncombined=true"),
        None,
    )
    .await;
    assert_eq!(
        ids(&overview["sessions_by_pipeline"]),
        vec!["alpha-new", "alpha-old"]
    );
    assert!(find(&overview["sessions_by_pipeline"], "alpha-new")["absorbed"].is_null());
    assert_eq!(overview["fires_by_pipeline"].as_array().unwrap().len(), 2);
    let (_, cost) = send(&state, "GET", &format!("{cost_uri}&uncombined=true"), None).await;
    assert_eq!(ids(&cost["by_pipeline"]), vec!["alpha-new", "alpha-old"]);
    let (_, performance) = send(
        &state,
        "GET",
        &format!("{performance_uri}&uncombined=true"),
        None,
    )
    .await;
    assert_eq!(
        ids(&performance["by_pipeline"]),
        vec!["alpha-new", "alpha-old"]
    );

    // Uncombine the last member: the absorption is gone, the rows are back.
    // The store is then exactly what it was before the Combine, so the
    // Performance memo answers from the entry that state computed — never
    // from the combined one.
    let recomputes = state.recomputes(from, to);
    let (status, list) = send(
        &state,
        "DELETE",
        "/stats/absorptions/member?dimension=pipeline&member=alpha-old",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["absorptions"], serde_json::json!([]));
    let (_, overview) = send(&state, "GET", &overview_uri, None).await;
    assert_eq!(
        ids(&overview["sessions_by_pipeline"]),
        vec!["alpha-new", "alpha-old"]
    );
    let (_, after) = send(&state, "GET", &performance_uri, None).await;
    assert_eq!(
        after, performance_before,
        "the uncombined state reads as before"
    );
    assert_eq!(
        state.recomputes(from, to),
        recomputes,
        "the pre-Combine memo entry serves the pre-Combine state"
    );
    let (status, _) = send(
        &state,
        "DELETE",
        "/stats/absorptions/member?dimension=pipeline&member=alpha-old",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// #892 / ADR-0077 — the HTTP contract of the Node and Model absorptions: a
/// Node absorption folds two Nodes of one Pipeline row in the three
/// `/stats/*` endpoints and every tree they show in (`uncombined=true`
/// separates them); Nodes of two Pipelines are refused and write nothing;
/// taking a Pipeline out of its absorption takes its Nodes out of the Node
/// absorption of that row; a Model absorption folds two ids on the « By
/// model » axis of Cost and Performance, their efforts meeting by effort,
/// and leaves the Pipeline axis alone.
#[tokio::test]
async fn stats_absorption_folds_nodes_and_models_and_cascades_a_pipeline_removal() {
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    let mut state = Harness::new().await;
    let home = state.scratch("test-stats-absorption-892");
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
        .bind(node.map(|_| 1_i64))
        .bind(payload.to_string())
        .execute(db)
        .await
        .unwrap();
    }

    // Pipeline `beta`: its reviewer Node was renamed between two versions
    // (`review` → `code-review`), and the model it ran on reads once as the
    // pinned alias `opus`, once as the observed id. Pipeline `gamma` runs a
    // `triage` Node, and a `review` Node of its own.
    let runs = [
        (
            "n-b1",
            "beta",
            "Beta",
            "2033-04-15",
            "review",
            "Review",
            "opus",
        ),
        (
            "n-b2",
            "beta",
            "Beta",
            "2033-04-16",
            "code-review",
            "Code review",
            "claude-opus-4-8",
        ),
        (
            "n-g1",
            "gamma",
            "Gamma",
            "2033-04-16",
            "triage",
            "Triage",
            "claude-opus-4-8",
        ),
        (
            "n-g2",
            "gamma",
            "Gamma",
            "2033-04-16",
            "review",
            "Review",
            "claude-sonnet-5",
        ),
    ];
    for (run, pipeline_id, name, day, node, node_name, model) in runs {
        insert_event(
            &state.db,
            run,
            &format!("{day}T09:00:00Z"),
            "run_started",
            None,
            serde_json::json!({
                "pipeline_id": pipeline_id,
                "pipeline_name": name,
                "target_repo": repo,
                "harness": "claude",
                "node_defs": [{"id": node, "name": node_name, "node_type": "agent", "isolated_worktree": true}]
            }),
        )
        .await;
        insert_event(
            &state.db,
            run,
            &format!("{day}T09:01:00Z"),
            "node_started",
            Some(node),
            serde_json::json!({
                "node_type": "agent", "isolated_worktree": true, "harness": "claude",
                "session_id": format!("{run}-s"), "model": model, "effort": "high"
            }),
        )
        .await;
        insert_event(
            &state.db,
            run,
            &format!("{day}T09:11:00Z"),
            "node_completed",
            Some(node),
            serde_json::json!({}),
        )
        .await;
        insert_event(
            &state.db,
            run,
            &format!("{day}T09:20:00Z"),
            "run_completed",
            None,
            serde_json::json!({}),
        )
        .await;
        let dir = home.join(".claude").join("projects").join(
            pdo_harness::claude_transcript::encode_working_dir(
                &pdo_workspace::domain::worktree_layout::sub_worktree_path(
                    &state.repo_root,
                    run,
                    node,
                    1,
                ),
            ),
        );
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(format!("{run}-s.jsonl")),
            format!(
                r#"{{"type":"assistant","requestId":"{run}","message":{{"id":"{run}-m","model":"{model}","usage":{{"input_tokens":1000,"output_tokens":0}}}}}}"#
            ),
        )
        .unwrap();
    }

    async fn send(
        state: &Harness,
        method: &str,
        uri: &str,
        body: Option<serde_json::Value>,
    ) -> (StatusCode, serde_json::Value) {
        let mut request = Request::builder().method(method).uri(uri);
        if body.is_some() {
            request = request.header("content-type", "application/json");
        }
        let response = state
            .router()
            .oneshot(
                request
                    .body(body.map_or_else(Body::empty, |b| Body::from(b.to_string())))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
        )
    }
    let window = "from=2033-04-15T00:00:00Z&to=2033-04-17T00:00:00Z";
    let overview_uri = format!("/stats/overview?{window}&bucket=day");
    let cost_uri = format!("/stats/cost?{window}&bucket=day");
    let performance_uri = format!("/stats/performance?{window}");
    // Node rows only: the Infrastructure and Unassigned rows are no Node.
    let ids = |rows: &serde_json::Value| -> Vec<String> {
        let mut ids: Vec<String> = rows
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["id"].as_str().unwrap().to_string())
            .filter(|id| !id.contains(':'))
            .collect();
        ids.sort();
        ids
    };
    let find = |rows: &serde_json::Value, id: &str| -> serde_json::Value {
        rows.as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == id)
            .cloned()
            .unwrap_or_else(|| panic!("no row {id} in {rows:#}"))
    };
    let strs = |values: &[&str]| -> Vec<String> { values.iter().map(|v| v.to_string()).collect() };
    // Every tree a `beta` Node shows in, with the Node ids each one reads.
    let beta_nodes = |cost: &serde_json::Value, performance: &serde_json::Value| {
        let mut trees = vec![
            ids(&find(&cost["by_pipeline"], "beta")["nodes"]),
            ids(&find(&cost["by_project"][0]["pipelines"], "beta")["nodes"]),
            ids(&find(&performance["by_pipeline"], "beta")["nodes"]),
        ];
        for axis in [&cost["by_model"], &performance["by_model"]] {
            let mut nodes = std::collections::BTreeSet::new();
            for model in axis.as_array().unwrap() {
                for effort in model["efforts"].as_array().unwrap() {
                    for pipeline in effort["pipelines"].as_array().unwrap() {
                        if pipeline["id"] == "beta" {
                            nodes.extend(ids(&pipeline["nodes"]));
                        }
                    }
                }
            }
            trees.push(nodes.into_iter().collect());
        }
        trees
    };

    // --- Before: two Node rows under `beta`, everywhere.
    let (_, overview) = send(&state, "GET", &overview_uri, None).await;
    assert_eq!(
        ids(&find(&overview["sessions_by_pipeline"], "beta")["nodes"]),
        strs(&["code-review", "review"])
    );
    let (_, cost) = send(&state, "GET", &cost_uri, None).await;
    let (_, performance) = send(&state, "GET", &performance_uri, None).await;
    for tree in beta_nodes(&cost, &performance) {
        assert_eq!(tree, strs(&["code-review", "review"]));
    }

    // --- Nodes of two Pipelines are refused, and nothing is written.
    let (status, refusal) = send(
        &state,
        "POST",
        "/stats/absorptions",
        Some(serde_json::json!({
            "dimension": "node",
            "scope": "beta",
            "scope_name": "Beta",
            "absorbent": {"key": "code-review", "name": "Code review", "scope": "beta"},
            "members": [{"key": "triage", "name": "Triage", "scope": "gamma"}],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{refusal}");
    let (_, list) = send(&state, "GET", "/stats/absorptions", None).await;
    assert_eq!(list["absorptions"], serde_json::json!([]));
    let (status, _) = send(
        &state,
        "POST",
        "/stats/absorptions",
        Some(serde_json::json!({
            "dimension": "node",
            "absorbent": {"key": "code-review", "name": "Code review"},
            "members": [{"key": "review", "name": "Review"}],
        })),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "a Node absorption needs its Pipeline"
    );

    // --- Combine the two Nodes of `beta`.
    let (status, list) = send(
        &state,
        "POST",
        "/stats/absorptions",
        Some(serde_json::json!({
            "dimension": "node",
            "scope": "beta",
            "scope_name": "Beta",
            "absorbent": {"key": "code-review", "name": "Code review", "scope": "beta"},
            "members": [{"key": "review", "name": "Review", "scope": "beta"}],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(list["absorptions"][0]["dimension"], "node");
    assert_eq!(list["absorptions"][0]["scope_name"], "Beta");

    let (_, overview) = send(&state, "GET", &overview_uri, None).await;
    let beta = find(&overview["sessions_by_pipeline"], "beta");
    assert_eq!(ids(&beta["nodes"]), strs(&["code-review"]));
    let node = find(&beta["nodes"], "code-review");
    assert_eq!(node["name"], "Code review");
    assert_eq!(node["executions"], 2);
    assert_eq!(node["runs"], 2);
    assert_eq!(node["absorbed"][0]["key"], "review");
    assert_eq!(node["absorbed"][0]["name"], "Review");
    assert_eq!(node["absorbed"][0]["executions"], 1);
    assert_eq!(
        ids(&find(&overview["sessions_by_pipeline"], "gamma")["nodes"]),
        strs(&["review", "triage"]),
        "another Pipeline's `review` stays itself"
    );
    let (_, cost) = send(&state, "GET", &cost_uri, None).await;
    let (_, performance) = send(&state, "GET", &performance_uri, None).await;
    for tree in beta_nodes(&cost, &performance) {
        assert_eq!(tree, strs(&["code-review"]));
    }
    let cost_node = find(&find(&cost["by_pipeline"], "beta")["nodes"], "code-review");
    assert_eq!(cost_node["executions"], 2);
    assert_eq!(cost_node["absorbed"][0]["key"], "review");
    let performance_node = find(
        &find(&performance["by_pipeline"], "beta")["nodes"],
        "code-review",
    );
    assert_eq!(performance_node["harnesses"][0]["duration"]["measured"], 2);
    assert_eq!(performance_node["absorbed"][0]["key"], "review");

    // Uncombined: two Node rows again.
    let (_, overview) = send(
        &state,
        "GET",
        &format!("{overview_uri}&uncombined=true"),
        None,
    )
    .await;
    assert_eq!(
        ids(&find(&overview["sessions_by_pipeline"], "beta")["nodes"]),
        strs(&["code-review", "review"])
    );
    let (_, cost) = send(&state, "GET", &format!("{cost_uri}&uncombined=true"), None).await;
    let (_, performance) = send(
        &state,
        "GET",
        &format!("{performance_uri}&uncombined=true"),
        None,
    )
    .await;
    for tree in beta_nodes(&cost, &performance) {
        assert_eq!(tree, strs(&["code-review", "review"]));
    }

    // --- Models: the alias and the observed id, one row on the model axis.
    let (_, cost) = send(&state, "GET", &cost_uri, None).await;
    let opus_executions = find(&cost["by_model"], "claude-opus-4-8")["executions"]
        .as_i64()
        .unwrap();
    assert_eq!(find(&cost["by_model"], "opus")["executions"], 1);
    let (status, list) = send(
        &state,
        "POST",
        "/stats/absorptions",
        Some(serde_json::json!({
            "dimension": "model",
            "absorbent": {"key": "claude-opus-4-8", "name": "claude-opus-4-8"},
            "members": [{"key": "opus", "name": "opus"}],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{list}");
    let (_, cost) = send(&state, "GET", &cost_uri, None).await;
    assert_eq!(
        ids(&cost["by_model"]),
        strs(&["claude-opus-4-8", "claude-sonnet-5"])
    );
    let opus = find(&cost["by_model"], "claude-opus-4-8");
    assert_eq!(opus["executions"], opus_executions + 1);
    assert_eq!(
        ids(&opus["efforts"]),
        strs(&["high"]),
        "the efforts meet by effort"
    );
    assert_eq!(opus["efforts"][0]["executions"], opus_executions + 1);
    assert_eq!(opus["absorbed"][0]["key"], "opus");
    assert_eq!(opus["absorbed"][0]["runs"], 1);
    assert!(opus["provenance"].is_string());
    // #906 (ADR-0078 amending ADR-0077): the model absorption reaches the
    // Node's own couples too — one couple, marked as globally absorbed.
    let node_pairs =
        find(&find(&cost["by_pipeline"], "beta")["nodes"], "code-review")["models"].clone();
    let pairs: Vec<String> = node_pairs
        .as_array()
        .unwrap()
        .iter()
        .map(|pair| pair["model"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(pairs, strs(&["claude-opus-4-8"]), "{node_pairs:#}");
    assert_eq!(node_pairs[0]["global_absorbed"][0]["key"], "opus|high");
    assert_eq!(node_pairs[0]["global_absorbed"][0]["name"], "opus · high");
    let (_, performance) = send(&state, "GET", &performance_uri, None).await;
    assert_eq!(
        ids(&performance["by_model"]),
        strs(&["claude-opus-4-8", "claude-sonnet-5"])
    );
    let opus = find(&performance["by_model"], "claude-opus-4-8");
    assert_eq!(ids(&opus["efforts"]), strs(&["high"]));
    assert_eq!(opus["absorbed"][0]["key"], "opus");
    let (_, cost) = send(&state, "GET", &format!("{cost_uri}&uncombined=true"), None).await;
    assert!(ids(&cost["by_model"]).contains(&"opus".to_string()));

    // --- Cascade: `beta` absorbs `gamma`, then `triage` joins `review`'s
    // absorbent under `beta`; taking `gamma` out takes `triage` out too.
    let (status, _) = send(
        &state,
        "POST",
        "/stats/absorptions",
        Some(serde_json::json!({
            "dimension": "pipeline",
            "absorbent": {"key": "beta", "name": "Beta"},
            "members": [{"key": "gamma", "name": "Gamma"}],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = send(
        &state,
        "POST",
        "/stats/absorptions",
        Some(serde_json::json!({
            "dimension": "node",
            "scope": "gamma",
            "absorbent": {"key": "code-review", "name": "Code review"},
            "members": [{"key": "triage", "name": "Triage"}],
        })),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "an absorbed Pipeline's Nodes combine under its absorbent"
    );
    let (status, _) = send(
        &state,
        "POST",
        "/stats/absorptions",
        Some(serde_json::json!({
            "dimension": "node",
            "scope": "beta",
            "scope_name": "Beta",
            "absorbent": {"key": "code-review", "name": "Code review", "scope": "beta"},
            "members": [{"key": "triage", "name": "Triage", "scope": "beta"}],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, overview) = send(&state, "GET", &overview_uri, None).await;
    let beta = find(&overview["sessions_by_pipeline"], "beta");
    // gamma's `review` meets beta's under `code-review` too (same id).
    assert_eq!(ids(&beta["nodes"]), strs(&["code-review"]));
    assert_eq!(find(&beta["nodes"], "code-review")["executions"], 4);

    let (status, list) = send(
        &state,
        "DELETE",
        "/stats/absorptions/member?dimension=pipeline&member=gamma",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let node_members: Vec<String> = list["absorptions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|absorption| absorption["dimension"] == "node")
        .flat_map(|absorption| absorption["members"].as_array().unwrap().clone())
        .map(|member| member["key"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        node_members,
        strs(&["review"]),
        "`triage` only ran under gamma: it left; `review` also runs under beta: it stays"
    );
    let (_, overview) = send(&state, "GET", &overview_uri, None).await;
    assert_eq!(
        ids(&find(&overview["sessions_by_pipeline"], "gamma")["nodes"]),
        strs(&["review", "triage"]),
        "gamma's Nodes find their rows under gamma again"
    );
    assert_eq!(
        ids(&find(&overview["sessions_by_pipeline"], "beta")["nodes"]),
        strs(&["code-review"])
    );

    // Uncombine the Node from its icon: the two rows come back.
    let (status, _) = send(
        &state,
        "DELETE",
        "/stats/absorptions/member?dimension=node&scope=beta&member=review",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, overview) = send(&state, "GET", &overview_uri, None).await;
    assert_eq!(
        ids(&find(&overview["sessions_by_pipeline"], "beta")["nodes"]),
        strs(&["code-review", "review"])
    );
}

/// #906 (ADR-0078) — the effort absorption of one model and the couple
/// absorption of one Node, both with a frozen scope, and the global
/// absorptions reaching the couples of every Node. The figures are ADR-0078's:
/// claude-opus-5 has *not set* 10 and high 20, claude-opus-5-5 has *not set* 5
/// and high 8, claude-opus-4-8 has *not set* 3.
#[tokio::test]
async fn stats_absorption_folds_efforts_and_couples_with_a_frozen_scope() {
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    let mut state = Harness::new().await;
    let home = state.scratch("test-stats-absorption-906");
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
        .bind(node.map(|_| 1_i64))
        .bind(payload.to_string())
        .execute(db)
        .await
        .unwrap();
    }

    // Pipeline `delta`: `coder` ran on claude-opus-5 and claude-opus-4-8,
    // `implementer` on claude-opus-5-5. One execution per Run.
    let groups: [(&str, &str, Option<&str>, usize); 5] = [
        ("coder", "claude-opus-5", None, 10),
        ("coder", "claude-opus-5", Some("high"), 20),
        ("implementer", "claude-opus-5-5", None, 5),
        ("implementer", "claude-opus-5-5", Some("high"), 8),
        ("coder", "claude-opus-4-8", None, 3),
    ];
    let mut index = 0;
    for (node, model, effort, count) in groups {
        for _ in 0..count {
            index += 1;
            let run = format!("e-{index:02}");
            let minute = format!("{:02}", index % 60);
            let hour = 8 + index / 60;
            let at = |second: &str| format!("2034-05-15T{hour:02}:{minute}:{second}Z");
            insert_event(
                &state.db,
                &run,
                &at("00"),
                "run_started",
                None,
                serde_json::json!({
                    "pipeline_id": "delta",
                    "pipeline_name": "Delta",
                    "target_repo": repo,
                    "harness": "claude",
                    "node_defs": [
                        {"id": "coder", "name": "Coder", "node_type": "agent", "isolated_worktree": true},
                        {"id": "implementer", "name": "Implementer", "node_type": "agent", "isolated_worktree": true}
                    ]
                }),
            )
            .await;
            let mut started = serde_json::json!({
                "node_type": "agent", "isolated_worktree": true, "harness": "claude",
                "session_id": format!("{run}-s"), "model": model,
            });
            if let Some(effort) = effort {
                started["effort"] = serde_json::json!(effort);
            }
            insert_event(
                &state.db,
                &run,
                &at("10"),
                "node_started",
                Some(node),
                started,
            )
            .await;
            insert_event(
                &state.db,
                &run,
                &at("40"),
                "node_completed",
                Some(node),
                serde_json::json!({}),
            )
            .await;
            insert_event(
                &state.db,
                &run,
                &at("50"),
                "run_completed",
                None,
                serde_json::json!({}),
            )
            .await;
            let dir = home.join(".claude").join("projects").join(
                pdo_harness::claude_transcript::encode_working_dir(
                    &pdo_workspace::domain::worktree_layout::sub_worktree_path(
                        &state.repo_root,
                        &run,
                        node,
                        1,
                    ),
                ),
            );
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join(format!("{run}-s.jsonl")),
                format!(
                    r#"{{"type":"assistant","requestId":"{run}","message":{{"id":"{run}-m","model":"{model}","usage":{{"input_tokens":1000,"output_tokens":0}}}}}}"#
                ),
            )
            .unwrap();
        }
    }

    async fn send(
        state: &Harness,
        method: &str,
        uri: &str,
        body: Option<serde_json::Value>,
    ) -> (StatusCode, serde_json::Value) {
        let mut request = Request::builder().method(method).uri(uri);
        if body.is_some() {
            request = request.header("content-type", "application/json");
        }
        let response = state
            .router()
            .oneshot(
                request
                    .body(body.map_or_else(Body::empty, |b| Body::from(b.to_string())))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
        )
    }
    let (from, to) = ("2034-05-15T00:00:00Z", "2034-05-16T00:00:00Z");
    let window = format!("from={from}&to={to}");
    let cost_uri = format!("/stats/cost?{window}&bucket=day");
    let performance_uri = format!("/stats/performance?{window}");
    let uncombined = "&uncombined=true";
    let find = |rows: &serde_json::Value, id: &str| -> serde_json::Value {
        rows.as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == id)
            .cloned()
            .unwrap_or_else(|| panic!("no row {id} in {rows:#}"))
    };
    // effort id → executions (Cost) of one model row of the « By model » axis.
    let cost_efforts = |cost: &serde_json::Value, model: &str| -> Vec<(String, i64)> {
        find(&cost["by_model"], model)["efforts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| {
                (
                    e["id"].as_str().unwrap().to_string(),
                    e["executions"].as_i64().unwrap(),
                )
            })
            .collect::<std::collections::BTreeMap<_, _>>()
            .into_iter()
            .collect()
    };
    // effort id → measured durations (Performance), the same way.
    let performance_efforts = |perf: &serde_json::Value, model: &str| -> Vec<(String, i64)> {
        find(&perf["by_model"], model)["efforts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| {
                (
                    e["id"].as_str().unwrap().to_string(),
                    e["harnesses"][0]["duration"]["measured"].as_i64().unwrap(),
                )
            })
            .collect::<std::collections::BTreeMap<_, _>>()
            .into_iter()
            .collect()
    };
    let counts = |pairs: &[(&str, i64)]| -> Vec<(String, i64)> {
        pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
    };
    // couple key → executions of one Node row, in every tree it shows in:
    // Cost by pipeline, Cost by project, Performance by pipeline.
    let node_couples = |cost: &serde_json::Value,
                        perf: &serde_json::Value,
                        node: &str|
     -> Vec<Vec<(String, i64)>> {
        let cost_rows = [
            find(&find(&cost["by_pipeline"], "delta")["nodes"], node),
            find(
                &find(&cost["by_project"][0]["pipelines"], "delta")["nodes"],
                node,
            ),
        ];
        let mut trees: Vec<Vec<(String, i64)>> = cost_rows
            .iter()
            .map(|row| {
                let mut pairs: Vec<(String, i64)> = row["models"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|p| {
                        (
                            p["key"].as_str().unwrap().to_string(),
                            p["executions"].as_i64().unwrap(),
                        )
                    })
                    .collect();
                pairs.sort();
                pairs
            })
            .collect();
        let perf_row = find(&find(&perf["by_pipeline"], "delta")["nodes"], node);
        let mut pairs: Vec<(String, i64)> = perf_row["models"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| {
                (
                    p["key"].as_str().unwrap().to_string(),
                    p["harnesses"][0]["duration"]["measured"].as_i64().unwrap(),
                )
            })
            .collect();
        pairs.sort();
        trees.push(pairs);
        trees
    };
    let couple_of = |tree: &serde_json::Value, node: &str, key: &str| -> serde_json::Value {
        find(&find(tree, "delta")["nodes"], node)["models"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["key"] == key)
            .cloned()
            .unwrap_or_else(|| panic!("no couple {key} under {node}"))
    };
    let post = |body: serde_json::Value| {
        let state = state.clone();
        async move { send(&state, "POST", "/stats/absorptions", Some(body)).await }
    };
    let delete = |query: String| {
        let state = state.clone();
        async move {
            send(
                &state,
                "DELETE",
                &format!("/stats/absorptions/member?{query}"),
                None,
            )
            .await
        }
    };
    let enc = |value: &str| -> String {
        value
            .bytes()
            .map(|byte| {
                if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
                    (byte as char).to_string()
                } else {
                    format!("%{byte:02X}")
                }
            })
            .collect()
    };
    let coder_scope = pdo_stats::domain::absorption::couple_scope("delta", "coder");

    // --- Before: two efforts under claude-opus-5.
    let (_, cost) = send(&state, "GET", &cost_uri, None).await;
    assert_eq!(
        cost_efforts(&cost, "claude-opus-5"),
        counts(&[("", 10), ("high", 20)])
    );
    let before_high = find(&find(&cost["by_model"], "claude-opus-5")["efforts"], "high");
    let before_not_set = find(&find(&cost["by_model"], "claude-opus-5")["efforts"], "");
    let (_, performance) = send(&state, "GET", &performance_uri, None).await;
    assert_eq!(
        performance_efforts(&performance, "claude-opus-5"),
        counts(&[("", 10), ("high", 20)])
    );
    let recomputes = state.recomputes(from, to);

    // --- Efforts of two models are refused, and nothing is written.
    let (status, refusal) = post(serde_json::json!({
        "dimension": "effort",
        "scope": "claude-opus-5",
        "absorbent": {"key": "high", "name": "high", "scope": "claude-opus-5"},
        "members": [{"key": "", "name": "not set", "scope": "claude-opus-5-5"}],
    }))
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{refusal}");
    let (_, list) = send(&state, "GET", "/stats/absorptions", None).await;
    assert_eq!(list["absorptions"], serde_json::json!([]));

    // --- Effort absorption: *not set* → high on claude-opus-5.
    let (status, list) = post(serde_json::json!({
        "dimension": "effort",
        "scope": "claude-opus-5",
        "scope_name": "claude-opus-5",
        "absorbent": {"key": "high", "name": "high", "scope": "claude-opus-5"},
        "members": [{"key": "", "name": "not set", "scope": "claude-opus-5"}],
    }))
    .await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(list["absorptions"][0]["dimension"], "effort");
    assert_eq!(list["absorptions"][0]["scope"], "claude-opus-5");
    assert_eq!(list["absorptions"][0]["scope_name"], "claude-opus-5");
    assert_eq!(list["absorptions"][0]["members"][0]["key"], "");

    let (_, cost) = send(&state, "GET", &cost_uri, None).await;
    assert_eq!(
        cost_efforts(&cost, "claude-opus-5"),
        counts(&[("high", 30)])
    );
    let high = find(&find(&cost["by_model"], "claude-opus-5")["efforts"], "high");
    assert_eq!(high["runs"], 30);
    assert_eq!(high["absorbed"][0]["key"], "");
    assert_eq!(high["absorbed"][0]["name"], "not set");
    assert_eq!(high["absorbed"][0]["runs"], 10);
    assert_eq!(
        high["absorbed"][0]["scopes"],
        serde_json::json!(["claude-opus-5"])
    );
    // The honesty marks add up, they are never smoothed away.
    for field in ["readable", "unknown"] {
        assert_eq!(
            high[field].as_i64().unwrap(),
            before_high[field].as_i64().unwrap() + before_not_set[field].as_i64().unwrap(),
            "{field}"
        );
    }
    assert_eq!(
        high["partial"].as_bool().unwrap(),
        before_high["partial"].as_bool().unwrap() || before_not_set["partial"].as_bool().unwrap()
    );
    let (_, performance) = send(&state, "GET", &performance_uri, None).await;
    assert_eq!(
        state.recomputes(from, to),
        recomputes + 1,
        "an effort absorption must recompute the Performance memo"
    );
    assert_eq!(
        performance_efforts(&performance, "claude-opus-5"),
        counts(&[("high", 30)])
    );
    assert_eq!(
        find(
            &find(&performance["by_model"], "claude-opus-5")["efforts"],
            "high"
        )["absorbed"][0]["key"],
        ""
    );
    // It reaches the couples of `coder`, in every tree, marked as global.
    for tree in node_couples(&cost, &performance, "coder") {
        assert_eq!(
            tree,
            counts(&[("claude-opus-4-8|", 3), ("claude-opus-5|high", 30)])
        );
    }
    for tree in [&cost["by_pipeline"], &performance["by_pipeline"]] {
        let couple = couple_of(tree, "coder", "claude-opus-5|high");
        assert_eq!(couple["global_absorbed"][0]["key"], "claude-opus-5|");
        assert_eq!(
            couple["global_absorbed"][0]["name"],
            "claude-opus-5 · not set"
        );
        assert_eq!(couple["global_absorbed"][0]["runs"], 10);
        assert!(couple_of(tree, "coder", "claude-opus-4-8|")["global_absorbed"].is_null());
    }
    // Uncombined: both efforts and both couples again.
    let (_, raw_cost) = send(&state, "GET", &format!("{cost_uri}{uncombined}"), None).await;
    let (_, raw_performance) = send(
        &state,
        "GET",
        &format!("{performance_uri}{uncombined}"),
        None,
    )
    .await;
    assert_eq!(
        cost_efforts(&raw_cost, "claude-opus-5"),
        counts(&[("", 10), ("high", 20)])
    );
    assert_eq!(
        performance_efforts(&raw_performance, "claude-opus-5"),
        counts(&[("", 10), ("high", 20)])
    );
    for tree in node_couples(&raw_cost, &raw_performance, "coder") {
        assert_eq!(
            tree,
            counts(&[
                ("claude-opus-4-8|", 3),
                ("claude-opus-5|", 10),
                ("claude-opus-5|high", 20)
            ])
        );
    }

    // --- ADR-0078, case 2: claude-opus-5 absorbs claude-opus-4-8 after the
    // pose — the newcomer's *not set* stays apart.
    let (status, _) = post(serde_json::json!({
        "dimension": "model",
        "absorbent": {"key": "claude-opus-5", "name": "claude-opus-5"},
        "members": [{"key": "claude-opus-4-8", "name": "claude-opus-4-8"}],
    }))
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, cost) = send(&state, "GET", &cost_uri, None).await;
    assert_eq!(
        cost_efforts(&cost, "claude-opus-5"),
        counts(&[("", 3), ("high", 30)])
    );
    let (_, performance) = send(&state, "GET", &performance_uri, None).await;
    assert_eq!(
        performance_efforts(&performance, "claude-opus-5"),
        counts(&[("", 3), ("high", 30)])
    );
    // The model absorption reaches `coder`'s couples too.
    for tree in node_couples(&cost, &performance, "coder") {
        assert_eq!(
            tree,
            counts(&[("claude-opus-5|", 3), ("claude-opus-5|high", 30)])
        );
    }
    for tree in [&cost["by_pipeline"], &performance["by_pipeline"]] {
        let couple = couple_of(tree, "coder", "claude-opus-5|");
        assert_eq!(couple["global_absorbed"][0]["key"], "claude-opus-4-8|");
    }
    let (status, _) = delete("dimension=model&member=claude-opus-4-8".to_string()).await;
    assert_eq!(status, StatusCode::OK);

    // --- ADR-0078, case 1: claude-opus-5-5 absorbs claude-opus-5 — the
    // absorption keeps to claude-opus-5's Runs: high 38, *not set* 5.
    let (status, _) = post(serde_json::json!({
        "dimension": "model",
        "absorbent": {"key": "claude-opus-5-5", "name": "claude-opus-5-5"},
        "members": [{"key": "claude-opus-5", "name": "claude-opus-5"}],
    }))
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, cost) = send(&state, "GET", &cost_uri, None).await;
    assert_eq!(
        cost_efforts(&cost, "claude-opus-5-5"),
        counts(&[("", 5), ("high", 38)])
    );
    let high = find(
        &find(&cost["by_model"], "claude-opus-5-5")["efforts"],
        "high",
    );
    assert_eq!(high["absorbed"][0]["key"], "", "the member still shows");
    assert_eq!(
        high["absorbed"][0]["scopes"],
        serde_json::json!(["claude-opus-5"])
    );
    let (_, performance) = send(&state, "GET", &performance_uri, None).await;
    assert_eq!(
        performance_efforts(&performance, "claude-opus-5-5"),
        counts(&[("", 5), ("high", 38)])
    );
    // An effort absorption on a model row counted under another is refused.
    let (status, _) = post(serde_json::json!({
        "dimension": "effort",
        "scope": "claude-opus-5",
        "absorbent": {"key": "high", "name": "high"},
        "members": [{"key": "low", "name": "low"}],
    }))
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // --- ADR-0078, case 3: taking claude-opus-5 out — it comes back with
    // high 30 and no *not set*.
    let (status, _) = delete("dimension=model&member=claude-opus-5".to_string()).await;
    assert_eq!(status, StatusCode::OK);
    let (_, cost) = send(&state, "GET", &cost_uri, None).await;
    assert_eq!(
        cost_efforts(&cost, "claude-opus-5"),
        counts(&[("high", 30)])
    );
    assert_eq!(
        cost_efforts(&cost, "claude-opus-5-5"),
        counts(&[("", 5), ("high", 8)])
    );

    // --- Couple absorption in `coder`: claude-opus-4-8 · not set into
    // claude-opus-5 · high. One couple row, never on the « By model » axis.
    let (status, refusal) = post(serde_json::json!({
        "dimension": "couple",
        "scope": coder_scope,
        "absorbent": {"key": "claude-opus-5|high", "name": "claude-opus-5 · high", "scope": coder_scope},
        "members": [{"key": "claude-opus-5-5|high", "name": "claude-opus-5-5 · high",
                     "scope": pdo_stats::domain::absorption::couple_scope("delta", "implementer")}],
    }))
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "couples of two Nodes: {refusal}"
    );
    let recomputes = state.recomputes(from, to);
    let (status, list) = post(serde_json::json!({
        "dimension": "couple",
        "scope": coder_scope,
        "scope_name": "Coder (Delta)",
        "absorbent": {"key": "claude-opus-5|high", "name": "claude-opus-5 · high", "scope": coder_scope},
        "members": [{"key": "claude-opus-4-8|", "name": "claude-opus-4-8 · not set", "scope": coder_scope}],
    }))
    .await;
    assert_eq!(status, StatusCode::OK, "{list}");
    let couple_view = list["absorptions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["dimension"] == "couple")
        .cloned()
        .unwrap();
    assert_eq!(couple_view["scope_name"], "Coder (Delta)");
    assert_eq!(couple_view["absorbent"]["name"], "claude-opus-5 · high");

    let (_, cost) = send(&state, "GET", &cost_uri, None).await;
    let (_, performance) = send(&state, "GET", &performance_uri, None).await;
    assert_eq!(
        state.recomputes(from, to),
        recomputes + 1,
        "a couple absorption must recompute the Performance memo"
    );
    for tree in node_couples(&cost, &performance, "coder") {
        assert_eq!(tree, counts(&[("claude-opus-5|high", 33)]));
    }
    for tree in [&cost["by_pipeline"], &performance["by_pipeline"]] {
        let couple = couple_of(tree, "coder", "claude-opus-5|high");
        assert_eq!(couple["absorbed"][0]["key"], "claude-opus-4-8|");
        assert_eq!(couple["absorbed"][0]["runs"], 3);
        assert_eq!(
            couple["absorbed"][0]["scopes"],
            serde_json::json!([coder_scope])
        );
    }
    // The « By model » axis never sees it.
    assert_eq!(cost_efforts(&cost, "claude-opus-4-8"), counts(&[("", 3)]));
    assert_eq!(
        performance_efforts(&performance, "claude-opus-4-8"),
        counts(&[("", 3)])
    );
    let (_, raw_cost) = send(&state, "GET", &format!("{cost_uri}{uncombined}"), None).await;
    let (_, raw_performance) = send(
        &state,
        "GET",
        &format!("{performance_uri}{uncombined}"),
        None,
    )
    .await;
    for tree in node_couples(&raw_cost, &raw_performance, "coder") {
        assert_eq!(tree.len(), 3, "Uncombined shows every raw couple: {tree:?}");
    }

    // --- Global before local: claude-opus-5-5 absorbs claude-opus-5, the
    // local absorbent's model — it carries the local absorption along.
    let (status, _) = post(serde_json::json!({
        "dimension": "model",
        "absorbent": {"key": "claude-opus-5-5", "name": "claude-opus-5-5"},
        "members": [{"key": "claude-opus-5", "name": "claude-opus-5"}],
    }))
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, cost) = send(&state, "GET", &cost_uri, None).await;
    let (_, performance) = send(&state, "GET", &performance_uri, None).await;
    for tree in node_couples(&cost, &performance, "coder") {
        assert_eq!(tree, counts(&[("claude-opus-5-5|high", 33)]));
    }
    let (status, _) = delete("dimension=model&member=claude-opus-5".to_string()).await;
    assert_eq!(status, StatusCode::OK);

    // --- Frozen scope of couples: `implementer` absorbs `coder` — its own
    // couples are not combined; `coder`'s stay combined.
    let (status, _) = post(serde_json::json!({
        "dimension": "node",
        "scope": "delta",
        "scope_name": "Delta",
        "absorbent": {"key": "implementer", "name": "Implementer", "scope": "delta"},
        "members": [{"key": "coder", "name": "Coder", "scope": "delta"}],
    }))
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, cost) = send(&state, "GET", &cost_uri, None).await;
    let (_, performance) = send(&state, "GET", &performance_uri, None).await;
    for tree in node_couples(&cost, &performance, "implementer") {
        assert_eq!(
            tree,
            counts(&[
                ("claude-opus-5-5|", 5),
                ("claude-opus-5-5|high", 8),
                ("claude-opus-5|high", 33)
            ])
        );
    }
    let (status, _) = delete("dimension=node&scope=delta&member=coder".to_string()).await;
    assert_eq!(status, StatusCode::OK);
    let (_, cost) = send(&state, "GET", &cost_uri, None).await;
    let (_, performance) = send(&state, "GET", &performance_uri, None).await;
    for tree in node_couples(&cost, &performance, "coder") {
        assert_eq!(
            tree,
            counts(&[("claude-opus-5|high", 33)]),
            "coder comes back with its couple combined"
        );
    }

    // --- Uncombine from the members list / Settings: the rows come back.
    // (A state the memo has already seen is served from it: take the effort
    // out first, leaving a couple-only state it never computed.)
    let recomputes = state.recomputes(from, to);
    let (status, _) = delete("dimension=effort&scope=claude-opus-5&member=".to_string()).await;
    assert_eq!(status, StatusCode::OK);
    let (_, performance) = send(&state, "GET", &performance_uri, None).await;
    assert_eq!(
        state.recomputes(from, to),
        recomputes + 1,
        "an Uncombine must recompute the Performance memo"
    );
    let (_, cost) = send(&state, "GET", &cost_uri, None).await;
    for tree in node_couples(&cost, &performance, "coder") {
        assert_eq!(
            tree,
            counts(&[("claude-opus-5|", 10), ("claude-opus-5|high", 23)]),
            "the couple absorption stays, the effort one is gone"
        );
    }
    let (status, list) = delete(format!(
        "dimension=couple&scope={}&member={}",
        enc(&coder_scope),
        enc("claude-opus-4-8|")
    ))
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["absorptions"], serde_json::json!([]));
    let (_, cost) = send(&state, "GET", &cost_uri, None).await;
    let (_, performance) = send(&state, "GET", &performance_uri, None).await;
    assert_eq!(
        cost_efforts(&cost, "claude-opus-5"),
        counts(&[("", 10), ("high", 20)])
    );
    for tree in node_couples(&cost, &performance, "coder") {
        assert_eq!(tree.len(), 3);
    }
}

/// #890 — an absorption is instance configuration, not a property of the
/// library: deleting the absorbed Pipeline's YAML leaves it in place, and a
/// dimension Stats does not combine is refused.
#[tokio::test]
async fn stats_absorption_survives_a_library_delete_and_refuses_unknown_dimensions() {
    let tmp = tempfile::tempdir().unwrap();
    // The library's YAML of the Pipeline about to be absorbed.
    let pipelines_dir = tmp.path().join(".pdo").join("pipelines");
    std::fs::create_dir_all(&pipelines_dir).unwrap();
    std::fs::write(pipelines_dir.join("doomed.yaml"), "name: doomed\n").unwrap();
    let state = Harness::with_dir(tmp.path()).await;
    let post = |body: serde_json::Value| {
        let state = state.clone();
        async move {
            state
                .router()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/stats/absorptions")
                        .header("content-type", "application/json")
                        .body(Body::from(body.to_string()))
                        .unwrap(),
                )
                .await
                .unwrap()
                .status()
        }
    };
    assert_eq!(
        post(serde_json::json!({
            "dimension": "pipeline",
            "absorbent": {"key": "keeper", "name": "Keeper"},
            "members": [{"key": "doomed", "name": "doomed"}],
        }))
        .await,
        StatusCode::OK
    );
    assert_eq!(
        post(serde_json::json!({
            "dimension": "project",
            "absorbent": {"key": "a", "name": "A"},
            "members": [{"key": "b", "name": "B"}],
        }))
        .await,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        post(serde_json::json!({
            "dimension": "pipeline",
            "absorbent": {"key": "a", "name": "A"},
            "members": [{"key": "a", "name": "A"}],
        }))
        .await,
        StatusCode::BAD_REQUEST,
        "absorbing only itself is no absorption"
    );

    // The pipeline context deletes the YAML; Stats never hears of it.
    std::fs::remove_file(tmp.path().join(".pdo/pipelines/doomed.yaml")).unwrap();

    let listed = state
        .router()
        .oneshot(
            Request::builder()
                .uri("/stats/absorptions")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(listed.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(body["absorptions"][0]["members"][0]["key"], "doomed");
}

/// #891 / ADR-0077 §4 — the rename-born absorption, as the pipeline context
/// records it through this crate's ledger: a rename that changes the id
/// records old → new (origin `rename`), and a chain of two renames leaves one
/// absorbent carrying both old ids. Stats then reads one row for the old and
/// the new Pipeline in every tab; `uncombined=true` reads two. (That a
/// display-only rename records nothing is the pipeline context's rule, its
/// use-case test `a_display_only_rename_splices_the_name_and_records_nothing`.)
#[tokio::test]
async fn stats_absorption_is_recorded_by_a_pipeline_rename() {
    let tmp = tempfile::tempdir().unwrap();
    let mut state = Harness::with_dir(tmp.path()).await;
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    state = state.with_home(home);
    let repo = state.repo_root.to_string_lossy().into_owned();

    async fn send(
        state: &Harness,
        method: &str,
        uri: &str,
        body: Option<serde_json::Value>,
    ) -> (StatusCode, serde_json::Value) {
        let mut request = Request::builder().method(method).uri(uri);
        if body.is_some() {
            request = request.header("content-type", "application/json");
        }
        let response = state
            .router()
            .oneshot(
                request
                    .body(body.map_or_else(Body::empty, |b| Body::from(b.to_string())))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
        )
    }
    async fn completed_run(
        db: &sqlx::SqlitePool,
        repo: &str,
        run: &str,
        pipeline_id: &str,
        name: &str,
        day: &str,
    ) {
        let events = [
            (
                "09:00",
                "run_started",
                None,
                serde_json::json!({
                    "pipeline_id": pipeline_id,
                    "pipeline_name": name,
                    "target_repo": repo,
                    "harness": "claude",
                    "node_defs": [{"id":"worker","name":"Worker","node_type":"agent"}]
                }),
            ),
            (
                "09:01",
                "node_started",
                Some("worker"),
                serde_json::json!({"node_type":"agent","harness":"claude","session_id":format!("{run}-s")}),
            ),
            (
                "09:11",
                "node_completed",
                Some("worker"),
                serde_json::json!({}),
            ),
            ("09:12", "run_completed", None, serde_json::json!({})),
        ];
        for (time, kind, node, payload) in events {
            sqlx::query(
                "INSERT INTO events (run_id, ts, kind, node_id, iter, payload) \
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(run)
            .bind(format!("{day}T{time}:00Z"))
            .bind(kind)
            .bind(node)
            .bind(node.map(|_| 1_i64))
            .bind(payload.to_string())
            .execute(db)
            .await
            .unwrap();
        }
    }
    let ids = |rows: &serde_json::Value| -> Vec<String> {
        let mut ids: Vec<String> = rows
            .as_array()
            .unwrap_or_else(|| panic!("not rows: {rows}"))
            .iter()
            .map(|row| row["id"].as_str().unwrap().to_string())
            .collect();
        ids.sort();
        ids
    };

    // A Run of `alpha`, then the rename that changes its id.
    completed_run(&state.db, &repo, "ren-1", "alpha", "alpha", "2033-04-10").await;
    state
        .record_rename(("alpha", "alpha"), ("beta", "Beta"))
        .await;
    let (_, list) = send(&state, "GET", "/stats/absorptions", None).await;
    assert_eq!(
        list["absorptions"],
        serde_json::json!([{
            "dimension": "pipeline",
            "scope": "",
            "absorbent": {"key": "beta", "name": "Beta"},
            "members": [{
                "key": "alpha",
                "name": "alpha",
                "origin": "rename",
                "created_at": list["absorptions"][0]["members"][0]["created_at"],
            }],
        }])
    );

    // A Run of the renamed Pipeline: Stats reads one row, uncombined two.
    completed_run(&state.db, &repo, "ren-2", "beta", "Beta", "2033-04-11").await;
    let window = "from=2033-04-10T00:00:00Z&to=2033-04-12T00:00:00Z";
    for (endpoint, rows) in [
        ("overview", "sessions_by_pipeline"),
        ("cost", "by_pipeline"),
        ("performance", "by_pipeline"),
    ] {
        let uri = format!("/stats/{endpoint}?{window}&bucket=day");
        let (status, combined) = send(&state, "GET", &uri, None).await;
        assert_eq!(status, StatusCode::OK, "{endpoint}: {combined}");
        assert_eq!(ids(&combined[rows]), vec!["beta"], "{endpoint}");
        let row = &combined[rows][0];
        assert_eq!(row["name"], "Beta", "{endpoint}");
        assert_eq!(row["absorbed"][0]["key"], "alpha", "{endpoint}");
        let (_, raw) = send(&state, "GET", &format!("{uri}&uncombined=true"), None).await;
        assert_eq!(ids(&raw[rows]), vec!["alpha", "beta"], "{endpoint}");
    }
    let (_, overview) = send(
        &state,
        "GET",
        &format!("/stats/overview?{window}&bucket=day"),
        None,
    )
    .await;
    assert_eq!(overview["sessions_by_pipeline"][0]["executions"], 2);

    // A second rename: one absorbent, both old ids under it.
    state
        .record_rename(("beta", "Beta"), ("gamma", "Gamma"))
        .await;
    let (_, list) = send(&state, "GET", "/stats/absorptions", None).await;
    let absorptions = list["absorptions"].as_array().unwrap();
    assert_eq!(absorptions.len(), 1, "{list}");
    assert_eq!(absorptions[0]["absorbent"]["key"], "gamma");
    assert_eq!(absorptions[0]["absorbent"]["name"], "Gamma");
    let mut members: Vec<(String, String)> = absorptions[0]["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| {
            (
                m["key"].as_str().unwrap().to_string(),
                m["origin"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    members.sort();
    assert_eq!(
        members,
        vec![
            ("alpha".to_string(), "rename".to_string()),
            ("beta".to_string(), "rename".to_string()),
        ]
    );
}

/// #891 — the ledger the pipeline context records a rename through (a save
/// whose `name:` change moves the file is one) writes the absorption row.
#[tokio::test]
async fn the_rename_ledger_records_a_rename_born_absorption() {
    let tmp = tempfile::tempdir().unwrap();
    let state = Harness::with_dir(tmp.path()).await;
    state
        .record_rename(("saved-old", "saved-old"), ("saved-new", "Saved New"))
        .await;
    let rows = state.absorption_rows().await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].member, "saved-old");
    assert_eq!(rows[0].member_name, "saved-old");
    assert_eq!(rows[0].absorbent, "saved-new");
    assert_eq!(rows[0].absorbent_name, "Saved New");
    assert_eq!(rows[0].origin, "rename");
}
