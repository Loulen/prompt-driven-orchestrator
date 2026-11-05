//! Structure test (ADR draft « chaque route a sa commande CLI, un test le
//! vérifie ») — **every route of the daemon is called by a `pdo` command, or is
//! excluded with its reason**, method by method.
//!
//! Adding a route without a command or an exclusion turns this red, and so
//! does an exclusion left behind once its command calls the route. How the
//! router and the CLI are read: `src/cli_parity/mod.rs`; the exclusions:
//! `src/cli_parity/exclusions.rs`.

use tidy::cli_parity::exclusions::{Excluded, Pending, EXCLUDED, PENDING};
use tidy::cli_parity::{cli_sources, parity, without_tests};
use tidy::routes::{context_routes, router_routes, Route};

fn listing(routes: &[Route]) -> String {
    routes
        .iter()
        .map(|(m, p)| format!("  {m} {p}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn every_route_has_its_command_or_a_reason() {
    let parity = parity(&context_routes(), &cli_sources(), EXCLUDED, PENDING);
    assert!(
        parity.uncovered.is_empty(),
        "{} route(s) have no `pdo` command and no exclusion — write the command (a thin \
         client of the route, see crates/pdo-platform/src/adapters/cli/daemon.rs), or \
         exclude the route with its reason in crates/tidy/src/cli_parity/exclusions.rs:\n{}",
        parity.uncovered.len(),
        listing(&parity.uncovered)
    );
    assert!(
        parity.covered_yet_excluded.is_empty(),
        "a command calls these routes: remove their exclusion from \
         crates/tidy/src/cli_parity/exclusions.rs:\n{}",
        listing(&parity.covered_yet_excluded)
    );
    assert!(
        parity.unknown.is_empty(),
        "these exclusions name a route the daemon does not declare (renamed? removed?):\n{}",
        listing(&parity.unknown)
    );
    assert!(
        parity.doubled.is_empty(),
        "these routes are excluded twice:\n{}",
        listing(&parity.doubled)
    );
    assert!(
        parity.unmotivated.is_empty(),
        "these exclusions give no reason:\n{}",
        listing(&parity.unmotivated)
    );
    assert!(parity.holds());
}

#[test]
fn the_provisional_exclusions_are_dated_and_named_by_family() {
    for pending in PENDING {
        assert!(
            !pending.family.trim().is_empty(),
            "a pending family has no name"
        );
        let date = pending.since.as_bytes();
        assert!(
            date.len() == 10
                && date[4] == b'-'
                && date[7] == b'-'
                && date
                    .iter()
                    .enumerate()
                    .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit()),
            "{}: `since` is not a YYYY-MM-DD date: {}",
            pending.family,
            pending.since
        );
        assert!(!pending.routes.is_empty(), "{}: no route", pending.family);
    }
}

#[test]
fn the_cli_reader_sees_the_operator_s_commands() {
    let sources = cli_sources();
    let files: Vec<&str> = sources.iter().map(|(f, _)| f.as_str()).collect();
    for expected in [
        "crates/pdo-workspace/src/adapters/cli.rs",
        "crates/pdo-trigger/src/adapters/cli.rs",
        "crates/pdo-run/src/adapters/cli/run_status.rs",
        "crates/pdo-review/src/adapters/cli.rs",
        "crates/pdo/src/cli/ext.rs",
    ] {
        assert!(
            files
                .iter()
                .any(|f| f.starts_with(expected.trim_end_matches(".rs"))),
            "{expected} missing from {files:?}"
        );
    }
    let routes = context_routes();
    let called = tidy::routes::calls_in(&sources, &routes.iter().map(|(_, p)| p.clone()).collect());
    for (method, path) in [
        ("POST", "/projects"),
        ("PATCH", "/projects/{project_id}"),
        ("DELETE", "/projects/{project_id}/members"),
        ("GET", "/projects/{project_id}/provisioning"),
        ("PUT", "/projects/{project_id}/provisioning"),
        ("POST", "/triggers/{trigger_id}/fire"),
        ("POST", "/triggers/pause"),
        ("GET", "/runs/{run_id}/children"),
    ] {
        assert!(
            called.contains_key(&(method.to_string(), path.to_string())),
            "{method} {path} is not seen as called"
        );
    }
}

/// The demonstration the rule asks for: a route added without a command or an
/// exclusion comes out uncovered, and an exclusion of a route a command calls
/// is caught.
#[test]
fn a_route_without_a_command_turns_the_parity_red() {
    let src = std::fs::read_to_string(
        tidy::workspace_root().join("crates/pdo-trigger/src/adapters/http/mod.rs"),
    )
    .unwrap();
    let injected = src.replacen(
        "    Router::new()\n",
        "    Router::new()\n        .route(\"/triggers/fictitious\", get(fictitious))\n",
        1,
    );
    assert_ne!(injected, src, "the injection point moved");
    let routes = router_routes(&injected, "pub fn router(");
    let none: &[Pending] = &[];
    let red = parity(&routes, &cli_sources(), &[], none);
    assert_eq!(
        red.uncovered,
        vec![("GET".to_string(), "/triggers/fictitious".to_string())]
    );

    const STALE: &[Excluded] = &[Excluded {
        reason: "stale",
        routes: &[("GET", "/triggers"), ("GET", "/triggers/fictitious")],
    }];
    let stale = parity(&routes, &cli_sources(), STALE, none);
    assert!(stale.uncovered.is_empty(), "{:?}", stale.uncovered);
    assert_eq!(
        stale.covered_yet_excluded,
        vec![("GET".to_string(), "/triggers".to_string())]
    );

    const UNKNOWN: &[Pending] = &[Pending {
        family: "Gone",
        since: "2026-11-05",
        routes: &[("GET", "/triggers/fictitious"), ("GET", "/no/such/route")],
    }];
    let unknown = parity(&routes, &cli_sources(), &[], UNKNOWN);
    assert_eq!(
        unknown.unknown,
        vec![("GET".to_string(), "/no/such/route".to_string())]
    );
}

#[test]
fn a_unit_test_s_literal_is_no_call() {
    let src = "fn a() {}\n#[cfg(test)]\nmod tests {\n    fn t() { d.get(\"/triggers\"); }\n}\n";
    assert_eq!(without_tests(src), "fn a() {}\n");
    assert_eq!(without_tests("fn a() {}\n"), "fn a() {}\n");
}
