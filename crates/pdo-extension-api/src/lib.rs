//! The extension API of PDO, version 0.x (ADR-0081).
//!
//! An [`Extension`] is registered when the daemon starts (`pdo::serve(config,
//! extensions)`) and plugs into the core's generic hooks: identify the [`Actor`]
//! of a request (and refuse one nobody identified, [`Admission`]), show an
//! Actor's name ([`ActorDisplay`]), authorize or refuse an [`Operation`], check
//! the data a creation carries for it ([`ExtensionData`]) and stamp
//! [`Attributes`] on a new entity, observe events, contribute routes and UI, answer a command line
//! (`pdo ext <id> <args…>`), bring skills into the Banque de skills, contribute
//! to the launch of every agent session (environment variables, a preamble),
//! observe who starts a session, takes control of its terminal and when it
//! ends ([`SessionEvent`]), suspend a Trigger, see a Run as people do: who
//! has it open ([`Presence`]) and what it cost ([`RunCost`]), and have the core
//! hold agents and shells outside any Run ([`HostedSession`]).
//!
//! An extension runs its **own** mutations through the same mechanisms, by the
//! [`Host`] its [`ExtensionContext`] carries: its entities are a
//! [`ResourceKind::Extension`], their Operations are refusable by every
//! extension, its events reach `/ws` and the other extensions, and the other
//! extensions stamp their attributes on its entities.
//!
//! An [`Operation`] carries the Projets it is in (empty: in none), which the
//! core fills for its own and an extension for its own; the [`Manifest`]
//! declares the Ressources an extension owns, and the core exposes the whole
//! catalogue ([`Host::catalogue`], `GET /resources`).
//!
//! The reads the core guards are Operations too ([`Verb::Read`]): listing and
//! reading the Runs (a refused Run is absent from the lists and from `/ws`,
//! `403` for its detail), the Projets, browsing the disk. The UI asks
//! whether an Operation would be allowed, without running it (`POST
//! /authorize`), to hide what would be refused.
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
mod admission;
mod authorization;
mod catalogue;
mod cli;
mod display;
mod entity;
mod event;
mod extension;
mod host;
mod hosted;
mod hosted_event;
mod launch;
mod manifest;
mod operation;
mod run_creation;
mod run_view;
mod session;
mod skill;
mod ui;

pub use actor::Actor;
pub use admission::{Admission, UNAUTHENTICATED};
pub use authorization::{Authorization, Denied, OPERATION_REFUSED};
pub use catalogue::{catalogue, core_resources, Resource, ResourceDeclaration};
pub use cli::{CliInvocation, CliOutput, CliSpec, EXIT_NO_CLI};
pub use display::ActorDisplay;
pub use entity::{Attributes, EntityRef};
pub use event::{OperationApplied, OPERATION_APPLIED};
pub use extension::{Extension, ExtensionContext, RequestParts};
pub use host::{ExtensionEvent, Host, NeutralHost, EXTENSION_EVENT};
pub use hosted::{
    EndReason, HostedConfig, HostedFolder, HostedGesture, HostedKind, HostedRefusal, HostedSession,
    HostedState, LiveArtifact, OpenSession, DEFAULT_TERMINAL_ZOOM, MAX_TERMINAL_ZOOM,
    MIN_TERMINAL_ZOOM,
};
pub use hosted_event::{
    HostedLiveArtifactsChanged, HostedSessionChanged, HostedSessionForgotten,
    HOSTED_LIVE_ARTIFACTS_CHANGED, HOSTED_SESSION_CHANGED, HOSTED_SESSION_FORGOTTEN,
};
pub use launch::{NodeEnv, NodeKind, NodeLaunch, TriggerProvenance, TriggerRef};
pub use manifest::Manifest;
pub use operation::{Operation, ResourceKind, Verb};
pub use run_creation::{ExtensionData, NewRun, RunCreation, RunNotCreated, RunRepo};
pub use run_view::{Presence, RunCost, RunCostRead, PRESENCE_ARRIVED, PRESENCE_LEFT};
pub use session::{
    SessionChange, SessionEvent, SessionKind, SessionLaunch, SessionRef, SESSION_ENDED,
    SESSION_STARTED, SESSION_TAKEN_OVER,
};
pub use skill::{ExtensionSkill, SkillFile};
pub use ui::{
    CommandOperation, CompletionRender, ContextKey, HostedLocation, Slot, UiCommand,
    UiContribution, UiContributions, UiElement, UiElementKind, UiLocation, UiSlot, When, UI_MODULE,
    UI_MODULE_MAJOR,
};

pub use axum;
pub use serde_json;

/// The version of this API an extension was compiled against.
pub const API_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The oldest version of this API that [`API_VERSION`] still honours: an
/// extension requiring any version from this one up to [`API_VERSION`] runs.
/// In 0.x a minor may break; this is where the API says which ones did not
/// (0.2 only added to 0.1, ADR-0081 amendment #29; 0.3 broke only compilation,
/// amendment #34; 0.4 only added to 0.3, amendment #32; 0.8 only added
/// fields, methods, hooks, types, the verb `read` and the `topbar/account`
/// location, amendments CORE-3, CORE-4, CORE-5 and CORE-2; 0.9 only added the
/// extension data of a creation, its hooks, `Host::create_run` and the
/// `stats/tab` and `settings/page` locations, amendments CORE-29 and CORE-30;
/// 0.10 only added the probed opening of the WebSockets, amendment CORE-32;
/// 0.11 only added `Host::project_name` and `Host::triggers_changed`; 0.12
/// only added the Presences, `Host::presences`, `Host::run_cost` and the
/// `run/item/people` location; 0.13 only added the launch hooks of every
/// session, `session_env` and `session_preamble`, and the session events; 0.16
/// only added `NewRun::project_id`, the Projet a Run is launched in; 0.17 only
/// added the primitives of the shared module `@pdo/extension-ui`, ADR-0091;
/// 0.18 only added the `palette` location, the Sources de recherche and the
/// Operation a command makes; 0.19 only added the Sessions hébergées: the
/// Host's `open_session`, `hosted_session`, `hosted_sessions`,
/// `terminate_session`, `relaunch_session`, `configure_session`,
/// `type_in_session`, `forget_session`, `zoom_session`, `live_artifacts` and
/// `live_artifact`, `SessionKind::Hosted`, the extension and key of a
/// `SessionRef`, and the `hosted_session_changed`, `hosted_session_forgotten`
/// and `hosted_live_artifacts_changed` events).
pub const OLDEST_COMPATIBLE_API_VERSION: &str = "0.1.0";
