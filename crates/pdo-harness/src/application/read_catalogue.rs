//! Read a harness's offered catalogue, cached by the version of its binary
//! (#616, #798, ADR-0053 §3), and re-probe it on demand.

use std::collections::{HashMap, HashSet};
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
    /// How many times the ladder re-ran at this version and still read nothing.
    empty_reprobes: u32,
    cached: CachedCatalogue,
}

/// The daemon's catalogue cache, in front of the [`BinaryProbe`].
pub struct CatalogueReader {
    inner: Arc<Inner>,
}

struct Inner {
    probe: Arc<dyn BinaryProbe>,
    /// The version re-check window, read at every call so a test can change it
    /// after boot.
    ttl: Box<dyn Fn() -> Duration + Send + Sync>,
    /// Held across a first probe and a refresh: two of them never run at once.
    /// A background re-check does NOT hold it while its binary runs.
    cache: tokio::sync::Mutex<HashMap<String, Entry>>,
    /// The harnesses whose version re-check runs in the background right now:
    /// a burst of reads past the window starts one, not one per read.
    rechecking: std::sync::Mutex<HashSet<String>>,
    /// Told when a re-check probes a harness again because its binary answers
    /// another version than the one cached (one that failed to answer at boot
    /// included): the open pages read the new catalogue.
    reprobe_notice: std::sync::OnceLock<ReprobeNotice>,
}

/// Who hears a harness probed again by a read.
type ReprobeNotice = Box<dyn Fn(&str) + Send + Sync>;

impl CatalogueReader {
    pub fn new(
        probe: Arc<dyn BinaryProbe>,
        ttl: impl Fn() -> Duration + Send + Sync + 'static,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                probe,
                ttl: Box::new(ttl),
                cache: tokio::sync::Mutex::new(HashMap::new()),
                rechecking: std::sync::Mutex::new(HashSet::new()),
                reprobe_notice: std::sync::OnceLock::new(),
            }),
        }
    }

    /// The same reader, telling `notice` the harness a re-check probed again.
    /// Set once: a second notice is ignored.
    pub fn with_reprobe_notice(self, notice: impl Fn(&str) + Send + Sync + 'static) -> Self {
        let _ = self.inner.reprobe_notice.set(Box::new(notice));
        self
    }

    /// Whether `binary` resolves on the harness probe `PATH`.
    pub fn installed(&self, binary: &str) -> bool {
        self.inner.probe.installed(binary)
    }

    /// `harness`'s offered catalogue (models + efforts + the version they were
    /// read from). The freshness protocol, per harness:
    ///
    /// 1. A cache entry younger than the window is returned as-is — a burst of
    ///    `/settings` reads pays no subprocess at all.
    /// 2. Past the window, the cached entry is STILL returned at once, and the
    ///    version is re-checked in the background. If it matches, the entry is
    ///    restamped; if it changed — or the cached ladder enumerated nothing —
    ///    the catalogue ladder re-runs and the next read serves the new
    ///    `{version, catalogue}`. A harness CLI can take seconds to
    ///    answer `--version` (copilot: 5 to 10 s measured), so the re-check never
    ///    sits on the request path.
    /// 3. With no entry at all, the ladder runs inline. The daemon's boot task
    ///    warms every installed harness, so a page fetch normally never pays it.
    ///
    /// Every probe runs under `spawn_blocking` with the probe's own timeouts, so a
    /// wedged binary can never hang a reader — it degrades to the last good
    /// catalogue (or an empty one), i.e. the free-text fallback.
    pub async fn read(&self, harness: &str, binary: &str) -> CachedCatalogue {
        let mut guard = self.inner.cache.lock().await;
        if let Some(entry) = guard.get(harness) {
            if entry.checked.elapsed() >= (self.inner.ttl)() {
                self.recheck_in_background(harness, binary);
            }
            return entry.cached.clone();
        }
        let version = self.inner.version(binary).await;
        self.inner
            .probe_into(&mut guard, harness, binary, version)
            .await
    }

    /// Re-probe `harness`'s catalogue now, whatever its version and its window
    /// (the « refresh models » gesture): a binary may change what it offers
    /// without changing its version. A probe of this harness already running
    /// when the refresh is asked is awaited and its result returned: the refresh
    /// never starts a second one.
    pub async fn refresh(&self, harness: &str, binary: &str) -> CachedCatalogue {
        let asked = Instant::now();
        let mut guard = self.inner.cache.lock().await;
        if let Some(entry) = guard.get(harness) {
            if entry.probed >= asked {
                return entry.cached.clone();
            }
        }
        let version = self.inner.version(binary).await;
        self.inner
            .probe_into(&mut guard, harness, binary, version)
            .await
    }

    /// Start the version re-check of `harness`, unless one already runs.
    fn recheck_in_background(&self, harness: &str, binary: &str) {
        let started = self
            .inner
            .rechecking
            .lock()
            .map(|mut set| set.insert(harness.to_string()))
            .unwrap_or(false);
        if !started {
            return;
        }
        let inner = self.inner.clone();
        let (harness, binary) = (harness.to_string(), binary.to_string());
        tokio::spawn(async move {
            inner.recheck(&harness, &binary).await;
            if let Ok(mut set) = inner.rechecking.lock() {
                set.remove(&harness);
            }
        });
    }
}

/// How many background re-runs an empty catalogue gets at one version. Past
/// them it is taken as the harness's answer (opencode lists nothing: free-text
/// model, no effort axis) until the version changes or « refresh models » asks.
const EMPTY_REPROBES: u32 = 3;

/// Whether a catalogue ladder enumerated anything at all.
fn answered(catalogue: &crate::domain::catalogue::Catalogue) -> bool {
    !catalogue.models.is_empty() || catalogue.has_effort_axis()
}

impl Inner {
    async fn version(&self, binary: &str) -> Option<String> {
        let probe = self.probe.clone();
        let bin = binary.to_string();
        tokio::task::spawn_blocking(move || probe.version(&bin))
            .await
            .ok()
            .flatten()
    }

    async fn catalogue(&self, binary: &str) -> crate::domain::catalogue::Catalogue {
        let probe = self.probe.clone();
        let bin = binary.to_string();
        tokio::task::spawn_blocking(move || probe.catalogue(&bin))
            .await
            .unwrap_or_default()
    }

    /// The background half of [`CatalogueReader::read`]: re-read the version
    /// with no lock held, restamp the entry when it still matches, re-run the
    /// ladder (again with no lock held) when it changed.
    async fn recheck(&self, harness: &str, binary: &str) {
        let version = self.version(binary).await;
        {
            let mut guard = self.cache.lock().await;
            match guard.get_mut(harness) {
                // Same version, and the ladder answered something: still current.
                // A ladder that answered nothing (a source timed out on a loaded
                // host) is worth another run, in the background, at the next
                // window — not a free-text picker until the binary updates.
                Some(entry)
                    if entry.cached.version == version
                        && (answered(&entry.cached.catalogue)
                            || entry.empty_reprobes >= EMPTY_REPROBES) =>
                {
                    entry.checked = Instant::now();
                    return;
                }
                // A refresh may have re-probed meanwhile; this re-check still
                // owes the version it read.
                _ => {}
            }
        }
        let catalogue = self.catalogue(binary).await;
        let mut guard = self.cache.lock().await;
        let (empty_reprobes, new_version) = match guard.get(harness) {
            Some(entry) if entry.cached.version == version => (
                if answered(&catalogue) {
                    0
                } else {
                    entry.empty_reprobes + 1
                },
                false,
            ),
            Some(_) => (0, true),
            None => (0, false),
        };
        Self::store(&mut guard, harness, version, catalogue, empty_reprobes);
        drop(guard);
        if new_version {
            if let Some(notice) = self.reprobe_notice.get() {
                notice(harness);
            }
        }
    }

    /// Run the catalogue ladder of `binary` and cache it at `version`.
    async fn probe_into(
        &self,
        cache: &mut HashMap<String, Entry>,
        harness: &str,
        binary: &str,
        version: Option<String>,
    ) -> CachedCatalogue {
        let catalogue = self.catalogue(binary).await;
        Self::store(cache, harness, version, catalogue, 0)
    }

    fn store(
        cache: &mut HashMap<String, Entry>,
        harness: &str,
        version: Option<String>,
        catalogue: crate::domain::catalogue::Catalogue,
        empty_reprobes: u32,
    ) -> CachedCatalogue {
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
                empty_reprobes,
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
        /// How long `--version` takes to answer: a slow harness CLI.
        pub version_delay: Mutex<Duration>,
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

        /// Wait until `n` probes have run (a background re-check included).
        pub(crate) async fn await_runs(&self, n: usize) {
            for _ in 0..200 {
                if self.runs().len() >= n {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            panic!("expected {n} probe runs, saw {:?}", self.runs());
        }
    }

    impl BinaryProbe for FakeBinaries {
        fn installed(&self, binary: &str) -> bool {
            self.installed.contains(&binary)
        }

        fn version(&self, binary: &str) -> Option<String> {
            std::thread::sleep(*self.version_delay.lock().unwrap());
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
    async fn a_read_that_finds_a_new_version_says_so_and_a_first_read_does_not() {
        // A boot probe that failed (a slow `--version`) leaves no version; the next
        // read past the window finds one and probes again: the open pages, which
        // still show « probe failed », must hear of it.
        let fake = binaries();
        let heard = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let notes = heard.clone();
        let reader = CatalogueReader::new(fake.clone(), || Duration::ZERO)
            .with_reprobe_notice(move |harness| notes.lock().unwrap().push(harness.to_string()));
        reader.read("claude", "claude").await;
        reader.read("claude", "claude").await;
        // The read past the window re-checks in the background: its version run.
        fake.await_runs(3).await;
        assert!(heard.lock().unwrap().is_empty(), "nothing changed yet");
        fake.set_version("claude", "2.2.0");
        reader.read("claude", "claude").await;
        // Its re-check: the version, then the ladder at the new one.
        fake.await_runs(5).await;
        for _ in 0..200 {
            if !heard.lock().unwrap().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(*heard.lock().unwrap(), vec!["claude".to_string()]);
    }

    #[tokio::test]
    async fn past_the_window_an_unchanged_version_keeps_the_catalogue() {
        let fake = binaries();
        let reader = CatalogueReader::new(fake.clone(), || Duration::ZERO);
        reader.read("claude", "claude").await;
        let cached = reader.read("claude", "claude").await;
        assert_eq!(cached.catalogue.models, vec!["opus", "sonnet"]);
        fake.await_runs(3).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(
            fake.runs(),
            vec!["claude --version", "claude --help", "claude --version"],
            "the version matched: no second --help"
        );
    }

    #[tokio::test]
    async fn a_changed_version_reprobes_the_catalogue_for_the_next_read() {
        let fake = binaries();
        let reader = CatalogueReader::new(fake.clone(), || Duration::ZERO);
        reader.read("claude", "claude").await;
        fake.set_version("claude", "2.2.0");
        let stale = reader.read("claude", "claude").await;
        assert_eq!(
            stale.version.as_deref(),
            Some("2.1.0"),
            "the read past the window answers the cached entry at once"
        );
        fake.await_runs(4).await;
        assert_eq!(
            fake.runs().last().map(String::as_str),
            Some("claude --help")
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
        let fresh = {
            // The window is zero, so this read re-checks again in the
            // background; what it answers is the re-probed entry.
            reader.read("claude", "claude").await
        };
        assert_eq!(fresh.version.as_deref(), Some("2.2.0"));
    }

    /// A harness CLI can take seconds to answer `--version` (copilot: 5 to
    /// 10 s measured). Past the window, a page fetch never waits for it: the
    /// cached entry is served at once and the re-check runs in the background,
    /// once for a burst of reads.
    #[tokio::test]
    async fn a_slow_version_recheck_never_holds_a_read() {
        let fake = binaries();
        let reader = CatalogueReader::new(fake.clone(), || Duration::ZERO);
        reader.read("claude", "claude").await;
        *fake.version_delay.lock().unwrap() = Duration::from_millis(1500);

        let started = Instant::now();
        let cached = reader.read("claude", "claude").await;
        let again = reader.read("claude", "claude").await;
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "the reads waited for the re-check: {:?}",
            started.elapsed()
        );
        assert_eq!(cached.catalogue.models, vec!["opus", "sonnet"]);
        assert_eq!(again, cached);

        fake.await_runs(3).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(
            fake.runs().len(),
            3,
            "one background re-check for the burst: {:?}",
            fake.runs()
        );
    }

    /// A ladder that answered nothing (its `--help` timed out on a loaded host)
    /// is re-run at the next window even though the version did not move: a
    /// slow boot must not leave a free-text picker until the binary updates.
    #[tokio::test]
    async fn an_empty_catalogue_is_reprobed_at_the_same_version() {
        let fake = Arc::new(FakeBinaries {
            installed: vec!["pi"],
            ..FakeBinaries::default()
        });
        fake.set_version("pi", "1.5.0");
        let reader = CatalogueReader::new(fake.clone(), || Duration::ZERO);
        let first = reader.read("pi", "pi").await;
        assert!(first.catalogue.efforts.is_empty());
        reader.read("pi", "pi").await;
        fake.await_runs(4).await;
        assert_eq!(
            fake.runs(),
            vec!["pi --version", "pi --help", "pi --version", "pi --help"]
        );
    }

    #[tokio::test]
    async fn an_empty_catalogue_stops_being_reprobed_after_a_few_tries() {
        let fake = Arc::new(FakeBinaries {
            installed: vec!["opencode"],
            ..FakeBinaries::default()
        });
        fake.set_version("opencode", "1.0.0");
        let reader = CatalogueReader::new(fake.clone(), || Duration::ZERO);
        reader.read("opencode", "opencode").await;
        // The first probe, then EMPTY_REPROBES re-runs of the ladder at the same version.
        let ladders = 1 + EMPTY_REPROBES as usize;
        for _ in 0..20 {
            reader.read("opencode", "opencode").await;
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let helps = fake.runs().iter().filter(|r| r.ends_with("--help")).count();
        assert_eq!(helps, ladders, "{:?}", fake.runs());
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
        reader.read("claude", "claude").await;
        fake.await_runs(3).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
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
