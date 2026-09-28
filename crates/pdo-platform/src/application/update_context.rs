//! What the update use cases share: their ports, the daemon's update knobs, and
//! the in-memory state of the version check (the cache, the in-flight lock, the
//! memoised release list, the apply lock).

use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::Mutex;

use super::ports::{
    Installation, ReleaseSource, Runs, UpdateCheckSetting, UpdateExecutor, UpdateJournal,
};
use crate::domain::install::InstallMethod;
use crate::domain::release::Release;
use crate::domain::release_channel::UpdateChannel;
use crate::domain::update_check::UpdateCache;

/// The ports the update use cases reach the outside through.
#[derive(Clone)]
pub struct UpdatePorts {
    pub setting: Arc<dyn UpdateCheckSetting>,
    pub source: Arc<dyn ReleaseSource>,
    pub journal: Arc<dyn UpdateJournal>,
    pub installation: Arc<dyn Installation>,
    pub executor: Arc<dyn UpdateExecutor>,
    pub runs: Arc<dyn Runs>,
}

/// The per-daemon knobs of the update.
#[derive(Debug, Clone, Default)]
pub struct UpdateOptions {
    /// The release channel (#24): the vanilla one by default, or the one a
    /// distribution declares.
    pub channel: UpdateChannel,
    /// The operator's release source (`PDO_UPDATE_SOURCE_URL`, the test seam):
    /// wins over the channel's. `None` → the channel's source, if any.
    pub source_url: Option<String>,
    /// The port the daemon listens on (the relaunch and the unit keep it).
    pub port: u16,
    /// The daemon's cwd: `service install` and the relaunch resolve the same
    /// repo root from it.
    pub working_dir: PathBuf,
    /// The daemon's own command line for an unsupervised relaunch; `None` →
    /// `<stable exe> daemon --port <port>`.
    pub relaunch_command: Option<Vec<String>>,
}

/// The shared state of the update use cases.
pub struct UpdateContext {
    pub(crate) ports: UpdatePorts,
    pub(crate) options: UpdateOptions,
    /// The last check, loaded from the journal once, then kept current.
    pub(crate) cache: Mutex<Option<UpdateCache>>,
    /// Held while a check is in flight: a second one is refused, not queued.
    pub(crate) check_lock: Mutex<()>,
    /// The release list, memoised per latest version.
    pub(crate) releases: Mutex<Option<(String, Vec<Release>)>>,
    /// Serialises the applies.
    pub(crate) apply_lock: Mutex<()>,
}

impl UpdateContext {
    pub fn new(ports: UpdatePorts, options: UpdateOptions) -> Arc<Self> {
        Arc::new(Self {
            ports,
            options,
            cache: Mutex::new(None),
            check_lock: Mutex::new(()),
            releases: Mutex::new(None),
            apply_lock: Mutex::new(()),
        })
    }

    /// The release-source URL: the operator's override, else the channel's;
    /// `None` when neither names one (nothing is checked).
    pub(crate) fn source_url(&self) -> Option<String> {
        self.options
            .source_url
            .clone()
            .or_else(|| self.options.channel.source_url.clone())
    }

    pub(crate) fn channel(&self) -> &UpdateChannel {
        &self.options.channel
    }

    /// The install method an Update delegates to and the command it runs: the
    /// detected method's for the vanilla channel, the declared one otherwise
    /// (the detection is then never consulted).
    pub(crate) fn upgrader(&self) -> (InstallMethod, Option<String>) {
        self.options
            .channel
            .upgrader(|| self.ports.installation.method())
    }

    /// Load the journal's cache into memory once (a no-op once loaded).
    pub(crate) async fn ensure_loaded(&self) {
        let mut memo = self.cache.lock().await;
        if memo.is_some() || !self.ports.journal.is_available() {
            return;
        }
        *memo = self.ports.journal.read_cache();
    }

    /// The last check of the current source. A cache written for another
    /// source (the same home shared with another binary), or any cache when
    /// there is no source, is no answer.
    pub(crate) async fn cached(&self) -> Option<UpdateCache> {
        let url = self.source_url()?;
        self.cache.lock().await.clone().filter(|c| c.is_for(&url))
    }

    /// Hit the release source once and record the outcome — success or failure —
    /// in memory and in the journal. Returns the recorded cache on success, the
    /// error message on failure. Does NOT consult the setting: the callers do,
    /// each at its own edge. Without a source, nothing leaves the daemon.
    pub(crate) async fn perform_check(&self) -> Result<UpdateCache, String> {
        let Some(url) = self.source_url() else {
            return Err(NO_SOURCE.to_string());
        };
        let Ok(_guard) = self.check_lock.try_lock() else {
            return Err(IN_FLIGHT.to_string());
        };
        let outcome = self.ports.source.latest(&url).await;
        let mut memo = self.cache.lock().await;
        // The last good version of another source is not kept beside this one.
        let mut cache = memo.clone().filter(|c| c.is_for(&url)).unwrap_or_default();
        cache.record(&url, pdo_kernel::clock::now_iso(), &outcome);
        if self.ports.journal.is_available() {
            if let Err(e) = self.ports.journal.write_cache(&cache) {
                tracing::warn!("update check: cannot write the cache: {e}");
            }
        }
        *memo = Some(cache.clone());
        match outcome {
            Ok(_) => Ok(cache),
            Err(_) => Err(cache.error.clone().unwrap_or_default()),
        }
    }
}

/// The refusal of a check while another one is in flight.
pub(crate) const IN_FLIGHT: &str = "a version check is already in flight";

/// The refusal of a check when the release channel declares no source.
pub(crate) const NO_SOURCE: &str =
    "no release source is declared for this build: there is nothing to check";
