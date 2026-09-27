//! The run use cases of this slice, over the context's ports: append an event
//! (a transition, which never asks the Autorisation d'opération, D10), read a
//! Run, project every Run.

pub mod append_event;
pub mod ports;
pub mod projections;
pub mod read_run;
pub mod run_id;
