//! Where the daemon looks for harness binaries, and whether one is installed
//! (ADR-0055, #550).
//!
//! Resolving a harness binary is a harness concern, not a tmux one: the session
//! manager only borrows [`harness_probe_path`] to export the same `PATH` into a
//! node session, and the spawn preflight asks [`binary_available`]. This module
//! is the one place that reads the user's `PATH` for that purpose. It never
//! executes a harness (the catalogue probes that do live beside the session
//! manager's runner).

use std::path::Path;
use std::sync::OnceLock;

/// The `PATH` the daemon searches for harness binaries (ADR-0055).
///
/// A daemon launched as a service inherits the **unit's** `PATH`, which misses the
/// entries a package manager (Homebrew, nvm, a user prefix) adds only to an
/// **interactive** shell — so a harness the user installed and can run by hand is
/// invisible to the service, and the spawn fails saying an installed binary does
/// not exist. This resolves the binary in the `PATH` the user has *when they type
/// the command*: the interactive shell's `PATH`, unioned with the process `PATH`
/// so nothing the service already saw is lost. Measured (ADR-0055): a *login*
/// shell does not suffice — package managers add their paths from the
/// **interactive** rc files — so the probe sources an interactive shell (`-i`).
///
/// Resolved **once** and cached for the daemon's lifetime: the cost of sourcing
/// the user's shell config is paid at the first probe, not per spawn, and a `PATH`
/// the user changes afterwards is seen only on the next daemon start (ADR-0055
/// limits, same freshness contract as the model catalogue, ADR-0053). The env
/// override `PDO_HARNESS_PROBE_PATH` short-circuits the shell probe (ops / tests).
pub fn harness_probe_path() -> String {
    // Explicit overrides are a deterministic per-process seam. Read them before
    // the production cache so the single integration-test binary can isolate
    // tests that intentionally provide different fake harness directories.
    if let Some(path) = std::env::var_os("PDO_HARNESS_PROBE_PATH") {
        if !path.is_empty() {
            return path.to_string_lossy().into_owned();
        }
    }
    static PROBE_PATH: OnceLock<String> = OnceLock::new();
    PROBE_PATH.get_or_init(resolve_harness_probe_path).clone()
}

/// Compute the probe `PATH` (uncached): the interactive shell's `PATH` unioned
/// with the process `PATH`. See [`harness_probe_path`].
fn resolve_harness_probe_path() -> String {
    // An explicit override wins outright — the deterministic seam a test or an
    // operator uses instead of the ambient shell.
    if let Some(p) = std::env::var_os("PDO_HARNESS_PROBE_PATH") {
        if !p.is_empty() {
            return p.to_string_lossy().into_owned();
        }
    }
    let process_path = std::env::var_os("PATH")
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    match interactive_path_via_shell() {
        Some(interactive) => union_paths(&interactive, &process_path),
        None => process_path,
    }
}

/// The user's `PATH` as an **interactive** shell reports it, or `None` when the
/// probe fails (no `$SHELL`, the shell errors, empty output). Runs `$SHELL -i -c`
/// so the interactive rc files — where version and package managers add their
/// paths — are sourced (a *login* shell, `-l`, is not enough; ADR-0055).
fn interactive_path_via_shell() -> Option<String> {
    let shell = std::env::var_os("SHELL").filter(|s| !s.is_empty())?;
    // `printf` with no trailing newline, stdout only — job-control chatter an
    // interactive shell may emit goes to stderr and is ignored.
    let output = std::process::Command::new(&shell)
        .arg("-i")
        .arg("-c")
        .arg(r#"printf '%s' "$PATH""#)
        .output()
        .ok()?;
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!path.is_empty()).then_some(path)
}

/// Union two `PATH` strings, `first` taking precedence, dropping duplicates and
/// empty entries while preserving order. Pure — the testable core of the ADR-0055
/// merge.
fn union_paths(first: &str, second: &str) -> String {
    let mut seen = std::collections::HashSet::new();
    let joined: Vec<String> = std::env::split_paths(first)
        .chain(std::env::split_paths(second))
        .filter(|p| !p.as_os_str().is_empty())
        .filter(|p| seen.insert(p.clone()))
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    joined.join(":")
}

/// Whether `binary` resolves in `path`. Pure — the testable core of the probe.
fn path_contains_binary(path: &str, binary: &str) -> bool {
    std::env::split_paths(path).any(|dir| dir.join(binary).is_file())
}

/// Whether `binary` resolves on the harness probe `PATH` — the fail-fast spawn
/// check (#550, AC #10). A name with a `/` is checked directly; a bare name is
/// searched across [`harness_probe_path`] (the user's interactive `PATH`, ADR-0055
/// — **not** the service's inherited one). **Never executes** the binary (a probe
/// run could hang a resident harness), and lives here (not in the pure
/// `harness_registry`) because it reads the environment. A missing binary makes
/// the spawn fail *before* any session or start event exists (ADR-0037); the
/// caller's diagnostic names [`harness_probe_path`] so "not found" cannot read as
/// "not installed".
pub fn binary_available(binary: &str) -> bool {
    if binary.is_empty() {
        return false;
    }
    if binary.contains('/') {
        return Path::new(binary).is_file();
    }
    path_contains_binary(&harness_probe_path(), binary)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #550/AC #10: the fail-fast PATH probe. A bare name absent from the probe
    /// `PATH` is unavailable; an empty name is never available; a slash-path is
    /// checked as a file directly. `sh` is on every CI box (and the process `PATH`
    /// is unioned into the probe path, ADR-0055), so it stands in for "installed".
    #[test]
    fn binary_available_probes_path_without_executing() {
        assert!(!binary_available(""), "empty name is never available");
        assert!(
            !binary_available("pdo-definitely-not-a-real-binary-xyz"),
            "a name absent from PATH is unavailable"
        );
        assert!(
            binary_available("sh"),
            "a ubiquitous binary resolves on PATH"
        );
        assert!(
            !binary_available("/no/such/absolute/path/binary"),
            "a missing slash-path is unavailable"
        );
    }

    /// ADR-0055: the pure core of the probe `PATH` search — a directory that holds
    /// the binary makes it resolvable; one that does not, does not.
    #[test]
    fn path_contains_binary_searches_each_entry() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("my-harness");
        std::fs::write(&bin, b"#!/bin/sh\n").unwrap();
        let path = format!("/nonexistent-a:{}:/nonexistent-b", dir.path().display());
        assert!(path_contains_binary(&path, "my-harness"));
        assert!(!path_contains_binary(&path, "absent-harness"));
        assert!(
            !path_contains_binary("/nonexistent-a:/nonexistent-b", "my-harness"),
            "no entry holds it"
        );
    }

    /// ADR-0055: the user's interactive `PATH` is unioned with the process one,
    /// first wins, duplicates and empty entries dropped, order preserved — so the
    /// package-manager prefix the service never inherited becomes searchable
    /// without losing any entry the service already had.
    #[test]
    fn union_paths_merges_first_precedence_dedup_order_preserved() {
        assert_eq!(
            union_paths("/opt/homebrew/bin:/usr/bin", "/usr/bin:/bin"),
            "/opt/homebrew/bin:/usr/bin:/bin"
        );
        // Empty operands collapse cleanly.
        assert_eq!(union_paths("", "/usr/bin:/bin"), "/usr/bin:/bin");
        assert_eq!(union_paths("/usr/bin", ""), "/usr/bin");
        assert_eq!(union_paths("", ""), "");
        // A stray empty entry ("::") never becomes a "search the cwd" hole.
        assert_eq!(union_paths("/a::/b", "/b"), "/a:/b");
    }
}
