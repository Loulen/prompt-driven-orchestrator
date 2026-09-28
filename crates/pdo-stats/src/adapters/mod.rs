//! The adapters of the stats context: its routes (`http/`), the absorptions'
//! store over SQLite, the Runs' event log through the run context, the Runs'
//! harness files and transcripts, the price files and the price source, and the
//! ports over the contexts it reads or tells.

pub mod contexts;
pub mod http;
pub mod models_dev;
pub mod price_files;
pub mod rename_ledger;
pub mod run_events;
pub mod run_files;
pub mod sqlite_absorptions;
pub mod transcripts;
