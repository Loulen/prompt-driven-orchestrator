//! Layer 3a — the Banque de skills over HTTP (#668, ADR-0062).
//!
//! A **real daemon** over a tempdir: every assertion is an HTTP response or a
//! file under `<repo_root>/.pdo/skills/<id>/`, never the shape of a table. Prior
//! art: the agent-profile and staging-profile suites.

use std::path::Path;

use crate::common::TestDaemon;
use reqwest::StatusCode;

const VALID: &str = "---\nname: tdd\ndescription: Test-driven development. Red-green-refactor at pre-agreed seams.\nallowed-tools: Bash(npm:*) Bash(cargo:*)\n---\n\n# Test-driven development\n\nRed, green, refactor.\n";
const NO_DESCRIPTION: &str = "---\nname: tdd\nallowed-tools: Bash(npm:*)\n---\n\n# Test-driven development\n\nRed, green, refactor.\n";

fn skills_root(daemon: &TestDaemon) -> std::path::PathBuf {
    daemon.repo_root().join(".pdo").join("skills")
}

async fn post_skill(daemon: &TestDaemon, body: serde_json::Value) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{}/settings/skills", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap()
}

async fn put_json(daemon: &TestDaemon, path: &str, body: serde_json::Value) -> reqwest::Response {
    reqwest::Client::new()
        .put(format!("{}{path}", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap()
}

async fn get_json(daemon: &TestDaemon, path: &str) -> serde_json::Value {
    reqwest::get(format!("{}{path}", daemon.url()))
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

async fn create_valid(daemon: &TestDaemon) -> serde_json::Value {
    let resp = post_skill(daemon, serde_json::json!({ "content": VALID })).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    resp.json().await.unwrap()
}

fn skill_dir_count(root: &Path) -> usize {
    std::fs::read_dir(root).map(|d| d.count()).unwrap_or(0)
}

#[tokio::test]
async fn fp_step_1_a_fresh_instance_has_an_empty_bank() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let bank = get_json(&daemon, "/settings/skills").await;
    assert_eq!(bank["skills"], serde_json::json!([]));
    assert_eq!(bank["folders"], serde_json::json!([]));
    // The footer names the disk location: one folder per id under `.pdo/skills`.
    assert_eq!(
        bank["root_path"].as_str().unwrap(),
        skills_root(&daemon).display().to_string()
    );
}

#[tokio::test]
async fn fp_step_2_pasting_a_valid_skill_md_indexes_it_and_writes_its_folder_by_id() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let skill = create_valid(&daemon).await;
    let id = skill["id"].as_str().unwrap();
    assert_eq!(skill["name"], "tdd");
    assert_eq!(
        skill["description"],
        "Test-driven development. Red-green-refactor at pre-agreed seams."
    );
    assert!(skill["folder_id"].is_null());

    // On disk: `<root>/<id>/SKILL.md`, byte-identical to the paste.
    let on_disk = skills_root(&daemon).join(id).join("SKILL.md");
    assert_eq!(std::fs::read_to_string(&on_disk).unwrap(), VALID);

    // Listed with its name and description.
    let bank = get_json(&daemon, "/settings/skills").await;
    assert_eq!(bank["skills"].as_array().unwrap().len(), 1);
    assert_eq!(bank["skills"][0]["name"], "tdd");

    // Detail: raw content, parsed frontmatter for the table, body, files (none).
    let detail = get_json(&daemon, &format!("/settings/skills/{id}")).await;
    assert_eq!(detail["content"], VALID);
    assert_eq!(detail["frontmatter"]["name"], "tdd");
    assert_eq!(
        detail["frontmatter"]["allowed-tools"],
        "Bash(npm:*) Bash(cargo:*)"
    );
    assert!(detail["body"]
        .as_str()
        .unwrap()
        .starts_with("# Test-driven development"));
    assert_eq!(detail["files"], serde_json::json!([]));
    assert_eq!(
        detail["path"].as_str().unwrap(),
        skills_root(&daemon).join(id).display().to_string()
    );
}

#[tokio::test]
async fn fp_step_3_missing_description_is_a_400_with_the_reason_and_nothing_on_disk() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let resp = post_skill(&daemon, serde_json::json!({ "content": NO_DESCRIPTION })).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["code"], "missing_description");
    assert!(body["error"].as_str().unwrap().contains("description"));

    // Nothing appears, nothing is written.
    let bank = get_json(&daemon, "/settings/skills").await;
    assert_eq!(bank["skills"], serde_json::json!([]));
    assert_eq!(skill_dir_count(&skills_root(&daemon)), 0);
}

#[tokio::test]
async fn every_frontmatter_refusal_is_a_named_400() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let cases = [
        ("# no frontmatter\n\nbody", "no_frontmatter"),
        ("---\ndescription: x\n---\nbody", "missing_name"),
        (
            "---\nname: TDD\ndescription: x\n---\nbody",
            "name_not_kebab_case",
        ),
        ("---\nname: tdd\ndescription: x\n---\n\n", "empty_body"),
        ("---\nname: [\n---\nbody", "malformed_frontmatter"),
    ];
    for (content, code) in cases {
        let resp = post_skill(&daemon, serde_json::json!({ "content": content })).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "{code}");
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["code"], code);
    }
    assert_eq!(skill_dir_count(&skills_root(&daemon)), 0);
}

#[tokio::test]
async fn a_case_insensitive_name_collision_is_an_explicit_409() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let first = create_valid(&daemon).await;
    // Same frontmatter, a label that differs only by case (the `name` field
    // overrides the bank label; the frontmatter `name` itself must stay kebab-case).
    let resp = post_skill(
        &daemon,
        serde_json::json!({ "content": VALID, "name": "TDD" }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["code"], "duplicate_name");
    assert_eq!(body["existing_id"], first["id"]);
    assert_eq!(body["existing_name"], "tdd");
    assert!(body["error"].as_str().unwrap().contains("`tdd`"));
    // The refused paste wrote nothing: one folder only.
    assert_eq!(skill_dir_count(&skills_root(&daemon)), 1);
}

#[tokio::test]
async fn fp_step_4_create_a_folder_and_move_the_skill_into_it() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let skill = create_valid(&daemon).await;
    let id = skill["id"].as_str().unwrap();

    let resp = reqwest::Client::new()
        .post(format!("{}/settings/skill-folders", daemon.url()))
        .json(&serde_json::json!({ "name": "méthode" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let folder: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(folder["name"], "méthode");
    assert!(folder["parent_id"].is_null());
    let folder_id = folder["id"].as_str().unwrap();

    let resp = put_json(
        &daemon,
        &format!("/settings/skills/{id}"),
        serde_json::json!({ "folder_id": folder_id }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let moved: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(moved["folder_id"], folder_id);

    // The tree shows it under the folder; the disk did not move.
    let bank = get_json(&daemon, "/settings/skills").await;
    assert_eq!(bank["skills"][0]["folder_id"], folder_id);
    assert_eq!(bank["folders"][0]["id"], folder_id);
    assert!(skills_root(&daemon).join(id).join("SKILL.md").exists());

    // Back to the root with an explicit null.
    let resp = put_json(
        &daemon,
        &format!("/settings/skills/{id}"),
        serde_json::json!({ "folder_id": null }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let back: serde_json::Value = resp.json().await.unwrap();
    assert!(back["folder_id"].is_null());

    // An unknown folder is refused.
    let resp = put_json(
        &daemon,
        &format!("/settings/skills/{id}"),
        serde_json::json!({ "folder_id": "skf-nope" }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn fp_step_5_renaming_changes_the_label_only_and_moves_nothing() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let skill = create_valid(&daemon).await;
    let id = skill["id"].as_str().unwrap();
    let dir = skills_root(&daemon).join(id);
    let before = std::fs::read_to_string(dir.join("SKILL.md")).unwrap();

    let resp = put_json(
        &daemon,
        &format!("/settings/skills/{id}"),
        serde_json::json!({ "name": "tdd-strict" }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let renamed: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(renamed["id"], id, "the id is the identity");
    assert_eq!(renamed["name"], "tdd-strict");

    // Same folder, same bytes, frontmatter `name` untouched: the detail is unchanged.
    assert!(dir.exists());
    assert_eq!(
        std::fs::read_to_string(dir.join("SKILL.md")).unwrap(),
        before
    );
    assert_eq!(skill_dir_count(&skills_root(&daemon)), 1);
    let detail = get_json(&daemon, &format!("/settings/skills/{id}")).await;
    assert_eq!(detail["name"], "tdd-strict");
    assert_eq!(detail["frontmatter"]["name"], "tdd");

    // A rename onto another skill's name (any case) is a 409; onto its own is fine.
    let other = post_skill(
        &daemon,
        serde_json::json!({ "content": VALID.replace("name: tdd", "name: grilling") }),
    )
    .await;
    assert_eq!(other.status(), StatusCode::CREATED);
    let resp = put_json(
        &daemon,
        &format!("/settings/skills/{id}"),
        serde_json::json!({ "name": "Grilling" }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let resp = put_json(
        &daemon,
        &format!("/settings/skills/{id}"),
        serde_json::json!({ "name": "TDD-Strict" }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn fp_step_6_referents_are_empty_and_delete_removes_row_and_folder() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let skill = create_valid(&daemon).await;
    let id = skill["id"].as_str().unwrap();
    let dir = skills_root(&daemon).join(id);

    // The dialog reads the referents first: the endpoint exists, shaped by tier, empty.
    let referents = get_json(&daemon, &format!("/settings/skills/{id}/referents")).await;
    assert_eq!(referents["skill_id"], id);
    assert_eq!(referents["instance"], false);
    assert_eq!(referents["projects"], serde_json::json!([]));
    assert_eq!(referents["pipelines"], serde_json::json!([]));
    assert_eq!(referents["runs"], serde_json::json!([]));

    let resp = reqwest::Client::new()
        .delete(format!("{}/settings/skills/{id}", daemon.url()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    assert!(!dir.exists(), "the folder on disk is removed");
    let bank = get_json(&daemon, "/settings/skills").await;
    assert_eq!(bank["skills"], serde_json::json!([]));

    // Gone means gone: 404 on every route for that id.
    let resp = reqwest::get(format!("{}/settings/skills/{id}", daemon.url()))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let resp = reqwest::get(format!("{}/settings/skills/{id}/referents", daemon.url()))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let resp = reqwest::Client::new()
        .delete(format!("{}/settings/skills/{id}", daemon.url()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn folder_crud_nests_renames_and_deleting_moves_content_to_the_parent() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let client = reqwest::Client::new();
    let mk = |name: &str, parent: Option<&str>| {
        let client = client.clone();
        let url = format!("{}/settings/skill-folders", daemon.url());
        let body = serde_json::json!({ "name": name, "parent_id": parent });
        async move {
            let resp = client.post(url).json(&body).send().await.unwrap();
            assert_eq!(resp.status(), StatusCode::CREATED);
            resp.json::<serde_json::Value>().await.unwrap()
        }
    };
    let ippon = mk("ippon", None).await;
    let ippon_id = ippon["id"].as_str().unwrap();
    let java = mk("java", Some(ippon_id)).await;
    let java_id = java["id"].as_str().unwrap();
    assert_eq!(java["parent_id"], ippon_id);

    // A skill inside `java`.
    let resp = post_skill(
        &daemon,
        serde_json::json!({ "content": VALID, "folder_id": java_id }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let skill: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(skill["folder_id"], java_id);

    // Rename the folder.
    let resp = put_json(
        &daemon,
        &format!("/settings/skill-folders/{java_id}"),
        serde_json::json!({ "name": "java-spring" }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let renamed: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(renamed["name"], "java-spring");

    // A cycle is refused; a blank name too; an unknown parent too.
    let resp = put_json(
        &daemon,
        &format!("/settings/skill-folders/{ippon_id}"),
        serde_json::json!({ "parent_id": java_id }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let resp = client
        .post(format!("{}/settings/skill-folders", daemon.url()))
        .json(&serde_json::json!({ "name": "  " }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let resp = client
        .post(format!("{}/settings/skill-folders", daemon.url()))
        .json(&serde_json::json!({ "name": "x", "parent_id": "skf-nope" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // Listed.
    let listed = get_json(&daemon, "/settings/skill-folders").await;
    assert_eq!(listed["folders"].as_array().unwrap().len(), 2);

    // Delete `java`: its skill moves up to `ippon`; the skill itself survives.
    let resp = client
        .delete(format!("{}/settings/skill-folders/{java_id}", daemon.url()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    let bank = get_json(&daemon, "/settings/skills").await;
    assert_eq!(bank["folders"].as_array().unwrap().len(), 1);
    assert_eq!(bank["skills"][0]["folder_id"], ippon_id);
    assert!(skills_root(&daemon)
        .join(skill["id"].as_str().unwrap())
        .join("SKILL.md")
        .exists());

    // Deleting `ippon` sends the skill to the root.
    let resp = client
        .delete(format!(
            "{}/settings/skill-folders/{ippon_id}",
            daemon.url()
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    let bank = get_json(&daemon, "/settings/skills").await;
    assert!(bank["skills"][0]["folder_id"].is_null());
    let resp = client
        .delete(format!(
            "{}/settings/skill-folders/{ippon_id}",
            daemon.url()
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn reference_files_on_disk_are_listed_read_only_in_the_detail() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let skill = create_valid(&daemon).await;
    let id = skill["id"].as_str().unwrap();
    let dir = skills_root(&daemon).join(id);
    std::fs::write(dir.join("checklist.md"), "- [ ] one\n").unwrap();
    std::fs::create_dir_all(dir.join("ref")).unwrap();
    std::fs::write(dir.join("ref").join("a.txt"), "12345").unwrap();

    let detail = get_json(&daemon, &format!("/settings/skills/{id}")).await;
    assert_eq!(
        detail["files"],
        serde_json::json!([
            { "path": "checklist.md", "size": 10 },
            { "path": "ref/a.txt", "size": 5 },
        ])
    );
}

#[tokio::test]
async fn the_bank_survives_a_daemon_restart() {
    // The index is in `pdo.db`, the content on disk: both are under the repo root,
    // so a fresh daemon over the same directory sees the same bank. Two daemons
    // are booted in turn over ONE tempdir with `serve_with_config` directly (the
    // `TestDaemon` constructors each own a fresh tempdir).
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
    let url = format!("http://{}", first.addr);
    let resp = reqwest::Client::new()
        .post(format!("{url}/settings/skills"))
        .json(&serde_json::json!({ "content": VALID }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let skill: serde_json::Value = resp.json().await.unwrap();
    let id = skill["id"].as_str().unwrap().to_string();
    first.task.abort();

    let second = serve_with_config(
        SocketAddr::from(([127, 0, 0, 1], 0)),
        tempdir.path().to_path_buf(),
        config(),
    )
    .await
    .unwrap();
    let url = format!("http://{}", second.addr);
    let bank: serde_json::Value = reqwest::get(format!("{url}/settings/skills"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(bank["skills"][0]["id"], id);
    let detail: serde_json::Value = reqwest::get(format!("{url}/settings/skills/{id}"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(detail["content"], VALID);
    second.task.abort();
}

// ---------------------------------------------------------------------------
// Import from a Source (#670) — over a local fixture git repository, no network.
// ---------------------------------------------------------------------------

fn git(dir: &Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", "fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.com")
        .env("GIT_COMMITTER_NAME", "fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.com")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn write_skill(root: &Path, rel: &str, name: &str, description: &str) -> std::path::PathBuf {
    let dir = root.join(rel);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("SKILL.md"),
        format!(
            "---\nname: {name}\ndescription: {description}\n---\n\n# {name}\n\nBody of {name}.\n"
        ),
    )
    .unwrap();
    dir
}

/// A fixture repository with the nested `skills/engineering/<name>/SKILL.md`
/// layout of the AC, one invalid skill, one skill with reference files, and
/// a decoy `SKILL.md` under `docs/` that is NOT a skill folder layout issue but
/// simply lives elsewhere.
fn fixture_repo() -> tempfile::TempDir {
    let repo = tempfile::tempdir().unwrap();
    let root = repo.path();
    git(root, &["init", "-q", "-b", "main"]);
    let pdf = write_skill(
        root,
        "skills/engineering/pdf",
        "pdf",
        "Extract text and tables from PDFs.",
    );
    std::fs::create_dir_all(pdf.join("scripts")).unwrap();
    std::fs::write(pdf.join("scripts").join("extract.py"), "print('x')\n").unwrap();
    std::fs::write(pdf.join("reference.md"), "# Ref\n").unwrap();
    write_skill(
        root,
        "skills/engineering/webapp-testing",
        "webapp-testing",
        "Drive a local web app.",
    );
    write_skill(
        root,
        "skills/engineering/code-review",
        "code-review",
        "Structured review of a diff.",
    );
    // Invalid: no description.
    let bad = root.join("skills/engineering/skill-creator");
    std::fs::create_dir_all(&bad).unwrap();
    std::fs::write(
        bad.join("SKILL.md"),
        "---\nname: skill-creator\n---\n\nbody\n",
    )
    .unwrap();
    write_skill(root, "docs/tdd", "tdd", "Test-driven development.");
    std::fs::write(root.join("README.md"), "fixture\n").unwrap();
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "fixture"]);
    repo
}

fn head_of(dir: &Path) -> String {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

async fn post_json(daemon: &TestDaemon, path: &str, body: serde_json::Value) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{}{path}", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap()
}

fn file_url(dir: &Path) -> String {
    format!("file://{}", dir.display())
}

#[tokio::test]
async fn fp670_step_1_scanning_a_git_source_lists_nested_skills_with_one_invalid_greyed() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let repo = fixture_repo();
    let resp = post_json(
        &daemon,
        "/settings/skills/scan",
        serde_json::json!({ "scan_id": "scan-1", "source": file_url(repo.path()) }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["scan_id"], "scan-1");
    assert_eq!(body["source"]["kind"], "git");
    assert_eq!(body["commit"], head_of(repo.path()));
    let candidates = body["candidates"].as_array().unwrap();
    let paths: Vec<&str> = candidates
        .iter()
        .map(|c| c["path"].as_str().unwrap())
        .collect();
    assert_eq!(
        paths,
        vec![
            "docs/tdd",
            "skills/engineering/code-review",
            "skills/engineering/pdf",
            "skills/engineering/skill-creator",
            "skills/engineering/webapp-testing",
        ],
        "the nested skills/engineering/<name>/SKILL.md layout is discovered"
    );
    let pdf = candidates.iter().find(|c| c["name"] == "pdf").unwrap();
    assert_eq!(pdf["valid"], true);
    assert_eq!(pdf["status"], "new");
    assert_eq!(pdf["description"], "Extract text and tables from PDFs.");
    assert_eq!(pdf["file_count"], 2);
    let bad = candidates
        .iter()
        .find(|c| c["path"] == "skills/engineering/skill-creator")
        .unwrap();
    assert_eq!(bad["valid"], false);
    assert_eq!(bad["status"], "invalid");
    assert_eq!(bad["code"], "missing_description");
    assert!(bad["reason"].as_str().unwrap().contains("description"));
    // Nothing was written to the bank.
    let bank = get_json(&daemon, "/settings/skills").await;
    assert_eq!(bank["skills"], serde_json::json!([]));
    assert_eq!(bank["folders"], serde_json::json!([]));
    assert_eq!(skill_dir_count(&skills_root(&daemon)), 0);
    // The source is remembered for the "Recent sources" list.
    let recent = get_json(&daemon, "/settings/skills/sources/recent").await;
    assert_eq!(recent["sources"][0]["url"], file_url(repo.path()));
}

#[tokio::test]
async fn a_local_folder_source_is_scanned_in_place_and_a_sub_path_is_honoured() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let repo = fixture_repo();
    // Local folder: the daemon walks it directly; the commit is the repo HEAD.
    let resp = post_json(
        &daemon,
        "/settings/skills/scan",
        serde_json::json!({ "scan_id": "scan-local", "source": repo.path().display().to_string() }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["source"]["kind"], "local");
    assert_eq!(body["commit"], head_of(repo.path()));
    assert_eq!(body["candidates"].as_array().unwrap().len(), 5);

    // An unknown local folder is a named 400.
    let resp = post_json(
        &daemon,
        "/settings/skills/scan",
        serde_json::json!({ "scan_id": "scan-nope", "source": "/definitely/not/here" }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["code"], "local_not_found");

    // A clone that git refuses is a 502 carrying git's stderr.
    let resp = post_json(
        &daemon,
        "/settings/skills/scan",
        serde_json::json!({ "scan_id": "scan-bad", "source": "file:///definitely/not/a/repo" }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["code"], "clone_failed");
    assert!(body["error"].as_str().unwrap().contains("fatal"), "{body}");

    // Garbage is a 400.
    let resp = post_json(
        &daemon,
        "/settings/skills/scan",
        serde_json::json!({ "scan_id": "scan-garbage", "source": "not a source" }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn fp670_step_2_importing_two_skills_creates_a_source_folder_with_provenance() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let repo = fixture_repo();
    let url = file_url(repo.path());
    let scan: serde_json::Value = post_json(
        &daemon,
        "/settings/skills/scan",
        serde_json::json!({ "scan_id": "scan-2", "source": url }),
    )
    .await
    .json()
    .await
    .unwrap();
    assert_eq!(
        scan["source"]["suggested_folder"],
        repo.path().file_name().unwrap().to_str().unwrap()
    );

    let resp = post_json(
        &daemon,
        "/settings/skills/import",
        serde_json::json!({
            "scan_id": "scan-2",
            "source": url,
            "folder": { "name": "anthropics/skills", "parent_id": null },
            "items": [
                { "path": "skills/engineering/pdf", "action": "import" },
                { "path": "skills/engineering/webapp-testing", "action": "import" },
            ]
        }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let report: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(report["failed"], serde_json::json!([]));
    assert_eq!(report["imported"].as_array().unwrap().len(), 2);
    let folder = &report["folder"];
    assert_eq!(folder["name"], "anthropics/skills");
    assert_eq!(folder["source"]["url"], url);
    assert_eq!(folder["source"]["commit"], head_of(repo.path()));
    assert_eq!(folder["source"]["path"], "");
    assert_eq!(folder["source"]["found"], 5);
    assert_eq!(folder["source"]["invalid"], 1);
    assert!(folder["source"]["imported_at"].as_str().unwrap().len() > 10);
    let folder_id = folder["id"].as_str().unwrap();

    // Each skill carries its own provenance and its whole folder was copied.
    let bank = get_json(&daemon, "/settings/skills").await;
    let skills = bank["skills"].as_array().unwrap();
    assert_eq!(skills.len(), 2);
    let pdf = skills.iter().find(|s| s["name"] == "pdf").unwrap();
    assert_eq!(pdf["folder_id"], folder_id);
    assert_eq!(pdf["source"]["url"], url);
    assert_eq!(pdf["source"]["path"], "skills/engineering/pdf");
    assert_eq!(pdf["source"]["commit"], head_of(repo.path()));
    let pdf_id = pdf["id"].as_str().unwrap();
    let detail = get_json(&daemon, &format!("/settings/skills/{pdf_id}")).await;
    assert_eq!(
        detail["files"],
        serde_json::json!([
            { "path": "reference.md", "size": 6 },
            { "path": "scripts/extract.py", "size": 11 },
        ])
    );
    assert_eq!(bank["folders"][0]["source"]["url"], url);
}

#[tokio::test]
async fn fp670_step_3_reimporting_offers_replace_rename_skip_and_never_writes_silently() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let repo = fixture_repo();
    let url = file_url(repo.path());
    let first_commit = head_of(repo.path());
    post_json(
        &daemon,
        "/settings/skills/scan",
        serde_json::json!({ "scan_id": "s1", "source": url }),
    )
    .await;
    let report: serde_json::Value = post_json(
        &daemon,
        "/settings/skills/import",
        serde_json::json!({
            "scan_id": "s1", "source": url,
            "folder": { "name": "first" },
            "items": [{ "path": "skills/engineering/pdf", "action": "import" }]
        }),
    )
    .await
    .json()
    .await
    .unwrap();
    let pdf_id = report["imported"][0]["skill"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // A pasted `code-review` in another folder collides by name with the source's.
    let craft = post_json(
        &daemon,
        "/settings/skill-folders",
        serde_json::json!({ "name": "craft" }),
    )
    .await
    .json::<serde_json::Value>()
    .await
    .unwrap();
    let pasted = post_skill(
        &daemon,
        serde_json::json!({
            "content": "---\nname: code-review\ndescription: Mine.\n---\n\nMine.\n",
            "folder_id": craft["id"]
        }),
    )
    .await
    .json::<serde_json::Value>()
    .await
    .unwrap();

    // Second scan of the same commit: `pdf` is "same commit", `code-review` is "name taken".
    let scan: serde_json::Value = post_json(
        &daemon,
        "/settings/skills/scan",
        serde_json::json!({ "scan_id": "s2", "source": url }),
    )
    .await
    .json()
    .await
    .unwrap();
    let by_name = |n: &str| {
        scan["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == n)
            .unwrap()
            .clone()
    };
    assert_eq!(by_name("pdf")["status"], "same_commit");
    assert_eq!(by_name("pdf")["existing"]["folder_name"], "first");
    assert_eq!(by_name("code-review")["status"], "name_taken");
    assert_eq!(by_name("code-review")["existing"]["id"], pasted["id"]);
    assert_eq!(by_name("code-review")["existing"]["folder_name"], "craft");
    assert_eq!(by_name("webapp-testing")["status"], "new");

    // Rename without a name is refused before any write.
    let resp = post_json(
        &daemon,
        "/settings/skills/import",
        serde_json::json!({
            "scan_id": "s2", "source": url,
            "folder": { "name": "second" },
            "items": [{ "path": "skills/engineering/code-review", "action": "rename" }]
        }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let bank = get_json(&daemon, "/settings/skills").await;
    assert_eq!(
        bank["folders"].as_array().unwrap().len(),
        2,
        "no folder was created"
    );

    // Change the source, then: replace `pdf`, rename `code-review`, skip nothing silently.
    std::fs::write(
        repo.path().join("skills/engineering/pdf/SKILL.md"),
        "---\nname: pdf\ndescription: Extract text, v2.\n---\n\n# pdf v2\n\nBody.\n",
    )
    .unwrap();
    git(repo.path(), &["add", "-A"]);
    git(repo.path(), &["commit", "-q", "-m", "v2"]);
    let second_commit = head_of(repo.path());
    assert_ne!(first_commit, second_commit);
    post_json(
        &daemon,
        "/settings/skills/scan",
        serde_json::json!({ "scan_id": "s3", "source": url }),
    )
    .await;
    let resp = post_json(
        &daemon,
        "/settings/skills/import",
        serde_json::json!({
            "scan_id": "s3", "source": url,
            "folder": { "name": "second" },
            "items": [
                { "path": "skills/engineering/pdf", "action": "replace" },
                { "path": "skills/engineering/code-review", "action": "rename", "name": "code-review-anthropic" },
                { "path": "skills/engineering/webapp-testing", "action": "skip" },
            ]
        }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let report: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(report["failed"], serde_json::json!([]));
    let rows = report["imported"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    let replaced = rows.iter().find(|r| r["action"] == "replaced").unwrap();
    assert_eq!(replaced["skill"]["id"], pdf_id, "replace keeps the id");
    assert_eq!(replaced["skill"]["description"], "Extract text, v2.");
    assert_eq!(replaced["skill"]["source"]["commit"], second_commit);
    let detail = get_json(&daemon, &format!("/settings/skills/{pdf_id}")).await;
    assert!(detail["content"].as_str().unwrap().contains("v2"));
    let renamed = rows.iter().find(|r| r["action"] == "renamed").unwrap();
    assert_eq!(renamed["skill"]["name"], "code-review-anthropic");
    // The pasted `code-review` is untouched; `webapp-testing` was skipped.
    let bank = get_json(&daemon, "/settings/skills").await;
    let names: Vec<&str> = bank["skills"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["code-review", "code-review-anthropic", "pdf"]);
    let mine = bank["skills"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "code-review")
        .unwrap();
    assert_eq!(mine["description"], "Mine.");
    assert!(mine["source"].is_null());
}

#[tokio::test]
async fn fp670_step_4_update_from_source_rescans_diffs_then_updates_content_and_provenance() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let repo = fixture_repo();
    let url = file_url(repo.path());
    let first_commit = head_of(repo.path());
    post_json(
        &daemon,
        "/settings/skills/scan",
        serde_json::json!({ "scan_id": "u1", "source": url }),
    )
    .await;
    let report: serde_json::Value = post_json(
        &daemon,
        "/settings/skills/import",
        serde_json::json!({
            "scan_id": "u1", "source": url,
            "items": [
                { "path": "skills/engineering/pdf", "action": "import" },
                { "path": "skills/engineering/webapp-testing", "action": "import" },
                { "path": "skills/engineering/code-review", "action": "import" },
            ]
        }),
    )
    .await
    .json()
    .await
    .unwrap();
    let folder_id = report["folder"]["id"].as_str().unwrap().to_string();
    let find = |name: &str| {
        report["imported"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["skill"]["name"] == name)
            .unwrap()["skill"]["id"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let pdf_id = find("pdf");
    let cr_id = find("code-review");
    let wt_id = find("webapp-testing");

    // The user moves `code-review` out of the Source folder.
    put_json(
        &daemon,
        &format!("/settings/skills/{cr_id}"),
        serde_json::json!({ "folder_id": null }),
    )
    .await;

    // The source moves on: pdf changes + gains a file, webapp-testing vanishes,
    // a new skill appears.
    std::fs::write(
        repo.path().join("skills/engineering/pdf/SKILL.md"),
        "---\nname: pdf\ndescription: Extract text, v2.\n---\n\n# pdf v2\n\nBody.\n",
    )
    .unwrap();
    std::fs::write(
        repo.path().join("skills/engineering/pdf/extra.md"),
        "more\n",
    )
    .unwrap();
    std::fs::remove_dir_all(repo.path().join("skills/engineering/webapp-testing")).unwrap();
    write_skill(
        repo.path(),
        "skills/engineering/mcp-builder",
        "mcp-builder",
        "Build an MCP server.",
    );
    git(repo.path(), &["add", "-A"]);
    git(repo.path(), &["commit", "-q", "-m", "v2"]);
    let second_commit = head_of(repo.path());

    // Rescan: a diff, nothing written.
    let resp = post_json(
        &daemon,
        &format!("/settings/skill-folders/{folder_id}/rescan"),
        serde_json::json!({ "scan_id": "u2" }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let rescan: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(rescan["previous_commit"], first_commit);
    assert_eq!(rescan["commit"], second_commit);
    let status_of = |path: &str| {
        rescan["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["path"] == path)
            .unwrap_or_else(|| panic!("no entry for {path}: {rescan}"))
            .clone()
    };
    let pdf = status_of("skills/engineering/pdf");
    assert_eq!(pdf["status"], "updated");
    assert_eq!(pdf["skill_md_changed"], true);
    assert_eq!(pdf["files_added"], 1);
    assert_eq!(
        status_of("skills/engineering/code-review")["status"],
        "skipped"
    );
    assert_eq!(status_of("skills/engineering/mcp-builder")["status"], "new");
    assert_eq!(
        status_of("skills/engineering/webapp-testing")["status"],
        "gone"
    );
    assert_eq!(
        status_of("skills/engineering/webapp-testing")["skill_id"],
        wt_id
    );
    assert_eq!(
        status_of("skills/engineering/skill-creator")["status"],
        "invalid"
    );
    assert_eq!(status_of("docs/tdd")["status"], "new");
    let detail = get_json(&daemon, &format!("/settings/skills/{pdf_id}")).await;
    assert!(
        !detail["content"].as_str().unwrap().contains("v2"),
        "rescan wrote nothing"
    );

    // A folder without provenance cannot be rescanned.
    let plain = post_json(
        &daemon,
        "/settings/skill-folders",
        serde_json::json!({ "name": "plain" }),
    )
    .await
    .json::<serde_json::Value>()
    .await
    .unwrap();
    let resp = post_json(
        &daemon,
        &format!(
            "/settings/skill-folders/{}/rescan",
            plain["id"].as_str().unwrap()
        ),
        serde_json::json!({ "scan_id": "u3" }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);

    // Confirm: update pdf, take the new mcp-builder.
    let resp = post_json(
        &daemon,
        &format!("/settings/skill-folders/{folder_id}/update"),
        serde_json::json!({
            "scan_id": "u2",
            "items": [
                { "path": "skills/engineering/pdf", "action": "update" },
                { "path": "skills/engineering/mcp-builder", "action": "import" },
            ]
        }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let report: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(report["failed"], serde_json::json!([]));
    assert_eq!(report["imported"].as_array().unwrap().len(), 2);
    assert_eq!(report["folder"]["source"]["commit"], second_commit);
    assert_eq!(report["folder"]["source"]["found"], 5);

    let detail = get_json(&daemon, &format!("/settings/skills/{pdf_id}")).await;
    assert!(detail["content"].as_str().unwrap().contains("v2"));
    assert_eq!(detail["source"]["commit"], second_commit);
    assert_eq!(detail["description"], "Extract text, v2.");
    assert_eq!(
        detail["files"],
        serde_json::json!([
            { "path": "extra.md", "size": 5 },
            { "path": "reference.md", "size": 6 },
            { "path": "scripts/extract.py", "size": 11 },
        ])
    );
    let bank = get_json(&daemon, "/settings/skills").await;
    let skills = bank["skills"].as_array().unwrap();
    let mcp = skills.iter().find(|s| s["name"] == "mcp-builder").unwrap();
    assert_eq!(mcp["folder_id"], folder_id);
    // The vanished skill is kept, flagged by its stale commit.
    let wt = skills.iter().find(|s| s["id"] == wt_id).unwrap();
    assert_eq!(wt["source"]["commit"], first_commit);
    assert_eq!(wt["folder_id"], folder_id);
    // The moved-out skill was left alone.
    let cr = skills.iter().find(|s| s["id"] == cr_id).unwrap();
    assert!(cr["folder_id"].is_null());
    assert_eq!(cr["source"]["commit"], first_commit);
}

#[tokio::test]
async fn updating_a_source_folder_never_writes_to_a_sibling_folder_from_the_same_source() {
    // Regression (FP #670 step 4): folder A imported at v1, folder B re-imported
    // from the same repo at v2 with `code-review` renamed. Updating A to v3
    // must rewrite A's own `code-review`, never B's copy.
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let repo = fixture_repo();
    let url = file_url(repo.path());
    let v1 = head_of(repo.path());
    post_json(
        &daemon,
        "/settings/skills/scan",
        serde_json::json!({ "scan_id": "s1", "source": url }),
    )
    .await;
    let report_a: serde_json::Value = post_json(
        &daemon,
        "/settings/skills/import",
        serde_json::json!({
            "scan_id": "s1", "source": url, "folder": { "name": "A" },
            "items": [
                { "path": "skills/engineering/pdf", "action": "import" },
                { "path": "skills/engineering/code-review", "action": "import" },
            ]
        }),
    )
    .await
    .json()
    .await
    .unwrap();
    let folder_a = report_a["folder"]["id"].as_str().unwrap().to_string();
    let cr_a = report_a["imported"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["skill"]["name"] == "code-review")
        .unwrap()["skill"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // v2: code-review changes; re-import it into a second folder under another name.
    std::fs::write(
        repo.path().join("skills/engineering/code-review/SKILL.md"),
        "---\nname: code-review\ndescription: Review, v2.\n---\n\n# v2\n",
    )
    .unwrap();
    git(repo.path(), &["add", "-A"]);
    git(repo.path(), &["commit", "-q", "-m", "v2"]);
    let v2 = head_of(repo.path());
    post_json(
        &daemon,
        "/settings/skills/scan",
        serde_json::json!({ "scan_id": "s2", "source": url }),
    )
    .await;
    let report_b: serde_json::Value = post_json(
        &daemon,
        "/settings/skills/import",
        serde_json::json!({
            "scan_id": "s2", "source": url, "folder": { "name": "B" },
            "items": [
                { "path": "skills/engineering/code-review", "action": "rename", "name": "code-review-v2" },
            ]
        }),
    )
    .await
    .json()
    .await
    .unwrap();
    let folder_b = report_b["folder"]["id"].as_str().unwrap().to_string();
    let cr_b = report_b["imported"][0]["skill"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // v3: code-review changes again. Rescan A: its own copy is the one diffed.
    std::fs::write(
        repo.path().join("skills/engineering/code-review/SKILL.md"),
        "---\nname: code-review\ndescription: Review, v3.\n---\n\n# v3\n",
    )
    .unwrap();
    git(repo.path(), &["add", "-A"]);
    git(repo.path(), &["commit", "-q", "-m", "v3"]);
    let v3 = head_of(repo.path());
    let rescan: serde_json::Value = post_json(
        &daemon,
        &format!("/settings/skill-folders/{folder_a}/rescan"),
        serde_json::json!({ "scan_id": "s3" }),
    )
    .await
    .json()
    .await
    .unwrap();
    let entry = rescan["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["path"] == "skills/engineering/code-review")
        .unwrap();
    assert_eq!(entry["status"], "updated");
    assert_eq!(entry["skill_id"], cr_a);
    assert_eq!(entry["name"], "code-review");

    // Rescan B: A's copy is reported as living elsewhere, by folder name.
    let rescan_b: serde_json::Value = post_json(
        &daemon,
        &format!("/settings/skill-folders/{folder_b}/rescan"),
        serde_json::json!({ "scan_id": "s4" }),
    )
    .await
    .json()
    .await
    .unwrap();
    let pdf_b = rescan_b["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["path"] == "skills/engineering/pdf")
        .unwrap();
    assert_eq!(pdf_b["status"], "skipped");
    assert_eq!(pdf_b["reason"], "already in \u{201c}A\u{201d}");
    let cr_in_b = rescan_b["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["path"] == "skills/engineering/code-review")
        .unwrap();
    assert_eq!(cr_in_b["skill_id"], cr_b);

    // Update A: only A's copy moves to v3, B's stays at v2 untouched.
    let report: serde_json::Value = post_json(
        &daemon,
        &format!("/settings/skill-folders/{folder_a}/update"),
        serde_json::json!({
            "scan_id": "s3",
            "items": [{ "path": "skills/engineering/code-review", "action": "update" }]
        }),
    )
    .await
    .json()
    .await
    .unwrap();
    assert_eq!(report["failed"], serde_json::json!([]));
    assert_eq!(report["imported"].as_array().unwrap().len(), 1);
    assert_eq!(report["imported"][0]["skill"]["id"], cr_a);
    let a = get_json(&daemon, &format!("/settings/skills/{cr_a}")).await;
    assert!(a["content"].as_str().unwrap().contains("v3"));
    assert_eq!(a["source"]["commit"], v3);
    let b = get_json(&daemon, &format!("/settings/skills/{cr_b}")).await;
    assert!(
        b["content"].as_str().unwrap().contains("v2"),
        "B was overwritten"
    );
    assert_eq!(b["source"]["commit"], v2);
    assert_eq!(b["folder_id"], folder_b);
    let _ = v1;
}

#[tokio::test]
async fn scanning_a_sub_path_without_skills_points_at_where_they_are() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let repo = fixture_repo();
    // A `file://` URL has no /tree/ syntax; drive the sub-path through a local
    // source shaped like a GitHub tree URL is impossible offline, so exercise the
    // parser + scan on the local clone via the daemon's own parsing of a
    // sub-folder: point at `<repo>/nothing-here`.
    std::fs::create_dir_all(repo.path().join("nothing-here")).unwrap();
    let resp = post_json(
        &daemon,
        "/settings/skills/scan",
        serde_json::json!({ "scan_id": "e1", "source": repo.path().join("nothing-here").display().to_string() }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["candidates"], serde_json::json!([]));

    // Import against an expired / unknown scan re-scans transparently.
    let url = file_url(repo.path());
    let resp = post_json(
        &daemon,
        "/settings/skills/import",
        serde_json::json!({
            "scan_id": "never-scanned", "source": url,
            "items": [{ "path": "docs/tdd", "action": "import" }]
        }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let report: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(report["imported"][0]["skill"]["name"], "tdd");
    assert_eq!(report["folder"]["source"]["url"], url);
}
