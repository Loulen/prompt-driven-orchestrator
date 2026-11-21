//! What the Agent de support reads besides its prompt, on a real daemon with
//! the test extension: the skills PDO manages (the core's and the one the test
//! extension brings) delivered into its folder as into a Run's worktree, and
//! the core's code at the tag of this binary, a shallow clone of a local
//! repository standing in for the public one, made by `pdo support source` at
//! the first demand and reused after; never tried for a version without a
//! public tag.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::cli_operator::pdo;
use crate::common::flow::{git, git_init};
use crate::common::TestDaemon;
use crate::test_extension::{Refusal, TestExtension, SKILL_NAME};

/// The tag the core of this binary was released at.
fn tag() -> String {
    format!("v{}", env!("CARGO_PKG_VERSION"))
}

/// A harness whose binary (`sh`) is installed everywhere.
const DESCRIPTORS: &str = r#"harnesses:
  fake:
    binary: sh
    launch: ["exec", "sh", "{prompt}"]
"#;

/// A daemon with the test extension, its home under its root, cloning the
/// core from `origin`.
async fn daemon(origin: String) -> TestDaemon {
    let (ext, _control) = TestExtension::new(Refusal::Nothing);
    TestDaemon::spawn_configured(
        |root| {
            let file = root.join("home/.pdo/harnesses/descriptors.yaml");
            std::fs::create_dir_all(file.parent().unwrap())?;
            std::fs::write(file, DESCRIPTORS)?;
            Ok(())
        },
        move |root, config| {
            config.sandbox_home_override = Some(root.join("home"));
            config.core_source_url = Some(origin);
            pdo::compose(config, ext.boxed(), None).unwrap();
        },
    )
    .await
    .unwrap()
}

/// A repository standing in for the core's, with one commit, tagged `tag`
/// when given.
fn origin(dir: &Path, tag: Option<&str>) -> String {
    git_init(dir).unwrap();
    std::fs::write(
        dir.join("public_url.rs"),
        "// the default of the Public URL\n",
    )
    .unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "-q", "-m", "core"]);
    if let Some(tag) = tag {
        git(dir, &["tag", tag]);
    }
    format!("file://{}", dir.display())
}

async fn open(daemon: &TestDaemon) -> Value {
    let resp = reqwest::Client::new()
        .post(format!("{}/sessions/support", daemon.url()))
        .json(&json!({ "choice": { "mode": "custom", "harness": "fake" } }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 201);
    resp.json().await.unwrap()
}

fn folder(daemon: &TestDaemon, opened: &Value) -> PathBuf {
    daemon
        .repo_root()
        .join("home/.pdo/support")
        .join(opened["session"].as_str().unwrap())
}

#[tokio::test]
async fn the_agent_receives_the_extensions_skill_and_the_core_code_at_its_tag() {
    let scratch = tempfile::tempdir().unwrap();
    let origin_dir = scratch.path().join("core");
    let daemon = daemon(origin(&origin_dir, Some(&tag()))).await;
    let opened = open(&daemon).await;
    let dir = folder(&daemon, &opened);

    // The extension's skill, as in a Run's worktree: a copy and its link.
    let skill = dir.join(".agents/skills").join(SKILL_NAME);
    assert!(skill.join("SKILL.md").is_file(), "{}", skill.display());
    let link = dir.join(".claude/skills").join(SKILL_NAME);
    assert!(std::fs::symlink_metadata(&link)
        .unwrap()
        .file_type()
        .is_symlink());
    assert!(link.join("SKILL.md").is_file());
    assert!(dir
        .join(".agents/skills/pdo-orchestrate/SKILL.md")
        .is_file());
    // The instructions name them and say how to read the core's code.
    let instructions = std::fs::read_to_string(dir.join("AGENTS.md")).unwrap();
    assert!(
        instructions.contains(&format!("`{SKILL_NAME}`")),
        "{instructions}"
    );
    assert!(
        instructions.contains(&format!("tag `{}`", tag())),
        "{instructions}"
    );
    assert!(
        instructions.contains("`pdo support source`"),
        "{instructions}"
    );

    // The first demand clones it, at the tag, one commit deep.
    let first = pdo(&daemon, &["support", "source"]).await;
    let path = PathBuf::from(first.ok().trim_end());
    assert_eq!(
        path,
        daemon.repo_root().join("home/.pdo/core-source").join(tag())
    );
    assert!(path.join("public_url.rs").is_file());
    assert_eq!(git(&path, &["rev-list", "--count", "HEAD"]), "1");
    assert_eq!(git(&path, &["describe", "--tags"]), tag());

    // Reused after: the origin may be gone.
    std::fs::remove_dir_all(&origin_dir).unwrap();
    let again = pdo(&daemon, &["support", "source", "--json"]).await;
    assert_eq!(again.json()["path"], path.display().to_string());
    assert_eq!(again.json()["tag"], tag());
}

#[tokio::test]
async fn a_version_without_a_public_tag_gets_no_clone_and_its_agent_is_told() {
    let scratch = tempfile::tempdir().unwrap();
    let daemon = daemon(origin(&scratch.path().join("core"), Some("v0.0.1"))).await;
    let opened = open(&daemon).await;
    let instructions = std::fs::read_to_string(folder(&daemon, &opened).join("AGENTS.md")).unwrap();
    assert!(
        instructions.contains(&format!("no public tag (`{}`)", tag())),
        "{instructions}"
    );

    let resp = reqwest::Client::new()
        .post(format!("{}/sessions/support/source", daemon.url()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 409);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "no_public_tag", "{body}");
    let refused = pdo(&daemon, &["support", "source"]).await;
    assert_eq!(refused.code, Some(1), "{}", refused.stderr);
    assert!(
        refused.stderr.contains("no public tag"),
        "{}",
        refused.stderr
    );
    assert!(
        !daemon.repo_root().join("home/.pdo/core-source").exists(),
        "no clone tried"
    );
}
