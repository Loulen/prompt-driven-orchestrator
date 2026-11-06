//! Layer 3a — `pdo skill` through the real binary, against a real daemon: a
//! skill added to the bank from its `SKILL.md`, renamed, moved, its files and
//! folders, exported and uploaded back, removed; a skill the core provides is
//! locked, as in the UI; `4` for a skill the daemon does not know.

use crate::cli_operator::pdo;
use crate::common::flow;
use crate::common::TestDaemon;

fn seed(repo: &std::path::Path) -> anyhow::Result<()> {
    flow::seed_solo(repo, &[("solo", false)])
}

const SKILL_MD: &str =
    "---\nname: cli-craft\ndescription: Craft from the command line.\n---\n\nBe careful.\n";

#[tokio::test]
async fn a_skill_is_added_changed_and_removed_as_settings_skills_does() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let skill_md = dir.path().join("SKILL.md");
    std::fs::write(&skill_md, SKILL_MD).unwrap();

    let added = pdo(
        &daemon,
        &["skill", "add", skill_md.to_str().unwrap(), "--json"],
    )
    .await
    .json();
    let id = added["id"].as_str().unwrap().to_string();
    assert_eq!(added["name"], "cli-craft");
    let bank = daemon.get_json("/settings/skills").await;
    assert!(
        bank["skills"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["id"] == id),
        "it is in the bank the UI reads: {bank}"
    );
    let list = pdo(&daemon, &["skill", "list"]).await;
    assert!(
        list.ok().contains(&format!("{id}\tcli-craft\t-\n")),
        "{}",
        list.stdout
    );
    let shown = pdo(&daemon, &["skill", "show", &id]).await;
    assert!(shown.ok().contains("Be careful."), "{}", shown.stdout);
    // Its name, as `add` and `list` print it, names it too.
    let by_name = pdo(&daemon, &["skill", "show", "cli-craft"]).await;
    assert!(by_name.ok().contains("Be careful."), "{}", by_name.stdout);
    let no_skill = pdo(&daemon, &["skill", "show", "no-such-skill"]).await;
    assert_eq!(no_skill.code, Some(4), "{}", no_skill.stderr);

    // A duplicate name is refused.
    let duplicate = pdo(&daemon, &["skill", "add", skill_md.to_str().unwrap()]).await;
    assert_eq!(duplicate.code, Some(1), "{}", duplicate.stderr);

    // Folders, rename and move.
    let folder = pdo(&daemon, &["skill", "folder", "add", "Craft", "--json"])
        .await
        .json();
    let folder_id = folder["id"].as_str().unwrap().to_string();
    let moved = pdo(
        &daemon,
        &[
            "skill", "update", &id, "--name", "crafty", "--folder", &folder_id,
        ],
    )
    .await;
    assert_eq!(moved.ok(), format!("{id}\tcrafty updated\n"));
    let list = pdo(&daemon, &["skill", "list"]).await;
    assert!(
        list.ok().contains(&format!("{id}\tcrafty\tCraft\n")),
        "{}",
        list.stdout
    );
    pdo(
        &daemon,
        &["skill", "folder", "update", &folder_id, "--name", "Crafts"],
    )
    .await
    .ok();
    assert!(pdo(&daemon, &["skill", "folder", "list"])
        .await
        .ok()
        .contains(&format!("{folder_id}\tCrafts\t-\t-\n")));

    // Files: added, read, written, removed.
    let fixture = dir.path().join("checklist.md");
    std::fs::write(&fixture, "- one\n").unwrap();
    let file = pdo(
        &daemon,
        &[
            "skill",
            "file",
            "add",
            &id,
            fixture.to_str().unwrap(),
            "--as",
            "refs/checklist.md",
        ],
    )
    .await;
    assert_eq!(file.ok(), "refs/checklist.md\t6 bytes added\n");
    assert_eq!(
        pdo(
            &daemon,
            &["skill", "file", "show", &id, "refs/checklist.md"]
        )
        .await
        .ok(),
        "- one\n"
    );
    std::fs::write(&fixture, "- two\n").unwrap();
    pdo(
        &daemon,
        &[
            "skill",
            "file",
            "write",
            &id,
            "refs/checklist.md",
            fixture.to_str().unwrap(),
        ],
    )
    .await
    .ok();
    assert_eq!(
        pdo(
            &daemon,
            &["skill", "file", "show", &id, "refs/checklist.md"]
        )
        .await
        .ok(),
        "- two\n"
    );
    pdo(
        &daemon,
        &["skill", "file", "remove", &id, "refs/checklist.md"],
    )
    .await
    .ok();
    let gone = pdo(
        &daemon,
        &["skill", "file", "show", &id, "refs/checklist.md"],
    )
    .await;
    assert_eq!(gone.code, Some(4), "{}", gone.stderr);

    // Export, then upload back: the bank keeps the id it already holds.
    let zip = dir.path().join("bank.zip");
    let exported = pdo(
        &daemon,
        &[
            "skill",
            "export",
            "--skill",
            &id,
            "-o",
            zip.to_str().unwrap(),
        ],
    )
    .await;
    assert!(exported.ok().contains("written"), "{}", exported.stdout);
    let uploaded = pdo(&daemon, &["skill", "upload", zip.to_str().unwrap()]).await;
    assert!(
        uploaded.ok().contains(&format!("kept\t{id}")),
        "{}",
        uploaded.stdout
    );

    assert_eq!(
        pdo(&daemon, &["skill", "referents", &id]).await.ok(),
        "none\n"
    );
    pdo(&daemon, &["skill", "remove", &id]).await.ok();
    let missing = pdo(&daemon, &["skill", "show", &id]).await;
    assert_eq!(missing.code, Some(4), "{}", missing.stderr);
    pdo(&daemon, &["skill", "folder", "remove", &folder_id])
        .await
        .ok();
}

#[tokio::test]
async fn a_skill_the_core_provides_is_locked() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let list = pdo(&daemon, &["skill", "list"]).await;
    assert!(list.ok().contains("pdo-interactive\t"), "{}", list.stdout);
    assert!(list.stdout.contains("\tlocked\n"), "{}", list.stdout);
    for args in [
        &["skill", "remove", "pdo-interactive"][..],
        &["skill", "update", "pdo-interactive", "--name", "mine"][..],
    ] {
        let refused = pdo(&daemon, args).await;
        assert_eq!(refused.code, Some(1), "{args:?}: {}", refused.stderr);
    }
}

#[tokio::test]
async fn a_local_source_is_scanned_and_imported() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let source = tempfile::tempdir().unwrap();
    let skill = source.path().join("skills/sourced");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(
        skill.join("SKILL.md"),
        "---\nname: sourced\ndescription: From a Source.\n---\nBody\n",
    )
    .unwrap();
    for args in [
        &["init", "-q", "-b", "main"][..],
        &["-c", "user.email=t@e", "-c", "user.name=T", "add", "."][..],
        &[
            "-c",
            "user.email=t@e",
            "-c",
            "user.name=T",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            "skills",
        ][..],
    ] {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(source.path())
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let path = source.path().to_str().unwrap();
    let scan = pdo(&daemon, &["skill", "scan", path]).await;
    assert!(
        scan.ok().contains("new\tskills/sourced\tsourced\n"),
        "{}",
        scan.stdout
    );
    let imported = pdo(&daemon, &["skill", "import", path]).await;
    assert!(imported.ok().contains("\tsourced\n"), "{}", imported.stdout);
    let again = pdo(&daemon, &["skill", "import", path]).await;
    assert_eq!(again.code, Some(1), "nothing new: {}", again.stderr);
    assert!(pdo(&daemon, &["skill", "sources"]).await.ok().len() > 1);
}
