//! In-memory adapters of every port (ADR-0080, stage 2): the use cases are
//! tested against them, fast and without SQLite, files or the network.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use pdo_extension_api::{Authorization, RequestParts};
use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::{Actor, Operation};
use pdo_platform::domain::audit::NewAuditEntry;
use pdo_run::application::ports::{EventWindow, NodeStart, RunStart};
use pdo_run::domain::event::Event;
use pdo_run::domain::prices::{Price, PriceTable};
use pdo_run::domain::run_cost::CostContribution;
use pdo_sandbox::adapters::staging::stores::HarnessStores;
use pdo_stats::application::ports::*;
use pdo_stats::domain::absorption::{AbsorptionRow, Named, Scope};
use pdo_stats::domain::overview::TriggersCreatedRuns;
use pdo_stats::{Stats, StatsOptions, StatsPorts};
use pdo_workspace::domain::project::Project;

/// An extension host that refuses everything, or allows everything, and
/// remembers what it was asked.
pub struct Host {
    pub refuse: bool,
    pub asked: Mutex<Vec<Operation>>,
}

impl Host {
    pub fn allowing() -> Arc<Self> {
        Arc::new(Self {
            refuse: false,
            asked: Mutex::new(Vec::new()),
        })
    }

    pub fn refusing() -> Arc<Self> {
        Arc::new(Self {
            refuse: true,
            asked: Mutex::new(Vec::new()),
        })
    }
}

impl ExtensionHost for Host {
    fn identify(&self, _request: &RequestParts) -> Actor {
        Actor::LocalOperator
    }

    fn authorize(&self, _actor: &Actor, operation: &Operation) -> Authorization {
        self.asked.lock().unwrap().push(operation.clone());
        if self.refuse {
            Authorization::deny("admins only")
        } else {
            Authorization::Allow
        }
    }

    fn publish(&self, _event: &serde_json::Value) {}
}

/// The event log, as fixed answers: the counts, the cohort, the Node starts,
/// each Run's events, and each Pipeline's history.
#[derive(Default)]
pub struct Events {
    pub counts: BTreeMap<String, Vec<(String, i64)>>,
    pub run_starts: Vec<RunStart>,
    /// The child Runs of the whole log (« Roll up child runs »).
    pub child_run_starts: Vec<RunStart>,
    pub node_starts: Vec<NodeStart>,
    pub events: BTreeMap<String, Vec<Event>>,
    pub ran: BTreeMap<String, BTreeSet<String>>,
    /// How many reads of one Run's events.
    pub reads: AtomicUsize,
}

impl RunEvents for Events {
    fn count_by_period<'a>(
        &'a self,
        kind: &'a str,
        _bucket_format: &'a str,
        _window: EventWindow<'a>,
    ) -> BoxFuture<'a, anyhow::Result<Vec<(String, i64)>>> {
        let rows = self.counts.get(kind).cloned().unwrap_or_default();
        Box::pin(async move { Ok(rows) })
    }

    fn run_starts<'a>(
        &'a self,
        _bucket_format: Option<&'a str>,
        _window: EventWindow<'a>,
    ) -> BoxFuture<'a, anyhow::Result<Vec<RunStart>>> {
        Box::pin(async move { Ok(self.run_starts.clone()) })
    }

    fn node_starts<'a>(
        &'a self,
        _bucket_format: &'a str,
        _window: EventWindow<'a>,
    ) -> BoxFuture<'a, anyhow::Result<Vec<NodeStart>>> {
        Box::pin(async move { Ok(self.node_starts.clone()) })
    }

    fn nodes_started_under<'a>(
        &'a self,
        pipeline: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<BTreeSet<String>>> {
        Box::pin(async move { Ok(self.ran.get(pipeline).cloned().unwrap_or_default()) })
    }

    fn latest_run_start<'a>(
        &'a self,
        _pipeline: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<Option<serde_json::Value>>> {
        Box::pin(async { Ok(None) })
    }

    fn events<'a>(&'a self, run_id: &'a str) -> BoxFuture<'a, anyhow::Result<Vec<Event>>> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move { Ok(self.events.get(run_id).cloned().unwrap_or_default()) })
    }

    fn child_run_starts(&self) -> BoxFuture<'_, anyhow::Result<Vec<RunStart>>> {
        Box::pin(async move { Ok(self.child_run_starts.clone()) })
    }
}

pub struct NoFires;

impl TriggerFires for NoFires {
    fn by_pipeline<'a>(
        &'a self,
        _from: &'a str,
        _to: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<Vec<(String, i64)>>> {
        Box::pin(async { Ok(vec![("old".to_string(), 2), ("new".to_string(), 1)]) })
    }

    fn created_runs<'a>(
        &'a self,
        _from: &'a str,
        _to: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<TriggersCreatedRuns>> {
        Box::pin(async {
            Ok(TriggersCreatedRuns {
                fired: 3,
                distinct_triggers: 1,
                enabled_triggers: 1,
            })
        })
    }
}

pub struct NoProjects;

impl Projects for NoProjects {
    fn list(&self) -> BoxFuture<'_, Result<Vec<Project>, String>> {
        Box::pin(async { Ok(Vec::new()) })
    }
}

/// The absorptions' store, as the writes it received and the rows it holds.
#[derive(Default)]
pub struct Store {
    pub rows: Mutex<Vec<AbsorptionRow>>,
    pub writes: Mutex<Vec<String>>,
}

impl Store {
    pub fn holding(rows: Vec<AbsorptionRow>) -> Arc<Self> {
        Arc::new(Self {
            rows: Mutex::new(rows),
            writes: Mutex::new(Vec::new()),
        })
    }

    pub fn writes(&self) -> Vec<String> {
        self.writes.lock().unwrap().clone()
    }
}

pub fn row(dimension: &str, scope: &str, member: &str, absorbent: &str) -> AbsorptionRow {
    AbsorptionRow {
        dimension: dimension.into(),
        scope: scope.into(),
        scope_name: String::new(),
        member: member.into(),
        member_name: member.into(),
        absorbent: absorbent.into(),
        absorbent_name: absorbent.into(),
        origin: "manual".into(),
        created_at: String::new(),
    }
}

impl AbsorptionStore for Store {
    fn list(&self) -> BoxFuture<'_, anyhow::Result<Vec<AbsorptionRow>>> {
        let rows = self.rows.lock().unwrap().clone();
        Box::pin(async move { Ok(rows) })
    }

    fn combine<'a>(
        &'a self,
        scope: Scope<'a>,
        absorbent: &'a Named,
        members: &'a [Named],
        origin: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<()>> {
        let keys: Vec<&str> = members.iter().map(|m| m.key.as_str()).collect();
        self.writes.lock().unwrap().push(format!(
            "combine {} [{}] {} <- {} ({origin})",
            scope.dimension,
            scope.key,
            absorbent.key,
            keys.join(",")
        ));
        let mut rows = self.rows.lock().unwrap();
        for member in members {
            rows.push(AbsorptionRow {
                origin: origin.to_string(),
                ..row(scope.dimension, scope.key, &member.key, &absorbent.key)
            });
        }
        Box::pin(async { Ok(()) })
    }

    fn combine_frozen<'a>(
        &'a self,
        dimension: &'a str,
        covered: &'a [Named],
        absorbent: &'a Named,
        _members: &'a [Named],
        _origin: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<()>> {
        let scopes: Vec<&str> = covered.iter().map(|c| c.key.as_str()).collect();
        self.writes.lock().unwrap().push(format!(
            "frozen {dimension} {} over [{}]",
            absorbent.key,
            scopes.join(" ")
        ));
        Box::pin(async { Ok(()) })
    }

    fn remove<'a>(
        &'a self,
        dimension: &'a str,
        scope: &'a str,
        member: &'a str,
        released_scope: &'a str,
        released: &'a [String],
    ) -> BoxFuture<'a, anyhow::Result<()>> {
        self.writes.lock().unwrap().push(format!(
            "remove {dimension} [{scope}] {member}, released [{released_scope}] {}",
            released.join(",")
        ));
        self.rows
            .lock()
            .unwrap()
            .retain(|r| !(r.dimension == dimension && r.scope == scope && r.member == member));
        Box::pin(async { Ok(()) })
    }
}

/// The Runs' files: one fixed footprint per Run, no transcript, no cost.
#[derive(Default)]
pub struct Files {
    pub footprint: Mutex<u64>,
}

impl RunFiles for Files {
    fn roots(&self, run_id: &str, _payload: &serde_json::Value) -> RunFilesRoots {
        let root = PathBuf::from("/nowhere").join(run_id);
        RunFilesRoots {
            claude_root: root.clone(),
            repo_root: root.clone(),
            stores: HarnessStores::for_run(false, run_id, &root, &root),
        }
    }

    fn cost_contributions(
        &self,
        _events: &[Event],
        _roots: &RunFilesRoots,
        _run_id: &str,
        _prices: &PriceTable,
    ) -> Vec<CostContribution> {
        Vec::new()
    }

    fn footprint(&self, _events: &[Event], _roots: &RunFilesRoots, _run_id: &str) -> u64 {
        *self.footprint.lock().unwrap()
    }

    fn prices(&self) -> PriceTable {
        PriceTable::builtin()
    }
}

/// No transcript anywhere; the sources are listable unless `unreadable`.
#[derive(Default)]
pub struct Transcripts {
    pub unreadable: bool,
}

impl SessionTranscripts for Transcripts {
    fn source_listable(&self, _root: &Path) -> Result<(), String> {
        if self.unreadable {
            Err("permission denied".to_string())
        } else {
            Ok(())
        }
    }

    fn main_session(&self, _: &str, _: &Path, _: &Path, _: Option<&str>) -> Option<String> {
        None
    }

    fn subagents(&self, _: &str, _: &Path, _: &Path, _: &str) -> Vec<(String, String)> {
        Vec::new()
    }

    fn claude_sessions_in(&self, _: &Path, _: &Path) -> Option<(PathBuf, Vec<String>)> {
        None
    }

    fn read(&self, _path: &Path) -> Option<String> {
        None
    }
}

/// The price files of a home held in memory.
pub struct PriceHome {
    pub home: Option<PathBuf>,
    pub table: Mutex<PriceTable>,
    pub fetched_exists: bool,
    pub written: Mutex<Vec<BTreeMap<String, Price>>>,
}

impl PriceHome {
    pub fn at(home: Option<&str>, fetched_exists: bool) -> Arc<Self> {
        Arc::new(Self {
            home: home.map(PathBuf::from),
            table: Mutex::new(PriceTable::builtin()),
            fetched_exists,
            written: Mutex::new(Vec::new()),
        })
    }
}

impl PriceFiles for PriceHome {
    fn home(&self) -> Result<PathBuf, String> {
        self.home.clone().ok_or_else(|| "no home".to_string())
    }

    fn load(&self, _home: &Path) -> PriceTable {
        self.table.lock().unwrap().clone()
    }

    fn paths(&self, home: &Path) -> (PathBuf, PathBuf) {
        // `fetched.json` "exists" only when asked to: `/` always does.
        let fetched = if self.fetched_exists {
            PathBuf::from("/")
        } else {
            home.join("never-written")
        };
        (home.join("models.yaml"), fetched)
    }

    fn write_fetched(
        &self,
        _path: &Path,
        _source: &str,
        _fetched_at: &str,
        rows: &BTreeMap<String, Price>,
    ) -> std::io::Result<()> {
        self.written.lock().unwrap().push(rows.clone());
        Ok(())
    }
}

/// The price source, as a fixed answer; counts its hits.
pub struct Source {
    pub body: Result<String, String>,
    pub hits: AtomicUsize,
}

impl Source {
    pub fn answering(body: Result<&str, &str>) -> Arc<Self> {
        Arc::new(Self {
            body: body.map(str::to_string).map_err(str::to_string),
            hits: AtomicUsize::new(0),
        })
    }
}

impl PriceSource for Source {
    fn fetch<'a>(&'a self, _url: &'a str) -> BoxFuture<'a, Result<String, String>> {
        self.hits.fetch_add(1, Ordering::SeqCst);
        let body = self.body.clone();
        Box::pin(async move { body })
    }
}

#[derive(Default)]
pub struct Audit(pub Mutex<Vec<NewAuditEntry>>);

impl AuditJournal for Audit {
    fn record(&self, entry: NewAuditEntry) -> BoxFuture<'_, ()> {
        self.0.lock().unwrap().push(entry);
        Box::pin(async {})
    }
}

#[derive(Default)]
pub struct Announced(pub Mutex<Vec<usize>>);

impl Announcements for Announced {
    fn cost_prices_synced(&self, rows: usize) {
        self.0.lock().unwrap().push(rows);
    }
}

/// Every fake of one context, and the context over them.
pub struct World {
    pub stats: Arc<Stats>,
    pub host: Arc<Host>,
    pub store: Arc<Store>,
    pub events: Arc<Events>,
    pub files: Arc<Files>,
    pub prices: Arc<PriceHome>,
    pub source: Arc<Source>,
    pub audit: Arc<Audit>,
    pub announced: Arc<Announced>,
}

pub struct Build {
    pub host: Arc<Host>,
    pub store: Arc<Store>,
    pub events: Events,
    pub transcripts: Transcripts,
    pub prices: Arc<PriceHome>,
    pub source: Arc<Source>,
    pub refresh_at_boot: bool,
}

impl Default for Build {
    fn default() -> Self {
        Self {
            host: Host::allowing(),
            store: Store::holding(Vec::new()),
            events: Events::default(),
            transcripts: Transcripts::default(),
            prices: PriceHome::at(Some("/home"), false),
            source: Source::answering(Err("offline")),
            refresh_at_boot: false,
        }
    }
}

impl Build {
    pub fn world(self) -> World {
        let events = Arc::new(self.events);
        let files = Arc::new(Files::default());
        let audit = Arc::new(Audit::default());
        let announced = Arc::new(Announced::default());
        let stats = Stats::new(
            StatsPorts {
                events: events.clone(),
                triggers: Arc::new(NoFires),
                projects: Arc::new(NoProjects),
                absorptions: self.store.clone(),
                run_files: files.clone(),
                transcripts: Arc::new(self.transcripts),
                price_files: self.prices.clone(),
                price_source: self.source.clone(),
                audit: audit.clone(),
                announcements: announced.clone(),
                extensions: self.host.clone(),
            },
            StatsOptions {
                repo_root: PathBuf::from("/daemon"),
                price_source_url: Some("https://prices.test/api.json".into()),
                price_refresh_at_boot: self.refresh_at_boot,
            },
        );
        World {
            stats,
            host: self.host,
            store: self.store,
            events,
            files,
            prices: self.prices,
            source: self.source,
            audit,
            announced,
        }
    }
}
