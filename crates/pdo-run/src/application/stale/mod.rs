//! The liveness sweep: dead sessions, turn ends, idle agents. A transition of
//! the daemon, which never asks the Autorisation d'opération (D10).

mod assess;
pub mod detector;
mod diagnostics;
pub mod sweep;
