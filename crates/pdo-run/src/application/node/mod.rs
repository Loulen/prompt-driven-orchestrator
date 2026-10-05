//! A node's lifecycle below the scheduler (ADR-0009, layer 2): its spawn, the
//! primitives, its prompt, its inputs and outputs, its reap and reattach.

pub mod extension_launch;
pub mod io;
pub mod launch;
pub mod output_validation;
pub mod outputs;
pub mod primitives;
pub mod project_tiers;
pub mod prompt;
pub mod reap;
pub mod reattach;
pub mod spawn;
