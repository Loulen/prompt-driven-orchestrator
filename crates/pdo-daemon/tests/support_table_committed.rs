//! The committed harness support table must match what the code declares (#617).
//!
//! `make check` is the gate a developer feels ([`Makefile`]'s `docs support-table
//! --check`); this test is the same gate in CI, so a PR that changes a capability
//! and forgets `make support-table` fails on `cargo test --workspace` rather than
//! shipping a reference page that quietly lies.
//!
//! Since #855 the table lives in `docs/reference/harnesses.md`, the CLI command
//! table in `docs/reference/cli.md`, and the README is a showcase with no
//! generated block.

use std::path::PathBuf;

fn repo_file(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

fn read(relative: &str) -> String {
    let path = repo_file(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()))
}

/// The one file carrying the generated block.
fn harnesses() -> String {
    read(pdo_daemon::harness_support::DOCUMENT)
}

#[test]
fn the_committed_support_table_matches_the_capability_declaration() {
    let document = harnesses();
    if let Err(why) = pdo_daemon::harness_support::check(&document) {
        panic!("{}: {why}", pdo_daemon::harness_support::DOCUMENT);
    }
}

#[test]
fn the_readme_carries_no_generated_block() {
    // One generated copy, one gate: a second block left in the README would
    // drift silently, since `make check` only looks at the reference page.
    for readme in ["README.md", "docs/readme/README.fr.md"] {
        let document = read(readme);
        assert!(
            !document.contains(pdo_daemon::harness_support::BEGIN_MARKER)
                && !document.contains(pdo_daemon::harness_support::END_MARKER),
            "{readme} still carries a generated support-table block"
        );
    }
}

#[test]
fn the_harness_reference_carries_the_harness_prerequisites() {
    // The other half of #617's promise: what PDO *assumes* you configured, and
    // does not configure for you, is named. Pinned so the section cannot be
    // dropped in a docs tidy — the trust-dialog paragraph in particular is the
    // one measured failure that leaves a node alive and mute.
    let document = harnesses();
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

#[test]
fn the_cli_reference_has_one_command_table_and_no_scattered_daemon_or_service_blocks() {
    let document = read("docs/reference/cli.md");
    let commands = [
        "pdo daemon",
        "pdo service install",
        "pdo service status",
        "pdo service uninstall",
        "pdo complete",
        "pdo fail",
        "pdo skip",
        "pdo wait-user",
        "pdo run wait",
        "pdo migrate",
        "pdo reap",
        "pdo docs support-table",
        "pdo run create",
        "pdo page mount",
        "pdo review list",
        "pdo review reply",
    ];
    for command in commands {
        assert!(
            document
                .lines()
                .any(|line| line.starts_with("| `pdo ") && line.contains(command)),
            "CLI command table is missing `{command}`"
        );
    }

    let mut in_code_block = false;
    for line in document.lines() {
        if line.starts_with("```") {
            in_code_block = !in_code_block;
        } else if in_code_block {
            assert!(
                !line.contains("pdo daemon") && !line.contains("pdo service"),
                "daemon and service examples belong in the CLI command table: {line}"
            );
        }
    }
}
