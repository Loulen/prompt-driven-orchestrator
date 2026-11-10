//! The adapters of the run context: its routes (`http/`) and its CLI (`cli/`),
//! the event store over SQLite and its cross-Run reads, the Runs the extensions
//! create (`extension_runs`), a Run's cost read from the harnesses' own
//! files (`run_cost/`) and the price table's disk tiers (`price_files`).

pub mod cli;
pub mod contexts;
pub mod disk_live_facts;
pub mod env;
pub mod extension_runs;
pub mod http;
pub mod instance_defaults;
pub mod manager_sessions;
pub mod pipeline_runs;
pub mod price_files;
pub mod run_cost;
pub mod run_dir_watch;
pub mod sqlite_event_store;
pub mod sqlite_event_windows;
mod sqlite_run_summaries;
