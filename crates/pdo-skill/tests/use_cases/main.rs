//! Stage 2 of the test pyramid (ADR-0080): the skill context's use cases
//! against in-memory adapters of every port.
//!
//! One binary for every use case, so the crate links its tests once.

mod bank;
mod delivery;
mod fakes;
mod files;
mod folders;
mod import;
mod seed;
mod sidecar;
mod update;
