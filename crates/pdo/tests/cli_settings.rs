//! Layer 3a — `pdo settings` and `pdo update` through the real binary, against
//! a real daemon: the instance settings read and changed with the daemon's
//! validation (a value Settings refuses is refused, nothing written), the
//! agentic and staging profiles, the provisioning rules, the caller's
//! Apparence, the telemetry export; `4` for a subject the daemon does not
//! know, `1` for a refusal.

use crate::cli_operator::pdo;
use crate::common::flow;
use crate::common::TestDaemon;

fn seed(repo: &std::path::Path) -> anyhow::Result<()> {
    flow::seed_solo(repo, &[("solo", false)])
}

#[tokio::test]
async fn an_instance_setting_is_set_as_settings_saves_it_and_refused_as_it_refuses() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();

    let set = pdo(&daemon, &["settings", "set", "session_cap=3"]).await;
    assert_eq!(set.ok(), "session_cap\t3\tstored\n");
    let page = daemon.get_json("/settings").await;
    assert_eq!(page["session_cap"]["stored"], 3, "{page}");
    assert_eq!(
        pdo(&daemon, &["settings", "get", "session_cap"]).await.ok(),
        "session_cap\t3\tstored\n"
    );
    let all = pdo(&daemon, &["settings", "get"]).await;
    assert!(all.ok().contains("session_cap\t3\tstored\n"), "{}", all.stdout);
    let json = pdo(&daemon, &["settings", "get", "session_cap", "--json"])
        .await
        .json();
    assert_eq!(json["session_cap"]["effective"], 3);

    // Invalid: the daemon's reason, exit 1, nothing written — even beside a
    // valid change.
    let refused = pdo(
        &daemon,
        &["settings", "set", "session_cap=0", "public_url=https://x.example"],
    )
    .await;
    assert_eq!(refused.code, Some(1), "{}", refused.stderr);
    assert!(
        refused.stderr.contains("session_cap must be >= 1"),
        "{}",
        refused.stderr
    );
    let page = daemon.get_json("/settings").await;
    assert_eq!(page["session_cap"]["stored"], 3);
    assert_ne!(page["public_url"]["stored"], "https://x.example");

    // A string, and a type error named by the daemon.
    pdo(&daemon, &["settings", "set", "public_url=https://pdo.example.net/"])
        .await
        .ok();
    let page = daemon.get_json("/settings").await;
    assert_eq!(page["public_url"]["stored"], "https://pdo.example.net");
    let typed = pdo(&daemon, &["settings", "set", "session_cap=many"]).await;
    assert_eq!(typed.code, Some(1), "{}", typed.stderr);
    assert!(typed.stderr.contains("invalid settings payload"), "{}", typed.stderr);

    // The instance's skills only gain skills of the bank, as a Projet's do.
    let foreign = pdo(
        &daemon,
        &["settings", "set", r#"skills=[{"id":"no-such-skill"}]"#],
    )
    .await;
    assert_eq!(foreign.code, Some(1), "{}", foreign.stderr);
    assert!(
        foreign
            .stderr
            .contains("not a skill of the bank: no-such-skill"),
        "{}",
        foreign.stderr
    );
    let skills = pdo(
        &daemon,
        &["settings", "set", r#"skills=[{"id":"pdo-interactive"}]"#, "--json"],
    )
    .await
    .json();
    assert_eq!(skills["skills"][0]["id"], "pdo-interactive", "{skills}");

    let unknown = pdo(&daemon, &["settings", "get", "no_such_setting"]).await;
    assert_eq!(unknown.code, Some(1));
    assert!(unknown.stderr.contains("no setting named `no_such_setting`"));
    let usage = pdo(&daemon, &["settings", "set", "no-equal-sign"]).await;
    assert_eq!(usage.code, Some(1), "{}", usage.stderr);
}

#[tokio::test]
async fn profiles_are_created_changed_and_deleted_by_the_cli() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();

    let created = pdo(
        &daemon,
        &[
            "settings",
            "agent-profile",
            "create",
            "Reviewer",
            "--harness",
            "claude",
            "--model",
            "opus",
            "--json",
        ],
    )
    .await
    .json();
    let id = created["id"].as_str().unwrap().to_string();
    let list = pdo(&daemon, &["settings", "agent-profile", "list"]).await;
    assert!(
        list.ok().contains(&format!("{id}\tReviewer\tclaude\topus\t-\n")),
        "{}",
        list.stdout
    );
    let updated = pdo(
        &daemon,
        &["settings", "agent-profile", "update", &id, "--effort", "high", "--model", ""],
    )
    .await;
    assert_eq!(
        updated.ok(),
        format!("{id}\tReviewer\tclaude\t-\thigh updated\n")
    );
    assert_eq!(
        pdo(&daemon, &["settings", "agent-profile", "referents", &id])
            .await
            .ok(),
        "none\n"
    );
    pdo(&daemon, &["settings", "agent-profile", "delete", &id])
        .await
        .ok();
    let gone = pdo(&daemon, &["settings", "agent-profile", "show", &id]).await;
    assert_eq!(gone.code, Some(4), "{}", gone.stderr);

    // A staging profile: created from the flags, changed, shown, deleted.
    let saved = pdo(
        &daemon,
        &[
            "settings",
            "sandbox-profile",
            "set",
            "web",
            "--env",
            "NODE_ENV=test",
            "--registry",
            "node:22",
            "--json",
        ],
    )
    .await
    .json();
    assert_eq!(saved["env"]["NODE_ENV"], "test", "{saved}");
    assert_eq!(saved["image"]["ref"], "node:22", "{saved}");
    pdo(
        &daemon,
        &["settings", "sandbox-profile", "set", "web", "--unset-env", "NODE_ENV", "--default-image"],
    )
    .await
    .ok();
    let shown = pdo(&daemon, &["settings", "sandbox-profile", "show", "web"]).await;
    assert!(shown.ok().contains("image\tdefault\n"), "{}", shown.stdout);
    assert!(!shown.stdout.contains("NODE_ENV"), "{}", shown.stdout);
    let refused = pdo(
        &daemon,
        &["settings", "set", "default_sandbox=no-such-profile"],
    )
    .await;
    assert_eq!(refused.code, Some(1), "{}", refused.stderr);
    pdo(&daemon, &["settings", "sandbox-profile", "delete", "web"])
        .await
        .ok();
}

#[tokio::test]
async fn provisioning_appearance_telemetry_and_update_are_read_and_written() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let dir = tempfile::tempdir().unwrap();

    let rules = dir.path().join("rules.json");
    std::fs::write(&rules, r#"{"copy":[".env"],"hardlink":[],"symlink":[]}"#).unwrap();
    let set = pdo(
        &daemon,
        &["settings", "provisioning", "set", rules.to_str().unwrap()],
    )
    .await;
    assert_eq!(set.ok(), "copy: .env\nhardlink: none\nsymlink: none\n");
    let stored = daemon.get_json("/settings/provisioning").await;
    assert_eq!(stored["copy"], serde_json::json!([".env"]), "{stored}");
    std::fs::write(daemon.repo_root().join(".env"), "A=1\n").unwrap();
    let preview = pdo(
        &daemon,
        &["settings", "provisioning", "preview", "--repo", "."],
    )
    .await;
    assert!(
        preview.ok().contains("copy\t.env\tinstance\t.env"),
        "{}",
        preview.stdout
    );

    assert!(pdo(&daemon, &["settings", "appearance"])
        .await
        .ok()
        .contains("never written"));
    let doc = dir.path().join("appearance.json");
    std::fs::write(&doc, r#"{"version":1,"theme":"classic","dark_mode":true}"#).unwrap();
    let written = pdo(
        &daemon,
        &["settings", "appearance", "--file", doc.to_str().unwrap()],
    )
    .await;
    assert!(written.ok().starts_with("theme\tclassic\ndark_mode\ttrue\n"), "{}", written.stdout);
    std::fs::write(&doc, r#"{"version":0}"#).unwrap();
    let refused = pdo(
        &daemon,
        &["settings", "appearance", "--file", doc.to_str().unwrap()],
    )
    .await;
    assert_eq!(refused.code, Some(1), "{}", refused.stderr);

    let telemetry = pdo(&daemon, &["settings", "telemetry"]).await;
    assert!(telemetry.ok().starts_with("not exporting"), "{}", telemetry.stdout);
    let test = pdo(&daemon, &["settings", "telemetry", "--test"]).await;
    assert_eq!(test.code, Some(1), "no collector: {}", test.stderr);

    let harness = pdo(&daemon, &["settings", "refresh-harness", "no-such-harness"]).await;
    assert_eq!(harness.code, Some(4), "{}", harness.stderr);

    let status = pdo(&daemon, &["update", "status", "--json"]).await.json();
    assert!(status["installed_version"].is_string(), "{status}");
    assert!(pdo(&daemon, &["update", "changelog"]).await.ok().len() > 1);
    let log = pdo(&daemon, &["update", "log", "no-such-attempt"]).await;
    assert_ne!(log.code, Some(0), "{}", log.stderr);
}
