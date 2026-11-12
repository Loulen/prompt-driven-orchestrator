//! The session use cases, one per file, and the ports they need (`ports.rs`).
//! Every mutation builds an Operation and goes through the Autorisation
//! d'opération; the internal transitions (the orphan sweep, the daemon's own
//! declared wait on a refused completion) never do.

pub mod assistant;
pub mod attach;
pub mod declared_wait;
pub mod hosted;
pub mod ignore_size;
pub mod launch;
pub mod messages;
pub mod open_shell;
pub mod ports;
pub mod read_pane;
pub mod reap;
pub mod session_ledger;
pub mod shared_terminal;
pub mod status;
pub mod sweep;
pub mod terminal;
pub mod terminal_text;
pub mod typed_input;
pub mod write_to_node;
