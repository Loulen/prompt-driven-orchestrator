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
// #669 — selection by tier, skills effectifs with origin, referents populated.
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
    let effective = &payload["skills"];
    assert_eq!(
        skill_ids(effective),
        vec![
            a["id"].as_str().unwrap(),
            b["id"].as_str().unwrap(),
            d["id"].as_str().unwrap(),
            c["id"].as_str().unwrap()
        ],
        "union, coarsest tier first, de-duplicated: {payload}"
    );
    assert_eq!(
        tiers_of(effective, &a["id"]),
        serde_json::json!(["instance", "project"])
    );
    assert_eq!(
        tiers_of(effective, &b["id"]),
        serde_json::json!(["project"])
    );
    assert_eq!(tiers_of(effective, &d["id"]), serde_json::json!(["run"]));
    assert_eq!(tiers_of(effective, &c["id"]), serde_json::json!(["node"]));
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
