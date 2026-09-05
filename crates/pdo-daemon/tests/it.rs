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

#[path = "cost_prices.rs"]
mod cost_prices;

#[path = "update_apply.rs"]
mod update_apply;
#[path = "update_check.rs"]
mod update_check;

#[path = "edit_self_write_loop.rs"]
mod edit_self_write_loop;

#[path = "frontmatter_validation.rs"]
mod frontmatter_validation;

#[path = "fs_browse.rs"]
mod fs_browse;

#[path = "guard_dry_run.rs"]
mod guard_dry_run;

#[path = "guard_dry_run_timeout.rs"]
mod guard_dry_run_timeout;

#[path = "harness_catalogue_served.rs"]
mod harness_catalogue_served;

#[path = "harness_catalogue_sources.rs"]
mod harness_catalogue_sources;

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

#[path = "recent_repos.rs"]
mod recent_repos;

#[path = "restart_node_truth.rs"]
mod restart_node_truth;

#[path = "retry_loop_member.rs"]
mod retry_loop_member;

#[path = "run_diff_range.rs"]
mod run_diff_range;

#[path = "run_naming.rs"]
mod run_naming;

#[path = "run_scoped_pipeline.rs"]
mod run_scoped_pipeline;

#[path = "run_shell.rs"]
mod run_shell;

#[path = "runs_list_target_repo.rs"]
mod runs_list_target_repo;

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

#[path = "sandbox_tracer.rs"]
mod sandbox_tracer;

#[path = "script_node.rs"]
mod script_node;

#[path = "serializer_round_trip.rs"]
mod serializer_round_trip;

#[path = "session_cap_admission.rs"]
mod session_cap_admission;

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
