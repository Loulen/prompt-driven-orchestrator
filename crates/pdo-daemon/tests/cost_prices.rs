//! Layer 3 — the three-tier price table and its out-of-band fetch (#427, ADR-0034).
//!
//! Drives a **real daemon** whose price source is a **local fixture server**, so no
//! test reaches models.dev, and whose sandbox home override puts `~/.pdo/prices/`
//! under the test's own tempdir — without that, a sync would write the developer's
//! real `~/.pdo/prices/fetched.json`.
//!
//! What is proven here rather than in unit tests (ADR-0004's règle d'or — an AC is
//! closed at layer ≥ 3):
//!   1. the route is REGISTERED (the anti-SPA gate: content-type, not just status);
//!   2. a sync repairs a `$0` and `GET /runs/:id` shows it **in the same process**,
//!      with no daemon restart;
//!   3. an unreachable source, a drifted schema and an empty harvest each answer
//!      502 and leave `fetched.json` **byte for byte** intact;
//!   4. a second sync with nothing to change answers `noop: true` + `reason`;
//!   5. two concurrent syncs → one 200, one 409;
//!   6. the manual tier wins over the fetched one, and the report **says so**;
//!   7. `GET /settings` names both paths even when neither file exists;
//!   8. the boot refresh never CREATES a cache — no egress before the first click —
//!      refreshes a stale one, and survives an unreachable source;
//!   9. `GET /stats/cost` carries the resolved table (#528): the embedded floor on
//!      a fresh home, a manual override reported `manual`, a fetched family
//!      `fetched`, no `<synthetic>` — the read view beside the "Sync costs" button.

mod common;

use common::TestDaemon;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

const PIPELINE_YAML: &str = r#"name: prices
version: "1.0"
prompt_required: false
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - { name: user_prompt, side: bottom }
  - id: worker
    name: Worker
    type: doc-only
    inputs:
      - { name: in, side: top }
    outputs:
      - { name: out, side: bottom }
  - id: end
    name: End
    type: end
    inputs:
      - { name: result, side: top }
edges:
  - source: { node: start, port: user_prompt }
    target: { node: worker, port: in }
  - source: { node: worker, port: out }
    target: { node: end, port: result }
"#;

/// The real shape of `models.dev/api.json`, trimmed to what the normaliser reads.
/// Three of these rows are the models #427 measured as unpriced ($0 on ~30 % of
/// the corpus); `eu.anthropic` is here to prove regional prices are ignored.
const MODELS_DEV: &str = r#"{
  "anthropic": { "models": {
    "claude-opus-4-8":   { "id": "claude-opus-4-8",   "cost": { "input": 5,  "output": 25 } },
    "claude-opus-5":     { "id": "claude-opus-5",     "cost": { "input": 5,  "output": 25 } },
    "claude-sonnet-5":   { "id": "claude-sonnet-5",   "cost": { "input": 2,  "output": 10 } },
    "claude-fable-5":    { "id": "claude-fable-5",    "cost": { "input": 10, "output": 50 } },
    "claude-haiku-4-5-20251001": { "id": "claude-haiku-4-5-20251001", "cost": { "input": 1, "output": 5 } }
  } },
  "eu.anthropic": { "models": {
    "eu.anthropic.claude-opus-5": { "id": "claude-opus-5", "cost": { "input": 5.5, "output": 27.5 } }
  } }
}"#;

/// The same payload with one price moved, so a second sync has something to write.
const MODELS_DEV_BUMPED: &str = r#"{
  "anthropic": { "models": {
    "claude-opus-5": { "id": "claude-opus-5", "cost": { "input": 6, "output": 30 } }
  } }
}"#;

// --- the fixture source ------------------------------------------------------

/// A local price source. Counts its hits, so a test can assert **zero requests**
/// — the only honest way to prove "no egress before the first click".
struct Fixture {
    addr: SocketAddr,
    hits: Arc<AtomicUsize>,
    _task: tokio::task::JoinHandle<()>,
}

impl Fixture {
    fn url(&self) -> String {
        format!("http://{}/api.json", self.addr)
    }
    fn hits(&self) -> usize {
        self.hits.load(Ordering::SeqCst)
    }
}

/// Serve `body` at `/api.json` on an ephemeral loopback port. Uses axum directly
/// (already a dependency; the crate has neither wiremock nor httpmock).
async fn spawn_fixture(body: &'static str) -> Fixture {
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = hits.clone();
    let app = axum::Router::new().route(
        "/api.json",
        axum::routing::get(move || {
            let counter = counter.clone();
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                ([("content-type", "application/json")], body)
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    Fixture {
        addr,
        hits,
        _task: task,
    }
}

// --- repo / run helpers ------------------------------------------------------

fn tmux_available() -> bool {
    Command::new("tmux")
        .arg("-V")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn seed(repo: &Path) -> anyhow::Result<()> {
    let pipelines = repo.join(".pdo").join("pipelines");
    std::fs::create_dir_all(&pipelines)?;
    std::fs::write(pipelines.join("prices.yaml"), PIPELINE_YAML)?;
    let run = |args: &[&str]| -> anyhow::Result<()> {
        let out = Command::new("git").args(args).current_dir(repo).output()?;
        anyhow::ensure!(out.status.success(), "git {args:?} failed");
        Ok(())
    };
    run(&["init", "-q", "-b", "main"])?;
    run(&["config", "user.email", "t@example.com"])?;
    run(&["config", "user.name", "Test"])?;
    run(&["config", "commit.gpgsign", "false"])?;
    std::fs::write(repo.join(".gitignore"), ".pdo/runs/\n")?;
    run(&["add", "."])?;
    run(&["commit", "-q", "-m", "init"])?;
    Ok(())
}

/// `<home>/.pdo/prices/{models.yaml, fetched.json}` — the same path arithmetic
/// `price_table::paths` does, restated here so the test pins the CONTRACT rather
/// than calling the code under test.
fn manual_path(daemon: &TestDaemon) -> PathBuf {
    daemon.repo_root().join(".pdo/prices/models.yaml")
}
fn fetched_path(daemon: &TestDaemon) -> PathBuf {
    daemon.repo_root().join(".pdo/prices/fetched.json")
}

async fn sync(daemon: &TestDaemon) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{}/settings/cost-prices/sync", daemon.url()))
        .send()
        .await
        .unwrap()
}

fn content_type(resp: &reqwest::Response) -> String {
    resp.headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string()
}

async fn settings(daemon: &TestDaemon) -> serde_json::Value {
    reqwest::get(format!("{}/settings", daemon.url()))
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

/// `GET /stats/cost` over a window wide enough to see everything. The `resolved`
/// price table (#528) rides on this payload, next to the "Sync costs" action in
/// the Stats → Cost tab — window-independent, a property of the price table.
async fn stats_cost(daemon: &TestDaemon) -> serde_json::Value {
    reqwest::get(format!(
        "{}/stats/cost?from=1970-01-01T00:00:00Z&to=2100-01-01T00:00:00Z&bucket=day",
        daemon.url()
    ))
    .await
    .unwrap()
    .json()
    .await
    .unwrap()
}

async fn get_run(daemon: &TestDaemon, run_id: &str) -> serde_json::Value {
    reqwest::get(format!("{}/runs/{run_id}", daemon.url()))
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

async fn start_run(daemon: &TestDaemon) -> String {
    let resp = reqwest::Client::new()
        .post(format!("{}/runs", daemon.url()))
        .json(&serde_json::json!({
            "pipeline": "prices",
            "input": "hello",
            // #470: the target repo is required at the create boundary (ADR-0033).
            "target_repo": daemon.target_repo(),
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "POST /runs should create the run");
    resp.json::<serde_json::Value>().await.unwrap()["run_id"]
        .as_str()
        .unwrap()
        .to_string()
}

/// Plant a transcript for `run_id`'s worktree cwd under the host `~/.claude`
/// (the tempdir home override), priced against `model`.
fn plant_transcript(daemon: &TestDaemon, run_id: &str, model: &str, input: u64) {
    let worktree = daemon
        .repo_root()
        .join(".pdo/runs")
        .join(run_id)
        .join("worktree");
    let enc = pdo_daemon::stale_detector::encode_working_dir(&worktree);
    let proj = daemon.repo_root().join(".claude/projects").join(enc);
    std::fs::create_dir_all(&proj).unwrap();
    std::fs::write(
        proj.join("s.jsonl"),
        format!(
            "{{\"type\":\"assistant\",\"requestId\":\"r1\",\"message\":{{\"id\":\"m1\",\
             \"model\":\"{model}\",\"usage\":{{\"input_tokens\":{input},\"output_tokens\":0}}}}}}\n"
        ),
    )
    .unwrap();
}

/// Back-date a `fetched.json`'s `fetched_at` by `hours`, leaving the rest intact.
fn age_fetched_at(path: &Path, hours: i64) {
    let mut doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let then = chrono::Utc::now() - chrono::Duration::hours(hours);
    doc["fetched_at"] = serde_json::json!(then.to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
    std::fs::write(path, serde_json::to_vec_pretty(&doc).unwrap()).unwrap();
}

// --- 1. the route exists, and the anti-SPA gate ------------------------------

#[tokio::test]
async fn sync_is_registered_and_answers_json() {
    // THE ANTI-SPA GATE (`tests/fs_browse.rs:234-249`). `static_handler` serves
    // index.html for every unmatched path AND every method, so a forgotten
    // `.route(...)` answers 200 + text/html and a status-only assertion goes green.
    let fixture = spawn_fixture(MODELS_DEV).await;
    let daemon = TestDaemon::spawn_with_price_source(seed, fixture.url())
        .await
        .unwrap();

    let resp = sync(&daemon).await;
    assert_eq!(resp.status(), 200);
    let ct = content_type(&resp);
    assert!(
        ct.starts_with("application/json"),
        "route must be registered — the SPA fallback would answer text/html, got {ct}"
    );
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["ok"], true);
    assert_eq!(body["source"], fixture.url());

    // The three models #427 measured as unpriced are now priced.
    let added: Vec<String> = serde_json::from_value(body["added"].clone()).unwrap();
    for k in ["claude-opus-5", "claude-sonnet-5", "claude-fable-5"] {
        assert!(added.contains(&k.to_string()), "added = {added:?}");
    }
    // `claude-opus-4-8` was ALREADY priced by the embedded floor at the same price →
    // unchanged, not added. Merge by key, never replacement.
    assert!(!added.contains(&"claude-opus-4-8".to_string()));
    // The dated source id landed de-dated.
    assert!(
        added.contains(&"claude-haiku-4-5".to_string()) || body["unchanged"].as_u64() > Some(0)
    );

    assert!(
        fetched_path(&daemon).exists(),
        "fetched.json must be written"
    );
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(fetched_path(&daemon)).unwrap()).unwrap();
    assert_eq!(doc["schema"], "prices-v1");
    assert_eq!(doc["models"]["claude-fable-5"]["input"], 10.0);
    // Regional prices must not have leaked in (+10 % on eu.anthropic).
    assert_eq!(doc["models"]["claude-opus-5"]["input"], 5.0);
    assert_eq!(fixture.hits(), 1);
}

// --- 2. the repair is visible in the SAME process ----------------------------

#[tokio::test]
async fn a_sync_reprices_a_live_run_without_restarting_the_daemon() {
    if !tmux_available() {
        eprintln!("tmux not on PATH — skipping");
        return;
    }
    let fixture = spawn_fixture(MODELS_DEV).await;
    let daemon = TestDaemon::spawn_with_price_source(seed, fixture.url())
        .await
        .unwrap();
    let run_id = start_run(&daemon).await;

    // A transcript on a model NO tier prices out of the box: 1 MTok of fable-5.
    plant_transcript(&daemon, &run_id, "claude-fable-5", 1_000_000);

    let before = get_run(&daemon, &run_id).await;
    assert_eq!(
        before["cost"]["usd"].as_f64(),
        Some(0.0),
        "an unpriced model contributes $0: {}",
        before["cost"]
    );
    assert_eq!(
        before["cost"]["partial"], true,
        "and flips the lower-bound flag"
    );
    // #425 AC#4: the offender is NAMED on the payload, not left anonymous — this
    // is the whole point of the re-scoped issue (a `$0 †` you can act on).
    let named: Vec<String> =
        serde_json::from_value(before["cost"]["unpriced_models"].clone()).unwrap();
    assert_eq!(
        named,
        vec!["claude-fable-5".to_string()],
        "GET /runs/:id must name the unpriced model, got {}",
        before["cost"]
    );

    assert_eq!(sync(&daemon).await.status(), 200);

    // Same process, same PID, no restart — just the next read.
    let after = get_run(&daemon, &run_id).await;
    let usd = after["cost"]["usd"].as_f64().unwrap_or(-1.0);
    assert!(
        (usd - 10.0).abs() < 1e-9,
        "1 MTok of claude-fable-5 at $10/MTok = $10 after the sync, got {usd} ({})",
        after["cost"]
    );
    assert_eq!(
        after["cost"]["partial"], false,
        "nothing is unpriced any more"
    );
    let named_after: Vec<String> =
        serde_json::from_value(after["cost"]["unpriced_models"].clone()).unwrap();
    assert!(
        named_after.is_empty(),
        "and no model is named as unpriced any more, got {}",
        after["cost"]
    );
}

// --- 3. every failure mode leaves the last known table intact ----------------

#[tokio::test]
async fn a_dead_port_answers_502_naming_the_url() {
    // 127.0.0.1:9 (discard) is reserved and never bound in this environment: a
    // refused connection, so the assertion is fast and not a timeout.
    let dead = "http://127.0.0.1:9/api.json".to_string();
    let daemon = TestDaemon::spawn_with_price_source(seed, dead.clone())
        .await
        .unwrap();

    let resp = sync(&daemon).await;
    assert_eq!(resp.status(), 502, "a network cut is not a daemon defect");
    let body: serde_json::Value = resp.json().await.unwrap();
    let err = body["error"].as_str().unwrap_or_default();
    assert!(err.contains(&dead), "the error must NAME the source: {err}");
    assert!(
        !fetched_path(&daemon).exists(),
        "a failed fetch must write nothing at all"
    );
}

#[tokio::test]
async fn an_empty_harvest_answers_502_and_leaves_the_table_byte_identical() {
    // The one path by which this feature could DESTROY something: an upstream
    // schema drift writing an empty file over the last known table.
    let good = spawn_fixture(MODELS_DEV).await;
    let daemon = TestDaemon::spawn_with_price_source(seed, good.url())
        .await
        .unwrap();
    assert_eq!(sync(&daemon).await.status(), 200);
    let before = std::fs::read(fetched_path(&daemon)).unwrap();

    // Now stand up a DRIFTED source and point a second daemon at it, sharing
    // nothing but the file we hand-copy in — the cleanest way to prove the guard
    // without mutating a live daemon's config.
    let drifted = spawn_fixture("{}").await;
    let daemon2 = TestDaemon::spawn_with_price_source(seed, drifted.url())
        .await
        .unwrap();
    std::fs::create_dir_all(fetched_path(&daemon2).parent().unwrap()).unwrap();
    std::fs::write(fetched_path(&daemon2), &before).unwrap();

    let resp = sync(&daemon2).await;
    assert_eq!(resp.status(), 502);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(
        body["error"]
            .as_str()
            .unwrap_or_default()
            .contains(&drifted.url()),
        "the error must name the source: {body}"
    );
    assert_eq!(
        std::fs::read(fetched_path(&daemon2)).unwrap(),
        before,
        "byte for byte — an empty harvest must never destroy the last known table"
    );
    // And the table still prices what it used to.
    let s = settings(&daemon2).await;
    assert_eq!(s["price_table"]["fetched_rows"].as_u64(), Some(5));
}

// --- 4. a second sync with nothing to change ---------------------------------

#[tokio::test]
async fn a_second_sync_with_nothing_to_change_is_an_explicit_noop() {
    // ADR-0025: never a blind `{ok:true}`.
    let fixture = spawn_fixture(MODELS_DEV).await;
    let daemon = TestDaemon::spawn_with_price_source(seed, fixture.url())
        .await
        .unwrap();
    assert_eq!(sync(&daemon).await.status(), 200);
    let first = std::fs::read(fetched_path(&daemon)).unwrap();

    let resp = sync(&daemon).await;
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["noop"], true);
    assert!(
        body["reason"]
            .as_str()
            .unwrap_or_default()
            .contains("up to date"),
        "a noop must SAY why: {body}"
    );
    assert_eq!(
        std::fs::read(fetched_path(&daemon)).unwrap(),
        first,
        "a noop rewrites nothing — which also keeps the cost memo warm"
    );
    assert_eq!(
        fixture.hits(),
        2,
        "it did ask; it just had nothing to write"
    );
}

// --- 5. concurrency ----------------------------------------------------------

#[tokio::test]
async fn two_concurrent_syncs_yield_one_200_and_one_409() {
    let fixture = spawn_fixture(MODELS_DEV).await;
    let daemon = TestDaemon::spawn_with_price_source(seed, fixture.url())
        .await
        .unwrap();

    let (a, b) = tokio::join!(sync(&daemon), sync(&daemon));
    let mut codes = [a.status().as_u16(), b.status().as_u16()];
    codes.sort_unstable();
    assert_eq!(
        codes,
        [200, 409],
        "a second click brings nothing, and ADR-0025 wants us to say so"
    );
}

// --- 6. the manual tier wins, and the report says so ------------------------

#[tokio::test]
async fn the_manual_tier_shadows_the_fetched_one_and_the_report_names_it() {
    let fixture = spawn_fixture(MODELS_DEV).await;
    let daemon = TestDaemon::spawn_with_price_source(seed, fixture.url())
        .await
        .unwrap();
    // An enterprise discount, written by hand.
    std::fs::create_dir_all(manual_path(&daemon).parent().unwrap()).unwrap();
    std::fs::write(
        manual_path(&daemon),
        "models:\n  claude-opus-5: { input: 1.0, output: 2.0 }\n",
    )
    .unwrap();

    let resp = sync(&daemon).await;
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let shadowed: Vec<String> = serde_json::from_value(body["shadowed_by_manual"].clone()).unwrap();
    assert!(
        shadowed.contains(&"claude-opus-5".to_string()),
        "a sync must never silently erase a hand correction: {body}"
    );

    // And the hand price is what actually applies.
    let s = settings(&daemon).await;
    let manual_keys: Vec<String> =
        serde_json::from_value(s["price_table"]["manual_keys"].clone()).unwrap();
    assert_eq!(manual_keys, ["claude-opus-5"]);

    // PDO never writes the manual file — one writer per file (ADR-0034).
    assert_eq!(
        std::fs::read_to_string(manual_path(&daemon)).unwrap(),
        "models:\n  claude-opus-5: { input: 1.0, output: 2.0 }\n",
        "the sync must leave the human's file byte for byte"
    );
}

// --- 7. discoverability: the paths are named even when absent ----------------

#[tokio::test]
async fn settings_names_both_price_paths_even_when_no_file_exists() {
    // Nothing is ever seeded (that would freeze a snapshot, ADR-0031 §2), so
    // naming the paths IS the entire discoverability story.
    let fixture = spawn_fixture(MODELS_DEV).await;
    let daemon = TestDaemon::spawn_with_price_source(seed, fixture.url())
        .await
        .unwrap();

    let s = settings(&daemon).await;
    let pt = &s["price_table"];
    assert_eq!(
        pt["manual_path"].as_str(),
        Some(manual_path(&daemon).to_string_lossy().as_ref())
    );
    assert_eq!(
        pt["fetched_path"].as_str(),
        Some(fetched_path(&daemon).to_string_lossy().as_ref())
    );
    assert!(!manual_path(&daemon).exists() && !fetched_path(&daemon).exists());
    // Absent is SILENT: no reason, no vintage, no rows.
    assert_eq!(pt["reason"], serde_json::Value::Null);
    assert_eq!(pt["fetched_at"], serde_json::Value::Null);
    assert_eq!(pt["fetched_rows"].as_u64(), Some(0));
    assert_eq!(fixture.hits(), 0, "reading settings must not egress");
}

// --- 7b. the resolved read view on /stats/cost: winning tier + $/MTok per family (#528) ---
//
// The resolved table rides on `GET /stats/cost` — beside the "Sync costs" action
// in the Stats → Cost tab, so pressing sync and reading what PDO can price happen
// at one endpoint. It reads the SAME `PriceTable` the cost fold bills with, so the
// view can never enumerate a set the pricer would price otherwise (#373).

#[tokio::test]
async fn stats_cost_resolved_lists_the_embedded_floor_on_a_fresh_home() {
    // With no disk tier, `/stats/cost` still exposes what PDO can price: the eleven
    // embedded families, every one flagged `embedded`. `resolved: []` would lie —
    // the const prices even with no HOME state (D9). Window-independent: no runs.
    let fixture = spawn_fixture(MODELS_DEV).await;
    let daemon = TestDaemon::spawn_with_price_source(seed, fixture.url())
        .await
        .unwrap();

    let c = stats_cost(&daemon).await;
    let rows = c["resolved"].as_array().unwrap();
    assert_eq!(
        rows.len(),
        11,
        "the embedded floor is eleven families: {rows:?}"
    );
    assert!(
        rows.iter().all(|r| r["tier"] == "embedded"),
        "every floor row is the embedded tier: {rows:?}"
    );

    let row = |key: &str| rows.iter().find(|r| r["key"] == key).unwrap();
    // The single most error-prone distinction: opus-4-8 at (5,25) ≠ opus-4-1 at (15,75).
    assert_eq!(row("claude-opus-4-8")["input"].as_f64(), Some(5.0));
    assert_eq!(row("claude-opus-4-8")["output"].as_f64(), Some(25.0));
    assert_eq!(row("claude-opus-4-1")["input"].as_f64(), Some(15.0));
    assert_eq!(row("claude-opus-4-1")["output"].as_f64(), Some(75.0));

    // The sentinel is a `price_for` short-circuit, never a table row.
    assert!(rows.iter().all(|r| r["key"] != "<synthetic>"));
    assert_eq!(fixture.hits(), 0, "reading /stats/cost must not egress");
}

#[tokio::test]
async fn stats_cost_resolved_reports_a_manual_override_as_manual() {
    let fixture = spawn_fixture(MODELS_DEV).await;
    let daemon = TestDaemon::spawn_with_price_source(seed, fixture.url())
        .await
        .unwrap();
    // A hand-written enterprise discount on a floor family.
    std::fs::create_dir_all(manual_path(&daemon).parent().unwrap()).unwrap();
    std::fs::write(
        manual_path(&daemon),
        "models:\n  claude-opus-4-8: { input: 4.5, output: 22.5 }\n",
    )
    .unwrap();

    let c = stats_cost(&daemon).await;
    let rows = c["resolved"].as_array().unwrap();
    let row = rows.iter().find(|r| r["key"] == "claude-opus-4-8").unwrap();
    assert_eq!(row["tier"], "manual");
    assert_eq!(row["input"].as_f64(), Some(4.5));
    assert_eq!(row["output"].as_f64(), Some(22.5));

    // The two endpoints must agree — a resolved `manual` row here is also named in
    // `GET /settings`'s `manual_keys`, the other signal that a tier shadows a sync.
    let s = settings(&daemon).await;
    let manual_keys: Vec<String> =
        serde_json::from_value(s["price_table"]["manual_keys"].clone()).unwrap();
    assert!(
        manual_keys.contains(&"claude-opus-4-8".to_string()),
        "manual_keys and the resolved tier must not disagree: {manual_keys:?}"
    );
}

#[tokio::test]
async fn stats_cost_resolved_reports_a_fetched_family_as_fetched() {
    let fixture = spawn_fixture(MODELS_DEV).await;
    let daemon = TestDaemon::spawn_with_price_source(seed, fixture.url())
        .await
        .unwrap();
    // The click IS the sync: the button lives on the Cost tab, next to this view.
    assert_eq!(sync(&daemon).await.status(), 200);

    let c = stats_cost(&daemon).await;
    let rows = c["resolved"].as_array().unwrap();
    // `claude-opus-5` is fetch-only (absent from the embedded floor) → Fetched decides.
    let row = rows.iter().find(|r| r["key"] == "claude-opus-5").unwrap();
    assert_eq!(row["tier"], "fetched");
    assert_eq!(row["input"].as_f64(), Some(5.0));
    assert_eq!(row["output"].as_f64(), Some(25.0));
    // Still no sentinel, even after a sync.
    assert!(rows.iter().all(|r| r["key"] != "<synthetic>"));
}

#[tokio::test]
async fn a_refused_row_is_inert_and_reported_but_never_fails_a_read() {
    let fixture = spawn_fixture(MODELS_DEV).await;
    let daemon = TestDaemon::spawn_with_price_source(seed, fixture.url())
        .await
        .unwrap();
    std::fs::create_dir_all(manual_path(&daemon).parent().unwrap()).unwrap();
    // A dated key: the single most likely mistake, since both natural sources (the
    // pricing page and the transcripts) give the dated form.
    std::fs::write(
        manual_path(&daemon),
        "models:\n  claude-opus-5-20260501: { input: 5.0, output: 25.0 }\n",
    )
    .unwrap();

    let s = settings(&daemon).await;
    let reason = s["price_table"]["reason"].as_str().unwrap_or_default();
    assert!(
        reason.contains("claude-opus-5"),
        "the refusal must be readable in the UI, and print the correct form: {reason}"
    );
    assert_eq!(
        s["price_table"]["manual_keys"].as_array().map(Vec::len),
        Some(0),
        "a refused row prices nothing"
    );

    // And a cost read still answers 200 — a bad price file can never fail one.
    let resp = reqwest::get(format!(
        "{}/stats/cost?from=1970-01-01T00:00:00Z&to=2100-01-01T00:00:00Z&bucket=day",
        daemon.url()
    ))
    .await
    .unwrap();
    assert_eq!(resp.status(), 200);
}

// --- 8. the boot refresh: refreshes, never seeds -----------------------------

#[tokio::test]
async fn the_boot_refresh_never_creates_a_cache() {
    // ADR-0034: no egress before the user has clicked once. The click IS the
    // consent, so an instance that has never synced must never reach the network —
    // even with the flag armed, which `spawn_with_price_source` sets to `true`.
    let fixture = spawn_fixture(MODELS_DEV).await;
    let daemon = TestDaemon::spawn_with_price_source(seed, fixture.url())
        .await
        .unwrap();
    assert!(!fetched_path(&daemon).exists());

    daemon.run_price_refresh_tick().await;

    assert_eq!(
        fixture.hits(),
        0,
        "an absent cache must produce ZERO requests"
    );
    assert!(!fetched_path(&daemon).exists(), "and seed nothing");
}

#[tokio::test]
async fn the_boot_refresh_is_a_noop_on_a_fresh_cache() {
    let fixture = spawn_fixture(MODELS_DEV).await;
    let daemon = TestDaemon::spawn_with_price_source(seed, fixture.url())
        .await
        .unwrap();
    assert_eq!(sync(&daemon).await.status(), 200);
    assert_eq!(fixture.hits(), 1);

    daemon.run_price_refresh_tick().await;
    assert_eq!(fixture.hits(), 1, "a cache under 24 h old is left alone");
}

#[tokio::test]
async fn the_boot_refresh_refetches_a_stale_cache_and_rewrites_it() {
    let fixture = spawn_fixture(MODELS_DEV_BUMPED).await;
    let daemon = TestDaemon::spawn_with_price_source(seed, fixture.url())
        .await
        .unwrap();
    assert_eq!(sync(&daemon).await.status(), 200);
    assert_eq!(fixture.hits(), 1);

    // Hand-write a table the refresh will have to replace, and age it past 24 h.
    let path = fetched_path(&daemon);
    let mut doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    doc["models"]["claude-opus-5"]["input"] = serde_json::json!(1.0);
    std::fs::write(&path, serde_json::to_vec_pretty(&doc).unwrap()).unwrap();
    age_fetched_at(&path, 25);

    daemon.run_price_refresh_tick().await;

    assert_eq!(fixture.hits(), 2, "a cache over 24 h old is refreshed");
    let after: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(
        after["models"]["claude-opus-5"]["input"], 6.0,
        "the refreshed table must be the source's: {after}"
    );
}

#[tokio::test]
async fn an_unreachable_source_at_boot_leaves_the_table_and_the_daemon_alive() {
    // `boot_recovery.rs:161-168`'s regime: a warn, never fatal. A daemon restarted
    // by systemd can legitimately come up before DNS is ready.
    let good = spawn_fixture(MODELS_DEV).await;
    let daemon = TestDaemon::spawn_with_price_source(seed, good.url())
        .await
        .unwrap();
    assert_eq!(sync(&daemon).await.status(), 200);
    let before = std::fs::read(fetched_path(&daemon)).unwrap();

    // A second daemon whose source is dead, carrying the same stale cache.
    let daemon2 = TestDaemon::spawn_with_price_source(seed, "http://127.0.0.1:9/api.json".into())
        .await
        .unwrap();
    std::fs::create_dir_all(fetched_path(&daemon2).parent().unwrap()).unwrap();
    std::fs::write(fetched_path(&daemon2), &before).unwrap();
    age_fetched_at(&fetched_path(&daemon2), 25);
    let stale = std::fs::read(fetched_path(&daemon2)).unwrap();

    daemon2.run_price_refresh_tick().await;

    assert_eq!(
        std::fs::read(fetched_path(&daemon2)).unwrap(),
        stale,
        "the table survives an unreachable source, byte for byte"
    );
    // The daemon is still serving.
    let s = settings(&daemon2).await;
    assert_eq!(s["price_table"]["fetched_rows"].as_u64(), Some(5));
}
