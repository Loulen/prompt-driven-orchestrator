//! The **derived** awaiting state of a parent (#588 / ADR-0069 §4).
//!
//! A child run `awaiting_user` does not wake `pdo run wait` (it is not terminal)
//! but it makes the parent node — and the parent run — `awaiting_user` **on
//! read**, exactly like `stalled` (#180): nothing is written to the parent's
//! event log, the lift is automatic when the child resumes, and the depth of the
//! tree is free because the overlay is recursive (a grandchild's wait climbs
//! through its parent to the grandparent).
//!
//! Pure: takes every run and rewrites the parents in place. The HTTP reads
//! (`GET /runs`, `GET /runs/{id}`, `GET /runs/{id}/children`) all go through
//! [`overlay`] so the list dot, the node banner and the Orchestration tab can
//! never disagree about the same child. It works on the projected states (the
//! detail) as on the Résumés de Run (the lists, ADR-0084): [`AwaitingTree`].

use std::collections::{HashMap, HashSet};

use crate::domain::state::{
    AwaitingInfo, NodeStatus, RunState, RunStatus, AWAITING_CAUSE_CHILD_AWAITING,
};

/// The banner reason a parent carries for an awaiting child.
pub fn child_awaiting_reason(child_name: &str) -> String {
    format!("child run {child_name} is awaiting you")
}

/// The awaiting child a parent is lifted by.
#[derive(Debug, Clone)]
pub struct ChildWait {
    pub child_id: String,
    /// Its name, else its id.
    pub child_name: String,
    /// The node of the parent the child hangs from.
    pub parent_node: Option<String>,
    /// When the child started waiting, when it says.
    pub since: Option<String>,
}

/// A Run as the overlay reads and lifts it.
pub trait AwaitingTree {
    fn run_id(&self) -> &str;
    fn parent_run_id(&self) -> Option<&str>;
    fn status(&self) -> &RunStatus;
    /// What a parent of this Run is lifted by, this Run awaiting.
    fn as_child_wait(&self) -> ChildWait;
    /// Become awaiting because of `child`.
    fn lift(&mut self, child: &ChildWait);
}

/// Apply the derived overlay to every run of `runs`. A `Running` run whose
/// (live) child is `AwaitingUser` — itself possibly by derivation — becomes
/// `AwaitingUser` with the reason naming the child; for a projected state, the
/// parent **node** the child hangs from (`parent_node_id`) becomes
/// `AwaitingUser` too when it holds a session, with `cause = child_awaiting`
/// and the child's id for the « Open <child> → » link. Nothing is written
/// anywhere.
///
/// Only a **`Running`** parent is lifted: a paused or terminal parent is not
/// waiting on anyone, and a parent already `AwaitingUser` on its own account
/// keeps its own reason (its own wait comes first).
pub fn overlay<T: AwaitingTree>(runs: &mut [T]) {
    let index: HashMap<String, usize> = runs
        .iter()
        .enumerate()
        .map(|(i, r)| (r.run_id().to_string(), i))
        .collect();
    let mut children: HashMap<usize, Vec<usize>> = HashMap::new();
    for (i, run) in runs.iter().enumerate() {
        if let Some(parent) = run.parent_run_id().and_then(|p| index.get(p)) {
            children.entry(*parent).or_default().push(i);
        }
    }
    let mut memo: HashMap<usize, bool> = HashMap::new();
    let mut visiting: HashSet<usize> = HashSet::new();
    for i in 0..runs.len() {
        effective_awaiting(i, runs, &children, &mut memo, &mut visiting);
    }
}

/// Is run `i` awaiting its user, on its own account or by derivation? Rewrites
/// the run when the answer comes from a child. Memoised; the `visiting` set
/// guards against a (malformed) cycle in the provenance links.
fn effective_awaiting<T: AwaitingTree>(
    i: usize,
    runs: &mut [T],
    children: &HashMap<usize, Vec<usize>>,
    memo: &mut HashMap<usize, bool>,
    visiting: &mut HashSet<usize>,
) -> bool {
    if let Some(&known) = memo.get(&i) {
        return known;
    }
    if !visiting.insert(i) {
        return false;
    }
    let result = if *runs[i].status() == RunStatus::AwaitingUser {
        true
    } else if *runs[i].status() != RunStatus::Running {
        false
    } else {
        // The first awaiting child, in a stable order (children are pushed in
        // run-id order, which `load_all_run_ids` sorts) — one reason, one link.
        let kids = children.get(&i).cloned().unwrap_or_default();
        let mut awaiting_child: Option<usize> = None;
        for k in kids {
            if !runs[k].status().is_live() {
                continue;
            }
            if effective_awaiting(k, runs, children, memo, visiting) {
                awaiting_child = Some(k);
                break;
            }
        }
        match awaiting_child {
            None => false,
            Some(k) => {
                let wait = runs[k].as_child_wait();
                runs[i].lift(&wait);
                true
            }
        }
    };
    visiting.remove(&i);
    memo.insert(i, result);
    result
}

impl AwaitingTree for RunState {
    fn run_id(&self) -> &str {
        &self.run_id
    }

    fn parent_run_id(&self) -> Option<&str> {
        self.parent_run_id.as_deref()
    }

    fn status(&self) -> &RunStatus {
        &self.status
    }

    fn as_child_wait(&self) -> ChildWait {
        ChildWait {
            child_id: self.run_id.clone(),
            child_name: self.name.clone().unwrap_or_else(|| self.run_id.clone()),
            parent_node: self.parent_node_id.clone(),
            // The child's own `since` when it has one, so the parent's
            // « waiting N min » counts from the same instant.
            since: parent_awaiting_since(self),
        }
    }

    fn lift(&mut self, child: &ChildWait) {
        self.status = RunStatus::AwaitingUser;
        if self.awaiting_reason.is_none() {
            self.awaiting_reason = Some(child_awaiting_reason(&child.child_name));
        }
        let Some(node) = child
            .parent_node
            .as_ref()
            .and_then(|id| self.nodes.get_mut(id))
        else {
            return;
        };
        if node.status != NodeStatus::Running {
            return;
        }
        node.status = NodeStatus::AwaitingUser;
        // The node detail renders the iteration **on screen** (#617), so the
        // latest iteration must agree with the rollup or the banner never
        // shows (FP #793, finding 1).
        let latest = node.iter;
        if let Some(attempt) = node.iterations.iter_mut().find(|a| a.iter == latest) {
            if attempt.status == NodeStatus::Running {
                attempt.status = NodeStatus::AwaitingUser;
            }
        }
        node.awaiting = Some(AwaitingInfo {
            cause: AWAITING_CAUSE_CHILD_AWAITING.to_string(),
            message: Some(child_awaiting_reason(&child.child_name)),
            since: child
                .since
                .clone()
                .unwrap_or_else(pdo_kernel::clock::now_iso),
            child_run_id: Some(child.child_id.clone()),
        });
    }
}

fn parent_awaiting_since(child: &RunState) -> Option<String> {
    child
        .nodes
        .values()
        .filter(|n| n.status == NodeStatus::AwaitingUser)
        .filter_map(|n| n.awaiting.as_ref().map(|a| a.since.clone()))
        .min()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::event::{Event, EventKind};
    use crate::domain::state::AWAITING_CAUSE_DECLARED;
    use pretty_assertions::assert_eq;

    fn ev(
        run: &str,
        kind: EventKind,
        node: Option<&str>,
        payload: Option<serde_json::Value>,
    ) -> Event {
        Event {
            id: None,
            run_id: run.into(),
            ts: "2026-09-12T10:00:00Z".into(),
            kind,
            node_id: node.map(str::to_string),
            iter: node.map(|_| 1),
            payload,
        }
    }

    fn run(
        id: &str,
        name: &str,
        parent: Option<(&str, &str)>,
        node_awaiting: Option<&str>,
    ) -> RunState {
        let mut started = serde_json::json!({ "pipeline_name": "p", "name": name });
        if let Some((prun, pnode)) = parent {
            started["parent_run_id"] = serde_json::json!(prun);
            started["parent_node_id"] = serde_json::json!(pnode);
        }
        let mut events = vec![
            ev(id, EventKind::RunStarted, None, Some(started)),
            ev(id, EventKind::NodeStarted, Some("worker"), None),
        ];
        if let Some(msg) = node_awaiting {
            events.push(ev(
                id,
                EventKind::NodeAwaitingUser,
                Some("worker"),
                Some(serde_json::json!({ "cause": AWAITING_CAUSE_DECLARED, "message": msg })),
            ));
        }
        crate::domain::projection::project(&events).unwrap()
    }

    #[test]
    fn an_awaiting_child_lifts_its_parent_node_and_run_on_read() {
        let mut runs = vec![
            run("parent", "orchestrate-588", None, None),
            run(
                "child",
                "grill-588",
                Some(("parent", "worker")),
                Some("Which layout?"),
            ),
        ];
        overlay(&mut runs);
        let parent = &runs[0];
        assert_eq!(parent.status, RunStatus::AwaitingUser);
        assert_eq!(
            parent.awaiting_reason.as_deref(),
            Some("child run grill-588 is awaiting you")
        );
        assert!(
            parent.awaiting_reason_code.is_none(),
            "derived: no machine slug"
        );
        let node = &parent.nodes["worker"];
        assert_eq!(node.status, NodeStatus::AwaitingUser);
        // The latest iteration follows the rollup: the node detail gates its
        // banner on the iteration on screen.
        let latest = node
            .iterations
            .iter()
            .find(|a| a.iter == node.iter)
            .unwrap();
        assert_eq!(latest.status, NodeStatus::AwaitingUser);
        let info = node.awaiting.as_ref().unwrap();
        assert_eq!(info.cause, AWAITING_CAUSE_CHILD_AWAITING);
        assert_eq!(info.child_run_id.as_deref(), Some("child"));
        // The child is untouched.
        assert_eq!(runs[1].status, RunStatus::AwaitingUser);
        assert_eq!(runs[1].awaiting_reason.as_deref(), Some("Which layout?"));
    }

    #[test]
    fn a_running_child_lifts_nothing() {
        let mut runs = vec![
            run("parent", "p", None, None),
            run("child", "c", Some(("parent", "worker")), None),
        ];
        overlay(&mut runs);
        assert_eq!(runs[0].status, RunStatus::Running);
        assert!(runs[0].awaiting_reason.is_none());
        assert_eq!(runs[0].nodes["worker"].status, NodeStatus::Running);
        assert!(runs[0].nodes["worker"].awaiting.is_none());
    }

    #[test]
    fn the_wait_climbs_two_levels_of_tree() {
        let mut runs = vec![
            run("grand", "grand", None, None),
            run("parent", "parent", Some(("grand", "worker")), None),
            run("child", "leaf", Some(("parent", "worker")), Some("q?")),
        ];
        overlay(&mut runs);
        assert_eq!(runs[1].status, RunStatus::AwaitingUser);
        assert_eq!(
            runs[1].awaiting_reason.as_deref(),
            Some("child run leaf is awaiting you")
        );
        assert_eq!(runs[0].status, RunStatus::AwaitingUser);
        assert_eq!(
            runs[0].awaiting_reason.as_deref(),
            Some("child run parent is awaiting you")
        );
        assert_eq!(
            runs[0].nodes["worker"]
                .awaiting
                .as_ref()
                .unwrap()
                .child_run_id
                .as_deref(),
            Some("parent")
        );
    }

    #[test]
    fn a_parent_awaiting_on_its_own_account_keeps_its_own_reason() {
        let mut runs = vec![
            run("parent", "p", None, Some("my own question")),
            run(
                "child",
                "c",
                Some(("parent", "worker")),
                Some("child question"),
            ),
        ];
        overlay(&mut runs);
        assert_eq!(runs[0].awaiting_reason.as_deref(), Some("my own question"));
        assert_eq!(
            runs[0].nodes["worker"].awaiting.as_ref().unwrap().cause,
            AWAITING_CAUSE_DECLARED
        );
    }

    #[test]
    fn a_terminal_or_paused_parent_is_not_lifted() {
        let mut runs = vec![
            run("parent", "p", None, None),
            run("child", "c", Some(("parent", "worker")), Some("q?")),
        ];
        runs[0].status = RunStatus::Paused;
        overlay(&mut runs);
        assert_eq!(runs[0].status, RunStatus::Paused);
        assert!(runs[0].awaiting_reason.is_none());
    }

    #[test]
    fn a_terminal_child_awaiting_status_cannot_happen_but_a_dead_child_is_ignored() {
        let mut runs = vec![
            run("parent", "p", None, None),
            run("child", "c", Some(("parent", "worker")), Some("q?")),
        ];
        runs[1].status = RunStatus::Failed;
        overlay(&mut runs);
        assert_eq!(runs[0].status, RunStatus::Running);
    }
}
