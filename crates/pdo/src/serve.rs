//! `serve(config, extensions)`: start a daemon with its extensions.

use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use crate::daemon::env::{daemon_bind_addr, daemon_cli_config};
use crate::daemon::router_layer::RouterLayer;
use crate::daemon::{serve_with_config, DaemonConfig, DaemonHandle, ExtensionHostHandle};
use anyhow::{Context, Result};
use axum::Router;
use pdo_extension_api::Extension;
use pdo_kernel::extension_host::{ExtensionHost, Extensions, HostServices};
use pdo_run::adapters::extension_runs::ExtensionRuns;
use pdo_platform::domain::release_channel::UpdateChannel;

use crate::attributes::SqliteAttributes;
use crate::contexts::{self, ContextResources};
use crate::extensions;
use crate::identify::with_identify;

/// How to start a daemon.
pub struct Config {
    addr: SocketAddr,
    repo_root: PathBuf,
    daemon: DaemonConfig,
    inner_layer: Option<RouterLayer>,
    release_channel: Option<ReleaseChannel>,
}

/// Where a binary checks for its updates, which version it declares installed,
/// and how it updates itself: its release channel.
///
/// A distribution (a binary that calls [`serve`] with its own extensions) is
/// not the vanilla `pdo`: its version is its own, its releases are published
/// elsewhere, and the vanilla upgrade command would replace it with a `pdo`
/// that has none of its extensions. It declares its channel with
/// [`Config::with_release_channel`]; the daemon then never tries to detect how
/// the binary was installed (Homebrew path, cargo-dist receipt).
///
/// ```no_run
/// # async fn start() -> anyhow::Result<()> {
/// let config = pdo::Config::from_cli(None, 5172)?.with_release_channel(pdo::ReleaseChannel {
///     name: "pdo-dist".into(),
///     installed_version: "0.3.0".into(),
///     source_url: Some("https://api.github.com/repos/acme/pdo-dist/releases/latest".into()),
///     upgrade_command: Some("brew update && brew upgrade acme/tap/pdo-dist".into()),
/// });
/// pdo::serve(config, vec![]).await?.wait().await
/// # }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseChannel {
    /// Shown in the UI (`pdo-dist`…), next to the version of the core the
    /// binary is built on. `PDO` is the vanilla binary's name.
    pub name: String,
    /// The version compared with the releases of `source_url`: the
    /// distribution's, not the core's.
    pub installed_version: String,
    /// An endpoint in the format of GitHub's `releases/latest` (a JSON object
    /// with a `tag_name`); the release notes are read from the same URL without
    /// its `/latest`. `None`: no version check. The operator's
    /// `PDO_UPDATE_SOURCE_URL` still wins over it.
    pub source_url: Option<String>,
    /// The command the Update button shows and runs, detached from the daemon,
    /// before the service is reinstalled and restarted. `None`: no Update
    /// button, only the reason.
    pub upgrade_command: Option<String>,
}

impl ReleaseChannel {
    /// The platform's view of the channel. Not a `From` impl: that would put a
    /// type of an uncovered crate into the semver-covered surface.
    fn into_update_channel(self) -> UpdateChannel {
        UpdateChannel::declared(
            self.name,
            self.installed_version,
            self.source_url,
            self.upgrade_command,
        )
    }
}

impl Config {
    /// The configuration of `pdo daemon`: listen on `bind:port` (every
    /// interface by default), keep the state under the current directory, read
    /// the environment once.
    pub fn from_cli(bind: Option<IpAddr>, port: u16) -> Result<Self> {
        let repo_root = std::env::current_dir().context("failed to determine current directory")?;
        Ok(Self {
            addr: daemon_bind_addr(bind, port),
            repo_root,
            daemon: daemon_cli_config(bind, port),
            inner_layer: None,
            release_channel: None,
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
            release_channel: None,
        }
    }

    /// Declare the release channel of this binary. Without this call the daemon
    /// is the vanilla `pdo`: named `PDO`, at the core's version, with no release
    /// source, updated through the install method it detects.
    pub fn with_release_channel(mut self, channel: ReleaseChannel) -> Self {
        self.release_channel = Some(channel);
        self
    }

    /// Not covered by semver: replace the daemon's knobs. The test suite
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
///
/// Refuses to start, before opening anything, when an extension's manifest does
/// not pass registration: an invalid or shared id, or a version of the extension
/// API this build does not implement. The error says which and why.
pub async fn serve(config: Config, extensions: Vec<Box<dyn Extension>>) -> Result<Daemon> {
    let Config {
        addr,
        repo_root,
        mut daemon,
        inner_layer,
        release_channel,
    } = config;
    if let Some(channel) = release_channel {
        daemon.release_channel = Some(channel.into_update_channel());
    }
    compose(&mut daemon, extensions, inner_layer)?;
    let handle = serve_with_config(addr, repo_root, daemon).await?;
    Ok(Daemon { handle })
}

/// Not covered by semver: register `extensions` and plug into the daemon's
/// knobs what `serve` plugs — the extension host every context asks, the
/// wiring of the contexts (which starts the host: the attribute
/// store, the data folders, the events), the extensions' routes under
/// `/ext/<id>/`, `inner` (if any) around the assembled router, then `identify`
/// in front of everything. The test suite's daemons use it so they go through
/// the same head as `pdo daemon`.
#[doc(hidden)]
pub fn compose(
    daemon: &mut DaemonConfig,
    extensions: Vec<Box<dyn Extension>>,
    inner: Option<RouterLayer>,
) -> Result<()> {
    let extensions = Extensions::register(extensions).context("the daemon refused an extension")?;
    daemon.extension_host = ExtensionHostHandle(Arc::new(extensions.clone()));
    daemon.router_layer = Some(identify_layer(extensions, inner));
    Ok(())
}

/// Not covered by semver: [`compose`]'s router head for a daemon with no
/// extension, for the tests that assemble their [`DaemonConfig`] by hand.
#[doc(hidden)]
pub fn composed_router_layer(inner: Option<RouterLayer>) -> RouterLayer {
    identify_layer(Extensions::default(), inner)
}

/// The wiring of the contexts (then the start of the extension
/// host), the extensions' routes, `inner` (if any), then `identify` asking the
/// host, in front of everything.
fn identify_layer(extensions: Extensions, inner: Option<RouterLayer>) -> RouterLayer {
    let host: Arc<dyn ExtensionHost> = Arc::new(extensions.clone());
    let contexts_host = host.clone();
    let started = extensions.clone();
    RouterLayer::new(move |router| {
        let router = extensions::mount(router, &extensions);
        let router = match &inner {
            Some(inner) => inner.apply(router),
            None => router,
        };
        with_identify(router, host.clone())
    })
    .with_contexts(move |resources| {
        started.start(HostServices {
            attributes: Arc::new(SqliteAttributes::new(resources.db.clone())),
            bus: resources.bus.clone(),
            data_root: extensions_data_root(&resources),
        });
        let contexts = contexts::wire(resources, contexts_host.clone());
        // API 0.9: an extension's `Host::create_run` creates through the Runs.
        started.serve_runs(Arc::new(ExtensionRuns::of(&contexts.run)));
        contexts
    })
}

/// Where the extensions keep their data: `~/.pdo/extensions/` (the daemon's
/// home override in the tests), or under the daemon's own `.pdo/` without a
/// home.
fn extensions_data_root(resources: &ContextResources) -> PathBuf {
    resources
        .home_root
        .clone()
        .unwrap_or_else(|| resources.repo_root.clone())
        .join(".pdo")
        .join("extensions")
}
