//! Stage 2 (ADR-0080): the use cases of the settings context over in-memory
//! ports — every mutation asks the Autorisation d'opération first, a refused or
//! invalid edit stores nothing, the page is the union of the contexts'
//! sections. They replace the legacy crate's router `oneshot` tests of the
//! settings and the agentic profiles.

mod agent_profiles;
mod boot;
mod fakes;
mod page;
mod referents;
mod update;
