//! The harness use cases and the ports they need.
//!
//! The context has no Actor-initiated mutation: its use cases read (the
//! descriptor file, the installed binaries) and cache what they read. A read
//! builds no Opération and never goes through the Autorisation d'opération
//! (CONTEXT.md § *Extensions*: it covers mutations).

pub mod harness_offers;
pub mod load_registry;
pub mod ports;
pub mod read_catalogue;
