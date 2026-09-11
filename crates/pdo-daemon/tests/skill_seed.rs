//! Layer 3a — the seeded skill over HTTP (#722, spec #719, ADR-0064).
//!
//! A **real daemon** over a tempdir: the seed runs at startup, so every
//! assertion is an HTTP response of the bank surface — the skill is present in
//! the « PDO » folder, locked against edit and delete — or a file on disk. The
//! re-seed on a content change is a unit-level concern (`skill_seed.rs`): the
//! built-in content cannot change inside a test binary.

use crate::common::TestDaemon;
use reqwest::StatusCode;

const SEEDED_ID: &str = "pdo-orchestrate";

async fn get_json(daemon: &TestDaemon, path: &str) -> serde_json::Value {
    reqwest::get(format!("{}{path}", daemon.url()))
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

async fn put(daemon: &TestDaemon, path: &str, body: serde_json::Value) -> reqwest::Response {
    reqwest::Client::new()
        .put(format!("{}{path}", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap()
}

#[tokio::test]
async fn startup_seeds_the_skill_in_the_pdo_folder_and_flags_it_locked() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let bank = get_json(&daemon, "/settings/skills").await;
    let folders = bank["folders"].as_array().unwrap();
    assert_eq!(folders.len(), 1, "{bank}");
    assert_eq!(folders[0]["id"], "skf-pdo");
    assert_eq!(folders[0]["name"], "PDO");

    let skills = bank["skills"].as_array().unwrap();
    assert_eq!(skills.len(), 1, "{bank}");
    assert_eq!(skills[0]["id"], SEEDED_ID);
    assert_eq!(skills[0]["name"], "pdo-orchestrate");
    assert_eq!(skills[0]["folder_id"], "skf-pdo");
    assert_eq!(skills[0]["locked"], true);

    // The detail answers like any skill: content, frontmatter with its
    // `skill_version`, the body the harness will read.
    let detail = get_json(&daemon, &format!("/settings/skills/{SEEDED_ID}")).await;
    assert_eq!(detail["name"], "pdo-orchestrate");
    assert_eq!(detail["locked"], true);
    // #779 bumped the seeded guidance (attachment flags, fixed examples).
    assert_eq!(detail["frontmatter"]["skill_version"], 2);
    assert!(detail["content"]
        .as_str()
        .unwrap()
        .contains("Orchestrating with PDO"));
    // On disk, under the id-keyed folder like any skill.
    let on_disk = daemon
        .repo_root()
        .join(".pdo/skills")
        .join(SEEDED_ID)
        .join("SKILL.md");
    assert!(on_disk.is_file(), "{on_disk:?}");
}

#[tokio::test]
async fn editing_and_deleting_are_refused_with_a_clear_message() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let skill_md_on_disk = daemon
        .repo_root()
        .join(".pdo/skills")
        .join(SEEDED_ID)
        .join("SKILL.md");
    let before = std::fs::read_to_string(&skill_md_on_disk).unwrap();

    // Rename (PUT /settings/skills/{id}).
    let resp = put(
        &daemon,
        &format!("/settings/skills/{SEEDED_ID}"),
        serde_json::json!({ "name": "mine-now" }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["code"], "locked");
    assert!(body["error"].as_str().unwrap().contains("managed by PDO"));

    // Move to a folder — the same PUT.
    let resp = put(
        &daemon,
        &format!("/settings/skills/{SEEDED_ID}"),
        serde_json::json!({ "folder_id": serde_json::Value::Null }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // Overwrite the SKILL.md text (the editor's save).
    let resp = reqwest::Client::new()
        .put(format!(
            "{}/settings/skills/{SEEDED_ID}/files/SKILL.md",
            daemon.url()
        ))
        .body("---\nname: pdo-orchestrate\ndescription: hijacked\n---\n\nmine\n")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["code"], "locked");

    // Delete the skill.
    let resp = reqwest::Client::new()
        .delete(format!("{}/settings/skills/{SEEDED_ID}", daemon.url()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["code"], "locked");

    // Nothing moved: the content on disk is untouched, the row still answers.
    assert_eq!(std::fs::read_to_string(&skill_md_on_disk).unwrap(), before);
    let detail = get_json(&daemon, &format!("/settings/skills/{SEEDED_ID}")).await;
    assert_eq!(detail["name"], "pdo-orchestrate");
}

#[tokio::test]
async fn other_skills_stay_fully_editable_next_to_the_seed() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let valid =
        "---\nname: tdd\ndescription: Test-driven development.\n---\n\n# TDD\n\nRed, green.\n";
    let resp = reqwest::Client::new()
        .post(format!("{}/settings/skills", daemon.url()))
        .json(&serde_json::json!({ "content": valid }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let skill: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(skill["locked"], false);

    let id = skill["id"].as_str().unwrap();
    let resp = reqwest::Client::new()
        .delete(format!("{}/settings/skills/{id}", daemon.url()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn a_wiped_bank_folder_is_reseeded_by_the_next_start() {
    // Two daemons boot in turn over ONE tempdir (`serve_with_config` directly,
    // the same idiom as `skill_bank.rs`'s restart test): the first seeds, the
    // skill's folder is erased on disk, the second re-seeds it at startup.
    use pdo_daemon::{serve_with_config, DaemonConfig};
    use std::net::SocketAddr;

    let tempdir = tempfile::tempdir().unwrap();
    let config = || DaemonConfig {
        tmux_cmd_override: Some("exec sleep 600".to_string()),
        panic_on_trigger_name: None,
        panic_on_stale_sweep: false,
        panic_on_spawn: false,
        service_health_override: None,
        docker_cmd_override: None,
        sandbox_home_override: None,
        price_source_url: None,
        price_refresh_at_boot: false,
        update_source_url: None,
        run_update_check_loop: false,
        update_executor_override: None,
        install_method_override: None,
        supervision_override: None,
        relaunch_command: None,
        allowed_ws_origins: Vec::new(),
        run_trigger_scheduler_loop: false,
        nested_daemon: true,
    };

    let first = serve_with_config(
        SocketAddr::from(([127, 0, 0, 1], 0)),
        tempdir.path().to_path_buf(),
        config(),
    )
    .await
    .unwrap();
    let skill_dir = tempdir.path().join(".pdo/skills").join(SEEDED_ID);
    assert!(skill_dir.join("SKILL.md").is_file());
    first.task.abort();

    // The bank is corrupted on disk: the whole skill folder is gone.
    std::fs::remove_dir_all(&skill_dir).unwrap();

    let second = serve_with_config(
        SocketAddr::from(([127, 0, 0, 1], 0)),
        tempdir.path().to_path_buf(),
        config(),
    )
    .await
    .unwrap();
    let url = format!("http://{}", second.addr);
    let detail: serde_json::Value = reqwest::get(format!("{url}/settings/skills/{SEEDED_ID}"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(detail["name"], "pdo-orchestrate");
    assert!(detail["content"]
        .as_str()
        .unwrap()
        .contains("Orchestrating with PDO"));
    assert!(skill_dir.join("SKILL.md").is_file());
    second.task.abort();
}
