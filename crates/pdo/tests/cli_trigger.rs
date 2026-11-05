//! Layer 3a — `pdo trigger` through the real binary, against a real daemon:
//! created, changed, paused and resumed (one and all), fired into a Run, its
//! fires and a guard's dry run read, then deleted and found in `pdo audit`; `4` for a Trigger the
//! daemon does not know, `1` for a refusal.

use crate::cli_operator::pdo;
use crate::common::flow;
use crate::common::TestDaemon;

fn seed(repo: &std::path::Path) -> anyhow::Result<()> {
    flow::seed_solo(repo, &[("solo", false)])
}

#[tokio::test]
async fn a_trigger_is_created_fired_paused_and_deleted_by_the_cli() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let repo = daemon.target_repo();

    let created = pdo(
        &daemon,
        &[
            "trigger",
            "create",
            "nightly",
            "--pipeline",
            "solo",
            "--cron",
            "0 3 * * *",
            "--repo",
            ".",
            "--agent-harness",
            "claude",
            "--model",
            "opus",
            "--input",
            "go {{env}}",
            "--var",
            "env=prod",
            "--json",
        ],
    )
    .await
    .json();
    let id = created["id"].as_str().unwrap().to_string();
    assert_eq!(created["enabled"], true);
    assert_eq!(created["cron"], "0 3 * * *");
    assert_eq!(created["variables"]["env"], "prod");
    assert_eq!(
        created["target_repo"],
        repo.as_str(),
        "--repo . is the working directory"
    );
    assert_eq!(
        created["agent_choice"],
        serde_json::json!({ "mode": "custom", "harness": "claude", "model": "opus" })
    );

    let list = pdo(&daemon, &["trigger", "list"]).await;
    assert!(
        list.ok()
            .contains(&format!("{id}\tenabled\t0 3 * * *\tsolo\tnightly\t")),
        "{}",
        list.stdout
    );
    let shown = pdo(&daemon, &["trigger", "show", &id]).await;
    assert!(
        shown
            .ok()
            .starts_with(&format!("{id}  nightly  (enabled)\n")),
        "{}",
        shown.stdout
    );
    assert!(
        shown.stdout.contains(&format!("repository: {repo}\n")),
        "{}",
        shown.stdout
    );

    // Changed: only the flags given.
    let updated = pdo(
        &daemon,
        &[
            "trigger",
            "update",
            &id,
            "--cron",
            "30 4 * * 1",
            "--guard",
            "exit 0",
            "--json",
        ],
    )
    .await
    .json();
    assert_eq!(updated["cron"], "30 4 * * 1");
    assert_eq!(updated["guard_command"], "exit 0");
    assert_eq!(updated["input_template"], "go {{env}}");
    let unguarded = pdo(
        &daemon,
        &["trigger", "update", &id, "--clear-guard", "--json"],
    )
    .await
    .json();
    assert!(
        unguarded["guard_command"].as_str().unwrap_or("").is_empty(),
        "{unguarded}"
    );

    // A skill of the bank goes on the Trigger; one the bank does not hold is
    // refused, and one the Trigger already carries is kept as the UI keeps it.
    let skilled = pdo(
        &daemon,
        &[
            "trigger",
            "update",
            &id,
            "--skill",
            "pdo-interactive=Interactif",
            "--json",
        ],
    )
    .await
    .json();
    assert_eq!(
        skilled["skills"],
        serde_json::json!([{ "id": "pdo-interactive", "name": "Interactif" }])
    );
    let unknown = pdo(
        &daemon,
        &[
            "trigger",
            "update",
            &id,
            "--skill",
            "pdo-interactive",
            "--skill",
            "no-such-skill",
        ],
    )
    .await;
    assert_eq!(unknown.code, Some(1), "{}", unknown.stderr);
    assert!(
        unknown
            .stderr
            .contains("not a skill of the bank: no-such-skill"),
        "{}",
        unknown.stderr
    );
    let kept = daemon.get_json(&format!("/triggers/{id}")).await;
    assert_eq!(
        kept["skills"], skilled["skills"],
        "a refusal writes nothing"
    );

    // Paused: the UI's toggle, and a paused Trigger does not fire.
    let paused = pdo(&daemon, &["trigger", "pause", &id]).await;
    assert_eq!(paused.ok(), format!("{id}\tnightly paused\n"));
    assert_eq!(
        daemon.get_json(&format!("/triggers/{id}")).await["enabled"],
        false
    );
    let refused = pdo(&daemon, &["trigger", "fire", &id]).await;
    assert_eq!(refused.code, Some(1), "{}", refused.stderr);
    pdo(&daemon, &["trigger", "resume", &id]).await.ok();
    assert_eq!(
        daemon.get_json(&format!("/triggers/{id}")).await["enabled"],
        true
    );

    // Fired: a Run starts, and the fire is in the history as manual.
    let fired = pdo(&daemon, &["trigger", "fire", &id, "--json"])
        .await
        .json();
    assert_eq!(fired["fired"], true, "{fired}");
    let run_id = fired["run_id"].as_str().unwrap();
    let run = daemon.get_json(&format!("/runs/{run_id}")).await;
    assert_eq!(run["run_id"], run_id, "{run}");
    let fires = pdo(&daemon, &["trigger", "fires", &id]).await;
    assert!(
        fires.ok().contains(&format!("\tmanual\tfired\t{run_id}\t")),
        "{}",
        fires.stdout
    );

    // A guard's dry run, without a fire.
    let pass = pdo(
        &daemon,
        &[
            "trigger",
            "test-guard",
            "--guard",
            "echo ready",
            "--repo",
            ".",
        ],
    )
    .await;
    assert_eq!(pass.ok(), "pass: a fire would go ahead\nstdout:\nready\n");
    let skip = pdo(
        &daemon,
        &[
            "trigger",
            "test-guard",
            "--guard",
            "exit 3",
            "--repo",
            &repo,
            "--json",
        ],
    )
    .await
    .json();
    assert_eq!(skip["outcome"], "skip");
    assert_eq!(skip["exit_code"], 3);

    // The global switch, and the heartbeat that reports it.
    assert_eq!(
        pdo(&daemon, &["trigger", "pause", "--all"]).await.ok(),
        "every Trigger paused\n"
    );
    let health = pdo(&daemon, &["trigger", "health", "--json"]).await.json();
    assert_eq!(health["paused"], true);
    let health = pdo(&daemon, &["trigger", "health"]).await;
    assert!(
        health.ok().ends_with("· every Trigger paused\n"),
        "{}",
        health.stdout
    );
    pdo(&daemon, &["trigger", "resume", "--all"]).await.ok();
    assert_eq!(daemon.get_json("/triggers/health").await["paused"], false);

    // Deleted: gone, and then unknown.
    assert_eq!(
        pdo(&daemon, &["trigger", "delete", &id, "--json"])
            .await
            .json(),
        serde_json::json!({ "id": id, "deleted": true })
    );
    assert!(!pdo(&daemon, &["trigger", "list"]).await.ok().contains(&id));
    let unknown = pdo(&daemon, &["trigger", "show", &id]).await;
    assert_eq!(unknown.code, Some(4), "{}", unknown.stderr);
    assert!(
        unknown.stderr.contains("trigger not found"),
        "{}",
        unknown.stderr
    );

    // The journal holds its creation and its deletion.
    let audit = pdo(
        &daemon,
        &["audit", "--target-kind", "trigger", "--target-id", &id],
    )
    .await;
    assert!(
        audit.ok().contains("\ttrigger.created\ttrigger\t"),
        "{}",
        audit.stdout
    );
    assert!(
        audit.stdout.contains("\ttrigger.deleted\ttrigger\t"),
        "{}",
        audit.stdout
    );
    let journal = pdo(&daemon, &["audit", "--limit", "1", "--json"])
        .await
        .json();
    assert_eq!(journal.as_array().map(Vec::len), Some(1), "{journal}");
}

#[tokio::test]
async fn a_trigger_the_daemon_refuses_exits_1_with_its_reason() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let bad_cron = pdo(
        &daemon,
        &[
            "trigger",
            "create",
            "n",
            "--pipeline",
            "solo",
            "--cron",
            "every day",
            "--repo",
            &daemon.target_repo(),
            "--input",
            "go",
        ],
    )
    .await;
    assert_eq!(bad_cron.code, Some(1), "{}", bad_cron.stderr);
    assert!(
        bad_cron.stderr.contains("invalid cron expression"),
        "{}",
        bad_cron.stderr
    );

    let unknown_skill = pdo(
        &daemon,
        &[
            "trigger",
            "create",
            "n",
            "--pipeline",
            "solo",
            "--cron",
            "0 3 * * *",
            "--repo",
            &daemon.target_repo(),
            "--input",
            "go",
            "--skill",
            "no-such-skill",
        ],
    )
    .await;
    assert_eq!(unknown_skill.code, Some(1), "{}", unknown_skill.stderr);
    assert!(
        unknown_skill
            .stderr
            .contains("not a skill of the bank: no-such-skill"),
        "{}",
        unknown_skill.stderr
    );
    assert_eq!(
        daemon.get_json("/triggers").await.as_array().map(Vec::len),
        Some(0),
        "a refused Trigger is not created"
    );

    let missing = pdo(&daemon, &["trigger", "pause"]).await;
    assert_eq!(
        missing.code,
        Some(2),
        "clap refuses a pause of nothing: {}",
        missing.stderr
    );
    let fire = pdo(&daemon, &["trigger", "fire", "no-such-trigger"]).await;
    assert_eq!(fire.code, Some(4), "{}", fire.stderr);
}
