//! The seed at startup (#722, ADR-0064; #588, ADR-0069).

use pdo_skill::application::Upload;
use pdo_skill::domain::error::SkillError;
use pdo_skill::domain::seed::{
    SeedOutcome, INTERACTIVE_SKILL_ID, INTERACTIVE_SKILL_MD, SEEDED_FOLDER_ID, SEEDED_FOLDER_NAME,
    SEEDED_SKILL_ID, SEEDED_SKILL_MD,
};

use crate::fakes::{fixture, op, skill_md, Fixture};

/// The outcome of one seed pass for `pdo-orchestrate`.
async fn seed_orchestrate(f: &Fixture) -> SeedOutcome {
    f.skills
        .seed()
        .await
        .unwrap()
        .into_iter()
        .find(|(id, _)| *id == SEEDED_SKILL_ID)
        .map(|(_, outcome)| outcome)
        .expect("pdo-orchestrate is seeded")
}

#[tokio::test]
async fn first_seed_creates_row_folder_and_content() {
    let f = fixture();
    assert_eq!(seed_orchestrate(&f).await, SeedOutcome::Created);
    let skill = f.index.one(SEEDED_SKILL_ID).unwrap();
    assert_eq!(skill.name, "pdo-orchestrate");
    assert_eq!(f.folders.one(SEEDED_FOLDER_ID).unwrap().name, "PDO");
    assert_eq!(skill.folder_id.as_deref(), Some(SEEDED_FOLDER_ID));
    assert_eq!(
        f.content.text(SEEDED_SKILL_ID, "SKILL.md").unwrap(),
        SEEDED_SKILL_MD
    );
    // The seed is the daemon's own transition: no Autorisation d'opération.
    assert!(f.host.asked().is_empty());
}

#[tokio::test]
async fn the_seed_carries_a_list_both_toggle_skills_land_in_the_pdo_folder() {
    let f = fixture();
    let outcomes = f.skills.seed().await.unwrap();
    assert_eq!(
        outcomes,
        vec![
            (SEEDED_SKILL_ID, SeedOutcome::Created),
            (INTERACTIVE_SKILL_ID, SeedOutcome::Created),
        ]
    );
    let interactive = f.index.one(INTERACTIVE_SKILL_ID).unwrap();
    assert_eq!(interactive.name, "pdo-interactive");
    assert_eq!(interactive.folder_id.as_deref(), Some(SEEDED_FOLDER_ID));
    assert!(interactive.locked);
    assert_eq!(
        f.content.text(INTERACTIVE_SKILL_ID, "SKILL.md").unwrap(),
        INTERACTIVE_SKILL_MD
    );
}

#[tokio::test]
async fn a_second_seed_is_a_noop() {
    let f = fixture();
    f.skills.seed().await.unwrap();
    assert!(f
        .skills
        .seed()
        .await
        .unwrap()
        .iter()
        .all(|(_, outcome)| *outcome == SeedOutcome::Unchanged));
}

#[tokio::test]
async fn a_version_bump_rewrites_the_content_keeping_id_and_row() {
    let f = fixture();
    f.skills.seed().await.unwrap();
    let before = f.index.one(SEEDED_SKILL_ID).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(2));
    let bumped = SEEDED_SKILL_MD.replace("skill_version: 3", "skill_version: 4");
    let outcome = f
        .skills
        .seed_with(
            SEEDED_SKILL_ID,
            SEEDED_FOLDER_ID,
            SEEDED_FOLDER_NAME,
            &bumped,
        )
        .await
        .unwrap();
    assert_eq!(outcome, SeedOutcome::Updated);
    let after = f.index.one(SEEDED_SKILL_ID).unwrap();
    assert_eq!(after.id, before.id);
    assert_eq!(after.created_at, before.created_at);
    assert_eq!(after.folder_id.as_deref(), Some(SEEDED_FOLDER_ID));
    assert_ne!(after.updated_at, before.updated_at);
    assert_eq!(f.content.text(SEEDED_SKILL_ID, "SKILL.md").unwrap(), bumped);
}

#[tokio::test]
async fn an_erased_or_corrupted_folder_is_reseeded() {
    let f = fixture();
    f.skills.seed().await.unwrap();
    f.content.trees.lock().unwrap().remove(SEEDED_SKILL_ID);
    assert_eq!(seed_orchestrate(&f).await, SeedOutcome::Updated);
    assert_eq!(
        f.content.text(SEEDED_SKILL_ID, "SKILL.md").unwrap(),
        SEEDED_SKILL_MD
    );
    f.content
        .trees
        .lock()
        .unwrap()
        .get_mut(SEEDED_SKILL_ID)
        .unwrap()
        .insert("SKILL.md".into(), b"corrupted".to_vec());
    assert_eq!(seed_orchestrate(&f).await, SeedOutcome::Updated);
    assert_eq!(
        f.content.text(SEEDED_SKILL_ID, "SKILL.md").unwrap(),
        SEEDED_SKILL_MD
    );
}

#[tokio::test]
async fn a_user_skill_already_owning_the_label_is_left_alone() {
    let f = fixture();
    let user = f
        .skills
        .create_skill(&op(), &skill_md("tdd"), Some("pdo-orchestrate"), None)
        .await
        .unwrap();
    assert!(matches!(
        seed_orchestrate(&f).await,
        SeedOutcome::Skipped { .. }
    ));
    assert!(f.index.one(SEEDED_SKILL_ID).is_none());
    // The other seeded skill is not held hostage by the skipped one.
    assert!(f.index.one(INTERACTIVE_SKILL_ID).is_some());
    assert_eq!(f.index.one(&user.id).unwrap().name, "pdo-orchestrate");
}

#[tokio::test]
async fn an_existing_root_folder_named_pdo_is_adopted_not_duplicated() {
    let f = fixture();
    let existing = f.skills.create_folder(&op(), "PDO", None).await.unwrap();
    f.skills.seed().await.unwrap();
    let skill = f.index.one(SEEDED_SKILL_ID).unwrap();
    assert_eq!(skill.folder_id.as_deref(), Some(existing.id.as_str()));
    assert!(f.folders.one(SEEDED_FOLDER_ID).is_none());
}

#[tokio::test]
async fn the_seeded_skill_is_locked_against_every_bank_write() {
    let f = fixture();
    f.skills.seed().await.unwrap();
    let id = SEEDED_SKILL_ID;
    let locked = |r: Result<(), SkillError>| matches!(r, Err(SkillError::Locked { .. }));
    assert!(locked(f.skills.delete_skill(&op(), id).await.map(|_| ())));
    assert!(locked(
        f.skills
            .update_skill(&op(), id, Some("renamed"), None)
            .await
            .map(|_| ())
    ));
    assert!(locked(
        f.skills
            .save_file(&op(), id, "SKILL.md", &skill_md("tdd"))
            .await
            .map(|_| ())
    ));
    let upload = Upload {
        path: "notes.md".into(),
        data: b"x".to_vec(),
    };
    let outcome = f.skills.upload_files(&op(), id, vec![upload]).await;
    assert!(matches!(outcome.failure, Some(SkillError::Locked { .. })));
    assert!(locked(
        f.skills
            .save_file(&op(), id, "notes.md", "x")
            .await
            .map(|_| ())
    ));
    assert!(locked(f.skills.delete_file(&op(), id, "notes.md").await));
    // Still there after every refusal.
    assert!(f.index.one(id).is_some());
}
