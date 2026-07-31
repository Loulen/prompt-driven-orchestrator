//! Layer 3a — sandbox tracer bullet (#407, slice D of PRD #403).
//!
//! Drives `POST /runs` against a **real daemon** with a **fake `docker`** (via
//! `docker_cmd_override`) and a tempdir-scoped sandbox home (via
//! `sandbox_home_override`), so no test needs Docker, touches the real `$HOME`,
//! or launches real claude. Asserts the run-advance wiring:
//!   1. a `minimal` run projects `sandbox=minimal`, prep runs (`create`+`start`), the
//!      node tail is wrapped (`docker exec … pdo-sbx-<run>`), and the run completes;
//!   2. Docker unavailable → `RunFailed`, ZERO host spawn (no `NodeStarted`);
//!   3. an `off` run invokes docker NOT AT ALL (argv log empty) and completes on
//!      the host, byte-for-byte as before;
//!   4. `cleanup_run` removes the container (`rm -f pdo-sbx-<run>`) + purges staging;
//!   5. `boot_recovery` re-ensures a live sandboxed run's container;
//!   6. killing a sandboxed node issues a targeted in-container `docker exec` kill
//!      carrying the session marker — and **exactly one** of them (#488: the reap
//!      contains the kill, so keeping the old bare kill alongside would double it);
//!   7. the **manager preamble text** names the daemon by the hostname reachable from
//!      the side it will run on — the gateway when sandboxed, `localhost` when `off`
//!      (#447). The preamble file is written before tmux is invoked, so this is
//!      assertable without a real claude.
//!
//! The real end-to-end run (a live container, `pdo complete` from inside it) is
//! the Layer-5 job — a fake `docker exec` cannot run the node's body, so tests
//! that need a terminal state SIMULATE the container's callback by POSTing the
//! node-done endpoint (exactly what `pdo complete` does over HTTP).

mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Once;
use std::time::{Duration, Instant};

use common::TestDaemon;
use tempfile::TempDir;

const NODE_ID: &str = "notify";

/// `start → notify(script, output `out`) → end`, with `end` fed from `notify.out`,
/// so completing `notify` (with its output present) drives the whole run terminal.
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

fn ensure_pdo_on_path() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let bin = Path::new(env!("CARGO_BIN_EXE_pdo"));
        let dir = bin.parent().expect("pdo binary has a parent dir");
        let existing = std::env::var("PATH").unwrap_or_default();
        std::env::set_var("PATH", format!("{}:{}", dir.display(), existing));
    });
}

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

fn seed(body: &str) -> impl FnOnce(&Path) -> anyhow::Result<()> {
    let body = body.to_string();
    move |repo: &Path| {
        let pipelines_dir = repo.join(".pdo").join("pipelines");
        std::fs::create_dir_all(&pipelines_dir)?;
        std::fs::write(pipelines_dir.join("sbx-cycle.yaml"), PIPELINE_YAML)?;
        let prompts_dir = pipelines_dir.join("sbx-cycle.prompts");
        std::fs::create_dir_all(&prompts_dir)?;
        std::fs::write(prompts_dir.join(format!("{NODE_ID}.md")), &body)?;
        git_init_with_commit(repo)?;
        Ok(())
    }
}

/// Write a fake `docker` into a test-owned dir and return `(dir, docker_path, log)`.
/// Logs every invocation's argv (one line per arg) to `argv.log`. Canned:
/// `image inspect` → present (exit 0, no build); `container inspect` → ABSENT
/// (exit 1 + sentinel), so `ensure_running` does `create` + `start`; every other
/// subcommand exits 0. `sq` single-quotes the embedded log path.
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

fn log_text(log: &Path) -> String {
    std::fs::read_to_string(log).unwrap_or_default()
}

/// `POST /runs` with an optional `sandbox` mode. Returns the new run id.
async fn start_run(daemon: &TestDaemon, sandbox: Option<&str>) -> String {
    // #470: the target repo is required at the create boundary (ADR-0033).
    let mut body = serde_json::json!({
        "pipeline": "sbx-cycle",
        "input": "hello",
        "target_repo": daemon.target_repo(),
    });
    if let Some(mode) = sandbox {
        body["sandbox"] = serde_json::json!(mode);
    }
    let resp = reqwest::Client::new()
        .post(format!("{}/runs", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "POST /runs should create the run");
    resp.json::<serde_json::Value>().await.unwrap()["run_id"]
        .as_str()
        .unwrap()
        .to_string()
}

async fn get_run(daemon: &TestDaemon, run_id: &str) -> serde_json::Value {
    reqwest::get(format!("{}/runs/{run_id}", daemon.url()))
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap()
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

async fn post_command(
    daemon: &TestDaemon,
    run_id: &str,
    body: serde_json::Value,
) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{}/runs/{run_id}/commands", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap()
}

/// Simulate the container writing the node's declared output to the shared mount
/// (host path == container path). Written on the host before the simulated
/// `pdo complete` so node-done's output validation passes.
fn write_node_output(daemon: &TestDaemon, run_id: &str, content: &str) {
    let out = daemon
        .repo_root()
        .join(".pdo/runs")
        .join(run_id)
        .join("worktree/.pdo/artifacts")
        .join(NODE_ID)
        .join("iter-1/out/output.md");
    std::fs::create_dir_all(out.parent().unwrap()).unwrap();
    std::fs::write(&out, content).unwrap();
}

async fn simulate_node_done(daemon: &TestDaemon, run_id: &str) {
    let resp = reqwest::Client::new()
        .post(format!(
            "{}/runs/{run_id}/nodes/{NODE_ID}/done",
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

// -- Test 1: minimal run wires end-to-end ------------------------------------

#[tokio::test]
async fn minimal_run_prepares_wraps_and_completes() {
    ensure_pdo_on_path();
    let (_fake_dir, docker, log) = write_fake_docker();
    let daemon =
        TestDaemon::spawn_with_docker_override(seed("#!/usr/bin/env bash\ntrue\n"), docker)
            .await
            .unwrap();

    let run_id = start_run(&daemon, Some("minimal")).await;

    // (a) The mode is projected onto the Run from RunStarted.
    let run = get_run(&daemon, &run_id).await;
    assert_eq!(
        run["sandbox"], "minimal",
        "run must project sandbox=minimal: {run}"
    );

    // (b) Eager prep created + started the container (ensure_ready).
    assert!(
        wait_until(|| {
            let t = log_text(&log);
            t.contains("create") && t.contains("start")
        })
        .await,
        "prep must create+start the container; log:\n{}",
        log_text(&log)
    );

    // (c) The node's tail was wrapped: a `docker exec … pdo-sbx-<run>` launched it.
    assert!(
        wait_until(|| {
            let t = log_text(&log);
            t.contains("exec") && t.contains(&format!("pdo-sbx-{run_id}"))
        })
        .await,
        "the node tail must run via `docker exec pdo-sbx-{run_id}`; log:\n{}",
        log_text(&log)
    );

    // (d) The node reaches Running (NodeStarted appended after prep OK). The
    // container would write its output to the shared mount then call `pdo
    // complete`; we simulate both (write the output on the host = same path, then
    // POST node-done) and assert the whole run reaches `completed`.
    let run = wait_node_status(&daemon, &run_id, "running").await;
    assert_eq!(run["nodes"][NODE_ID]["status"], "running", "run: {run}");

    // (d-bis) #426 G3: with no host `~/.claude` at all, the floor SYNTHESISES the
    // staged `settings.json` down to the single bypass key — otherwise the session
    // would stall on the bypass-permissions prompt with nobody watching.
    let home = staged_home(&daemon, &run_id);
    let settings: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(home.join("settings.json")).unwrap())
            .unwrap();
    assert_eq!(
        settings,
        serde_json::json!({ BYPASS_PERMISSIONS_KEY: true }),
        "minimal must synthesise settings.json to the floor's single key"
    );

    write_node_output(&daemon, &run_id, "hello from the sandbox\n");
    simulate_node_done(&daemon, &run_id).await;
    let run = wait_run_status(&daemon, &run_id, "completed").await;
    assert_eq!(
        run["status"], "completed",
        "minimal run must complete: {run}"
    );
}

// -- Test 2: Docker unavailable → RunFailed, no host spawn -------------------

#[tokio::test]
async fn docker_unavailable_fails_run_with_no_host_spawn() {
    ensure_pdo_on_path();
    // A docker binary that does not exist: `ensure_image` hits ErrorKind::NotFound.
    let daemon = TestDaemon::spawn_with_docker_override(
        seed("#!/usr/bin/env bash\ntrue\n"),
        "/nonexistent/pdo-fake-docker-xyz".to_string(),
    )
    .await
    .unwrap();

    let run_id = start_run(&daemon, Some("minimal")).await;

    let run = wait_run_status(&daemon, &run_id, "failed").await;
    assert_eq!(
        run["status"], "failed",
        "a sandboxed run must fail loud when Docker is unavailable: {run}"
    );
    // ZERO host spawn: the node was never started (no host fallback).
    assert!(
        run["nodes"].get(NODE_ID).is_none()
            || run["nodes"][NODE_ID]["status"] == serde_json::Value::Null,
        "no NodeStarted — the sandboxed node must NOT fall back to a host spawn: {run}"
    );
}

// -- Test 3: off run invokes no docker, completes on the host ----------------

#[tokio::test]
async fn off_run_never_invokes_docker() {
    ensure_pdo_on_path();
    let (_fake_dir, docker, log) = write_fake_docker();
    // A real body that writes its declared output and self-signals `pdo complete`
    // on the host (off path). The sentinel is untracked → passes the
    // doc-only-effect clean guard.
    let daemon = TestDaemon::spawn_with_docker_override(
        seed(
            "#!/usr/bin/env bash\nset -euo pipefail\n\
             echo ok > OFF_SENTINEL\n\
             printf 'off output\\n' > \"$PDO_OUTPUT_OUT\"\n",
        ),
        docker,
    )
    .await
    .unwrap();

    // No `sandbox` param → Off → host execution.
    let run_id = start_run(&daemon, None).await;
    let run = wait_run_status(&daemon, &run_id, "completed").await;
    assert_eq!(
        run["status"], "completed",
        "off run must complete on host: {run}"
    );
    assert_eq!(run["sandbox"], "off", "default mode is off: {run}");

    // Docker was NEVER invoked on the off parcours.
    assert_eq!(
        log_text(&log),
        "",
        "the `off` path must not invoke docker at all"
    );
}

// -- Test 4: cleanup_run removes the container + purges staging ---------------

#[tokio::test]
async fn cleanup_run_removes_container_and_staging() {
    ensure_pdo_on_path();
    let (_fake_dir, docker, log) = write_fake_docker();
    let daemon =
        TestDaemon::spawn_with_docker_override(seed("#!/usr/bin/env bash\ntrue\n"), docker)
            .await
            .unwrap();

    let run_id = start_run(&daemon, Some("minimal")).await;
    wait_node_status(&daemon, &run_id, "running").await;
    write_node_output(&daemon, &run_id, "done\n");
    simulate_node_done(&daemon, &run_id).await;
    wait_run_status(&daemon, &run_id, "completed").await;

    // Staging landed under the tempdir home override (hermetic).
    let staging = daemon.repo_root().join(".pdo/sandbox").join(&run_id);
    assert!(
        wait_until(|| staging.exists()).await,
        "staging dir should exist before cleanup: {staging:?}"
    );

    let resp = post_command(
        &daemon,
        &run_id,
        serde_json::json!({ "kind": "cleanup_run" }),
    )
    .await;
    assert!(resp.status().is_success(), "cleanup_run should archive");
    wait_run_status(&daemon, &run_id, "archived").await;

    // The container was removed and the staging purged.
    assert!(
        log_text(&log).contains(&format!("pdo-sbx-{run_id}")) && log_text(&log).contains("rm"),
        "cleanup must `docker rm -f pdo-sbx-{run_id}`; log:\n{}",
        log_text(&log)
    );
    assert!(
        !staging.exists(),
        "cleanup must purge the staging dir: {staging:?}"
    );
}

// -- Test 5: boot_recovery re-ensures a live sandboxed container -------------

#[tokio::test]
async fn boot_recovery_reensures_sandbox_container() {
    ensure_pdo_on_path();
    let (_fake_dir, docker, log) = write_fake_docker();
    let daemon =
        TestDaemon::spawn_with_docker_override(seed("#!/usr/bin/env bash\ntrue\n"), docker)
            .await
            .unwrap();

    let run_id = start_run(&daemon, Some("minimal")).await;
    let count_creates = |log: &Path| log_text(log).lines().filter(|l| *l == "create").count();

    // Wait for the first prep to create+start (so the run is live), targeting
    // this run's container.
    assert!(
        wait_until(|| {
            let t = log_text(&log);
            t.contains("create") && t.contains("start") && t.contains(&format!("pdo-sbx-{run_id}"))
        })
        .await,
        "initial prep must create+start pdo-sbx-{run_id}; log:\n{}",
        log_text(&log)
    );
    let creates_before = count_creates(&log);

    // Boot recovery reconciles the live sandboxed run — re-ensures the container.
    daemon.run_boot_recovery_tick().await;

    assert!(
        count_creates(&log) > creates_before,
        "boot_recovery must re-ensure the container (a fresh create); log:\n{}",
        log_text(&log)
    );
}

// -- Test 6: killing a sandboxed node targets the container ------------------

#[tokio::test]
async fn kill_node_targets_the_container() {
    ensure_pdo_on_path();
    let (_fake_dir, docker, log) = write_fake_docker();
    let daemon =
        TestDaemon::spawn_with_docker_override(seed("#!/usr/bin/env bash\ntrue\n"), docker)
            .await
            .unwrap();

    let run_id = start_run(&daemon, Some("minimal")).await;
    wait_node_status(&daemon, &run_id, "running").await;

    let resp = post_command(
        &daemon,
        &run_id,
        serde_json::json!({ "kind": "kill_node", "node_id": NODE_ID, "iter": 1 }),
    )
    .await;
    assert!(resp.status().is_success(), "kill_node should succeed");

    let marker = format!("PDO_SBX_SESSION=pdo-{run_id}-{NODE_ID}-iter-1");
    assert!(
        wait_until(|| log_text(&log).contains(&marker)).await,
        "kill must issue a targeted in-container exec carrying the session marker \
         `{marker}`; log:\n{}",
        log_text(&log)
    );
}

/// #488: the reap CONTAINS the in-container kill. Keeping the old
/// `kill_session_best_effort` alongside it would double the `docker exec` without
/// any test catching it — `kill_node_targets_the_container` looks for a marker the
/// SPAWN already writes (via its `-e`), so it would pass either way. This test
/// counts, so it discriminates.
#[tokio::test]
async fn kill_node_issues_exactly_one_in_container_kill() {
    ensure_pdo_on_path();
    let (_fake_dir, docker, log) = write_fake_docker();
    let daemon =
        TestDaemon::spawn_with_docker_override(seed("#!/usr/bin/env bash\ntrue\n"), docker)
            .await
            .unwrap();

    let run_id = start_run(&daemon, Some("minimal")).await;
    wait_node_status(&daemon, &run_id, "running").await;

    let resp = post_command(
        &daemon,
        &run_id,
        serde_json::json!({ "kind": "kill_node", "node_id": NODE_ID, "iter": 1 }),
    )
    .await;
    assert!(resp.status().is_success(), "kill_node should succeed");

    // The tail of `kill_one_liner` (sandbox_container.rs) — emitted by the kill
    // path and by it alone, so it counts the `docker exec` kills exactly.
    const KILL_ONELINER_TAIL: &str = "k TERM; sleep 2; k KILL";

    assert!(
        wait_until(|| log_text(&log).contains(KILL_ONELINER_TAIL)).await,
        "kill_node must issue the targeted in-container kill; log:\n{}",
        log_text(&log)
    );
    assert_eq!(
        log_text(&log).matches(KILL_ONELINER_TAIL).count(),
        1,
        "exactly one in-container kill — the reap contains it, do not double it; log:\n{}",
        log_text(&log)
    );
}

// -- #409: mode `full` de bout en bout ---------------------------------------
//
// The harness collocates `home_root == host_home == repo_root == tempdir`
// (`sandbox_home_override`), so the fake host `~/.claude` lives at
// `<repo>/.claude`. It is fabricated AFTER spawn (untracked, out of the seed
// commit) and BEFORE `start_run` (eager prep reads it). The fake `docker exec`
// cannot run the container body, so a real end-to-end `full` run (skills actually
// loaded, kernel isolation, real auth) is the Layer-5 job (FP-409). Layer-3 proves
// what PDO **stages** and the **argv/mounts** it hands `docker create`.

/// Org managed-settings baseline cached by Claude Code in `~/.claude/` — the
/// guarantee G2 of the staging floor (#426).
const ORG_BASELINE_FILE: &str = "remote-settings.json";
/// Stand-in content for [`ORG_BASELINE_FILE`]. The real host file carries an org
/// OTEL bearer: assertions here compare against this fixture, never the real one.
const ORG_BASELINE: &str = r#"{"org":"baseline"}"#;
/// The top-level key that disarms the `--dangerously-skip-permissions` prompt —
/// guarantee G3 of the staging floor (#426).
const BYPASS_PERMISSIONS_KEY: &str = "skipDangerousModePermissionPrompt";

/// Fabricate a realistic host `~/.claude` (+ sibling `.claude.json`) under `home`.
/// Mirrors the unit `sandbox_staging::fabricate_home`: allowlist dirs/files, an
/// INTRA-tree symlink, an ESCAPING symlink to `~/.agents` (deref target, #409 D2),
/// 0600 creds, bulky host state that must be EXCLUDED, a pre-existing host
/// transcript under `projects/`, and a profile `.claude.json` with `oauthAccount`.
fn fabricate_host_claude(home: &Path) {
    let claude = home.join(".claude");
    let write = |p: PathBuf, c: &str| {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, c).unwrap();
    };
    let write_mode = |p: PathBuf, c: &str, mode: u32| {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, c).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode)).unwrap();
    };
    // Allowlist dirs.
    write(claude.join("skills/foo/skill.md"), "# skill\n");
    write_mode(
        claude.join("skills/foo/run.sh"),
        "#!/bin/sh\necho hi\n",
        0o755,
    );
    // INTRA-tree symlink (stays a link).
    std::os::unix::fs::symlink("skill.md", claude.join("skills/foo/link.md")).unwrap();
    // ESCAPING skill → ~/.agents/skills/esc (outside ~/.claude): must be
    // dereferenced into the staged tree, else it dangles in the container.
    write(
        home.join(".agents/skills/esc/SKILL.md"),
        "# escaped skill\n",
    );
    std::os::unix::fs::symlink("../../.agents/skills/esc", claude.join("skills/esc")).unwrap();
    write(claude.join("plugins/bar/plugin.json"), "{}\n");
    write(claude.join("agents/a.md"), "agent\n");
    write(claude.join("commands/c.md"), "cmd\n");
    write(claude.join("output-styles/s.md"), "style\n");
    // Allowlist files (hooks live inside settings.json).
    write(claude.join("settings.json"), r#"{"hooks":{"Stop":[]}}"#);
    write(claude.join("settings.local.json"), r#"{"local":true}"#);
    // Org managed-settings baseline (#426): OUTSIDE the `full` allowlist — the
    // staging floor is its single writer, in BOTH modes. Deliberate mirror of the
    // unit fixture `sandbox_staging::tests::fabricate_home`; keep them in step.
    // Stand-in content: the real file carries an org OTEL bearer.
    write(claude.join(ORG_BASELINE_FILE), ORG_BASELINE);
    write_mode(
        claude.join(".credentials.json"),
        r#"{"token":"secret"}"#,
        0o600,
    );
    write(claude.join("CLAUDE.md"), "# global\n");
    write(claude.join("RTK.md"), "# rtk\n");
    // Bulky host state — must stay EXCLUDED from the staging.
    write(claude.join("history.jsonl"), "{\"cmd\":\"ls\"}\n");
    write(claude.join("file-history/big.bin"), "xxxxxxxxxx");
    write(claude.join("session-env/env-1/data"), "junk");
    // Pre-existing host transcript — `prepare` must NOT copy it.
    write(
        claude.join("projects/-enc-host/old.jsonl"),
        "{\"host\":1}\n",
    );
    // Sibling profile `.claude.json` (PII-bearing; `full` stages it, the floor
    // then merges onboarding + trust into it).
    write(
        home.join(".claude.json"),
        r#"{"host":"profile","oauthAccount":{"x":1}}"#,
    );
    // #432: host files OUTSIDE `~/.claude` that a staging profile may declare as
    // extras. Present here even though the default `full` profile does NOT carry them —
    // that is the point: `full_never_mounts_the_real_host_claude` and
    // `full_completes_without_host_config_writeback` assert they are neither mounted nor
    // written, so a future slice that starts staging them by default trips those tests.
    // Deliberate mirror of the unit fixture `sandbox_staging::tests::fabricate_home` and
    // of `sandbox_profiles::fabricate_host_home`; keep the three in step.
    write(
        home.join(".gitconfig"),
        "[user]\n\tname = Host User\n\temail = host@example.com\n",
    );
    write(
        home.join(".config/gh/hosts.yml"),
        "github.com:\n  user: me\n",
    );
}

/// The staged Claude home of a run under the tempdir override
/// (`<repo>/.pdo/sandbox/<run>/claude-home`).
fn staged_home(daemon: &TestDaemon, run_id: &str) -> PathBuf {
    daemon
        .repo_root()
        .join(".pdo/sandbox")
        .join(run_id)
        .join("claude-home")
}

/// The staged `.claude.json` sibling (`<repo>/.pdo/sandbox/<run>/.claude.json`).
fn staged_json(daemon: &TestDaemon, run_id: &str) -> PathBuf {
    daemon
        .repo_root()
        .join(".pdo/sandbox")
        .join(run_id)
        .join(".claude.json")
}

/// Every `-v` mount spec (the arg following each `-v`) in the fake-docker argv log.
fn mount_specs(log: &Path) -> Vec<String> {
    let lines: Vec<String> = log_text(log).lines().map(str::to_string).collect();
    let mut specs = Vec::new();
    let mut iter = lines.iter();
    while let Some(line) = iter.next() {
        if line == "-v" {
            if let Some(spec) = iter.next() {
                specs.push(spec.clone());
            }
        }
    }
    specs
}

// -- Test 7: full stages the allowlist (deref + trust) and completes ---------

#[tokio::test]
async fn full_run_stages_allowlist_and_completes() {
    ensure_pdo_on_path();
    let (_fake_dir, docker, _log) = write_fake_docker();
    let daemon =
        TestDaemon::spawn_with_docker_override(seed("#!/usr/bin/env bash\ntrue\n"), docker)
            .await
            .unwrap();
    fabricate_host_claude(daemon.repo_root());

    let run_id = start_run(&daemon, Some("full")).await;
    let run = get_run(&daemon, &run_id).await;
    assert_eq!(
        run["sandbox"], "full",
        "run must project sandbox=full: {run}"
    );

    // Node reaches Running ⇒ eager prep (incl. the full walk + the floor) is done.
    let run = wait_node_status(&daemon, &run_id, "running").await;
    assert_eq!(run["nodes"][NODE_ID]["status"], "running", "run: {run}");

    let home = staged_home(&daemon, &run_id);
    assert!(
        wait_until(|| home.join("settings.json").is_file()).await,
        "staged settings.json should exist once the node is running"
    );

    // Allowlist dirs staged.
    assert!(home.join("skills/foo/skill.md").is_file());
    assert!(home.join("plugins/bar/plugin.json").is_file());
    assert!(home.join("agents/a.md").is_file());
    assert!(home.join("commands/c.md").is_file());
    assert!(home.join("output-styles/s.md").is_file());
    // #409 D2: the escaping skill is DEREFERENCED into a regular file (not a
    // dangling symlink) — exercised through the real prep path, not just a unit.
    let esc = home.join("skills/esc/SKILL.md");
    assert!(
        std::fs::symlink_metadata(&esc)
            .unwrap()
            .file_type()
            .is_file(),
        "the escaping skill must be dereferenced, not a dangling symlink"
    );
    assert_eq!(std::fs::read_to_string(&esc).unwrap(), "# escaped skill\n");
    // Allowlist files (hooks live inside settings.json).
    assert!(std::fs::read_to_string(home.join("settings.json"))
        .unwrap()
        .contains("hooks"));
    // #426 G2: the org managed-settings baseline is staged VERBATIM — through the
    // daemon's real prep path, not just the unit. It lives OUTSIDE the `full`
    // allowlist: the floor is its single writer.
    assert_eq!(
        std::fs::read_to_string(home.join(ORG_BASELINE_FILE)).unwrap(),
        ORG_BASELINE,
        "the org managed-settings baseline must be staged byte-for-byte"
    );
    // #426 G3: the bypass key is MERGED into the copied host settings — the hooks
    // survive (a naive overwrite would drop them).
    let staged_settings: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(home.join("settings.json")).unwrap())
            .unwrap();
    assert_eq!(
        staged_settings[BYPASS_PERMISSIONS_KEY],
        serde_json::json!(true),
        "staged settings.json must carry the bypass key: {staged_settings}"
    );
    assert!(
        staged_settings["hooks"]["Stop"].is_array(),
        "the merge must be non-destructive: {staged_settings}"
    );
    assert!(home.join("settings.local.json").is_file());
    assert!(home.join(".credentials.json").is_file());
    assert!(home.join("CLAUDE.md").is_file());
    assert!(home.join("RTK.md").is_file());

    // `.claude.json` sibling (OUTSIDE claude-home/): host profile preserved verbatim
    // + trust seeded for the Run's repo_root (#409 D5).
    let staged = staged_json(&daemon, &run_id);
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&staged).unwrap()).unwrap();
    assert_eq!(
        json["oauthAccount"]["x"],
        serde_json::json!(1),
        "host profile keys preserved verbatim: {json}"
    );
    let repo_key = daemon.repo_root().to_string_lossy().into_owned();
    assert_eq!(
        json["projects"][&repo_key]["hasTrustDialogAccepted"],
        serde_json::json!(true),
        "the floor seeds trust for the Run's repo_root: {json}"
    );
    assert!(
        !home.join(".claude.json").exists(),
        ".claude.json must NOT live inside claude-home/"
    );

    // projects/ staged EMPTY (host transcripts never copied).
    assert!(home.join("projects").is_dir());
    assert_eq!(std::fs::read_dir(home.join("projects")).unwrap().count(), 0);

    // Drive the run terminal (simulate the container's output + `pdo complete`).
    write_node_output(&daemon, &run_id, "full output\n");
    simulate_node_done(&daemon, &run_id).await;
    let run = wait_run_status(&daemon, &run_id, "completed").await;
    assert_eq!(run["status"], "completed", "full run must complete: {run}");
}

// -- Test 7-bis (#426): minimal against a FABRICATED host — the floor's fork ---
//
// The only layer-3 test that drives `minimal` with a real host `~/.claude` present.
// That is exactly where the floor's copy-vs-synthesis fork lives: `remote-settings`
// is COPIED in both modes (G2), while `settings.json` is SYNTHESISED in `minimal`
// even though a rich host file sits right there (G3). Without a fabricated host, G2
// would take its no-op branch in every layer-3 test.

#[tokio::test]
async fn minimal_run_stages_the_floor_against_a_fabricated_host() {
    ensure_pdo_on_path();
    let (_fake_dir, docker, _log) = write_fake_docker();
    let daemon =
        TestDaemon::spawn_with_docker_override(seed("#!/usr/bin/env bash\ntrue\n"), docker)
            .await
            .unwrap();
    fabricate_host_claude(daemon.repo_root());

    let run_id = start_run(&daemon, Some("minimal")).await;
    wait_node_status(&daemon, &run_id, "running").await;

    let home = staged_home(&daemon, &run_id);
    assert!(
        wait_until(|| home.join("settings.json").is_file()).await,
        "staging should be seeded once the node is running"
    );

    // (a) G2 — COPY branch: the org baseline is staged even in `minimal`.
    assert_eq!(
        std::fs::read_to_string(home.join(ORG_BASELINE_FILE)).unwrap(),
        ORG_BASELINE,
        "the floor stages the org baseline in `minimal` too"
    );

    // (b) G3 — SYNTHESIS branch: the host `settings.json` carries `hooks`, the staged
    // one carries the bypass key and NOTHING of the host.
    let settings: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(home.join("settings.json")).unwrap())
            .unwrap();
    assert_eq!(
        settings,
        serde_json::json!({ BYPASS_PERMISSIONS_KEY: true }),
        "`minimal` synthesises settings.json — it never copies the host's: {settings}"
    );

    // (c) `minimal` really is minimal: nothing from the `full` profile leaked in.
    assert!(!home.join("skills").exists());
    assert!(!home.join("plugins").exists());
    assert!(!home.join("CLAUDE.md").exists());

    // (d) The host `.claude` is untouched (no write-back through the floor).
    let host_settings = daemon.repo_root().join(".claude/settings.json");
    assert_eq!(
        std::fs::read_to_string(&host_settings).unwrap(),
        r#"{"hooks":{"Stop":[]}}"#,
        "the floor must never write to the host `~/.claude`"
    );

    write_node_output(&daemon, &run_id, "minimal output\n");
    simulate_node_done(&daemon, &run_id).await;
    let run = wait_run_status(&daemon, &run_id, "completed").await;
    assert_eq!(
        run["status"], "completed",
        "minimal run must complete: {run}"
    );
}

// -- Test 8: full excludes projects/ and bulky host state --------------------

#[tokio::test]
async fn full_excludes_projects_and_bulky_host_state() {
    ensure_pdo_on_path();
    let (_fake_dir, docker, _log) = write_fake_docker();
    let daemon =
        TestDaemon::spawn_with_docker_override(seed("#!/usr/bin/env bash\ntrue\n"), docker)
            .await
            .unwrap();
    fabricate_host_claude(daemon.repo_root());

    let run_id = start_run(&daemon, Some("full")).await;
    wait_node_status(&daemon, &run_id, "running").await;

    let home = staged_home(&daemon, &run_id);
    assert!(
        wait_until(|| home.join("settings.json").is_file()).await,
        "staging should be seeded once the node is running"
    );

    // Bulky/transient host state is NEVER staged (allowlist, not denylist).
    assert!(!home.join("history.jsonl").exists());
    assert!(!home.join("file-history").exists());
    assert!(!home.join("session-env").exists());
    // Host transcripts are never copied by prepare; projects/ is created EMPTY.
    assert!(!home.join("projects/-enc-host/old.jsonl").exists());
    assert!(home.join("projects").is_dir());
    assert_eq!(std::fs::read_dir(home.join("projects")).unwrap().count(), 0);
}

// -- Test 9: the real host ~/.claude is never a mount SOURCE -----------------

#[tokio::test]
async fn full_never_mounts_the_real_host_claude() {
    ensure_pdo_on_path();
    let (_fake_dir, docker, log) = write_fake_docker();
    let daemon =
        TestDaemon::spawn_with_docker_override(seed("#!/usr/bin/env bash\ntrue\n"), docker)
            .await
            .unwrap();
    fabricate_host_claude(daemon.repo_root());

    let run_id = start_run(&daemon, Some("full")).await;
    // `container inspect` → ABSENT → ensure_running does `create` (mounts logged).
    assert!(
        wait_until(|| log_text(&log).contains("create")).await,
        "prep must `docker create` the container; log:\n{}",
        log_text(&log)
    );

    let specs = mount_specs(&log);
    let host_claude = daemon.repo_root().join(".claude");
    let host_json = daemon.repo_root().join(".claude.json");

    // Positive: the STAGED home is mounted at <repo>/.claude — source = staging.
    let expected_home_mount = format!(
        "{}:{}:rw",
        staged_home(&daemon, &run_id).display(),
        host_claude.display()
    );
    assert!(
        specs.contains(&expected_home_mount),
        "staged home must mount to <repo>/.claude (source = staging); specs={specs:?}"
    );

    // Negative (load-bearing): NO mount has ANY real host config path as its SOURCE.
    // Widened in #432 from "not the real `.claude`" to "no real host path at all", now
    // that a profile can name entries outside `~/.claude`: ADR-0031 §4 says such an entry
    // is COPIED then mounted, never bind-mounted from the host.
    //
    // Inspect the source SEGMENT (split ':'), never `contains` — the mount TARGETS
    // legitimately ARE host paths here (override home == repo_root), so a substring check
    // would false-positive on every spec.
    let host_gitconfig = daemon.repo_root().join(".gitconfig");
    let host_gh = daemon.repo_root().join(".config/gh");
    for spec in &specs {
        let source = spec.split(':').next().unwrap_or(spec);
        for forbidden in [&host_claude, &host_json, &host_gitconfig, &host_gh] {
            assert_ne!(
                source,
                forbidden.display().to_string(),
                "a real host path must never be a mount source; spec={spec}"
            );
        }
    }
    // And the `full` default declares nothing outside `~/.claude`, so the queue is empty:
    // exactly the 4 fixed mounts, argv byte-identical to #406.
    assert_eq!(
        specs.len(),
        4,
        "`full` must add no `$HOME`-exception mount; specs={specs:?}"
    );
}

// -- Test 10: no host config write-back; transcripts DO flow back ------------

#[tokio::test]
async fn full_completes_without_host_config_writeback() {
    ensure_pdo_on_path();
    let (_fake_dir, docker, _log) = write_fake_docker();
    let daemon =
        TestDaemon::spawn_with_docker_override(seed("#!/usr/bin/env bash\ntrue\n"), docker)
            .await
            .unwrap();
    fabricate_host_claude(daemon.repo_root());

    let host_claude = daemon.repo_root().join(".claude");
    let host_json = daemon.repo_root().join(".claude.json");
    // #432: `~/.gitconfig` rides along. It is the file ADR-0031 §4 names explicitly — an
    // agent that hits `unable to auto-detect email address` very naturally reaches for
    // `git config --global`, and a direct bind would have it rewrite the user's identity.
    let host_gitconfig = daemon.repo_root().join(".gitconfig");
    // Snapshot the host config BEFORE the run (bytes, load-bearing).
    let settings_before = std::fs::read(host_claude.join("settings.json")).unwrap();
    let json_before = std::fs::read(&host_json).unwrap();
    let gitconfig_before = std::fs::read(&host_gitconfig).unwrap();

    let run_id = start_run(&daemon, Some("full")).await;
    wait_node_status(&daemon, &run_id, "running").await;
    write_node_output(&daemon, &run_id, "done\n");
    simulate_node_done(&daemon, &run_id).await;
    wait_run_status(&daemon, &run_id, "completed").await;

    let staging = daemon.repo_root().join(".pdo/sandbox").join(&run_id);
    assert!(
        wait_until(|| staging.exists()).await,
        "staging must exist before cleanup: {staging:?}"
    );

    // Plant a transcript in the staged projects/ sink: the cleanup merge_back must
    // land it on the host (positive), while config stays untouched (negative).
    let staged_proj = staged_home(&daemon, &run_id).join("projects/-enc-test");
    std::fs::create_dir_all(&staged_proj).unwrap();
    std::fs::write(staged_proj.join("t.jsonl"), "{\"line\":1}\n").unwrap();

    let resp = post_command(
        &daemon,
        &run_id,
        serde_json::json!({ "kind": "cleanup_run" }),
    )
    .await;
    assert!(resp.status().is_success(), "cleanup_run should archive");
    wait_run_status(&daemon, &run_id, "archived").await;

    // AC: NO config write ever comes back to the host. Copy + trust seeding all
    // land in the STAGED tree; the host config stays byte-identical.
    assert_eq!(
        std::fs::read(host_claude.join("settings.json")).unwrap(),
        settings_before,
        "host settings.json must be byte-identical (no write-back)"
    );
    assert_eq!(
        std::fs::read(&host_json).unwrap(),
        json_before,
        "host .claude.json must be byte-identical (trust seeding writes only the staged copy)"
    );
    assert_eq!(
        std::fs::read(&host_gitconfig).unwrap(),
        gitconfig_before,
        "host ~/.gitconfig must be byte-identical (ADR-0031 §4: copy, never bind)"
    );
    // Positive counterpart: transcripts DO merge back to the host projects dir.
    assert!(
        host_claude.join("projects/-enc-test/t.jsonl").is_file(),
        "merge_back must land staged transcripts on the host"
    );
    // And the staging is purged.
    assert!(
        !staging.exists(),
        "cleanup must purge the staging dir: {staging:?}"
    );
}

// -- #411/#471: which acquisition path a Run takes (pull vs build) -----------
//
// Like `write_fake_docker` but cans `image inspect` → ABSENT (exit 1), so
// `ensure_image` proceeds past the fast-path to acquire the image: a `docker pull`
// for the hash-derived image at the seeded location, or a `docker build` when the
// resolved Dockerfile is somewhere else. Every other subcommand — `pull`, `tag`,
// `build`, `container`(create+start), `exec`, `rm` — exits 0.
//
// #471 removed the `image_source` / `dockerfile_path` settings these tests used to PUT, so what
// drives the choice here is the **staging profile**. The "dockerfile mode never pulls" property
// (the `ImageSource::Dockerfile` branch) is pinned by `sandbox_image::dockerfile_mode_never_pulls`
// as a unit test against the same kind of fake docker; at this layer the only remaining
// instance-wide way to reach it is the daemon's environment, which a shared test process cannot
// set without racing every sibling test in this file.
fn write_fake_docker_image_absent() -> (TempDir, String, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("fake-docker");
    let log = dir.path().join("argv.log");
    let sq = |s: &str| format!("'{}'", s.replace('\'', "'\\''"));
    let script = format!(
        "#!/usr/bin/env bash\n\
         printf '%s\\n' \"$@\" >> {log}\n\
         case \"$1\" in\n\
         image) exit 1 ;;\n\
         container) printf '%s' 'Error: No such container' >&2; exit 1 ;;\n\
         *) exit 0 ;;\n\
         esac\n",
        log = sq(&log.display().to_string()),
    );
    std::fs::write(&bin, script).unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    (dir, bin.to_str().unwrap().to_string(), log)
}

/// `PUT /settings/sandbox-profiles/{name}` posing an image source, the ONLY way to choose a Run's
/// image since #471. `minimal` is the profile every test here launches with, and a bare `image`
/// upsert materialises it with an empty diff — which is what `minimal` already resolves to.
async fn put_profile_image(daemon: &TestDaemon, name: &str, image: serde_json::Value) {
    let resp = reqwest::Client::new()
        .put(format!("{}/settings/sandbox-profiles/{name}", daemon.url()))
        .json(&serde_json::json!({
            "disabled": [],
            "extras": [],
            "env": {},
            "image": image,
        }))
        .send()
        .await
        .unwrap();
    let status = resp.status();
    assert!(
        status.is_success(),
        "PUT the `{name}` profile image should succeed: {status} {}",
        resp.text().await.unwrap_or_default()
    );
}

/// Whether a docker subcommand keyword appears as a standalone argv line (`$1`).
fn log_has_subcommand(log: &Path, name: &str) -> bool {
    log_text(log).lines().any(|l| l == name)
}

// -- Test 11: the built-in default pulls the image (no build on a successful pull) --

/// The profile default of #471 (`sandbox_profile::DEFAULT_PROFILE_IMAGE`) is registry-pulled and
/// hash-derived, so this needs no setup at all any more — which is the point: on an untouched
/// instance, a sandboxed Run pulls.
#[tokio::test]
async fn the_default_profile_image_pulls_the_sandbox_image() {
    ensure_pdo_on_path();
    let (_fake_dir, docker, log) = write_fake_docker_image_absent();
    let daemon =
        TestDaemon::spawn_with_docker_override(seed("#!/usr/bin/env bash\ntrue\n"), docker)
            .await
            .unwrap();

    // No PUT: `minimal` poses no image, and the profile default decides.
    let _run_id = start_run(&daemon, Some("minimal")).await;

    // ensure_image (image absent) attempts a pull before any build.
    assert!(
        wait_until(|| log_has_subcommand(&log, "pull")).await,
        "a profile posing no image must `docker pull` the hash-derived image; log:\n{}",
        log_text(&log)
    );
    // The fake pull succeeds (exit 0) → retag → NO fallback build.
    assert!(
        !log_has_subcommand(&log, "build"),
        "a successful pull must NOT fall back to a local build; log:\n{}",
        log_text(&log)
    );
}

// -- #431/#467: a profile's Dockerfile drives the -f flag AND skips the pull ---

/// Argv of the `docker build` invocation in the log. Exactly 6 args
/// (`build -t <tag> -f <dockerfile> <context>`), so we slice a fixed window rather
/// than "to the end" — unlike the in-module helper, the build is NOT the last
/// invocation here (the container create/start/exec follow it).
fn build_argv(log: &Path) -> Vec<String> {
    let content = log_text(log);
    let lines: Vec<String> = content.lines().map(str::to_string).collect();
    match lines.iter().position(|l| l == "build") {
        Some(i) => lines[i..(i + 6).min(lines.len())].to_vec(),
        None => Vec::new(),
    }
}

/// Every event of the Run, from `GET /runs/<id>/events` — where a `RunFailed`'s
/// `reason` lives (the Run projection carries no reason field).
async fn run_events(daemon: &TestDaemon, run_id: &str) -> Vec<serde_json::Value> {
    reqwest::get(format!("{}/runs/{run_id}/events", daemon.url()))
        .await
        .unwrap()
        .json::<Vec<serde_json::Value>>()
        .await
        .unwrap()
}

/// The 12-hex content hash of `bytes`, the way `release.yml` and `sandbox_image`
/// both compute it (`sha256sum | cut -c1-12`). Shelled out on purpose: it proves
/// the daemon's Rust hash matches the canonical CI recipe, not just itself.
fn content_tag(bytes: &[u8]) -> String {
    use std::io::Write;
    let mut child = Command::new("sha256sum")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.as_mut().unwrap().write_all(bytes).unwrap();
    let out = child.wait_with_output().unwrap();
    let hex = String::from_utf8(out.stdout).unwrap();
    format!("pdo-sandbox:h-{}", &hex[..12])
}

/// The `-f` flag, the content tag, the empty build context and the skipped pull, all from a
/// Dockerfile a **profile** points at (#467). Since #471 that is the only way to point at one from
/// the UI, and the "no pull" half is non-vacuous precisely because the source is still the
/// registry-pulling default: what suppresses the pull is the LOCATION predicate, not a mode.
#[tokio::test]
async fn a_profile_dockerfile_builds_from_it_without_pulling() {
    ensure_pdo_on_path();
    let (_fake_dir, docker, log) = write_fake_docker_image_absent();
    let daemon =
        TestDaemon::spawn_with_docker_override(seed("#!/usr/bin/env bash\ntrue\n"), docker)
            .await
            .unwrap();

    // A self-contained custom Dockerfile (no COPY: the build context is empty by
    // design, ADR-0030 §5 as amended). Lives inside the daemon's repo — the use case
    // the issue targets (versioned with the team's repo).
    let custom = daemon.repo_root().join("docker").join("sbx.Dockerfile");
    std::fs::create_dir_all(custom.parent().unwrap()).unwrap();
    let custom_bytes: &[u8] = b"FROM ubuntu:24.04\nRUN echo fp-431 custom dockerfile\n";
    std::fs::write(&custom, custom_bytes).unwrap();

    put_profile_image(
        &daemon,
        "minimal",
        serde_json::json!({ "kind": "dockerfile", "path": custom.display().to_string() }),
    )
    .await;

    let _run_id = start_run(&daemon, Some("minimal")).await;

    assert!(
        wait_until(|| log_has_subcommand(&log, "build")).await,
        "a custom Dockerfile must be built locally; log:\n{}",
        log_text(&log)
    );
    // THE assertion: the registry-pulling default is in force, yet no pull — the hash of a
    // custom Dockerfile cannot exist upstream (the fake pull would have SUCCEEDED, so this is a
    // real signal). And no retag either, since there was nothing to retag.
    assert!(
        !log_has_subcommand(&log, "pull"),
        "a custom Dockerfile must skip the GHCR pull; log:\n{}",
        log_text(&log)
    );
    assert!(
        !log_has_subcommand(&log, "tag"),
        "no pull ⇒ no retag; log:\n{}",
        log_text(&log)
    );

    let argv = build_argv(&log);
    // `-f` is exactly the custom path…
    assert_eq!(
        argv.iter().position(|a| a == "-f").map(|i| &argv[i + 1]),
        Some(&custom.display().to_string()),
        "`docker build -f` must point at the resolved custom Dockerfile; argv: {argv:?}"
    );
    // …the tag is the hash of ITS bytes, differing from the seeded default's…
    let custom_tag = content_tag(custom_bytes);
    let seeded_bytes = std::fs::read(daemon.repo_root().join(".pdo/sandbox/Dockerfile"))
        .expect("the seed must still land at the default path");
    assert_ne!(
        custom_tag,
        content_tag(&seeded_bytes),
        "fixture bug: the two Dockerfiles must hash differently"
    );
    assert_eq!(
        argv.iter().position(|a| a == "-t").map(|i| &argv[i + 1]),
        Some(&custom_tag),
        "the tag must be the content hash of the CUSTOM Dockerfile; argv: {argv:?}"
    );
    // …and the build context is still the dedicated EMPTY dir, never sandbox_root and
    // never the repo (D8 / ADR-0030 §5).
    let ctx = argv.last().unwrap();
    assert_eq!(
        ctx,
        &daemon
            .repo_root()
            .join(".pdo/sandbox/.build-ctx")
            .display()
            .to_string(),
        "the build context stays <sandbox_root>/.build-ctx; argv: {argv:?}"
    );

    // The custom Dockerfile was never overwritten by the seed.
    assert_eq!(
        std::fs::read(&custom).unwrap(),
        custom_bytes,
        "the seed must never write to a custom path"
    );
}

#[tokio::test]
async fn a_profile_dockerfile_that_vanished_fails_the_run_naming_path_and_tier() {
    // Realistic TOCTOU: the path passes the profile write's existence gate, then disappears
    // before the run. The prep must fail LOUD (ADR-0030 pt 4) — never silently build
    // the seeded default, which would mean running an image the team never versioned.
    ensure_pdo_on_path();
    let (_fake_dir, docker, log) = write_fake_docker_image_absent();
    let daemon =
        TestDaemon::spawn_with_docker_override(seed("#!/usr/bin/env bash\ntrue\n"), docker)
            .await
            .unwrap();

    let custom = daemon.repo_root().join("docker").join("sbx.Dockerfile");
    std::fs::create_dir_all(custom.parent().unwrap()).unwrap();
    std::fs::write(&custom, b"FROM ubuntu:24.04\nRUN echo gone-soon\n").unwrap();
    put_profile_image(
        &daemon,
        "minimal",
        serde_json::json!({ "kind": "dockerfile", "path": custom.display().to_string() }),
    )
    .await;

    // …and now it's gone.
    std::fs::remove_file(&custom).unwrap();

    let run_id = start_run(&daemon, Some("minimal")).await;
    let run = wait_run_status(&daemon, &run_id, "failed").await;
    assert_eq!(
        run["status"], "failed",
        "a vanished Dockerfile must fail the run, never fall back: {run}"
    );

    // The reason lives on the `run_failed` event, not on the Run projection.
    let evs = run_events(&daemon, &run_id).await;
    let failed = evs
        .iter()
        .find(|e| e["kind"] == "run_failed")
        .unwrap_or_else(|| panic!("a RunFailed event must be recorded; events: {evs:#?}"));
    let reason = serde_json::to_string(failed).unwrap();
    assert!(
        reason.contains(custom.to_str().unwrap()),
        "the failure reason must name the path: {reason}"
    );
    assert!(
        reason.contains("`profile` tier"),
        "the failure reason must name the winning tier: {reason}"
    );
    assert!(
        reason.contains("staging profile"),
        "…and send the user to the profile, the only place that path can be fixed: {reason}"
    );
    // The bail precedes the fast-path AND the build: nothing was built.
    assert!(
        !log_has_subcommand(&log, "build"),
        "no build must be attempted for an unnameable image; log:\n{}",
        log_text(&log)
    );
}

// -- #410: run-level exposure + sources of config ----------------------------

async fn get_settings_json(daemon: &TestDaemon) -> serde_json::Value {
    reqwest::get(format!("{}/settings", daemon.url()))
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap()
}

/// `PUT /settings {"default_sandbox": <mode>}` against the real daemon.
async fn put_default_sandbox(daemon: &TestDaemon, mode: &str) {
    let resp = reqwest::Client::new()
        .put(format!("{}/settings", daemon.url()))
        .json(&serde_json::json!({ "default_sandbox": mode }))
        .send()
        .await
        .unwrap();
    assert!(
        resp.status().is_success(),
        "PUT /settings default_sandbox={mode} should succeed: {}",
        resp.status()
    );
}

/// `POST /triggers` with an optional per-Trigger `sandbox` mode. Returns the id.
/// A non-empty `input_template` keeps the prompt-required reject rule satisfied.
async fn create_trigger(daemon: &TestDaemon, sandbox: Option<&str>) -> String {
    // #470: a Trigger is a Run template — no target repo, no Trigger (ADR-0033).
    let mut body = serde_json::json!({
        "name": "sbx-trigger",
        "pipeline_id": "sbx-cycle",
        "cron": "0 9 * * *",
        "input_template": "fired input",
        "target_repo": daemon.target_repo(),
    });
    if let Some(mode) = sandbox {
        body["sandbox"] = serde_json::json!(mode);
    }
    let resp = reqwest::Client::new()
        .post(format!("{}/triggers", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        201,
        "POST /triggers should create the trigger"
    );
    resp.json::<serde_json::Value>().await.unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string()
}

/// Fire a trigger by forcing it due and running one scheduler tick, then return the
/// resulting Run's id (the run whose `triggered_by` matches). Polls `GET /runs`.
async fn fire_trigger_and_get_run(daemon: &TestDaemon, trigger_id: &str) -> String {
    daemon.force_trigger_due(trigger_id).await;
    daemon.run_trigger_tick().await;
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        let runs = reqwest::get(format!("{}/runs", daemon.url()))
            .await
            .unwrap()
            .json::<Vec<serde_json::Value>>()
            .await
            .unwrap();
        if let Some(run) = runs.iter().find(|r| r["triggered_by"] == trigger_id) {
            return run["run_id"].as_str().unwrap().to_string();
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("trigger {trigger_id} must have produced a Run within the deadline");
}

// -- Test 13: GET /settings surfaces Docker availability (advisory probe) -----

#[tokio::test]
async fn get_settings_reports_docker_available_with_working_fake() {
    ensure_pdo_on_path();
    // The standard fake docker answers `version` (via the `*) exit 0` arm) → the
    // probe reports available.
    let (_fake_dir, docker, _log) = write_fake_docker();
    let daemon =
        TestDaemon::spawn_with_docker_override(seed("#!/usr/bin/env bash\ntrue\n"), docker)
            .await
            .unwrap();

    let view = get_settings_json(&daemon).await;
    let sd = &view["sandbox_docker"];
    assert_eq!(
        sd["available"], true,
        "a working docker must probe available: {sd}"
    );
    assert!(sd["reason"].is_null(), "available → no reason: {sd}");
    assert!(sd["checked_at"].is_string(), "checked_at present: {sd}");
}

#[tokio::test]
async fn get_settings_reports_docker_unavailable_when_binary_absent() {
    ensure_pdo_on_path();
    let daemon = TestDaemon::spawn_with_docker_override(
        seed("#!/usr/bin/env bash\ntrue\n"),
        "/nonexistent/pdo-fake-docker-probe".to_string(),
    )
    .await
    .unwrap();

    let view = get_settings_json(&daemon).await;
    let sd = &view["sandbox_docker"];
    assert_eq!(
        sd["available"], false,
        "an absent docker binary must probe unavailable: {sd}"
    );
    assert!(
        sd["reason"].as_str().unwrap_or("").contains("docker"),
        "unavailable must carry a human-readable reason: {sd}"
    );
}

// -- Test 14: a per-Trigger sandbox mode fires sandboxed Runs -----------------

#[tokio::test]
async fn trigger_with_sandbox_minimal_fires_minimal_run() {
    ensure_pdo_on_path();
    let (_fake_dir, docker, _log) = write_fake_docker();
    let daemon =
        TestDaemon::spawn_with_docker_override(seed("#!/usr/bin/env bash\ntrue\n"), docker)
            .await
            .unwrap();

    let trigger_id = create_trigger(&daemon, Some("minimal")).await;
    let run_id = fire_trigger_and_get_run(&daemon, &trigger_id).await;

    let run = get_run(&daemon, &run_id).await;
    assert_eq!(
        run["sandbox"], "minimal",
        "a trigger with sandbox=minimal must fire a minimal Run: {run}"
    );
}

// -- Test 15: a null-sandbox Trigger defers to the instance default -----------

#[tokio::test]
async fn trigger_without_sandbox_defers_to_instance_default() {
    ensure_pdo_on_path();
    let (_fake_dir, docker, _log) = write_fake_docker();
    let daemon =
        TestDaemon::spawn_with_docker_override(seed("#!/usr/bin/env bash\ntrue\n"), docker)
            .await
            .unwrap();

    // Instance default = full; the Trigger carries no sandbox → it inherits.
    put_default_sandbox(&daemon, "full").await;
    let trigger_id = create_trigger(&daemon, None).await;
    let run_id = fire_trigger_and_get_run(&daemon, &trigger_id).await;

    let run = get_run(&daemon, &run_id).await;
    assert_eq!(
        run["sandbox"], "full",
        "a null-sandbox Trigger must defer to the instance default (full): {run}"
    );
}

// -- Test 16 (#445): the watcher may not spawn into a container that isn't up --
//
// The reported failure, reproduced through the production trigger. `create_run`'s
// detached prep task waits for `ensure_ready` before advancing the Run, but the
// pipeline watcher did not: the FIRST read of a fresh `<run>/pipeline.yaml` is
// reported by inotify as an external modification, so merely opening the Run in the
// UI woke `handle_run_pipeline_modifications` mid-prep, which called the same
// advance path with no precondition. The node's tail `docker exec`ed into a
// container that did not exist yet — exit 1 in ~30 ms, the tmux window's command
// ended, and ~25 s later the stale detector rendered `session_died`.
//
// A fake `docker` whose `create` SLEEPS gives a deterministic prep window (the real
// trigger is ~1 GB of `~/.claude` staging, measured at 83-87 s for a 2 GB profile).
// Under the same fixture the pre-#445 daemon appends `node_started` while
// `sandbox_prep` is still `pending`, which is exactly what this asserts against.

/// Like [`write_fake_docker`] but `create` sleeps `secs` first, holding the Run in
/// `sandbox_prep = pending` long enough to fire a watcher event inside the window.
fn write_slow_create_docker(secs: u64) -> (TempDir, String, PathBuf) {
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
         create) sleep {secs}; exit 0 ;;\n\
         *) exit 0 ;;\n\
         esac\n",
        log = sq(&log.display().to_string()),
    );
    std::fs::write(&bin, script).unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    (dir, bin.to_str().unwrap().to_string(), log)
}

async fn wait_for_event_kind(
    daemon: &TestDaemon,
    run_id: &str,
    kind: &str,
    within: Duration,
) -> bool {
    let deadline = Instant::now() + within;
    while Instant::now() < deadline {
        if run_events(daemon, run_id)
            .await
            .iter()
            .any(|e| e["kind"] == kind)
        {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    false
}

#[tokio::test]
async fn watcher_advance_mid_prep_never_spawns_and_is_replayed() {
    ensure_pdo_on_path();
    // 6s: comfortably longer than the watcher's ~1s debounce plus the round trips
    // below, so the `pipeline_modified` advance provably lands inside the window.
    let (_fake_dir, docker, log) = write_slow_create_docker(6);
    let daemon =
        TestDaemon::spawn_with_docker_override(seed("#!/usr/bin/env bash\ntrue\n"), docker)
            .await
            .unwrap();

    let run_id = start_run(&daemon, Some("full")).await;

    // The prep has started and is blocked in `docker create`.
    assert!(
        wait_for_event_kind(
            &daemon,
            &run_id,
            "sandbox_prep_started",
            Duration::from_secs(10)
        )
        .await,
        "the detached prep task must announce itself before we probe the window"
    );
    let run = get_run(&daemon, &run_id).await;
    assert_eq!(
        run["sandbox_prep"], "pending",
        "precondition: the Run must be mid-prep for this test to mean anything: {run}"
    );

    // Wake the watcher exactly as the UI does — an external touch of the run-scoped
    // YAML. In production this is a *read*; a write is the same event to the daemon
    // and is what a test can trigger deterministically.
    let yaml_path = daemon
        .repo_root()
        .join(".pdo")
        .join("runs")
        .join(&run_id)
        .join("pipeline.yaml");
    let bumped = std::fs::read_to_string(&yaml_path)
        .unwrap()
        .replace("version: \"1.0\"", "version: \"1.1\"");
    std::fs::write(&yaml_path, bumped).unwrap();

    assert!(
        wait_for_event_kind(
            &daemon,
            &run_id,
            "pipeline_modified",
            Duration::from_secs(5)
        )
        .await,
        "the external write must reach the watcher — without this event the test \
         proves nothing about the spawn path it drives"
    );

    // THE ASSERTION. The watcher-driven advance ran; the container is still absent.
    let events = run_events(&daemon, &run_id).await;
    let prep_ready_seen = events.iter().any(|e| e["kind"] == "sandbox_prep_ready");
    assert!(
        !prep_ready_seen,
        "precondition: the prep must still be in flight at this point"
    );
    assert!(
        !events.iter().any(|e| e["kind"] == "node_started"),
        "the watcher-driven advance must NOT start a node while the container is \
         still being created — that spawn is what dies as session_died: {events:#?}"
    );
    assert_eq!(
        get_run(&daemon, &run_id).await["status"],
        "running",
        "and the Run must still be alive, not failed"
    );

    // THE OTHER HALF: the deferred spawn is replayed once the container is up.
    assert!(
        wait_for_event_kind(
            &daemon,
            &run_id,
            "sandbox_prep_ready",
            Duration::from_secs(20)
        )
        .await,
        "the prep must finish"
    );
    let run = wait_node_status(&daemon, &run_id, "running").await;
    assert_eq!(
        run["nodes"][NODE_ID]["status"], "running",
        "the spawn deferred during the prep must be replayed after \
         sandbox_prep_ready — a Run whose only trigger was the watcher would \
         otherwise wedge for ever: {run}"
    );

    // And it entered the container, not the host.
    let t = log_text(&log);
    assert!(
        t.contains("exec") && t.contains(&format!("pdo-sbx-{run_id}")),
        "the replayed tail must run inside the Run's container; log:\n{t}"
    );
}

// -- Test 12: the manager preamble carries the URL of the side it runs on (#447)

/// The manager's runtime preamble as written to disk by `tmux_session_manager::spawn`
/// (`<worktree>/.pdo/prompts/__manager__-iter-0.md`). Written *before* tmux is
/// invoked, so it exists whether or not the harmless tail actually started.
fn manager_preamble_path(daemon: &TestDaemon, run_id: &str) -> PathBuf {
    daemon
        .repo_root()
        .join(".pdo/runs")
        .join(run_id)
        .join("worktree/.pdo/prompts/__manager__-iter-0.md")
}

async fn read_manager_preamble(daemon: &TestDaemon, run_id: &str) -> String {
    let path = manager_preamble_path(daemon, run_id);
    assert!(
        wait_until(|| path.exists()).await,
        "the manager preamble must be written at {}",
        path.display()
    );
    std::fs::read_to_string(&path).unwrap()
}

/// #447: a sandboxed manager execs into the Run's container, where `localhost` is
/// the container — so the `curl` lines of its own preamble must name the host
/// gateway. Before the fix the text said `localhost:<port>`, the manager obeyed it,
/// got connection-refused on every call, and reported "the daemon is down" on a
/// perfectly healthy daemon — losing its whole command surface (starting with the
/// `rename_run` its preamble demands as a first action).
///
/// The assertion that matters is the ABSENCE of `localhost:<port>`: a preamble that
/// merely *mentions* the gateway somewhere while still printing host-only `curl`
/// commands reproduces the bug exactly.
#[tokio::test]
async fn sandboxed_manager_preamble_uses_the_container_side_url() {
    ensure_pdo_on_path();
    let (_fake_dir, docker, _log) = write_fake_docker();
    let daemon =
        TestDaemon::spawn_with_docker_override(seed("#!/usr/bin/env bash\ntrue\n"), docker)
            .await
            .unwrap();

    let run_id = start_run(&daemon, Some("minimal")).await;
    // The manager is spawned by the prep task, right after `sandbox_prep_ready`.
    wait_node_status(&daemon, &run_id, "running").await;

    let port = daemon.addr.port();
    let preamble = read_manager_preamble(&daemon, &run_id).await;

    assert!(
        preamble.contains(&format!(
            "Daemon base URL: `http://host.docker.internal:{port}`"
        )),
        "preamble:\n{preamble}"
    );
    assert!(
        !preamble.contains(&format!("localhost:{port}")),
        "no line may keep the host-only URL — from inside the container it resolves \
         to the container itself, so every command fails and the manager concludes \
         the daemon is dead:\n{preamble}"
    );
    // The command endpoint specifically: this is the surface the bug removed.
    assert!(
        preamble.contains(&format!(
            "POST http://host.docker.internal:{port}/runs/{run_id}/commands"
        )),
        "preamble:\n{preamble}"
    );
}

/// Non-regression twin: an `off` Run's manager runs on the host, so its preamble
/// must stay exactly as it was — `localhost`, and no gateway hostname anywhere.
#[tokio::test]
async fn off_run_manager_preamble_stays_on_localhost() {
    ensure_pdo_on_path();
    let (_fake_dir, docker, log) = write_fake_docker();
    let daemon =
        TestDaemon::spawn_with_docker_override(seed("#!/usr/bin/env bash\ntrue\n"), docker)
            .await
            .unwrap();

    let run_id = start_run(&daemon, None).await;
    let port = daemon.addr.port();
    let preamble = read_manager_preamble(&daemon, &run_id).await;

    assert!(
        preamble.contains(&format!("Daemon base URL: `http://localhost:{port}`")),
        "preamble:\n{preamble}"
    );
    assert!(
        !preamble.contains("host.docker.internal"),
        "the host path must never be handed the container-only hostname:\n{preamble}"
    );
    // And the `off` parcours still never touches docker.
    assert_eq!(
        log_text(&log),
        "",
        "the `off` path must not invoke docker at all"
    );
}

// -- Test 13: the host uid gets a named identity inside the container (#414) ---

/// The `argv.log` window of the identity `docker exec` (#414). The `0:0` line is its
/// UNIQUE witness: every other invocation of the fake docker either carries no `--user`
/// at all or carries the host `<uid>:<gid>`, never root.
fn identity_exec_argv(log: &Path) -> Option<Vec<String>> {
    let content = log_text(log);
    let lines: Vec<String> = content.lines().map(str::to_string).collect();
    let user_at = lines.iter().position(|l| l == "0:0")?;
    let at = user_at.checked_sub(2)?; // `exec`, `--user`, `0:0`
    Some(lines[at..(at + 7).min(lines.len())].to_vec())
}

/// #414: a container runs as `--user <uid>:<gid>` NUMERIC, and `ubuntu:24.04` only knows
/// uid 1000. On any other host uid, `sudo` calls `getpwuid()` before applying NOPASSWD and
/// gives up ("you do not exist in the passwd database") — the agent loses `apt install`,
/// which is the entire reason the image ships `sudo`. So the prep runs one
/// `docker exec --user 0:0` right after the `start` that APPENDS the missing lines to the
/// image's REAL `/etc/passwd` and `/etc/group`, behind a `getent` guard.
///
/// The daemon here runs under the live host uid, so this asserts the SHAPE, never a uid
/// value: the argv, the guard, the two appends, the `*` password field, and the home field
/// — which must be the harness's `sandbox_home_override`, i.e. the very path the create
/// posed as `-e HOME=`. `getent`, `~` and the environment naming three different
/// directories is precisely the class of bug this pins.
///
/// Includes its own negative control (an `off` Run adds no such exec), stronger than
/// re-reading `off_run_never_invokes_docker`: it proves the injection is gated by the mode
/// on a daemon that HAS just performed one.
#[tokio::test]
async fn sandbox_prep_identifies_the_host_uid_in_the_container() {
    ensure_pdo_on_path();
    let (_fake_dir, docker, log) = write_fake_docker();
    // A body that writes its declared output on the host, so the `off` half of this test
    // (the negative control) completes for real instead of failing output validation.
    let daemon = TestDaemon::spawn_with_docker_override(
        seed(
            "#!/usr/bin/env bash\nset -euo pipefail\n\
             printf 'ok\\n' > \"$PDO_OUTPUT_OUT\"\n",
        ),
        docker,
    )
    .await
    .unwrap();

    let run_id = start_run(&daemon, Some("minimal")).await;
    assert!(
        wait_until(|| identity_exec_argv(&log).is_some()).await,
        "the prep must run the identity `docker exec`; log:\n{}",
        log_text(&log)
    );
    let argv = identity_exec_argv(&log).unwrap();

    // (a) The argv: root, this Run's container, `sh -c <script>`. No `-e` (a session
    // marker here would make the first targeted kill take this exec down too), no tty.
    assert_eq!(
        &argv[..6],
        &[
            "exec".to_string(),
            "--user".to_string(),
            "0:0".to_string(),
            format!("pdo-sbx-{run_id}"),
            "sh".to_string(),
            "-c".to_string(),
        ][..],
        "identity exec argv; log:\n{}",
        log_text(&log)
    );

    // (b) The script: guarded, appending, `*` in the password field.
    let script = &argv[6];
    for needle in [
        "getent passwd ",
        "getent group ",
        ">> /etc/passwd",
        ">> /etc/group",
        ":*:",
    ] {
        assert!(
            script.contains(needle),
            "the identity script must contain `{needle}`: {script}"
        );
    }
    // The home field IS the `-e HOME=` of the create (the harness collocates
    // host_home == repo_root via `sandbox_home_override`).
    let home = daemon.repo_root().display().to_string();
    assert!(
        script.contains(&format!("PDO sandbox:{home}:/bin/bash")),
        "the injected home must be the container's `$HOME` ({home}): {script}"
    );

    // (c) It runs AFTER the start — an exec into a container that is not up yet fails.
    let content = log_text(&log);
    let lines: Vec<&str> = content.lines().collect();
    let start_at = lines
        .iter()
        .position(|l| *l == "start")
        .expect("the prep must start the container");
    let identity_at = lines
        .iter()
        .position(|l| *l == "0:0")
        .expect("the identity exec must be logged");
    assert!(
        start_at < identity_at,
        "the identity exec must follow `docker start`; log:\n{content}"
    );

    // (d) Negative control on the SAME daemon: an `off` Run adds no identity exec.
    let identity_execs = |log: &Path| log_text(log).lines().filter(|l| *l == "0:0").count();
    let before = identity_execs(&log);
    let off_id = start_run(&daemon, None).await;
    let off = wait_run_status(&daemon, &off_id, "completed").await;
    assert_eq!(off["status"], "completed", "off run must complete: {off}");
    assert_eq!(
        identity_execs(&log),
        before,
        "an `off` Run must not identify anything — it never touches docker; log:\n{}",
        log_text(&log)
    );
}

/// #489 / ADR-0037 — `restart_node` mid-prep is a `409`, raised **before** the kill.
///
/// The `sandbox_spawn_block` precondition (#445) used to be discovered only inside
/// `spawn_node`, i.e. after the arm had already killed the tmux session and appended
/// its `CommandIssued` — and the caller was then told `200 {"ok":true}` because the
/// `SpawnOutcome::Deferred` was thrown away. The predicate is pure, so #489 evaluates
/// it at the head of the arm: no session dies for a container that is still building.
#[tokio::test]
async fn restart_node_mid_prep_is_refused_before_anything_is_touched() {
    ensure_pdo_on_path();
    // 6s: comfortably longer than the round trips below, so the restart provably
    // lands inside the prep window.
    let (_fake_dir, docker, _log) = write_slow_create_docker(6);
    let daemon =
        TestDaemon::spawn_with_docker_override(seed("#!/usr/bin/env bash\ntrue\n"), docker)
            .await
            .unwrap();

    let run_id = start_run(&daemon, Some("full")).await;
    assert!(
        wait_for_event_kind(
            &daemon,
            &run_id,
            "sandbox_prep_started",
            Duration::from_secs(10)
        )
        .await,
        "the detached prep task must announce itself before we probe the window"
    );
    let run = get_run(&daemon, &run_id).await;
    assert_eq!(
        run["sandbox_prep"], "pending",
        "precondition: the Run must be mid-prep, or this test means nothing: {run}"
    );

    let before = run_events(&daemon, &run_id).await;
    let resp = reqwest::Client::new()
        .post(format!("{}/runs/{run_id}/commands", daemon.url()))
        .json(&serde_json::json!({
            "kind": "restart_node", "node_id": NODE_ID, "iter": 1
        }))
        .send()
        .await
        .unwrap();
    let status = resp.status();
    let body: serde_json::Value = resp.json().await.unwrap();

    assert_eq!(status, 409, "{body}");
    assert_eq!(body["error"], "sandbox_prep_not_ready", "{body}");
    assert_eq!(body["recoverable"], true, "{body}");
    // THE point: the probe runs ahead of the kill, so nothing was destroyed.
    assert_eq!(body["session_killed"], false, "{body}");

    // …and nothing was written either.
    let after = run_events(&daemon, &run_id).await;
    assert!(
        !after
            .iter()
            .skip(before.len())
            .any(|e| e["kind"] == "command_issued" || e["kind"] == "node_started"),
        "a pre-kill refusal appends nothing: {:#?}",
        &after[before.len().min(after.len())..]
    );
}
