//! Provisioning on disk: resolve rules over a real source repository and
//! materialize them into a real worktree (ADR-0061).

use pdo_workspace::adapters::fs::provisioner::{
    provision_missing, provision_node_worktree, resolve,
};
use pdo_workspace::adapters::git::manifest::{materialized_paths, record_materialized_paths};
use pdo_workspace::domain::provisioning::{
    ProvisioningMode, ProvisioningRules, ProvisioningScope, ScopedRules,
};

#[test]
fn specific_rules_override_modes_exclude_paths_and_report_conflicts() {
    let repo = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(repo.path().join("fixtures")).unwrap();
    std::fs::write(repo.path().join(".env"), "secret").unwrap();
    std::fs::write(repo.path().join("fixtures/a.bin"), "a").unwrap();
    std::fs::write(repo.path().join("fixtures/b.bin"), "b").unwrap();

    let plan = resolve(
        repo.path(),
        &[
            ScopedRules {
                scope: ProvisioningScope::Instance,
                rules: ProvisioningRules {
                    copy: vec![".env".into(), "fixtures/**".into()],
                    ..Default::default()
                },
            },
            ScopedRules {
                scope: ProvisioningScope::Run,
                rules: ProvisioningRules {
                    copy: vec![".env".into()],
                    hardlink: vec!["fixtures/**".into(), "!fixtures/b.bin".into()],
                    symlink: vec![".env".into()],
                },
            },
        ],
    )
    .unwrap();

    assert_eq!(plan.conflicts[0].relative_path, ".env");
    assert_eq!(
        plan.entries
            .iter()
            .map(|entry| (&entry.relative_path, entry.mode))
            .collect::<Vec<_>>(),
        vec![(&"fixtures/a.bin".to_string(), ProvisioningMode::Hardlink)]
    );
    let inherited = plan
        .rules
        .iter()
        .find(|rule| rule.scope == ProvisioningScope::Instance && rule.pattern == "fixtures/**")
        .unwrap();
    assert!(inherited.paths.contains(&"fixtures/a.bin".to_string()));
    assert!(inherited.excluded_paths.iter().any(|excluded| {
        excluded.relative_path == "fixtures/b.bin"
            && excluded.excluded_by_scope == ProvisioningScope::Run
    }));
}

#[test]
fn directory_patterns_include_nested_files_recursively() {
    let repo = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(repo.path().join("fixtures/nested")).unwrap();
    std::fs::write(repo.path().join("fixtures/nested/data.bin"), "data").unwrap();
    std::fs::write(repo.path().join("outside.bin"), "outside").unwrap();

    let plan = resolve(
        repo.path(),
        &[ScopedRules {
            scope: ProvisioningScope::Run,
            rules: ProvisioningRules {
                copy: vec!["fixtures/".into()],
                ..Default::default()
            },
        }],
    )
    .unwrap();

    assert_eq!(
        plan.entries
            .iter()
            .map(|entry| entry.relative_path.as_str())
            .collect::<Vec<_>>(),
        vec!["fixtures/nested/data.bin"]
    );
}

#[cfg(unix)]
#[test]
fn symlink_directory_pattern_creates_one_directory_link() {
    let repo = tempfile::tempdir().unwrap();
    let worktree = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(repo.path().join("node_modules/left-pad")).unwrap();
    std::fs::write(
        repo.path().join("node_modules/left-pad/index.js"),
        "module.exports = 1;",
    )
    .unwrap();

    let plan = resolve(
        repo.path(),
        &[ScopedRules {
            scope: ProvisioningScope::Run,
            rules: ProvisioningRules {
                symlink: vec!["node_modules".into()],
                ..Default::default()
            },
        }],
    )
    .unwrap();

    assert_eq!(
        plan.entries
            .iter()
            .map(|entry| entry.relative_path.as_str())
            .collect::<Vec<_>>(),
        vec!["node_modules"]
    );
    provision_missing(repo.path(), worktree.path(), &plan).unwrap();
    assert!(
        std::fs::symlink_metadata(worktree.path().join("node_modules"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[test]
fn plan_marks_paths_already_provided_by_git() {
    let repo = tempfile::tempdir().unwrap();
    let git = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(repo.path())
            .output()
            .unwrap();
        assert!(output.status.success());
    };
    git(&["init", "--quiet"]);
    git(&["config", "user.email", "test@test.com"]);
    git(&["config", "user.name", "Test"]);
    std::fs::write(repo.path().join("tracked.txt"), "tracked").unwrap();
    git(&["add", "tracked.txt"]);
    git(&["commit", "--quiet", "-m", "tracked"]);
    std::fs::write(repo.path().join("untracked.txt"), "untracked").unwrap();

    let plan = resolve(
        repo.path(),
        &[ScopedRules {
            scope: ProvisioningScope::Run,
            rules: ProvisioningRules {
                copy: vec!["*.txt".into()],
                ..Default::default()
            },
        }],
    )
    .unwrap();

    assert!(
        plan.entries
            .iter()
            .find(|entry| entry.relative_path == "tracked.txt")
            .unwrap()
            .provided_by_git
    );
    assert!(
        !plan
            .entries
            .iter()
            .find(|entry| entry.relative_path == "untracked.txt")
            .unwrap()
            .provided_by_git
    );
}

#[test]
fn provisioning_is_additive_and_preserves_source_symlinks() {
    let repo = tempfile::tempdir().unwrap();
    let worktree = tempfile::tempdir().unwrap();
    std::fs::write(repo.path().join("copy.txt"), "source").unwrap();
    std::fs::write(repo.path().join("hard.txt"), "hard").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink("copy.txt", repo.path().join("source-link")).unwrap();
    std::fs::write(worktree.path().join("copy.txt"), "git").unwrap();
    let rules = ProvisioningRules {
        copy: vec!["copy.txt".into(), "source-link".into()],
        hardlink: vec!["hard.txt".into()],
        symlink: vec![],
    };
    let plan = resolve(
        repo.path(),
        &[ScopedRules {
            scope: ProvisioningScope::Run,
            rules,
        }],
    )
    .unwrap();

    provision_missing(repo.path(), worktree.path(), &plan).unwrap();

    assert_eq!(
        std::fs::read_to_string(worktree.path().join("copy.txt")).unwrap(),
        "git"
    );
    assert_eq!(
        std::fs::read_to_string(worktree.path().join("hard.txt")).unwrap(),
        "hard"
    );
    #[cfg(unix)]
    assert!(
        std::fs::symlink_metadata(worktree.path().join("source-link"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[test]
fn a_node_worktree_is_provisioned_and_its_paths_recorded_in_the_manifest() {
    let repo = tempfile::tempdir().unwrap();
    crate::common::init_test_repo(repo.path());
    std::fs::write(repo.path().join(".env"), "secret").unwrap();
    let worktree = repo.path().join("wt");
    crate::common::run_git(
        repo.path(),
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "node",
            worktree.to_str().unwrap(),
        ],
    );

    let plan = provision_node_worktree(
        repo.path(),
        &worktree,
        &[],
        &ProvisioningRules {
            copy: vec![".env".into()],
            ..Default::default()
        },
        "HEAD",
    )
    .unwrap();
    assert_eq!(
        plan.entries[0].origin_scope,
        ProvisioningScope::IsolatedNode
    );
    assert_eq!(
        std::fs::read_to_string(worktree.join(".env")).unwrap(),
        "secret"
    );
    assert_eq!(
        materialized_paths(&worktree).unwrap(),
        vec![b".env".to_vec()]
    );

    record_materialized_paths(&worktree, &["other".to_string()]).unwrap();
    assert_eq!(
        materialized_paths(&worktree).unwrap(),
        vec![b".env".to_vec(), b"other".to_vec()]
    );
}

#[test]
fn outside_a_git_worktree_the_manifest_is_empty_and_recording_is_a_noop() {
    let dir = tempfile::tempdir().unwrap();
    record_materialized_paths(dir.path(), &["x".to_string()]).unwrap();
    assert!(materialized_paths(dir.path()).unwrap().is_empty());
}
