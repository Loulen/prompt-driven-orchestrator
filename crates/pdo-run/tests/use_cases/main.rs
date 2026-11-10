//! Stage 2 of the test pyramid (ADR-0080): the run context's use cases
//! against in-memory adapters of every port.
//!
//! One binary for every use case, so the crate links its tests once.

mod append_event;
mod commands;
mod fakes;
mod gestures;
mod on_demand;
mod operation_projects;
mod ports;
mod project_tiers;
mod projections;
mod pruning;
mod read_access;
mod read_run;
mod run_summary;
mod scheduler;
mod surfaces;
