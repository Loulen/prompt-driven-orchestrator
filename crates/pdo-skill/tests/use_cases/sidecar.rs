//! Import the skills sidecar of a pipeline document (#673).

use std::collections::BTreeMap;

use pdo_skill::domain::selection::SkillRef;
use pdo_skill::domain::sidecar::SidecarSkill;

use crate::fakes::{denying, fixture, op, skill_md};

fn r(id: &str, name: &str) -> SkillRef {
    SkillRef {
        id: id.into(),
        name: name.into(),
    }
}

fn carried(id: &str, name: &str) -> SidecarSkill {
    let mut files = BTreeMap::new();
    files.insert("SKILL.md".to_string(), skill_md(name).into_bytes());
    files.insert("examples/one.md".to_string(), b"# example\n".to_vec());
    SidecarSkill {
        id: id.into(),
        files,
    }
}

#[tokio::test]
async fn unknown_ids_are_created_known_kept_taken_labels_suffixed_absent_warned() {
    let f = fixture();
    let kept = f
        .skills
        .create_skill(&op(), &skill_md("grilling"), None, None)
        .await
        .unwrap();
    f.skills
        .create_skill(&op(), &skill_md("tdd"), None, None)
        .await
        .unwrap();
    let report = f
        .skills
        .import_sidecar(
            &op(),
            "My pipeline",
            vec![
                r(&kept.id, "old-label"),
                r("aaaa", "tdd"),
                r("gone", "deleted-skill"),
            ],
            &[carried("aaaa", "tdd")],
        )
        .await
        .unwrap();
    assert_eq!(report.kept, vec![r(&kept.id, "grilling")]);
    assert_eq!(report.created, vec![r("aaaa", "tdd-2")]);
    assert_eq!(report.renamed[0].from, "tdd");
    assert_eq!(report.missing, vec![r("gone", "deleted-skill")]);
    assert_eq!(
        report.folder.as_ref().unwrap().name,
        "importés avec My pipeline"
    );
    assert_eq!(report.warnings.len(), 2);
    // The created skill keeps the document's id and its reference files.
    assert_eq!(
        f.content.text("aaaa", "examples/one.md").as_deref(),
        Some("# example\n")
    );
    let names = report.final_names();
    assert_eq!(names.get(&kept.id).map(String::as_str), Some("grilling"));
    assert_eq!(names.get("aaaa").map(String::as_str), Some("tdd-2"));
}

#[tokio::test]
async fn an_invalid_carried_skill_md_is_a_warning_not_a_failure() {
    let f = fixture();
    let mut bad = carried("bbbb", "x");
    bad.files
        .insert("SKILL.md".into(), b"no frontmatter".to_vec());
    let report = f
        .skills
        .import_sidecar(&op(), "p", vec![r("bbbb", "")], &[bad])
        .await
        .unwrap();
    assert!(report.created.is_empty());
    assert_eq!(report.missing.len(), 1);
    assert!(report.warnings[0].contains("is invalid"));
    assert!(report.folder.is_none());
}

#[tokio::test]
async fn a_pipeline_without_skills_asks_nothing() {
    let f = denying("no");
    let report = f
        .skills
        .import_sidecar(&op(), "p", vec![], &[])
        .await
        .unwrap();
    assert!(report.created.is_empty());
    assert!(f.host.asked().is_empty());
    assert!(f
        .skills
        .import_sidecar(&op(), "p", vec![r("a", "")], &[])
        .await
        .is_err());
}

#[tokio::test]
async fn the_sidecar_of_references_reads_the_bank() {
    let f = fixture();
    let skill = f
        .skills
        .create_skill(&op(), &skill_md("tdd"), None, None)
        .await
        .unwrap();
    f.content
        .trees
        .lock()
        .unwrap()
        .get_mut(&skill.id)
        .unwrap()
        .insert("ref.md".into(), b"r".to_vec());
    let (skills, absent) =
        f.skills
            .sidecar_of(&[r(&skill.id, "tdd"), r("../evil", ""), r("gone", "")]);
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0].files.len(), 2);
    assert_eq!(absent, vec!["../evil".to_string(), "gone".to_string()]);
}
