/// Pure decision logic for Merge node outcomes, plus the merge-back decision
/// #503 needed: *may a conflicting merge-back be resolved in the node's favour?*
///
/// Given the result of attempting git merges on upstream code-mutating branches,
/// determines whether the Merge node can auto-complete (no conflicts) or needs
/// to spawn a Claude Code resolver session (conflicts detected).
///
/// This module is the *decision* half of the split `worktree_ops` documents: the
/// git **effect** (`MergeResult`, the shell-outs) lives there, the pure verdict
/// lives here. `spawn_base_sha` follows that rule — it reads the event log and
/// nothing else; comparing its answer to the branch's actual tip is the effect
/// layer's job.
use crate::event_log::{Event, EventKind};

/// The commit a node's sub-worktree was cut from, as its `NodeStarted` recorded
/// it (#503, ADR-0036) — the `base_sha` payload key written by both spawn paths.
///
/// This is the whole basis of the merge-back adoption rule: the pipeline branch is
/// created from the run's base and only ever receives this run's own work, so if
/// its tip is *still* this commit, then every commit the tip has and the node's
/// branch lacks is a commit the node **started from** and rewrote (a `Ship It`
/// node rebasing onto a moved `main` rewrites exactly that). Resolving the
/// conflict in the node's favour then supersedes the run's own history and
/// nothing else.
///
/// Deliberately **structural**, not content-based: no predicate over trees or
/// paths can tell "the same work, rewritten" from "different work" — the three
/// candidates were measured against the real occurrence and all three refuse it
/// (ADR-0036 §3).
///
/// Anchored on the **last** `NodeStarted` for `(node_id, iter)`: `restart_node`
/// and `invalidate_nodes` re-spawn the same iteration, and only the most recent
/// spawn says what the current sub-worktree was cut from.
///
/// `None` — a run created by a pre-#503 daemon, or a spawn path that recorded no
/// base — means *no adoption*. An unknown base is not a licence to rewrite a
/// branch.
pub fn spawn_base_sha(events: &[Event], node_id: &str, iter: i64) -> Option<String> {
    events
        .iter()
        .rev()
        .find(|e| {
            e.kind == EventKind::NodeStarted
                && e.node_id.as_deref() == Some(node_id)
                && e.iter.unwrap_or(1) == iter
        })?
        .payload
        .as_ref()?
        .get("base_sha")?
        .as_str()
        .map(str::trim)
        .filter(|sha| !sha.is_empty())
        .map(String::from)
}

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

    fn ev(kind: EventKind, node_id: Option<&str>, iter: Option<i64>) -> Event {
        Event {
            id: None,
            run_id: "r".to_string(),
            ts: "2026-07-31T09:00:00Z".to_string(),
            kind,
            node_id: node_id.map(String::from),
            iter,
            payload: None,
        }
    }

    fn spawn(node_id: &str, iter: i64, base_sha: &str) -> Event {
        Event {
            payload: Some(serde_json::json!({ "base_sha": base_sha })),
            ..ev(EventKind::NodeStarted, Some(node_id), Some(iter))
        }
    }

    #[test]
    fn the_spawn_base_comes_off_the_node_started_payload() {
        let events = vec![
            ev(EventKind::RunStarted, None, None),
            spawn("impl", 1, "aaaa1111"),
            ev(EventKind::NodeCompleted, Some("impl"), Some(1)),
            spawn("ship", 1, "bbbb2222"),
        ];
        assert_eq!(
            spawn_base_sha(&events, "ship", 1).as_deref(),
            Some("bbbb2222")
        );
        assert_eq!(
            spawn_base_sha(&events, "impl", 1).as_deref(),
            Some("aaaa1111")
        );
    }

    /// Anchored on the LAST spawn of the iteration: `restart_node` and
    /// `invalidate_nodes` re-cut the sub-worktree from wherever the branch is *then*.
    #[test]
    fn a_respawn_of_the_same_iteration_wins() {
        let events = vec![
            ev(EventKind::RunStarted, None, None),
            spawn("ship", 1, "old00000"),
            ev(EventKind::NodeFailed, Some("ship"), Some(1)),
            spawn("ship", 1, "new11111"),
        ];
        assert_eq!(
            spawn_base_sha(&events, "ship", 1).as_deref(),
            Some("new11111")
        );
    }

    /// Iterations are separate bases: a loop lap must not read the previous lap's.
    #[test]
    fn each_iteration_carries_its_own_base() {
        let events = vec![
            ev(EventKind::RunStarted, None, None),
            spawn("impl", 1, "lap11111"),
            ev(EventKind::NodeCompleted, Some("impl"), Some(1)),
            spawn("impl", 2, "lap22222"),
        ];
        assert_eq!(
            spawn_base_sha(&events, "impl", 2).as_deref(),
            Some("lap22222")
        );
    }

    /// An unknown base is not a licence to rewrite a branch: a pre-#503 Run, a
    /// spawn that recorded nothing, a blank value, or no spawn at all → `None`.
    #[test]
    fn an_unknown_base_stays_unknown() {
        let no_payload = vec![
            ev(EventKind::RunStarted, None, None),
            ev(EventKind::NodeStarted, Some("ship"), Some(1)),
        ];
        assert!(spawn_base_sha(&no_payload, "ship", 1).is_none());

        let blank = vec![
            ev(EventKind::RunStarted, None, None),
            spawn("ship", 1, "   "),
        ];
        assert!(spawn_base_sha(&blank, "ship", 1).is_none());

        let never_started = vec![ev(EventKind::RunStarted, None, None)];
        assert!(spawn_base_sha(&never_started, "ship", 1).is_none());
    }

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
