/// Pure decision logic for Merge node outcomes.
///
/// Given the result of attempting git merges on upstream code-mutating branches,
/// determines whether the Merge node can auto-complete (no conflicts) or needs
/// to spawn a Claude Code resolver session (conflicts detected).

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeOutcome {
    AutoMerged {
        branch_count: usize,
        merged_md: String,
    },
    NeedsResolver {
        conflict_description: String,
    },
}

pub fn determine_outcome(
    upstream_branches: &[&str],
    conflict_count: usize,
    conflict_files: &[String],
) -> MergeOutcome {
    if conflict_count == 0 {
        let merged_md = format!(
            "---\nconflict_count: 0\nbranches:\n{}\n---\n\nAuto-merged {} branches with no conflicts.\n",
            upstream_branches
                .iter()
                .map(|b| format!("  - {b}"))
                .collect::<Vec<_>>()
                .join("\n"),
            upstream_branches.len(),
        );
        MergeOutcome::AutoMerged {
            branch_count: upstream_branches.len(),
            merged_md,
        }
    } else {
        let desc = format!(
            "{} conflict(s) in file(s): {}",
            conflict_count,
            if conflict_files.is_empty() {
                "(unknown)".to_string()
            } else {
                conflict_files.join(", ")
            }
        );
        MergeOutcome::NeedsResolver {
            conflict_description: desc,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn no_conflict_produces_auto_merged() {
        let branches = vec!["impl-a-branch", "impl-b-branch"];
        let outcome = determine_outcome(&branches, 0, &[]);
        match outcome {
            MergeOutcome::AutoMerged {
                branch_count,
                ref merged_md,
            } => {
                assert_eq!(branch_count, 2);
                assert!(merged_md.contains("conflict_count: 0"));
                assert!(merged_md.contains("impl-a-branch"));
                assert!(merged_md.contains("impl-b-branch"));
                assert!(merged_md.contains("Auto-merged 2 branches"));
            }
            _ => panic!("expected AutoMerged, got {outcome:?}"),
        }
    }

    #[test]
    fn conflict_produces_needs_resolver() {
        let branches = vec!["impl-a-branch", "impl-b-branch"];
        let files = vec!["src/main.rs".to_string(), "README.md".to_string()];
        let outcome = determine_outcome(&branches, 2, &files);
        match outcome {
            MergeOutcome::NeedsResolver {
                ref conflict_description,
            } => {
                assert!(conflict_description.contains("2 conflict(s)"));
                assert!(conflict_description.contains("src/main.rs"));
                assert!(conflict_description.contains("README.md"));
            }
            _ => panic!("expected NeedsResolver, got {outcome:?}"),
        }
    }

    #[test]
    fn single_branch_no_conflict() {
        let branches = vec!["only-branch"];
        let outcome = determine_outcome(&branches, 0, &[]);
        match outcome {
            MergeOutcome::AutoMerged {
                branch_count,
                ref merged_md,
            } => {
                assert_eq!(branch_count, 1);
                assert!(merged_md.contains("Auto-merged 1 branches"));
            }
            _ => panic!("expected AutoMerged, got {outcome:?}"),
        }
    }

    #[test]
    fn conflict_with_unknown_files() {
        let branches = vec!["a", "b"];
        let outcome = determine_outcome(&branches, 1, &[]);
        match outcome {
            MergeOutcome::NeedsResolver {
                ref conflict_description,
            } => {
                assert!(conflict_description.contains("(unknown)"));
            }
            _ => panic!("expected NeedsResolver, got {outcome:?}"),
        }
    }
}
