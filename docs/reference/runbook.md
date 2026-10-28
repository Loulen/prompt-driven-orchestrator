# Runbook

Operating a PDO daemon: where its logs go, and how to turn up the detail of one part of it.

## Logs

The daemon logs to stderr, through `tracing`. Under the service unit (`pdo service install`),
that is the journal: `journalctl --user -u pdo -f` on Linux, the LaunchAgent's log file on macOS.
In a terminal, it is the terminal.

Without `RUST_LOG`, the daemon logs at `info` for everything: the startup banner (`PDO daemon
listening on …`), the boot's fix-ups, the warnings. `RUST_LOG` takes the usual
[`EnvFilter`](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html)
directives, and a target is the Rust module path of the code that logs:

```bash
RUST_LOG=info,pdo_run::application::scheduler=debug pdo daemon
```

A target names a module, and every module below it: `pdo_run=debug` turns up the whole run
context, `pdo_run::application::node::spawn=debug` only the spawns.

## Targets, by bounded context

PDO is one crate per bounded context (ADR-0079), and a target starts with the crate:

| Crate | What logs there |
| --- | --- |
| `pdo` | The composition root: the boot (`pdo::daemon`), its database and fix-ups (`pdo::daemon::storage`), its background loops (`pdo::daemon::background`), the WebSocket origins it accepts (`pdo::daemon::origins`), the extensions (`pdo::extensions`), the session context's Run-side ports (`pdo::ports::session`) |
| `pdo_run` | A Run: its events, the scheduler, the nodes' lifecycle, the Pipeline Manager's commands, creation, archive |
| `pdo_pipeline` | The pipeline documents, the registry, the watcher, the migrator, the workflow import |
| `pdo_review` | The review of a Run's diff and its comments |
| `pdo_session` | tmux, the terminals, the declared wait, the reaper, the library assistant, the Sessions hébergées (`pdo_session::application::hosted`) |
| `pdo_harness` | The harnesses: registry, probes, catalogues |
| `pdo_sandbox` | Docker, the staging of a sandbox's home, the staging profiles |
| `pdo_skill` | The Banque de skills, its import, the delivery into a worktree |
| `pdo_trigger` | The Triggers, their heartbeat, their fires and guards |
| `pdo_stats` | The Stats, the absorptions, the price sync |
| `pdo_workspace` | The Projets, the repositories, git, the worktrees, the provisioning |
| `pdo_settings` | The instance settings, the agentic profiles, who names a skill or a staging profile |
| `pdo_platform` | The installed daemon: update, service unit, page mounts, audit journal |
| `pdo_kernel` | The shared kernel: the event bus, the extension host, the authorization |

## Old targets → new targets

Up to 1.110, the daemon was a single crate, `pdo_daemon`, and every target started with it. The
migration to one crate per context (spec #913) moved the code, and its targets with it. A
`RUST_LOG=pdo_daemon…` in a service unit or a script now matches nothing: replace it with the
new target below.

| Old target | New target |
| --- | --- |
| `pdo_daemon` (the startup banner, the boot, the seed, the background loops) | `pdo::daemon` (`::storage`, `::background`, `::origins`) |
| `pdo_daemon` (the session context's Run-side facts) | `pdo::ports::session` |
| `pdo_daemon` (append an event, `/ws`) | `pdo_run::application::append_event`, `pdo_run::adapters::http::ws` |
| `pdo_daemon::event_log` | `pdo_run::domain::projection` |
| `pdo_daemon::scheduler`, `pdo_daemon::scheduler_interpreter` | `pdo_run::application::scheduler`, `pdo_run::application::scheduler::interpreter` |
| `pdo_daemon::run_advance` | `pdo_run::application::scheduler::advance` |
| `pdo_daemon::run_command` | `pdo_run::application::commands` |
| `pdo_daemon::node_spawn` | `pdo_run::application::node::spawn` |
| `pdo_daemon::node_primitives` | `pdo_run::application::node::primitives` |
| `pdo_daemon::prompt_augmenter` | `pdo_run::application::node::prompt` |
| `pdo_daemon::stale_detector` | `pdo_run::application::stale` |
| `pdo_daemon::boot_recovery` | `pdo_run::application::boot_recovery` |
| `pdo_daemon::run_sandbox` | `pdo_run::application::sandbox` |
| `pdo_daemon::graph_resolver`, `::loop_region`, `::edge_router`, `::input_resolution` | `pdo_run::domain::graph_resolver`, `::loop_region`, `::edge_router`, `::input_resolution` |
| `pdo_daemon` (the node handlers) | `pdo_run::application::{complete_node, fail_node, skip_node, start_node, stop_node, retry_node, release_completion}` |
| `pdo_daemon` (create, archive, forget, repositories, run-scoped pipeline, manager) | `pdo_run::application::{create_run, archive, forget_run, edit_repos, run_pipeline, manager, retry_all}` |
| `pdo_daemon::price_table` | `pdo_run::domain::prices` |
| `pdo_daemon::run_cost` | `pdo_run::adapters::run_cost` |
| `pdo_daemon::run_refs` | `pdo_run::application::refs` |
| `pdo_daemon::review_comments` | `pdo_run::domain::review_comment` (the projection), `pdo_review::domain::comments` (the rules) |
| `pdo_daemon` (the review handlers), `pdo_daemon::structured_diff` | `pdo_review::application::{run_diff, structured_diff, list_comments, send_comments, reply, decide, comment_events}` |
| `pdo_daemon` (the paste of a batch into the manager) | `pdo_review::adapters::runs` |
| `pdo_daemon` (the pipeline handlers) | `pdo_pipeline::application` |
| `pdo_daemon::pipeline_watcher` | `pdo_pipeline::adapters::watcher` |
| `pdo_daemon::pipeline_migrator` | `pdo_pipeline::adapters::fs_migration` |
| `pdo_daemon::pipeline`, `::mutation_validator`, `::portable_pipeline`, `::workflow_importer` | `pdo_pipeline::domain::pipeline`, `::mutation`, `::portable`, `::workflow_import` |
| `pdo_daemon::library_store` | `pdo_pipeline::adapters::library_fs` |
| `pdo_daemon::tmux_session_manager` | `pdo_session::adapters::tmux` (and `pdo_session::application::launch` for the spawns) |
| `pdo_daemon::pty_bridge` | `pdo_session::adapters::pty` |
| `pdo_daemon::declared_wait`, `::shared_terminal` | `pdo_session::application::declared_wait`, `::shared_terminal` |
| `pdo_daemon::harness_registry` | `pdo_harness::application::load_registry` |
| `pdo_daemon::harness_probes`, `::harness_catalogue` | `pdo_harness::adapters::binary_probe` |
| `pdo_daemon::sandbox_staging` | `pdo_sandbox::adapters::staging` |
| `pdo_daemon::sandbox_container` | `pdo_sandbox::application::container` |
| `pdo_daemon::sandbox_run` | `pdo_sandbox::application::prepare` |
| `pdo_daemon::sandbox_image`, `::sandbox_profile` | `pdo_sandbox::application::image`, `::profiles` |
| `pdo_daemon::skill_bank`, `::skill_import`, `::skill_delivery` | `pdo_skill::application` (`::import_skills`, `::deliver`…), `pdo_skill::adapters::delivery` |
| `pdo_daemon` (the Trigger handlers, the scheduler's tick and fire), `pdo_daemon::trigger_scheduler` | `pdo_trigger::application` (`::fire`, `::fire::tick`, `::create_trigger`, `::update_trigger`…), `pdo_trigger::adapters::heartbeat` |
| `pdo_daemon::trigger_store` | `pdo_trigger::adapters::sqlite` |
| `pdo_daemon::guard_runner` | `pdo_trigger::adapters::guard_shell` |
| `pdo_daemon` (the price sync and refresh) | `pdo_stats::application::prices` |
| `pdo_daemon::stats`, `::stats_performance` | `pdo_stats::application` (`::performance`…) |
| `pdo_daemon::stats_absorption` | `pdo_stats::application::absorptions` (`::record_rename`…) |
| `pdo_daemon::project_store`, `::provisioning` | `pdo_workspace::application`, `pdo_workspace::adapters::sqlite` |
| `pdo_daemon::worktree_ops` | `pdo_workspace::adapters::git::worktrees` |
| `pdo_daemon::repo_scaffold` | `pdo_workspace::adapters::git::scaffold` |
| `pdo_daemon` (the settings handlers, the boot's warning), `pdo_daemon::instance_config`, `::agent_profile` | `pdo_settings::application` (`::boot`, `::agent_profiles::read`…), `pdo_settings::adapters::sqlite` |
| `pdo_daemon` (who names a skill or a staging profile) | `pdo_settings::adapters::contexts` |
| `pdo_daemon::update_check`, `::update_executor` | `pdo_platform::application` (`::check_for_update`, `::apply_update`), `pdo_platform::adapters::detached_executor` |
| `pdo_daemon::service_unit` | `pdo_platform::application` (`::install_service`, `::service_status`…) |
| `pdo_daemon::page_mount`, `::audit_log` | `pdo_platform::application` (`::mount_page`, `::record_audit`…), `pdo_platform::adapters` |

The default filter follows: it was `pdo_daemon=info,info`, it is `info`.
