//! The committed README support table must match what the code declares (#617).
//!
//! `make check` is the gate a developer feels ([`Makefile`]'s `docs support-table
//! --check`); this test is the same gate in CI, so a PR that changes a capability
//! and forgets `make support-table` fails on `cargo test --workspace` rather than
//! shipping a README that quietly lies.

use std::path::PathBuf;

/// The repository's README — the one file carrying the generated block.
fn readme() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../README.md")
}

#[test]
fn the_committed_support_table_matches_the_capability_declaration() {
    let path = readme();
    let document = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));

    if let Err(why) = pdo_daemon::harness_support::check(&document) {
        panic!("{}: {why}", path.display());
    }
}

#[test]
fn the_readme_carries_the_harness_prerequisites() {
    // The other half of #617's promise: what PDO *assumes* you configured, and
    // does not configure for you, is named. Pinned so the section cannot be
    // dropped in a README tidy — the trust-dialog paragraph in particular is the
    // one measured failure that leaves a node alive and mute.
    let document = std::fs::read_to_string(readme()).expect("README is readable");
    assert!(
        document.contains("## Prerequisites"),
        "the harness prerequisites section is gone"
    );
    for expected in [
        "Authentication",
        "An approved working directory",
        "An installed version",
        "trust cascades to subdirectories",
        "does not stage any harness's home",
    ] {
        assert!(
            document.contains(expected),
            "the prerequisites section no longer says: {expected}"
        );
    }
}
