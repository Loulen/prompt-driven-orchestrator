//! Scanning a Source: a local folder in place, a git repository cloned
//! shallow through the workspace's git adapter.

use pdo_skill::adapters::source::{scan_dir, GitSourceScanner};
use pdo_skill::application::ports::SourceScanner;
use pdo_skill::domain::import_error::ImportError;
use pdo_skill::domain::source::parse_source;

use crate::common::{git, init_repo, write_skill};

#[test]
fn scan_finds_nested_skills_and_flags_invalid_ones() {
    let root = tempfile::tempdir().unwrap();
    let mk = |rel: &str, content: &str| {
        let dir = root.path().join(rel);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), content).unwrap();
        dir
    };
    let pdf = mk(
        "skills/engineering/pdf",
        "---\nname: pdf\ndescription: Extract text.\n---\n\nbody\n",
    );
    std::fs::write(pdf.join("ref.md"), "x").unwrap();
    std::fs::create_dir_all(pdf.join("nested")).unwrap();
    std::fs::write(
        pdf.join("nested").join("SKILL.md"),
        "not a skill: inside a skill",
    )
    .unwrap();
    mk("skills/engineering/bad", "---\nname: bad\n---\n\nbody\n");
    mk(
        ".agents/skills/hidden-ok",
        "---\nname: hidden-ok\ndescription: d\n---\n\nbody\n",
    );
    std::fs::create_dir_all(root.path().join(".git").join("x")).unwrap();
    std::fs::write(
        root.path().join(".git").join("x").join("SKILL.md"),
        "---\nname: git\ndescription: d\n---\nb",
    )
    .unwrap();

    let all = scan_dir(root.path(), "");
    let paths: Vec<&str> = all.iter().map(|c| c.path.as_str()).collect();
    assert_eq!(
        paths,
        vec![
            ".agents/skills/hidden-ok",
            "skills/engineering/bad",
            "skills/engineering/pdf"
        ]
    );
    let pdf = all.iter().find(|c| c.name == "pdf").unwrap();
    assert!(pdf.valid);
    // The nested SKILL.md is a reference file of `pdf`, not a skill.
    assert_eq!(pdf.file_count, 2);
    let bad = all.iter().find(|c| c.path.ends_with("bad")).unwrap();
    assert!(!bad.valid);
    assert_eq!(bad.code.as_deref(), Some("missing_description"));

    assert_eq!(scan_dir(root.path(), "skills/engineering").len(), 2);
    assert!(scan_dir(root.path(), "docs").is_empty());
}

#[tokio::test]
async fn a_local_folder_is_scanned_in_place_and_kept() {
    let root = tempfile::tempdir().unwrap();
    write_skill(&root.path().join("skills").join("tdd"), "tdd");
    let scanner = GitSourceScanner::new();
    let source = parse_source(root.path().to_str().unwrap(), None).unwrap();
    let outcome = scanner.scan("local-1", &source).await.unwrap();
    assert_eq!(outcome.candidates.len(), 1);
    assert_eq!(outcome.commit, None, "a plain folder has no commit");
    let kept = scanner.kept("local-1").unwrap();
    assert_eq!(kept.root, root.path());
    let dir = scanner.candidate_dir(&kept.root, "skills/tdd").unwrap();
    assert!(scanner.read_skill_md(&dir).unwrap().contains("name: tdd"));
    assert_eq!(scanner.hashes(&dir).len(), 2);
    assert!(matches!(
        scanner.candidate_dir(&kept.root, "skills/../skills/tdd"),
        Err(ImportError::UnknownCandidate(_))
    ));
    assert!(matches!(
        scanner.candidate_dir(&kept.root, "skills"),
        Err(ImportError::UnknownCandidate(_))
    ));
    // A local folder is never deleted with its scan.
    assert!(root.path().join("skills").is_dir());

    let missing = parse_source("/definitely/not/here", None).unwrap();
    assert_eq!(
        scanner.scan("local-2", &missing).await.unwrap_err(),
        ImportError::LocalNotFound("/definitely/not/here".into())
    );
}

#[tokio::test]
async fn a_git_source_is_cloned_shallow_at_its_commit() {
    let origin = tempfile::tempdir().unwrap();
    init_repo(origin.path());
    write_skill(&origin.path().join("skills").join("pdf"), "pdf");
    git(origin.path(), &["add", "."]);
    git(origin.path(), &["commit", "-q", "-m", "skills"]);
    let head = git(origin.path(), &["rev-parse", "HEAD"])
        .trim()
        .to_string();

    let scanner = GitSourceScanner::new();
    let url = format!("file://{}", origin.path().display());
    let source = parse_source(&url, None).unwrap();
    let scan_id = format!("git-{}", std::process::id());
    let outcome = scanner.scan(&scan_id, &source).await.unwrap();
    assert_eq!(outcome.commit.as_deref(), Some(head.as_str()));
    assert_eq!(outcome.candidates[0].path, "skills/pdf");
    let kept = scanner.kept(&scan_id).unwrap();
    assert_ne!(kept.root, origin.path(), "a clone, not the origin");

    // Nothing is in flight any more: a cancel finds nothing to kill.
    assert!(!scanner.cancel(&scan_id));

    let broken = parse_source("file:///definitely/not/a/repo.git", None).unwrap();
    assert!(matches!(
        scanner.scan(&format!("{scan_id}-broken"), &broken).await,
        Err(ImportError::CloneFailed(_))
    ));
}
