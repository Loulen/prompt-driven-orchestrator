//! Layer 3a — skills of the bank travel as a zip: export a skill, a selection
//! or a folder (`POST /settings/skills/export`), upload the zip into another
//! bank (`POST /settings/skills/upload`).
//!
//! Two real daemons over two tempdirs: the round trip is the ticket's FP —
//! export a folder from one bank, upload it into a bank that lacks it, and the
//! skills are back with their ids, their reference files and a folder named
//! after the zip.

use std::io::Read;

use crate::common::TestDaemon;
use reqwest::StatusCode;

fn skill_md(name: &str) -> String {
    format!(
        "---\nname: {name}\ndescription: The {name} method.\n---\n\n# {name}\n\nBody of {name}.\n"
    )
}

async fn post(daemon: &TestDaemon, path: &str, body: serde_json::Value) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{}{path}", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap()
}

async fn create_folder(daemon: &TestDaemon, name: &str, parent: Option<&str>) -> String {
    let resp = post(
        daemon,
        "/settings/skill-folders",
        serde_json::json!({ "name": name, "parent_id": parent }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED, "folder `{name}`");
    let folder: serde_json::Value = resp.json().await.unwrap();
    folder["id"].as_str().unwrap().to_string()
}

async fn create_skill(daemon: &TestDaemon, name: &str, folder: Option<&str>) -> String {
    let resp = post(
        daemon,
        "/settings/skills",
        serde_json::json!({ "content": skill_md(name), "folder_id": folder }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED, "skill `{name}`");
    let skill: serde_json::Value = resp.json().await.unwrap();
    skill["id"].as_str().unwrap().to_string()
}

async fn export(daemon: &TestDaemon, body: serde_json::Value) -> reqwest::Response {
    post(daemon, "/settings/skills/export", body).await
}

async fn upload(daemon: &TestDaemon, file_name: &str, bytes: Vec<u8>) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{}/settings/skills/upload", daemon.url()))
        .query(&[("name", file_name)])
        .header("content-type", "application/zip")
        .body(bytes)
        .send()
        .await
        .unwrap()
}

async fn bank(daemon: &TestDaemon) -> serde_json::Value {
    reqwest::get(format!("{}/settings/skills", daemon.url()))
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

fn zip_names(bytes: &[u8]) -> Vec<String> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut names = (0..archive.len())
        .map(|i| archive.by_index(i).unwrap().name().to_string())
        .collect::<Vec<_>>();
    names.sort();
    names
}

fn zip_entry(bytes: &[u8], name: &str) -> String {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut entry = archive.by_name(name).unwrap();
    let mut text = String::new();
    entry.read_to_string(&mut text).unwrap();
    text
}

fn write_reference(daemon: &TestDaemon, id: &str, rel: &str, text: &str) {
    let path = daemon
        .repo_root()
        .join(".pdo")
        .join("skills")
        .join(id)
        .join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// FP step 4: export a folder (its sub-folders included), upload the zip into a
/// bank that lacks the skills — they come back with their ids, their reference
/// files, under a folder named after the zip. A second upload keeps them.
#[tokio::test]
async fn a_folder_exported_then_uploaded_into_another_bank_comes_back_whole() {
    let source = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let team = create_folder(&source, "Team", None).await;
    let nested = create_folder(&source, "Nested", Some(&team)).await;
    let tdd = create_skill(&source, "tdd", Some(&team)).await;
    let review = create_skill(&source, "review-checklist", Some(&nested)).await;
    let outside = create_skill(&source, "release-notes", None).await;
    write_reference(&source, &tdd, "examples/login.spec.ts", "it('logs in')\n");

    let resp = export(&source, serde_json::json!({ "folder_ids": [team] })).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers()["content-type"], "application/zip");
    assert!(resp.headers()["content-disposition"]
        .to_str()
        .unwrap()
        .contains("filename=\"Team.skills.zip\""));
    assert_eq!(resp.headers()["x-pdo-skills"], "2");
    let bytes = resp.bytes().await.unwrap().to_vec();
    let mut expected = vec![
        format!("Team.skills/{tdd}/SKILL.md"),
        format!("Team.skills/{tdd}/examples/login.spec.ts"),
        format!("Team.skills/{review}/SKILL.md"),
    ];
    expected.sort();
    assert_eq!(zip_names(&bytes), expected);
    assert!(!zip_names(&bytes).iter().any(|n| n.contains(&outside)));
    assert_eq!(
        zip_entry(&bytes, &format!("Team.skills/{tdd}/SKILL.md")),
        skill_md("tdd")
    );

    let target = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let resp = upload(&target, "Team.skills.zip", bytes.clone()).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let report: serde_json::Value = resp.json().await.unwrap();
    let mut created = report["created"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    created.sort();
    let mut ids = vec![tdd.clone(), review.clone()];
    ids.sort();
    assert_eq!(created, ids);
    assert_eq!(report["folder"]["name"], "Team");
    assert_eq!(report["warnings"], serde_json::json!([]));

    let after = bank(&target).await;
    let folder_id = report["folder"]["id"].as_str().unwrap();
    for id in [&tdd, &review] {
        let skill = after["skills"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["id"] == id.as_str())
            .unwrap_or_else(|| panic!("skill {id} is back"));
        assert_eq!(skill["folder_id"], folder_id);
    }
    let reference = target
        .repo_root()
        .join(".pdo/skills")
        .join(&tdd)
        .join("examples/login.spec.ts");
    assert_eq!(
        std::fs::read_to_string(reference).unwrap(),
        "it('logs in')\n"
    );

    // The same zip again: the bank's copies win, nothing is created.
    let report: serde_json::Value = upload(&target, "Team.skills.zip", bytes)
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(report["created"], serde_json::json!([]));
    assert_eq!(report["kept"].as_array().unwrap().len(), 2);
}

/// One skill names the zip after itself; a selection of skills and folders is
/// `selection.skills.zip`, each skill once.
#[tokio::test]
async fn a_skill_or_a_selection_exports_with_its_reference_files() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let folder = create_folder(&daemon, "Team", None).await;
    let tdd = create_skill(&daemon, "tdd", Some(&folder)).await;
    let notes = create_skill(&daemon, "release-notes", None).await;
    write_reference(&daemon, &notes, "template.md", "# Notes\n");

    let resp = export(&daemon, serde_json::json!({ "skill_ids": [notes] })).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(resp.headers()["content-disposition"]
        .to_str()
        .unwrap()
        .contains("filename=\"release-notes.skills.zip\""));
    let bytes = resp.bytes().await.unwrap();
    assert_eq!(
        zip_names(&bytes),
        vec![
            format!("release-notes.skills/{notes}/SKILL.md"),
            format!("release-notes.skills/{notes}/template.md"),
        ]
    );

    let resp = export(
        &daemon,
        serde_json::json!({ "skill_ids": [tdd, notes], "folder_ids": [folder] }),
    )
    .await;
    assert_eq!(resp.headers()["x-pdo-skills"], "2");
    let names = zip_names(&resp.bytes().await.unwrap());
    assert!(
        names.iter().all(|n| n.starts_with("selection.skills/")),
        "{names:?}"
    );
    assert_eq!(names.len(), 3, "{names:?}");
}

/// Nothing to export, an unknown id, a zip that is not one or holds no skill:
/// each a 400/404 with its code, nothing written.
#[tokio::test]
async fn refusals_name_their_reason() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let empty = create_folder(&daemon, "Empty", None).await;

    let resp = export(&daemon, serde_json::json!({ "folder_ids": [empty] })).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["code"], "nothing_to_export");

    let resp = export(&daemon, serde_json::json!({ "skill_ids": ["nope"] })).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    let resp = upload(&daemon, "x.zip", b"not a zip".to_vec()).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["code"], "invalid_archive");

    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    zip.start_file("readme.txt", zip::write::SimpleFileOptions::default())
        .unwrap();
    std::io::Write::write_all(&mut zip, b"no skill here").unwrap();
    let bytes = zip.finish().unwrap().into_inner();
    let resp = upload(&daemon, "x.zip", bytes).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["code"], "invalid_archive");
}
