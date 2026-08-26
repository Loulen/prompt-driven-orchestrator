//! Layer 3a — staging profiles (#432, slice K of PRD #403; ADR-0031 §2-§7).
//!
//! Same harness as `sandbox_tracer`: a **real daemon**, a **fake `docker`** (via
//! `docker_cmd_override`) and a tempdir-scoped sandbox home (via
//! `sandbox_home_override`, which moves the `.claude` SOURCE, the staging ROOT *and* the
//! container's `HOME` in one go). No test needs Docker, touches the real `$HOME`, or
//! launches real claude.
//!
//! What is pinned here, one acceptance criterion per test:
//!  1. a profile with `.claude/plugins` unchecked, assigned to a **Trigger**, stages
//!     without `plugins/` (AC1);
//!  2. `.gitconfig` as an extra is **copied** into `<staging>/home/` and bind-mounted at
//!     `$HOME/.gitconfig` (AC2), and the host file is never a mount SOURCE (AC3);
//!  3. an entry under `.claude/` produces **no** extra `-v` (AC4);
//!  4. an absolute / `..` / floor-owned / glob entry is a **400 at write** (AC5);
//!  5. editing a profile after a Run started does not change its staging, and a boot
//!     recovery replays the **frozen** list identically (AC6);
//!  6. an unknown profile fails hard at create, at a Trigger fire, and at boot recovery —
//!     never silently `off` or on another profile (AC7);
//!  7. unedited `full`/`minimal` have **no row**; editing materialises a **diff**, so a
//!     default enriched by a later version stays visible (AC8);
//!  8. unchecking `.claude/settings.json` still starts (the floor re-synthesises it, AC9);
//!  9. deleting a referenced profile can list its referents first (AC10).
//!
//! ## #468 — per-profile environment
//!
//! Four more, at the bottom of the file:
//!  10. a profile's `env` lands as `-e KEY=value` at `docker create`, and a profile
//!      **without** env poses nothing extra (the negative control is mandatory: asserting
//!      only the positive half would pass against a fake that echoed its own argv);
//!  11. a PUT naming `HOME` / `PDO_DAEMON_URL` / `PDO_RUN_ID` is a **400 naming the key**;
//!  12. editing the env of a **multi-node** Run in flight changes nothing for its NEXT
//!      node — a single-node Run would prove nothing, since the container is created once;
//!  13. an unreadable frozen `sandbox_env` is a hard `RunFailed`, never a silent "no env".
//!
//! ## #467 — per-profile image source
//!
//! Five more, at the very bottom:
//!  14. two profiles, one on a Dockerfile and one on an explicit registry ref, put two
//!      concurrent Runs in **two different images** — the layer-3 stand-in for
//!      `docker inspect --format '{{.Config.Image}}'` being the image arg of `docker create`;
//!  15. a profile that poses **nothing** keeps the content-addressed tag and grows the payload by
//!      not one key (AC4's layer-3 half; the byte-identical argv is pinned in `sandbox_run`);
//!  16. an unreachable explicit ref fails the Run with a reason **naming the ref**, and launches
//!      **no `docker build`** (AC3);
//!  17. editing a **multi-node** Run's profile image changes nothing for its next node (AC2);
//!  18. the write-time refusals: a relative path, a path that does not exist, an unknown `kind`,
//!      a ref starting with `-`.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use crate::common::{ensure_pdo_on_path, TestDaemon};
use tempfile::TempDir;

const NODE_ID: &str = "notify";

/// `start → notify(script, output `out`) → end` — same shape as the tracer's, so
/// completing `notify` drives the whole run terminal.
const PIPELINE_YAML: &str = r#"name: sbx-cycle
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: notify
    name: notify
    type: script
    outputs:
      - name: out
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: notify, port: in }
  - source: { node: notify, port: out }
    target: { node: end, port: result }
"#;

fn git_init_with_commit(repo: &Path) -> anyhow::Result<()> {
    let run = |args: &[&str]| -> anyhow::Result<()> {
        let out = Command::new("git").args(args).current_dir(repo).output()?;
        if !out.status.success() {
            anyhow::bail!(
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        Ok(())
    };
    run(&["init", "-q", "-b", "main"])?;
    run(&["config", "user.email", "test@example.com"])?;
    run(&["config", "user.name", "Test"])?;
    run(&["config", "commit.gpgsign", "false"])?;
    std::fs::write(repo.join(".gitignore"), ".pdo/runs/\n")?;
    run(&["add", "."])?;
    run(&["commit", "-q", "-m", "init"])?;
    Ok(())
}

/// `start → notify → notify2 → end` (#468). A SECOND node is what makes "editing the env
/// mid-Run does not change the next node" a real assertion: with one node the container is
/// created once and nothing could have changed anyway.
const PIPELINE_TWO_NODES_YAML: &str = r#"name: sbx-two
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: notify
    name: notify
    type: script
    outputs:
      - name: out
  - id: notify2
    name: notify2
    type: script
    outputs:
      - name: out
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: notify, port: in }
  - source: { node: notify, port: out }
    target: { node: notify2, port: in }
  - source: { node: notify2, port: out }
    target: { node: end, port: result }
"#;

const PIPELINE_NAME: &str = "sbx-cycle";
const TWO_NODE_PIPELINE_NAME: &str = "sbx-two";
const NODE_ID_2: &str = "notify2";

/// Write one pipeline yaml + a trivial prompt per script node.
fn write_pipeline(repo: &Path, name: &str, yaml: &str, nodes: &[&str]) -> anyhow::Result<()> {
    let pipelines_dir = repo.join(".pdo").join("pipelines");
    std::fs::create_dir_all(&pipelines_dir)?;
    std::fs::write(pipelines_dir.join(format!("{name}.yaml")), yaml)?;
    let prompts_dir = pipelines_dir.join(format!("{name}.prompts"));
    std::fs::create_dir_all(&prompts_dir)?;
    for node in nodes {
        std::fs::write(
            prompts_dir.join(format!("{node}.md")),
            "#!/usr/bin/env bash\ntrue\n",
        )?;
    }
    Ok(())
}

fn seed() -> impl FnOnce(&Path) -> anyhow::Result<()> {
    move |repo: &Path| {
        write_pipeline(repo, PIPELINE_NAME, PIPELINE_YAML, &[NODE_ID])?;
        git_init_with_commit(repo)?;
        Ok(())
    }
}

/// Both pipelines, so a test can pick the two-node one without a second harness (#468).
fn seed_with_two_node_pipeline() -> impl FnOnce(&Path) -> anyhow::Result<()> {
    move |repo: &Path| {
        write_pipeline(repo, PIPELINE_NAME, PIPELINE_YAML, &[NODE_ID])?;
        write_pipeline(
            repo,
            TWO_NODE_PIPELINE_NAME,
            PIPELINE_TWO_NODES_YAML,
            &[NODE_ID, NODE_ID_2],
        )?;
        git_init_with_commit(repo)?;
        Ok(())
    }
}

/// Fake `docker`: logs argv, `image inspect` → present, `container inspect` → ABSENT
/// (so `ensure_running` does `create` + `start`, which is what makes the mount queue
/// observable), everything else exit 0.
fn write_fake_docker() -> (TempDir, String, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("fake-docker");
    let log = dir.path().join("argv.log");
    let sq = |s: &str| format!("'{}'", s.replace('\'', "'\\''"));
    let script = format!(
        "#!/usr/bin/env bash\n\
         printf '%s\\n' \"$@\" >> {log}\n\
         case \"$1\" in\n\
         image) exit 0 ;;\n\
         container) printf '%s' 'Error: No such container' >&2; exit 1 ;;\n\
         *) exit 0 ;;\n\
         esac\n",
        log = sq(&log.display().to_string()),
    );
    std::fs::write(&bin, script).unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    (dir, bin.to_str().unwrap().to_string(), log)
}

/// Like [`write_fake_docker`] but the image is **absent** and `pull` **fails** (#467) — the
/// unreachable-ref case. `build` still exits 0, so "no build was launched" is a real signal: were
/// a build attempted, it would SUCCEED and the Run would start.
fn write_fake_docker_failing_pull() -> (TempDir, String, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("fake-docker");
    let log = dir.path().join("argv.log");
    let sq = |s: &str| format!("'{}'", s.replace('\'', "'\\''"));
    let script = format!(
        "#!/usr/bin/env bash\n\
         printf '%s\\n' \"$@\" >> {log}\n\
         case \"$1\" in\n\
         image) exit 1 ;;\n\
         pull) printf '%s' 'Error response from daemon: manifest unknown' >&2; exit 1 ;;\n\
         container) printf '%s' 'Error: No such container' >&2; exit 1 ;;\n\
         *) exit 0 ;;\n\
         esac\n",
        log = sq(&log.display().to_string()),
    );
    std::fs::write(&bin, script).unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    (dir, bin.to_str().unwrap().to_string(), log)
}

fn log_text(log: &Path) -> String {
    std::fs::read_to_string(log).unwrap_or_default()
}

/// Every arg that follows `flag` in the fake-docker argv log.
fn args_after(log: &Path, flag: &str) -> Vec<String> {
    let lines: Vec<String> = log_text(log).lines().map(str::to_string).collect();
    let mut specs = Vec::new();
    let mut iter = lines.iter();
    while let Some(line) = iter.next() {
        if line == flag {
            if let Some(spec) = iter.next() {
                specs.push(spec.clone());
            }
        }
    }
    specs
}

/// Every `-v` mount spec in the fake-docker argv log.
fn mount_specs(log: &Path) -> Vec<String> {
    args_after(log, "-v")
}

/// Every `-e KEY=VALUE` in the fake-docker argv log (#468) — the layer-3 stand-in for
/// `docker inspect --format '{{.Config.Env}}'`, which needs a real daemon.
///
/// Covers `create` AND every `exec`: that is deliberate, because "the profile env must not
/// leak into the per-node `docker exec`" is as much a property as "it must reach the
/// create". Tests that care about one or the other filter on the key.
fn env_specs(log: &Path) -> Vec<String> {
    args_after(log, "-e")
}

/// How many `docker create` invocations the fake logged.
fn create_count(log: &Path) -> usize {
    log_text(log).lines().filter(|l| *l == "create").count()
}

/// The image ref of every `docker create` in the log (#467) — the layer-3 stand-in for
/// `docker inspect --format '{{.Config.Image}}'`, which needs a real daemon.
///
/// Keyed on the argv shape `create … <image> sleep infinity`: the image is the arg right before
/// `sleep`. That is not a coincidence to be defended in a comment — Docker *forces* it (after the
/// image, everything is the command), and `sandbox_container`'s golden test pins it.
fn create_images(log: &Path) -> Vec<String> {
    let lines: Vec<String> = log_text(log).lines().map(str::to_string).collect();
    lines
        .iter()
        .enumerate()
        .filter(|(_, l)| *l == "sleep")
        .filter_map(|(i, _)| i.checked_sub(1).and_then(|p| lines.get(p)).cloned())
        .collect()
}

/// A realistic host `$HOME`: a `~/.claude` with the allowlist content, plus the two
/// **`$HOME` exception** fixtures this slice exists for — `~/.gitconfig` (the identity a
/// committing agent needs) and `~/.config/gh` (a multi-segment DIRECTORY entry, which is
/// the case that proves the `.config` parent is not created root-owned).
fn fabricate_host_home(home: &Path) {
    let claude = home.join(".claude");
    let write = |p: PathBuf, c: &str| {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, c).unwrap();
    };
    write(claude.join("skills/foo/skill.md"), "# skill\n");
    write(claude.join("plugins/bar/plugin.json"), "{}\n");
    write(
        claude.join("plugins/bar/node_modules/big/index.js"),
        "// bulk\n",
    );
    write(claude.join("agents/a.md"), "agent\n");
    write(claude.join("commands/c.md"), "cmd\n");
    write(claude.join("output-styles/s.md"), "style\n");
    write(claude.join("settings.json"), r#"{"hooks":{"Stop":[]}}"#);
    write(claude.join("settings.local.json"), r#"{"local":true}"#);
    write(claude.join("CLAUDE.md"), "# global\n");
    std::fs::create_dir_all(claude.join(".credentials.json").parent().unwrap()).unwrap();
    std::fs::write(claude.join(".credentials.json"), r#"{"token":"secret"}"#).unwrap();
    std::fs::set_permissions(
        claude.join(".credentials.json"),
        std::fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    write(
        home.join(".claude.json"),
        r#"{"host":"profile","oauthAccount":{"x":1}}"#,
    );
    // -- the `$HOME` exception fixtures (#432) --
    write(home.join(".gitconfig"), HOST_GITCONFIG);
    write(
        home.join(".config/gh/hosts.yml"),
        "github.com:\n  user: me\n",
    );
}

const HOST_GITCONFIG: &str = "[user]\n\tname = Host User\n\temail = host@example.com\n";

fn staging_root(daemon: &TestDaemon, run_id: &str) -> PathBuf {
    daemon.repo_root().join(".pdo/sandbox").join(run_id)
}

fn staged_home(daemon: &TestDaemon, run_id: &str) -> PathBuf {
    staging_root(daemon, run_id).join("claude-home")
}

/// `<staging>/home` — the `$HOME`-exception copies.
fn staged_extras(daemon: &TestDaemon, run_id: &str) -> PathBuf {
    staging_root(daemon, run_id).join("home")
}

async fn wait_until<F>(mut pred: F) -> bool
where
    F: FnMut() -> bool,
{
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if pred() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    pred()
}

async fn get_run(daemon: &TestDaemon, run_id: &str) -> serde_json::Value {
    reqwest::get(format!("{}/runs/{run_id}", daemon.url()))
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap()
}

async fn wait_node_status(daemon: &TestDaemon, run_id: &str, expected: &str) -> serde_json::Value {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut last = serde_json::Value::Null;
    while Instant::now() < deadline {
        let run = get_run(daemon, run_id).await;
        if run["nodes"][NODE_ID]["status"] == expected {
            return run;
        }
        last = run;
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    last
}

async fn wait_run_status(daemon: &TestDaemon, run_id: &str, expected: &str) -> serde_json::Value {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut last = serde_json::Value::Null;
    while Instant::now() < deadline {
        let run = get_run(daemon, run_id).await;
        if run["status"] == expected {
            return run;
        }
        last = run;
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    last
}

/// #431 trap, worth restating: a Run's failure `reason` lives on `GET /runs/<id>/events`,
/// NOT on `GET /runs/<id>`.
async fn run_events(daemon: &TestDaemon, run_id: &str) -> Vec<serde_json::Value> {
    reqwest::get(format!("{}/runs/{run_id}/events", daemon.url()))
        .await
        .unwrap()
        .json::<Vec<serde_json::Value>>()
        .await
        .unwrap()
}

/// Whether any `run_failed` event blames the **sandbox prep**.
///
/// Deliberately narrower than "the Run failed": this harness pins the tmux tail to
/// `exec true`, so the node's session collapses instantly and a boot-recovery tick
/// legitimately marks the orphaned node Failed. That is the ORPHAN arm doing its job and
/// has nothing to do with the staging — the question these tests ask is whether the
/// *sandbox* arm fired.
async fn sandbox_prep_failure(daemon: &TestDaemon, run_id: &str) -> Option<String> {
    run_events(daemon, run_id)
        .await
        .iter()
        .filter(|e| e["kind"] == "run_failed")
        .filter_map(|e| e["payload"]["reason"].as_str().map(str::to_string))
        .find(|r| r.contains("sandbox prep"))
}

async fn post_run(daemon: &TestDaemon, sandbox: Option<&str>) -> reqwest::Response {
    post_run_of(daemon, PIPELINE_NAME, sandbox).await
}

async fn post_run_of(
    daemon: &TestDaemon,
    pipeline: &str,
    sandbox: Option<&str>,
) -> reqwest::Response {
    // #470: the target repo is required at the create boundary (ADR-0033).
    let mut body = serde_json::json!({
        "pipeline": pipeline,
        "input": "hello",
        "target_repo": daemon.target_repo(),
    });
    if let Some(mode) = sandbox {
        body["sandbox"] = serde_json::json!(mode);
    }
    reqwest::Client::new()
        .post(format!("{}/runs", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap()
}

async fn start_run(daemon: &TestDaemon, sandbox: Option<&str>) -> String {
    start_run_of(daemon, PIPELINE_NAME, sandbox).await
}

async fn start_run_of(daemon: &TestDaemon, pipeline: &str, sandbox: Option<&str>) -> String {
    let resp = post_run_of(daemon, pipeline, sandbox).await;
    assert_eq!(resp.status(), 201, "POST /runs should create the run");
    resp.json::<serde_json::Value>().await.unwrap()["run_id"]
        .as_str()
        .unwrap()
        .to_string()
}

/// `PUT /settings/sandbox-profiles/<name>` with an explicit diff.
async fn put_profile(
    daemon: &TestDaemon,
    name: &str,
    disabled: &[&str],
    extras: &[&str],
) -> reqwest::Response {
    put_profile_full(daemon, name, disabled, extras, &[]).await
}

/// Like [`put_profile`] but with an `env` map too (#468). The body is a FULL replacement,
/// so an empty slice here really does mean "no environment".
async fn put_profile_full(
    daemon: &TestDaemon,
    name: &str,
    disabled: &[&str],
    extras: &[&str],
    env: &[(&str, &str)],
) -> reqwest::Response {
    let env_obj: serde_json::Map<String, serde_json::Value> = env
        .iter()
        .map(|(k, v)| (k.to_string(), serde_json::json!(v)))
        .collect();
    put_profile_body(
        daemon,
        name,
        serde_json::json!({
            "disabled": disabled,
            "extras": extras,
            "env": env_obj,
        }),
    )
    .await
}

/// `PUT /settings/sandbox-profiles/<name>` with a hand-written body — the seam the #467 tests
/// need, since `image` is a tagged object rather than a flat list and a malformed one is half the
/// point.
async fn put_profile_body(
    daemon: &TestDaemon,
    name: &str,
    body: serde_json::Value,
) -> reqwest::Response {
    reqwest::Client::new()
        .put(format!("{}/settings/sandbox-profiles/{name}", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap()
}

/// `PUT` a profile whose only interesting field is its image source (#467).
async fn put_profile_image(
    daemon: &TestDaemon,
    name: &str,
    image: serde_json::Value,
) -> reqwest::Response {
    put_profile_body(
        daemon,
        name,
        serde_json::json!({ "disabled": [], "extras": [], "env": {}, "image": image }),
    )
    .await
}

/// A self-contained Dockerfile VARIANT on disk, whose filename drives the image NAME (#466) and
/// whose bytes drive the tag. Returns `(path, expected local ref)` — the expected ref is computed
/// the way the daemon computes it, in the one place a test may duplicate it: `sha256[..12]` of the
/// exact bytes, which is also the CI-canonical form (`sha256sum | cut -c1-12`).
fn write_variant_dockerfile(dir: &Path, variant: &str, marker: &str) -> (PathBuf, String) {
    let path = dir.join(format!("Dockerfile.{variant}"));
    let bytes = format!("FROM ubuntu:24.04\nRUN echo {marker}\n");
    std::fs::write(&path, &bytes).unwrap();
    let digest = <sha2::Sha256 as sha2::Digest>::digest(bytes.as_bytes());
    let hash: String = format!("{digest:x}")[..12].to_string();
    (path, format!("pdo-sandbox-{variant}:h-{hash}"))
}

async fn get_profile(daemon: &TestDaemon, name: &str) -> reqwest::Response {
    reqwest::get(format!("{}/settings/sandbox-profiles/{name}", daemon.url()))
        .await
        .unwrap()
}

async fn get_settings(daemon: &TestDaemon) -> serde_json::Value {
    reqwest::get(format!("{}/settings", daemon.url()))
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

async fn put_default_sandbox(daemon: &TestDaemon, value: &str) -> reqwest::Response {
    reqwest::Client::new()
        .put(format!("{}/settings", daemon.url()))
        .json(&serde_json::json!({ "default_sandbox": value }))
        .send()
        .await
        .unwrap()
}

/// Create a Trigger with a `sandbox` value, due far in the future (the test forces it).
async fn create_trigger(daemon: &TestDaemon, name: &str, sandbox: &str) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{}/triggers", daemon.url()))
        // #470: a Trigger is a Run template — no target repo, no Trigger (ADR-0033).
        .json(&serde_json::json!({
            "name": name,
            "pipeline_id": "sbx-cycle",
            "cron": "0 4 * * *",
            "input_template": "from the trigger",
            "sandbox": sandbox,
            "target_repo": daemon.target_repo(),
        }))
        .send()
        .await
        .unwrap()
}

async fn fire_history(daemon: &TestDaemon, trigger_id: &str) -> Vec<serde_json::Value> {
    let body: serde_json::Value =
        reqwest::get(format!("{}/triggers/{trigger_id}/fires", daemon.url()))
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
    body.as_array()
        .cloned()
        .or_else(|| body["fires"].as_array().cloned())
        .unwrap_or_default()
}

fn write_node_output(daemon: &TestDaemon, run_id: &str, content: &str) {
    write_node_output_for(daemon, run_id, NODE_ID, content);
}

fn write_node_output_for(daemon: &TestDaemon, run_id: &str, node_id: &str, content: &str) {
    let out = daemon
        .repo_root()
        .join(".pdo/runs")
        .join(run_id)
        .join("worktree/.pdo/artifacts")
        .join(node_id)
        .join("iter-1/out/output.md");
    std::fs::create_dir_all(out.parent().unwrap()).unwrap();
    std::fs::write(&out, content).unwrap();
}

async fn simulate_node_done(daemon: &TestDaemon, run_id: &str) {
    simulate_node_done_for(daemon, run_id, NODE_ID).await;
}

async fn simulate_node_done_for(daemon: &TestDaemon, run_id: &str, node_id: &str) {
    let resp = reqwest::Client::new()
        .post(format!(
            "{}/runs/{run_id}/nodes/{node_id}/done",
            daemon.url()
        ))
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();
    assert!(
        resp.status().is_success(),
        "simulated pdo complete should succeed: {}",
        resp.status()
    );
}

async fn wait_node_status_for(
    daemon: &TestDaemon,
    run_id: &str,
    node_id: &str,
    expected: &str,
) -> serde_json::Value {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut last = serde_json::Value::Null;
    while Instant::now() < deadline {
        let run = get_run(daemon, run_id).await;
        if run["nodes"][node_id]["status"] == expected {
            return run;
        }
        last = run;
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    last
}

// -- AC1: a profile with plugins unchecked, on a Trigger -----------------------

/// The headline acceptance criterion. `full-sans-plugins` (diff
/// `disabled: [".claude/plugins"]`) is assigned to a **Trigger**; firing it stages
/// everything else and NOT `plugins/` — an order of magnitude less on disk, since
/// `plugins/*/node_modules` is the measured cost centre.
#[tokio::test]
async fn a_profile_with_plugins_unchecked_stages_without_them_through_a_trigger() {
    ensure_pdo_on_path();
    let (_fake, docker, _log) = write_fake_docker();
    let daemon = TestDaemon::spawn_with_docker_override(seed(), docker)
        .await
        .unwrap();
    fabricate_host_home(daemon.repo_root());

    let resp = put_profile(&daemon, "full-sans-plugins", &[".claude/plugins"], &[]).await;
    assert_eq!(resp.status(), 200, "the profile must be accepted");
    let view: serde_json::Value = resp.json().await.unwrap();
    assert!(
        !view["resolved"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e == ".claude/plugins"),
        "resolved list must drop the unchecked entry: {view}"
    );

    let trig = create_trigger(&daemon, "nightly", "full-sans-plugins").await;
    assert_eq!(trig.status(), 201, "the Trigger must accept the profile");
    let trigger_id = trig.json::<serde_json::Value>().await.unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    daemon.force_trigger_due(&trigger_id).await;
    daemon.run_trigger_tick().await;

    // The fired Run is the only one; find it through the fire history.
    let fires = fire_history(&daemon, &trigger_id).await;
    let run_id = fires
        .iter()
        .find_map(|f| f["run_id"].as_str())
        .unwrap_or_else(|| panic!("the fire must have produced a Run: {fires:?}"))
        .to_string();

    let run = get_run(&daemon, &run_id).await;
    assert_eq!(
        run["sandbox"], "full-sans-plugins",
        "the Run must carry the profile NAME: {run}"
    );
    // The frozen list is on the Run too — that is what makes an edit non-retroactive.
    let frozen: Vec<String> = run["sandbox_entries"]
        .as_array()
        .unwrap_or_else(|| panic!("the Run must freeze its entry list: {run}"))
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert!(!frozen.iter().any(|e| e == ".claude/plugins"));
    assert!(frozen.iter().any(|e| e == ".claude/skills"));

    wait_node_status(&daemon, &run_id, "running").await;
    let home = staged_home(&daemon, &run_id);
    assert!(
        wait_until(|| home.join("settings.json").is_file()).await,
        "staging should be seeded once the node is running"
    );
    assert!(
        !home.join("plugins").exists(),
        "the unchecked entry must NOT be staged"
    );
    // Everything else still is — the diff removed one line, not the profile.
    assert!(home.join("skills/foo/skill.md").is_file());
    assert!(home.join("agents/a.md").is_file());
    assert!(home.join("CLAUDE.md").is_file());
}

// -- AC2 + AC3: the `$HOME` exception mounts ---------------------------------

/// `.gitconfig` as an extra: the host file is **copied** into `<staging>/home/` and
/// bind-mounted rw at `$HOME/.gitconfig`, so a `git commit` inside the container runs
/// under the host identity. The negative half is what ADR-0031 §4 exists for: the host
/// path is never a mount SOURCE, so no container write can reach it.
///
/// A multi-segment DIRECTORY entry (`.config/gh`) rides along, because it is the case
/// where a missing source would have Docker create `<staging>/home/.config` root-owned
/// and leave the staging permanently undeletable (mount rule M1).
#[tokio::test]
async fn an_extra_is_copied_into_the_staging_and_mounted_never_bound_from_the_host() {
    ensure_pdo_on_path();
    let (_fake, docker, log) = write_fake_docker();
    let daemon = TestDaemon::spawn_with_docker_override(seed(), docker)
        .await
        .unwrap();
    let host = daemon.repo_root();
    fabricate_host_home(host);

    assert_eq!(
        put_profile(&daemon, "with-git", &[], &[".gitconfig", ".config/gh"])
            .await
            .status(),
        200
    );

    let run_id = start_run(&daemon, Some("with-git")).await;
    assert!(
        wait_until(|| log_text(&log).contains("create")).await,
        "prep must `docker create`; log:\n{}",
        log_text(&log)
    );

    // (a) COPIED, not referenced: the content matches the host, at the staged path.
    let staged_git = staged_extras(&daemon, &run_id).join(".gitconfig");
    assert!(
        wait_until(|| staged_git.is_file()).await,
        "the extra must be copied to {}",
        staged_git.display()
    );
    assert_eq!(
        std::fs::read_to_string(&staged_git).unwrap(),
        HOST_GITCONFIG
    );
    assert!(staged_extras(&daemon, &run_id)
        .join(".config/gh/hosts.yml")
        .is_file());

    // (b) MOUNTED rw at the container's `$HOME/<rel>` (host_home == repo_root here).
    let specs = mount_specs(&log);
    let expect_git = format!(
        "{}:{}:rw",
        staged_git.display(),
        host.join(".gitconfig").display()
    );
    assert!(
        specs.contains(&expect_git),
        "the staged extra must be mounted at $HOME/.gitconfig; specs={specs:?}"
    );
    let expect_gh = format!(
        "{}:{}:rw",
        staged_extras(&daemon, &run_id).join(".config/gh").display(),
        host.join(".config/gh").display()
    );
    assert!(
        specs.contains(&expect_gh),
        "the staged directory extra must be mounted too; specs={specs:?}"
    );

    // (c) THE NEGATIVE, load-bearing: no mount SOURCE is a real host path. Compare the
    // source SEGMENT, never `contains` — the mount TARGETS legitimately are host paths
    // (`sandbox_home_override == repo_root` in this harness), so a substring check would
    // false-positive on every single spec.
    for spec in &specs {
        let source = spec.split(':').next().unwrap_or(spec);
        for forbidden in [
            host.join(".gitconfig"),
            host.join(".config/gh"),
            host.join(".claude"),
            host.join(".claude.json"),
        ] {
            assert_ne!(
                source,
                forbidden.display().to_string(),
                "a real host path must never be a mount source; spec={spec}"
            );
        }
    }

    // (d) And the host file is byte-identical after the Run — nothing wrote back.
    write_node_output(&daemon, &run_id, "done\n");
    simulate_node_done(&daemon, &run_id).await;
    wait_run_status(&daemon, &run_id, "completed").await;
    assert_eq!(
        std::fs::read_to_string(host.join(".gitconfig")).unwrap(),
        HOST_GITCONFIG,
        "the host ~/.gitconfig must be untouched"
    );
}

/// The positive half of "a container write lands in the staged copy": simulate the
/// container writing THROUGH the mount (host path == staged path from the container's
/// point of view) and assert the host original is still intact.
///
/// Proven on a **directory** entry, deliberately. `git config --global` — the operation
/// the issue's AC3 named — cannot work here and it is NOT this slice's fault: `$HOME`
/// does not exist inside the image (`ubuntu:24.04` ships `/home/ubuntu`, not
/// `/home/<host user>`), so Docker creates it root-owned `0755` as the parent of the
/// mounts, and `git config` needs a writable `$HOME` for its lock-file-then-rename. That
/// is a pre-existing condition since #406, already true for `full` today; making `$HOME`
/// writable touches the ADR-0030 §1 identity mounts and is a follow-up.
#[tokio::test]
async fn a_write_under_a_staged_directory_entry_stays_in_the_staging() {
    ensure_pdo_on_path();
    let (_fake, docker, _log) = write_fake_docker();
    let daemon = TestDaemon::spawn_with_docker_override(seed(), docker)
        .await
        .unwrap();
    let host = daemon.repo_root();
    fabricate_host_home(host);
    let host_before = std::fs::read(host.join(".config/gh/hosts.yml")).unwrap();

    assert_eq!(
        put_profile(&daemon, "with-gh", &[], &[".config/gh"])
            .await
            .status(),
        200
    );
    let run_id = start_run(&daemon, Some("with-gh")).await;
    let staged_gh = staged_extras(&daemon, &run_id).join(".config/gh");
    assert!(wait_until(|| staged_gh.join("hosts.yml").is_file()).await);

    // The container refreshes its `gh` token: it writes into the mount, i.e. the staged
    // copy. The mount is rw and carries the STAGED source's ownership, so this succeeds.
    std::fs::write(
        staged_gh.join("hosts.yml"),
        "github.com:\n  user: refreshed\n",
    )
    .unwrap();

    assert_eq!(
        std::fs::read(host.join(".config/gh/hosts.yml")).unwrap(),
        host_before,
        "the host file must be byte-identical: the container writes to the COPY"
    );
}

// -- AC4: an entry under `.claude/` produces no extra `-v` --------------------

/// Not a special case — a consequence of the single `landing()` classifier. `.claude/…`
/// is already served by the fixed `claude-home` mount, so it must add nothing to the
/// queue; and with NO `$HOME`-exception entry the argv is byte-identical to #406, which
/// is exactly the "accommodate, don't freeze" property the golden test pins in-crate.
#[tokio::test]
async fn an_entry_under_claude_adds_no_mount() {
    ensure_pdo_on_path();
    let (_fake, docker, log) = write_fake_docker();
    let daemon = TestDaemon::spawn_with_docker_override(seed(), docker)
        .await
        .unwrap();
    fabricate_host_home(daemon.repo_root());

    // A profile whose only edit is under `.claude/` — plus a redundant extra ALSO under
    // `.claude/`, so both the default and the extra paths are exercised.
    assert_eq!(
        put_profile(
            &daemon,
            "claude-only",
            &[".claude/plugins"],
            &[".claude/skills"]
        )
        .await
        .status(),
        200
    );
    let run_id = start_run(&daemon, Some("claude-only")).await;
    assert!(wait_until(|| log_text(&log).contains("create")).await);

    let specs = mount_specs(&log);
    assert_eq!(
        specs.len(),
        4,
        "only the 4 FIXED mounts (repo, claude-home, .claude.json, pdo bin); specs={specs:?}"
    );
    assert!(
        !staged_extras(&daemon, &run_id).exists(),
        "no `$HOME` exception ⇒ no <staging>/home at all"
    );
}

// -- AC5: entry validation at write time --------------------------------------

#[tokio::test]
async fn a_bad_entry_is_rejected_at_profile_write() {
    ensure_pdo_on_path();
    let (_fake, docker, _log) = write_fake_docker();
    let daemon = TestDaemon::spawn_with_docker_override(seed(), docker)
        .await
        .unwrap();
    fabricate_host_home(daemon.repo_root());

    for bad in [
        "/etc/passwd",                  // absolute
        "../outside",                   // escapes $HOME
        ".config/../../etc/passwd",     // escapes after a normalisation
        "..\\..\\etc",                  // a backslash is a legal Linux filename char
        ".claude/projects",             // the runtime transcripts sink (floor-owned)
        ".claude/projects/-enc",        // …and anything under it
        ".claude/.credentials.json",    // floor-owned whole
        ".claude/remote-settings.json", // floor-owned whole
        ".claude",                      // the staged home is already mounted whole
        ".pdo",                         // holds the staging root itself
        ".claude/*.md",                 // a glob: authored by the default, never by hand
        ".",                            // $HOME itself
    ] {
        let resp = put_profile(&daemon, "probe", &[], &[bad]).await;
        assert_eq!(
            resp.status(),
            400,
            "`{bad}` must be rejected at write; body: {}",
            resp.text().await.unwrap_or_default()
        );
    }

    // …and a path that simply does not exist under $HOME — an early UX gate, with the
    // picker to hand (necessary, never sufficient: `prepare` warns-and-skips later).
    let resp = put_profile(&daemon, "probe", &[], &[".nope-not-here"]).await;
    assert_eq!(resp.status(), 400);

    // Nothing was persisted by any of the refusals.
    let listed: serde_json::Value =
        reqwest::get(format!("{}/settings/sandbox-profiles", daemon.url()))
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
    assert!(
        !listed["profiles"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["name"] == "probe"),
        "a rejected write must persist nothing: {listed}"
    );

    // A contradictory diff is refused too: resolving it silently would freeze into
    // `RunStarted` a winner the user never picked.
    let resp = put_profile(&daemon, "probe", &[".claude/skills"], &[".claude/skills"]).await;
    assert_eq!(resp.status(), 400);

    // A sensitive prefix, by contrast, is ALLOWED (ADR-0031 §3: warn, don't forbid).
    std::fs::create_dir_all(daemon.repo_root().join(".ssh")).unwrap();
    std::fs::write(daemon.repo_root().join(".ssh/id_ed25519"), "key\n").unwrap();
    let resp = put_profile(&daemon, "risky", &[], &[".ssh"]).await;
    assert_eq!(resp.status(), 200, "`.ssh` is allowed with a warning");
    let view: serde_json::Value = resp.json().await.unwrap();
    let ssh = view["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["path"] == ".ssh")
        .unwrap()
        .clone();
    assert_eq!(ssh["sensitive"], serde_json::json!(true), "{view}");
}

// -- AC6: the freeze ----------------------------------------------------------

/// Editing a profile after a Run has started must not change that Run's staging, and a
/// daemon restart must replay the **frozen** list identically (ADR-0031 §6). Without the
/// freeze, a restarted daemon would produce an incoherent home between two nodes of the
/// same Run — with `plugins/` physically present despite being unchecked.
#[tokio::test]
async fn editing_a_profile_does_not_change_a_running_runs_staging() {
    ensure_pdo_on_path();
    let (_fake, docker, _log) = write_fake_docker();
    let daemon = TestDaemon::spawn_with_docker_override(seed(), docker)
        .await
        .unwrap();
    fabricate_host_home(daemon.repo_root());

    // Start from a profile that DOES carry plugins.
    assert_eq!(put_profile(&daemon, "frozen", &[], &[]).await.status(), 200);
    let run_id = start_run(&daemon, Some("frozen")).await;
    wait_node_status(&daemon, &run_id, "running").await;
    let home = staged_home(&daemon, &run_id);
    assert!(wait_until(|| home.join("plugins").is_dir()).await);

    let frozen_before: Vec<String> = get_run(&daemon, &run_id).await["sandbox_entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert!(frozen_before.iter().any(|e| e == ".claude/plugins"));

    // Now uncheck plugins AND add an extra. The live Run must see neither.
    assert_eq!(
        put_profile(&daemon, "frozen", &[".claude/plugins"], &[".gitconfig"])
            .await
            .status(),
        200
    );

    // Force a SECOND `ensure_ready` for the same Run through boot recovery. `prepare` is
    // gated on the staging dir existing, so it does not re-run — and `extra_mounts` is
    // derived from the FROZEN list, so no `.gitconfig` mount appears either.
    daemon.run_boot_recovery_tick().await;

    let run = get_run(&daemon, &run_id).await;
    let frozen_after: Vec<String> = run["sandbox_entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        frozen_after, frozen_before,
        "the frozen list is immutable: {run}"
    );
    assert_eq!(
        sandbox_prep_failure(&daemon, &run_id).await,
        None,
        "boot recovery must replay the frozen list, not fail the sandbox prep"
    );
    assert!(
        home.join("plugins").is_dir(),
        "the already-staged entry must not be removed (prepare is additive)"
    );
    assert!(
        !staged_extras(&daemon, &run_id).join(".gitconfig").exists(),
        "an entry added AFTER the Run started must not appear in its staging"
    );
}

/// The other half of the freeze: a Run whose profile has been **deleted** since it
/// started still replays, because the list — not the name — is what `prepare` consumes.
/// Surviving that is the whole point.
#[tokio::test]
async fn boot_recovery_replays_a_frozen_list_whose_profile_was_deleted() {
    ensure_pdo_on_path();
    let (_fake, docker, _log) = write_fake_docker();
    let daemon = TestDaemon::spawn_with_docker_override(seed(), docker)
        .await
        .unwrap();
    fabricate_host_home(daemon.repo_root());

    assert_eq!(
        put_profile(&daemon, "doomed", &[".claude/plugins"], &[])
            .await
            .status(),
        200
    );
    let run_id = start_run(&daemon, Some("doomed")).await;
    wait_node_status(&daemon, &run_id, "running").await;

    let resp = reqwest::Client::new()
        .delete(format!("{}/settings/sandbox-profiles/doomed", daemon.url()))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        204,
        "delete is unconditional (soft guard-rail)"
    );
    assert_eq!(get_profile(&daemon, "doomed").await.status(), 404);

    daemon.run_boot_recovery_tick().await;

    assert_eq!(
        sandbox_prep_failure(&daemon, &run_id).await,
        None,
        "a live Run must survive its profile's deletion — the FROZEN LIST is what \
         `prepare` consumes, not the name"
    );
    assert!(
        !staged_home(&daemon, &run_id).join("plugins").exists(),
        "…and still reflect the list it froze"
    );
}

// -- AC7: an unknown profile fails hard, everywhere ---------------------------

#[tokio::test]
async fn an_unknown_profile_is_a_400_at_create() {
    ensure_pdo_on_path();
    let (_fake, docker, log) = write_fake_docker();
    let daemon = TestDaemon::spawn_with_docker_override(seed(), docker)
        .await
        .unwrap();
    fabricate_host_home(daemon.repo_root());

    let resp = post_run(&daemon, Some("does-not-exist")).await;
    assert_eq!(resp.status(), 400, "no Run may be created");
    let body: serde_json::Value = resp.json().await.unwrap();
    let msg = body["error"].as_str().unwrap_or_default();
    assert!(
        msg.contains("does-not-exist") && msg.contains("unknown sandbox profile"),
        "the error must name the profile: {body}"
    );

    // Strictly nothing happened: no Run, no worktree, no docker call.
    let runs: serde_json::Value = reqwest::get(format!("{}/runs", daemon.url()))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let list = runs.as_array().cloned().unwrap_or_default();
    assert!(list.is_empty(), "no Run may exist: {runs}");
    assert_eq!(log_text(&log), "", "docker must not be invoked");
}

/// A Trigger pointing at a vanished profile: the fire is **visibly** in error, and it
/// produces no Run at all. Zero new code paid for this — `fire_one_trigger`'s existing
/// `Err` arm turns the 400's message into the history line.
#[tokio::test]
async fn an_unknown_profile_makes_a_trigger_fire_visibly_fail() {
    ensure_pdo_on_path();
    let (_fake, docker, _log) = write_fake_docker();
    let daemon = TestDaemon::spawn_with_docker_override(seed(), docker)
        .await
        .unwrap();
    fabricate_host_home(daemon.repo_root());

    // Create the profile, point a Trigger at it, then delete it — the only way to get a
    // dangling reference, since the write-time gate refuses an unknown name up front.
    assert_eq!(
        put_profile(&daemon, "vanishing", &[], &[]).await.status(),
        200
    );
    let trigger_id = create_trigger(&daemon, "nightly", "vanishing")
        .await
        .json::<serde_json::Value>()
        .await
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        reqwest::Client::new()
            .delete(format!(
                "{}/settings/sandbox-profiles/vanishing",
                daemon.url()
            ))
            .send()
            .await
            .unwrap()
            .status(),
        204
    );

    daemon.force_trigger_due(&trigger_id).await;
    daemon.run_trigger_tick().await;

    let fires = fire_history(&daemon, &trigger_id).await;
    let last = fires
        .first()
        .unwrap_or_else(|| panic!("the tick must have recorded a fire: {fires:?}"));
    assert_eq!(last["outcome"], "error", "the fire must be red: {last}");
    assert!(
        last["reason"]
            .as_str()
            .unwrap_or_default()
            .contains("vanishing"),
        "the reason must name the profile: {last}"
    );
    assert!(last["run_id"].is_null(), "no Run may be created: {last}");

    // The Trigger is NOT dangled: `next_fire_at` survives, so recreating the profile
    // heals it. A `Dangling` transition would have erased the schedule for something the
    // user can fix in ten seconds.
    let trig: serde_json::Value = reqwest::get(format!("{}/triggers/{trigger_id}", daemon.url()))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(
        !trig["next_fire_at"].is_null(),
        "the schedule must survive: {trig}"
    );
}

/// The instance default naming a vanished profile: a Run that falls back to it fails with
/// a message that names the TIER, and `GET /settings` discloses the dangling reference so
/// the user can see it before launching anything.
#[tokio::test]
async fn an_unknown_instance_default_is_a_distinct_400_and_is_disclosed() {
    ensure_pdo_on_path();
    let (_fake, docker, _log) = write_fake_docker();
    let daemon = TestDaemon::spawn_with_docker_override(seed(), docker)
        .await
        .unwrap();
    fabricate_host_home(daemon.repo_root());

    // The write gate refuses an unknown name, so go through create-then-delete.
    assert_eq!(put_profile(&daemon, "gone", &[], &[]).await.status(), 200);
    assert_eq!(put_default_sandbox(&daemon, "gone").await.status(), 200);
    assert_eq!(
        reqwest::Client::new()
            .delete(format!("{}/settings/sandbox-profiles/gone", daemon.url()))
            .send()
            .await
            .unwrap()
            .status(),
        204
    );

    // Disclosed, with a reason (the env tier passes through no validator at all, so this
    // is the only honest place to say it).
    let settings = get_settings(&daemon).await;
    assert_eq!(settings["default_sandbox"]["effective"], "gone");
    assert!(
        settings["default_sandbox"]["reason"]
            .as_str()
            .unwrap_or_default()
            .contains("gone"),
        "the settings view must disclose the dangling default: {}",
        settings["default_sandbox"]
    );

    // A Run with no explicit tier falls back to it → 400 naming the tier, NOT a silent
    // demotion to `off` and NOT another profile.
    let resp = post_run(&daemon, None).await;
    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    let msg = body["error"].as_str().unwrap_or_default();
    assert!(
        msg.contains("instance default"),
        "the message must name the losing tier: {body}"
    );

    // …and an explicit tier that DOES resolve is unaffected: only the WINNER is resolved.
    let resp = post_run(&daemon, Some("off")).await;
    assert_eq!(
        resp.status(),
        201,
        "an unconsulted broken default must not block an explicit `off`"
    );
}

/// A `sandbox_entries` payload that cannot be read is a **hard** `RunFailed` at boot
/// recovery, with the raw value in the reason — never a silent re-resolve, which would
/// change what the nodes that already launched saw.
#[tokio::test]
async fn an_unreadable_frozen_list_fails_the_run_at_boot_recovery() {
    ensure_pdo_on_path();
    let (_fake, docker, _log) = write_fake_docker();
    let daemon = TestDaemon::spawn_with_docker_override(seed(), docker)
        .await
        .unwrap();
    fabricate_host_home(daemon.repo_root());

    let run_id = start_run(&daemon, Some("full")).await;
    wait_node_status(&daemon, &run_id, "running").await;

    // Corrupt the frozen list in the event log, the way a hand-edited DB or a future
    // encoder bug would. Straight SQL against the daemon's own SQLite file — no `sqlite3`
    // CLI, which is not a build dependency of this repo.
    let db_path = daemon.repo_root().join(".pdo").join("pdo.db");
    let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}", db_path.display()))
        .await
        .unwrap();
    let updated = sqlx::query(
        "UPDATE events SET payload = json_set(payload, '$.sandbox_entries', 42) \
         WHERE run_id = ? AND kind = 'run_started'",
    )
    .bind(&run_id)
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(
        updated.rows_affected(),
        1,
        "the run_started row must be patched"
    );
    pool.close().await;

    daemon.run_boot_recovery_tick().await;

    let reason = sandbox_prep_failure(&daemon, &run_id)
        .await
        .unwrap_or_default();
    assert!(
        !reason.is_empty(),
        "an unreadable frozen list must fail the Run loud, not re-resolve silently"
    );
    let run = wait_run_status(&daemon, &run_id, "failed").await;
    assert_eq!(run["status"], "failed", "{run}");
    // #431 trap: the reason lives on /events, not on /runs/<id>.
    assert!(
        reason.contains("42") && reason.contains("unreadable"),
        "the reason must carry the offending raw value: {reason}"
    );
}

// -- AC8: virtual defaults, and the diff (not a snapshot) ---------------------

#[tokio::test]
async fn unedited_defaults_have_no_row_and_editing_stores_a_diff() {
    ensure_pdo_on_path();
    let (_fake, docker, _log) = write_fake_docker();
    let daemon = TestDaemon::spawn_with_docker_override(seed(), docker)
        .await
        .unwrap();
    fabricate_host_home(daemon.repo_root());

    // Both virtual defaults resolve with NO row.
    for name in ["full", "minimal"] {
        let view: serde_json::Value = get_profile(&daemon, name).await.json().await.unwrap();
        assert_eq!(view["virtual"], serde_json::json!(true), "{name}: {view}");
        assert_eq!(
            view["materialised"],
            serde_json::json!(false),
            "{name} must have no DB row until edited: {view}"
        );
        assert!(view["updated_at"].is_null(), "{name}: {view}");
    }
    // `minimal` IS the floor: an empty entry list, plus the read-only floor block —
    // without which the screen looks broken and the user wrongly concludes the container
    // starts with no credentials.
    let minimal: serde_json::Value = get_profile(&daemon, "minimal").await.json().await.unwrap();
    assert_eq!(minimal["resolved"], serde_json::json!([]));
    assert!(
        minimal["floor"].as_array().unwrap().len() >= 5,
        "the floor block must be present and read-only: {minimal}"
    );

    // Editing `full` MATERIALISES it, and stores the DIFF — not a snapshot.
    let view: serde_json::Value = put_profile(&daemon, "full", &[".claude/plugins"], &[])
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(view["materialised"], serde_json::json!(true));
    assert_eq!(
        view["virtual"],
        serde_json::json!(true),
        "still a default name"
    );
    assert_eq!(view["disabled"], serde_json::json!([".claude/plugins"]));
    assert!(
        view["extras"].as_array().unwrap().is_empty(),
        "the row holds the intention, not the effective list: {view}"
    );
    // The snapshot test: the resolved list still carries every OTHER default entry, so a
    // future release that adds one would be seen by this profile too.
    let resolved: Vec<&str> = view["resolved"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(resolved.contains(&".claude/skills"));
    assert!(resolved.contains(&".claude/*.md"));
    assert!(!resolved.contains(&".claude/plugins"));

    // Deleting an edited default reverts it to virtual (it does not disappear).
    assert_eq!(
        reqwest::Client::new()
            .delete(format!("{}/settings/sandbox-profiles/full", daemon.url()))
            .send()
            .await
            .unwrap()
            .status(),
        204
    );
    let view: serde_json::Value = get_profile(&daemon, "full").await.json().await.unwrap();
    assert_eq!(view["materialised"], serde_json::json!(false));
    assert!(view["resolved"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e == ".claude/plugins"));

    // A `disabled` naming an entry THIS version does not have is remembered as inactive,
    // not rejected — ADR-0031 §2's forward-compatibility clause.
    let view: serde_json::Value = put_profile(&daemon, "minimal", &[".claude/plugins"], &[])
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(
        view["inactive_disabled"],
        serde_json::json!([".claude/plugins"]),
        "an inactive `disabled` is a signalled no-op: {view}"
    );
    assert_eq!(view["resolved"], serde_json::json!([]));
}

// -- AC9: the floor holds when a class-(b) entry is unchecked ------------------

/// Unchecking `.claude/settings.json` must NOT block the Run: the floor re-synthesises a
/// one-key `settings.json` carrying the permissions bypass. This is the case that makes
/// ADR-0031 §1 state the floor as *guarantees* rather than as *files* — a user whose host
/// hooks do not exist in the container must get the synthesis, not a refusal.
#[tokio::test]
async fn unchecking_settings_json_still_starts_thanks_to_the_floor() {
    ensure_pdo_on_path();
    let (_fake, docker, _log) = write_fake_docker();
    let daemon = TestDaemon::spawn_with_docker_override(seed(), docker)
        .await
        .unwrap();
    fabricate_host_home(daemon.repo_root());

    assert_eq!(
        put_profile(&daemon, "no-settings", &[".claude/settings.json"], &[])
            .await
            .status(),
        200
    );
    let run_id = start_run(&daemon, Some("no-settings")).await;
    let run = wait_node_status(&daemon, &run_id, "running").await;
    assert_eq!(
        run["nodes"][NODE_ID]["status"], "running",
        "the Run must start with no dialog: {run}"
    );

    let staged = staged_home(&daemon, &run_id).join("settings.json");
    assert!(
        wait_until(|| staged.is_file()).await,
        "the floor must synthesise settings.json even when unchecked"
    );
    let settings: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&staged).unwrap()).unwrap();
    assert_eq!(
        settings,
        serde_json::json!({ "skipDangerousModePermissionPrompt": true }),
        "SYNTHESISED, not copied: the host's `hooks` must not be there: {settings}"
    );
    // The other floor guarantees hold too.
    assert!(staged_home(&daemon, &run_id)
        .join(".credentials.json")
        .is_file());
    assert!(staged_home(&daemon, &run_id).join("projects").is_dir());

    write_node_output(&daemon, &run_id, "ok\n");
    simulate_node_done(&daemon, &run_id).await;
    let run = wait_run_status(&daemon, &run_id, "completed").await;
    assert_eq!(run["status"], "completed", "{run}");
}

// -- AC10: referents before deletion ------------------------------------------

/// The dialog cannot be built client-side: `RunListEntry` does not carry `sandbox` (only
/// the full `RunState` does), so a browser would need N requests. And the third class is
/// the one that matters most: live Runs already froze their list and are **unaffected**,
/// while the instance default and Triggers are NOT repointed and their next Run fails.
#[tokio::test]
async fn referents_lists_the_three_classes_before_a_delete() {
    ensure_pdo_on_path();
    let (_fake, docker, _log) = write_fake_docker();
    let daemon = TestDaemon::spawn_with_docker_override(seed(), docker)
        .await
        .unwrap();
    fabricate_host_home(daemon.repo_root());

    assert_eq!(put_profile(&daemon, "shared", &[], &[]).await.status(), 200);
    assert_eq!(put_default_sandbox(&daemon, "shared").await.status(), 200);
    let trigger_id = create_trigger(&daemon, "nightly audit", "shared")
        .await
        .json::<serde_json::Value>()
        .await
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let run_id = start_run(&daemon, Some("shared")).await;
    wait_node_status(&daemon, &run_id, "running").await;

    let refs: serde_json::Value = reqwest::get(format!(
        "{}/settings/sandbox-profiles/shared/referents",
        daemon.url()
    ))
    .await
    .unwrap()
    .json()
    .await
    .unwrap();

    assert_eq!(refs["instance_default"], serde_json::json!(true), "{refs}");
    let triggers = refs["triggers"].as_array().unwrap();
    assert_eq!(triggers.len(), 1, "{refs}");
    assert_eq!(triggers[0]["id"], serde_json::json!(trigger_id));
    assert_eq!(triggers[0]["name"], "nightly audit");
    let runs = refs["runs"].as_array().unwrap();
    assert!(
        runs.iter().any(|r| r["run_id"] == run_id.as_str()),
        "the live Run must be listed (as unaffected): {refs}"
    );

    // A profile nobody references reports three empties, not a 404 — the dialog must be
    // able to say "nothing points at this".
    assert_eq!(put_profile(&daemon, "lonely", &[], &[]).await.status(), 200);
    let refs: serde_json::Value = reqwest::get(format!(
        "{}/settings/sandbox-profiles/lonely/referents",
        daemon.url()
    ))
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    assert_eq!(refs["instance_default"], serde_json::json!(false));
    assert!(refs["triggers"].as_array().unwrap().is_empty());
    assert!(refs["runs"].as_array().unwrap().is_empty());
}

// -- the settings surface -----------------------------------------------------

/// `GET /settings` gains exactly two things: the profile NAME list (never entry lists —
/// this is the launch dialog's hot path) and the host `$HOME`, which the editor needs to
/// turn the explorer's absolute pick into a `$HOME`-relative entry.
#[tokio::test]
async fn get_settings_exposes_profile_names_and_home() {
    ensure_pdo_on_path();
    let (_fake, docker, _log) = write_fake_docker();
    let daemon = TestDaemon::spawn_with_docker_override(seed(), docker)
        .await
        .unwrap();
    fabricate_host_home(daemon.repo_root());

    assert_eq!(put_profile(&daemon, "custom", &[], &[]).await.status(), 200);
    let settings = get_settings(&daemon).await;

    let names: Vec<(&str, bool)> = settings["sandbox_profiles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| (p["name"].as_str().unwrap(), p["virtual"].as_bool().unwrap()))
        .collect();
    assert_eq!(
        names,
        vec![("custom", false), ("full", true), ("minimal", true)],
        "sorted, virtuals ∪ materialised, flagged: {settings}"
    );
    // NAMES ONLY — the contract, so a future slice does not start serving entry lists on
    // the launch dialog's hot path.
    for p in settings["sandbox_profiles"].as_array().unwrap() {
        assert!(p.get("entries").is_none(), "names only: {p}");
        assert!(p.get("resolved").is_none(), "names only: {p}");
    }
    // `home` honours `sandbox_home_override` — otherwise the editor and the daemon would
    // disagree about what "under $HOME" means.
    assert_eq!(
        settings["home"].as_str().unwrap(),
        daemon.repo_root().to_string_lossy(),
        "{settings}"
    );
}

/// The name grammar, at the edge. `off` and a blank name are reserved (they are the
/// "no sandbox" token and the *clear* sentinel), and an uppercase name is refused rather
/// than folded — folding would have the UI hunt through a list it displays in lowercase.
#[tokio::test]
async fn profile_name_grammar_is_enforced_at_the_edge() {
    ensure_pdo_on_path();
    let (_fake, docker, _log) = write_fake_docker();
    let daemon = TestDaemon::spawn_with_docker_override(seed(), docker)
        .await
        .unwrap();

    for bad in ["off", "OFF", "Foo", "under_score", "-lead", "with.dot"] {
        let resp = put_profile(&daemon, bad, &[], &[]).await;
        assert_eq!(resp.status(), 400, "`{bad}` must be refused");
    }
    // `full` / `minimal` ARE allowed — creating them is what materialises them.
    for ok in ["full", "minimal", "full-no-mcp", "a9"] {
        assert_eq!(
            put_profile(&daemon, ok, &[], &[]).await.status(),
            200,
            "{ok}"
        );
    }
    // A `disabled` that is not a built-in default entry is refused: unchecking is for
    // defaults, dropping is for extras, and conflating them makes the diff ambiguous.
    let resp = put_profile(&daemon, "full-no-mcp", &[".gitconfig"], &[]).await;
    assert_eq!(resp.status(), 400);
}

// =============================================================================
// #468 — per-profile environment (ADR-0031 §8)
// =============================================================================

/// The four `-e` PDO poses itself, so a test can assert "and nothing else".
const RUN_CONSTANT_ENV_PREFIXES: &[&str] = &[
    "HOME=",
    "PDO_DAEMON_URL=",
    "PDO_RUN_ID=",
    "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=",
];

/// AC1, **both halves**. A profile with `env: {FOO: bar}` poses `-e FOO=bar` at
/// `docker create`; a Run on a profile **without** env does not. The negative control is
/// not optional: asserting only the positive half would pass just as well against a fake
/// docker that echoed whatever it was handed, or against a daemon that posed every
/// profile's env to every Run.
///
/// Layer 3a reads the fake's argv log rather than `docker inspect --format
/// '{{.Config.Env}}'` — same bytes, no real daemon needed. The real `docker inspect` is
/// the manual check recorded on the issue.
#[tokio::test]
async fn a_profiles_env_is_posed_at_create_and_only_for_that_profile() {
    ensure_pdo_on_path();
    let (_fake, docker, log) = write_fake_docker();
    let daemon = TestDaemon::spawn_with_docker_override(seed(), docker)
        .await
        .unwrap();
    fabricate_host_home(daemon.repo_root());

    assert_eq!(
        put_profile_full(&daemon, "with-env", &[], &[], &[("FOO", "bar")])
            .await
            .status(),
        200
    );
    // The profile with no env at all — the negative control, created in the same daemon so
    // nothing but the profile differs between the two Runs.
    assert_eq!(
        put_profile_full(&daemon, "no-env", &[], &[], &[])
            .await
            .status(),
        200
    );

    // (a) POSITIVE.
    let with = start_run_of(&daemon, PIPELINE_NAME, Some("with-env")).await;
    assert!(
        wait_until(|| env_specs(&log).iter().any(|e| e == "FOO=bar")).await,
        "the profile env must be posed at create; env={:?}",
        env_specs(&log)
    );
    // Frozen on the Run too — that is what makes an edit non-retroactive.
    let run = get_run(&daemon, &with).await;
    assert_eq!(
        run["sandbox_env"]["FOO"], "bar",
        "the Run must freeze the env: {run}"
    );

    // (b) NEGATIVE CONTROL: a second Run, other profile, same daemon and same fake.
    std::fs::write(
        std::path::Path::new(&log),
        "", // truncate so the second Run's argv is unambiguous
    )
    .unwrap();
    let without = start_run_of(&daemon, PIPELINE_NAME, Some("no-env")).await;
    assert!(
        wait_until(|| log_text(&log).contains("create")).await,
        "the second Run must reach `docker create`; log:\n{}",
        log_text(&log)
    );
    let env = env_specs(&log);
    assert!(
        !env.iter().any(|e| e.starts_with("FOO=")),
        "a Run on a profile WITHOUT env must not carry another profile's env; env={env:?}"
    );
    // AC5 restated at layer 3: the only `-e` are the ones PDO owns.
    for spec in &env {
        assert!(
            RUN_CONSTANT_ENV_PREFIXES
                .iter()
                .any(|p| spec.starts_with(p))
                || spec.starts_with("PDO_SBX_SESSION=")
                || spec.starts_with("PDO_ARTIFACTS_DIR=")
                || spec.starts_with("PDO_INPUT_")
                || spec.starts_with("PDO_OUTPUT_")
                || spec.starts_with("PDO_VAR_")
                || spec == "PDO_NODE_ID"
                || spec == "PDO_NODE_ITER",
            "unexpected `-e {spec}` for a profile with no env; env={env:?}"
        );
    }
    // And its frozen payload carries no env key at all (the back-compat shape).
    let run = get_run(&daemon, &without).await;
    assert!(
        run.get("sandbox_env").is_none(),
        "an empty env must not grow the payload: {run}"
    );
}

/// AC2. The three run-constant keys are a **400 naming the key**, never a silent skip.
/// A silent skip would leave an editor that shows `HOME` set and a container where it is
/// not; a `HOME` that DID land would break the `.claude` and `.claude.json` mounts at once,
/// since both are computed from it.
#[tokio::test]
async fn a_run_constant_env_key_is_a_400_that_names_it() {
    ensure_pdo_on_path();
    let (_fake, docker, _log) = write_fake_docker();
    let daemon = TestDaemon::spawn_with_docker_override(seed(), docker)
        .await
        .unwrap();
    fabricate_host_home(daemon.repo_root());

    for key in ["HOME", "PDO_DAEMON_URL", "PDO_RUN_ID"] {
        let resp = put_profile_full(&daemon, "probe", &[], &[], &[(key, "x")]).await;
        assert_eq!(resp.status(), 400, "`{key}` must be refused");
        let body: serde_json::Value = resp.json().await.unwrap();
        let msg = body["error"].as_str().unwrap_or_default();
        assert!(
            msg.contains(key),
            "the 400 must NAME the offending key, got: {body}"
        );
    }
    // The malformed and the illegible are refused too — same 400, same naming.
    for (key, value) in [
        ("9LEADING_DIGIT", "x"),
        ("WITH-DASH", "x"),
        ("WITH SPACE", "x"),
        ("MULTI", "line1\nline2"),
        ("NUL", "a\0b"),
    ] {
        let resp = put_profile_full(&daemon, "probe", &[], &[], &[(key, value)]).await;
        assert_eq!(resp.status(), 400, "`{key}`={value:?} must be refused");
    }
    // Nothing was written: a refused PUT must not half-materialise the profile.
    assert_eq!(
        get_profile(&daemon, "probe").await.status(),
        404,
        "a rejected PUT must not create the row"
    );

    // …and a legal one goes through, so the refusals above are not a blanket "env is broken".
    let resp = put_profile_full(
        &daemon,
        "probe",
        &[],
        &[],
        &[("PUPPETEER_EXECUTABLE_PATH", "/usr/bin/chromium")],
    )
    .await;
    assert_eq!(resp.status(), 200);
    let view: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        view["env"]["PUPPETEER_EXECUTABLE_PATH"],
        "/usr/bin/chromium"
    );
    // The reserved list is served, so the editor greys them out instead of hard-coding them.
    let reserved: Vec<&str> = view["reserved_env_keys"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(
        reserved,
        vec!["HOME", "PDO_DAEMON_URL", "PDO_RUN_ID"],
        "{view}"
    );
}

/// AC3, on a **multi-node** Run — the point the issue insists on. Editing the env of a
/// profile while a Run is alive must change nothing for that Run's NEXT node. A single-node
/// Run proves nothing.
///
/// The env survives for the next node through **three** independent mechanisms, and this
/// test walks all three, because each closes a hole the others do not:
///
/// 1. **One container per Run.** The second node enters by `docker exec`, not by a second
///    `create` — so it inherits the environment of the container the first node created.
///    (The env is posed at `create` precisely because it is a Run constant; the `exec` path
///    has its own per-node list, `PDO_NODE_ID` / `PDO_ARTIFACTS_DIR` / …, which the profile
///    env must NOT contaminate.)
/// 2. **The frozen payload.** `sandbox_env` on the Run is immutable, so any *later*
///    `ensure_ready` — boot recovery after a daemon restart, `resume_run`, `open_run_shell`
///    — re-derives the same env. Forced here with a boot-recovery tick, since this harness's
///    fake reports the container absent on every probe and therefore re-creates it: the one
///    way to make the re-derivation observable at all.
/// 3. **Docker itself.** `docker start` never re-evaluates a pre-existing container's env,
///    any more than its mounts (ADR-0031 §6).
#[tokio::test]
async fn editing_the_env_does_not_change_a_live_multi_node_runs_next_node() {
    ensure_pdo_on_path();
    let (_fake, docker, log) = write_fake_docker();
    let daemon = TestDaemon::spawn_with_docker_override(seed_with_two_node_pipeline(), docker)
        .await
        .unwrap();
    fabricate_host_home(daemon.repo_root());

    assert_eq!(
        put_profile_full(&daemon, "evolving", &[], &[], &[("FOO", "before")])
            .await
            .status(),
        200
    );
    let run_id = start_run_of(&daemon, TWO_NODE_PIPELINE_NAME, Some("evolving")).await;
    wait_node_status_for(&daemon, &run_id, NODE_ID, "running").await;
    assert!(
        wait_until(|| env_specs(&log).iter().any(|e| e == "FOO=before")).await,
        "the first node's container must carry the frozen env; env={:?}",
        env_specs(&log)
    );
    let creates_after_first_node = create_count(&log);
    assert_eq!(
        creates_after_first_node,
        1,
        "one container per Run; log:\n{}",
        log_text(&log)
    );

    // Now edit the env (change one variable, add another) while the Run is alive.
    assert_eq!(
        put_profile_full(
            &daemon,
            "evolving",
            &[],
            &[],
            &[("FOO", "after"), ("ADDED", "yes")],
        )
        .await
        .status(),
        200
    );

    // (1) Advance to the SECOND node. It enters the SAME container by `docker exec` — no
    // second `create`, hence no opportunity to pose a different env at all.
    write_node_output_for(&daemon, &run_id, NODE_ID, "one\n");
    simulate_node_done_for(&daemon, &run_id, NODE_ID).await;
    wait_node_status_for(&daemon, &run_id, NODE_ID_2, "running").await;
    assert_eq!(
        create_count(&log),
        creates_after_first_node,
        "the next node must reuse the Run's container, not create a second one; log:\n{}",
        log_text(&log)
    );

    // (2) Force the re-derivation path a daemon restart would take. With the container
    // reported absent, this re-creates it — from the FROZEN env, not the edited profile.
    daemon.run_boot_recovery_tick().await;
    assert!(
        wait_until(|| create_count(&log) > creates_after_first_node).await,
        "boot recovery must re-enter the sandbox prep; log:\n{}",
        log_text(&log)
    );

    // The frozen payload is untouched…
    let run = get_run(&daemon, &run_id).await;
    assert_eq!(run["sandbox_env"]["FOO"], "before", "{run}");
    assert!(
        run["sandbox_env"].get("ADDED").is_none(),
        "an env added AFTER the Run started must not appear on it: {run}"
    );
    // …and NOTHING the daemon posed, at any point, carries the edited values.
    let env = env_specs(&log);
    assert!(
        env.iter().any(|e| e == "FOO=before"),
        "the frozen value must still be the one posed; env={env:?}"
    );
    assert!(
        !env.iter().any(|e| e == "FOO=after"),
        "the edited value must never reach this Run; env={env:?}"
    );
    assert!(
        !env.iter().any(|e| e.starts_with("ADDED=")),
        "a variable added mid-Run must never reach it; env={env:?}"
    );
    assert_eq!(
        sandbox_prep_failure(&daemon, &run_id).await,
        None,
        "the re-derivation must replay the frozen env, not fail"
    );

    // A Run started AFTER the edit does get the new env — the freeze is per-Run, not a
    // one-off snapshot of the profile.
    let fresh = start_run_of(&daemon, PIPELINE_NAME, Some("evolving")).await;
    let fresh_run = get_run(&daemon, &fresh).await;
    assert_eq!(fresh_run["sandbox_env"]["FOO"], "after", "{fresh_run}");
    assert_eq!(fresh_run["sandbox_env"]["ADDED"], "yes", "{fresh_run}");
}

/// The hard-error arm, twin of `an_unreadable_frozen_list_fails_the_run_at_boot_recovery`:
/// an unreadable `sandbox_env` is a `RunFailed` naming the raw value, never a silent "no
/// env" — which would start the container without the variables its MCP servers need and
/// look like a plugin bug.
#[tokio::test]
async fn an_unreadable_frozen_env_fails_the_run_at_boot_recovery() {
    ensure_pdo_on_path();
    let (_fake, docker, _log) = write_fake_docker();
    let daemon = TestDaemon::spawn_with_docker_override(seed(), docker)
        .await
        .unwrap();
    fabricate_host_home(daemon.repo_root());

    assert_eq!(
        put_profile_full(&daemon, "corrupt", &[], &[], &[("FOO", "bar")])
            .await
            .status(),
        200
    );
    let run_id = start_run(&daemon, Some("corrupt")).await;
    wait_node_status(&daemon, &run_id, "running").await;

    // Corrupt the frozen env the way a hand-edited DB or a future encoder bug would.
    let db_path = daemon.repo_root().join(".pdo").join("pdo.db");
    let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}", db_path.display()))
        .await
        .unwrap();
    let updated = sqlx::query(
        "UPDATE events SET payload = json_set(payload, '$.sandbox_env', 42) \
         WHERE run_id = ? AND kind = 'run_started'",
    )
    .bind(&run_id)
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(updated.rows_affected(), 1);
    pool.close().await;

    daemon.run_boot_recovery_tick().await;

    let reason = sandbox_prep_failure(&daemon, &run_id)
        .await
        .unwrap_or_default();
    assert!(
        !reason.is_empty(),
        "an unreadable frozen env must fail the Run loud, not degrade to `no env`"
    );
    let run = wait_run_status(&daemon, &run_id, "failed").await;
    assert_eq!(run["status"], "failed", "{run}");
    // #431 trap: the reason lives on /events, not on /runs/<id>.
    assert!(
        reason.contains("42") && reason.contains("unreadable"),
        "the reason must carry the offending raw value: {reason}"
    );
}

// -- #467 : la source d'image appartient au profil de staging ------------------

/// AC1. Two named profiles, one `kind: dockerfile` on a Dockerfile variant, one `kind: registry`
/// on an explicit ref. Two Runs, **each in its own image** — proven on the image arg of
/// `docker create`, which is what `docker inspect --format '{{.Config.Image}}'` would report.
///
/// Both halves matter, and neither would pass alone: the Dockerfile half proves the profile beats
/// the built-in default *and* carries the #466 variant NAME, while the registry half proves an
/// explicit ref is used verbatim rather than re-tagged under `pdo-sandbox:h-…`.
#[tokio::test]
async fn two_profiles_put_two_concurrent_runs_in_two_different_images() {
    ensure_pdo_on_path();
    let (_fake, docker, log) = write_fake_docker();
    let daemon = TestDaemon::spawn_with_docker_override(seed(), docker)
        .await
        .unwrap();
    fabricate_host_home(daemon.repo_root());

    let (dockerfile_a, expected_a) =
        write_variant_dockerfile(daemon.repo_root(), "variant-a", "variant-a");
    let explicit_ref = "ghcr.io/acme/agent:1.4";

    assert_eq!(
        put_profile_image(
            &daemon,
            "on-dockerfile",
            serde_json::json!({ "kind": "dockerfile", "path": dockerfile_a.display().to_string() }),
        )
        .await
        .status(),
        200
    );
    let resp = put_profile_image(
        &daemon,
        "on-registry",
        serde_json::json!({ "kind": "registry", "ref": explicit_ref }),
    )
    .await;
    assert_eq!(resp.status(), 200);
    // The view serves it back, so the editor needs no refetch and the round-trip is pinned.
    let view: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(view["image"]["kind"], "registry", "{view}");
    assert_eq!(view["image"]["ref"], explicit_ref, "{view}");

    // Two Runs, concurrently, on the same daemon and the same fake docker.
    let run_df = start_run_of(&daemon, PIPELINE_NAME, Some("on-dockerfile")).await;
    let run_reg = start_run_of(&daemon, PIPELINE_NAME, Some("on-registry")).await;

    assert!(
        wait_until(|| create_images(&log).len() >= 2).await,
        "both Runs must create their container; log:\n{}",
        log_text(&log)
    );
    let mut images = create_images(&log);
    images.sort();
    images.dedup();
    assert!(
        images.contains(&expected_a),
        "the Dockerfile profile's Run must run the variant's content-addressed image \
         ({expected_a}); images={images:?}"
    );
    assert!(
        images.contains(&explicit_ref.to_string()),
        "the registry profile's Run must run the explicit ref VERBATIM; images={images:?}"
    );
    assert_eq!(
        images.len(),
        2,
        "two profiles, two images; images={images:?}"
    );

    // …and each Run froze its own source, which is what makes the two independent.
    let a = get_run(&daemon, &run_df).await;
    assert_eq!(a["sandbox_image"]["kind"], "dockerfile", "{a}");
    assert_eq!(
        a["sandbox_image"]["path"],
        dockerfile_a.display().to_string(),
        "{a}"
    );
    let b = get_run(&daemon, &run_reg).await;
    assert_eq!(b["sandbox_image"]["kind"], "registry", "{b}");
    assert_eq!(b["sandbox_image"]["ref"], explicit_ref, "{b}");

    // No build, no pull: the fake reports the image present, and the fast-path precedes both on
    // BOTH branches. This is the offline-safe reuse property, restated at layer 3.
    assert!(
        !log_text(&log).lines().any(|l| l == "build" || l == "pull"),
        "a locally present image must skip build AND pull on both branches; log:\n{}",
        log_text(&log)
    );
}

/// AC4's layer-3 half: a profile that poses **nothing** is bit-for-bit the pre-#467 behaviour —
/// the content-addressed `pdo-sandbox:h-…` tag, and a `run_started` payload that does not grow by
/// one key. (The byte-identical `docker create` argv is pinned as a unit test in `sandbox_run`,
/// where two contexts can be compared verbatim.)
#[tokio::test]
async fn a_profile_without_an_image_source_keeps_the_content_addressed_tag() {
    ensure_pdo_on_path();
    let (_fake, docker, log) = write_fake_docker();
    let daemon = TestDaemon::spawn_with_docker_override(seed(), docker)
        .await
        .unwrap();
    fabricate_host_home(daemon.repo_root());

    // A materialised profile — so this is "poses no image", not "has no row".
    assert_eq!(
        put_profile(&daemon, "plain", &[".claude/plugins"], &[])
            .await
            .status(),
        200
    );
    let view: serde_json::Value = get_profile(&daemon, "plain").await.json().await.unwrap();
    assert!(
        view["image"].is_null(),
        "a profile that poses nothing serves `null`, not a fabricated default: {view}"
    );

    let run_id = start_run_of(&daemon, PIPELINE_NAME, Some("plain")).await;
    assert!(
        wait_until(|| !create_images(&log).is_empty()).await,
        "the Run must create its container; log:\n{}",
        log_text(&log)
    );
    let images = create_images(&log);
    assert!(
        images.iter().all(|i| i.starts_with("pdo-sandbox:h-")),
        "no profile source ⇒ the base content-addressed tag, exactly as before #467; \
         images={images:?}"
    );
    let run = get_run(&daemon, &run_id).await;
    assert!(
        run.get("sandbox_image").is_none(),
        "posing nothing must not grow the payload: {run}"
    );
}

/// AC3. An explicit ref that cannot be pulled fails the Run with a reason **naming the ref**, and
/// launches **no `docker build`**. The fake's `build` exits 0, so a build would have SUCCEEDED and
/// the Run would have started in an unrelated image — its absence is the assertion.
#[tokio::test]
async fn an_unreachable_explicit_ref_fails_the_run_and_never_builds() {
    ensure_pdo_on_path();
    let (_fake, docker, log) = write_fake_docker_failing_pull();
    let daemon = TestDaemon::spawn_with_docker_override(seed(), docker)
        .await
        .unwrap();
    fabricate_host_home(daemon.repo_root());

    assert_eq!(
        put_profile_image(
            &daemon,
            "ghost",
            serde_json::json!({ "kind": "registry", "ref": "ghcr.io/acme/nope:9" }),
        )
        .await
        .status(),
        200,
        "PDO does not probe the ref at write time — it cannot, and says so"
    );

    let run_id = start_run(&daemon, Some("ghost")).await;
    let run = wait_run_status(&daemon, &run_id, "failed").await;
    assert_eq!(run["status"], "failed", "{run}");

    // #431 trap: the prep reason lives on /events, not on /runs/<id>.
    let reason = sandbox_prep_failure(&daemon, &run_id)
        .await
        .unwrap_or_default();
    assert!(
        reason.contains("ghcr.io/acme/nope:9"),
        "the reason MUST name the ref (AC3): {reason}"
    );
    assert!(
        reason.contains("ghost"),
        "…and the profile that named it: {reason}"
    );
    assert!(
        !log_text(&log).lines().any(|l| l == "build"),
        "an explicit ref has no content hash, hence NO build to fall back to; log:\n{}",
        log_text(&log)
    );
    assert_eq!(
        create_count(&log),
        0,
        "and no container from an image that was never obtained; log:\n{}",
        log_text(&log)
    );
}

/// AC2, on a **multi-node** Run — the point the issue insists on. Editing a profile's image while
/// a Run is alive must change nothing for that Run's next node. A single-node Run would prove
/// nothing: the container is created once, so there would be no second occasion to get it wrong.
///
/// Same three mechanisms as the #468 env twin, walked in the same order: one container per Run
/// (the next node enters by `docker exec`), the frozen payload (so a boot-recovery re-creation
/// re-derives the SAME image), and Docker itself (`docker start` never re-evaluates a
/// pre-existing container's image any more than its env).
#[tokio::test]
async fn editing_a_profiles_image_does_not_change_a_live_multi_node_run() {
    ensure_pdo_on_path();
    let (_fake, docker, log) = write_fake_docker();
    let daemon = TestDaemon::spawn_with_docker_override(seed_with_two_node_pipeline(), docker)
        .await
        .unwrap();
    fabricate_host_home(daemon.repo_root());

    let before = "ghcr.io/acme/agent:before";
    let after = "ghcr.io/acme/agent:after";
    assert_eq!(
        put_profile_image(
            &daemon,
            "evolving-image",
            serde_json::json!({ "kind": "registry", "ref": before }),
        )
        .await
        .status(),
        200
    );

    let run_id = start_run_of(&daemon, TWO_NODE_PIPELINE_NAME, Some("evolving-image")).await;
    wait_node_status_for(&daemon, &run_id, NODE_ID, "running").await;
    assert!(
        wait_until(|| create_images(&log).iter().any(|i| i == before)).await,
        "the first node's container must run the frozen image; images={:?}",
        create_images(&log)
    );
    let creates_after_first_node = create_count(&log);
    assert_eq!(creates_after_first_node, 1, "one container per Run");

    // Edit the image while the Run is alive — and swap the KIND too, the most disruptive edit
    // available: were the live profile consulted, the Run would switch pull semantics mid-flight.
    let (variant, variant_ref) = write_variant_dockerfile(daemon.repo_root(), "late", "late");
    assert_eq!(
        put_profile_image(
            &daemon,
            "evolving-image",
            serde_json::json!({ "kind": "dockerfile", "path": variant.display().to_string() }),
        )
        .await
        .status(),
        200
    );
    // A second edit, back to a registry ref, so the final state differs from the frozen one in
    // VALUE as well as kind.
    assert_eq!(
        put_profile_image(
            &daemon,
            "evolving-image",
            serde_json::json!({ "kind": "registry", "ref": after }),
        )
        .await
        .status(),
        200
    );

    // (1) The second node enters the SAME container by `docker exec` — no second create.
    write_node_output_for(&daemon, &run_id, NODE_ID, "one\n");
    simulate_node_done_for(&daemon, &run_id, NODE_ID).await;
    wait_node_status_for(&daemon, &run_id, NODE_ID_2, "running").await;
    assert_eq!(
        create_count(&log),
        creates_after_first_node,
        "the next node must reuse the Run's container; log:\n{}",
        log_text(&log)
    );

    // (2) Force the re-derivation a daemon restart would take. The fake reports the container
    // absent, so this re-creates it — from the FROZEN source, not the edited profile.
    daemon.run_boot_recovery_tick().await;
    assert!(
        wait_until(|| create_count(&log) > creates_after_first_node).await,
        "boot recovery must re-enter the sandbox prep; log:\n{}",
        log_text(&log)
    );

    let run = get_run(&daemon, &run_id).await;
    assert_eq!(run["sandbox_image"]["ref"], before, "{run}");
    let images = create_images(&log);
    assert!(
        images.iter().all(|i| i == before),
        "every container of this Run must run the frozen image; images={images:?}"
    );
    assert!(
        !images.iter().any(|i| i == after || *i == variant_ref),
        "no edited value may reach this Run; images={images:?}"
    );
    assert_eq!(
        sandbox_prep_failure(&daemon, &run_id).await,
        None,
        "the re-derivation must replay the frozen source, not fail"
    );

    // A Run started AFTER the edits does get the new source — the freeze is per-Run, not a
    // one-off snapshot of the profile.
    let fresh = start_run_of(&daemon, PIPELINE_NAME, Some("evolving-image")).await;
    let fresh_run = get_run(&daemon, &fresh).await;
    assert_eq!(fresh_run["sandbox_image"]["ref"], after, "{fresh_run}");
}

/// The write-time refusals (#467). Each one is a 400 that names the offending value, and none of
/// them half-materialises the profile — the same contract as every other field of this PUT.
#[tokio::test]
async fn a_bad_image_source_is_rejected_at_profile_write() {
    ensure_pdo_on_path();
    let (_fake, docker, _log) = write_fake_docker();
    let daemon = TestDaemon::spawn_with_docker_override(seed(), docker)
        .await
        .unwrap();
    fabricate_host_home(daemon.repo_root());

    let missing = daemon.repo_root().join("no-such-Dockerfile");
    for bad in [
        // A relative path: the daemon's cwd is not the user's.
        serde_json::json!({ "kind": "dockerfile", "path": "docker/Dockerfile" }),
        // Absolute but absent — the early UX gate, not the authoritative one (`ensure_image`).
        serde_json::json!({ "kind": "dockerfile", "path": missing.display().to_string() }),
        // A directory is not a regular file (the `exists()` vs `is_file()` trap).
        serde_json::json!({ "kind": "dockerfile", "path": daemon.repo_root().display().to_string() }),
        // `docker pull -rm` would read the ref as a flag.
        serde_json::json!({ "kind": "registry", "ref": "-rm" }),
        serde_json::json!({ "kind": "registry", "ref": "" }),
        serde_json::json!({ "kind": "registry", "ref": "acme/agent :1" }),
        // An unknown kind: refused by the wire format itself, before any handler logic.
        serde_json::json!({ "kind": "ecr", "ref": "acme/agent:1" }),
        // …and a right kind with the wrong field.
        serde_json::json!({ "kind": "registry", "path": "/x" }),
    ] {
        let resp = put_profile_image(&daemon, "probe-image", bad.clone()).await;
        assert!(
            resp.status() == 400 || resp.status() == 422,
            "{bad} must be refused, got {}",
            resp.status()
        );
    }
    assert_eq!(
        get_profile(&daemon, "probe-image").await.status(),
        404,
        "a rejected PUT must not create the row"
    );

    // …and the legal forms go through, so the refusals above are not a blanket "image is broken".
    let (variant, _) = write_variant_dockerfile(daemon.repo_root(), "ok", "ok");
    for good in [
        serde_json::json!({ "kind": "dockerfile", "path": variant.display().to_string() }),
        serde_json::json!({ "kind": "registry", "ref": "ghcr.io/acme/agent:1.4" }),
        // `null` and an absent key both mean "poses nothing" — which is how you go back to the
        // instance-wide setting, and therefore has to be expressible.
        serde_json::json!(null),
    ] {
        let resp = put_profile_image(&daemon, "probe-image", good.clone()).await;
        assert_eq!(resp.status(), 200, "{good} must be accepted");
        let view: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(view["image"], good, "the view must round-trip it: {view}");
    }
    // Omitting the key entirely clears it too (a FULL replacement, like every other field).
    let resp = put_profile_body(
        &daemon,
        "probe-image",
        serde_json::json!({ "disabled": [], "extras": [], "env": {} }),
    )
    .await;
    assert_eq!(resp.status(), 200);
    let view: serde_json::Value = resp.json().await.unwrap();
    assert!(
        view["image"].is_null(),
        "an omitted image clears it: {view}"
    );
}
