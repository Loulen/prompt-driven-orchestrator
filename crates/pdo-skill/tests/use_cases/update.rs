//! Rescan a Source folder and update it from its source (#670).

use pdo_skill::domain::import::{ImportAction, ImportFolder, ImportItem};
use pdo_skill::domain::import_error::ImportError;
use pdo_skill::domain::skill::SkillFolder;
use pdo_skill::domain::update::{UpdateAction, UpdateItem, UpdateStatus};

use crate::fakes::{fixture, folder_of, op, Fixture};

const SOURCE: &str = "https://github.com/o/r";

/// A Source folder that imported `skills/a` and `skills/b` at commit `c1`.
async fn imported() -> (Fixture, SkillFolder) {
    let f = fixture();
    f.scanner
        .holds("skills/a", folder_of("a", &[("ref.md", "1")]));
    f.scanner.holds("skills/b", folder_of("b", &[]));
    let items = ["skills/a", "skills/b"]
        .map(|path| ImportItem {
            path: path.into(),
            action: ImportAction::Import,
            name: None,
        })
        .to_vec();
    let report = f
        .skills
        .import_skills(&op(), "s0", SOURCE, None, ImportFolder::default(), items)
        .await
        .unwrap();
    (f, report.folder)
}

#[tokio::test]
async fn a_rescan_diffs_the_source_against_the_folder() {
    let (f, folder) = imported().await;
    *f.scanner.commit.lock().unwrap() = Some("c2".into());
    f.scanner.holds(
        "skills/a",
        folder_of("a", &[("ref.md", "2"), ("new.md", "n")]),
    );
    f.scanner.candidates.lock().unwrap().remove("skills/b");
    f.scanner
        .holds("skills/c", folder_of("c", &[("x.md", "x")]));
    let report = f
        .skills
        .rescan_folder(&op(), &folder.id, "s1")
        .await
        .unwrap();
    assert_eq!(report.previous_commit.as_deref(), Some("c1"));
    assert_eq!(report.commit.as_deref(), Some("c2"));
    let entry = |path: &str| report.entries.iter().find(|e| e.path == path).unwrap();
    let a = entry("skills/a");
    assert_eq!(a.status, UpdateStatus::Updated);
    assert_eq!((a.files_added, a.files_changed, a.files_removed), (1, 1, 0));
    assert!(!a.skill_md_changed);
    assert_eq!(entry("skills/b").status, UpdateStatus::Gone);
    let c = entry("skills/c");
    assert_eq!(c.status, UpdateStatus::New);
    assert_eq!(c.files_added, 1);
    // Read-only on the bank.
    assert_eq!(f.index.all().len(), 2);
}

#[tokio::test]
async fn a_skill_moved_to_another_folder_is_skipped_by_the_rescan() {
    let (f, folder) = imported().await;
    let other = f
        .skills
        .create_folder(&op(), "elsewhere", None)
        .await
        .unwrap();
    let a = f.index.all().into_iter().find(|s| s.name == "a").unwrap();
    f.skills
        .update_skill(&op(), &a.id, None, Some(Some(&other.id)))
        .await
        .unwrap();
    let report = f
        .skills
        .rescan_folder(&op(), &folder.id, "s1")
        .await
        .unwrap();
    let entry = report
        .entries
        .iter()
        .find(|e| e.path == "skills/a")
        .unwrap();
    assert_eq!(entry.status, UpdateStatus::Skipped);
    assert_eq!(
        entry.reason.as_deref(),
        Some("already in \u{201c}elsewhere\u{201d}")
    );
    let b = report
        .entries
        .iter()
        .find(|e| e.path == "skills/b")
        .unwrap();
    assert_eq!(b.status, UpdateStatus::Unchanged);
}

#[tokio::test]
async fn a_folder_without_provenance_cannot_be_rescanned() {
    let f = fixture();
    let plain = f.skills.create_folder(&op(), "plain", None).await.unwrap();
    assert_eq!(
        f.skills
            .rescan_folder(&op(), &plain.id, "s")
            .await
            .unwrap_err(),
        ImportError::NotASourceFolder
    );
}

#[tokio::test]
async fn an_update_replaces_imports_and_moves_the_unchanged_forward() {
    let (f, folder) = imported().await;
    *f.scanner.commit.lock().unwrap() = Some("c2".into());
    f.scanner
        .holds("skills/a", folder_of("a", &[("ref.md", "2")]));
    f.scanner.holds("skills/c", folder_of("c", &[]));
    let report = f
        .skills
        .update_from_source(
            &op(),
            &folder.id,
            "s2",
            vec![
                UpdateItem {
                    path: "skills/a".into(),
                    action: UpdateAction::Update,
                },
                UpdateItem {
                    path: "skills/c".into(),
                    action: UpdateAction::Import,
                },
                UpdateItem {
                    path: "skills/zzz".into(),
                    action: UpdateAction::Update,
                },
            ],
        )
        .await
        .unwrap();
    let actions: Vec<_> = report.imported.iter().map(|r| r.action).collect();
    assert_eq!(actions, vec!["updated", "imported"]);
    assert_eq!(report.failed[0].code, "unknown_candidate");
    let by_name = |name: &str| f.index.all().into_iter().find(|s| s.name == name).unwrap();
    let a = by_name("a");
    assert_eq!(f.content.text(&a.id, "ref.md").as_deref(), Some("2"));
    assert_eq!(a.source.unwrap().commit.as_deref(), Some("c2"));
    // `b` did not change at the source: only its commit moved.
    assert_eq!(by_name("b").source.unwrap().commit.as_deref(), Some("c2"));
    assert_eq!(report.folder.source.unwrap().commit.as_deref(), Some("c2"));
}
