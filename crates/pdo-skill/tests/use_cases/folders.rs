//! The bank's folder hierarchy.

use pdo_kernel::{ResourceKind, Verb};
use pdo_skill::domain::error::SkillError;

use crate::fakes::{denying, fixture, op, skill_md};

#[tokio::test]
async fn deleting_a_folder_moves_its_content_to_the_parent() {
    let f = fixture();
    let parent = f.skills.create_folder(&op(), "acme", None).await.unwrap();
    let child = f
        .skills
        .create_folder(&op(), "java", Some(&parent.id))
        .await
        .unwrap();
    let grandchild = f
        .skills
        .create_folder(&op(), "spring", Some(&child.id))
        .await
        .unwrap();
    let skill = f
        .skills
        .create_skill(&op(), &skill_md("tdd"), None, Some(&child.id))
        .await
        .unwrap();
    assert!(f.skills.delete_folder(&op(), &child.id).await.unwrap());
    assert_eq!(
        f.index.one(&skill.id).unwrap().folder_id.as_deref(),
        Some(parent.id.as_str())
    );
    assert_eq!(
        f.folders.one(&grandchild.id).unwrap().parent_id.as_deref(),
        Some(parent.id.as_str())
    );
    assert!(f.folders.one(&child.id).is_none());
    // Skill content untouched.
    assert!(f.content.tree(&skill.id).is_some());
    assert!(!f.skills.delete_folder(&op(), &child.id).await.unwrap());
}

#[tokio::test]
async fn folder_moves_refuse_cycles_and_blank_names() {
    let f = fixture();
    let a = f.skills.create_folder(&op(), "a", None).await.unwrap();
    let b = f
        .skills
        .create_folder(&op(), "b", Some(&a.id))
        .await
        .unwrap();
    assert_eq!(
        f.skills
            .update_folder(&op(), &a.id, None, Some(Some(&b.id)))
            .await
            .unwrap_err(),
        SkillError::FolderCycle
    );
    assert_eq!(
        f.skills
            .update_folder(&op(), &a.id, None, Some(Some(&a.id)))
            .await
            .unwrap_err(),
        SkillError::FolderCycle
    );
    assert_eq!(
        f.skills.create_folder(&op(), "  ", None).await.unwrap_err(),
        SkillError::EmptyFolderName
    );
    let renamed = f
        .skills
        .update_folder(&op(), &b.id, Some("bee"), Some(None))
        .await
        .unwrap();
    assert_eq!(renamed.name, "bee");
    assert_eq!(renamed.parent_id, None);
    assert_eq!(
        f.skills
            .update_folder(&op(), "skf-nope", Some("x"), None)
            .await
            .unwrap_err(),
        SkillError::FolderNotFound
    );
}

#[tokio::test]
async fn a_folder_under_an_unknown_parent_is_refused() {
    let f = fixture();
    assert_eq!(
        f.skills
            .create_folder(&op(), "x", Some("skf-nope"))
            .await
            .unwrap_err(),
        SkillError::FolderNotFound
    );
    assert!(f.folders.all().is_empty());
}

#[tokio::test]
async fn folder_gestures_ask_the_authorization_about_the_folder() {
    let f = denying("read only");
    assert!(matches!(
        f.skills.create_folder(&op(), "x", None).await,
        Err(SkillError::Denied(_))
    ));
    assert!(matches!(
        f.skills
            .update_folder(&op(), "skf-1", Some("y"), None)
            .await,
        Err(SkillError::Denied(_))
    ));
    assert!(matches!(
        f.skills.delete_folder(&op(), "skf-1").await,
        Err(SkillError::Denied(_))
    ));
    let asked = f.host.asked();
    assert!(asked
        .iter()
        .all(|o| o.resource == ResourceKind::SkillFolder));
    assert_eq!(
        asked.iter().map(|o| o.verb.clone()).collect::<Vec<_>>(),
        vec![Verb::Create, Verb::Update, Verb::Delete]
    );
    assert!(f.folders.all().is_empty());
}
