//! The CLI parity (ADR draft « chaque route a sa commande CLI, un test le
//! vérifie »): **every route of the daemon is called by a `pdo` command, or is
//! excluded with its reason** (`tests/cli_parity.rs`).
//!
//! Both sides are read statically, with the readers of the route coverage
//! ([`crate::routes`]):
//!
//! - **the routes**: [`crate::routes::context_routes`], every router the daemon
//!   merges;
//! - **the commands**: the URL literals of the CLI's sources ([`cli_sources`]:
//!   every file under an `adapters/cli/` folder or named `adapters/cli.rs`, and
//!   `crates/pdo/src/cli/`), their unit tests left out, each tied to its HTTP
//!   method the way the route coverage ties a test's call. Write the call so
//!   the reader sees it: `daemon.post(&format!("/triggers/{id}/fire"), None)`.
//!
//! A route no command calls is listed in [`exclusions`]: [`exclusions::EXCLUDED`]
//! for good, with the reason a command line has nothing to do with it, or
//! [`exclusions::PENDING`] for now, a command still to write.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::routes::{calls_in, Route};

pub mod exclusions;

/// The CLI's sources, as `(path relative to the repository root, source)`,
/// each without its `#[cfg(test)]` module.
pub fn cli_sources() -> Vec<(String, String)> {
    let root = crate::workspace_root();
    let mut files = Vec::new();
    for krate in std::fs::read_dir(root.join("crates")).unwrap().flatten() {
        let src = krate.path().join("src");
        collect(&src.join("adapters/cli"), &mut files);
        let single = src.join("adapters/cli.rs");
        if single.is_file() {
            files.push(single);
        }
    }
    collect(&root.join("crates/pdo/src/cli"), &mut files);
    files.sort();
    files
        .into_iter()
        .map(|path| {
            let rel = path
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            let source = std::fs::read_to_string(&path).unwrap();
            (rel, without_tests(&source).to_string())
        })
        .collect()
}

fn collect(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// `source` up to its top-level `#[cfg(test)]` module: a literal of a unit
/// test is no call of a command.
pub fn without_tests(source: &str) -> &str {
    match source.find("\n#[cfg(test)]\nmod ") {
        Some(at) => &source[..at + 1],
        None => source,
    }
}

/// Where each route stands: called by a command, excluded, pending, or
/// nowhere.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Parity {
    /// Routes no command calls and no exclusion names.
    pub uncovered: Vec<Route>,
    /// Exclusions (excluded or pending) naming a route the router does not
    /// declare.
    pub unknown: Vec<Route>,
    /// Exclusions naming a route a command calls: the command is written,
    /// the exclusion goes.
    pub covered_yet_excluded: Vec<Route>,
    /// Routes two exclusions name.
    pub doubled: Vec<Route>,
    /// Excluded routes whose reason is empty.
    pub unmotivated: Vec<Route>,
}

impl Parity {
    pub fn holds(&self) -> bool {
        *self == Parity::default()
    }
}

/// The parity of `routes` with the commands of `sources`, given the
/// `excluded` and `pending` lists.
pub fn parity(
    routes: &[Route],
    sources: &[(String, String)],
    excluded: &[exclusions::Excluded],
    pending: &[exclusions::Pending],
) -> Parity {
    let declared: BTreeSet<Route> = routes.iter().cloned().collect();
    let paths: BTreeSet<String> = routes.iter().map(|(_, p)| p.clone()).collect();
    let called: BTreeMap<Route, BTreeSet<String>> = calls_in(sources, &paths);

    let mut parity = Parity::default();
    let mut named: BTreeMap<Route, usize> = BTreeMap::new();
    let listed = excluded
        .iter()
        .flat_map(|e| e.routes.iter().map(move |r| (r, e.reason)))
        .chain(
            pending
                .iter()
                .flat_map(|p| p.routes.iter().map(move |r| (r, p.family))),
        );
    for ((method, path), why) in listed {
        let route = (method.to_string(), path.to_string());
        *named.entry(route.clone()).or_default() += 1;
        if why.trim().is_empty() {
            parity.unmotivated.push(route.clone());
        }
        if !declared.contains(&route) {
            parity.unknown.push(route);
        } else if called.contains_key(&route) {
            parity.covered_yet_excluded.push(route);
        }
    }
    parity.doubled = named
        .into_iter()
        .filter(|(_, n)| *n > 1)
        .map(|(r, _)| r)
        .collect();
    let mut seen = BTreeSet::new();
    for route in routes {
        let known = called.contains_key(route) || exclusions::names(excluded, pending, route);
        if !known && seen.insert(route.clone()) {
            parity.uncovered.push(route.clone());
        }
    }
    for list in [
        &mut parity.uncovered,
        &mut parity.unknown,
        &mut parity.covered_yet_excluded,
        &mut parity.unmotivated,
    ] {
        list.sort();
        list.dedup();
    }
    parity
}
