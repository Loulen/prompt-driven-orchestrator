//! Create, rename, move and delete a skill; the reads of the bank.

use pdo_kernel::{ResourceKind, Verb};
use pdo_skill::application::ports::{NamedRef, Referents};
use pdo_skill::domain::error::SkillError;

use crate::fakes::{denying, fixture, op, skill_md};

#[tokio::test]
async fn create_writes_the_folder_keyed_by_id_and_indexes_the_row() {
    let f = fixture();
    let skill = f
        .skills
        .create_skill(&op(), &skill_md("tdd"), None, None)
        .await
        .unwrap();
    assert_eq!(skill.name, "tdd");
    assert_eq!(skill.description, "The tdd method.");
    assert_eq!(skill.folder_id, None);
    assert!(!skill.locked);
    assert_eq!(
        f.content.text(&skill.id, "SKILL.md").unwrap(),
        skill_md("tdd")
    );
    assert_eq!(f.index.all().len(), 1);
    assert!(f.skills.files(&skill.id).is_empty());
    let asked = f.host.asked();
    assert_eq!(asked[0].resource, ResourceKind::Skill);
    assert_eq!(asked[0].verb, Verb::Create);
}

#[tokio::test]
async fn invalid_content_writes_nothing() {
    let f = fixture();
    let err = f
        .skills
        .create_skill(&op(), "---\nname: tdd\n---\nbody", None, None)
        .await
        .unwrap_err();
    assert_eq!(err, SkillError::MissingDescription);
    assert!(f.index.all().is_empty());
    assert!(f.content.trees.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_disk_failure_rolls_the_row_back() {
    let f = fixture();
    *f.content.fail_writes.lock().unwrap() = true;
    let err = f
        .skills
        .create_skill(&op(), &skill_md("tdd"), None, None)
        .await
        .unwrap_err();
    assert!(
        matches!(&err, SkillError::Storage(m) if m.starts_with("failed to write /bank/.pdo/skills/")),
        "{err:?}"
    );
    assert!(f.index.all().is_empty());
}

#[tokio::test]
async fn names_are_unique_case_insensitively() {
    let f = fixture();
    f.skills
        .create_skill(&op(), &skill_md("tdd"), None, None)
        .await
        .unwrap();
    let err = f
        .skills
        .create_skill(&op(), &skill_md("tdd"), Some("TDD"), None)
        .await
        .unwrap_err();
    assert!(matches!(err, SkillError::DuplicateName { ref name, .. } if name == "tdd"));
    // The refused create wrote nothing.
    assert_eq!(f.content.trees.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn a_create_into_an_unknown_folder_is_refused() {
    let f = fixture();
    let err = f
        .skills
        .create_skill(&op(), &skill_md("tdd"), None, Some("skf-nope"))
        .await
        .unwrap_err();
    assert_eq!(err, SkillError::FolderNotFound);
    assert!(f.index.all().is_empty());
}

#[tokio::test]
async fn rename_touches_the_label_only() {
    let f = fixture();
    let skill = f
        .skills
        .create_skill(&op(), &skill_md("tdd"), None, None)
        .await
        .unwrap();
    let renamed = f
        .skills
        .update_skill(&op(), &skill.id, Some("tdd-strict"), None)
        .await
        .unwrap();
    assert_eq!(renamed.id, skill.id);
    assert_eq!(renamed.name, "tdd-strict");
    // Content untouched: frontmatter name still `tdd`.
    assert_eq!(
        f.content.text(&skill.id, "SKILL.md").unwrap(),
        skill_md("tdd")
    );
}

#[tokio::test]
async fn rename_collision_is_refused_and_self_case_change_allowed() {
    let f = fixture();
    let a = f
        .skills
        .create_skill(&op(), &skill_md("tdd"), None, None)
        .await
        .unwrap();
    f.skills
        .create_skill(&op(), &skill_md("tdd"), Some("grilling"), None)
        .await
        .unwrap();
    let err = f
        .skills
        .update_skill(&op(), &a.id, Some("Grilling"), None)
        .await
        .unwrap_err();
    assert!(matches!(err, SkillError::DuplicateName { .. }));
    let ok = f
        .skills
        .update_skill(&op(), &a.id, Some("TDD"), None)
        .await
        .unwrap();
    assert_eq!(ok.name, "TDD");
}

#[tokio::test]
async fn move_into_a_folder_and_back_to_root() {
    let f = fixture();
    let skill = f
        .skills
        .create_skill(&op(), &skill_md("tdd"), None, None)
        .await
        .unwrap();
    let folder = f
        .skills
        .create_folder(&op(), "méthode", None)
        .await
        .unwrap();
    let moved = f
        .skills
        .update_skill(&op(), &skill.id, None, Some(Some(&folder.id)))
        .await
        .unwrap();
    assert_eq!(moved.folder_id.as_deref(), Some(folder.id.as_str()));
    let back = f
        .skills
        .update_skill(&op(), &skill.id, None, Some(None))
        .await
        .unwrap();
    assert_eq!(back.folder_id, None);
    let err = f
        .skills
        .update_skill(&op(), &skill.id, None, Some(Some("skf-nope")))
        .await
        .unwrap_err();
    assert_eq!(err, SkillError::FolderNotFound);
}

#[tokio::test]
async fn delete_removes_row_and_folder() {
    let f = fixture();
    let skill = f
        .skills
        .create_skill(&op(), &skill_md("tdd"), None, None)
        .await
        .unwrap();
    assert!(f.skills.delete_skill(&op(), &skill.id).await.unwrap());
    assert!(f.content.tree(&skill.id).is_none());
    assert!(f.index.one(&skill.id).is_none());
    assert!(!f.skills.delete_skill(&op(), &skill.id).await.unwrap());
}

#[tokio::test]
async fn a_refusal_writes_nothing_and_names_the_operation() {
    let f = denying("admins only");
    let err = f
        .skills
        .create_skill(&op(), &skill_md("tdd"), None, None)
        .await
        .unwrap_err();
    assert!(matches!(&err, SkillError::Denied(d) if d.reason == "admins only"));
    assert_eq!(err.code(), "denied");
    assert!(f.index.all().is_empty());
    let err = f.skills.delete_skill(&op(), "s1").await.unwrap_err();
    assert!(matches!(err, SkillError::Denied(_)));
    let asked = f.host.asked();
    assert_eq!(asked[1].verb, Verb::Delete);
    assert_eq!(asked[1].target.as_deref(), Some("s1"));
}

#[tokio::test]
async fn the_detail_reads_the_content_and_survives_a_vanished_folder() {
    let f = fixture();
    let skill = f
        .skills
        .create_skill(&op(), &skill_md("tdd"), None, None)
        .await
        .unwrap();
    let detail = f.skills.skill_detail(&skill.id).await.unwrap();
    assert_eq!(detail.content.as_deref(), Some(skill_md("tdd").as_str()));
    assert_eq!(detail.parsed.unwrap().body, "# tdd\n\nBody.");
    assert_eq!(
        detail.path.to_str(),
        Some(format!("/bank/.pdo/skills/{}", skill.id).as_str())
    );
    f.content.trees.lock().unwrap().clear();
    let detail = f.skills.skill_detail(&skill.id).await.unwrap();
    assert!(detail.content.is_none() && detail.parsed.is_none());
    assert_eq!(
        f.skills.skill_detail("nope").await.unwrap_err(),
        SkillError::NotFound
    );
}

#[tokio::test]
async fn referents_come_from_the_tiers_downstream_of_a_known_skill() {
    let f = fixture();
    let skill = f
        .skills
        .create_skill(&op(), &skill_md("tdd"), None, None)
        .await
        .unwrap();
    *f.referring.0.lock().unwrap() = Referents {
        instance: true,
        projects: vec![NamedRef {
            id: "p1".into(),
            name: "Front".into(),
        }],
        ..Referents::default()
    };
    let referents = f.skills.referents(&skill.id).await.unwrap();
    assert!(referents.instance);
    assert_eq!(referents.projects[0].name, "Front");
    assert_eq!(
        f.skills.referents("nope").await.unwrap_err(),
        SkillError::NotFound
    );
}

#[tokio::test]
async fn the_bank_names_snapshot_maps_ids_to_labels() {
    let f = fixture();
    let skill = f
        .skills
        .create_skill(&op(), &skill_md("tdd"), Some("strict"), None)
        .await
        .unwrap();
    let names = f.skills.bank_names().await.unwrap();
    assert_eq!(names.get(&skill.id).map(String::as_str), Some("strict"));
    let bank = f.skills.bank().await.unwrap();
    assert_eq!(bank.skills.len(), 1);
    assert_eq!(bank.root_path.to_str(), Some("/bank/.pdo/skills"));
}

#[tokio::test]
async fn a_selection_gains_only_skills_of_the_bank_and_keeps_a_deleted_one() {
    use pdo_skill::domain::selection::SkillRef;
    let f = fixture();
    let tdd = f
        .skills
        .create_skill(&op(), &skill_md("tdd"), None, None)
        .await
        .unwrap();
    let skill = |id: &str| SkillRef {
        id: id.to_string(),
        name: id.to_string(),
    };
    let foreign = |current: Vec<SkillRef>, next: Vec<SkillRef>| {
        let skills = &f.skills;
        async move { skills.foreign_skill(&current, &next).await.unwrap() }
    };
    assert_eq!(foreign(vec![], vec![skill(&tdd.id)]).await, None);
    assert_eq!(foreign(vec![], vec![]).await, None);
    assert_eq!(
        foreign(vec![], vec![skill(&tdd.id), skill("ghost"), skill("other")]).await,
        Some("ghost".to_string()),
        "the first skill the bank does not hold"
    );
    assert_eq!(
        foreign(vec![skill("ghost")], vec![skill("ghost"), skill(&tdd.id)]).await,
        None,
        "a skill already selected stays, though the bank no longer holds it"
    );
}
