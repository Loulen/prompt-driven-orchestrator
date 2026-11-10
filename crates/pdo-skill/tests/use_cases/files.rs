//! A skill's reference files and the `SKILL.md` editor (#671).

use std::path::Path;

use pdo_skill::application::{SavedFile, Upload};
use pdo_skill::domain::error::{SkillError, MAX_FILE_BYTES};

use crate::fakes::{fixture, op, skill_md, Fixture};

async fn with_skill() -> (Fixture, String) {
    let f = fixture();
    let skill = f
        .skills
        .create_skill(&op(), &skill_md("tdd"), None, None)
        .await
        .unwrap();
    (f, skill.id)
}

fn upload(path: &str, data: &[u8]) -> Upload {
    Upload {
        path: path.into(),
        data: data.to_vec(),
    }
}

#[tokio::test]
async fn upload_read_save_and_delete_a_reference_file() {
    let (f, id) = with_skill().await;
    let outcome = f
        .skills
        .upload_files(
            &op(),
            &id,
            vec![upload("examples/login.spec.ts", b"test()")],
        )
        .await;
    assert!(outcome.failure.is_none());
    assert_eq!(outcome.uploaded[0].path, "examples/login.spec.ts");
    assert_eq!(outcome.uploaded[0].size, 6);

    let file = f
        .skills
        .read_file(&id, "examples/login.spec.ts")
        .await
        .unwrap();
    assert_eq!(file.size, 6);
    assert_eq!(file.text.as_deref(), Some("test()"));

    let saved = f
        .skills
        .save_file(&op(), &id, "examples/login.spec.ts", "edited")
        .await
        .unwrap();
    assert!(matches!(saved, SavedFile::Reference(ref file) if file.size == 6));
    assert_eq!(
        f.content.text(&id, "examples/login.spec.ts").unwrap(),
        "edited"
    );
    assert!(matches!(
        f.skills.save_file(&op(), &id, "missing.md", "x").await,
        Err(SkillError::FileNotFound(_))
    ));

    f.skills
        .delete_file(&op(), &id, "examples/login.spec.ts")
        .await
        .unwrap();
    assert!(f.skills.files(&id).is_empty());
    assert!(matches!(
        f.skills
            .delete_file(&op(), &id, "examples/login.spec.ts")
            .await,
        Err(SkillError::FileNotFound(_))
    ));
}

#[tokio::test]
async fn the_first_refusal_stops_the_batch_and_keeps_what_landed() {
    let (f, id) = with_skill().await;
    let outcome = f
        .skills
        .upload_files(
            &op(),
            &id,
            vec![
                upload("a.md", b"a"),
                upload("../escape.md", b"x"),
                upload("b.md", b"b"),
            ],
        )
        .await;
    assert_eq!(outcome.uploaded.len(), 1);
    assert!(matches!(outcome.failure, Some(SkillError::InvalidPath(_))));
    assert!(!f.content.is_file_named(&id, "b.md"));
}

#[tokio::test]
async fn skill_md_is_reserved_for_the_files_endpoints_but_readable() {
    let (f, id) = with_skill().await;
    let outcome = f
        .skills
        .upload_files(&op(), &id, vec![upload("SKILL.md", b"x")])
        .await;
    assert_eq!(outcome.failure, Some(SkillError::SkillMdReserved));
    assert_eq!(
        f.skills
            .delete_file(&op(), &id, "SKILL.md")
            .await
            .unwrap_err(),
        SkillError::SkillMdReserved
    );
    let file = f.skills.read_file(&id, "SKILL.md").await.unwrap();
    assert_eq!(file.text.unwrap(), skill_md("tdd"));
}

#[tokio::test]
async fn saving_skill_md_revalidates_and_writes_nothing_on_refusal() {
    let (f, id) = with_skill().await;
    let err = f
        .skills
        .save_file(&op(), &id, "SKILL.md", "no frontmatter")
        .await
        .unwrap_err();
    assert_eq!(err, SkillError::NoFrontmatter);
    assert_eq!(f.content.text(&id, "SKILL.md").unwrap(), skill_md("tdd"));

    let edited = skill_md("tdd").replace("The tdd method.", "Edited.");
    let saved = f
        .skills
        .save_file(&op(), &id, "SKILL.md", &edited)
        .await
        .unwrap();
    let SavedFile::SkillMd { size, skill } = saved else {
        panic!("a SKILL.md save answers with the skill");
    };
    assert_eq!(size, edited.len());
    assert_eq!(skill.description, "Edited.");
    assert_eq!(skill.name, "tdd", "the label is not renamed by an edit");
    assert_eq!(f.content.text(&id, "SKILL.md").unwrap(), edited);
}

#[tokio::test]
async fn copying_a_host_file_refuses_folders_relative_paths_and_oversize() {
    let (f, id) = with_skill().await;
    f.content
        .host
        .lock()
        .unwrap()
        .insert("/home/me/cheatsheet.md".into(), b"# sheet".to_vec());
    let file = f
        .skills
        .copy_host_file(&op(), &id, Path::new("/home/me/cheatsheet.md"), None)
        .await
        .unwrap();
    assert_eq!(file.path, "cheatsheet.md");
    assert_eq!(file.size, 7);
    assert!(matches!(
        f.skills
            .copy_host_file(&op(), &id, Path::new("/home/me"), None)
            .await,
        Err(SkillError::SourceNotAFile(_))
    ));
    assert!(matches!(
        f.skills.copy_host_file(&op(), &id, Path::new("rel.md"), None).await,
        Err(SkillError::SourceNotAFile(p)) if p == "rel.md"
    ));
    let outcome = f
        .skills
        .upload_files(
            &op(),
            &id,
            vec![upload("big.bin", &vec![0u8; MAX_FILE_BYTES as usize + 1])],
        )
        .await;
    assert!(matches!(
        outcome.failure,
        Some(SkillError::FileTooLarge { .. })
    ));
}

#[tokio::test]
async fn files_of_an_unknown_skill_are_not_found() {
    let f = fixture();
    assert_eq!(
        f.skills.read_file("nope", "a.md").await.unwrap_err(),
        SkillError::NotFound
    );
    assert_eq!(
        f.skills
            .save_file(&op(), "nope", "a.md", "x")
            .await
            .unwrap_err(),
        SkillError::NotFound
    );
    assert_eq!(
        f.skills
            .delete_file(&op(), "nope", "a.md")
            .await
            .unwrap_err(),
        SkillError::NotFound
    );
}
