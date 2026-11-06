//! The settings context over its real adapters, driven through its router: the
//! stored tier over SQLite, every context's section under a temporary home,
//! the real `docker` probe (absent in CI → unavailable, with a reason). The
//! legacy crate's router `oneshot` tests of `/settings`, moved.

mod put;
mod telemetry;
mod view;

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::{Extension, Router};
use pdo_harness::adapters::binary_probe::installed_catalogue_reader;
use pdo_kernel::extension_host::NoExtensions;
use pdo_kernel::Actor;
use pdo_sandbox::adapters::docker_cli::ProcessDocker;
use pdo_sandbox::adapters::sqlite_profiles::SqliteProfiles;
use pdo_settings::adapters::contexts::{ContextReferents, SandboxStagingProfiles};
use pdo_settings::adapters::sections::{
    HarnessCatalogue, HostHome, PlatformDefaults, PriceTableSection, ReviewDefaults, RunDefaults,
    SandboxSection, SessionDefaults, TelemetryDefaults, TriggerDefaults,
};
use pdo_settings::adapters::sqlite::{
    SqliteAgentProfiles, SqliteAppearances, SqliteInstanceConfig,
};
use pdo_settings::application::ports::{
    BoxFuture, CatalogueEvents, SkillBank, TelemetryReloads, UpdateChecks,
};
use pdo_skill::domain::selection::SkillRef;
use pdo_settings::{Settings, SettingsPorts};
use sqlx::SqlitePool;
use tower::ServiceExt;

/// The version check: nothing to reach from a test.
struct NoChecks;

impl UpdateChecks for NoChecks {
    fn check_now(&self) {}
}

impl TelemetryReloads for NoChecks {
    fn reload(&self) {}
}

/// A bank holding every skill: the routes' tests are not about it.
struct EveryoneSkill;

impl SkillBank for EveryoneSkill {
    fn refusal<'a>(
        &'a self,
        _current: &'a [SkillRef],
        _next: &'a [SkillRef],
    ) -> BoxFuture<'a, Result<Option<String>, String>> {
        Box::pin(std::future::ready(Ok(None)))
    }
}

struct NoCatalogueEvents;

impl CatalogueEvents for NoCatalogueEvents {
    fn catalogue_refreshed(&self, _harness: &str) {}
}

pub(super) struct TestState {
    pub db: SqlitePool,
    pub settings: Arc<Settings>,
    _home: tempfile::TempDir,
}

/// The context over a fresh in-memory database carrying every table its
/// adapters read (the Projets, the Triggers, the staging profiles).
pub(super) async fn test_state() -> TestState {
    let db = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    pdo_settings::adapters::sqlite::init(&db).await.unwrap();
    pdo_sandbox::adapters::sqlite_profiles::init(&db)
        .await
        .unwrap();
    pdo_workspace::adapters::sqlite::init(&db).await.unwrap();
    pdo_trigger::adapters::sqlite::init(&db).await.unwrap();
    let home_dir = tempfile::tempdir().unwrap();
    let home = HostHome {
        override_home: Some(home_dir.path().to_path_buf()),
    };
    let profiles = Arc::new(SqliteProfiles::new(db.clone()));
    let harnesses = Arc::new(HarnessCatalogue {
        home: home.clone(),
        reader: installed_catalogue_reader(),
    });
    let settings = Settings::new(SettingsPorts {
        config: Arc::new(SqliteInstanceConfig::new(db.clone())),
        profiles: Arc::new(SqliteAgentProfiles::new(db.clone())),
        appearances: Arc::new(SqliteAppearances::new(db.clone())),
        sections: vec![
            Arc::new(RunDefaults),
            Arc::new(SessionDefaults),
            Arc::new(TriggerDefaults),
            Arc::new(ReviewDefaults),
            Arc::new(PlatformDefaults),
            Arc::new(TelemetryDefaults),
            Arc::new(SandboxSection::new(
                profiles.clone(),
                Arc::new(ProcessDocker::from_override(None)),
                home.clone(),
            )),
            harnesses.clone(),
            Arc::new(PriceTableSection { home }),
        ],
        harnesses,
        catalogue_events: Arc::new(NoCatalogueEvents),
        staging_profiles: Arc::new(SandboxStagingProfiles(profiles)),
        skills: Arc::new(EveryoneSkill),
        update_checks: Arc::new(NoChecks),
        telemetry: Arc::new(NoChecks),
        referents: Arc::new(ContextReferents {
            db: db.clone(),
            repo_root: home_dir.path().to_path_buf(),
        }),
        extensions: Arc::new(NoExtensions),
    });
    TestState {
        db,
        settings,
        _home: home_dir,
    }
}

/// The context's router as the composition root merges it, behind the
/// identification of the local operator.
fn app(state: &TestState) -> Router {
    pdo_settings::router(state.settings.clone()).layer(Extension(Actor::LocalOperator))
}

pub(super) async fn get_settings_json(state: &TestState) -> serde_json::Value {
    let resp = app(state)
        .oneshot(
            Request::builder()
                .uri("/settings")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "GET /settings should 200");
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&body).unwrap()
}

pub(super) async fn post_json(state: &TestState, uri: &str) -> (StatusCode, serde_json::Value) {
    let resp = app(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, serde_json::from_slice(&body).unwrap())
}

pub(super) async fn put_settings_resp(
    state: &TestState,
    body: &str,
) -> (StatusCode, serde_json::Value) {
    let resp = app(state)
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/settings")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}
