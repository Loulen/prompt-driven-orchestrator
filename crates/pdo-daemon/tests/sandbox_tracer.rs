//! Layer 3a — sandbox run-advance wiring.
//!
//! Drives `POST /runs` against a **real daemon** with a **fake `docker`** (via
//! `docker_cmd_override`) and a tempdir-scoped sandbox home (via
//! `sandbox_home_override`), so no test needs Docker, touches the real `$HOME`,
//! or launches real claude.
//!
//! The real end-to-end run (a live container, `pdo complete` from inside it) is
//! the Layer-5 job — a fake `docker exec` cannot run the node's body, so tests
//! that need a terminal state SIMULATE the container's callback by POSTing the
//! node-done endpoint (exactly what `pdo complete` does over HTTP).

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use crate::common::{ensure_pdo_on_path, TestDaemon};
use tempfile::TempDir;

const NODE_ID: &str = "notify";

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

/// A fake `docker` logging every argv to `argv.log`. `container inspect` must report
/// ABSENT, so `ensure_running` does `create` + `start` and the mounts get logged.
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

async fn start_run(daemon: &TestDaemon, sandbox: Option<&str>) -> String {
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

/// Stands in for the container writing the node's declared output to the shared mount
/// (host path == container path). Must precede the simulated `pdo complete`, or
/// node-done's output validation rejects it.
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

#[tokio::test]
async fn minimal_run_prepares_wraps_and_completes() {
    ensure_pdo_on_path();
    let (_fake_dir, docker, log) = write_fake_docker();
    let daemon =
        TestDaemon::spawn_with_docker_override(seed("#!/usr/bin/env bash\ntrue\n"), docker)
            .await
            .unwrap();

    let run_id = start_run(&daemon, Some("minimal")).await;

    let run = get_run(&daemon, &run_id).await;
    assert_eq!(
        run["sandbox"], "minimal",
        "run must project sandbox=minimal: {run}"
    );

    assert!(
        wait_until(|| {
            let t = log_text(&log);
            t.contains("create") && t.contains("start")
        })
        .await,
        "prep must create+start the container; log:\n{}",
        log_text(&log)
    );

    assert!(
        wait_until(|| {
            let t = log_text(&log);
            t.contains("exec") && t.contains(&format!("pdo-sbx-{run_id}"))
        })
        .await,
        "the node tail must run via `docker exec pdo-sbx-{run_id}`; log:\n{}",
        log_text(&log)
    );

    let run = wait_node_status(&daemon, &run_id, "running").await;
    assert_eq!(run["nodes"][NODE_ID]["status"], "running", "run: {run}");

    // Floor guarantee G3: with no host `~/.claude` at all, the staged `settings.json`
    // is SYNTHESISED down to the bypass key — otherwise the session stalls on the
    // bypass-permissions prompt with nobody watching.
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
    // No host fallback: the node must never start.
    assert!(
        run["nodes"].get(NODE_ID).is_none()
            || run["nodes"][NODE_ID]["status"] == serde_json::Value::Null,
        "no NodeStarted — the sandboxed node must NOT fall back to a host spawn: {run}"
    );
}

#[tokio::test]
async fn off_run_never_invokes_docker() {
    ensure_pdo_on_path();
    let (_fake_dir, docker, log) = write_fake_docker();
    // The sentinel is untracked, so it passes the doc-only-effect clean guard.
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

    let run_id = start_run(&daemon, None).await;
    let run = wait_run_status(&daemon, &run_id, "completed").await;
    assert_eq!(
        run["status"], "completed",
        "off run must complete on host: {run}"
    );
    assert_eq!(run["sandbox"], "off", "default mode is off: {run}");

    assert_eq!(
        log_text(&log),
        "",
        "the `off` path must not invoke docker at all"
    );
}

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

    daemon.run_boot_recovery_tick().await;

    assert!(
        count_creates(&log) > creates_before,
        "boot_recovery must re-ensure the container (a fresh create); log:\n{}",
        log_text(&log)
    );
}

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

/// The reap CONTAINS the in-container kill; keeping a bare `kill_session_best_effort`
/// alongside it would double the `docker exec`. `kill_node_targets_the_container` cannot
/// catch that — it looks for a marker the SPAWN already writes — so this test COUNTS.
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

    // The tail of `kill_one_liner` (sandbox_container.rs), emitted by the kill path
    // and by it alone, so counting it counts the kills exactly.
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

/// A realistic host `~/.claude` (+ sibling `.claude.json`) under `home`. Deliberate
/// mirror of the unit fixture `sandbox_staging::fabricate_home`; keep them in step.
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
    write(claude.join("skills/foo/skill.md"), "# skill\n");
    write_mode(
        claude.join("skills/foo/run.sh"),
        "#!/bin/sh\necho hi\n",
        0o755,
    );
    std::os::unix::fs::symlink("skill.md", claude.join("skills/foo/link.md")).unwrap();
    // Escapes ~/.claude: must be dereferenced into the staged tree, or it dangles
    // inside the container.
    write(
        home.join(".agents/skills/esc/SKILL.md"),
        "# escaped skill\n",
    );
    std::os::unix::fs::symlink("../../.agents/skills/esc", claude.join("skills/esc")).unwrap();
    write(claude.join("plugins/bar/plugin.json"), "{}\n");
    write(claude.join("agents/a.md"), "agent\n");
    write(claude.join("commands/c.md"), "cmd\n");
    write(claude.join("output-styles/s.md"), "style\n");
    write(claude.join("settings.json"), r#"{"hooks":{"Stop":[]}}"#);
    write(claude.join("settings.local.json"), r#"{"local":true}"#);
    // OUTSIDE the `full` allowlist: the staging floor is its single writer, in BOTH
    // modes. Stand-in content — the real host file carries an org OTEL bearer.
    write(claude.join(ORG_BASELINE_FILE), ORG_BASELINE);
    write_mode(
        claude.join(".credentials.json"),
        r#"{"token":"secret"}"#,
        0o600,
    );
    write(claude.join("CLAUDE.md"), "# global\n");
    write(claude.join("RTK.md"), "# rtk\n");
    // Must stay EXCLUDED from the staging.
    write(claude.join("history.jsonl"), "{\"cmd\":\"ls\"}\n");
    write(claude.join("file-history/big.bin"), "xxxxxxxxxx");
    write(claude.join("session-env/env-1/data"), "junk");
    // `prepare` must NOT copy this.
    write(
        claude.join("projects/-enc-host/old.jsonl"),
        "{\"host\":1}\n",
    );
    // PII-bearing: `full` stages it, then the floor merges onboarding + trust into it.
    write(
        home.join(".claude.json"),
        r#"{"host":"profile","oauthAccount":{"x":1}}"#,
    );
    // Host files OUTSIDE `~/.claude` that a profile MAY declare as extras. Present even
    // though the default `full` does not carry them: that is the point — a future slice
    // that starts staging them by default trips the two tests below.
    write(
        home.join(".gitconfig"),
        "[user]\n\tname = Host User\n\temail = host@example.com\n",
    );
    write(
        home.join(".config/gh/hosts.yml"),
        "github.com:\n  user: me\n",
    );
}

fn staged_home(daemon: &TestDaemon, run_id: &str) -> PathBuf {
    daemon
        .repo_root()
        .join(".pdo/sandbox")
        .join(run_id)
        .join("claude-home")
}

fn staged_json(daemon: &TestDaemon, run_id: &str) -> PathBuf {
    daemon
        .repo_root()
        .join(".pdo/sandbox")
        .join(run_id)
        .join(".claude.json")
}

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

    // Running ⇒ eager prep (the full walk + the floor) is done.
    let run = wait_node_status(&daemon, &run_id, "running").await;
    assert_eq!(run["nodes"][NODE_ID]["status"], "running", "run: {run}");

    let home = staged_home(&daemon, &run_id);
    assert!(
        wait_until(|| home.join("settings.json").is_file()).await,
        "staged settings.json should exist once the node is running"
    );

    assert!(home.join("skills/foo/skill.md").is_file());
    assert!(home.join("plugins/bar/plugin.json").is_file());
    assert!(home.join("agents/a.md").is_file());
    assert!(home.join("commands/c.md").is_file());
    assert!(home.join("output-styles/s.md").is_file());
    // The escaping skill must be a regular file, not a dangling symlink.
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
    // Floor guarantee G2: the org baseline is staged VERBATIM though it lives OUTSIDE
    // the `full` allowlist — the floor is its single writer.
    assert_eq!(
        std::fs::read_to_string(home.join(ORG_BASELINE_FILE)).unwrap(),
        ORG_BASELINE,
        "the org managed-settings baseline must be staged byte-for-byte"
    );
    // Floor guarantee G3: the bypass key is MERGED, so the host hooks survive; a naive
    // overwrite would drop them.
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

    // The `.claude.json` sibling lives OUTSIDE claude-home/.
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

    assert!(home.join("projects").is_dir());
    assert_eq!(std::fs::read_dir(home.join("projects")).unwrap().count(), 0);

    write_node_output(&daemon, &run_id, "full output\n");
    simulate_node_done(&daemon, &run_id).await;
    let run = wait_run_status(&daemon, &run_id, "completed").await;
    assert_eq!(run["status"], "completed", "full run must complete: {run}");
}

/// The only layer-3 test driving `minimal` with a real host `~/.claude` present, which is
/// where the floor's copy-vs-synthesis fork lives. Without a fabricated host, G2 takes its
/// no-op branch in every layer-3 test.
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

    // G2, COPY branch: the org baseline is staged even in `minimal`.
    assert_eq!(
        std::fs::read_to_string(home.join(ORG_BASELINE_FILE)).unwrap(),
        ORG_BASELINE,
        "the floor stages the org baseline in `minimal` too"
    );

    // G3, SYNTHESIS branch: the host `settings.json` carries `hooks`; the staged one
    // must carry the bypass key and NOTHING of the host.
    let settings: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(home.join("settings.json")).unwrap())
            .unwrap();
    assert_eq!(
        settings,
        serde_json::json!({ BYPASS_PERMISSIONS_KEY: true }),
        "`minimal` synthesises settings.json — it never copies the host's: {settings}"
    );

    // Nothing from the `full` profile may leak in.
    assert!(!home.join("skills").exists());
    assert!(!home.join("plugins").exists());
    assert!(!home.join("CLAUDE.md").exists());

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

    // Allowlist, not denylist.
    assert!(!home.join("history.jsonl").exists());
    assert!(!home.join("file-history").exists());
    assert!(!home.join("session-env").exists());
    assert!(!home.join("projects/-enc-host/old.jsonl").exists());
    assert!(home.join("projects").is_dir());
    assert_eq!(std::fs::read_dir(home.join("projects")).unwrap().count(), 0);
}

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
    assert!(
        wait_until(|| log_text(&log).contains("create")).await,
        "prep must `docker create` the container; log:\n{}",
        log_text(&log)
    );

    let specs = mount_specs(&log);
    let host_claude = daemon.repo_root().join(".claude");
    let host_json = daemon.repo_root().join(".claude.json");

    let expected_home_mount = format!(
        "{}:{}:rw",
        staged_home(&daemon, &run_id).display(),
        host_claude.display()
    );
    assert!(
        specs.contains(&expected_home_mount),
        "staged home must mount to <repo>/.claude (source = staging); specs={specs:?}"
    );

    // ADR-0031 §4: an entry outside `~/.claude` is COPIED then mounted, never bind-mounted
    // from the host. Inspect the source SEGMENT (split ':'), never `contains` — the mount
    // TARGETS legitimately ARE host paths here, so a substring check false-positives on
    // every spec.
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
    // The `full` default declares nothing outside `~/.claude`, so the extra-mount queue is
    // empty: exactly the 4 fixed mounts.
    assert_eq!(
        specs.len(),
        4,
        "`full` must add no `$HOME`-exception mount; specs={specs:?}"
    );
}

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
    // `~/.gitconfig` rides along: an agent hitting `unable to auto-detect email address`
    // reaches for `git config --global`, and a direct bind would rewrite the user's identity.
    let host_gitconfig = daemon.repo_root().join(".gitconfig");
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

    // The cleanup merge_back must land this on the host while config stays untouched.
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

    // Copy + trust seeding all land in the STAGED tree; the host config stays
    // byte-identical.
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
    // Transcripts, by contrast, DO merge back.
    assert!(
        host_claude.join("projects/-enc-test/t.jsonl").is_file(),
        "merge_back must land staged transcripts on the host"
    );
    assert!(
        !staging.exists(),
        "cleanup must purge the staging dir: {staging:?}"
    );
}

/// Like `write_fake_docker` but `image inspect` reports ABSENT, so `ensure_image` proceeds
/// past the fast path and the acquisition (pull vs build) becomes observable.
///
/// The choice is driven by the **staging profile**. The `ImageSource::Dockerfile` branch's
/// "never pulls" property is pinned by the `sandbox_image` unit test instead: at this layer
/// the only instance-wide way to reach it is the daemon's environment, which a shared test
/// process cannot set without racing every sibling test in this file.
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

/// Posing an image source on a profile is the ONLY way to choose a Run's image. A bare `image`
/// upsert materialises `minimal` with an empty diff, which is what it already resolves to.
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

fn log_has_subcommand(log: &Path, name: &str) -> bool {
    log_text(log).lines().any(|l| l == name)
}

/// `DEFAULT_PROFILE_IMAGE` is registry-pulled and hash-derived, so this needs no setup —
/// which is the point: on an untouched instance, a sandboxed Run pulls.
#[tokio::test]
async fn the_default_profile_image_pulls_the_sandbox_image() {
    ensure_pdo_on_path();
    let (_fake_dir, docker, log) = write_fake_docker_image_absent();
    let daemon =
        TestDaemon::spawn_with_docker_override(seed("#!/usr/bin/env bash\ntrue\n"), docker)
            .await
            .unwrap();

    // No PUT: `minimal` poses no image, so the profile default decides.
    let _run_id = start_run(&daemon, Some("minimal")).await;

    assert!(
        wait_until(|| log_has_subcommand(&log, "pull")).await,
        "a profile posing no image must `docker pull` the hash-derived image; log:\n{}",
        log_text(&log)
    );
    // The fake pull succeeds, so there must be no fallback build.
    assert!(
        !log_has_subcommand(&log, "build"),
        "a successful pull must NOT fall back to a local build; log:\n{}",
        log_text(&log)
    );
}

/// Argv of the `docker build` invocation. A fixed 6-arg window rather than "to the end":
/// unlike the in-module helper, the build is NOT the last invocation here (create/start/exec
/// follow it).
fn build_argv(log: &Path) -> Vec<String> {
    let content = log_text(log);
    let lines: Vec<String> = content.lines().map(str::to_string).collect();
    match lines.iter().position(|l| l == "build") {
        Some(i) => lines[i..(i + 6).min(lines.len())].to_vec(),
        None => Vec::new(),
    }
}

/// A `RunFailed`'s `reason` lives here, not on the Run projection.
async fn run_events(daemon: &TestDaemon, run_id: &str) -> Vec<serde_json::Value> {
    reqwest::get(format!("{}/runs/{run_id}/events", daemon.url()))
        .await
        .unwrap()
        .json::<Vec<serde_json::Value>>()
        .await
        .unwrap()
}

/// The 12-hex content hash, shelled out on purpose: it proves the daemon's Rust hash
/// matches the canonical CI recipe (`sha256sum | cut -c1-12`), not just itself.
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

/// The "no pull" half is non-vacuous precisely because the source is still the
/// registry-pulling default: what suppresses the pull is the LOCATION predicate, not a mode.
#[tokio::test]
async fn a_profile_dockerfile_builds_from_it_without_pulling() {
    ensure_pdo_on_path();
    let (_fake_dir, docker, log) = write_fake_docker_image_absent();
    let daemon =
        TestDaemon::spawn_with_docker_override(seed("#!/usr/bin/env bash\ntrue\n"), docker)
            .await
            .unwrap();

    // Self-contained (no COPY): the build context is empty by design, ADR-0030 §5.
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
    // The registry-pulling default is in force, yet no pull: the hash of a custom Dockerfile
    // cannot exist upstream. The fake pull would have SUCCEEDED, so this is a real signal.
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
    assert_eq!(
        argv.iter().position(|a| a == "-f").map(|i| &argv[i + 1]),
        Some(&custom.display().to_string()),
        "`docker build -f` must point at the resolved custom Dockerfile; argv: {argv:?}"
    );
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
    // The build context must stay the dedicated EMPTY dir, never sandbox_root and never
    // the repo (ADR-0030 §5).
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

    assert_eq!(
        std::fs::read(&custom).unwrap(),
        custom_bytes,
        "the seed must never write to a custom path"
    );
}

#[tokio::test]
async fn a_profile_dockerfile_that_vanished_fails_the_run_naming_path_and_tier() {
    // TOCTOU: the path passes the profile write's existence gate, then disappears before
    // the run. The prep must fail LOUD (ADR-0030 pt 4) rather than silently build the
    // seeded default — that would run an image the team never versioned.
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

    std::fs::remove_file(&custom).unwrap();

    let run_id = start_run(&daemon, Some("minimal")).await;
    let run = wait_run_status(&daemon, &run_id, "failed").await;
    assert_eq!(
        run["status"], "failed",
        "a vanished Dockerfile must fail the run, never fall back: {run}"
    );

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
    // The bail must precede the fast path AND the build.
    assert!(
        !log_has_subcommand(&log, "build"),
        "no build must be attempted for an unnameable image; log:\n{}",
        log_text(&log)
    );
}

async fn get_settings_json(daemon: &TestDaemon) -> serde_json::Value {
    reqwest::get(format!("{}/settings", daemon.url()))
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap()
}

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

/// A non-empty `input_template` keeps the prompt-required reject rule satisfied.
/// A Trigger is a Run template, so it needs its own target repo (ADR-0033).
async fn create_trigger(daemon: &TestDaemon, sandbox: Option<&str>) -> String {
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

#[tokio::test]
async fn get_settings_reports_docker_available_with_working_fake() {
    ensure_pdo_on_path();
    // The standard fake docker answers `version` via its `*) exit 0` arm.
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

#[tokio::test]
async fn trigger_without_sandbox_defers_to_instance_default() {
    ensure_pdo_on_path();
    let (_fake_dir, docker, _log) = write_fake_docker();
    let daemon =
        TestDaemon::spawn_with_docker_override(seed("#!/usr/bin/env bash\ntrue\n"), docker)
            .await
            .unwrap();

    put_default_sandbox(&daemon, "full").await;
    let trigger_id = create_trigger(&daemon, None).await;
    let run_id = fire_trigger_and_get_run(&daemon, &trigger_id).await;

    let run = get_run(&daemon, &run_id).await;
    assert_eq!(
        run["sandbox"], "full",
        "a null-sandbox Trigger must defer to the instance default (full): {run}"
    );
}

// The bug this section pins: `create_run`'s detached prep task waits for `ensure_ready`
// before advancing the Run, but the pipeline watcher did not. inotify reports the FIRST
// read of a fresh `<run>/pipeline.yaml` as an external modification, so merely opening the
// Run in the UI woke `handle_run_pipeline_modifications` mid-prep, which called the same
// advance path with no precondition — and the node's tail `docker exec`ed into a container
// that did not exist yet.
//
// A fake `docker` whose `create` SLEEPS gives a deterministic prep window; in production
// the window is ~1 GB of `~/.claude` staging (83-87 s measured for a 2 GB profile).

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
    // Longer than the watcher's ~1s debounce plus the round trips below, so the
    // `pipeline_modified` advance provably lands inside the prep window.
    let (_fake_dir, docker, log) = write_slow_create_docker(6);
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
        "precondition: the Run must be mid-prep for this test to mean anything: {run}"
    );

    // In production the UI's *read* wakes the watcher; a write is the same event to the
    // daemon and is what a test can trigger deterministically.
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

    // The watcher-driven advance ran, yet the container is still absent.
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

    // The deferred spawn is replayed once the container is up.
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

    let t = log_text(&log);
    assert!(
        t.contains("exec") && t.contains(&format!("pdo-sbx-{run_id}")),
        "the replayed tail must run inside the Run's container; log:\n{t}"
    );
}

/// The manager's runtime preamble, written by `tmux_session_manager::spawn` *before* tmux is
/// invoked — so it exists whether or not the harmless tail actually started.
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

/// A sandboxed manager execs into the Run's container, where `localhost` is the container —
/// so the `curl` lines of its own preamble must name the host gateway, or every call gets
/// connection-refused and the manager reports "the daemon is down" on a healthy daemon.
///
/// The assertion that matters is the ABSENCE of `localhost:<port>`: a preamble that merely
/// *mentions* the gateway while still printing host-only `curl` commands reproduces the bug.
#[tokio::test]
async fn sandboxed_manager_preamble_uses_the_container_side_url() {
    ensure_pdo_on_path();
    let (_fake_dir, docker, _log) = write_fake_docker();
    let daemon =
        TestDaemon::spawn_with_docker_override(seed("#!/usr/bin/env bash\ntrue\n"), docker)
            .await
            .unwrap();

    let run_id = start_run(&daemon, Some("minimal")).await;
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
    // The command endpoint is the surface the bug removed.
    assert!(
        preamble.contains(&format!(
            "POST http://host.docker.internal:{port}/runs/{run_id}/commands"
        )),
        "preamble:\n{preamble}"
    );
}

/// An `off` Run's manager runs on the host, so its preamble must stay `localhost` with no
/// gateway hostname anywhere.
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
    assert_eq!(
        log_text(&log),
        "",
        "the `off` path must not invoke docker at all"
    );
}

/// The `argv.log` window of the identity `docker exec`. The `0:0` line is its UNIQUE witness:
/// every other invocation carries no `--user` at all, or the host `<uid>:<gid>`, never root.
fn identity_exec_argv(log: &Path) -> Option<Vec<String>> {
    let content = log_text(log);
    let lines: Vec<String> = content.lines().map(str::to_string).collect();
    let user_at = lines.iter().position(|l| l == "0:0")?;
    let at = user_at.checked_sub(2)?; // `exec`, `--user`, `0:0`
    Some(lines[at..(at + 7).min(lines.len())].to_vec())
}

/// A container runs as `--user <uid>:<gid>` NUMERIC, and `ubuntu:24.04` only knows uid 1000.
/// On any other host uid, `sudo` calls `getpwuid()` before applying NOPASSWD and gives up
/// ("you do not exist in the passwd database"), costing the agent `apt install`. Hence one
/// `docker exec --user 0:0` right after the `start`, appending the missing lines to the
/// image's REAL `/etc/passwd` and `/etc/group` behind a `getent` guard.
///
/// The daemon runs under the live host uid, so this asserts the SHAPE, never a uid value.
/// The home field must be the very path the create posed as `-e HOME=`: `getent`, `~` and
/// the environment naming three different directories is the class of bug this pins.
///
/// The negative control is stronger than re-reading `off_run_never_invokes_docker`: it
/// proves the injection is gated by the mode on a daemon that HAS just performed one.
#[tokio::test]
async fn sandbox_prep_identifies_the_host_uid_in_the_container() {
    ensure_pdo_on_path();
    let (_fake_dir, docker, log) = write_fake_docker();
    // Writes its declared output on the host, so the negative control completes for real
    // instead of failing output validation.
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

    // No `-e`: a session marker here would make the first targeted kill take this exec
    // down too.
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
    // The home field IS the `-e HOME=` of the create.
    let home = daemon.repo_root().display().to_string();
    assert!(
        script.contains(&format!("PDO sandbox:{home}:/bin/bash")),
        "the injected home must be the container's `$HOME` ({home}): {script}"
    );

    // Must run AFTER the start: an exec into a container that is not up yet fails.
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

    // Negative control on the SAME daemon.
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

/// ADR-0037 — `restart_node` mid-prep is a `409`, raised **before** the kill.
///
/// The `sandbox_spawn_block` precondition used to be discovered only inside `spawn_node`,
/// i.e. after the arm had already killed the tmux session and appended its `CommandIssued`,
/// and the caller was then told `200 {"ok":true}` because the `SpawnOutcome::Deferred` was
/// thrown away. The predicate is pure, so it is evaluated at the head of the arm: no session
/// dies for a container that is still building.
#[tokio::test]
async fn restart_node_mid_prep_is_refused_before_anything_is_touched() {
    ensure_pdo_on_path();
    // Longer than the round trips below, so the restart provably lands inside the window.
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
    // The probe runs ahead of the kill, so nothing was destroyed.
    assert_eq!(body["session_killed"], false, "{body}");

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
