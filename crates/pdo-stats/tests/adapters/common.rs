//! The stats context over its real adapters: the SQLite event log (through the
//! run context's ports) and absorptions of an in-memory database, the Runs'
//! files and transcripts on disk, the price files of a scratch home — driven
//! through its router, as the local operator. Every root lives in a temporary
//! directory: nothing lands in the source tree.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::Router;
use pdo_kernel::extension_host::NoExtensions;
use pdo_kernel::Actor;
use pdo_platform::domain::audit::NewAuditEntry;
use pdo_run::adapters::sqlite_event_store::SqliteEventStore;
use pdo_stats::adapters::models_dev::HttpPriceSource;
use pdo_stats::adapters::price_files::HomePriceFiles;
use pdo_stats::adapters::run_events::RunContextEvents;
use pdo_stats::adapters::run_files::DiskRunFiles;
use pdo_stats::adapters::sqlite_absorptions::SqliteAbsorptions;
use pdo_stats::adapters::transcripts::HarnessTranscripts;
use pdo_stats::application::ports::{
    Announcements, AuditJournal, BoxFuture, Projects, TriggerFires,
};
use pdo_stats::domain::overview::TriggersCreatedRuns;
use pdo_stats::{Stats, StatsOptions, StatsPorts};
use pdo_workspace::domain::project::Project;

/// The Triggers' two tables, as far as the fires go: the trigger context owns
/// the real ones (the composition root tests its adapter against them).
const TRIGGER_TABLES: [&str; 2] = [
    "CREATE TABLE triggers (id TEXT PRIMARY KEY, name TEXT NOT NULL, \
     pipeline_id TEXT NOT NULL, cron TEXT NOT NULL, enabled INTEGER NOT NULL DEFAULT 1, \
     created_at TEXT NOT NULL)",
    "CREATE TABLE trigger_fires (id INTEGER PRIMARY KEY AUTOINCREMENT, \
     trigger_id TEXT NOT NULL, ts TEXT NOT NULL, outcome TEXT NOT NULL, run_id TEXT)",
];

/// The fires of the test database's Triggers.
pub struct DbFires(pub sqlx::SqlitePool);

impl TriggerFires for DbFires {
    fn by_pipeline<'a>(
        &'a self,
        from: &'a str,
        to: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<Vec<(String, i64)>>> {
        Box::pin(async move {
            Ok(sqlx::query_as::<_, (String, i64)>(
                "SELECT COALESCE(t.pipeline_id, '(deleted trigger)') AS pk, COUNT(*) \
                 FROM trigger_fires f LEFT JOIN triggers t ON f.trigger_id = t.id \
                 WHERE f.ts >= ? AND f.ts < ? GROUP BY pk",
            )
            .bind(from)
            .bind(to)
            .fetch_all(&self.0)
            .await?)
        })
    }

    fn created_runs<'a>(
        &'a self,
        from: &'a str,
        to: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<TriggersCreatedRuns>> {
        Box::pin(async move {
            let (fired, distinct_triggers, enabled_triggers) =
                sqlx::query_as::<_, (i64, i64, i64)>(
                    "SELECT \
                       (SELECT COUNT(*) FROM trigger_fires \
                        WHERE outcome = 'fired' AND ts >= ?1 AND ts < ?2), \
                       (SELECT COUNT(DISTINCT f.trigger_id) FROM trigger_fires f \
                        JOIN triggers t ON t.id = f.trigger_id \
                        WHERE t.enabled = 1 AND f.outcome = 'fired' AND f.ts >= ?1 AND f.ts < ?2), \
                       (SELECT COUNT(*) FROM triggers WHERE enabled = 1)",
                )
                .bind(from)
                .bind(to)
                .fetch_one(&self.0)
                .await?;
            Ok(TriggersCreatedRuns {
                fired,
                distinct_triggers,
                enabled_triggers,
            })
        })
    }
}

/// The Projets of the test database, read by the workspace's own adapter.
pub struct DbProjects(pub sqlx::SqlitePool);

impl Projects for DbProjects {
    fn list(&self) -> BoxFuture<'_, Result<Vec<Project>, String>> {
        Box::pin(async move {
            pdo_workspace::adapters::sqlite::list(&self.0)
                .await
                .map_err(|e| e.to_string())
        })
    }
}

/// An audit journal nobody reads.
pub struct Unaudited;

impl AuditJournal for Unaudited {
    fn record(&self, _entry: NewAuditEntry) -> BoxFuture<'_, ()> {
        Box::pin(async {})
    }
}

/// Nobody listens.
pub struct Silent;

impl Announcements for Silent {
    fn cost_prices_synced(&self, _rows: usize) {}
}

/// A stats context over real adapters, with its database and its roots.
#[derive(Clone)]
pub struct Harness {
    pub db: sqlx::SqlitePool,
    pub stats: Arc<Stats>,
    /// The daemon's repo root (inside the scratch directory).
    pub repo_root: PathBuf,
    scratch: Arc<tempfile::TempDir>,
}

impl Harness {
    /// A fresh database, the repo root in a scratch directory, `$HOME` as is.
    pub async fn new() -> Self {
        let scratch = Arc::new(tempfile::tempdir().unwrap());
        let repo_root = scratch.path().join("repo");
        std::fs::create_dir_all(&repo_root).unwrap();
        Self::build(scratch, repo_root, None).await
    }

    /// A fresh database, with `dir` as the repo root.
    pub async fn with_dir(dir: &Path) -> Self {
        let scratch = Arc::new(tempfile::tempdir().unwrap());
        Self::build(scratch, dir.to_path_buf(), None).await
    }

    async fn build(
        scratch: Arc<tempfile::TempDir>,
        repo_root: PathBuf,
        home: Option<PathBuf>,
    ) -> Self {
        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        pdo_run::adapters::sqlite_event_store::init(&db)
            .await
            .unwrap();
        pdo_stats::adapters::sqlite_absorptions::init(&db)
            .await
            .unwrap();
        pdo_workspace::adapters::sqlite::init(&db).await.unwrap();
        for table in TRIGGER_TABLES {
            sqlx::query(table).execute(&db).await.unwrap();
        }
        let stats = stats(&db, &repo_root, home);
        Self {
            db,
            stats,
            repo_root,
            scratch,
        }
    }

    /// The same database and repo root, with `home` in place of `$HOME` for
    /// the transcripts, the stores and the price files.
    pub fn with_home(self, home: PathBuf) -> Self {
        Self {
            stats: stats(&self.db, &self.repo_root, Some(home)),
            ..self
        }
    }

    /// A directory of the scratch space, for a test's own home.
    pub fn scratch(&self, name: &str) -> PathBuf {
        let dir = self.scratch.path().join(name);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The context's router, as the local operator.
    pub fn router(&self) -> Router {
        pdo_stats::router(self.stats.clone()).layer(axum::Extension(Actor::LocalOperator))
    }

    /// How many times the Performance fold ran for `[from, to)`.
    pub fn recomputes(&self, from: &str, to: &str) -> u32 {
        self.stats.read_performance().recomputes(from, to)
    }

    /// A rename, recorded as the pipeline context's ledger records it.
    pub async fn record_rename(&self, old: (&str, &str), new: (&str, &str)) {
        use pdo_pipeline::application::ports::{RenameLedger, Renamed};
        pdo_stats::adapters::rename_ledger::StatsRenames(self.stats.clone())
            .record(&Renamed {
                old_id: old.0.into(),
                old_name: old.1.into(),
                new_id: new.0.into(),
                new_name: new.1.into(),
            })
            .await;
    }

    /// The stored absorptions, row by row.
    pub async fn absorption_rows(&self) -> Vec<pdo_stats::domain::absorption::AbsorptionRow> {
        pdo_stats::adapters::sqlite_absorptions::list(&self.db)
            .await
            .unwrap()
    }
}

fn stats(db: &sqlx::SqlitePool, repo_root: &Path, home: Option<PathBuf>) -> Arc<Stats> {
    let store = Arc::new(SqliteEventStore::new(db.clone()));
    Stats::new(
        StatsPorts {
            events: Arc::new(RunContextEvents::new(store.clone(), store)),
            triggers: Arc::new(DbFires(db.clone())),
            projects: Arc::new(DbProjects(db.clone())),
            absorptions: Arc::new(SqliteAbsorptions::new(db.clone())),
            run_files: Arc::new(DiskRunFiles::new(repo_root.to_path_buf(), home.clone())),
            transcripts: Arc::new(HarnessTranscripts),
            price_files: Arc::new(HomePriceFiles::new(home)),
            price_source: Arc::new(HttpPriceSource),
            audit: Arc::new(Unaudited),
            announcements: Arc::new(Silent),
            extensions: Arc::new(NoExtensions),
        },
        StatsOptions {
            repo_root: repo_root.to_path_buf(),
            price_source_url: None,
            price_refresh_at_boot: false,
        },
    )
}
