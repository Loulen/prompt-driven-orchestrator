//! The extension API of PDO, version 0.x (ADR-0081).
//!
//! An [`Extension`] is registered when the daemon starts (`pdo::serve(config,
//! extensions)`) and plugs into the core's generic hooks: identify the [`Actor`]
//! of a request, authorize or refuse an [`Operation`], stamp [`Attributes`] on a
//! new entity, observe events, contribute routes and UI.
//!
//! An extension runs its **own** mutations through the same mechanisms, by the
//! [`Host`] its [`ExtensionContext`] carries: its entities are a
//! [`ResourceKind::Extension`], their Operations are refusable by every
//! extension, its events reach `/ws` and the other extensions, and the other
//! extensions stamp their attributes on its entities.
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
mod event;
mod extension;
mod host;
mod manifest;
mod operation;
mod ui;

pub use actor::Actor;
pub use entity::{Attributes, EntityRef};
pub use event::{OperationApplied, OPERATION_APPLIED};
pub use extension::{
    Authorization, Denied, Extension, ExtensionContext, RequestParts, OPERATION_REFUSED,
};
pub use host::{ExtensionEvent, Host, NeutralHost, EXTENSION_EVENT};
pub use manifest::Manifest;
pub use operation::{Operation, ResourceKind, Verb};
pub use ui::{Slot, UiContributions, UiSlot};

pub use axum;
pub use serde_json;

/// The version of this API an extension was compiled against.
pub const API_VERSION: &str = env!("CARGO_PKG_VERSION");
