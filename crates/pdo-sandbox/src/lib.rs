//! The `sandbox` bounded context (ADR-0079): how a Run is isolated in a Docker
//! container — its staging profile, its image, its long-lived container, its staged
//! home.
//!
//! - [`domain`]: the [`domain::mode::SandboxMode`], the staging profiles and their
//!   resolution, the image vocabulary and plan, the container argv, the staging path
//!   math, the frozen choice of a Run. Pure.
//! - [`application`]: the ports ([`application::ports`]) — docker behind
//!   [`application::ports::DockerCli`] — the profile use cases, which go through the
//!   Autorisation d'opération, and the preparation of a Run's sandbox.
//! - [`adapters`]: the docker CLI, the SQLite store, the staged homes and the
//!   Dockerfiles on disk, the environment, and the `/settings/sandbox-profiles*`
//!   routes.
//!
//! Upstream of it: the harness context (the staging sets each harness declares) and
//! the kernel. The run, session and settings contexts depend on this one, never the
//! reverse.

pub mod adapters;
pub mod application;
pub mod domain;
