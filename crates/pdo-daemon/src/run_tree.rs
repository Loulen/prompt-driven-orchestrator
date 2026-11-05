//! The run tree (ADR-0064, ADR-0081): every descendant of a Run, with its
//! **depth** (1 = direct child), resolved through the mechanical
//! `parent_run_id` link a child's `RunStarted` froze.
//!
//! ONE walk for every consumer — the archive cascade (#815, which needs
//! grandchildren before their parent: [`RunTree::descendants_post_order`]) and
//! the Coût de l'arbre (pdo-issues#3, which needs the depth of each
//! descendant). Archived runs are ordinary members of the tree (their event
//! log survives); a forgotten run has no event log any more, so it never
//! reaches the links this tree is built from and drops out by construction.
//!
//! Pure: built from `(run_id, parent_run_id)` links, no store access.

use std::collections::{HashMap, HashSet};

/// One descendant of the walked Run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TreeMember {
    pub run_id: String,
    /// 1 = a direct child, 2 = a grandchild, …
    pub depth: usize,
}

/// The parent → children index of the store, children kept in input order.
#[derive(Debug, Default)]
pub(crate) struct RunTree {
    children: HashMap<String, Vec<String>>,
}

impl RunTree {
    /// Index the store from `(run_id, parent_run_id)` pairs. A root run (no
    /// parent) is a pair with `None`; it indexes nothing but may still be walked.
    pub(crate) fn from_links<I, R, P>(links: I) -> Self
    where
        I: IntoIterator<Item = (R, Option<P>)>,
        R: Into<String>,
        P: Into<String>,
    {
        let mut children: HashMap<String, Vec<String>> = HashMap::new();
        for (run_id, parent) in links {
            if let Some(parent) = parent {
                children
                    .entry(parent.into())
                    .or_default()
                    .push(run_id.into());
            }
        }
        RunTree { children }
    }

    /// Index a projected store (the archive cascade's read model).
    pub(crate) fn from_states(runs: &[crate::event_log::RunState]) -> Self {
        Self::from_links(
            runs.iter()
                .map(|r| (r.run_id.as_str(), r.parent_run_id.as_deref())),
        )
    }

    /// Every descendant of `run_id` in **pre-order** (a run before its own
    /// descendants), with its depth. `run_id` itself is not listed. A cycle in
    /// the links (impossible by construction, ADR-0064) is cut, never looped.
    pub(crate) fn descendants(&self, run_id: &str) -> Vec<TreeMember> {
        let mut out = Vec::new();
        let mut seen: HashSet<String> = HashSet::from([run_id.to_string()]);
        self.walk(run_id, 1, &mut seen, &mut out, false);
        out
    }

    /// Every descendant of `run_id` in **post-order**: a run always comes after
    /// all of its own descendants, so a cascade that walks the list front to
    /// back tears grandchildren down before their parent.
    pub(crate) fn descendants_post_order(&self, run_id: &str) -> Vec<TreeMember> {
        let mut out = Vec::new();
        let mut seen: HashSet<String> = HashSet::from([run_id.to_string()]);
        self.walk(run_id, 1, &mut seen, &mut out, true);
        out
    }

    fn walk(
        &self,
        run_id: &str,
        depth: usize,
        seen: &mut HashSet<String>,
        out: &mut Vec<TreeMember>,
        post_order: bool,
    ) {
        let Some(children) = self.children.get(run_id) else {
            return;
        };
        for child in children {
            if !seen.insert(child.clone()) {
                continue;
            }
            let member = TreeMember {
                run_id: child.clone(),
                depth,
            };
            if !post_order {
                out.push(member.clone());
            }
            self.walk(child, depth + 1, seen, out, post_order);
            if post_order {
                out.push(member);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> RunTree {
        RunTree::from_links([
            ("root", None),
            ("a", Some("root")),
            ("b", Some("root")),
            ("a1", Some("a")),
            ("a1x", Some("a1")),
            ("stranger", None::<&str>),
        ])
    }

    fn ids(members: &[TreeMember]) -> Vec<(&str, usize)> {
        members
            .iter()
            .map(|m| (m.run_id.as_str(), m.depth))
            .collect()
    }

    #[test]
    fn descendants_carry_their_depth_in_pre_order() {
        assert_eq!(
            ids(&tree().descendants("root")),
            vec![("a", 1), ("a1", 2), ("a1x", 3), ("b", 1)]
        );
    }

    #[test]
    fn post_order_lists_grandchildren_before_their_parent() {
        assert_eq!(
            ids(&tree().descendants_post_order("root")),
            vec![("a1x", 3), ("a1", 2), ("a", 1), ("b", 1)]
        );
    }

    #[test]
    fn depth_is_relative_to_the_walked_run() {
        assert_eq!(ids(&tree().descendants("a")), vec![("a1", 1), ("a1x", 2)]);
    }

    #[test]
    fn a_leaf_or_unknown_run_has_no_descendant() {
        assert!(tree().descendants("b").is_empty());
        assert!(tree().descendants("stranger").is_empty());
        assert!(tree().descendants("nope").is_empty());
    }

    #[test]
    fn a_cycle_is_cut_not_looped() {
        let t = RunTree::from_links([("x", Some("y")), ("y", Some("x"))]);
        assert_eq!(ids(&t.descendants("x")), vec![("y", 1)]);
    }
}
