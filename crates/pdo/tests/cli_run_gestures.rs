//! Layer 3a — the gestures of `pdo run` and `pdo node` through the real
//! binary, against a real daemon: a Run cancelled then retried, paused and
//! resumed, renamed, a node stopped or killed then retried, skipped, a Run archived then
//! forgotten; the reads (show, events, refs, costs, an injected artifact, the
//! Run's pipeline). `4` for a Run the daemon does not know, `1` for a refusal.

use crate::cli_operator::pdo;
use crate::common::flow::{self, wait_node_status, wait_run, wait_run_status};
use crate::common::TestDaemon;

fn seed(repo: &std::path::Path) -> anyhow::Result<()> {
    flow::seed_solo(repo, &[("solo", false)])
}

#[tokio::test]
async fn a_run_is_cancelled_then_retried_by_the_cli() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = flow::create_run(&daemon, "solo").await;
    wait_node_status(&daemon, &run_id, "worker", "running").await;

    let shown = pdo(&daemon, &["run", "show", &run_id]).await;
    assert!(
        shown.ok().contains("  worker\trunning\titer 1\n"),
        "{}",
        shown.stdout
    );

    let cancelled = pdo(&daemon, &["run", "cancel", &run_id]).await;
    assert_eq!(cancelled.ok(), format!("{run_id} cancelled\n"));
    let run = wait_run_status(&daemon, &run_id, "halted").await;
    assert_eq!(run["failure_reason"], "cancelled");
    assert_eq!(run["nodes"]["worker"]["status"], "stopped");
    assert!(
        !flow::tmux_session_exists(&daemon, &flow::session_name(&run_id, "worker", 1)),
        "the worker's session is gone"
    );

    // Same refusal as the UI: a finished Run is not cancelled again.
    let again = pdo(&daemon, &["run", "cancel", &run_id]).await;
    assert_eq!(again.code, Some(1), "{}", again.stdout);
    assert!(
        again.stderr.contains("cannot cancel run"),
        "{}",
        again.stderr
    );

    let retried = pdo(&daemon, &["run", "retry", &run_id, "--json"])
        .await
        .json();
    let new_id = retried["run_id"].as_str().unwrap().to_string();
    assert_ne!(new_id, run_id);
    wait_node_status(&daemon, &new_id, "worker", "running").await;
    wait_run_status(&daemon, &run_id, "archived").await;
}

#[tokio::test]
async fn a_run_is_paused_resumed_renamed_and_its_node_killed_then_retried() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = flow::create_run(&daemon, "solo").await;
    wait_node_status(&daemon, &run_id, "worker", "running").await;

    assert_eq!(
        pdo(&daemon, &["run", "pause", &run_id]).await.ok(),
        format!("{run_id} paused\n")
    );
    wait_run_status(&daemon, &run_id, "paused").await;
    pdo(&daemon, &["run", "resume", &run_id]).await.ok();
    wait_run_status(&daemon, &run_id, "running").await;

    assert_eq!(
        pdo(&daemon, &["run", "rename", &run_id, "Renamed"])
            .await
            .ok(),
        format!("{run_id} renamed Renamed\n")
    );
    wait_run(&daemon, &run_id, "renamed", |run| run["name"] == "Renamed").await;

    let preview = pdo(&daemon, &["node", "retry", &run_id, "worker", "--preview"]).await;
    assert_eq!(
        preview.ok(),
        "retrying worker invalidates end (0 with artifacts)\n"
    );

    // The node panel's Stop, then its Retry.
    assert_eq!(
        pdo(&daemon, &["node", "stop", &run_id, "worker"])
            .await
            .ok(),
        "worker stopped\n"
    );
    wait_node_status(&daemon, &run_id, "worker", "stopped").await;
    pdo(&daemon, &["node", "retry", &run_id, "worker"])
        .await
        .ok();
    wait_node_status(&daemon, &run_id, "worker", "running").await;

    assert_eq!(
        pdo(&daemon, &["node", "kill", &run_id, "worker"])
            .await
            .ok(),
        "worker killed\n"
    );
    wait_node_status(&daemon, &run_id, "worker", "failed").await;

    assert_eq!(
        pdo(&daemon, &["node", "retry", &run_id, "worker"])
            .await
            .ok(),
        "worker retried\n"
    );
    wait_node_status(&daemon, &run_id, "worker", "running").await;

    let prompt = pdo(&daemon, &["node", "prompt", &run_id, "worker"]).await;
    assert!(!prompt.ok().is_empty(), "the worker received a prompt");

    // An unknown node of a known Run: the daemon's 404, exit 4.
    let unknown = pdo(&daemon, &["node", "prompt", &run_id, "nobody"]).await;
    assert_eq!(unknown.code, Some(4), "{}", unknown.stderr);
}

#[tokio::test]
async fn a_skipped_node_ends_the_run_which_is_archived_then_forgotten() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = flow::create_run(&daemon, "solo").await;
    wait_node_status(&daemon, &run_id, "worker", "running").await;

    let written = pdo(
        &daemon,
        &[
            "run",
            "inject",
            &run_id,
            "notes/hello.md",
            "--content",
            "hello",
        ],
    )
    .await;
    assert_eq!(written.ok(), format!("{run_id} notes/hello.md written\n"));
    let artifact = pdo(&daemon, &["run", "artifact", &run_id, "notes/hello.md"]).await;
    assert_eq!(artifact.ok(), "hello");

    let skipped = pdo(
        &daemon,
        &["node", "skip", &run_id, "worker", "--output", "out=skipped"],
    )
    .await;
    assert_eq!(skipped.ok(), "worker skipped\n");
    wait_run_status(&daemon, &run_id, "completed").await;

    let events = pdo(&daemon, &["run", "events", &run_id]).await;
    assert!(events.ok().contains("\trun_started\t"), "{}", events.stdout);
    let refs = pdo(&daemon, &["run", "refs", &run_id, "--json"])
        .await
        .json();
    assert!(refs["refs"].is_array(), "{refs}");
    let costs = pdo(&daemon, &["run", "costs", &run_id]).await;
    assert!(
        costs.ok().starts_with(&format!("{run_id}\t")),
        "{}",
        costs.stdout
    );
    let pipeline = pdo(&daemon, &["run", "pipeline", "show", &run_id]).await;
    assert!(pipeline.ok().contains("name: solo"), "{}", pipeline.stdout);
    let health = pdo(&daemon, &["run", "stale-health"]).await;
    assert!(health.ok().starts_with("last tick "), "{}", health.stdout);

    let archived = pdo(&daemon, &["run", "archive", &run_id]).await;
    assert_eq!(archived.ok(), format!("{run_id} archived\n"));
    wait_run_status(&daemon, &run_id, "archived").await;

    // As in the UI: an archived Run is not reopened.
    let reopen = pdo(&daemon, &["run", "reopen", &run_id]).await;
    assert_eq!(reopen.code, Some(1), "{}", reopen.stdout);

    assert_eq!(
        pdo(&daemon, &["run", "forget", &run_id]).await.ok(),
        format!("{run_id} forgotten\n")
    );
}

#[tokio::test]
async fn a_failed_run_is_reopened_and_an_unknown_run_exits_4() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = flow::create_run(&daemon, "solo").await;
    wait_node_status(&daemon, &run_id, "worker", "running").await;
    pdo(&daemon, &["run", "cancel", &run_id]).await.ok();
    wait_run_status(&daemon, &run_id, "halted").await;

    assert_eq!(
        pdo(&daemon, &["run", "reopen", &run_id]).await.ok(),
        format!("{run_id} reopened\n")
    );
    wait_run_status(&daemon, &run_id, "running").await;

    for args in [
        vec!["run", "show", "no-such-run"],
        vec!["run", "cancel", "no-such-run"],
        vec!["run", "pipeline", "show", "no-such-run"],
    ] {
        let out = pdo(&daemon, &args).await;
        assert_eq!(out.code, Some(4), "{args:?}: {}{}", out.stdout, out.stderr);
    }
}

/// A mistyped node id on a finished Run: every node gesture exits 4, and the
/// Run is left as it was, not reopened, with no ghost node in its state.
#[tokio::test]
async fn an_unknown_node_exits_4_and_leaves_a_finished_run_untouched() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = flow::create_run(&daemon, "solo").await;
    wait_node_status(&daemon, &run_id, "worker", "running").await;
    pdo(&daemon, &["run", "cancel", &run_id]).await.ok();
    wait_run_status(&daemon, &run_id, "halted").await;
    // The events a gesture could write. `pipeline_modified` is left out: the
    // pipeline watcher records it on its own schedule once the Run's snapshot
    // file lands, whatever the gestures do.
    let events = |daemon: &TestDaemon| {
        let path = format!("/runs/{run_id}/events");
        let daemon_url = daemon.url();
        async move {
            reqwest::get(format!("{daemon_url}{path}"))
                .await
                .unwrap()
                .json::<serde_json::Value>()
                .await
                .unwrap()
                .as_array()
                .unwrap()
                .iter()
                .filter(|event| event["kind"] != "pipeline_modified")
                .count()
        }
    };
    let before = events(&daemon).await;

    for gesture in [
        "skip", "kill", "restart", "recover", "done", "start", "stop", "retry", "release",
    ] {
        let out = pdo(&daemon, &["node", gesture, &run_id, "ghost"]).await;
        assert_eq!(out.code, Some(4), "{gesture}: {}{}", out.stdout, out.stderr);
        assert!(out.stderr.contains("ghost"), "{gesture}: {}", out.stderr);
    }
    for args in [
        vec!["node", "extend", &run_id, "ghost", "2"],
        vec!["node", "retry", &run_id, "ghost", "--preview"],
    ] {
        let out = pdo(&daemon, &args).await;
        assert_eq!(out.code, Some(4), "{args:?}: {}", out.stderr);
    }

    let run = daemon.get_json(&format!("/runs/{run_id}")).await;
    assert_eq!(run["status"], "halted", "{run}");
    assert!(run["nodes"].get("ghost").is_none(), "{run}");
    assert_eq!(events(&daemon).await, before, "nothing was written");
}
