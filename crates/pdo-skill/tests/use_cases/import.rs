//! Scan a Source and import its candidates (#670).

use pdo_kernel::{ResourceKind, Verb};
use pdo_skill::domain::error::SkillError;
use pdo_skill::domain::import::{CandidateStatus, ImportAction, ImportFolder, ImportItem};
use pdo_skill::domain::import_error::ImportError;

use crate::fakes::{denying, fixture, folder_of, op, skill_md};

const SOURCE: &str = "https://github.com/anthropics/skills/tree/main/skills";

fn item(path: &str, action: ImportAction, name: Option<&str>) -> ImportItem {
    ImportItem {
        path: path.into(),
        action,
        name: name.map(str::to_string),
    }
}

#[tokio::test]
async fn a_scan_reports_candidates_against_the_bank_and_remembers_the_source() {
    let f = fixture();
    f.scanner
        .holds("skills/pdf", folder_of("pdf", &[("ref.md", "x")]));
    f.scanner.holds("skills/tdd", folder_of("tdd", &[]));
    f.scanner.holds(
        "skills/bad",
        [("SKILL.md".to_string(), b"no".to_vec())].into(),
    );
    f.skills
        .create_skill(&op(), &skill_md("tdd"), None, None)
        .await
        .unwrap();
    let report = f
        .skills
        .scan_source(&op(), "scan-1", SOURCE, None)
        .await
        .unwrap();
    assert_eq!(
        report.outcome.source.url,
        "https://github.com/anthropics/skills"
    );
    assert_eq!(report.outcome.commit.as_deref(), Some("c1"));
    let status = |path: &str| {
        report
            .candidates
            .iter()
            .find(|c| c.candidate.path == path)
            .map(|c| c.status.clone())
    };
    assert_eq!(status("skills/pdf"), Some(CandidateStatus::New));
    assert_eq!(status("skills/tdd"), Some(CandidateStatus::NameTaken));
    assert_eq!(status("skills/bad"), Some(CandidateStatus::Invalid));
    // Nothing written to the bank; the source joins the recent ones.
    assert_eq!(f.index.all().len(), 1);
    let recent = f.skills.recent_sources().await.unwrap();
    assert_eq!(recent[0].url, "https://github.com/anthropics/skills");
    assert_eq!(recent[0].git_ref.as_deref(), Some("main"));
    assert_eq!(recent[0].path, "skills");
    assert_eq!(f.history.0.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn a_source_that_is_not_one_is_refused_before_any_clone() {
    let f = fixture();
    let err = f
        .skills
        .scan_source(&op(), "scan-1", "not a source", None)
        .await
        .unwrap_err();
    assert!(matches!(err, ImportError::InvalidSource(_)));
    assert_eq!(*f.scanner.scans.lock().unwrap(), 0);
}

#[tokio::test]
async fn import_creates_a_source_folder_with_its_provenance() {
    let f = fixture();
    f.scanner
        .holds("skills/pdf", folder_of("pdf", &[("ref.md", "x")]));
    f.skills
        .scan_source(&op(), "s", SOURCE, None)
        .await
        .unwrap();
    let report = f
        .skills
        .import_skills(
            &op(),
            "s",
            SOURCE,
            None,
            ImportFolder::default(),
            vec![item("skills/pdf", ImportAction::Import, None)],
        )
        .await
        .unwrap();
    assert_eq!(report.folder.name, "anthropics/skills · skills");
    let provenance = report.folder.source.clone().unwrap();
    assert_eq!(provenance.commit.as_deref(), Some("c1"));
    assert_eq!((provenance.found, provenance.invalid), (1, 0));
    assert_eq!(report.imported[0].action, "imported");
    let skill = &report.imported[0].skill;
    assert_eq!(skill.folder_id.as_deref(), Some(report.folder.id.as_str()));
    assert_eq!(skill.source.as_ref().unwrap().path, "skills/pdf");
    assert_eq!(f.content.text(&skill.id, "ref.md").as_deref(), Some("x"));
    // The kept clone served the import: no second scan.
    assert_eq!(*f.scanner.scans.lock().unwrap(), 1);
}

#[tokio::test]
async fn collisions_are_replaced_renamed_or_skipped_as_asked() {
    let f = fixture();
    let existing = f
        .skills
        .create_skill(&op(), &skill_md("tdd"), None, None)
        .await
        .unwrap();
    f.scanner
        .holds("skills/tdd", folder_of("tdd", &[("new.md", "n")]));
    f.scanner.holds("skills/pdf", folder_of("pdf", &[]));
    let report = f
        .skills
        .import_skills(
            &op(),
            "s",
            SOURCE,
            None,
            ImportFolder::default(),
            vec![
                item("skills/tdd", ImportAction::Replace, None),
                item("skills/pdf", ImportAction::Rename, Some(" pdf-extract ")),
            ],
        )
        .await
        .unwrap();
    let actions: Vec<_> = report.imported.iter().map(|r| r.action).collect();
    assert_eq!(actions, vec!["replaced", "renamed"]);
    // Replace keeps the id, label and folder, swaps the content.
    assert_eq!(report.imported[0].skill.id, existing.id);
    assert_eq!(report.imported[0].skill.folder_id, None);
    assert_eq!(f.content.text(&existing.id, "new.md").as_deref(), Some("n"));
    assert_eq!(report.imported[1].skill.name, "pdf-extract");
    // Scan expired: the import re-scanned once.
    assert_eq!(*f.scanner.scans.lock().unwrap(), 1);
}

#[tokio::test]
async fn an_unresolved_rename_or_unknown_candidate_refuses_before_any_write() {
    let f = fixture();
    f.scanner.holds("skills/pdf", folder_of("pdf", &[]));
    let err = f
        .skills
        .import_skills(
            &op(),
            "s",
            SOURCE,
            None,
            ImportFolder::default(),
            vec![item("skills/pdf", ImportAction::Rename, Some("  "))],
        )
        .await
        .unwrap_err();
    assert_eq!(err, ImportError::UnresolvedCollision("skills/pdf".into()));
    let err = f
        .skills
        .import_skills(
            &op(),
            "s",
            SOURCE,
            None,
            ImportFolder::default(),
            vec![item("skills/../x", ImportAction::Import, None)],
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ImportError::UnknownCandidate(_)));
    assert!(f.folders.all().is_empty());
    assert!(f.index.all().is_empty());
}

#[tokio::test]
async fn a_failed_row_is_reported_and_an_empty_new_folder_is_not_left_behind() {
    let f = fixture();
    f.skills
        .create_skill(&op(), &skill_md("pdf"), None, None)
        .await
        .unwrap();
    f.scanner.holds("skills/pdf", folder_of("pdf", &[]));
    let report = f
        .skills
        .import_skills(
            &op(),
            "s",
            SOURCE,
            None,
            ImportFolder {
                name: Some("Mine".into()),
                ..ImportFolder::default()
            },
            vec![item("skills/pdf", ImportAction::Import, None)],
        )
        .await
        .unwrap();
    assert!(report.imported.is_empty());
    assert_eq!(report.failed[0].code, "duplicate_name");
    assert_eq!(report.folder.name, "Mine");
    assert!(
        f.folders.all().is_empty(),
        "the empty Source folder is removed"
    );
}

#[tokio::test]
async fn an_import_into_an_unknown_folder_is_refused() {
    let f = fixture();
    f.scanner.holds("skills/pdf", folder_of("pdf", &[]));
    let err = f
        .skills
        .import_skills(
            &op(),
            "s",
            SOURCE,
            None,
            ImportFolder {
                id: Some("skf-nope".into()),
                ..ImportFolder::default()
            },
            vec![item("skills/pdf", ImportAction::Import, None)],
        )
        .await
        .unwrap_err();
    assert_eq!(err, ImportError::Skill(SkillError::FolderNotFound));
}

#[tokio::test]
async fn scan_import_and_cancel_ask_the_authorization() {
    let f = denying("no imports");
    f.scanner.holds("skills/pdf", folder_of("pdf", &[]));
    assert!(matches!(
        f.skills.scan_source(&op(), "s", SOURCE, None).await,
        Err(ImportError::Skill(SkillError::Denied(_)))
    ));
    assert!(matches!(
        f.skills
            .import_skills(
                &op(),
                "s",
                SOURCE,
                None,
                ImportFolder::default(),
                vec![item("skills/pdf", ImportAction::Import, None)]
            )
            .await,
        Err(ImportError::Skill(SkillError::Denied(_)))
    ));
    assert!(f.skills.cancel_scan(&op(), "running").is_err());
    assert_eq!(*f.scanner.scans.lock().unwrap(), 0);
    let verbs: Vec<_> = f.host.asked().into_iter().map(|o| o.verb).collect();
    assert_eq!(
        verbs,
        vec![
            Verb::Action("scan".into()),
            Verb::Action("import".into()),
            Verb::Action("cancel_scan".into())
        ]
    );
    assert!(f
        .host
        .asked()
        .iter()
        .all(|o| o.resource == ResourceKind::Skill));
}

#[tokio::test]
async fn cancel_reaches_the_running_clone() {
    let f = fixture();
    assert!(f.skills.cancel_scan(&op(), "running").unwrap());
    assert!(!f.skills.cancel_scan(&op(), "idle").unwrap());
}
