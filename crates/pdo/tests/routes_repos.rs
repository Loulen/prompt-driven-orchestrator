//! Layer 4 (ADR-0080) — the repository routes no integration test called before
//! the refactor net (#914): scaffold a repository, fast-forward a branch onto its
//! upstream, and preview a provisioning plan. Real git, real temporary repos.

use std::path::Path;

use crate::common::flow;
use crate::common::TestDaemon;

fn seed(repo: &Path) -> anyhow::Result<()> {
    flow::git_init(repo)
}

// --- GET /repos/remote ---------------------------------------------------------

#[tokio::test]
async fn remote_shows_the_url_of_origin_without_its_credentials() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let repo = tempfile::tempdir().unwrap();
    flow::git_init(repo.path()).unwrap();
    let path = repo.path().to_string_lossy().to_string();

    let (status, body) = remote(&daemon, &path).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body, serde_json::json!({ "remote": null }));

    flow::git(
        repo.path(),
        &[
            "remote",
            "add",
            "origin",
            "https://me:tok@example.com/acme/app.git",
        ],
    );
    let (status, body) = remote(&daemon, &path).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        body,
        serde_json::json!({ "remote": "https://example.com/acme/app.git" })
    );

    let (status, _) = remote(&daemon, "relative/dir").await;
    assert_eq!(status, 400);
}

async fn remote(daemon: &TestDaemon, path: &str) -> (u16, serde_json::Value) {
    let resp = reqwest::get(format!(
        "{}/repos/remote?path={}",
        daemon.url(),
        urlencode(path)
    ))
    .await
    .unwrap();
    let status = resp.status().as_u16();
    (status, resp.json().await.unwrap_or_default())
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

// --- POST /repos/create --------------------------------------------------------

#[tokio::test]
async fn create_repo_initialises_main_with_the_given_files_then_reuses_it() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let parent = tempfile::tempdir().unwrap();
    let parent_path = parent.path().to_str().unwrap();

    let (status, body) = flow::post_json(
        &daemon,
        "/repos/create",
        serde_json::json!({
            "parent": parent_path,
            "name": "scaffolded",
            "files": [
                { "path": "README.md", "content": "# Scaffolded\n" },
                { "path": "src/main.rs", "content": "fn main() {}\n" },
            ],
        }),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let target = parent.path().join("scaffolded");
    assert_eq!(
        body,
        serde_json::json!({ "path": target.to_string_lossy(), "created": true })
    );
    assert_eq!(flow::git(&target, &["branch", "--show-current"]), "main");
    assert_eq!(
        flow::git(&target, &["ls-files"]),
        "README.md\nsrc/main.rs",
        "both files are in the initial commit"
    );

    let (status, body) = flow::post_json(
        &daemon,
        "/repos/create",
        serde_json::json!({ "parent": parent_path, "name": "scaffolded" }),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["created"], false, "an existing repo is reused: {body}");
}

#[tokio::test]
async fn create_repo_refuses_what_it_cannot_create() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let parent = tempfile::tempdir().unwrap();
    let parent_path = parent.path().to_str().unwrap();
    std::fs::create_dir(parent.path().join("occupied")).unwrap();
    std::fs::write(parent.path().join("occupied/notes.txt"), "mine").unwrap();

    for (request, error) in [
        (
            serde_json::json!({ "parent": "relative/dir", "name": "x" }),
            "parent must be an absolute path: relative/dir",
        ),
        (
            serde_json::json!({ "parent": parent_path, "name": "a/b" }),
            "name must be a single folder name: a/b",
        ),
        (
            serde_json::json!({ "parent": parent_path, "name": "  " }),
            "name must not be empty",
        ),
    ] {
        let (status, body) = flow::post_json(&daemon, "/repos/create", request).await;
        assert_eq!(status, 400, "{body}");
        assert_eq!(body["error"], error);
    }

    let (status, body) = flow::post_json(
        &daemon,
        "/repos/create",
        serde_json::json!({ "parent": parent_path, "name": "occupied" }),
    )
    .await;
    assert_eq!(
        status, 400,
        "a non-empty non-repo folder is refused: {body}"
    );
    assert!(!parent.path().join("occupied/.git").exists());
}

// --- POST /repos/fast-forward -----------------------------------------------------

/// `origin` (bare) ← `work` (clone tracking `origin/main`), then a second clone
/// pushes `ahead` commits and `work` fetches them: `main` is behind by `ahead`.
fn behind_clone(root: &Path, ahead: usize) -> std::path::PathBuf {
    let seed = root.join("seed");
    flow::git_init(&seed).unwrap();
    let origin = root.join("origin.git");
    flow::git(
        root,
        &[
            "clone",
            "-q",
            "--bare",
            seed.to_str().unwrap(),
            origin.to_str().unwrap(),
        ],
    );
    let work = root.join("work");
    let other = root.join("other");
    for clone in [&work, &other] {
        flow::git(
            root,
            &[
                "clone",
                "-q",
                origin.to_str().unwrap(),
                clone.to_str().unwrap(),
            ],
        );
        flow::git(clone, &["config", "user.email", "test@example.com"]);
        flow::git(clone, &["config", "user.name", "Test"]);
        flow::git(clone, &["config", "commit.gpgsign", "false"]);
    }
    for i in 0..ahead {
        flow::git(
            &other,
            &[
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                &format!("upstream {i}"),
            ],
        );
    }
    flow::git(&other, &["push", "-q", "origin", "main"]);
    flow::git(&work, &["fetch", "-q", "origin"]);
    work
}

async fn fast_forward(daemon: &TestDaemon, repo: &Path, branch: &str) -> (u16, serde_json::Value) {
    let resp = reqwest::Client::new()
        .post(format!("{}/repos/fast-forward", daemon.url()))
        .query(&[("path", repo.to_str().unwrap())])
        .json(&serde_json::json!({ "branch": branch }))
        .send()
        .await
        .unwrap();
    let status = resp.status().as_u16();
    (status, resp.json().await.unwrap())
}

#[tokio::test]
async fn fast_forward_moves_the_checked_out_branch_onto_its_upstream() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let root = tempfile::tempdir().unwrap();
    let work = behind_clone(root.path(), 2);
    let before = flow::git(&work, &["rev-parse", "--short", "main"]);

    let (status, body) = fast_forward(&daemon, &work, "main").await;
    assert_eq!(status, 200, "{body}");
    let ff = &body["fast_forward"];
    assert_eq!(ff["branch"], "main");
    assert_eq!(ff["upstream"], "origin/main");
    assert_eq!(ff["commits"], 2);
    assert_eq!(ff["from"], before.as_str());
    assert_eq!(ff["subject"], "upstream 1");
    assert_eq!(ff["moved_checkout"], work.to_string_lossy().as_ref());
    assert_eq!(
        flow::git(&work, &["rev-parse", "main"]),
        flow::git(&work, &["rev-parse", "origin/main"])
    );
    let main = body["branches"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["name"] == "main")
        .unwrap();
    assert_eq!(
        main["behind"], 0,
        "the refreshed list travels along: {body}"
    );

    // Nothing left to take: a named refusal, with the list.
    let (status, body) = fast_forward(&daemon, &work, "main").await;
    assert_eq!(status, 409, "{body}");
    assert_eq!(body["refusal"]["reason"], "up_to_date");
    assert!(body["branches"].is_array(), "{body}");
}

#[tokio::test]
async fn fast_forward_refuses_a_dirty_tree_a_branch_without_upstream_and_a_bad_request() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let root = tempfile::tempdir().unwrap();
    let work = behind_clone(root.path(), 1);

    std::fs::write(work.join("lib.rs"), "fn dirty() {}\n").unwrap();
    let (status, body) = fast_forward(&daemon, &work, "main").await;
    assert_eq!(status, 409, "{body}");
    assert_eq!(body["refusal"]["reason"], "dirty_tree");
    assert_eq!(body["refusal"]["dirty_count"], 1);
    flow::git(&work, &["checkout", "-q", "--", "lib.rs"]);

    flow::git(&work, &["branch", "-q", "lonely"]);
    let (status, body) = fast_forward(&daemon, &work, "lonely").await;
    assert_eq!(status, 409, "{body}");
    assert_eq!(body["refusal"]["reason"], "no_upstream");

    let (status, body) = fast_forward(&daemon, &work, "no-such-branch").await;
    assert_eq!(status, 400, "{body}");
    let (status, body) = fast_forward(&daemon, &work, "  ").await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(body["error"], "branch is required");
    let (status, _) = fast_forward(&daemon, root.path(), "main").await;
    assert_eq!(status, 400, "not a repository");
}

// --- POST /repos/provisioning/preview ---------------------------------------------

fn provisioning_repo(root: &Path) -> std::path::PathBuf {
    let repo = root.join("app");
    flow::git_init(&repo).unwrap();
    std::fs::write(repo.join(".env"), "SECRET=1\n").unwrap();
    std::fs::write(repo.join(".env.local"), "LOCAL=1\n").unwrap();
    std::fs::create_dir_all(repo.join("node_modules/dep")).unwrap();
    std::fs::write(repo.join("node_modules/dep/index.js"), "\n").unwrap();
    repo
}

#[tokio::test]
async fn provisioning_preview_resolves_the_rules_against_the_repository() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let root = tempfile::tempdir().unwrap();
    let repo = provisioning_repo(root.path());

    let (status, plan) = flow::post_json(
        &daemon,
        "/repos/provisioning/preview",
        serde_json::json!({
            "repository": repo.to_str().unwrap(),
            "scope": "run",
            "rules": { "copy": [".env*", "!.env.local"], "symlink": ["node_modules"] },
        }),
    )
    .await;
    assert_eq!(status, 200, "{plan}");
    let entries: Vec<(&str, &str)> = plan["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            (
                e["relative_path"].as_str().unwrap(),
                e["mode"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        entries,
        vec![(".env", "copy"), ("node_modules", "symlink")],
        "{plan}"
    );
    assert!(plan["entries"]
        .as_array()
        .unwrap()
        .iter()
        .all(|e| e["origin_scope"] == "run" && e["provided_by_git"] == false));
    assert_eq!(plan["rules"].as_array().unwrap().len(), 3, "{plan}");
    assert_eq!(plan["conflicts"], serde_json::json!([]));
}

#[tokio::test]
async fn provisioning_preview_folds_the_stored_instance_rules_below_the_edited_scope() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let root = tempfile::tempdir().unwrap();
    let repo = provisioning_repo(root.path());
    let resp = reqwest::Client::new()
        .put(format!("{}/settings/provisioning", daemon.url()))
        .json(&serde_json::json!({ "copy": [".env"] }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let (status, plan) = flow::post_json(
        &daemon,
        "/repos/provisioning/preview",
        serde_json::json!({
            "repository": repo.to_str().unwrap(),
            "scope": "project",
            "rules": { "hardlink": [".env.local"] },
        }),
    )
    .await;
    assert_eq!(status, 200, "{plan}");
    let scopes: Vec<(&str, &str)> = plan["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            (
                e["relative_path"].as_str().unwrap(),
                e["origin_scope"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        scopes,
        vec![(".env", "instance"), (".env.local", "project")],
        "{plan}"
    );

    let (status, body) = flow::post_json(
        &daemon,
        "/repos/provisioning/preview",
        serde_json::json!({ "repository": "relative/app", "scope": "run" }),
    )
    .await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(body["error"], "repository must be an absolute path");

    let (status, body) = flow::post_json(
        &daemon,
        "/repos/provisioning/preview",
        serde_json::json!({
            "repository": repo.to_str().unwrap(),
            "scope": "run",
            "git_ref": "no-such-ref",
        }),
    )
    .await;
    assert_eq!(status, 400, "an unknown ref is named: {body}");
}
