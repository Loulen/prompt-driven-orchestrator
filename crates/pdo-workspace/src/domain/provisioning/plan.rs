//! Resolve provisioning rules into a plan: which path of the source repository
//! each level provides, in which mode, and what the preview shows per rule.
//!
//! Pure over its inputs: the caller lists the source repository's paths and the
//! paths git already provides at the base ref. Matching follows gitignore
//! syntax, one pattern at a time.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Context, Result};
use ignore::gitignore::GitignoreBuilder;

use super::rules::{
    ExcludedPathPreview, ModeConflict, ProvisioningEntry, ProvisioningMode, ProvisioningPlan,
    RulePreview, ScopedRules,
};

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

/// Resolve `scoped` (coarsest level first) over the source repository.
///
/// `paths` lists every path of the source below `repository` (relative,
/// `/`-separated, sorted, `true` for a directory), `.git` and `.pdo` excluded.
/// `git_tracked` holds the paths git provides at the Run's base ref: those are
/// flagged `provided_by_git`, and a directory holding one is never symlinked.
pub fn resolve(
    repository: &Path,
    paths: &[(String, bool)],
    git_tracked: &BTreeSet<String>,
    scoped: &[ScopedRules],
) -> Result<ProvisioningPlan> {
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
            for (path, is_dir) in paths {
                if *is_dir && (excluded || mode != ProvisioningMode::Symlink) {
                    continue;
                }
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

    let directory_paths: Vec<&str> = paths
        .iter()
        .filter_map(|(path, is_dir)| is_dir.then_some(path.as_str()))
        .collect();
    for directory in directory_paths {
        let Some(directory_entry) = final_entries.get(directory).cloned() else {
            continue;
        };
        if directory_entry.mode != ProvisioningMode::Symlink {
            continue;
        }
        let prefix = format!("{directory}/");
        let descendants: Vec<&str> = paths
            .iter()
            .filter_map(|(path, _)| path.starts_with(&prefix).then_some(path.as_str()))
            .collect();
        let can_link_directory = !descendants.is_empty()
            && descendants.iter().all(|path| {
                final_entries.get(*path).is_some_and(|entry| {
                    entry.mode == ProvisioningMode::Symlink
                        && entry.origin_scope == directory_entry.origin_scope
                        && entry.pattern == directory_entry.pattern
                        && !entry.provided_by_git
                })
            });
        if can_link_directory {
            final_entries.retain(|path, _| !path.starts_with(&prefix));
            for preview in &mut previews {
                if preview.scope == directory_entry.origin_scope
                    && preview.mode == directory_entry.mode
                    && preview.pattern == directory_entry.pattern
                {
                    preview.paths.retain(|path| !path.starts_with(&prefix));
                }
            }
        } else {
            final_entries.remove(directory);
            for preview in &mut previews {
                preview.paths.retain(|path| path != directory);
            }
        }
    }

    Ok(ProvisioningPlan {
        entries: final_entries.into_values().collect(),
        rules: previews,
        conflicts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::provisioning::rules::{ProvisioningRules, ProvisioningScope};

    fn paths(list: &[(&str, bool)]) -> Vec<(String, bool)> {
        list.iter().map(|(p, d)| (p.to_string(), *d)).collect()
    }

    #[test]
    fn a_finer_level_excludes_what_a_coarser_one_matched() {
        let source = paths(&[
            ("fixtures", true),
            ("fixtures/a.bin", false),
            ("fixtures/b.bin", false),
        ]);
        let scoped = [
            ScopedRules {
                scope: ProvisioningScope::Instance,
                rules: ProvisioningRules {
                    copy: vec!["fixtures/**".into()],
                    ..Default::default()
                },
            },
            ScopedRules {
                scope: ProvisioningScope::Run,
                rules: ProvisioningRules {
                    copy: vec!["!fixtures/b.bin".into()],
                    ..Default::default()
                },
            },
        ];
        let plan = resolve(Path::new("/repo"), &source, &BTreeSet::new(), &scoped).unwrap();
        let entries: Vec<_> = plan
            .entries
            .iter()
            .map(|e| e.relative_path.as_str())
            .collect();
        assert_eq!(entries, vec!["fixtures/a.bin"]);
        assert!(plan.rules[0].excluded_paths.iter().any(|excluded| {
            excluded.relative_path == "fixtures/b.bin"
                && excluded.excluded_by_scope == ProvisioningScope::Run
        }));
    }

    #[test]
    fn two_modes_on_one_path_at_one_level_is_a_conflict() {
        let source = paths(&[(".env", false)]);
        let scoped = [ScopedRules {
            scope: ProvisioningScope::Run,
            rules: ProvisioningRules {
                copy: vec![".env".into()],
                symlink: vec![".env".into()],
                ..Default::default()
            },
        }];
        let plan = resolve(Path::new("/repo"), &source, &BTreeSet::new(), &scoped).unwrap();
        assert!(plan.entries.is_empty());
        assert_eq!(plan.conflicts[0].relative_path, ".env");
        assert_eq!(
            plan.conflicts[0].modes,
            vec![ProvisioningMode::Copy, ProvisioningMode::Symlink]
        );
    }

    #[test]
    fn a_path_git_provides_is_flagged() {
        let source = paths(&[("tracked.txt", false)]);
        let tracked: BTreeSet<String> = ["tracked.txt".to_string()].into();
        let scoped = [ScopedRules {
            scope: ProvisioningScope::Instance,
            rules: ProvisioningRules {
                copy: vec!["*.txt".into()],
                ..Default::default()
            },
        }];
        let plan = resolve(Path::new("/repo"), &source, &tracked, &scoped).unwrap();
        assert!(plan.entries[0].provided_by_git);
        assert!(!plan.rules[0].unmatched);
    }
}
