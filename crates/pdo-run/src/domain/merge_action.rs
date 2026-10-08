//! Pure reads of a node's frozen spawn facts off the event log: the merge-back
//! decision #503 needed (*may a conflicting merge-back be resolved in the node's
//! favour?*) and the isolation frozen at spawn (#653).
//!
//! The Merge node and its resolver are gone (ADR-0082): every isolated node
//! merges its own sub-worktree back into the Run branch when it finishes, so the
//! only merge decisions left are the merge-back ones below.
//!
//! This module is the *decision* half of the split `worktree_ops` documents: the
//! git **effect** (`MergeResult`, the shell-outs) lives there, the pure verdict
//! lives here. `spawn_base_sha` follows that rule — it reads the event log and
//! nothing else; comparing its answer to the branch's actual tip is the effect
//! layer's job.

use pdo_workspace::domain::commit_identity::CommitIdentity;

use crate::domain::event::Event;
use crate::domain::event::EventKind;

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
/// spawn says what the current sub-worktree stands on.
///
/// Careful with the word "cut" here — #489 changed what a re-spawn does. It used
/// to re-cut the sub-worktree "from wherever the branch is then"; since
/// `ensure_sub_worktree`, a re-spawn of a still-present sub-worktree **reuses it in
/// place** and CARRIES THIS VERY VALUE FORWARD onto the new `NodeStarted`. So the
/// last spawn is still authoritative, but on a re-spawn it is authoritative because
/// it copied the original cut, not because it made a new one. Re-deriving the base
/// at reuse time would either kill the adoption rule for every restarted node or arm
/// it falsely — see ADR-0037 §6.
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

/// The isolation FROZEN onto `(node_id, iter)` at spawn (#653, ADR-0060).
///
/// Anchored on the **last** `NodeStarted` for the pair, exactly like
/// [`spawn_base_sha`] above: `restart_node` and `invalidate_nodes` re-spawn the
/// same iteration, and the spawn path carries the frozen value forward onto the
/// new event rather than re-reading the document. That is what makes "an
/// isolation edit cannot move a running iteration" true — the re-spawn lands
/// back in the working directory the interrupted one left.
///
/// `None` — the node never started, or a pre-#653 run whose events say nothing —
/// means the caller falls back to the document's answer.
pub fn frozen_isolation(events: &[Event], node_id: &str, iter: i64) -> Option<bool> {
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
        .get("isolated_worktree")?
        .as_bool()
}

/// The git identity FROZEN onto `(node_id, iter)` at spawn (CORE-43): the four
/// `GIT_AUTHOR_*` / `GIT_COMMITTER_*` variables the extensions put in the Node's
/// environment, which sign its delivery.
///
/// Anchored on the **last** `NodeStarted` for the pair, like [`spawn_base_sha`]:
/// a re-spawn asks the extensions again and freezes their answer anew. Empty —
/// no extension gave one, a pre-CORE-43 Run, or a payload this build cannot read
/// — means the daemon's own identity.
pub fn frozen_git_identity(events: &[Event], node_id: &str, iter: i64) -> CommitIdentity {
    events
        .iter()
        .rev()
        .find(|e| {
            e.kind == EventKind::NodeStarted
                && e.node_id.as_deref() == Some(node_id)
                && e.iter.unwrap_or(1) == iter
        })
        .and_then(|e| e.payload.as_ref()?.get("git_identity").cloned())
        .and_then(|identity| serde_json::from_value(identity).ok())
        .unwrap_or_default()
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

    fn spawn_signed(node_id: &str, iter: i64, author: &str) -> Event {
        Event {
            payload: Some(serde_json::json!({
                "git_identity": { "GIT_AUTHOR_NAME": author, "GIT_COMMITTER_NAME": author },
            })),
            ..ev(EventKind::NodeStarted, Some(node_id), Some(iter))
        }
    }

    fn authors(identity: &CommitIdentity) -> Vec<(String, String)> {
        identity
            .vars()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn the_git_identity_comes_off_the_last_node_started() {
        let events = vec![
            ev(EventKind::RunStarted, None, None),
            spawn_signed("impl", 1, "Alice"),
            ev(EventKind::NodeInterrupted, Some("impl"), Some(1)),
            spawn_signed("impl", 1, "Bob"),
            spawn_signed("impl", 2, "Carol"),
        ];
        assert_eq!(
            authors(&frozen_git_identity(&events, "impl", 1)),
            vec![
                ("GIT_AUTHOR_NAME".to_string(), "Bob".to_string()),
                ("GIT_COMMITTER_NAME".to_string(), "Bob".to_string()),
            ]
        );
    }

    #[test]
    fn no_frozen_identity_is_the_empty_one() {
        let events = vec![
            ev(EventKind::RunStarted, None, None),
            spawn("impl", 1, "aaaa1111"),
            Event {
                payload: Some(serde_json::json!({ "git_identity": "garbage" })),
                ..ev(EventKind::NodeStarted, Some("ship"), Some(1))
            },
        ];
        assert!(frozen_git_identity(&events, "impl", 1).is_empty());
        assert!(frozen_git_identity(&events, "ship", 1).is_empty());
        assert!(frozen_git_identity(&events, "never", 1).is_empty());
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

    /// The case where a re-spawn genuinely re-cut the sub-worktree
    /// (`invalidate_nodes`, or a `restart_node` whose worktree was `Recyclable`).
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

    /// …and the #489 shape, which reads the same but means something else: the
    /// re-spawn REUSED the sub-worktree, so `ensure_sub_worktree` copied the
    /// original base forward. "Last spawn wins" still holds, and it has to yield the
    /// FIRST cut's SHA — that is the whole point of carrying it (ADR-0037 §6).
    #[test]
    fn a_reusing_respawn_carries_the_original_base_forward() {
        let events = vec![
            ev(EventKind::RunStarted, None, None),
            spawn("ship", 1, "cut00000"),
            // restart_node: the sub-worktree was `Reusable`, nothing was re-cut.
            spawn("ship", 1, "cut00000"),
        ];
        assert_eq!(
            spawn_base_sha(&events, "ship", 1).as_deref(),
            Some("cut00000")
        );
    }

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
}
