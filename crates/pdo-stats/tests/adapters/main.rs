//! Stage 3 (ADR-0080): the stats context over its real adapters — SQLite
//! (the event log through the run context's ports, the absorptions), the Runs'
//! files and transcripts on disk, the price files — driven through its router.

mod absorptions;
mod common;
mod overview_cost;
mod performance;
mod store;
