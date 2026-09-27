//! `serve(config, extensions)`: start a daemon with its extensions.

use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::Router;
use pdo_daemon::{DaemonConfig, DaemonHandle, ExtensionHostHandle, RouterLayer};
use pdo_extension_api::Extension;
use pdo_kernel::extension_host::{ExtensionHost, Extensions};

use crate::identify::with_identify;

/// How to start a daemon.
pub struct Config {
    addr: SocketAddr,
    repo_root: PathBuf,
    daemon: DaemonConfig,
    inner_layer: Option<RouterLayer>,
}

impl Config {
    /// The configuration of `pdo daemon`: listen on `bind:port` (every
    /// interface by default), keep the state under the current directory, read
    /// the environment once.
    pub fn from_cli(bind: Option<IpAddr>, port: u16) -> Result<Self> {
        let repo_root = std::env::current_dir().context("failed to determine current directory")?;
        Ok(Self {
            addr: pdo_daemon::daemon_bind_addr(bind, port),
            repo_root,
            daemon: pdo_daemon::daemon_cli_config(bind, port),
            inner_layer: None,
        })
    }

    /// Listen on `addr` (port `0` picks a free one), keep the state under
    /// `repo_root`, read the environment once.
    pub fn new(addr: SocketAddr, repo_root: impl Into<PathBuf>) -> Self {
        Self {
            addr,
            repo_root: repo_root.into(),
            daemon: DaemonConfig::from_env(),
            inner_layer: None,
        }
    }

    /// Not covered by semver: replace the legacy daemon's knobs. The test suite
    /// uses it to start a daemon that spawns no real agent.
    #[doc(hidden)]
    pub fn with_daemon_config(mut self, daemon: DaemonConfig) -> Self {
        self.daemon = daemon;
        self
    }

    /// Not covered by semver: wrap the router *inside* `identify`, so the
    /// wrapper sees every request after the actor is set. The test suite uses it
    /// to observe the actor of each request.
    #[doc(hidden)]
    pub fn with_inner_layer(
        mut self,
        layer: impl Fn(Router) -> Router + Send + Sync + 'static,
    ) -> Self {
        self.inner_layer = Some(RouterLayer::new(layer));
        self
    }
}

/// A running daemon.
pub struct Daemon {
    handle: DaemonHandle,
}

impl Daemon {
    /// The address the daemon actually listens on.
    pub fn addr(&self) -> SocketAddr {
        self.handle.addr
    }

    /// Wait until the daemon stops serving.
    pub async fn wait(self) -> Result<()> {
        self.handle.task.await.context("daemon task join error")?
    }
}

/// Start a daemon with its extensions, in registration order. Returns once the
/// daemon listens.
///
/// With no extension, the caller of every request is the local operator (or the
/// agent of the node session a CLI call comes from), every Operation is
/// allowed, and PDO behaves exactly as without the extension API.
pub async fn serve(config: Config, extensions: Vec<Box<dyn Extension>>) -> Result<Daemon> {
    let Config {
        addr,
        repo_root,
        mut daemon,
        inner_layer,
    } = config;
    let host: Arc<dyn ExtensionHost> = Arc::new(Extensions::new(extensions));
    daemon.extension_host = ExtensionHostHandle(host.clone());
    daemon.router_layer = Some(identify_layer(host, inner_layer));
    let handle = pdo_daemon::serve_with_config(addr, repo_root, daemon).await?;
    Ok(Daemon { handle })
}

/// Not covered by semver: what `serve` wraps the legacy router with — `inner`
/// (if any), then `identify` in front of everything. The test suite's daemons
/// use it so every integration test goes through the same head as `pdo daemon`.
#[doc(hidden)]
pub fn composed_router_layer(
    extensions: Vec<Box<dyn Extension>>,
    inner: Option<RouterLayer>,
) -> RouterLayer {
    identify_layer(Arc::new(Extensions::new(extensions)), inner)
}

/// `inner` (if any), then `identify` asking `host`, in front of everything.
fn identify_layer(host: Arc<dyn ExtensionHost>, inner: Option<RouterLayer>) -> RouterLayer {
    RouterLayer::new(move |router| {
        let router = match &inner {
            Some(inner) => inner.apply(router),
            None => router,
        };
        with_identify(router, host.clone())
    })
}
