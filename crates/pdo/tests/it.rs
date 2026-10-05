//! Single integration-test binary.
//!
//! Every file in `tests/` is a module of this one target instead of its own test
//! binary. Cargo statically links the whole crate into each test binary; at ~254 MB
//! a piece, 50 binaries meant ~12.6 GB of linker output and 50 serial links on every
//! full build — paid again in each fresh PDO worktree. One target, one link.
//!
//! Adding a test file: drop it in `tests/`, add a `#[path]` line below, and use
//! `crate::common::` for the shared harness (a per-file `mod common;` no longer
//! resolves from a submodule).
//!
//! Tests now share a process, so they run as threads under `cargo test`. Use
//! `cargo nextest run` for per-test process isolation.

mod common;

pub(crate) static HARNESS_PROBE_ENV_LOCK: tokio::sync::Mutex<()> =
    tokio::sync::Mutex::const_new(());

#[path = "admission_concurrency.rs"]
mod admission_concurrency;

#[path = "cli_complete_does_not_panic.rs"]
mod cli_complete_does_not_panic;

#[path = "cli_run_create.rs"]
mod cli_run_create;

#[path = "cli_wait_exit_codes.rs"]
mod cli_wait_exit_codes;

#[path = "cost_prices.rs"]
mod cost_prices;

#[path = "declared_wait.rs"]
mod declared_wait;

#[path = "daemon_bind.rs"]
mod daemon_bind;

#[path = "route_coverage.rs"]
mod route_coverage;

// Structure (ADR-0079): the hexagonal rule inside a context crate, and the
// 400-line limit per production file.
#[path = "architecture.rs"]
mod architecture;

#[path = "file_size.rs"]
mod file_size;

// The composition root: `serve(config, extensions)` and the `identify` middleware.
#[path = "composition_root.rs"]
mod composition_root;

// The session context's Operations under a refusing extension (#920).
#[path = "session_authorization.rs"]
mod session_authorization;

// The extension API (#930): the internal test extension, the Autorisation
// d'opération against the real daemon, and the demo daemon of the Feature Paths
// (ignored: it serves until killed).
mod test_extension;

#[path = "extension_authorization.rs"]
mod extension_authorization;

// The whole `pdo` binary of a distribution, `main_with(extensions)` (#23).
#[path = "main_with.rs"]
mod main_with;

#[path = "extension_mutation_routes.rs"]
mod extension_mutation_routes;

// A Trigger fire is an Operation whose Acteur is the Trigger (#927).
#[path = "extension_trigger_fire.rs"]
mod extension_trigger_fire;

// The rest of the extension API (#931): attributes, events, routes, `/ext`.
#[path = "extension_hooks.rs"]
mod extension_hooks;

// `pdo ext <id> <args…>` (#33): the agent's command line of an extension.
#[path = "extension_cli.rs"]
mod extension_cli;
// An extension's own mutations through its Host (#34): refusable by any
// extension, published on `/ws`, stamped by the others.
#[path = "extension_interop.rs"]
mod extension_interop;

// CORE-3: the catalogue of the Ressources, and the Projets of an Operation.
#[path = "extension_catalogue.rs"]
mod extension_catalogue;

// `/ext/<id>` of an extension that is not there: 404 JSON, never the SPA (#22).
#[path = "extension_not_found.rs"]
mod extension_not_found;

#[path = "demo_daemon.rs"]
mod demo_daemon;

// The refactor net (#914): one file per bounded context, for the routes no
// other test called.
#[path = "routes_pipeline.rs"]
mod routes_pipeline;

#[path = "routes_repos.rs"]
mod routes_repos;

#[path = "routes_review.rs"]
mod routes_review;

#[path = "routes_run.rs"]
mod routes_run;

#[path = "routes_settings.rs"]
mod routes_settings;

#[path = "routes_stats.rs"]
mod routes_stats;

#[path = "routes_trigger.rs"]
mod routes_trigger;

#[path = "routes_workspace.rs"]
mod routes_workspace;

#[path = "run_create_upload_errors.rs"]
mod run_create_upload_errors;

#[path = "stats_cost_by_model.rs"]
mod stats_cost_by_model;

#[path = "release_channel.rs"]
mod release_channel;
#[path = "update_apply.rs"]
mod update_apply;
#[path = "update_check.rs"]
mod update_check;

#[path = "docs_links.rs"]
mod docs_links;

#[path = "edit_self_write_loop.rs"]
mod edit_self_write_loop;

#[path = "frontmatter_validation.rs"]
mod frontmatter_validation;

#[path = "fs_browse.rs"]
mod fs_browse;

#[path = "page_mounts.rs"]
mod page_mounts;

#[path = "guard_dry_run.rs"]
mod guard_dry_run;

#[path = "guard_dry_run_timeout.rs"]
mod guard_dry_run_timeout;

#[path = "harness_catalogue_served.rs"]
mod harness_catalogue_served;

#[path = "harness_catalogue_sources.rs"]
mod harness_catalogue_sources;

#[path = "harness_catalogue_sources_pi.rs"]
mod harness_catalogue_sources_pi;

#[path = "harness_default_registration.rs"]
mod harness_default_registration;

#[path = "harness_freeze_resume.rs"]
mod harness_freeze_resume;

#[path = "harness_resume_frozen_gone.rs"]
mod harness_resume_frozen_gone;

#[path = "hot_reload_conflict.rs"]
mod hot_reload_conflict;

#[path = "libassist.rs"]
mod libassist;

#[path = "library_node_ports.rs"]
mod library_node_ports;

#[path = "log_level_default.rs"]
mod log_level_default;

#[path = "loop_command_truth.rs"]
mod loop_command_truth;

#[path = "manager_pty.rs"]
mod manager_pty;

#[path = "mid_run_edits.rs"]
mod mid_run_edits;

#[path = "multi_iter.rs"]
mod multi_iter;

#[path = "multi_repo_run.rs"]
mod multi_repo_run;

#[path = "mutation_policy.rs"]
mod mutation_policy;

#[path = "node_delivery.rs"]
mod node_delivery;

#[path = "node_done_detach.rs"]
mod node_done_detach;

#[path = "node_io.rs"]
mod node_io;

#[path = "node_prompt.rs"]
mod node_prompt;

#[path = "notes_round_trip.rs"]
mod notes_round_trip;

#[path = "pipeline_prompt_orphans.rs"]
mod pipeline_prompt_orphans;

#[path = "process_lifecycle.rs"]
mod process_lifecycle;

#[path = "project_harness_resolution.rs"]
mod project_harness_resolution;

#[path = "pty_bridge.rs"]
mod pty_bridge;

#[path = "shared_terminal.rs"]
mod shared_terminal;

#[path = "recent_repos.rs"]
mod recent_repos;

#[path = "restart_node_truth.rs"]
mod restart_node_truth;

#[path = "retry_loop_member.rs"]
mod retry_loop_member;

#[path = "run_diff_range.rs"]
mod run_diff_range;

#[path = "review_comments.rs"]
mod review_comments;

#[path = "run_naming.rs"]
mod run_naming;

#[path = "run_scoped_pipeline.rs"]
mod run_scoped_pipeline;

#[path = "run_shell.rs"]
mod run_shell;

#[path = "runs_list_target_repo.rs"]
mod runs_list_target_repo;

#[path = "run_provenance.rs"]
mod run_provenance;

#[path = "orchestrator_binding.rs"]
mod orchestrator_binding;

#[path = "sandbox_observability.rs"]
mod sandbox_observability;

#[path = "sandbox_profiles.rs"]
mod sandbox_profiles;

#[path = "skill_bank.rs"]
mod skill_bank;

#[path = "skill_document.rs"]
mod skill_document;

#[path = "skill_delivery.rs"]
mod skill_delivery;

#[path = "skill_seed.rs"]
mod skill_seed;

#[path = "sandbox_tracer.rs"]
mod sandbox_tracer;

#[path = "script_node.rs"]
mod script_node;

#[path = "serializer_round_trip.rs"]
mod serializer_round_trip;

#[path = "session_cap_admission.rs"]
mod session_cap_admission;

#[path = "service_bind_cli.rs"]
mod service_bind_cli;

#[path = "smoke_daemon.rs"]
mod smoke_daemon;

#[path = "spawn_abort_recovery.rs"]
mod spawn_abort_recovery;

#[path = "start_node.rs"]
mod start_node;

#[path = "sub_worktree_survive.rs"]
mod sub_worktree_survive;

#[path = "support_table_committed.rs"]
mod support_table_committed;

#[path = "switch_when_validation.rs"]
mod switch_when_validation;

#[path = "tmux_lifecycle.rs"]
mod tmux_lifecycle;

#[path = "tmux_run_spawn.rs"]
mod tmux_run_spawn;

#[path = "trigger_scheduler.rs"]
mod trigger_scheduler;

#[path = "turn_end_autocomplete.rs"]
mod turn_end_autocomplete;

#[path = "waiting_node_starvation.rs"]
mod waiting_node_starvation;

/// `autotests = false` means a new file in `tests/` is compiled only if it is
/// declared above — otherwise it silently never runs, and nothing fails. Catch
/// that here: every `tests/*.rs` must appear in this file's `#[path]` list.
#[test]
fn every_test_file_is_declared_in_it_rs() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let manifest = std::fs::read_to_string(dir.join("it.rs")).expect("read tests/it.rs");

    let mut undeclared: Vec<String> = std::fs::read_dir(&dir)
        .expect("read tests/")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".rs") && name != "it.rs")
        .filter(|name| !manifest.contains(&format!("#[path = \"{name}\"]")))
        .collect();
    undeclared.sort();

    assert!(
        undeclared.is_empty(),
        "these test files are not declared in tests/it.rs, so they never run: {undeclared:?}"
    );
}
