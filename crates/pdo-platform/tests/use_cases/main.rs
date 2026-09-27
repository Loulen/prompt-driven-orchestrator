//! Stage 2 (ADR-0080): the use cases of the platform context against in-memory
//! adapters of their ports. The real wiring is proven at stage 4, by the
//! daemon's integration tests.

mod fakes;

mod audit;
mod pages;
mod service;
mod update;
