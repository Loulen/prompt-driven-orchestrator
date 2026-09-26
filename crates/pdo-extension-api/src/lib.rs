//! The extension API of PDO, version 0.x (ADR-0081).
//!
//! An [`Extension`] is registered when the daemon starts (`pdo::serve(config,
//! extensions)`) and plugs into the core's generic hooks: identify the [`Actor`]
//! of a request, authorize or refuse an [`Operation`], stamp [`Attributes`] on a
//! new entity, observe events, contribute routes and UI.
//!
//! **Every hook has a neutral default.** An extension overrides only what it does;
//! with no extension installed, the caller is the local operator, everything is
//! allowed, and PDO behaves exactly as before.
//!
//! This crate depends on nothing in the core. It re-exports [`axum`] and
//! [`serde_json`], which are part of the contract: an extension's routes are an
//! axum [`Router`](axum::Router), and the events it observes are the JSON the
//! daemon already broadcasts on `/ws`, never an internal Rust enum.

mod actor;
mod entity;
mod extension;
mod manifest;
mod operation;

pub use actor::Actor;
pub use entity::{Attributes, EntityRef};
pub use extension::{Authorization, Extension, RequestParts, UiContributions};
pub use manifest::Manifest;
pub use operation::{Operation, ResourceKind, Verb};

pub use axum;
pub use serde_json;

/// The version of this API an extension was compiled against.
pub const API_VERSION: &str = env!("CARGO_PKG_VERSION");
