//! Stage 3 (ADR-0080): the settings context over its real adapters — the
//! stored tier and the agentic profiles over SQLite, the page's sections over
//! the owning contexts, the routes driven through the context's router, the
//! ports the upstream contexts read. The legacy crate's in-crate tests of this
//! code, moved.

mod agent_profiles;
mod instance_config;
mod router;
mod upstream;
