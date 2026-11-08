//! The **root** of a Run: the Run at the top of its chain of parents (ADR-0064),
//! the one a person launched. A Run with no parent is its own root.
//!
//! The browser's Portée d'alerte (CONTEXT.md « Portée d'alerte ») judges a
//! descendant by its root: a Run an orchestration I started has created alerts
//! me like the orchestration itself. The list (`GET /runs`) carries the root of
//! each child, computed here over **every** Run before the list keeps those the
//! Actor may read, so a hidden intermediate parent does not cut the chain.
//!
//! Pure: a parent link per Run in, a root per Run out.

use std::collections::{HashMap, HashSet};

/// The root of each Run of `parents` (`run id -> parent run id, if any`).
///
/// A parent the map does not hold (forgotten, reaped) ends the chain: the last
/// Run known is the root. A cycle — which provenance never writes — ends at the
/// Run the walk would visit twice, so the walk always stops.
pub fn root_run_ids<'a>(
    parents: impl IntoIterator<Item = (&'a str, Option<&'a str>)>,
) -> HashMap<String, String> {
    let parents: HashMap<&str, Option<&str>> = parents.into_iter().collect();
    let mut roots = HashMap::with_capacity(parents.len());
    for &run in parents.keys() {
        let mut current = run;
        let mut seen = HashSet::from([run]);
        while let Some(Some(parent)) = parents.get(current) {
            if !parents.contains_key(parent) || !seen.insert(parent) {
                break;
            }
            current = parent;
        }
        roots.insert(run.to_string(), current.to_string());
    }
    roots
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roots(links: &[(&'static str, Option<&'static str>)]) -> HashMap<String, String> {
        root_run_ids(links.iter().copied())
    }

    #[test]
    fn a_run_without_a_parent_is_its_own_root() {
        assert_eq!(roots(&[("a", None)])["a"], "a");
    }

    #[test]
    fn a_descendant_climbs_to_the_top_of_its_chain() {
        let r = roots(&[
            ("a", None),
            ("b", Some("a")),
            ("c", Some("b")),
            ("d", Some("c")),
        ]);
        assert_eq!(r["b"], "a");
        assert_eq!(r["c"], "a");
        assert_eq!(r["d"], "a");
    }

    #[test]
    fn a_parent_no_longer_known_ends_the_chain() {
        let r = roots(&[("b", Some("gone")), ("c", Some("b"))]);
        assert_eq!(r["b"], "b");
        assert_eq!(r["c"], "b");
    }

    #[test]
    fn a_cycle_stops_the_walk() {
        let r = roots(&[("a", Some("b")), ("b", Some("a"))]);
        assert_eq!(r["a"], "b");
        assert_eq!(r["b"], "a");
    }
}
