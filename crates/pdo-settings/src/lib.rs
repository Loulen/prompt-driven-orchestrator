//! The `settings` bounded context (ADR-0079): the instance settings page, the
//! agentic profiles (CONTEXT.md → *Profil agentique*) and the Apparence of
//! each Actor (CONTEXT.md → *Apparence*).
//!
//! - [`domain`] holds the pure rules: the stored tier of the instance defaults
//!   and the edit a `PUT` may make of it, an agentic profile, who names one,
//!   the shape of an Apparence.
//! - [`application`] holds the ports and the use cases: the page, its edit,
//!   the profiles' create / edit / delete and their reads, the boot's warning
//!   about retired settings, the caller's Apparence. Every mutation of the
//!   instance asks the Autorisation d'opération
//!   (`instance_settings` × `update`, `agent_profile` × `create` / `update` /
//!   `delete`).
//! - [`adapters`] holds SQLite, the routes `/settings`,
//!   `/settings/agent-profiles*` and `/actors/me/appearance`, the sections of the page, and the ports the
//!   other contexts read their instance settings through.
//!
//! The hub is dissolved: each context owns its instance defaults — their env
//! var, their built-in value, their `stored → env → default` resolver and how
//! the page discloses them (`<context>::adapters::instance_defaults`). This
//! context keeps the stored tier and assembles the page. It is the last context
//! of the map: it depends on the others, never the other way round.

pub mod adapters;
pub mod application;
pub mod domain;

pub use adapters::http::router;
pub use application::settings::{Settings, SettingsPorts};
