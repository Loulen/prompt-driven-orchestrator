//! Stage 2 of the test pyramid (ADR-0080): the run context's use cases
//! against in-memory adapters of every port.
//!
//! One binary for every use case, so the crate links its tests once.

mod append_event;
mod fakes;
mod projections;
mod read_run;
