use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::{Actor, ResourceKind, Verb};
use pdo_platform::application::apply_update::{ApplyError, ApplyUpdate};
use pdo_platform::application::check_for_update::{CheckError, CheckForUpdate};
use pdo_platform::application::get_changelog::GetChangelog;
use pdo_platform::application::get_update_status::GetUpdateStatus;
use pdo_platform::application::read_attempt_log::{LogError, ReadAttemptLog};
use pdo_platform::application::update_context::{UpdateContext, UpdateOptions, UpdatePorts};
use pdo_platform::domain::attempt::AttemptStatus;
use pdo_platform::domain::changelog::ChangelogSource;
use pdo_platform::domain::install::{InstallMethod, Supervision};
use pdo_platform::domain::release::{Release, INSTALLED_VERSION};
use pdo_platform::domain::update_check::UpdateCache;

use crate::fakes::{Executor, FakeRuns, FixedInstallation, Host, Journal, Setting, Source};

/// A version strictly newer than this build.
fn newer() -> String {
    let (major, _, _) = pdo_platform::domain::release::parse_version(INSTALLED_VERSION).unwrap();
    format!("{}.0.0", major + 1)
}

struct World {
    setting: Arc<Setting>,
    source: Arc<Source>,
    journal: Arc<Journal>,
    executor: Arc<Executor>,
    host: Arc<Host>,
    context: Arc<UpdateContext>,
}

fn world(
    enabled: bool,
    source: Source,
    journal: Journal,
    method: InstallMethod,
    exit_code: Option<i32>,
) -> World {
    let setting = Arc::new(Setting(AtomicBool::new(enabled)));
    let source = Arc::new(source);
    let journal = Arc::new(journal);
    let executor = Arc::new(Executor {
        exit_code,
        plans: Mutex::new(Vec::new()),
    });
    let host = Host::allowing();
    let context = UpdateContext::new(
        UpdatePorts {
            setting: setting.clone(),
            source: source.clone(),
            journal: journal.clone(),
            installation: Arc::new(FixedInstallation(method, Supervision::None)),
            executor: executor.clone(),
            runs: Arc::new(FakeRuns {
                live: 2,
                ..FakeRuns::default()
            }),
        },
        UpdateOptions {
            source_url: Some("http://127.0.0.1:9/releases/latest".into()),
            port: 5172,
            working_dir: PathBuf::from("/home/u/.pdo/app"),
            relaunch_command: None,
        },
    );
    World {
        setting,
        source,
        journal,
        executor,
        host,
        context,
    }
}

impl World {
    fn check(&self) -> CheckForUpdate {
        CheckForUpdate::new(
            self.context.clone(),
            self.host.clone() as Arc<dyn ExtensionHost>,
        )
    }
    fn status(&self) -> GetUpdateStatus {
        GetUpdateStatus::new(self.context.clone())
    }
    fn changelog(&self) -> GetChangelog {
        GetChangelog::new(self.context.clone())
    }
    fn apply(&self) -> ApplyUpdate {
        ApplyUpdate::new(
            self.context.clone(),
            self.host.clone() as Arc<dyn ExtensionHost>,
        )
    }
}

fn source_ok(latest: &str) -> Source {
    Source::new(Ok(latest), Ok(Vec::new()))
}

#[tokio::test]
async fn a_check_on_demand_records_the_answer_and_journals_it() {
    let w = world(
        true,
        source_ok(&newer()),
        Journal::default(),
        InstallMethod::Script,
        Some(0),
    );
    w.check().execute(&Actor::LocalOperator).await.unwrap();
    let status = w.status().execute().await;
    assert_eq!(status.latest_version, Some(newer()));
    assert!(status.newer_available);
    assert_eq!(status.active_runs, 2);
    assert_eq!(status.source, "127.0.0.1:9");
    let journaled = w.journal.cache.lock().unwrap().clone().unwrap();
    assert_eq!(journaled.latest_version, Some(newer()));
    let asked = w.host.asked.lock().unwrap();
    assert_eq!(asked[0].resource, ResourceKind::Update);
    assert_eq!(asked[0].verb, Verb::Action("check".into()));
}

#[tokio::test]
async fn off_means_no_request_ever_leaves() {
    let w = world(
        false,
        source_ok("9.9.9"),
        Journal::default(),
        InstallMethod::Script,
        Some(0),
    );
    assert_eq!(
        w.check().execute(&Actor::LocalOperator).await,
        Err(CheckError::Off)
    );
    w.check().periodic().await;
    assert_eq!(w.source.latest_calls.load(Ordering::SeqCst), 0);
    let status = w.status().execute().await;
    assert_eq!(status.reason, Some("Update check is off."));
}

#[tokio::test]
async fn a_failed_check_keeps_the_last_good_version_and_says_so() {
    let journal = Journal::default();
    *journal.cache.lock().unwrap() = Some(UpdateCache {
        schema: "update-check-v1".into(),
        latest_version: Some("1.0.0".into()),
        ..UpdateCache::default()
    });
    let w = world(
        true,
        Source::new(Err("HTTP 500"), Ok(vec![])),
        journal,
        InstallMethod::Script,
        Some(0),
    );
    let err = w.check().execute(&Actor::LocalOperator).await.unwrap_err();
    assert_eq!(
        err,
        CheckError::Failed(
            "release source unreachable: http://127.0.0.1:9/releases/latest: HTTP 500".into()
        )
    );
    let status = w.status().execute().await;
    assert_eq!(status.latest_version.as_deref(), Some("1.0.0"));
    assert!(status.last_error.unwrap().contains("HTTP 500"));
}

#[tokio::test]
async fn the_periodic_pass_skips_a_fresh_cache() {
    let w = world(
        true,
        source_ok("1.0.0"),
        Journal::default(),
        InstallMethod::Script,
        Some(0),
    );
    w.check().periodic().await;
    w.check().periodic().await;
    assert_eq!(
        w.source.latest_calls.load(Ordering::SeqCst),
        1,
        "the cache IS the answer"
    );
    w.check().now().await;
    assert_eq!(
        w.source.latest_calls.load(Ordering::SeqCst),
        2,
        "now ignores the cache"
    );
}

/// The daemon's own checks (the 6 h loop, the one re-enabling the setting
/// triggers) are internal transitions, not Operations: an extension that
/// refuses everything neither stops them nor is asked about them.
#[tokio::test]
async fn the_daemons_own_checks_never_ask_the_authorization() {
    let mut w = world(
        true,
        source_ok(&newer()),
        Journal::default(),
        InstallMethod::Script,
        Some(0),
    );
    w.host = Host::refusing();
    w.check().periodic().await;
    w.check().now().await;
    assert_eq!(w.source.latest_calls.load(Ordering::SeqCst), 2);
    assert_eq!(w.status().execute().await.latest_version, Some(newer()));
    assert!(w.host.asked.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_refused_check_touches_nothing() {
    let mut w = world(
        true,
        source_ok("1.0.0"),
        Journal::default(),
        InstallMethod::Script,
        Some(0),
    );
    w.host = Host::refusing();
    assert!(matches!(
        w.check().execute(&Actor::LocalOperator).await,
        Err(CheckError::Refused(_))
    ));
    assert!(matches!(
        w.apply().execute(&Actor::LocalOperator).await,
        Err(ApplyError::Refused(_))
    ));
    assert_eq!(w.source.latest_calls.load(Ordering::SeqCst), 0);
    assert!(w.executor.plans.lock().unwrap().is_empty());
}

fn release(version: &str, body: &str) -> Release {
    Release {
        version: version.into(),
        published_at: Some("2026-09-01T00:00:00Z".into()),
        html_url: None,
        body: body.into(),
    }
}

#[tokio::test]
async fn the_changelog_falls_back_with_its_reason_and_memoises_the_list() {
    let w = world(
        false,
        source_ok("1.0.0"),
        Journal::default(),
        InstallMethod::Script,
        Some(0),
    );
    let off = w.changelog().execute().await;
    assert_eq!(
        off.fallback_reason.as_deref(),
        Some("The update check is off.")
    );

    w.setting.0.store(true, Ordering::SeqCst);
    let unchecked = w.changelog().execute().await;
    assert_eq!(
        unchecked.fallback_reason.as_deref(),
        Some("The release source has not been checked yet.")
    );

    let latest = newer();
    let w = world(
        true,
        Source::new(Ok(&latest), Ok(vec![release(&latest, "- big")])),
        Journal::default(),
        InstallMethod::Script,
        Some(0),
    );
    w.check().execute(&Actor::LocalOperator).await.unwrap();
    let c = w.changelog().execute().await;
    assert_eq!(c.source, ChangelogSource::Releases);
    assert_eq!(c.missed_versions, std::slice::from_ref(&latest));
    w.changelog().execute().await;
    assert_eq!(
        w.source.releases_calls.load(Ordering::SeqCst),
        1,
        "memoised per latest"
    );

    let w = world(
        true,
        Source::new(Ok(&latest), Err("down")),
        Journal::default(),
        InstallMethod::Script,
        Some(0),
    );
    w.check().execute(&Actor::LocalOperator).await.unwrap();
    let down = w.changelog().execute().await;
    assert_eq!(down.source, ChangelogSource::Embedded);
    assert_eq!(down.latest_version, Some(latest));
    assert_eq!(
        down.fallback_reason.as_deref(),
        Some("The release source was unreachable when loading the release notes.")
    );
}

#[tokio::test]
async fn an_unknown_method_never_applies_and_no_home_is_said() {
    let w = world(
        true,
        source_ok("1.0.0"),
        Journal::default(),
        InstallMethod::Unknown,
        Some(0),
    );
    match w.apply().execute(&Actor::LocalOperator).await {
        Err(ApplyError::Blocked { reason, method }) => {
            assert_eq!(method, InstallMethod::Unknown);
            assert!(reason.unwrap().starts_with("Install method not detected"));
        }
        other => panic!("expected a closed gate, got {other:?}"),
    }
    let homeless = Journal {
        unavailable: true,
        ..Journal::default()
    };
    let w = world(
        true,
        source_ok("1.0.0"),
        homeless,
        InstallMethod::Script,
        Some(0),
    );
    assert_eq!(
        w.apply().execute(&Actor::LocalOperator).await,
        Err(ApplyError::NoHome)
    );
}

#[tokio::test]
async fn an_apply_spawns_the_plan_journals_it_and_records_the_exit() {
    let w = world(
        true,
        source_ok("1.0.0"),
        Journal::default(),
        InstallMethod::Homebrew,
        Some(3),
    );
    let started = w.apply().execute(&Actor::LocalOperator).await.unwrap();
    assert_eq!(
        started.command,
        "brew update && brew upgrade Loulen/tap/pdo"
    );
    assert_eq!(started.from_version, INSTALLED_VERSION);
    let plans = w.executor.plans.lock().unwrap().clone();
    assert_eq!(plans.len(), 1);
    assert_eq!(
        plans[0].relaunch,
        [
            "/home/linuxbrew/.linuxbrew/bin/pdo",
            "daemon",
            "--port",
            "5172"
        ]
    );
    assert!(w.journal.scripts.lock().unwrap()[&started.attempt_id].starts_with("#!/bin/sh"));
    // The executor exits at once with 3: the reaper records it off-thread.
    for _ in 0..100 {
        let a = w.journal.attempt.lock().unwrap().clone().unwrap();
        if a.status != AttemptStatus::Running {
            assert_eq!(a.status, AttemptStatus::Failed);
            assert_eq!(a.exit_code, Some(3));
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("the exit was never recorded");
}

#[tokio::test]
async fn a_running_attempt_closes_the_gate_and_a_spawn_failure_is_journaled() {
    let w = world(
        true,
        source_ok("1.0.0"),
        Journal::default(),
        InstallMethod::Script,
        None,
    );
    let err = w.apply().execute(&Actor::LocalOperator).await.unwrap_err();
    let ApplyError::Failed { error, attempt_id } = err else {
        panic!("expected a spawn failure, got {err:?}");
    };
    assert!(error.contains("cannot spawn"));
    let a = w.journal.attempt.lock().unwrap().clone().unwrap();
    assert_eq!(a.status, AttemptStatus::Failed);
    let log = ReadAttemptLog::new(w.context.clone())
        .execute(&attempt_id)
        .unwrap();
    assert!(log.contains("cannot spawn"));

    let mut running = a.clone();
    running.status = AttemptStatus::Running;
    running.started_at = pdo_kernel::clock::now_iso();
    *w.journal.attempt.lock().unwrap() = Some(running);
    assert!(matches!(
        w.apply().execute(&Actor::LocalOperator).await,
        Err(ApplyError::Blocked { .. })
    ));
    assert!(!w.status().execute().await.can_apply);
}

#[tokio::test]
async fn attempt_logs_are_read_by_a_safe_id_only() {
    let w = world(
        true,
        source_ok("1.0.0"),
        Journal::default(),
        InstallMethod::Script,
        Some(0),
    );
    let read = ReadAttemptLog::new(w.context.clone());
    assert_eq!(read.execute("../etc"), Err(LogError::InvalidId));
    assert_eq!(read.execute("a1"), Err(LogError::NotFound));
}
