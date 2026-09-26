//! # TEMPORARY — contract snapshots of the refactor net (spec #913, issue #914)
//!
//! **This whole folder is deleted by #932**, the last ticket of the spec, once
//! the daemon has been moved to one crate per bounded context. Don't extend it
//! with new features; write a layer-4 test (ADR-0080) instead.
//!
//! Why it exists: the migration of spec #913 must not change one byte of the
//! HTTP and WebSocket contract. Eight business scenarios are played against the
//! real daemon, and every HTTP response and every `/ws` message they produce is
//! frozen as an `insta` JSON snapshot (`snapshots/*.snap`). A refactor that
//! changes a field, a status code or an event turns one of them red.
//!
//! What is masked (see [`mask`]): ids, SHAs, timestamps, dates, paths, the port,
//! wall-clock numbers. What is not frozen: JSON key order (`serde_json` sorts
//! keys) and the interleaving of `/ws` messages (frozen as a sorted list).
//!
//! A deliberate change of the contract updates the snapshots in the same PR:
//! `INSTA_UPDATE=always cargo nextest run -E 'test(/snapshots_temporary/)'`, then
//! review the diff of `snapshots/`.
//!
//! The daemon is **nested** (no sweeps, no stale detector, no reaper: nothing
//! fires on a clock) and its home is its tempdir (no transcript of the real
//! `~/.claude` can reach the Stats folds).

mod mask;
mod recorder;

use std::path::Path;

use recorder::Recorder;
use serde_json::json;

use crate::common::flow;
use crate::common::TestDaemon;

const SOLO: &str = "solo";
const GATED: &str = "gated";
const SIBLING: &str = "sibling";

fn seed(repo: &Path) -> anyhow::Result<()> {
    flow::seed_solo(repo, &[(SOLO, false), (GATED, true), (SIBLING, false)])
}

async fn daemon() -> TestDaemon {
    TestDaemon::spawn_configured(seed, |root, config| {
        config.nested_daemon = true;
        config.sandbox_home_override = Some(root.to_path_buf());
    })
    .await
    .unwrap()
}

/// `from=…&to=…` around now — a one-day Stats window whose bucket dates mask
/// to `[date]` whatever the day the suite runs.
fn window() -> String {
    let now = chrono::Utc::now();
    let from = (now - chrono::Duration::hours(12)).format("%Y-%m-%dT%H:%M:%SZ");
    let to = (now + chrono::Duration::hours(12)).format("%Y-%m-%dT%H:%M:%SZ");
    format!("from={from}&to={to}")
}

async fn launch(rec: &mut Recorder<'_>, daemon: &TestDaemon, pipeline: &str) -> String {
    let (status, body) = rec
        .post(
            "/runs",
            json!({
                "pipeline": pipeline,
                "input": "go",
                "name": format!("{pipeline} run"),
                "target_repo": daemon.target_repo(),
            }),
        )
        .await;
    assert_eq!(status, 201, "{body}");
    let run_id = body["run_id"].as_str().unwrap().to_string();
    rec.alias(&run_id, "run");
    flow::wait_node_status(daemon, &run_id, "worker", "running").await;
    run_id
}

async fn complete(rec: &mut Recorder<'_>, daemon: &TestDaemon, run_id: &str) {
    complete_iter(rec, daemon, run_id, 1).await;
}

async fn complete_iter(rec: &mut Recorder<'_>, daemon: &TestDaemon, run_id: &str, iter: i64) {
    flow::write_out_artifact_at(daemon, run_id, "worker", iter);
    let (status, body) = rec
        .post(
            &format!("/runs/{run_id}/nodes/worker/done"),
            json!({ "iter": iter }),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    flow::wait_run_status(daemon, run_id, "completed").await;
}

// --- 1. Pipeline: create, edit, rename, export ------------------------------------

#[tokio::test]
async fn pipeline_create_edit_rename_export() {
    let daemon = daemon().await;
    let mut rec = Recorder::start(&daemon).await;

    let (status, _) = rec.post("/pipelines", json!({ "name": "Snap Flow" })).await;
    assert_eq!(status, 201);
    rec.get("/pipelines/snap-flow").await;

    let edited = flow::solo_pipeline_yaml("Snap Flow", false);
    let (status, body) = rec
        .put(
            "/pipelines/snap-flow",
            json!({ "yaml": edited, "prompts": { "worker": "Edited prompt.\n" } }),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    rec.get("/pipelines/snap-flow").await;

    let (status, _) = rec
        .put(
            "/pipelines/snap-flow/rename",
            json!({ "name": "Snap Flow Renamed" }),
        )
        .await;
    assert_eq!(status, 200);
    rec.get("/pipelines").await;
    rec.get("/pipelines/snap-flow-renamed/document").await;
    rec.get("/pipelines/snap-flow-renamed/document/skills")
        .await;
    rec.post("/pipelines/snap-flow-renamed/duplicate", json!({}))
        .await;
    rec.delete("/pipelines/snap-flow-renamed").await;
    rec.get("/pipelines").await;

    rec.finish("pipeline_create_edit_rename_export").await;
}

// --- 2. Run: launch, complete, fail, reopen, retry ---------------------------------

#[tokio::test]
async fn run_launch_complete_fail_retry_reopen() {
    let daemon = daemon().await;
    let mut rec = Recorder::start(&daemon).await;

    let done = launch(&mut rec, &daemon, SOLO).await;
    rec.get(&format!("/runs/{done}")).await;
    complete(&mut rec, &daemon, &done).await;
    rec.get(&format!("/runs/{done}")).await;
    rec.get(&format!("/runs/{done}/events")).await;
    rec.get(&format!("/runs/{done}/nodes/worker/io")).await;

    // A `pdo fail` terminalises the Run instead of parking it (ADR-0049).
    let (status, _) = rec.put("/settings", json!({ "auto_fail": true })).await;
    assert_eq!(status, 200);
    let failed = launch(&mut rec, &daemon, SIBLING).await;
    let (status, body) = rec
        .post(
            &format!("/runs/{failed}/nodes/worker/fail"),
            json!({ "iter": 1, "reason": "driven terminal on purpose" }),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    flow::wait_run_status(&daemon, &failed, "failed").await;
    rec.get(&format!("/runs/{failed}")).await;

    let (status, body) = rec
        .post(
            &format!("/runs/{failed}/commands"),
            json!({ "kind": "reopen_run" }),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    flow::wait_run_status(&daemon, &failed, "running").await;
    rec.get(&format!("/runs/{failed}")).await;

    let (status, body) = rec
        .post(&format!("/runs/{failed}/nodes/worker/retry"), json!({}))
        .await;
    assert_eq!(status, 200, "{body}");
    let iter = body["iter"].as_i64().unwrap();
    flow::wait_node_status(&daemon, &failed, "worker", "running").await;
    rec.get(&format!("/runs/{failed}")).await;
    complete_iter(&mut rec, &daemon, &failed, iter).await;
    rec.get(&format!("/runs/{failed}")).await;
    rec.get(&format!("/runs/{failed}/events")).await;

    rec.finish("run_launch_complete_fail_retry_reopen").await;
}

// --- 3. Review of the diff -------------------------------------------------------------

#[tokio::test]
async fn review_of_the_diff() {
    let daemon = daemon().await;
    let mut rec = Recorder::start(&daemon).await;

    let run_id = launch(&mut rec, &daemon, SOLO).await;
    let wt = flow::worktree(&daemon, &run_id);
    std::fs::write(
        wt.join("lib.rs"),
        "fn a() {}\nfn b() {}\nfn c() {}\nfn d() {}\n",
    )
    .unwrap();
    flow::git(&wt, &["add", "lib.rs"]);
    flow::git(&wt, &["commit", "-q", "-m", "run work"]);

    rec.get(&format!("/runs/{run_id}/refs")).await;
    rec.get(&format!("/runs/{run_id}/diff?from=fork&to=tip"))
        .await;
    rec.get(&format!("/runs/{run_id}/diff/structured?from=fork&to=tip"))
        .await;
    rec.get(&format!("/runs/{run_id}/file?path=lib.rs&ref=tip"))
        .await;
    let (status, body) = rec
        .post(
            &format!("/runs/{run_id}/review/comments/send"),
            json!({
                "comments": [{ "path": "lib.rs", "side": "new", "line": 4, "from": "fork", "to": "tip", "text": "Name it delta." }]
            }),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    rec.post(
        &format!("/runs/{run_id}/review/comments/rc-001/reply"),
        json!({ "text": "Renamed.", "resolved": true, "author": "worker" }),
    )
    .await;
    rec.post(
        &format!("/runs/{run_id}/review/comments/rc-001/resolve"),
        json!({}),
    )
    .await;
    rec.post(
        &format!("/runs/{run_id}/review/comments/rc-001/reopen"),
        json!({}),
    )
    .await;
    rec.get(&format!("/runs/{run_id}/review/comments?state=all"))
        .await;
    rec.post(&format!("/runs/{run_id}/manager/stop"), json!({}))
        .await;

    rec.finish("review_of_the_diff").await;
}

// --- 4. Trigger: create, run now, fires --------------------------------------------------

#[tokio::test]
async fn trigger_create_run_now_fires() {
    let daemon = daemon().await;
    let mut rec = Recorder::start(&daemon).await;

    let (status, trigger) = rec
        .post(
            "/triggers",
            json!({
                "name": "nightly",
                "pipeline_id": SOLO,
                "cron": "0 3 * * *",
                "input_template": "audit",
                "target_repo": daemon.target_repo(),
            }),
        )
        .await;
    assert_eq!(status, 201, "{trigger}");
    let id = trigger["id"].as_str().unwrap().to_string();
    rec.alias(&id, "trigger");
    rec.get("/triggers").await;
    rec.patch(
        &format!("/triggers/{id}"),
        json!({ "name": "nightly audit" }),
    )
    .await;
    let (status, fired) = rec.post(&format!("/triggers/{id}/fire"), json!({})).await;
    assert_eq!(status, 200, "{fired}");
    let run_id = fired["run_id"].as_str().unwrap().to_string();
    rec.alias(&run_id, "run");
    flow::wait_node_status(&daemon, &run_id, "worker", "running").await;
    rec.get(&format!("/triggers/{id}/fires")).await;
    rec.get(&format!("/triggers/{id}")).await;
    rec.get("/triggers/health").await;
    rec.post("/triggers/pause", json!({ "paused": true })).await;
    rec.delete(&format!("/triggers/{id}")).await;
    rec.get("/triggers").await;

    rec.finish("trigger_create_run_now_fires").await;
}

// --- 5. Stats: overview, cost, performance, absorption -------------------------------------

#[tokio::test]
async fn stats_overview_cost_performance_absorption() {
    let daemon = daemon().await;
    let mut rec = Recorder::start(&daemon).await;
    // Wall-clock distributions of the Performance fold.
    for key in ["stats", "active", "total", "waiting"] {
        rec.mask.masked_keys.insert(key.to_string());
    }

    let first = launch(&mut rec, &daemon, SOLO).await;
    complete(&mut rec, &daemon, &first).await;
    let second = launch(&mut rec, &daemon, SIBLING).await;
    complete(&mut rec, &daemon, &second).await;

    let window = window();
    rec.get(&format!("/stats/overview?{window}&bucket=day"))
        .await;
    rec.get(&format!("/stats/cost?{window}&bucket=day")).await;
    rec.get(&format!("/stats/performance?{window}&refresh=true"))
        .await;
    let (status, body) = rec
        .post(
            "/stats/absorptions",
            json!({
                "dimension": "pipeline",
                "absorbent": { "key": SOLO, "name": SOLO },
                "members": [{ "key": SIBLING, "name": SIBLING }],
            }),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    rec.get("/stats/absorptions").await;
    rec.get(&format!("/stats/overview?{window}&bucket=day"))
        .await;
    rec.get(&format!("/stats/performance?{window}&refresh=true"))
        .await;
    rec.delete(&format!(
        "/stats/absorptions/member?dimension=pipeline&member={SIBLING}"
    ))
    .await;
    rec.get(&format!(
        "/stats/overview?{window}&bucket=day&completed_only=true"
    ))
    .await;

    rec.finish("stats_overview_cost_performance_absorption")
        .await;
}

// --- 6. Settings and agent profiles ----------------------------------------------------------

#[tokio::test]
async fn settings_and_agent_profiles() {
    let daemon = daemon().await;
    let mut rec = Recorder::start(&daemon).await;

    rec.get("/settings").await;
    let (status, _) = rec
        .put(
            "/settings",
            json!({ "manager_enabled": true, "review_agent_can_resolve": true }),
        )
        .await;
    assert_eq!(status, 200);
    rec.get("/settings/agent-profiles").await;
    let (status, profile) = rec
        .post(
            "/settings/agent-profiles",
            json!({ "name": "Deep review", "harness": "claude", "model": "opus", "effort": "high" }),
        )
        .await;
    assert_eq!(status, 201, "{profile}");
    let id = profile["id"].as_str().unwrap().to_string();
    rec.alias(&id, "profile");
    rec.put(
        &format!("/settings/agent-profiles/{id}"),
        json!({ "name": "Quick review", "harness": "claude", "effort": "low" }),
    )
    .await;
    rec.get(&format!("/settings/agent-profiles/{id}")).await;
    rec.put(
        "/settings",
        json!({ "agent_choice": { "mode": "profile", "profile_id": id } }),
    )
    .await;
    rec.get(&format!("/settings/agent-profiles/{id}/referents"))
        .await;
    rec.delete("/settings/agent-profiles/default").await;
    rec.put("/settings", json!({ "agent_choice": null })).await;
    rec.delete(&format!("/settings/agent-profiles/{id}")).await;
    rec.get("/settings/sandbox-profiles").await;
    rec.get("/settings").await;

    rec.finish("settings_and_agent_profiles").await;
}

// --- 7. Skills ---------------------------------------------------------------------------------

const SKILL: &str = "---\nname: tdd\ndescription: Test-driven development. Red-green-refactor at pre-agreed seams.\n---\n\n# Test-driven development\n\nRed, green, refactor.\n";

#[tokio::test]
async fn skills_bank_folders_and_files() {
    let daemon = daemon().await;
    let mut rec = Recorder::start(&daemon).await;

    rec.get("/settings/skills").await;
    let (status, folder) = rec
        .post("/settings/skill-folders", json!({ "name": "Craft" }))
        .await;
    assert_eq!(status, 201, "{folder}");
    let folder_id = folder["id"].as_str().unwrap().to_string();
    rec.alias(&folder_id, "folder");
    let (status, skill) = rec
        .post(
            "/settings/skills",
            json!({ "content": SKILL, "folder_id": folder_id }),
        )
        .await;
    assert_eq!(status, 201, "{skill}");
    let id = skill["id"].as_str().unwrap().to_string();
    rec.alias(&id, "skill");
    rec.get(&format!("/settings/skills/{id}")).await;
    let notes = daemon.repo_root().join("notes.md");
    std::fs::write(&notes, "A reference note.\n").unwrap();
    rec.post(
        &format!("/settings/skills/{id}/files"),
        json!({ "from_path": notes }),
    )
    .await;
    rec.put(
        &format!("/settings/skills/{id}/files/notes.md"),
        json!("An edited reference note.\n"),
    )
    .await;
    rec.get(&format!("/settings/skills/{id}/files/notes.md"))
        .await;
    rec.put(
        &format!("/settings/skills/{id}"),
        json!({ "name": "tdd craft" }),
    )
    .await;
    rec.get(&format!("/settings/skills/{id}/referents")).await;
    rec.get("/settings/skill-folders").await;
    rec.delete(&format!("/settings/skills/{id}/files/notes.md"))
        .await;
    rec.delete(&format!("/settings/skills/{id}")).await;
    rec.delete(&format!("/settings/skill-folders/{folder_id}"))
        .await;
    rec.get("/settings/skills").await;

    rec.finish("skills_bank_folders_and_files").await;
}

// --- 8. Projects and provisioning ------------------------------------------------------------

#[tokio::test]
async fn projects_and_provisioning() {
    let daemon = daemon().await;
    let mut rec = Recorder::start(&daemon).await;
    let repo = daemon.target_repo();
    std::fs::write(daemon.repo_root().join(".env"), "SECRET=1\n").unwrap();

    let (status, project) = rec.post("/projects", json!({ "name": "Team" })).await;
    assert_eq!(status, 201, "{project}");
    let id = project["id"].as_str().unwrap().to_string();
    rec.alias(&id, "project");
    rec.post(&format!("/projects/{id}/members"), json!({ "path": repo }))
        .await;
    rec.patch(
        &format!("/projects/{id}"),
        json!({ "name": "Team A", "harness": "claude", "auto_fail": true }),
    )
    .await;
    rec.get("/projects").await;
    rec.put("/settings/provisioning", json!({ "copy": [".env"] }))
        .await;
    rec.put(
        &format!("/projects/{id}/provisioning"),
        json!({ "symlink": ["lib.rs"] }),
    )
    .await;
    rec.get(&format!("/projects/{id}/provisioning")).await;
    rec.get("/settings/provisioning").await;
    rec.post(
        "/repos/provisioning/preview",
        json!({ "repository": repo, "scope": "run", "rules": { "copy": ["!.env"] } }),
    )
    .await;
    rec.get(&format!("/repos/branches?path={repo}")).await;
    rec.get(&format!("/repos/validate?path={repo}")).await;
    rec.call(
        reqwest::Method::DELETE,
        &format!("/projects/{id}/members"),
        Some(json!({ "path": repo })),
    )
    .await;
    rec.delete(&format!("/projects/{id}")).await;
    rec.get("/projects").await;

    rec.finish("projects_and_provisioning").await;
}
