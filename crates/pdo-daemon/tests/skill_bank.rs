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

/// The seeded skill (#722) shares every bank surface with user skills; these
/// helpers keep the assertions about what the *user* created readable.
fn user_skills(bank: &serde_json::Value) -> Vec<&serde_json::Value> {
    bank["skills"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["id"] != "pdo-orchestrate" && s["id"] != "pdo-interactive")
        .collect()
}

fn user_skill_dirs(root: &Path) -> usize {
    std::fs::read_dir(root)
        .map(|entries| {
            entries
                .filter(|e| {
                    let name = e.as_ref().unwrap().file_name();
                    name != "pdo-orchestrate" && name != "pdo-interactive"
                })
                .count()
        })
        .unwrap_or(0)
}

#[tokio::test]
async fn fp_step_1_a_fresh_instance_has_only_the_seeded_skill() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let bank = get_json(&daemon, "/settings/skills").await;
    // #722 / #588: the seeds are the pre-existing rows — `pdo-orchestrate` and
    // `pdo-interactive` in « PDO », flagged locked; the user-owned bank is empty.
    assert_eq!(user_skills(&bank), Vec::<&serde_json::Value>::new());
    let skills = bank["skills"].as_array().unwrap();
    assert_eq!(skills.len(), 2, "{bank}");
    let mut ids: Vec<&str> = skills.iter().map(|s| s["id"].as_str().unwrap()).collect();
    ids.sort();
    assert_eq!(ids, ["pdo-interactive", "pdo-orchestrate"]);
    assert!(skills.iter().all(|s| s["locked"] == true), "{bank}");
    let folders = bank["folders"].as_array().unwrap();
    assert_eq!(folders.len(), 1, "{bank}");
    assert_eq!(folders[0]["id"], "skf-pdo");
    assert_eq!(folders[0]["name"], "PDO");
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

    // Listed with its name and description (the seeded skill shares the bank).
    let bank = get_json(&daemon, "/settings/skills").await;
    assert_eq!(user_skills(&bank).len(), 1);
    assert_eq!(user_skills(&bank)[0]["name"], "tdd");
    assert_eq!(user_skills(&bank)[0]["locked"], false);

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
    assert_eq!(user_skills(&bank), Vec::<&serde_json::Value>::new());
    assert_eq!(user_skill_dirs(&skills_root(&daemon)), 0);
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
    assert_eq!(user_skill_dirs(&skills_root(&daemon)), 0);
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
    // The refused paste wrote nothing: one user folder only.
    assert_eq!(user_skill_dirs(&skills_root(&daemon)), 1);
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
    let row = user_skills(&bank)
        .into_iter()
        .find(|s| s["id"] == id)
        .unwrap();
    assert_eq!(row["folder_id"], folder_id);
    assert!(bank["folders"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["id"] == folder_id));
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
    assert_eq!(user_skill_dirs(&skills_root(&daemon)), 1);
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
    assert_eq!(user_skills(&bank), Vec::<&serde_json::Value>::new());

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
    assert_eq!(listed["folders"].as_array().unwrap().len(), 3);

    // Delete `java`: its skill moves up to `ippon`; the skill itself survives.
    let resp = client
        .delete(format!("{}/settings/skill-folders/{java_id}", daemon.url()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    let bank = get_json(&daemon, "/settings/skills").await;
    assert_eq!(bank["folders"].as_array().unwrap().len(), 2);
    let row = user_skills(&bank)
        .into_iter()
        .find(|s| s["id"] == skill["id"])
        .unwrap();
    assert_eq!(row["folder_id"], ippon_id);
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
    let row = user_skills(&bank)
        .into_iter()
        .find(|s| s["id"] == skill["id"])
        .unwrap();
    assert!(row["folder_id"].is_null());
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
    assert_eq!(
        bank["skills"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["id"] == id)
            .map(|s| s["id"].clone())
            .unwrap(),
        id
    );
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
// Reference files (#671): upload, read, edit, delete — always inside the folder.
// ---------------------------------------------------------------------------

/// A hand-rolled multipart body: `reqwest` is built without its `multipart`
/// feature here, and the wire format is short enough to spell out.
fn multipart_body(parts: &[(&str, Option<&str>, &[u8])]) -> (String, Vec<u8>) {
    let boundary = "----pdo-test-boundary-671";
    let mut body = Vec::new();
    for (name, filename, data) in parts {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        match filename {
            Some(filename) => body.extend_from_slice(
                format!(
                    "Content-Disposition: form-data; name=\"{name}\"; filename=\"{filename}\"\r\n\
                     Content-Type: application/octet-stream\r\n\r\n"
                )
                .as_bytes(),
            ),
            None => body.extend_from_slice(
                format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
            ),
        }
        body.extend_from_slice(data);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

async fn upload(
    daemon: &TestDaemon,
    id: &str,
    parts: &[(&str, Option<&str>, &[u8])],
) -> reqwest::Response {
    let (content_type, body) = multipart_body(parts);
    reqwest::Client::new()
        .post(format!("{}/settings/skills/{id}/files", daemon.url()))
        .header("content-type", content_type)
        .body(body)
        .send()
        .await
        .unwrap()
}

async fn put_text(daemon: &TestDaemon, path: &str, text: &str) -> reqwest::Response {
    reqwest::Client::new()
        .put(format!("{}{path}", daemon.url()))
        .header("content-type", "text/plain; charset=utf-8")
        .body(text.to_string())
        .send()
        .await
        .unwrap()
}

#[tokio::test]
async fn fp_671_step_1_2_multipart_upload_writes_the_files_and_the_detail_lists_them() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let skill = create_valid(&daemon).await;
    let id = skill["id"].as_str().unwrap();

    let resp = upload(
        &daemon,
        id,
        &[
            ("file", Some("selectors-cheatsheet.md"), b"# Selectors\n"),
            // A `path` part right before a `file` part overrides the filename:
            // this is how a browser keeps `examples/login.spec.ts` (webkitRelativePath).
            ("path", None, b"examples/login.spec.ts"),
            ("file", Some("login.spec.ts"), b"test('login', () => {});\n"),
        ],
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["uploaded"],
        serde_json::json!([
            { "path": "selectors-cheatsheet.md", "size": 12 },
            { "path": "examples/login.spec.ts", "size": 25 },
        ])
    );
    assert_eq!(body["files"].as_array().unwrap().len(), 2);

    // Verify (FP): the disk confirms the tree of the skill.
    let dir = skills_root(&daemon).join(id);
    assert_eq!(
        std::fs::read_to_string(dir.join("selectors-cheatsheet.md")).unwrap(),
        "# Selectors\n"
    );
    assert!(dir.join("examples").join("login.spec.ts").is_file());

    let detail = get_json(&daemon, &format!("/settings/skills/{id}")).await;
    assert_eq!(
        detail["files"],
        serde_json::json!([
            { "path": "examples/login.spec.ts", "size": 25 },
            { "path": "selectors-cheatsheet.md", "size": 12 },
        ])
    );
}

#[tokio::test]
async fn fp_671_step_3_from_path_copies_the_explorer_pick_and_delete_removes_one_file() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let skill = create_valid(&daemon).await;
    let id = skill["id"].as_str().unwrap();
    upload(&daemon, id, &[("file", Some("first.md"), b"1")]).await;

    // The explorer path: the daemon copies from the host.
    let src = tempfile::tempdir().unwrap();
    std::fs::write(src.path().join("notes.md"), "some notes").unwrap();
    let resp = reqwest::Client::new()
        .post(format!("{}/settings/skills/{id}/files", daemon.url()))
        .json(&serde_json::json!({ "from_path": src.path().join("notes.md") }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["uploaded"],
        serde_json::json!([{ "path": "notes.md", "size": 10 }])
    );

    // A folder is refused in place, nothing written.
    let resp = reqwest::Client::new()
        .post(format!("{}/settings/skills/{id}/files", daemon.url()))
        .json(&serde_json::json!({ "from_path": src.path() }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["code"], "source_not_a_file");

    // Delete the first one.
    let resp = reqwest::Client::new()
        .delete(format!(
            "{}/settings/skills/{id}/files/first.md",
            daemon.url()
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    assert!(!skills_root(&daemon).join(id).join("first.md").exists());
    let detail = get_json(&daemon, &format!("/settings/skills/{id}")).await;
    assert_eq!(
        detail["files"],
        serde_json::json!([{ "path": "notes.md", "size": 10 }])
    );

    // Deleting it again is a 404, not a 500.
    let resp = reqwest::Client::new()
        .delete(format!(
            "{}/settings/skills/{id}/files/first.md",
            daemon.url()
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn paths_leaving_the_skill_folder_are_a_400_and_skill_md_is_reserved() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let skill = create_valid(&daemon).await;
    let id = skill["id"].as_str().unwrap();
    let root = skills_root(&daemon);

    for bad in ["../escape.md", "a/../../escape.md", "/etc/escape.md"] {
        let resp = upload(
            &daemon,
            id,
            &[
                ("path", None, bad.as_bytes()),
                ("file", Some("x"), b"pwned"),
            ],
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "{bad}");
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["code"], "invalid_path", "{bad}");
    }
    assert!(!root.join("escape.md").exists());
    assert!(!daemon.repo_root().join(".pdo").join("escape.md").exists());

    // The wildcard route cannot express `..` after normalisation, but a DELETE on
    // an encoded traversal must still be refused, not resolved.
    let resp = reqwest::Client::new()
        .delete(format!(
            "{}/settings/skills/{id}/files/..%2Fother",
            daemon.url()
        ))
        .send()
        .await
        .unwrap();
    assert!(
        resp.status() == StatusCode::BAD_REQUEST || resp.status() == StatusCode::NOT_FOUND,
        "got {}",
        resp.status()
    );

    // SKILL.md is not a reference file: upload and delete are refused, the text
    // on disk is untouched.
    let resp = upload(&daemon, id, &[("file", Some("SKILL.md"), b"replaced")]).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["code"], "skill_md_reserved");
    assert_eq!(
        std::fs::read_to_string(root.join(id).join("SKILL.md")).unwrap(),
        VALID
    );
    let resp = reqwest::Client::new()
        .delete(format!(
            "{}/settings/skills/{id}/files/SKILL.md",
            daemon.url()
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert!(root.join(id).join("SKILL.md").is_file());
}

#[tokio::test]
async fn a_file_over_10_mb_is_a_413_and_the_files_before_it_stay() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let skill = create_valid(&daemon).await;
    let id = skill["id"].as_str().unwrap();
    let big = vec![b'x'; 10 * 1024 * 1024 + 1];
    let resp = upload(
        &daemon,
        id,
        &[
            ("file", Some("small.md"), b"ok"),
            ("file", Some("big.bin"), &big),
        ],
    )
    .await;
    assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["code"], "file_too_large");
    assert!(body["error"].as_str().unwrap().contains("10 MB limit"));
    // The batch stops at the refusal; what landed is reported.
    assert_eq!(
        body["uploaded"],
        serde_json::json!([{ "path": "small.md", "size": 2 }])
    );
    assert!(!skills_root(&daemon).join(id).join("big.bin").exists());
}

#[tokio::test]
async fn a_file_is_read_and_edited_as_plain_text_and_binaries_are_flagged() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let skill = create_valid(&daemon).await;
    let id = skill["id"].as_str().unwrap();
    upload(
        &daemon,
        id,
        &[
            ("file", Some("notes.md"), b"# Notes\n"),
            (
                "file",
                Some("logo.png"),
                &[0x89, b'P', b'N', b'G', 0xff, 0xfe],
            ),
        ],
    )
    .await;

    let file = get_json(&daemon, &format!("/settings/skills/{id}/files/notes.md")).await;
    assert_eq!(file["text"], "# Notes\n");
    assert_eq!(file["binary"], false);
    assert_eq!(file["size"], 8);

    let png = get_json(&daemon, &format!("/settings/skills/{id}/files/logo.png")).await;
    assert_eq!(png["binary"], true);
    assert!(png["text"].is_null());

    // SKILL.md is readable through the same seam (the editor opens it too).
    let md = get_json(&daemon, &format!("/settings/skills/{id}/files/SKILL.md")).await;
    assert_eq!(md["text"], VALID);

    // Save the editor's text.
    let resp = put_text(
        &daemon,
        &format!("/settings/skills/{id}/files/notes.md"),
        "# Notes\n\nEdited.\n",
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        std::fs::read_to_string(skills_root(&daemon).join(id).join("notes.md")).unwrap(),
        "# Notes\n\nEdited.\n"
    );
    // Saving a file that does not exist is a 404 (the editor never creates).
    let resp = put_text(
        &daemon,
        &format!("/settings/skills/{id}/files/ghost.md"),
        "x",
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    // A 404 skill.
    let resp = put_text(&daemon, "/settings/skills/nope/files/notes.md", "x").await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn saving_skill_md_reruns_the_five_checks_and_refuses_without_writing() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let skill = create_valid(&daemon).await;
    let id = skill["id"].as_str().unwrap();
    let path = format!("/settings/skills/{id}/files/SKILL.md");

    let resp = put_text(&daemon, &path, NO_DESCRIPTION).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["code"], "missing_description");
    assert_eq!(
        std::fs::read_to_string(skills_root(&daemon).join(id).join("SKILL.md")).unwrap(),
        VALID
    );

    let edited = VALID.replace("Red-green-refactor at pre-agreed seams.", "Edited by drop.");
    let resp = put_text(&daemon, &path, &edited).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["skill"]["description"],
        "Test-driven development. Edited by drop."
    );
    assert_eq!(body["skill"]["name"], "tdd");
    assert_eq!(
        std::fs::read_to_string(skills_root(&daemon).join(id).join("SKILL.md")).unwrap(),
        edited
    );
    let detail = get_json(&daemon, &format!("/settings/skills/{id}")).await;
    assert_eq!(
        detail["description"],
        "Test-driven development. Edited by drop."
    );
    assert_eq!(detail["content"], edited);
}

#[tokio::test]
async fn file_endpoints_on_an_unknown_skill_are_404() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let resp = upload(&daemon, "nope", &[("file", Some("a.md"), b"a")]).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let resp = reqwest::get(format!("{}/settings/skills/nope/files/a.md", daemon.url()))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// #669 — selection by tier, skills actifs with origin, referents populated.
// A real daemon, a seeded pipeline, nodes spawned through the tmux command seam
// (a harmless `sleep`). Every assertion is an HTTP response or an event payload.
// ---------------------------------------------------------------------------

const TIER_PIPELINE: &str = "skills-tiers";

fn tier_pipeline_yaml(node_skills: &str) -> String {
    format!(
        r#"name: skills-tiers
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: aaaaaaaa
    name: worker
    type: agent
    isolated_worktree: false
{node_skills}    outputs:
      - name: out
    view: {{ x: 200, y: 60 }}
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: {{ node: start, port: user_prompt }}
    target: {{ node: aaaaaaaa, port: task }}
  - source: {{ node: aaaaaaaa, port: out }}
    target: {{ node: end, port: result }}
"#
    )
}

fn seed_tier_pipeline(repo: &Path) -> anyhow::Result<()> {
    let pipelines_dir = repo.join(".pdo").join("pipelines");
    std::fs::create_dir_all(&pipelines_dir)?;
    std::fs::write(
        pipelines_dir.join(format!("{TIER_PIPELINE}.yaml")),
        tier_pipeline_yaml(""),
    )?;
    let prompts_dir = pipelines_dir.join(format!("{TIER_PIPELINE}.prompts"));
    std::fs::create_dir_all(&prompts_dir)?;
    std::fs::write(prompts_dir.join("aaaaaaaa.md"), "You are the worker.\n")?;
    let run = |args: &[&str]| -> anyhow::Result<()> {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()?;
        anyhow::ensure!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
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

/// A valid skill named `name`, created through the API; returns its `{id, name}`
/// reference as the tiers store it.
async fn create_named_skill(daemon: &TestDaemon, name: &str) -> serde_json::Value {
    let content =
        format!("---\nname: {name}\ndescription: The {name} method.\n---\n\n# {name}\n\nBody.\n");
    let resp = post_skill(daemon, serde_json::json!({ "content": content })).await;
    assert_eq!(resp.status(), StatusCode::CREATED, "skill `{name}`");
    let skill: serde_json::Value = resp.json().await.unwrap();
    serde_json::json!({ "id": skill["id"], "name": skill["name"] })
}

async fn post_json(daemon: &TestDaemon, path: &str, body: serde_json::Value) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{}{path}", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap()
}

async fn patch_json(daemon: &TestDaemon, path: &str, body: serde_json::Value) -> reqwest::Response {
    reqwest::Client::new()
        .patch(format!("{}{path}", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap()
}

async fn events_of(daemon: &TestDaemon, run_id: &str) -> Vec<serde_json::Value> {
    let v: serde_json::Value = get_json(daemon, &format!("/runs/{run_id}/events")).await;
    v.as_array().cloned().unwrap_or_default()
}

/// The worker node's `NodeStarted` payload, once it exists.
async fn wait_for_node_started(daemon: &TestDaemon, run_id: &str) -> serde_json::Value {
    for _ in 0..80 {
        let evs = events_of(daemon, run_id).await;
        if let Some(e) = evs
            .iter()
            .rev()
            .find(|e| e["kind"] == "node_started" && e["node_id"] == "aaaaaaaa")
        {
            return e["payload"].clone();
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("worker node should have started within the timeout");
}

fn skill_ids(list: &serde_json::Value) -> Vec<String> {
    list.as_array()
        .map(|a| {
            a.iter()
                .map(|s| s["id"].as_str().unwrap().to_string())
                .collect()
        })
        .unwrap_or_default()
}

fn tiers_of(list: &serde_json::Value, id: &serde_json::Value) -> serde_json::Value {
    list.as_array()
        .and_then(|a| a.iter().find(|s| s["id"] == *id))
        .map(|s| s["tiers"].clone())
        .unwrap_or(serde_json::Value::Null)
}

/// FP steps 1–2 (#669): A at the instance tier, B on the Projet owning the repo,
/// C on the node; the node's `NodeStarted` freezes the union with each origin,
/// and the Run projection exposes it on the node.
#[tokio::test]
async fn skills_of_the_four_tiers_are_unioned_at_spawn_with_their_origin() {
    let daemon = TestDaemon::spawn(seed_tier_pipeline).await.unwrap();
    let primary = daemon.target_repo();
    let a = create_named_skill(&daemon, "tdd").await;
    let b = create_named_skill(&daemon, "grilling").await;
    let c = create_named_skill(&daemon, "code-review").await;
    let d = create_named_skill(&daemon, "docs").await;

    // Instance tier — a settings knob, read back on GET.
    let resp = put_json(&daemon, "/settings", serde_json::json!({ "skills": [a] })).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let settings = get_json(&daemon, "/settings").await;
    assert_eq!(
        skill_ids(&settings["skills"]),
        vec![a["id"].as_str().unwrap()]
    );

    // Projet tier — the Projet owning the Run's primary repo.
    let project: serde_json::Value = post_json(
        &daemon,
        "/projects",
        serde_json::json!({ "name": "Product" }),
    )
    .await
    .json()
    .await
    .unwrap();
    let project_id = project["id"].as_str().unwrap().to_string();
    assert_eq!(
        post_json(
            &daemon,
            &format!("/projects/{project_id}/members"),
            serde_json::json!({ "path": primary })
        )
        .await
        .status(),
        StatusCode::OK
    );
    let resp = patch_json(
        &daemon,
        &format!("/projects/{project_id}"),
        serde_json::json!({ "skills": [b, a] }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let project: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(skill_ids(&project["skills"]).len(), 2, "{project}");

    // Node tier — the document references skills by id, name beside it.
    let node_skills = format!(
        "    skills:\n      - id: {}\n        name: {}\n",
        c["id"].as_str().unwrap(),
        c["name"].as_str().unwrap()
    );
    let resp = put_json(
        &daemon,
        &format!("/pipelines/{TIER_PIPELINE}"),
        serde_json::json!({ "yaml": tier_pipeline_yaml(&node_skills), "prompts": {} }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    // Run tier — frozen on `RunStarted` when non-empty.
    let resp = post_json(
        &daemon,
        "/runs",
        serde_json::json!({
            "pipeline": TIER_PIPELINE,
            "input": "go",
            "target_repo": primary,
            "skills": [d],
        }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let run: serde_json::Value = resp.json().await.unwrap();
    let run_id = run["run_id"].as_str().unwrap().to_string();

    let payload = wait_for_node_started(&daemon, &run_id).await;
    let active = &payload["skills"];
    assert_eq!(
        skill_ids(active),
        vec![
            a["id"].as_str().unwrap(),
            b["id"].as_str().unwrap(),
            d["id"].as_str().unwrap(),
            c["id"].as_str().unwrap()
        ],
        "union, coarsest tier first, de-duplicated: {payload}"
    );
    assert_eq!(
        tiers_of(active, &a["id"]),
        serde_json::json!(["instance", "project"])
    );
    assert_eq!(tiers_of(active, &b["id"]), serde_json::json!(["project"]));
    assert_eq!(tiers_of(active, &d["id"]), serde_json::json!(["run"]));
    assert_eq!(tiers_of(active, &c["id"]), serde_json::json!(["node"]));
    assert_eq!(payload["missing_skills"], serde_json::json!([]));

    // The Run tier was frozen on `RunStarted`; the projection exposes both.
    let evs = events_of(&daemon, &run_id).await;
    let started = evs.iter().find(|e| e["kind"] == "run_started").unwrap();
    assert_eq!(
        skill_ids(&started["payload"]["skills"]),
        vec![d["id"].as_str().unwrap()]
    );
    let state = get_json(&daemon, &format!("/runs/{run_id}")).await;
    assert_eq!(skill_ids(&state["skills"]), vec![d["id"].as_str().unwrap()]);
    assert_eq!(
        skill_ids(&state["nodes"]["aaaaaaaa"]["skills"]).len(),
        4,
        "{state}"
    );
}

/// FP step 4 (#669): a skill deleted from the bank but still selected produces a
/// warning on the node (`missing_skills`) and the Run launches anyway.
#[tokio::test]
async fn a_deleted_skill_is_a_warning_on_the_node_never_a_refusal_to_launch() {
    let daemon = TestDaemon::spawn(seed_tier_pipeline).await.unwrap();
    let gone = create_named_skill(&daemon, "ephemeral").await;
    let kept = create_named_skill(&daemon, "kept").await;
    let gone_id = gone["id"].as_str().unwrap().to_string();

    // Selected at the instance tier, then deleted from the bank.
    assert_eq!(
        put_json(
            &daemon,
            "/settings",
            serde_json::json!({ "skills": [gone, kept] })
        )
        .await
        .status(),
        StatusCode::OK
    );
    let resp = reqwest::Client::new()
        .delete(format!("{}/settings/skills/{gone_id}", daemon.url()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    let resp = post_json(
        &daemon,
        "/runs",
        serde_json::json!({ "pipeline": TIER_PIPELINE, "input": "go", "target_repo": daemon.target_repo() }),
    )
    .await;
    assert_eq!(
        resp.status(),
        StatusCode::CREATED,
        "the Run launches anyway"
    );
    let run: serde_json::Value = resp.json().await.unwrap();
    let run_id = run["run_id"].as_str().unwrap().to_string();

    let payload = wait_for_node_started(&daemon, &run_id).await;
    assert_eq!(
        skill_ids(&payload["skills"]),
        vec![kept["id"].as_str().unwrap()]
    );
    assert_eq!(payload["missing_skills"][0]["id"], gone_id);
    assert_eq!(
        payload["missing_skills"][0]["name"], "ephemeral",
        "the stored label survives"
    );
    assert_eq!(
        payload["missing_skills"][0]["tiers"],
        serde_json::json!(["instance"])
    );
    let state = get_json(&daemon, &format!("/runs/{run_id}")).await;
    assert_eq!(state["status"], "running", "{state}");
    assert_eq!(
        state["nodes"]["aaaaaaaa"]["missing_skills"][0]["id"],
        gone_id
    );
}

/// AC (#669): a `script` node carrying `skills` is refused at parse with a clear
/// message — on the single-node parse endpoint and on a pipeline write alike.
#[tokio::test]
async fn a_script_node_with_skills_is_refused_at_parse_over_http() {
    let daemon = TestDaemon::spawn(seed_tier_pipeline).await.unwrap();
    let resp = post_json(
        &daemon,
        "/nodes/parse",
        serde_json::json!({
            "yaml": "name: sh\ntype: script\nskills:\n  - id: 11111111-1111-1111-1111-111111111111\n    name: tdd\nprompt: |\n  echo hi\n"
        }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = resp.json().await.unwrap();
    let message = body.to_string();
    assert!(message.contains("script"), "{message}");
    assert!(message.contains("skills"), "{message}");

    let yaml = tier_pipeline_yaml(
        "    skills:\n      - id: 11111111-1111-1111-1111-111111111111\n        name: tdd\n",
    )
    .replace(
        "    type: agent\n    isolated_worktree: false",
        "    type: script\n    isolated_worktree: false",
    );
    let resp = put_json(
        &daemon,
        &format!("/pipelines/{TIER_PIPELINE}"),
        serde_json::json!({ "yaml": yaml, "prompts": {} }),
    )
    .await;
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "a pipeline write is refused too"
    );
}

/// AC (#669): `GET /settings/skills/{id}/referents` lists the instance, the
/// Projets, the Triggers and the pipelines' nodes that select the skill — and a
/// rename never breaks any of them (identity is the id).
#[tokio::test]
async fn referents_list_every_tier_that_selects_the_skill() {
    let daemon = TestDaemon::spawn(seed_tier_pipeline).await.unwrap();
    let primary = daemon.target_repo();
    let s = create_named_skill(&daemon, "shared").await;
    let other = create_named_skill(&daemon, "other").await;
    let id = s["id"].as_str().unwrap().to_string();

    // Nothing selects it yet.
    let referents = get_json(&daemon, &format!("/settings/skills/{id}/referents")).await;
    assert_eq!(referents["instance"], false);
    assert_eq!(referents["projects"], serde_json::json!([]));
    assert_eq!(referents["triggers"], serde_json::json!([]));
    assert_eq!(referents["pipelines"], serde_json::json!([]));
    assert!(referents["runs"].is_array());

    assert_eq!(
        put_json(
            &daemon,
            "/settings",
            serde_json::json!({ "skills": [s.clone()] })
        )
        .await
        .status(),
        StatusCode::OK
    );
    let project: serde_json::Value = post_json(
        &daemon,
        "/projects",
        serde_json::json!({ "name": "Product" }),
    )
    .await
    .json()
    .await
    .unwrap();
    let project_id = project["id"].as_str().unwrap().to_string();
    post_json(
        &daemon,
        &format!("/projects/{project_id}/members"),
        serde_json::json!({ "path": primary }),
    )
    .await;
    assert_eq!(
        patch_json(
            &daemon,
            &format!("/projects/{project_id}"),
            serde_json::json!({ "skills": [s.clone()] })
        )
        .await
        .status(),
        StatusCode::OK
    );
    let trigger: serde_json::Value = post_json(
        &daemon,
        "/triggers",
        serde_json::json!({
            "name": "nightly",
            "pipeline_id": TIER_PIPELINE,
            "cron": "0 3 * * *",
            "input_template": "audit",
            "target_repo": primary,
            "skills": [s.clone(), other.clone()],
        }),
    )
    .await
    .json()
    .await
    .unwrap();
    let trigger_id = trigger["id"].as_str().unwrap().to_string();
    assert_eq!(skill_ids(&trigger["skills"]).len(), 2, "{trigger}");
    let node_skills = format!("    skills:\n      - id: {id}\n        name: shared\n");
    assert_eq!(
        put_json(
            &daemon,
            &format!("/pipelines/{TIER_PIPELINE}"),
            serde_json::json!({ "yaml": tier_pipeline_yaml(&node_skills), "prompts": {} }),
        )
        .await
        .status(),
        StatusCode::OK
    );

    // A rename touches the label only: every referent still resolves.
    assert_eq!(
        put_json(
            &daemon,
            &format!("/settings/skills/{id}"),
            serde_json::json!({ "name": "shared-renamed" })
        )
        .await
        .status(),
        StatusCode::OK
    );

    let referents = get_json(&daemon, &format!("/settings/skills/{id}/referents")).await;
    assert_eq!(referents["skill_id"], id);
    assert_eq!(referents["instance"], true);
    assert_eq!(referents["projects"][0]["id"], project_id);
    assert_eq!(referents["projects"][0]["name"], "Product");
    assert_eq!(referents["triggers"][0]["id"], trigger_id);
    assert_eq!(referents["pipelines"][0]["name"], TIER_PIPELINE);
    assert_eq!(referents["pipelines"][0]["node_id"], "aaaaaaaa");
    assert!(referents["runs"].is_array());

    // `other` is referenced by the trigger only.
    let other_id = other["id"].as_str().unwrap();
    let referents = get_json(&daemon, &format!("/settings/skills/{other_id}/referents")).await;
    assert_eq!(referents["instance"], false);
    assert_eq!(referents["projects"], serde_json::json!([]));
    assert_eq!(referents["triggers"].as_array().unwrap().len(), 1);
    assert_eq!(referents["pipelines"], serde_json::json!([]));

    // Clearing a tier: an empty list clears (flat, no double-Option).
    assert_eq!(
        patch_json(
            &daemon,
            &format!("/triggers/{trigger_id}"),
            serde_json::json!({ "skills": [] })
        )
        .await
        .status(),
        StatusCode::OK
    );
    let trigger = get_json(&daemon, &format!("/triggers/{trigger_id}")).await;
    assert!(skill_ids(&trigger["skills"]).is_empty(), "{trigger}");
    assert_eq!(
        put_json(&daemon, "/settings", serde_json::json!({ "skills": [] }))
            .await
            .status(),
        StatusCode::OK
    );
    let settings = get_json(&daemon, "/settings").await;
    assert!(skill_ids(&settings["skills"]).is_empty(), "{settings}");
    let referents = get_json(&daemon, &format!("/settings/skills/{id}/referents")).await;
    assert_eq!(referents["instance"], false);
    assert_eq!(referents["triggers"], serde_json::json!([]));
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
    assert_eq!(user_skills(&bank), Vec::<&serde_json::Value>::new());
    assert_eq!(user_skill_dirs(&skills_root(&daemon)), 0);
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
    let skills = user_skills(&bank);
    assert_eq!(skills.len(), 2, "{bank}");
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
    assert_eq!(user_skills(&bank).len(), 2, "no folder was created");

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
    let names: Vec<&str> = user_skills(&bank)
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

// ---------------------------------------------------------------------------
// Archive de skills — export (pdo-issues#2, #1026, ADR-0084)
// ---------------------------------------------------------------------------

async fn export_archive(daemon: &TestDaemon, body: serde_json::Value) -> reqwest::Response {
    post_json(daemon, "/settings/skills/export", body).await
}

/// Every entry of a zip, by name, in archive order.
fn zip_entries(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    use std::io::Read;
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    (0..archive.len())
        .map(|i| {
            let mut entry = archive.by_index(i).unwrap();
            let mut data = Vec::new();
            entry.read_to_end(&mut data).unwrap();
            (entry.name().to_string(), data)
        })
        .collect()
}

fn disposition(resp: &reqwest::Response) -> String {
    resp.headers()
        .get("content-disposition")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn fp1026_export_is_a_flat_zip_with_reference_files_and_a_manifest_of_ids() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let review = create_named_skill(&daemon, "code-review").await;
    let checklist = create_named_skill(&daemon, "review-checklist").await;
    let scan = create_named_skill(&daemon, "security-scan").await;
    let review_id = review["id"].as_str().unwrap();
    let checklist_id = checklist["id"].as_str().unwrap();
    // A folder in the bank: its hierarchy must not leak into the archive.
    let folder: serde_json::Value = post_json(
        &daemon,
        "/settings/skill-folders",
        serde_json::json!({ "name": "team-review" }),
    )
    .await
    .json()
    .await
    .unwrap();
    let resp = put_json(
        &daemon,
        &format!("/settings/skills/{review_id}"),
        serde_json::json!({ "folder_id": folder["id"] }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let resp = upload(
        &daemon,
        review_id,
        &[
            ("path", None, b"checklist.md"),
            ("file", Some("checklist.md"), b"# Checklist\n"),
            ("path", None, b"scripts/run.sh"),
            ("file", Some("run.sh"), b"#!/bin/sh\necho ok\n"),
        ],
    )
    .await;
    assert!(resp.status().is_success(), "{:?}", resp.text().await);

    let resp = export_archive(
        &daemon,
        serde_json::json!({ "ids": [review_id, checklist_id], "file_name": "team-skills" }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers()["content-type"], "application/zip");
    assert_eq!(resp.headers()["x-pdo-skills"], "2");
    assert!(
        disposition(&resp).contains("filename=\"team-skills.zip\""),
        "{}",
        disposition(&resp)
    );
    let bytes = resp.bytes().await.unwrap();
    let entries = zip_entries(&bytes);
    let names = entries.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>();
    assert_eq!(
        names,
        vec![
            "code-review/SKILL.md",
            "code-review/checklist.md",
            "code-review/scripts/run.sh",
            "review-checklist/SKILL.md",
            "pdo-skills.json",
        ],
        "flat: one folder per skill, no bank folder, no unchecked skill"
    );
    // SKILL.md byte for byte as the bank stores it.
    let on_disk = std::fs::read(skills_root(&daemon).join(review_id).join("SKILL.md")).unwrap();
    assert_eq!(entries[0].1, on_disk);
    assert_eq!(entries[2].1, b"#!/bin/sh\necho ok\n");
    assert!(
        !names.iter().any(|n| n.starts_with("security-scan")),
        "{scan}"
    );

    let manifest: serde_json::Value = serde_json::from_slice(&entries[4].1).unwrap();
    assert_eq!(manifest["format"], "pdo-skills");
    assert_eq!(manifest["version"], 1);
    assert!(
        chrono::DateTime::parse_from_rfc3339(manifest["exported_at"].as_str().unwrap()).is_ok(),
        "{manifest}"
    );
    assert_eq!(
        manifest["skills"],
        serde_json::json!([
            { "folder": "code-review", "id": review_id, "name": "code-review" },
            { "folder": "review-checklist", "id": checklist_id, "name": "review-checklist" },
        ])
    );

    // The bank is not modified by an export.
    let bank = get_json(&daemon, "/settings/skills").await;
    assert_eq!(user_skills(&bank).len(), 3);
}

#[tokio::test]
async fn fp1026_export_refuses_unknown_and_seeded_ids_and_an_empty_selection() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let tdd = create_named_skill(&daemon, "tdd").await;

    let resp = export_archive(
        &daemon,
        serde_json::json!({ "ids": [tdd["id"], "nope-0000"] }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["code"], "unknown_skill");
    assert_eq!(body["id"], "nope-0000");
    assert!(
        body["error"].as_str().unwrap().contains("nope-0000"),
        "{body}"
    );

    for seeded in ["pdo-orchestrate", "pdo-interactive"] {
        let resp = export_archive(&daemon, serde_json::json!({ "ids": [tdd["id"], seeded] })).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN, "{seeded}");
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["code"], "locked");
        assert_eq!(body["id"], seeded);
    }

    let resp = export_archive(&daemon, serde_json::json!({ "ids": [] })).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["code"], "empty_selection");
}

#[tokio::test]
async fn fp1026_export_file_name_defaults_and_gets_zip_appended() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let tdd = create_named_skill(&daemon, "tdd").await;
    let grilling = create_named_skill(&daemon, "grilling").await;

    // One skill: `<name>.skills.zip`.
    let resp = export_archive(&daemon, serde_json::json!({ "ids": [tdd["id"]] })).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(
        disposition(&resp).contains("filename=\"tdd.skills.zip\""),
        "{}",
        disposition(&resp)
    );

    // Several: `pdo-skills-<YYYYMMDD-HHMM>.zip`.
    let resp = export_archive(
        &daemon,
        serde_json::json!({ "ids": [tdd["id"], grilling["id"]], "file_name": "" }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let header = disposition(&resp);
    let name = header
        .split("filename=\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .unwrap();
    assert!(
        name.starts_with("pdo-skills-") && name.ends_with(".zip"),
        "{name}"
    );
    let stamp = &name["pdo-skills-".len()..name.len() - ".zip".len()];
    assert_eq!(stamp.len(), 13, "{name}");
    assert!(
        stamp
            .chars()
            .enumerate()
            .all(|(i, c)| if i == 8 { c == '-' } else { c.is_ascii_digit() }),
        "{name}"
    );

    // An explicit name keeps its `.zip`, or gets one.
    let resp = export_archive(
        &daemon,
        serde_json::json!({ "ids": [tdd["id"]], "file_name": "mine.zip" }),
    )
    .await;
    assert!(disposition(&resp).contains("filename=\"mine.zip\""));
    let resp = export_archive(
        &daemon,
        serde_json::json!({ "ids": [tdd["id"]], "file_name": "équipe" }),
    )
    .await;
    assert!(
        disposition(&resp).contains("filename*=UTF-8''%C3%A9quipe.zip"),
        "{}",
        disposition(&resp)
    );
}

#[tokio::test]
async fn fp1026_an_unzipped_archive_is_a_valid_local_folder_source() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let pdf = create_named_skill(&daemon, "pdf").await;
    let tdd = create_named_skill(&daemon, "tdd").await;
    let pdf_id = pdf["id"].as_str().unwrap();
    upload(
        &daemon,
        pdf_id,
        &[
            ("path", None, b"reference.md"),
            ("file", Some("reference.md"), b"# Ref\n"),
        ],
    )
    .await;
    let resp = export_archive(&daemon, serde_json::json!({ "ids": [pdf_id, tdd["id"]] })).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.bytes().await.unwrap();

    let unzipped = tempfile::tempdir().unwrap();
    for (name, data) in zip_entries(&bytes) {
        let path = unzipped.path().join(&name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, data).unwrap();
    }
    let resp = post_json(
        &daemon,
        "/settings/skills/scan",
        serde_json::json!({ "scan_id": "scan-archive", "source": unzipped.path().display().to_string() }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["source"]["kind"], "local");
    let candidates = body["candidates"].as_array().unwrap();
    let mut names = candidates
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(names, vec!["pdf", "tdd"], "{body}");
    assert!(candidates.iter().all(|c| c["valid"] == true), "{body}");
    let pdf_row = candidates.iter().find(|c| c["name"] == "pdf").unwrap();
    assert_eq!(pdf_row["file_count"], 1, "{body}");
}

// ---------------------------------------------------------------------------
// Import d'archive (pdo-issues#2, #1027, ADR-0084): a dropped `.zip` is a Source
// ---------------------------------------------------------------------------

/// Scan an uploaded archive, as the import popup does (base64 in the body).
async fn scan_archive(
    daemon: &TestDaemon,
    scan_id: &str,
    file_name: &str,
    bytes: &[u8],
) -> (StatusCode, serde_json::Value) {
    use base64::Engine as _;
    let resp = post_json(
        daemon,
        "/settings/skills/scan",
        serde_json::json!({
            "scan_id": scan_id,
            "archive": {
                "file_name": file_name,
                "data": base64::engine::general_purpose::STANDARD.encode(bytes),
            },
        }),
    )
    .await;
    let status = resp.status();
    (status, resp.json().await.unwrap())
}

async fn import_archive(
    daemon: &TestDaemon,
    scan_id: &str,
    file_name: &str,
    folder: serde_json::Value,
    items: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let resp = post_json(
        daemon,
        "/settings/skills/import",
        serde_json::json!({
            "scan_id": scan_id,
            "source": file_name,
            "kind": "file",
            "folder": folder,
            "items": items,
        }),
    )
    .await;
    let status = resp.status();
    (status, resp.json().await.unwrap())
}

fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
    use std::io::Write;
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, data) in entries {
        zip.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn skill_md_of(name: &str) -> String {
    format!("---\nname: {name}\ndescription: The {name} method.\n---\n\n# {name}\n\nBody.\n")
}

fn candidate<'a>(scan: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
    scan["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == name)
        .unwrap_or_else(|| panic!("no candidate `{name}` in {scan}"))
}

async fn export_bytes(daemon: &TestDaemon, ids: &[&str], file_name: &str) -> Vec<u8> {
    let resp = export_archive(
        daemon,
        serde_json::json!({ "ids": ids, "file_name": file_name }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    resp.bytes().await.unwrap().to_vec()
}

/// FP steps 1–3: export two skills → the archive dropped back is « already up to
/// date »; one of them deleted → it is offered for creation **with its id**, and
/// lands in a new folder named after the file, tagged « file », with no update.
#[tokio::test]
async fn fp1027_reimport_is_up_to_date_then_a_deleted_skill_comes_back_with_its_id() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let review = create_named_skill(&daemon, "code-review").await;
    let checklist = create_named_skill(&daemon, "review-checklist").await;
    let review_id = review["id"].as_str().unwrap().to_string();
    let checklist_id = checklist["id"].as_str().unwrap().to_string();
    upload(
        &daemon,
        &review_id,
        &[
            ("path", None, b"scripts/run.sh"),
            ("file", Some("run.sh"), b"#!/bin/sh\necho ok\n"),
        ],
    )
    .await;
    let bytes = export_bytes(&daemon, &[&review_id, &checklist_id], "team-review").await;

    // Step 2: both known with identical content.
    let (status, scan) = scan_archive(&daemon, "a-1", "team-review.zip", &bytes).await;
    assert_eq!(status, StatusCode::OK, "{scan}");
    assert_eq!(scan["source"]["kind"], "file");
    assert_eq!(scan["source"]["url"], "team-review.zip");
    assert_eq!(scan["source"]["suggested_folder"], "team-review");
    assert!(scan["commit"].is_null());
    assert_eq!(scan["archive"]["format"], "pdo_archive");
    assert_eq!(scan["archive"]["file_name"], "team-review.zip");
    assert!(scan["archive"]["existing_folder"].is_null(), "{scan}");
    assert_eq!(scan["archive"]["new_folder_name"], "team-review");
    for (name, id) in [
        ("code-review", &review_id),
        ("review-checklist", &checklist_id),
    ] {
        let row = candidate(&scan, name);
        assert_eq!(row["status"], "id_up_to_date", "{scan}");
        assert_eq!(row["id"], id.as_str());
        assert_eq!(row["existing"]["id"], id.as_str());
    }
    // A scan never writes, nor remembers a file as a recent source.
    let recent = get_json(&daemon, "/settings/skills/sources/recent").await;
    assert_eq!(recent["sources"], serde_json::json!([]), "{recent}");

    // Step 3: delete one → it is new, id kept.
    let resp = reqwest::Client::new()
        .delete(format!("{}/settings/skills/{review_id}", daemon.url()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    let (_, scan) = scan_archive(&daemon, "a-2", "team-review.zip", &bytes).await;
    let row = candidate(&scan, "code-review");
    assert_eq!(row["status"], "new", "{scan}");
    assert_eq!(row["id"], review_id.as_str());
    assert_eq!(row["file_count"], 1);
    assert_eq!(
        candidate(&scan, "review-checklist")["status"],
        "id_up_to_date"
    );

    let (status, report) = import_archive(
        &daemon,
        "a-2",
        "team-review.zip",
        serde_json::json!({ "name": "team-review" }),
        serde_json::json!([{ "path": "code-review", "action": "import" }]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["failed"], serde_json::json!([]));
    assert_eq!(report["imported"][0]["skill"]["id"], review_id.as_str());
    let folder = &report["folder"];
    assert_eq!(folder["name"], "team-review");
    assert_eq!(folder["source"]["kind"], "file");
    assert_eq!(folder["source"]["url"], "team-review.zip");
    assert!(folder["source"]["commit"].is_null());
    assert!(folder["source"]["ref"].is_null());
    let folder_id = folder["id"].as_str().unwrap().to_string();

    let detail = get_json(&daemon, &format!("/settings/skills/{review_id}")).await;
    assert_eq!(detail["folder_id"], folder_id.as_str(), "{detail}");
    assert_eq!(detail["source"]["kind"], "file");
    assert_eq!(detail["source"]["url"], "team-review.zip");
    assert_eq!(detail["source"]["path"], "code-review");
    assert_eq!(
        detail["files"],
        serde_json::json!([{ "path": "scripts/run.sh", "size": 18 }])
    );

    // One-shot: no rescan / update on a folder imported from a file.
    let resp = post_json(
        &daemon,
        &format!("/settings/skill-folders/{folder_id}/rescan"),
        serde_json::json!({ "scan_id": "a-3" }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["code"], "archive_folder", "{body}");
}

/// FP step 5: a folder named after the file exists → the scan says so and
/// proposes a free suffixed name; **merge** files the created skills in it and
/// leaves its provenance alone; **new folder** creates the suffixed one.
#[tokio::test]
async fn fp1027_merge_into_the_existing_folder_or_create_a_new_one() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let existing: serde_json::Value = post_json(
        &daemon,
        "/settings/skill-folders",
        serde_json::json!({ "name": "team-review" }),
    )
    .await
    .json()
    .await
    .unwrap();
    let existing_id = existing["id"].as_str().unwrap();
    let bytes = zip_of(&[
        ("alpha/SKILL.md", skill_md_of("alpha").as_bytes()),
        ("beta/SKILL.md", skill_md_of("beta").as_bytes()),
    ]);

    let (_, scan) = scan_archive(&daemon, "m-1", "team-review.skills.zip", &bytes).await;
    assert_eq!(scan["archive"]["format"], "plain_zip");
    assert_eq!(scan["archive"]["existing_folder"]["id"], existing_id);
    assert_eq!(scan["archive"]["existing_folder"]["name"], "team-review");
    assert_eq!(scan["archive"]["new_folder_name"], "team-review 2");

    // Merge: alpha joins the existing folder, which keeps no file provenance.
    let (status, report) = import_archive(
        &daemon,
        "m-1",
        "team-review.skills.zip",
        serde_json::json!({ "id": existing_id }),
        serde_json::json!([{ "path": "alpha", "action": "import" }]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["folder"]["id"], existing_id);
    assert!(report["folder"].get("source").is_none(), "{report}");
    let alpha_id = report["imported"][0]["skill"]["id"].as_str().unwrap();
    assert_ne!(alpha_id, "alpha", "a zip without manifest gets new ids");
    let bank = get_json(&daemon, "/settings/skills").await;
    let alpha = user_skills(&bank)
        .into_iter()
        .find(|s| s["name"] == "alpha")
        .unwrap();
    assert_eq!(alpha["folder_id"], existing_id);

    // New folder: the suffixed name, a second root folder.
    let (_, scan) = scan_archive(&daemon, "m-2", "team-review.skills.zip", &bytes).await;
    assert_eq!(candidate(&scan, "alpha")["status"], "name_taken");
    assert_eq!(candidate(&scan, "beta")["status"], "new");
    let (status, report) = import_archive(
        &daemon,
        "m-2",
        "team-review.skills.zip",
        serde_json::json!({ "name": scan["archive"]["new_folder_name"] }),
        serde_json::json!([{ "path": "beta", "action": "import" }]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["folder"]["name"], "team-review 2");
    assert_ne!(report["folder"]["id"], existing_id);
    assert_eq!(report["folder"]["source"]["kind"], "file");
}

/// FP step 4: the sidecar of a pipeline export is read too — the folder is the
/// id; a re-zipped `<id>/` sidecar at the root as well.
#[tokio::test]
async fn fp1027_a_pipeline_sidecar_lists_its_skills_by_id() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let tdd = create_named_skill(&daemon, "tdd").await;
    let tdd_id = tdd["id"].as_str().unwrap();
    let on_disk = std::fs::read(skills_root(&daemon).join(tdd_id).join("SKILL.md")).unwrap();
    let gone_id = "0b7c5f0e-8a2b-4d0e-9a51-4f1d2c3b4a59";
    let bytes = zip_of(&[
        (&format!("with-skills.skills/{tdd_id}/SKILL.md"), &on_disk),
        (
            &format!("with-skills.skills/{gone_id}/SKILL.md"),
            skill_md_of("grilling").as_bytes(),
        ),
    ]);
    let (status, scan) = scan_archive(&daemon, "s-1", "with-skills.skills.zip", &bytes).await;
    assert_eq!(status, StatusCode::OK, "{scan}");
    assert_eq!(scan["archive"]["format"], "pipeline_sidecar");
    assert_eq!(scan["source"]["suggested_folder"], "with-skills");
    assert_eq!(candidate(&scan, "tdd")["status"], "id_up_to_date");
    assert_eq!(candidate(&scan, "grilling")["status"], "new");
    assert_eq!(candidate(&scan, "grilling")["id"], gone_id);

    let (_, report) = import_archive(
        &daemon,
        "s-1",
        "with-skills.skills.zip",
        serde_json::json!({ "name": "with-skills" }),
        serde_json::json!([{ "path": format!("with-skills.skills/{gone_id}"), "action": "import" }]),
    )
    .await;
    assert_eq!(report["failed"], serde_json::json!([]), "{report}");
    assert_eq!(report["imported"][0]["skill"]["id"], gone_id);

    // Re-zipped at the root: `<uuid>/SKILL.md`.
    let bytes = zip_of(&[(
        &format!("{gone_id}/SKILL.md"),
        skill_md_of("grilling").as_bytes(),
    )]);
    let (_, scan) = scan_archive(&daemon, "s-2", "sidecar.zip", &bytes).await;
    assert_eq!(scan["archive"]["format"], "pipeline_sidecar");
    assert_eq!(candidate(&scan, "grilling")["status"], "id_up_to_date");
}

/// Statuses by id: a seeded id is `locked` (never importable); a known id with
/// another content is left as is here (`id_changed`); an unknown id whose name
/// is taken keeps the name rules; an invalid `SKILL.md` is greyed.
#[tokio::test]
async fn fp1027_seeded_ids_are_locked_and_name_rules_still_apply_without_a_known_id() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let tdd = create_named_skill(&daemon, "tdd").await;
    let tdd_id = tdd["id"].as_str().unwrap();
    let interactive = std::fs::read(
        skills_root(&daemon)
            .join("pdo-interactive")
            .join("SKILL.md"),
    )
    .unwrap();
    let manifest = serde_json::json!({
        "format": "pdo-skills",
        "version": 1,
        "exported_at": "2026-11-05T10:00:00Z",
        "skills": [
            { "folder": "pdo-interactive", "id": "pdo-interactive", "name": "pdo-interactive" },
            { "folder": "tdd", "id": tdd_id, "name": "tdd" },
            { "folder": "tdd-copy", "id": "other-id-1", "name": "tdd" },
            { "folder": "broken", "id": "other-id-2", "name": "broken" },
        ],
    })
    .to_string();
    let changed = "---\nname: tdd\ndescription: A newer tdd.\n---\n\n# tdd\n\nNew body.\n";
    let bytes = zip_of(&[
        ("pdo-interactive/SKILL.md", &interactive),
        ("tdd/SKILL.md", changed.as_bytes()),
        ("tdd-copy/SKILL.md", skill_md_of("tdd").as_bytes()),
        (
            "broken/SKILL.md",
            b"---\nname: broken\n---\n\nno description\n",
        ),
        ("pdo-skills.json", manifest.as_bytes()),
    ]);
    let (status, scan) = scan_archive(&daemon, "l-1", "x.zip", &bytes).await;
    assert_eq!(status, StatusCode::OK, "{scan}");
    let rows = scan["candidates"].as_array().unwrap();
    let by_path = |path: &str| rows.iter().find(|c| c["path"] == path).unwrap();
    assert_eq!(by_path("pdo-interactive")["status"], "locked");
    assert_eq!(by_path("tdd")["status"], "id_changed");
    assert_eq!(by_path("tdd")["existing"]["id"], tdd_id);
    assert_eq!(by_path("tdd-copy")["status"], "name_taken");
    assert_eq!(by_path("tdd-copy")["id"], "other-id-1");
    assert_eq!(by_path("broken")["status"], "invalid");

    // Even sent, neither the locked nor the known id is written over.
    let (status, report) = import_archive(
        &daemon,
        "l-1",
        "x.zip",
        serde_json::json!({ "name": "x" }),
        serde_json::json!([
            { "path": "pdo-interactive", "action": "import" },
            { "path": "tdd", "action": "import" },
            { "path": "tdd-copy", "action": "rename", "name": "tdd-imported" },
        ]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{report}");
    let failed = report["failed"].as_array().unwrap();
    assert_eq!(failed.len(), 2, "{report}");
    assert_eq!(failed[0]["code"], "locked");
    assert_eq!(failed[1]["code"], "id_known");
    // The renamed one is created with the archive's id.
    assert_eq!(report["imported"][0]["skill"]["id"], "other-id-1");
    assert_eq!(report["imported"][0]["skill"]["name"], "tdd-imported");
    let detail = get_json(&daemon, &format!("/settings/skills/{tdd_id}")).await;
    assert_eq!(detail["content"], skill_md_of("tdd"), "untouched");
}

/// Clear refusals: not a zip, a corrupted zip, a path-traversal entry, no
/// `SKILL.md`, bad base64 — all before anything is written.
#[tokio::test]
async fn fp1027_unreadable_archives_are_refused_with_a_named_reason() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let cases: Vec<(&str, Vec<u8>, &str)> = vec![
        ("notes.pdf", b"%PDF-1.7 not a zip".to_vec(), "not_a_zip"),
        ("notes.zip", b"plain text, not a zip".to_vec(), "not_a_zip"),
        (
            "broken.zip",
            b"PK\x03\x04garbage".to_vec(),
            "corrupt_archive",
        ),
        (
            "evil.zip",
            zip_of(&[
                ("a/SKILL.md", skill_md_of("a").as_bytes()),
                ("../../escape.sh", b"rm -rf /"),
            ]),
            "unsafe_entry",
        ),
        (
            "empty.zip",
            zip_of(&[("readme.txt", b"hello")]),
            "no_skills",
        ),
    ];
    for (file_name, bytes, code) in cases {
        let (status, body) = scan_archive(&daemon, "e-1", file_name, &bytes).await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{file_name}: {body}"
        );
        assert_eq!(body["code"], code, "{file_name}: {body}");
        // Shown as is in the import popup: a plain sentence, no Markdown.
        let error = body["error"].as_str().unwrap();
        assert!(error.len() > 10, "{body}");
        assert!(!error.contains('`'), "no Markdown backticks: {error}");
        assert!(error.ends_with('.'), "a full sentence: {error}");
    }
    let resp = post_json(
        &daemon,
        "/settings/skills/scan",
        serde_json::json!({ "scan_id": "e-2", "archive": { "file_name": "x.zip", "data": "%%%" } }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["code"], "invalid_upload");
    assert!(!daemon.repo_root().join("escape.sh").exists());

    // The client-chosen scan id names a temp folder: a path in it is refused
    // before anything is deleted or written.
    let victim = tempfile::tempdir().unwrap();
    std::fs::write(victim.path().join("keep.txt"), "keep").unwrap();
    let evil_id = format!("x/../../../../../../..{}", victim.path().display());
    let (status, body) = scan_archive(
        &daemon,
        &evil_id,
        "ok.zip",
        &zip_of(&[("a/SKILL.md", skill_md_of("a").as_bytes())]),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "invalid_scan_id");
    assert!(victim.path().join("keep.txt").is_file());
    assert!(!victim.path().join("a").exists());
    let bank = get_json(&daemon, "/settings/skills").await;
    assert_eq!(user_skills(&bank).len(), 0);

    // An import against an unknown archive scan is a 410, never a re-scan.
    let (status, body) = import_archive(
        &daemon,
        "nope",
        "x.zip",
        serde_json::json!({ "name": "x" }),
        serde_json::json!([{ "path": "a", "action": "import" }]),
    )
    .await;
    assert_eq!(status, StatusCode::GONE, "{body}");
    assert_eq!(body["code"], "scan_expired");
}

/// A file over the bank's per-file limit fails its own row; the others import.
#[tokio::test]
async fn fp1027_a_file_over_the_per_file_limit_fails_on_its_row_only() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let big = vec![b'x'; 10 * 1024 * 1024 + 1];
    let bytes = zip_of(&[
        ("heavy/SKILL.md", skill_md_of("heavy").as_bytes()),
        ("heavy/data/huge.bin", &big),
        ("light/SKILL.md", skill_md_of("light").as_bytes()),
    ]);
    let (status, scan) = scan_archive(&daemon, "big-1", "mixed.zip", &bytes).await;
    assert_eq!(status, StatusCode::OK, "{scan}");
    let heavy = candidate(&scan, "heavy");
    assert_eq!(heavy["status"], "invalid");
    assert_eq!(heavy["code"], "file_too_large");
    assert!(
        heavy["reason"].as_str().unwrap().contains("data/huge.bin"),
        "{heavy}"
    );
    assert_eq!(candidate(&scan, "light")["status"], "new");

    let (status, report) = import_archive(
        &daemon,
        "big-1",
        "mixed.zip",
        serde_json::json!({ "name": "mixed" }),
        serde_json::json!([
            { "path": "heavy", "action": "import" },
            { "path": "light", "action": "import" },
        ]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["failed"][0]["path"], "heavy");
    assert_eq!(report["failed"][0]["code"], "file_too_large");
    assert_eq!(report["imported"].as_array().unwrap().len(), 1);
    assert_eq!(report["imported"][0]["skill"]["name"], "light");
}

/// The round-trip of the AC: export on daemon A → import on a **fresh** daemon
/// B, ids preserved → a pipeline referencing those ids then imports on B with no
/// « skill absent » warning (no sidecar needed: the bank already has them).
#[tokio::test]
async fn fp1027_round_trip_a_to_b_keeps_ids_and_pipelines_resolve_them() {
    let a = TestDaemon::spawn(seed_tier_pipeline).await.unwrap();
    let tdd = create_named_skill(&a, "tdd").await;
    let grilling = create_named_skill(&a, "grilling").await;
    let tdd_id = tdd["id"].as_str().unwrap().to_string();
    let grilling_id = grilling["id"].as_str().unwrap().to_string();
    upload(
        &a,
        &tdd_id,
        &[
            ("path", None, b"examples/login.spec.ts"),
            ("file", Some("login.spec.ts"), b"it('logs in')\n"),
        ],
    )
    .await;
    let skills_yaml = format!(
        "    skills:\n      - id: {tdd_id}\n        name: tdd\n      - id: {grilling_id}\n        name: grilling\n"
    );
    let resp = put_json(
        &a,
        &format!("/pipelines/{TIER_PIPELINE}"),
        serde_json::json!({ "yaml": tier_pipeline_yaml(&skills_yaml), "prompts": {} }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let document = reqwest::get(format!("{}/pipelines/{TIER_PIPELINE}/document", a.url()))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let archive = export_bytes(&a, &[&tdd_id, &grilling_id], "team-skills").await;

    let b = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let (status, scan) = scan_archive(&b, "ab-1", "team-skills.zip", &archive).await;
    assert_eq!(status, StatusCode::OK, "{scan}");
    let items = scan["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            assert_eq!(c["status"], "new", "{scan}");
            serde_json::json!({ "path": c["path"], "action": "import" })
        })
        .collect::<Vec<_>>();
    let (status, report) = import_archive(
        &b,
        "ab-1",
        "team-skills.zip",
        serde_json::json!({ "name": "team-skills" }),
        serde_json::Value::Array(items),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["failed"], serde_json::json!([]), "{report}");
    let mut ids = report["imported"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["skill"]["id"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    ids.sort();
    let mut expected = vec![tdd_id.clone(), grilling_id.clone()];
    expected.sort();
    assert_eq!(ids, expected, "ids preserved across instances");
    let detail = get_json(&b, &format!("/settings/skills/{tdd_id}")).await;
    assert_eq!(
        detail["files"],
        serde_json::json!([{ "path": "examples/login.spec.ts", "size": 14 }])
    );

    // The pipeline imported afterwards, without any sidecar, finds them.
    let resp = post_json(
        &b,
        "/pipelines/import",
        serde_json::json!({ "document": document }),
    )
    .await;
    let status = resp.status();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["warnings"], serde_json::json!([]), "{body}");
    assert_eq!(body["skills"]["missing"], serde_json::json!([]), "{body}");
}

// ---------------------------------------------------------------------------
// Known id, different content (pdo-issues#2, #1028, ADR-0084): keep / replace /
// move / duplicate, row by row
// ---------------------------------------------------------------------------

async fn create_folder(daemon: &TestDaemon, name: &str) -> String {
    let folder: serde_json::Value = post_json(
        daemon,
        "/settings/skill-folders",
        serde_json::json!({ "name": name }),
    )
    .await
    .json()
    .await
    .unwrap();
    folder["id"].as_str().unwrap().to_string()
}

async fn file_in_folder(daemon: &TestDaemon, id: &str, folder_id: &str) {
    let resp = put_json(
        daemon,
        &format!("/settings/skills/{id}"),
        serde_json::json!({ "folder_id": folder_id }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
}

async fn edit_text(daemon: &TestDaemon, id: &str, content: &str) {
    let resp = put_text(
        daemon,
        &format!("/settings/skills/{id}/files/SKILL.md"),
        content,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
}

fn edited_md_of(name: &str) -> String {
    format!("---\nname: {name}\ndescription: The {name} method.\n---\n\n# {name}\n\nEdited in the bank.\n")
}

/// FP: four skills of one folder exported, edited in the bank, the archive
/// dropped back → four `changed · in "ops"` rows; keep / replace / move /
/// duplicate each have their effect on content, files, name, folder and id.
#[tokio::test]
async fn fp1028_keep_replace_move_duplicate_each_have_their_effect_on_the_bank() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let ops = create_folder(&daemon, "ops").await;
    let mut ids = Vec::new();
    for name in ["kept-one", "replaced-one", "moved-one", "duplicated-one"] {
        let skill = create_named_skill(&daemon, name).await;
        let id = skill["id"].as_str().unwrap().to_string();
        file_in_folder(&daemon, &id, &ops).await;
        ids.push(id);
    }
    let (kept, replaced, moved, duplicated) = (&ids[0], &ids[1], &ids[2], &ids[3]);
    // A reference file that the replace must bring back.
    upload(
        &daemon,
        replaced,
        &[
            ("path", None, b"scripts/run.sh"),
            ("file", Some("run.sh"), b"#!/bin/sh\necho ok\n"),
        ],
    )
    .await;
    let id_refs: Vec<&str> = ids.iter().map(String::as_str).collect();
    let bytes = export_bytes(&daemon, &id_refs, "team-review").await;

    // Step 2: edit the four in the bank; drop the reference file.
    for (id, name) in ids
        .iter()
        .zip(["kept-one", "replaced-one", "moved-one", "duplicated-one"])
    {
        edit_text(&daemon, id, &edited_md_of(name)).await;
    }
    let resp = reqwest::Client::new()
        .delete(format!(
            "{}/settings/skills/{replaced}/files/scripts/run.sh",
            daemon.url()
        ))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "{}", resp.status());

    // Step 3: four rows `id_changed`, each pointing at its skill in « ops ».
    let (status, scan) = scan_archive(&daemon, "c-1", "team-review.zip", &bytes).await;
    assert_eq!(status, StatusCode::OK, "{scan}");
    for (id, name) in ids
        .iter()
        .zip(["kept-one", "replaced-one", "moved-one", "duplicated-one"])
    {
        let row = candidate(&scan, name);
        assert_eq!(row["status"], "id_changed", "{scan}");
        assert_eq!(row["existing"]["id"], id.as_str());
        assert_eq!(row["existing"]["folder_id"], ops.as_str());
        assert_eq!(row["existing"]["folder_name"], "ops");
    }

    // Step 4: keep / replace / move / duplicate.
    let (status, report) = import_archive(
        &daemon,
        "c-1",
        "team-review.zip",
        serde_json::json!({ "name": "team-review" }),
        serde_json::json!([
            { "path": "kept-one", "action": "keep" },
            { "path": "replaced-one", "action": "replace" },
            { "path": "moved-one", "action": "move" },
            { "path": "duplicated-one", "action": "duplicate" },
        ]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["failed"], serde_json::json!([]), "{report}");
    let target = report["folder"]["id"].as_str().unwrap().to_string();
    assert_eq!(report["folder"]["name"], "team-review");
    let rows = report["imported"].as_array().unwrap();
    assert_eq!(rows.len(), 3, "keep writes nothing: {report}");
    let row = |path: &str| rows.iter().find(|r| r["path"] == path).unwrap();
    assert_eq!(row("replaced-one")["action"], "replaced");
    assert_eq!(row("replaced-one")["skill"]["id"], replaced.as_str());
    assert_eq!(row("replaced-one")["skill"]["folder_id"], ops.as_str());
    assert!(row("replaced-one").get("warning").is_none(), "{report}");
    assert_eq!(row("moved-one")["action"], "moved");
    assert_eq!(row("moved-one")["skill"]["id"], moved.as_str());
    assert_eq!(row("moved-one")["skill"]["folder_id"], target.as_str());
    assert_eq!(row("moved-one")["previous_folder_id"], ops.as_str());
    assert_eq!(row("duplicated-one")["action"], "duplicated");
    let copy_id = row("duplicated-one")["skill"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_ne!(&copy_id, duplicated, "a duplicate gets a new id");
    assert_eq!(row("duplicated-one")["skill"]["name"], "duplicated-one-2");
    assert_eq!(row("duplicated-one")["skill"]["folder_id"], target.as_str());

    // Step 5: the bank.
    let detail = |id: String| {
        let daemon = &daemon;
        async move { get_json(daemon, &format!("/settings/skills/{id}")).await }
    };
    let k = detail(kept.clone()).await;
    assert_eq!(k["content"], edited_md_of("kept-one"), "kept as is");
    assert_eq!(k["folder_id"], ops.as_str());
    let r = detail(replaced.clone()).await;
    assert_eq!(
        r["content"],
        skill_md_of("replaced-one"),
        "exported text back"
    );
    assert_eq!(r["folder_id"], ops.as_str(), "replace stays in its folder");
    assert_eq!(
        r["files"],
        serde_json::json!([{ "path": "scripts/run.sh", "size": 18 }]),
        "reference files replaced too"
    );
    let m = detail(moved.clone()).await;
    assert_eq!(m["content"], skill_md_of("moved-one"));
    assert_eq!(
        m["folder_id"],
        target.as_str(),
        "move files it in the import folder"
    );
    let original = detail(duplicated.clone()).await;
    assert_eq!(
        original["content"],
        edited_md_of("duplicated-one"),
        "original intact"
    );
    assert_eq!(original["name"], "duplicated-one");
    assert_eq!(original["folder_id"], ops.as_str());
    let copy = detail(copy_id.clone()).await;
    assert_eq!(copy["content"], skill_md_of("duplicated-one"));
    assert_eq!(copy["name"], "duplicated-one-2");
}

/// Replace takes the archive's name only when it is free (else keeps the
/// current one and says so on the row); a typed duplicate name must be free;
/// keep alone writes nothing and leaves no empty folder behind.
#[tokio::test]
async fn fp1028_replace_takes_a_free_name_only_and_duplicate_honours_the_typed_name() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let alpha = create_named_skill(&daemon, "alpha").await;
    let beta = create_named_skill(&daemon, "beta").await;
    create_named_skill(&daemon, "taken-name").await;
    let alpha_id = alpha["id"].as_str().unwrap();
    let beta_id = beta["id"].as_str().unwrap();
    let manifest = serde_json::json!({
        "format": "pdo-skills",
        "version": 1,
        "exported_at": "2026-11-05T10:00:00Z",
        "skills": [
            { "folder": "alpha", "id": alpha_id, "name": "alpha-renamed" },
            { "folder": "beta", "id": beta_id, "name": "taken-name" },
        ],
    })
    .to_string();
    let bytes = zip_of(&[
        ("alpha/SKILL.md", skill_md_of("alpha-renamed").as_bytes()),
        ("beta/SKILL.md", skill_md_of("taken-name").as_bytes()),
        ("pdo-skills.json", manifest.as_bytes()),
    ]);

    // Keep only: nothing written, no folder left behind.
    let (_, scan) = scan_archive(&daemon, "n-1", "names.zip", &bytes).await;
    assert_eq!(candidate(&scan, "alpha-renamed")["status"], "id_changed");
    assert_eq!(candidate(&scan, "taken-name")["status"], "id_changed");
    let (status, report) = import_archive(
        &daemon,
        "n-1",
        "names.zip",
        serde_json::json!({ "name": "names" }),
        serde_json::json!([
            { "path": "alpha", "action": "keep" },
            { "path": "beta", "action": "keep" },
        ]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["imported"], serde_json::json!([]));
    let folders = get_json(&daemon, "/settings/skill-folders").await;
    assert!(
        !folders.to_string().contains("\"names\""),
        "no empty folder: {folders}"
    );

    // Replace: alpha takes its free new name; beta keeps « beta ».
    let (status, report) = import_archive(
        &daemon,
        "n-1",
        "names.zip",
        serde_json::json!({ "name": "names" }),
        serde_json::json!([
            { "path": "alpha", "action": "replace" },
            { "path": "beta", "action": "replace" },
        ]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{report}");
    let rows = report["imported"].as_array().unwrap();
    let row = |path: &str| rows.iter().find(|r| r["path"] == path).unwrap();
    assert_eq!(row("alpha")["skill"]["id"], alpha_id);
    assert_eq!(row("alpha")["skill"]["name"], "alpha-renamed");
    assert!(row("alpha").get("warning").is_none());
    assert_eq!(row("beta")["skill"]["id"], beta_id);
    assert_eq!(row("beta")["skill"]["name"], "beta");
    let warning = row("beta")["warning"].as_str().unwrap();
    assert!(warning.contains("taken-name"), "{warning}");
    let detail = get_json(&daemon, &format!("/settings/skills/{beta_id}")).await;
    assert_eq!(detail["content"], skill_md_of("taken-name"));

    // Duplicate with a typed name: taken → failed row; free → used.
    let (_, _) = scan_archive(&daemon, "n-2", "names.zip", &bytes).await;
    let (status, report) = import_archive(
        &daemon,
        "n-2",
        "names.zip",
        serde_json::json!({ "name": "names" }),
        serde_json::json!([
            { "path": "alpha", "action": "duplicate", "name": "taken-name" },
            { "path": "beta", "action": "duplicate", "name": "beta-copy" },
        ]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["failed"][0]["path"], "alpha", "{report}");
    assert_eq!(report["failed"][0]["code"], "duplicate_name");
    assert_eq!(report["imported"][0]["skill"]["name"], "beta-copy");
    assert_ne!(report["imported"][0]["skill"]["id"], beta_id);
}
