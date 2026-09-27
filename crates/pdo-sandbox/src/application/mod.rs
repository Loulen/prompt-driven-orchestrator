//! The sandbox use cases and the ports they need.
//!
//! The mutations an Actor asks for — editing and deleting a staging profile — go
//! through [`profiles::SandboxProfiles`], which builds the Opération and asks the
//! Autorisation d'opération before writing. Preparing, cleaning up and killing inside
//! a Run's sandbox ([`prepare::SandboxRuntime`]) are transitions of the Run the
//! scheduler drives, never gated by an extension (CONTEXT.md § *Extensions*).

pub mod container;
pub mod context;
pub mod delete_profile;
pub mod docker_probe;
pub mod image;
pub mod ports;
pub mod prepare;
pub mod profiles;
pub mod upsert_profile;

#[cfg(test)]
pub(crate) mod testing;
