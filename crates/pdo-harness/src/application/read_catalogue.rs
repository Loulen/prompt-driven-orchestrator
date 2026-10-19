//! Read a harness's offered catalogue, cached by the version of its binary
//! (#616, #798, ADR-0053 §3), and re-probe it on demand.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use pdo_kernel::clock::now_iso;

use crate::application::ports::BinaryProbe;
use crate::domain::catalogue::CachedCatalogue;

/// One harness's cache entry.
struct Entry {
    /// When the version was last checked: the start of the freshness window.
    checked: Instant,
    /// When the catalogue ladder last ran for it.
    probed: Instant,
    cached: CachedCatalogue,
}

/// The daemon's catalogue cache, in front of the [`BinaryProbe`].
pub struct CatalogueReader {
    probe: Arc<dyn BinaryProbe>,
    /// The version re-check window, read at every call so a test can change it
    /// after boot.
    ttl: Box<dyn Fn() -> Duration + Send + Sync>,
    /// Held across a probe: two probes of the catalogues never run at once.
    cache: tokio::sync::Mutex<HashMap<String, Entry>>,
}

impl CatalogueReader {
    pub fn new(
        probe: Arc<dyn BinaryProbe>,
        ttl: impl Fn() -> Duration + Send + Sync + 'static,
    ) -> Self {
        Self {
            probe,
            ttl: Box::new(ttl),
            cache: tokio::sync::Mutex::new(HashMap::new()),
        }
    }

    /// Whether `binary` resolves on the harness probe `PATH`.
    pub fn installed(&self, binary: &str) -> bool {
        self.probe.installed(binary)
    }

    /// `harness`'s offered catalogue (models + efforts + the version they were
    /// read from), from the cache when fresh, else by (re)probing `binary`. The
    /// freshness protocol, per harness:
    ///
    /// 1. A cache entry younger than the window is returned as-is — a burst of
    ///    `/settings` reads pays no subprocess at all.
    /// 2. Past the window, `--version` is re-read (off the runtime thread). If it
    ///    matches the cached version, the catalogue is still current: restamp and
    ///    return it, no `--help` run.
    /// 3. On a first miss or a changed version, the catalogue ladder is re-run
    ///    (help, and every source the binary declares — the RPC session included,
    ///    #798) and the new `{version, catalogue}` cached.
    ///
    /// Every probe runs under `spawn_blocking` with the probe's own timeouts, so a
    /// wedged binary can never hang the settings response — it degrades to the
    /// last good catalogue (or an empty one), i.e. the free-text fallback.
    pub async fn read(&self, harness: &str, binary: &str) -> CachedCatalogue {
        let mut guard = self.cache.lock().await;
        if let Some(entry) = guard.get(harness) {
            if entry.checked.elapsed() < (self.ttl)() {
                return entry.cached.clone();
            }
        }

        let version = self.version(binary).await;
        if let Some(entry) = guard.get_mut(harness) {
            if entry.cached.version == version {
                entry.checked = Instant::now();
                return entry.cached.clone();
            }
        }
        self.probe_into(&mut guard, harness, binary, version).await
    }

    /// Re-probe `harness`'s catalogue now, whatever its version and its window
    /// (the « refresh models » gesture): a binary may change what it offers
    /// without changing its version. A probe of this harness already running
    /// when the refresh is asked is awaited and its result returned: the refresh
    /// never starts a second one.
    pub async fn refresh(&self, harness: &str, binary: &str) -> CachedCatalogue {
        let asked = Instant::now();
        let mut guard = self.cache.lock().await;
        if let Some(entry) = guard.get(harness) {
            if entry.probed >= asked {
                return entry.cached.clone();
            }
        }
        let version = self.version(binary).await;
        self.probe_into(&mut guard, harness, binary, version).await
    }

    async fn version(&self, binary: &str) -> Option<String> {
        let probe = self.probe.clone();
        let bin = binary.to_string();
        tokio::task::spawn_blocking(move || probe.version(&bin))
            .await
            .ok()
            .flatten()
    }

    /// Run the catalogue ladder of `binary` and cache it at `version`.
    async fn probe_into(
        &self,
        cache: &mut HashMap<String, Entry>,
        harness: &str,
        binary: &str,
        version: Option<String>,
    ) -> CachedCatalogue {
        let probe = self.probe.clone();
        let bin = binary.to_string();
        let catalogue = tokio::task::spawn_blocking(move || probe.catalogue(&bin))
            .await
            .unwrap_or_default();
        let cached = CachedCatalogue {
            version,
            catalogue,
            probed_at: Some(now_iso()),
        };
        let now = Instant::now();
        cache.insert(
            harness.to_string(),
            Entry {
                checked: now,
                probed: now,
                cached: cached.clone(),
            },
        );
        cached
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::domain::catalogue::Catalogue;

    /// Installed binaries held in memory: a version per binary, a catalogue per
    /// binary, and a record of every probe run.
    #[derive(Default)]
    pub(crate) struct FakeBinaries {
        pub installed: Vec<&'static str>,
        pub versions: Mutex<HashMap<String, String>>,
        pub catalogues: HashMap<&'static str, Catalogue>,
        pub runs: Mutex<Vec<String>>,
    }

    impl FakeBinaries {
        pub(crate) fn set_version(&self, binary: &str, version: &str) {
            self.versions
                .lock()
                .unwrap()
                .insert(binary.to_string(), version.to_string());
        }

        pub(crate) fn runs(&self) -> Vec<String> {
            self.runs.lock().unwrap().clone()
        }
    }

    impl BinaryProbe for FakeBinaries {
        fn installed(&self, binary: &str) -> bool {
            self.installed.contains(&binary)
        }

        fn version(&self, binary: &str) -> Option<String> {
            self.runs
                .lock()
                .unwrap()
                .push(format!("{binary} --version"));
            self.versions.lock().unwrap().get(binary).cloned()
        }

        fn catalogue(&self, binary: &str) -> Catalogue {
            self.runs.lock().unwrap().push(format!("{binary} --help"));
            self.catalogues.get(binary).cloned().unwrap_or_default()
        }
    }

    pub(crate) fn catalogue(models: &[&str], efforts: &[&str]) -> Catalogue {
        Catalogue {
            models: models.iter().map(|m| m.to_string()).collect(),
            efforts: efforts.iter().map(|e| e.to_string()).collect(),
            ..Catalogue::default()
        }
    }

    fn binaries() -> Arc<FakeBinaries> {
        let fake = FakeBinaries {
            installed: vec!["claude"],
            catalogues: HashMap::from([(
                "claude",
                catalogue(&["opus", "sonnet"], &["low", "high"]),
            )]),
            ..FakeBinaries::default()
        };
        fake.set_version("claude", "2.1.0");
        Arc::new(fake)
    }

    #[tokio::test]
    async fn a_first_read_probes_the_version_then_the_catalogue() {
        let fake = binaries();
        let reader = CatalogueReader::new(fake.clone(), || Duration::from_secs(3600));
        let cached = reader.read("claude", "claude").await;
        assert_eq!(cached.version.as_deref(), Some("2.1.0"));
        assert_eq!(cached.catalogue.models, vec!["opus", "sonnet"]);
        assert_eq!(fake.runs(), vec!["claude --version", "claude --help"]);
    }

    #[tokio::test]
    async fn a_fresh_entry_is_served_without_running_anything() {
        let fake = binaries();
        let reader = CatalogueReader::new(fake.clone(), || Duration::from_secs(3600));
        reader.read("claude", "claude").await;
        reader.read("claude", "claude").await;
        assert_eq!(fake.runs().len(), 2, "the second read ran nothing");
    }

    #[tokio::test]
    async fn past_the_window_an_unchanged_version_keeps_the_catalogue() {
        let fake = binaries();
        let reader = CatalogueReader::new(fake.clone(), || Duration::ZERO);
        reader.read("claude", "claude").await;
        let cached = reader.read("claude", "claude").await;
        assert_eq!(cached.catalogue.models, vec!["opus", "sonnet"]);
        assert_eq!(
            fake.runs(),
            vec!["claude --version", "claude --help", "claude --version"],
            "the version matched: no second --help"
        );
    }

    #[tokio::test]
    async fn a_changed_version_reprobes_the_catalogue() {
        let fake = binaries();
        let reader = CatalogueReader::new(fake.clone(), || Duration::ZERO);
        reader.read("claude", "claude").await;
        fake.set_version("claude", "2.2.0");
        let cached = reader.read("claude", "claude").await;
        assert_eq!(cached.version.as_deref(), Some("2.2.0"));
        assert_eq!(
            fake.runs().last().map(String::as_str),
            Some("claude --help")
        );
        assert_eq!(fake.runs().len(), 4);
    }

    #[tokio::test]
    async fn a_refresh_reprobes_at_the_same_version() {
        let fake = binaries();
        let reader = CatalogueReader::new(fake.clone(), || Duration::from_secs(3600));
        let first = reader.read("claude", "claude").await;
        fake.runs.lock().unwrap().clear();
        let refreshed = reader.refresh("claude", "claude").await;
        assert_eq!(fake.runs(), vec!["claude --version", "claude --help"]);
        assert_eq!(refreshed.version.as_deref(), Some("2.1.0"));
        assert!(refreshed.probed_at.is_some());
        assert!(refreshed.probed_at >= first.probed_at);
        // The refreshed catalogue is the one the next reads serve.
        fake.runs.lock().unwrap().clear();
        assert_eq!(reader.read("claude", "claude").await, refreshed);
        assert!(fake.runs().is_empty());
    }

    #[tokio::test]
    async fn a_version_recheck_keeps_the_time_of_the_probe() {
        let fake = binaries();
        let reader = CatalogueReader::new(fake.clone(), || Duration::ZERO);
        let first = reader.read("claude", "claude").await;
        let again = reader.read("claude", "claude").await;
        assert_eq!(again.probed_at, first.probed_at);
    }

    #[tokio::test]
    async fn a_refresh_during_a_probe_awaits_it_instead_of_starting_another() {
        let fake = binaries();
        let reader = CatalogueReader::new(fake.clone(), || Duration::from_secs(3600));
        let (a, b) = tokio::join!(
            reader.refresh("claude", "claude"),
            reader.refresh("claude", "claude")
        );
        assert_eq!(a, b);
        assert_eq!(
            fake.runs(),
            vec!["claude --version", "claude --help"],
            "one probe for two refreshes asked while it ran"
        );
        // A refresh asked once that probe is over probes again.
        reader.refresh("claude", "claude").await;
        assert_eq!(fake.runs().len(), 4);
    }
}
