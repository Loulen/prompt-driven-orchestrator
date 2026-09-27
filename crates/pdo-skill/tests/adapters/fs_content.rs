//! The bank's content on a real filesystem.

use pdo_skill::adapters::fs::{skill_dir, FsSkillContent};
use pdo_skill::application::ports::SkillContent;
use pdo_skill::domain::skill::SkillFile;

use crate::common::write_skill;

#[test]
fn folders_are_keyed_by_id_under_the_repo_pdo_dir() {
    let root = tempfile::tempdir().unwrap();
    let content = FsSkillContent::new(root.path());
    assert_eq!(content.root(), root.path().join(".pdo").join("skills"));
    assert_eq!(content.dir("s1"), skill_dir(root.path(), "s1"));
    content.write_skill_md("s1", "text").unwrap();
    assert_eq!(content.read_skill_md("s1").unwrap(), "text");
    assert!(content.read_skill_md("nope").is_err());
}

#[test]
fn list_files_excludes_skill_md_and_walks_subdirs() {
    let root = tempfile::tempdir().unwrap();
    let content = FsSkillContent::new(root.path());
    content.write_skill_md("s1", "x").unwrap();
    let dir = content.dir("s1");
    std::fs::write(dir.join("checklist.md"), "abc").unwrap();
    std::fs::create_dir_all(dir.join("ref")).unwrap();
    std::fs::write(dir.join("ref").join("a.txt"), "12345").unwrap();
    assert_eq!(
        content.list_files("s1").unwrap(),
        vec![
            SkillFile {
                path: "checklist.md".into(),
                size: 3
            },
            SkillFile {
                path: "ref/a.txt".into(),
                size: 5
            },
        ]
    );
    assert!(content.list_files("absent").unwrap().is_empty());
}

#[test]
fn write_read_delete_a_reference_file_and_prune_empty_subfolders() {
    let root = tempfile::tempdir().unwrap();
    let content = FsSkillContent::new(root.path());
    content.write_skill_md("s1", "x").unwrap();
    let dir = content.dir("s1");
    content
        .write_file("s1", "examples/deep/login.spec.ts", b"test()")
        .unwrap();
    assert!(content.is_file("s1", "examples/deep/login.spec.ts"));
    assert_eq!(
        content
            .read_file("s1", "examples/deep/login.spec.ts")
            .unwrap(),
        Some(b"test()".to_vec())
    );
    assert_eq!(content.read_file("s1", "examples").unwrap(), None);
    content
        .delete_file("s1", "examples/deep/login.spec.ts")
        .unwrap();
    assert!(
        !dir.join("examples").exists(),
        "the emptied sub-folders are pruned"
    );
    assert!(
        dir.join("SKILL.md").is_file(),
        "the skill folder itself stays"
    );
    content.remove("s1").unwrap();
    assert!(!dir.exists());
    content.remove("s1").unwrap();
}

#[test]
fn copy_and_replace_from_a_source_folder() {
    let root = tempfile::tempdir().unwrap();
    let src = tempfile::tempdir().unwrap();
    let content = FsSkillContent::new(root.path());
    write_skill(&src.path().join("tdd"), "tdd");
    std::fs::create_dir_all(src.path().join("tdd").join(".git")).unwrap();
    content.copy_from("s1", &src.path().join("tdd")).unwrap();
    assert!(content.is_file("s1", "references/notes.md"));
    assert!(
        !content.dir("s1").join(".git").exists(),
        ".git is never copied"
    );
    let before = content.hashes("s1");
    assert_eq!(before.len(), 2);

    write_skill(&src.path().join("v2"), "tdd");
    std::fs::write(src.path().join("v2").join("new.md"), "n").unwrap();
    content.replace_from("s1", &src.path().join("v2")).unwrap();
    assert!(content.is_file("s1", "new.md"));
    assert!(!root.path().join(".pdo/skills/.s1.replacing").exists());

    // A failed copy restores the previous content and names both paths.
    let err = content
        .replace_from("s1", &src.path().join("missing"))
        .unwrap_err();
    assert!(err.to_string().starts_with("failed to copy "), "{err}");
    assert!(content.is_file("s1", "new.md"));
    let err = content
        .copy_from("s2", &src.path().join("missing"))
        .unwrap_err();
    assert!(err.to_string().contains(" into "), "{err}");
    assert!(!content.dir("s2").exists());
}

#[test]
fn host_files_are_read_only_when_regular() {
    let root = tempfile::tempdir().unwrap();
    let content = FsSkillContent::new(root.path());
    let file = root.path().join("sheet.md");
    std::fs::write(&file, "# sheet").unwrap();
    assert_eq!(content.host_file_size(&file), Some(7));
    assert_eq!(content.host_file_size(root.path()), None);
    assert_eq!(content.read_host_file(&file).unwrap(), b"# sheet");
}
