//! Layer 3a — the operator's commands through the real binary, against a real
//! daemon: `pdo project` and `pdo whoami` make the gestures of
//! Settings › Projects and print what the daemon answers, in text and with
//! `--json`; `4` for a Projet the daemon does not know, `1` for a refusal.
//! `pdo trigger` and `pdo audit`: `cli_trigger.rs`; `pdo session`:
//! `cli_session.rs`.

use std::process::Command;

use serde_json::Value;

use crate::common::flow;
use crate::common::TestDaemon;

fn seed(repo: &std::path::Path) -> anyhow::Result<()> {
    flow::seed_solo(repo, &[("solo", false)])
}

/// What one `pdo` command answered.
pub struct Out {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl Out {
    /// The standard output as JSON, once the command succeeded.
    pub fn json(&self) -> Value {
        assert_eq!(self.code, Some(0), "{}\n{}", self.stdout, self.stderr);
        serde_json::from_str(&self.stdout).unwrap_or_else(|e| panic!("{e}: {}", self.stdout))
    }

    /// The standard output, once the command succeeded.
    pub fn ok(&self) -> &str {
        assert_eq!(self.code, Some(0), "{}\n{}", self.stdout, self.stderr);
        &self.stdout
    }
}

/// Run the real binary out of any node session, from `cwd`, on a blocking
/// task so the host runtime keeps serving the daemon it talks to.
pub async fn pdo_in(url: &str, cwd: &std::path::Path, args: &[&str]) -> Out {
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    let url = url.to_string();
    let cwd = cwd.to_path_buf();
    let output = tokio::task::spawn_blocking(move || {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_pdo"));
        cmd.args(&args).current_dir(&cwd);
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("PDO_") {
                cmd.env_remove(key);
            }
        }
        cmd.env("PDO_DAEMON_URL", &url)
            .output()
            .expect("failed to spawn pdo")
    })
    .await
    .expect("blocking task panicked");
    let out = Out {
        code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    };
    assert!(
        !out.stderr.contains("panicked"),
        "pdo panicked: {}",
        out.stderr
    );
    out
}

pub async fn pdo(daemon: &TestDaemon, args: &[&str]) -> Out {
    pdo_in(&daemon.url(), daemon.repo_root(), args).await
}

#[tokio::test]
async fn a_projet_is_created_changed_and_deleted_by_the_cli_as_the_ui_does() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let repo = daemon.target_repo();

    // Created with a member given relative to the working directory.
    let created = pdo(&daemon, &["project", "create", "Web", "--member", "."]).await;
    let line = created.ok().trim_end().to_string();
    let (id, rest) = line.split_once('\t').expect("id<TAB>name created");
    assert_eq!(rest, "Web created");
    let id = id.to_string();
    let project = daemon.get_json(&format!("/projects/{id}")).await;
    assert_eq!(project["name"], "Web");
    assert_eq!(project["members"], serde_json::json!([repo]));

    let list = pdo(&daemon, &["project", "list"]).await;
    assert!(
        list.ok().contains(&format!("{id}\tWeb\t1\t{repo}\n")),
        "{}",
        list.stdout
    );
    assert_eq!(
        pdo(&daemon, &["project", "list", "--json"]).await.json()[0]["id"],
        id.as_str()
    );

    // Renamed, its harness and auto-fail set: what the route keeps.
    let updated = pdo(
        &daemon,
        &[
            "project",
            "update",
            &id,
            "--name",
            "Web app",
            "--harness",
            "claude",
            "--auto-fail",
            "off",
            "--json",
        ],
    )
    .await
    .json();
    assert_eq!(updated["name"], "Web app");
    assert_eq!(updated["harness"], "claude");
    assert_eq!(updated["auto_fail"], false);
    let shown = pdo(&daemon, &["project", "show", &id]).await;
    assert!(
        shown.ok().starts_with(&format!("{id}  Web app\n")),
        "{}",
        shown.stdout
    );
    assert!(
        shown.stdout.contains("agent: harness claude\n"),
        "{}",
        shown.stdout
    );
    assert!(
        shown.stdout.contains("auto-fail: off\n"),
        "{}",
        shown.stdout
    );
    let cleared = pdo(
        &daemon,
        &[
            "project",
            "update",
            &id,
            "--clear-harness",
            "--auto-fail",
            "inherit",
            "--json",
        ],
    )
    .await
    .json();
    assert!(cleared.get("harness").is_none(), "{cleared}");
    assert!(cleared.get("auto_fail").is_none(), "{cleared}");

    // A skill added, then a second, then one removed: the route keeps the whole
    // selection.
    let skills = pdo(
        &daemon,
        &["project", "skill", "add", &id, "tdd", "--name", "TDD"],
    )
    .await;
    assert_eq!(skills.ok(), "tdd\tTDD\n");
    pdo(&daemon, &["project", "skill", "add", &id, "review"])
        .await
        .ok();
    let stored = daemon.get_json(&format!("/projects/{id}")).await;
    assert_eq!(
        stored["skills"],
        serde_json::json!([{ "id": "tdd", "name": "TDD" }, { "id": "review", "name": "review" }])
    );
    let skills = pdo(&daemon, &["project", "skill", "remove", &id, "tdd"]).await;
    assert_eq!(skills.ok(), "review\treview\n");
    let absent = pdo(&daemon, &["project", "skill", "remove", &id, "tdd"]).await;
    assert_eq!(absent.code, Some(1), "{}", absent.stderr);
    pdo(&daemon, &["project", "skill", "set", &id]).await.ok();
    let listed = pdo(&daemon, &["project", "skill", "list", &id, "--json"])
        .await
        .json();
    assert_eq!(listed, serde_json::json!([]), "an empty selection is none");

    // A second member, then detached.
    let other = tempfile::tempdir().unwrap();
    let other_path = other.path().display().to_string();
    let added = pdo(
        &daemon,
        &["project", "member", "add", &id, &other_path, "--json"],
    )
    .await;
    assert_eq!(
        added.json()["members"],
        serde_json::json!([repo, other_path])
    );
    let removed = pdo(&daemon, &["project", "member", "remove", &id, &other_path]).await;
    assert_eq!(removed.ok(), format!("{other_path} removed: {repo}\n"));
    let primary = pdo(
        &daemon,
        &["project", "update", &id, "--primary-repo", &repo, "--json"],
    )
    .await;
    assert_eq!(primary.json()["primary_repo"], repo.as_str());

    // Its provisioning rules, replaced from a file and read back.
    let rules = daemon.repo_root().join("rules.json");
    std::fs::write(
        &rules,
        r#"{ "copy": [".env"], "hardlink": [], "symlink": [] }"#,
    )
    .unwrap();
    let set = pdo(
        &daemon,
        &[
            "project",
            "provisioning",
            &id,
            "--file",
            rules.to_str().unwrap(),
        ],
    )
    .await;
    assert_eq!(set.ok(), "copy: .env\nhardlink: none\nsymlink: none\n");
    let read = pdo(&daemon, &["project", "provisioning", &id, "--json"])
        .await
        .json();
    assert_eq!(read["copy"], serde_json::json!([".env"]));

    // The caller.
    assert_eq!(pdo(&daemon, &["whoami"]).await.ok(), "local_operator\n");
    assert_eq!(
        pdo(&daemon, &["whoami", "--json"]).await.json()["actor"]["kind"],
        "local_operator"
    );

    // Deleted: gone from the list, kept among the deleted ones.
    assert_eq!(
        pdo(&daemon, &["project", "delete", &id]).await.ok(),
        format!("{id} deleted\n")
    );
    assert!(!pdo(&daemon, &["project", "list"]).await.ok().contains(&id));
    let deleted = pdo(&daemon, &["project", "list", "--deleted"]).await;
    assert!(
        deleted.ok().contains(&format!("{id}\tWeb app\n")),
        "{}",
        deleted.stdout
    );
}

#[tokio::test]
async fn an_unknown_projet_exits_4_and_a_refusal_1() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let unknown = pdo(&daemon, &["project", "show", "no-such-project"]).await;
    assert_eq!(unknown.code, Some(4), "{}", unknown.stderr);
    assert!(unknown.stdout.is_empty(), "{}", unknown.stdout);
    assert!(
        unknown.stderr.contains("no such project"),
        "{}",
        unknown.stderr
    );
    let gone = pdo(&daemon, &["project", "delete", "no-such-project"]).await;
    assert_eq!(gone.code, Some(4), "{}", gone.stderr);

    let blank = pdo(&daemon, &["project", "create", " "]).await;
    assert_eq!(blank.code, Some(1), "{}", blank.stderr);
    assert!(
        blank.stderr.contains("daemon returned 400"),
        "{}",
        blank.stderr
    );

    let project = pdo(&daemon, &["project", "create", "Web", "--json"])
        .await
        .json();
    let id = project["id"].as_str().unwrap();
    let nothing = pdo(&daemon, &["project", "update", id]).await;
    assert_eq!(nothing.code, Some(1), "{}", nothing.stderr);
    assert!(
        nothing.stderr.contains("nothing to change"),
        "{}",
        nothing.stderr
    );
}

#[test]
fn an_unreachable_daemon_exits_1() {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_pdo"));
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("PDO_") {
            cmd.env_remove(key);
        }
    }
    let output = cmd
        .args(["project", "list"])
        .env("PDO_DAEMON_URL", "http://127.0.0.1:1")
        .output()
        .expect("failed to spawn pdo");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("failed to reach daemon"), "{stderr}");
}
