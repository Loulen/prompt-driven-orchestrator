//! Layer 3a — skills travel with the portable document (#673, ADR-0062 "Voyage
//! par document").
//!
//! A real daemon over a tempdir; every assertion is an HTTP response, a zip
//! entry, or a folder under `<repo_root>/.pdo/skills/<id>/`. The round-trip is
//! the ticket's FP: export a pipeline whose node carries two skills → delete them
//! from the bank → import the document → the skills are back **with the same
//! ids**, so the pipeline's references resolve and no warning remains.

use std::io::Read;

use crate::common::TestDaemon;
use reqwest::StatusCode;

const PIPELINE: &str = "with-skills";

fn skills_root(daemon: &TestDaemon) -> std::path::PathBuf {
    daemon.repo_root().join(".pdo").join("skills")
}

fn pipeline_yaml(node_skills: &str) -> String {
    format!(
        r#"name: {PIPELINE}
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: worker
    name: Worker
    type: agent
    isolated_worktree: false
{node_skills}    outputs:
      - name: out
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: {{ node: start, port: user_prompt }}
    target: {{ node: worker, port: task }}
  - source: {{ node: worker, port: out }}
    target: {{ node: end, port: result }}
"#
    )
}

fn seed(repo: &std::path::Path) -> anyhow::Result<()> {
    let dir = repo.join(".pdo").join("pipelines");
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join(format!("{PIPELINE}.yaml")), pipeline_yaml(""))?;
    let prompts = dir.join(format!("{PIPELINE}.prompts"));
    std::fs::create_dir_all(&prompts)?;
    std::fs::write(prompts.join("worker.md"), "You are the worker.\n")?;
    // A git repo, so the Run of the last test has a target to fork from.
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

fn skill_md(name: &str) -> String {
    format!(
        "---\nname: {name}\ndescription: The {name} method.\n---\n\n# {name}\n\nBody of {name}.\n"
    )
}

async fn create_skill(daemon: &TestDaemon, name: &str) -> serde_json::Value {
    let resp = reqwest::Client::new()
        .post(format!("{}/settings/skills", daemon.url()))
        .json(&serde_json::json!({ "content": skill_md(name) }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED, "skill `{name}`");
    resp.json().await.unwrap()
}

fn node_skills_yaml(skills: &[&serde_json::Value]) -> String {
    let mut out = String::from("    skills:\n");
    for skill in skills {
        out.push_str(&format!(
            "      - id: {}\n        name: {}\n",
            skill["id"].as_str().unwrap(),
            skill["name"].as_str().unwrap()
        ));
    }
    out
}

async fn put_pipeline(daemon: &TestDaemon, node_skills: &str) {
    let resp = reqwest::Client::new()
        .put(format!("{}/pipelines/{PIPELINE}", daemon.url()))
        .json(&serde_json::json!({ "yaml": pipeline_yaml(node_skills), "prompts": {} }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

async fn get_text(daemon: &TestDaemon, path: &str) -> String {
    reqwest::get(format!("{}{path}", daemon.url()))
        .await
        .unwrap()
        .text()
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

async fn get_sidecar(daemon: &TestDaemon, path: &str) -> reqwest::Response {
    reqwest::get(format!("{}{path}", daemon.url()))
        .await
        .unwrap()
}

async fn import(
    daemon: &TestDaemon,
    document: &str,
    sidecar: Option<&[u8]>,
) -> (StatusCode, serde_json::Value) {
    use base64::Engine as _;
    let mut body = serde_json::json!({ "document": document });
    if let Some(bytes) = sidecar {
        body["skills_sidecar"] =
            serde_json::Value::String(base64::engine::general_purpose::STANDARD.encode(bytes));
    }
    let resp = reqwest::Client::new()
        .post(format!("{}/pipelines/import", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = resp.status();
    (status, resp.json().await.unwrap())
}

fn zip_entries(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
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

fn skills_of_node(document: &serde_json::Value, node_id: &str) -> Vec<(String, String)> {
    document["pipeline"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == node_id)
        .and_then(|n| n["skills"].as_array())
        .map(|skills| {
            skills
                .iter()
                .map(|s| {
                    (
                        s["id"].as_str().unwrap().to_string(),
                        s["name"].as_str().unwrap_or("").to_string(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// FP step 1: the export is the YAML (unchanged but for the `skills` field) plus
/// a sidecar `<pipeline>.skills/<id>/…` holding `SKILL.md` and the reference
/// files of each skill the nodes select.
#[tokio::test]
async fn fp_step_1_export_ships_the_yaml_and_a_sidecar_per_referenced_skill() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let tdd = create_skill(&daemon, "tdd").await;
    let grilling = create_skill(&daemon, "grilling").await;
    let tdd_id = tdd["id"].as_str().unwrap();
    // A reference file, on disk under the skill's folder (#671).
    let examples = skills_root(&daemon).join(tdd_id).join("examples");
    std::fs::create_dir_all(&examples).unwrap();
    std::fs::write(examples.join("login.spec.ts"), "it('logs in')\n").unwrap();
    put_pipeline(&daemon, &node_skills_yaml(&[&tdd, &grilling])).await;

    // The YAML: a portable document whose node still names both skills by id.
    let document = get_text(&daemon, &format!("/pipelines/{PIPELINE}/document")).await;
    assert!(document.starts_with("pdo_pipeline: 1"), "{document}");
    assert!(document.contains(tdd_id), "{document}");
    assert!(
        document.contains(grilling["id"].as_str().unwrap()),
        "{document}"
    );
    assert!(document.contains("name: tdd"), "{document}");
    assert!(!document.contains("skills_sidecar"), "{document}");

    // The sidecar: one zip, `<pipeline>.skills/<id>/…`.
    let resp = get_sidecar(&daemon, &format!("/pipelines/{PIPELINE}/document/skills")).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()["content-type"].to_str().unwrap(),
        "application/zip"
    );
    assert!(resp.headers()["content-disposition"]
        .to_str()
        .unwrap()
        .contains(&format!("{PIPELINE}.skills.zip")));
    assert_eq!(resp.headers()["x-pdo-skills"].to_str().unwrap(), "2");
    let bytes = resp.bytes().await.unwrap();
    let mut names = zip_entries(&bytes)
        .into_iter()
        .map(|(name, _)| name)
        .collect::<Vec<_>>();
    names.sort();
    let mut expected = vec![
        format!("{PIPELINE}.skills/{tdd_id}/SKILL.md"),
        format!("{PIPELINE}.skills/{tdd_id}/examples/login.spec.ts"),
        format!(
            "{PIPELINE}.skills/{}/SKILL.md",
            grilling["id"].as_str().unwrap()
        ),
    ];
    expected.sort();
    assert_eq!(names, expected);
    let content = zip_entries(&bytes)
        .into_iter()
        .find(|(name, _)| name.ends_with(&format!("{tdd_id}/SKILL.md")))
        .unwrap()
        .1;
    assert_eq!(String::from_utf8(content).unwrap(), skill_md("tdd"));
}

/// A pipeline without skills has no sidecar to ship: 204, the YAML is the
/// whole document.
#[tokio::test]
async fn a_pipeline_without_skills_has_no_sidecar() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let resp = get_sidecar(&daemon, &format!("/pipelines/{PIPELINE}/document/skills")).await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    let resp = get_sidecar(&daemon, "/pipelines/nope/document/skills").await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

/// FP steps 2–3, the round-trip of the AC: export → delete the skills → import
/// → the skills are recreated **with the same ids**, in a folder named after the
/// pipeline, and the import carries no warning.
#[tokio::test]
async fn fp_round_trip_recreates_deleted_skills_with_the_same_ids_and_no_warning() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let tdd = create_skill(&daemon, "tdd").await;
    let grilling = create_skill(&daemon, "grilling").await;
    let tdd_id = tdd["id"].as_str().unwrap().to_string();
    let grilling_id = grilling["id"].as_str().unwrap().to_string();
    let examples = skills_root(&daemon).join(&tdd_id).join("examples");
    std::fs::create_dir_all(&examples).unwrap();
    std::fs::write(examples.join("login.spec.ts"), "it('logs in')\n").unwrap();
    put_pipeline(&daemon, &node_skills_yaml(&[&tdd, &grilling])).await;

    let document = get_text(&daemon, &format!("/pipelines/{PIPELINE}/document")).await;
    let sidecar = get_sidecar(&daemon, &format!("/pipelines/{PIPELINE}/document/skills"))
        .await
        .bytes()
        .await
        .unwrap();

    // FP step 2: the skills leave the bank — folders gone, referents keep the id.
    let client = reqwest::Client::new();
    for id in [&tdd_id, &grilling_id] {
        let resp = client
            .delete(format!("{}/settings/skills/{id}", daemon.url()))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        assert!(!skills_root(&daemon).join(id).exists());
    }
    assert_eq!(
        get_json(&daemon, "/settings/skills").await["skills"],
        serde_json::json!([])
    );

    // FP step 3: import the document with its sidecar.
    let (status, body) = import(&daemon, &document, Some(&sidecar)).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["warnings"], serde_json::json!([]), "{body}");
    let mut created = body["skills"]["created"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    created.sort();
    let mut expected = vec![tdd_id.clone(), grilling_id.clone()];
    expected.sort();
    assert_eq!(created, expected, "{body}");
    assert_eq!(body["skills"]["kept"], serde_json::json!([]));
    assert_eq!(body["skills"]["missing"], serde_json::json!([]));
    assert_eq!(body["skills"]["renamed"], serde_json::json!([]));
    assert_eq!(
        body["skills"]["folder"]["name"],
        format!("importés avec {PIPELINE}")
    );

    // The bank has both back, same ids, same content, reference file included,
    // filed under the import folder.
    let bank = get_json(&daemon, "/settings/skills").await;
    let skills = bank["skills"].as_array().unwrap();
    assert_eq!(skills.len(), 2, "{bank}");
    let folder_id = body["skills"]["folder"]["id"].as_str().unwrap();
    for skill in skills {
        assert_eq!(skill["folder_id"], folder_id, "{skill}");
    }
    assert_eq!(
        bank["folders"][0]["name"],
        format!("importés avec {PIPELINE}")
    );
    assert_eq!(
        std::fs::read_to_string(skills_root(&daemon).join(&tdd_id).join("SKILL.md")).unwrap(),
        skill_md("tdd")
    );
    assert_eq!(
        std::fs::read_to_string(
            skills_root(&daemon)
                .join(&tdd_id)
                .join("examples")
                .join("login.spec.ts")
        )
        .unwrap(),
        "it('logs in')\n"
    );
    let detail = get_json(&daemon, &format!("/settings/skills/{tdd_id}")).await;
    assert_eq!(detail["name"], "tdd");
    assert_eq!(detail["files"][0]["path"], "examples/login.spec.ts");

    // The imported pipeline references the same ids, and is launchable: its
    // referents list both the original and the imported pipeline's node.
    let imported_id = body["id"].as_str().unwrap();
    let imported = get_json(&daemon, &format!("/pipelines/{imported_id}")).await;
    let mut refs = skills_of_node(&imported, "worker");
    refs.sort();
    let mut expected_refs = vec![
        (tdd_id.clone(), "tdd".to_string()),
        (grilling_id.clone(), "grilling".to_string()),
    ];
    expected_refs.sort();
    assert_eq!(refs, expected_refs, "{imported}");
    let referents = get_json(&daemon, &format!("/settings/skills/{tdd_id}/referents")).await;
    let pipelines = referents["pipelines"].as_array().unwrap();
    assert!(
        pipelines.iter().any(|p| p["id"] == imported_id),
        "{referents}"
    );

    // Exporting the imported pipeline yields the same sidecar (same ids, same
    // bytes): the document seam is stable.
    let again = get_sidecar(
        &daemon,
        &format!("/pipelines/{imported_id}/document/skills"),
    )
    .await
    .bytes()
    .await
    .unwrap();
    let strip = |bytes: &[u8]| {
        let mut entries = zip_entries(bytes)
            .into_iter()
            .map(|(name, data)| (name.split_once('/').unwrap().1.to_string(), data))
            .collect::<Vec<_>>();
        entries.sort();
        entries
    };
    assert_eq!(strip(&sidecar), strip(&again));
}

/// A known id is left as it is (the bank's copy wins), and a label already used
/// by **another** id is suffixed — with a warning the UI shows.
#[tokio::test]
async fn known_ids_are_kept_and_taken_names_are_suffixed_with_a_warning() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let tdd = create_skill(&daemon, "tdd").await;
    let grilling = create_skill(&daemon, "grilling").await;
    let tdd_id = tdd["id"].as_str().unwrap().to_string();
    let grilling_id = grilling["id"].as_str().unwrap().to_string();
    put_pipeline(&daemon, &node_skills_yaml(&[&tdd, &grilling])).await;
    let document = get_text(&daemon, &format!("/pipelines/{PIPELINE}/document")).await;
    let sidecar = get_sidecar(&daemon, &format!("/pipelines/{PIPELINE}/document/skills"))
        .await
        .bytes()
        .await
        .unwrap();

    // Rename the bank's `tdd` (same id, new label) so the import sees a KNOWN id;
    // delete `grilling`, then create an unrelated skill that takes its name.
    let client = reqwest::Client::new();
    let resp = client
        .put(format!("{}/settings/skills/{tdd_id}", daemon.url()))
        .json(&serde_json::json!({ "name": "tdd-renamed" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let resp = client
        .delete(format!("{}/settings/skills/{grilling_id}", daemon.url()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    let squatter = create_skill(&daemon, "grilling").await;
    let squatter_id = squatter["id"].as_str().unwrap().to_string();
    assert_ne!(squatter_id, grilling_id);

    let (status, body) = import(&daemon, &document, Some(&sidecar)).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    // Known id: kept, untouched (label stays the renamed one; disk untouched).
    assert_eq!(body["skills"]["kept"][0]["id"], tdd_id, "{body}");
    assert_eq!(body["skills"]["kept"][0]["name"], "tdd-renamed");
    let detail = get_json(&daemon, &format!("/settings/skills/{tdd_id}")).await;
    assert_eq!(detail["name"], "tdd-renamed");
    assert_eq!(detail["content"], skill_md("tdd"));

    // Taken name: created with the SAME id, suffixed label, one warning.
    assert_eq!(body["skills"]["created"][0]["id"], grilling_id, "{body}");
    assert_eq!(body["skills"]["created"][0]["name"], "grilling-2");
    assert_eq!(body["skills"]["renamed"][0]["from"], "grilling");
    assert_eq!(body["skills"]["renamed"][0]["to"], "grilling-2");
    let warnings = body["warnings"].as_array().unwrap();
    assert_eq!(warnings.len(), 1, "{body}");
    assert!(
        warnings[0]
            .as_str()
            .unwrap()
            .contains(&format!("skills.{grilling_id}")),
        "{body}"
    );
    assert!(
        warnings[0].as_str().unwrap().contains("grilling-2"),
        "{body}"
    );
    let squatter_detail = get_json(&daemon, &format!("/settings/skills/{squatter_id}")).await;
    assert_eq!(squatter_detail["name"], "grilling");

    // The imported pipeline's references carry the bank's labels.
    let imported = get_json(
        &daemon,
        &format!("/pipelines/{}", body["id"].as_str().unwrap()),
    )
    .await;
    let mut refs = skills_of_node(&imported, "worker");
    refs.sort();
    let mut expected = vec![
        (tdd_id, "tdd-renamed".to_string()),
        (grilling_id, "grilling-2".to_string()),
    ];
    expected.sort();
    assert_eq!(refs, expected, "{imported}");
    // The bank: three skills, no overwrite.
    assert_eq!(
        get_json(&daemon, "/settings/skills").await["skills"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
}

/// A document with unknown ids and **no sidecar** imports with the "skill
/// absent" warning — never a failure — and the node keeps its references.
#[tokio::test]
async fn a_document_without_sidecar_imports_with_a_skill_absent_warning() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let tdd = create_skill(&daemon, "tdd").await;
    let tdd_id = tdd["id"].as_str().unwrap().to_string();
    put_pipeline(&daemon, &node_skills_yaml(&[&tdd])).await;
    let document = get_text(&daemon, &format!("/pipelines/{PIPELINE}/document")).await;
    let resp = reqwest::Client::new()
        .delete(format!("{}/settings/skills/{tdd_id}", daemon.url()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    let (status, body) = import(&daemon, &document, None).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let warnings = body["warnings"].as_array().unwrap();
    assert_eq!(warnings.len(), 1, "{body}");
    let warning = warnings[0].as_str().unwrap();
    assert!(warning.contains(&format!("skills.{tdd_id}")), "{warning}");
    assert!(warning.contains("absent"), "{warning}");
    assert_eq!(body["skills"]["missing"][0]["id"], tdd_id);
    assert!(body["skills"].get("folder").is_none(), "{body}");
    // Nothing created, no folder, the reference is kept for the day the skill arrives.
    let bank = get_json(&daemon, "/settings/skills").await;
    assert_eq!(bank["skills"], serde_json::json!([]));
    assert_eq!(bank["folders"], serde_json::json!([]));
    let imported = get_json(
        &daemon,
        &format!("/pipelines/{}", body["id"].as_str().unwrap()),
    )
    .await;
    assert_eq!(
        skills_of_node(&imported, "worker"),
        vec![(tdd_id, "tdd".to_string())]
    );
}

/// A sidecar that is not a zip is a 400 naming the field; a sidecar whose
/// `SKILL.md` fails the five checks is a warning, not a failure.
#[tokio::test]
async fn a_broken_sidecar_is_a_400_and_an_invalid_skill_md_is_a_warning() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let tdd = create_skill(&daemon, "tdd").await;
    let tdd_id = tdd["id"].as_str().unwrap().to_string();
    put_pipeline(&daemon, &node_skills_yaml(&[&tdd])).await;
    let document = get_text(&daemon, &format!("/pipelines/{PIPELINE}/document")).await;

    let (status, body) = import(&daemon, &document, Some(b"definitely not a zip")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body["error"].as_str().unwrap().contains("skills_sidecar"),
        "{body}"
    );

    // Hand-made sidecar, `<id>/SKILL.md` at the root (a user re-zipped the
    // folder), but without a description.
    let bytes = {
        use std::io::Write as _;
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        zip.start_file(
            format!("{tdd_id}/SKILL.md"),
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(b"---\nname: tdd\n---\n\nBody.\n").unwrap();
        zip.finish().unwrap().into_inner()
    };
    reqwest::Client::new()
        .delete(format!("{}/settings/skills/{tdd_id}", daemon.url()))
        .send()
        .await
        .unwrap();
    let (status, body) = import(&daemon, &document, Some(&bytes)).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let warning = body["warnings"][0].as_str().unwrap();
    assert!(warning.contains("description"), "{warning}");
    assert_eq!(body["skills"]["missing"][0]["id"], tdd_id);
    assert!(!skills_root(&daemon).join(&tdd_id).exists());
}

/// The Run's document has the same sidecar seam.
#[tokio::test]
async fn a_run_document_has_a_skills_sidecar_too() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let tdd = create_skill(&daemon, "tdd").await;
    put_pipeline(&daemon, &node_skills_yaml(&[&tdd])).await;
    let run: serde_json::Value = reqwest::Client::new()
        .post(format!("{}/runs", daemon.url()))
        .json(&serde_json::json!({
            "pipeline": PIPELINE,
            "input": "go",
            "target_repo": daemon.target_repo(),
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let run_id = run["run_id"]
        .as_str()
        .unwrap_or_else(|| panic!("no run id in {run}"));
    let resp = get_sidecar(&daemon, &format!("/runs/{run_id}/pipeline/document/skills")).await;
    assert_eq!(resp.status(), StatusCode::OK, "{run}");
    let names = zip_entries(&resp.bytes().await.unwrap())
        .into_iter()
        .map(|(name, _)| name)
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        vec![format!(
            "{PIPELINE}.skills/{}/SKILL.md",
            tdd["id"].as_str().unwrap()
        )]
    );
}
