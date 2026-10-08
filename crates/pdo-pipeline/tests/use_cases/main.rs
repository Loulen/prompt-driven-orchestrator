//! Stage 2 of the test pyramid (ADR-0080): the pipeline context's use cases
//! against in-memory adapters of every port.
//!
//! One binary for every use case, so the crate links its tests once.

mod authorization;
mod create_delete;
mod documents;
mod fakes;
mod library;
mod overwrite;
mod queries;
mod rename;
mod save;
mod workflow;
