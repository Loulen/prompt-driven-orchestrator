//! The seed of the extensions' skills at startup (#33, ADR-0081).

use pdo_skill::application::Upload;
use pdo_skill::domain::error::SkillError;
use pdo_skill::domain::extension_skill::{extension_folder_id, ExtensionSkillSeed};
use pdo_skill::domain::seed::SeedOutcome;
use pdo_skill::domain::skill::ProvidedBy;

use crate::fakes::{fixture, op, skill_md};

fn seed(name: &str, version: &str, files: &[(&str, &str)]) -> ExtensionSkillSeed {
    ExtensionSkillSeed {
        extension: ProvidedBy {
            id: "test".into(),
            version: version.into(),
        },
        extension_name: "Test extension".into(),
        name: name.into(),
        skill_md: skill_md(name),
        files: files
            .iter()
            .map(|(path, body)| (path.to_string(), body.as_bytes().to_vec()))
            .collect(),
    }
}

#[tokio::test]
async fn an_extension_skill_lands_locked_in_the_folder_of_its_extension() {
    let f = fixture();
    let skills = [seed("test-cli", "1.0.0", &[("references/cli.md", "# CLI")])];
    let outcomes = f.skills.seed_extension("test", &skills).await.unwrap();
    assert_eq!(
        outcomes,
        vec![("ext-test-test-cli".to_string(), SeedOutcome::Created)]
    );
    let skill = f.index.one("ext-test-test-cli").unwrap();
    assert_eq!(skill.name, "test-cli");
    assert!(skill.locked);
    assert_eq!(
        skill.extension,
        Some(ProvidedBy {
            id: "test".into(),
            version: "1.0.0".into()
        })
    );
    let folder = extension_folder_id("test");
    assert_eq!(skill.folder_id.as_deref(), Some(folder.as_str()));
    assert_eq!(f.folders.one(&folder).unwrap().name, "Test extension");
    assert_eq!(
        f.content
            .text("ext-test-test-cli", "references/cli.md")
            .unwrap(),
        "# CLI"
    );
    assert!(f.host.asked().is_empty(), "the seed asks no authorization");
}

#[tokio::test]
async fn the_update_follows_the_version_of_the_extension() {
    let f = fixture();
    let v1 = [seed("test-cli", "1.0.0", &[("a.md", "one")])];
    f.skills.seed_extension("test", &v1).await.unwrap();
    let again = f.skills.seed_extension("test", &v1).await.unwrap();
    assert_eq!(again[0].1, SeedOutcome::Unchanged);

    // A new version of the extension rewrites the skill, content and all.
    let v2 = [seed("test-cli", "1.1.0", &[("b.md", "two")])];
    let outcomes = f.skills.seed_extension("test", &v2).await.unwrap();
    assert_eq!(outcomes[0].1, SeedOutcome::Updated);
    let skill = f.index.one("ext-test-test-cli").unwrap();
    assert_eq!(skill.extension.unwrap().version, "1.1.0");
    assert!(f.content.text("ext-test-test-cli", "a.md").is_none());
    assert_eq!(f.content.text("ext-test-test-cli", "b.md").unwrap(), "two");

    // A touched copy is restored, even at the same version.
    f.content
        .trees
        .lock()
        .unwrap()
        .get_mut("ext-test-test-cli")
        .unwrap()
        .insert("b.md".into(), b"edited".to_vec());
    let outcomes = f.skills.seed_extension("test", &v2).await.unwrap();
    assert_eq!(outcomes[0].1, SeedOutcome::Updated);
    assert_eq!(f.content.text("ext-test-test-cli", "b.md").unwrap(), "two");
}

#[tokio::test]
async fn a_skill_the_extension_no_longer_provides_is_removed() {
    let f = fixture();
    let both = [seed("keep", "1.0.0", &[]), seed("drop", "1.0.0", &[])];
    f.skills.seed_extension("test", &both).await.unwrap();
    let outcomes = f
        .skills
        .seed_extension("test", &[seed("keep", "1.0.1", &[])])
        .await
        .unwrap();
    assert_eq!(
        outcomes,
        vec![
            ("ext-test-keep".to_string(), SeedOutcome::Updated),
            ("ext-test-drop".to_string(), SeedOutcome::Removed),
        ]
    );
    assert!(f.index.one("ext-test-drop").is_none());
    assert!(f.content.text("ext-test-drop", "SKILL.md").is_none());
    // Another extension's skills are none of this seed's business.
    f.skills
        .seed_extension("other", &[])
        .await
        .unwrap()
        .is_empty()
        .then_some(())
        .unwrap();
    assert!(f.index.one("ext-test-keep").is_some());
}

#[tokio::test]
async fn a_skill_that_does_not_pass_the_gate_is_skipped_alone() {
    let f = fixture();
    let mut misnamed = seed("one", "1.0.0", &[]);
    misnamed.skill_md = skill_md("other");
    let escaping = seed("two", "1.0.0", &[("../x", "x")]);
    let user = f
        .skills
        .create_skill(&op(), &skill_md("three"), None, None)
        .await
        .unwrap();
    let skills = [
        misnamed,
        escaping,
        seed("three", "1.0.0", &[]),
        seed("four", "1.0.0", &[]),
    ];
    let outcomes = f.skills.seed_extension("test", &skills).await.unwrap();
    let skipped = |i: usize| matches!(outcomes[i].1, SeedOutcome::Skipped { .. });
    assert!(skipped(0) && skipped(1) && skipped(2), "{outcomes:?}");
    assert_eq!(outcomes[3].1, SeedOutcome::Created);
    assert_eq!(f.index.one(&user.id).unwrap().name, "three");
}

#[tokio::test]
async fn an_extension_skill_is_locked_against_every_bank_write() {
    let f = fixture();
    f.skills
        .seed_extension("test", &[seed("test-cli", "1.0.0", &[("a.md", "x")])])
        .await
        .unwrap();
    let id = "ext-test-test-cli";
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
            .save_file(&op(), id, "SKILL.md", &skill_md("test-cli"))
            .await
            .map(|_| ())
    ));
    assert!(locked(
        f.skills.save_file(&op(), id, "a.md", "y").await.map(|_| ())
    ));
    assert!(locked(f.skills.delete_file(&op(), id, "a.md").await));
    let upload = Upload {
        path: "notes.md".into(),
        data: b"x".to_vec(),
    };
    let outcome = f.skills.upload_files(&op(), id, vec![upload]).await;
    assert!(matches!(outcome.failure, Some(SkillError::Locked { .. })));
    assert_eq!(f.content.text(id, "a.md").unwrap(), "x");
}

#[tokio::test]
async fn an_extension_that_now_provides_no_skill_loses_the_ones_it_provided() {
    let f = fixture();
    f.skills
        .seed_extension("test", &[seed("test-cli", "1.0.0", &[])])
        .await
        .unwrap();
    let outcomes = f.skills.seed_extension("test", &[]).await.unwrap();
    assert_eq!(
        outcomes,
        vec![("ext-test-test-cli".to_string(), SeedOutcome::Removed)]
    );
    assert!(f.index.one("ext-test-test-cli").is_none());
}
