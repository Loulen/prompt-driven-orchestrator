use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Context, Result};
use ignore::gitignore::GitignoreBuilder;
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProvisioningScope {
    Instance,
    Project,
    Run,
    IsolatedNode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ProvisioningMode {
    Copy,
    Hardlink,
    Symlink,
}

impl std::fmt::Display for ProvisioningMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Copy => write!(f, "copy"),
            Self::Hardlink => write!(f, "hardlink"),
            Self::Symlink => write!(f, "symlink"),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ProvisioningRules {
    #[serde(default)]
    pub copy: Vec<String>,
    #[serde(default)]
    pub hardlink: Vec<String>,
    #[serde(default)]
    pub symlink: Vec<String>,
}

impl ProvisioningRules {
    fn patterns(&self) -> impl Iterator<Item = (ProvisioningMode, &str)> {
        self.copy
            .iter()
            .map(|p| (ProvisioningMode::Copy, p.as_str()))
            .chain(
                self.hardlink
                    .iter()
                    .map(|p| (ProvisioningMode::Hardlink, p.as_str())),
            )
            .chain(
                self.symlink
                    .iter()
                    .map(|p| (ProvisioningMode::Symlink, p.as_str())),
            )
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.copy.is_empty() && self.hardlink.is_empty() && self.symlink.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ScopedRules {
    pub scope: ProvisioningScope,
    #[serde(default)]
    pub rules: ProvisioningRules,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ProvisioningEntry {
    pub relative_path: String,
    pub mode: ProvisioningMode,
    pub origin_scope: ProvisioningScope,
    pub pattern: String,
    pub provided_by_git: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ExcludedPathPreview {
    pub relative_path: String,
    pub excluded_by_scope: ProvisioningScope,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RulePreview {
    pub scope: ProvisioningScope,
    pub mode: ProvisioningMode,
    pub pattern: String,
    pub paths: Vec<String>,
    pub excluded_paths: Vec<ExcludedPathPreview>,
    pub unmatched: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ModeConflict {
    pub scope: ProvisioningScope,
    pub relative_path: String,
    pub modes: Vec<ProvisioningMode>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ProvisioningPlan {
    pub entries: Vec<ProvisioningEntry>,
    pub rules: Vec<RulePreview>,
    pub conflicts: Vec<ModeConflict>,
}

fn repository_paths(repository: &Path) -> Result<Vec<(String, bool)>> {
    fn visit(root: &Path, dir: &Path, out: &mut Vec<(String, bool)>) -> Result<()> {
        for entry in std::fs::read_dir(dir)
            .with_context(|| format!("read provisioning source {}", dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            let rel = path
                .strip_prefix(root)
                .expect("walk stays below root")
                .to_string_lossy()
                .replace('\\', "/");
            if dir == root && (rel == ".git" || rel == ".pdo") {
                continue;
            }
            let ty = entry.file_type()?;
            if ty.is_dir() {
                visit(root, &path, out)?;
            } else {
                out.push((rel, false));
            }
        }
        Ok(())
    }

    let mut paths = Vec::new();
    visit(repository, repository, &mut paths)?;
    paths.sort();
    Ok(paths)
}

fn git_tracked_paths(repository: &Path, git_ref: &str) -> Result<BTreeSet<String>> {
    if !repository.join(".git").exists() {
        return Ok(BTreeSet::new());
    }
    let output = std::process::Command::new("git")
        .args(["ls-tree", "-r", "--name-only", "-z", git_ref])
        .current_dir(repository)
        .output()
        .with_context(|| format!("list Git-provided paths in {}", repository.display()))?;
    if !output.status.success() {
        anyhow::bail!(
            "list Git-provided paths in {}: {}",
            repository.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| String::from_utf8_lossy(path).replace('\\', "/"))
        .collect())
}

fn matches_pattern(repository: &Path, pattern: &str, path: &str, is_dir: bool) -> Result<bool> {
    let mut builder = GitignoreBuilder::new(repository);
    let normalized = pattern.strip_prefix('!').unwrap_or(pattern).trim();
    builder
        .add_line(None, normalized)
        .with_context(|| format!("invalid provisioning pattern `{pattern}`"))?;
    let matcher = builder
        .build()
        .with_context(|| format!("invalid provisioning pattern `{pattern}`"))?;
    Ok(matcher
        .matched_path_or_any_parents(Path::new(path), is_dir)
        .is_ignore())
}

#[cfg(test)]
pub(crate) fn resolve(repository: &Path, scoped: &[ScopedRules]) -> Result<ProvisioningPlan> {
    resolve_at_git_ref(repository, scoped, "HEAD")
}

pub(crate) fn resolve_at_git_ref(
    repository: &Path,
    scoped: &[ScopedRules],
    git_ref: &str,
) -> Result<ProvisioningPlan> {
    let paths = repository_paths(repository)?;
    let git_tracked = git_tracked_paths(repository, git_ref)?;
    let mut final_entries: BTreeMap<String, ProvisioningEntry> = BTreeMap::new();
    let mut previews = Vec::new();
    let mut conflicts = Vec::new();

    for level in scoped {
        let mut positives: BTreeMap<String, Vec<(ProvisioningMode, String)>> = BTreeMap::new();
        let mut exclusions = BTreeSet::new();
        let mut level_previews = Vec::new();

        for (mode, raw_pattern) in level.rules.patterns() {
            let pattern = raw_pattern.trim();
            if pattern.is_empty() {
                continue;
            }
            let excluded = pattern.starts_with('!');
            let mut matched = Vec::new();
            for (path, is_dir) in &paths {
                if matches_pattern(repository, pattern, path, *is_dir)? {
                    matched.push(path.clone());
                }
            }
            if excluded {
                exclusions.extend(matched.iter().cloned());
            } else {
                for path in &matched {
                    positives
                        .entry(path.clone())
                        .or_default()
                        .push((mode, pattern.to_string()));
                }
            }
            level_previews.push(RulePreview {
                scope: level.scope,
                mode,
                pattern: pattern.to_string(),
                paths: if excluded {
                    Vec::new()
                } else {
                    matched.clone()
                },
                excluded_paths: if excluded {
                    matched
                        .iter()
                        .map(|relative_path| ExcludedPathPreview {
                            relative_path: relative_path.clone(),
                            excluded_by_scope: level.scope,
                        })
                        .collect()
                } else {
                    Vec::new()
                },
                unmatched: matched.is_empty(),
            });
        }

        for path in exclusions {
            final_entries.remove(&path);
            for preview in previews.iter_mut().chain(level_previews.iter_mut()) {
                if preview.paths.contains(&path) {
                    preview.paths.retain(|p| p != &path);
                    preview.excluded_paths.push(ExcludedPathPreview {
                        relative_path: path.clone(),
                        excluded_by_scope: level.scope,
                    });
                }
            }
        }

        for (path, declarations) in positives {
            if declarations.iter().any(|_| {
                level_previews.iter().any(|preview| {
                    preview
                        .excluded_paths
                        .iter()
                        .any(|excluded| excluded.relative_path == path)
                })
            }) {
                continue;
            }
            let modes: BTreeSet<ProvisioningMode> =
                declarations.iter().map(|(mode, _)| *mode).collect();
            if modes.len() > 1 {
                final_entries.remove(&path);
                conflicts.push(ModeConflict {
                    scope: level.scope,
                    relative_path: path,
                    modes: modes.into_iter().collect(),
                });
                continue;
            }
            let (mode, pattern) = declarations.last().expect("positive declaration");
            let provided_by_git = git_tracked.contains(&path);
            final_entries.insert(
                path.clone(),
                ProvisioningEntry {
                    relative_path: path,
                    mode: *mode,
                    origin_scope: level.scope,
                    pattern: pattern.clone(),
                    provided_by_git,
                },
            );
        }
        previews.extend(level_previews);
    }

    Ok(ProvisioningPlan {
        entries: final_entries.into_values().collect(),
        rules: previews,
        conflicts,
    })
}

fn ensure_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create provisioning parent {}", parent.display()))?;
    }
    Ok(())
}

pub(crate) fn provision_missing(
    repository: &Path,
    worktree: &Path,
    plan: &ProvisioningPlan,
) -> Result<()> {
    if let Some(conflict) = plan.conflicts.first() {
        anyhow::bail!(
            "mode conflict in {:?}: {} is declared in {} modes",
            conflict.scope,
            conflict.relative_path,
            conflict.modes.len()
        );
    }

    for entry in &plan.entries {
        let source = repository.join(&entry.relative_path);
        let target = worktree.join(&entry.relative_path);
        if std::fs::symlink_metadata(&target).is_ok() {
            continue;
        }
        ensure_parent(&target)?;
        let metadata = std::fs::symlink_metadata(&source)
            .with_context(|| format!("inspect provisioning source {}", source.display()))?;
        let effect = match entry.mode {
            ProvisioningMode::Copy if metadata.file_type().is_symlink() => {
                let link_target = std::fs::read_link(&source)?;
                #[cfg(unix)]
                {
                    std::os::unix::fs::symlink(link_target, &target)
                }
                #[cfg(not(unix))]
                {
                    std::fs::copy(&source, &target).map(|_| ())
                }
            }
            ProvisioningMode::Copy => std::fs::copy(&source, &target).map(|_| ()),
            ProvisioningMode::Hardlink => std::fs::hard_link(&source, &target),
            ProvisioningMode::Symlink => {
                #[cfg(unix)]
                {
                    std::os::unix::fs::symlink(&source, &target)
                }
                #[cfg(not(unix))]
                {
                    std::os::windows::fs::symlink_file(&source, &target)
                }
            }
        };
        effect.with_context(|| {
            format!(
                "{} `{}` from {} to {}",
                entry.mode,
                entry.relative_path,
                source.display(),
                target.display()
            )
        })?;
    }
    Ok(())
}

/// Read the node extension directly from the frozen pipeline document. Keeping
/// provisioning as an extension avoids widening the heavily shared NodeDef
/// runtime shape while still preserving it through YAML authoring.
pub(crate) fn node_rules_from_pipeline(
    pipeline_path: &Path,
    node_id: &str,
) -> Result<ProvisioningRules> {
    let yaml = std::fs::read_to_string(pipeline_path)
        .with_context(|| format!("read pipeline {}", pipeline_path.display()))?;
    let document: serde_yaml::Value = serde_yaml::from_str(&yaml)
        .with_context(|| format!("parse pipeline {}", pipeline_path.display()))?;
    let Some(nodes) = document
        .get("nodes")
        .and_then(serde_yaml::Value::as_sequence)
    else {
        return Ok(ProvisioningRules::default());
    };
    for node in nodes {
        if node.get("id").and_then(serde_yaml::Value::as_str) == Some(node_id) {
            return node
                .get("provisioning")
                .cloned()
                .map(serde_yaml::from_value)
                .transpose()
                .context("parse node provisioning rules")
                .map(Option::unwrap_or_default);
        }
    }
    Ok(ProvisioningRules::default())
}

pub(crate) async fn init(db: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS provisioning_rules (
            scope TEXT NOT NULL,
            scope_id TEXT NOT NULL,
            rules_json TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            PRIMARY KEY (scope, scope_id)
        )",
    )
    .execute(db)
    .await?;
    Ok(())
}

pub(crate) async fn load(
    db: &SqlitePool,
    scope: ProvisioningScope,
    scope_id: &str,
) -> Result<ProvisioningRules, sqlx::Error> {
    let row =
        sqlx::query("SELECT rules_json FROM provisioning_rules WHERE scope = ? AND scope_id = ?")
            .bind(format!("{scope:?}").to_ascii_lowercase())
            .bind(scope_id)
            .fetch_optional(db)
            .await?;
    Ok(row
        .and_then(|row| serde_json::from_str(&row.get::<String, _>("rules_json")).ok())
        .unwrap_or_default())
}

pub(crate) async fn save(
    db: &SqlitePool,
    scope: ProvisioningScope,
    scope_id: &str,
    rules: &ProvisioningRules,
) -> Result<(), sqlx::Error> {
    let json = serde_json::to_string(rules).unwrap_or_else(|_| "{}".into());
    sqlx::query(
        "INSERT INTO provisioning_rules (scope, scope_id, rules_json, updated_at)
         VALUES (?, ?, ?, ?)
         ON CONFLICT(scope, scope_id) DO UPDATE SET
           rules_json = excluded.rules_json, updated_at = excluded.updated_at",
    )
    .bind(format!("{scope:?}").to_ascii_lowercase())
    .bind(scope_id)
    .bind(json)
    .bind(crate::event_log::now_iso())
    .execute(db)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
