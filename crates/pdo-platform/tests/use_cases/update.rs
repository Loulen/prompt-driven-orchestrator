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
use pdo_platform::domain::release::Release;
use pdo_platform::domain::release_channel::{
    ReleaseCredential, UpdateChannel, CORE_VERSION as INSTALLED_VERSION,
};
use pdo_platform::domain::update_check::UpdateCache;

use crate::fakes::{
    Executor, FakeRuns, FixedInstallation, Host, Journal, Setting, Snapshotter, Source,
};

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
    snapshotter: Arc<Snapshotter>,
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
    let options = UpdateOptions {
        channel: UpdateChannel::vanilla(),
        source_url: Some("http://127.0.0.1:9/releases/latest".into()),
        port: 5172,
        working_dir: PathBuf::from("/home/u/.pdo/app"),
        relaunch_command: None,
        verify_timeout_secs: None,
    };
    world_with(options, enabled, source, journal, method, exit_code)
}

/// A world whose update knobs (the release channel, the operator's source) are
/// `options`.
fn world_with(
    options: UpdateOptions,
    enabled: bool,
    source: Source,
    journal: Journal,
    method: InstallMethod,
    exit_code: Option<i32>,
) -> World {
    world_with_snapshot(
        options,
        enabled,
        source,
        journal,
        method,
        exit_code,
        Snapshotter::default(),
    )
}

/// [`world_with`], with the snapshotter of the Retour automatique.
fn world_with_snapshot(
    options: UpdateOptions,
    enabled: bool,
    source: Source,
    journal: Journal,
    method: InstallMethod,
    exit_code: Option<i32>,
    snapshotter: Snapshotter,
) -> World {
    let snapshotter = Arc::new(snapshotter);
    let setting = Arc::new(Setting(AtomicBool::new(enabled)));
    let source = Arc::new(source);
    let journal = Arc::new(journal);
    let executor = Arc::new(Executor {
        exit_code,
        plans: Mutex::new(Vec::new()),
        tokens: Mutex::new(Vec::new()),
    });
    let host = Host::allowing();
    let context = UpdateContext::new(
        UpdatePorts {
            setting: setting.clone(),
            source: source.clone(),
            journal: journal.clone(),
            installation: Arc::new(FixedInstallation(method, Supervision::None)),
            executor: executor.clone(),
            snapshot: snapshotter.clone(),
            runs: Arc::new(FakeRuns {
                live: 2,
                ..FakeRuns::default()
            }),
        },
        options,
    );
    World {
        setting,
        source,
        journal,
        executor,
        snapshotter,
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
    assert_eq!(status.source.as_deref(), Some("127.0.0.1:9"));
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
        Err(ApplyError::Blocked { reason, method, .. }) => {
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
    // The Retour automatique: the snapshot is taken before the spawn, and the
    // plan carries it, the readiness marker and the verification's bound.
    assert_eq!(*w.snapshotter.taken.lock().unwrap(), 1);
    let snapshot = plans[0].snapshot.as_ref().expect("a snapshot");
    assert_eq!(snapshot.dir, PathBuf::from("/home/u/.pdo/update/snapshot"));
    assert_eq!(
        snapshot.binary.original,
        PathBuf::from("/home/linuxbrew/.linuxbrew/bin/pdo")
    );
    assert_eq!(
        plans[0].ready_marker,
        PathBuf::from("/home/u/.pdo/update/daemon-5172.json")
    );
    assert_eq!(plans[0].verify_timeout_secs, 90);
    let running = w.journal.attempt.lock().unwrap().clone().unwrap();
    assert_eq!(running.verify_timeout_secs, Some(90));
    assert!(!running.rolled_back);
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
async fn an_update_whose_snapshot_fails_does_not_start() {
    let options = UpdateOptions {
        channel: UpdateChannel::vanilla(),
        source_url: Some("http://127.0.0.1:9/releases/latest".into()),
        port: 5172,
        working_dir: PathBuf::from("/home/u/.pdo/app"),
        relaunch_command: None,
        verify_timeout_secs: Some(5),
    };
    let w = world_with_snapshot(
        options,
        true,
        source_ok("1.0.0"),
        Journal::default(),
        InstallMethod::Homebrew,
        Some(0),
        Snapshotter {
            error: Some("disk full".into()),
            ..Snapshotter::default()
        },
    );
    let err = w.apply().execute(&Actor::LocalOperator).await.unwrap_err();
    let ApplyError::Failed { error, .. } = err else {
        panic!("expected a failure, got {err:?}");
    };
    assert_eq!(error, "the snapshot before the update failed: disk full");
    assert!(
        w.executor.plans.lock().unwrap().is_empty(),
        "nothing spawned"
    );
    assert!(
        w.journal.attempt.lock().unwrap().is_none(),
        "nothing journaled"
    );
}

#[tokio::test]
async fn the_daemon_announces_it_serves_in_its_channels_version() {
    let w = world_with(
        channel_options(None, Some("acme-upgrade")),
        true,
        source_ok("1.0.0"),
        Journal::default(),
        InstallMethod::Declared,
        Some(0),
    );
    w.context.announce_ready();
    let marker = w.journal.ready.lock().unwrap().clone().unwrap();
    assert_eq!(marker.version, "0.3.0");
    assert_eq!(marker.pid, std::process::id());
}

#[tokio::test]
async fn the_verification_bound_targets_the_newest_known_version() {
    let newer = newer();
    let options = UpdateOptions {
        channel: UpdateChannel::vanilla(),
        source_url: Some("http://127.0.0.1:9/releases/latest".into()),
        port: 5172,
        working_dir: PathBuf::from("/home/u/.pdo/app"),
        relaunch_command: None,
        verify_timeout_secs: Some(7),
    };
    let w = world_with(
        options,
        true,
        source_ok(&newer),
        Journal::default(),
        InstallMethod::Homebrew,
        Some(0),
    );
    w.check().now().await;
    w.apply().execute(&Actor::LocalOperator).await.unwrap();
    let plan = w.executor.plans.lock().unwrap()[0].clone();
    assert_eq!(plan.verify_timeout_secs, 7);
    assert_eq!(plan.to_version.as_deref(), Some(newer.as_str()));
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

// ---- The release channel of a distribution (#24) ---------------------------

fn channel_options(source_url: Option<&str>, upgrade_command: Option<&str>) -> UpdateOptions {
    UpdateOptions {
        channel: UpdateChannel::declared(
            "acme",
            "0.3.0",
            source_url.map(str::to_string),
            upgrade_command.map(str::to_string),
        ),
        source_url: None,
        port: 5172,
        working_dir: PathBuf::from("/home/u/.pdo/app"),
        relaunch_command: None,
        verify_timeout_secs: None,
    }
}

#[tokio::test]
async fn a_declared_channel_compares_its_own_version_and_runs_its_own_command() {
    // A Homebrew install detected on the host must not matter: the channel says.
    let w = world_with(
        channel_options(
            Some("http://127.0.0.1:9/releases/latest"),
            Some("acme-upgrade"),
        ),
        true,
        source_ok("0.4.0"),
        Journal::default(),
        InstallMethod::Homebrew,
        Some(0),
    );
    w.check().execute(&Actor::LocalOperator).await.unwrap();
    let status = w.status().execute().await;
    assert_eq!(status.channel_name, "acme");
    assert_eq!(status.installed_version, "0.3.0");
    assert_eq!(status.core_version, INSTALLED_VERSION);
    assert_eq!(status.latest_version.as_deref(), Some("0.4.0"));
    assert!(status.newer_available);
    assert_eq!(status.install_method, InstallMethod::Declared);
    assert_eq!(status.manual_command.as_deref(), Some("acme-upgrade"));
    assert!(status.can_apply);

    let started = w.apply().execute(&Actor::LocalOperator).await.unwrap();
    assert_eq!(started.command, "acme-upgrade");
    assert_eq!(started.method, InstallMethod::Declared);
    assert_eq!(started.from_version, "0.3.0");
    assert_eq!(
        w.executor.plans.lock().unwrap()[0].upgrade_command,
        "acme-upgrade"
    );
}

#[tokio::test]
async fn a_channel_without_a_command_never_applies() {
    let w = world_with(
        channel_options(Some("http://127.0.0.1:9/releases/latest"), None),
        true,
        source_ok("0.4.0"),
        Journal::default(),
        InstallMethod::Script,
        Some(0),
    );
    let status = w.status().execute().await;
    assert!(!status.can_apply);
    assert!(status
        .apply_blocked_reason
        .is_some_and(|r| r.starts_with("acme declares no update command")));
    match w.apply().execute(&Actor::LocalOperator).await {
        Err(ApplyError::Blocked {
            method,
            manual_command,
            ..
        }) => {
            assert_eq!(method, InstallMethod::Declared);
            assert_eq!(manual_command, None);
        }
        other => panic!("expected a closed gate, got {other:?}"),
    }
    assert!(w.executor.plans.lock().unwrap().is_empty());
}

#[tokio::test]
async fn no_source_means_no_request_and_an_explicit_reason() {
    let w = world_with(
        channel_options(None, Some("acme-upgrade")),
        true,
        source_ok("0.4.0"),
        Journal::default(),
        InstallMethod::Script,
        Some(0),
    );
    assert!(matches!(
        w.check().execute(&Actor::LocalOperator).await,
        Err(CheckError::NoSource(_))
    ));
    w.check().periodic().await;
    w.check().now().await;
    let changelog = w.changelog().execute().await;
    assert_eq!(w.source.latest_calls.load(Ordering::SeqCst), 0);
    assert_eq!(w.source.releases_calls.load(Ordering::SeqCst), 0);
    assert!(changelog.fallback_reason.is_some());
    let status = w.status().execute().await;
    assert_eq!(status.latest_version, None);
    assert!(status.reason.is_some_and(|r| !r.is_empty()));
    assert_eq!(status.source_url, None);
}

#[tokio::test]
async fn the_operators_source_wins_over_the_channels() {
    let mut options = channel_options(Some("http://channel.test/releases/latest"), None);
    options.source_url = Some("http://127.0.0.1:9/releases/latest".into());
    let w = world_with(
        options,
        true,
        source_ok("0.4.0"),
        Journal::default(),
        InstallMethod::Script,
        Some(0),
    );
    w.check().execute(&Actor::LocalOperator).await.unwrap();
    let status = w.status().execute().await;
    assert_eq!(
        status.source_url.as_deref(),
        Some("http://127.0.0.1:9/releases/latest")
    );
    assert_eq!(status.latest_version.as_deref(), Some("0.4.0"));
}

#[tokio::test]
async fn a_cache_left_by_another_source_is_never_an_answer() {
    // `~/.pdo/update/check.json` is shared by every binary of the same home.
    let journal = Journal::default();
    *journal.cache.lock().unwrap() = Some(UpdateCache {
        schema: "update-check-v1".into(),
        source: "https://api.github.com/repos/someone/else/releases/latest".into(),
        checked_at: Some(pdo_kernel::clock::now_iso()),
        latest_version: Some("9.0.0".into()),
        error: None,
    });
    let w = world_with(
        channel_options(Some("http://127.0.0.1:9/releases/latest"), None),
        true,
        source_ok("0.4.0"),
        journal,
        InstallMethod::Script,
        Some(0),
    );
    assert_eq!(w.status().execute().await.latest_version, None);
    // Fresh or not, another source's cache does not spare this source a check.
    w.check().periodic().await;
    assert_eq!(w.source.latest_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        w.status().execute().await.latest_version.as_deref(),
        Some("0.4.0")
    );
}

// ---- The release identifier of a channel (CORE-313) ------------------------

/// A channel whose identifier provider answers what `value` holds at the time.
fn credential_options(
    value: &Arc<Mutex<Option<String>>>,
    credential: fn(Arc<Mutex<Option<String>>>) -> ReleaseCredential,
) -> UpdateOptions {
    let mut options = channel_options(
        Some("http://127.0.0.1:9/releases/latest"),
        Some("acme-upgrade"),
    );
    options.channel.credential = Some(credential(value.clone()));
    options
}

fn declared(value: Arc<Mutex<Option<String>>>) -> ReleaseCredential {
    ReleaseCredential::declared(move || value.lock().unwrap().clone())
}

fn operator(value: Arc<Mutex<Option<String>>>) -> ReleaseCredential {
    ReleaseCredential::operator(move || value.lock().unwrap().clone())
}

#[tokio::test]
async fn every_check_presents_the_identifier_the_provider_answers_then() {
    let value = Arc::new(Mutex::new(Some("tok-1".to_string())));
    let latest = "0.4.0";
    let w = world_with(
        credential_options(&value, declared),
        true,
        Source::new(Ok(latest), Ok(vec![release(latest, "- a")])),
        Journal::default(),
        InstallMethod::Script,
        Some(0),
    );
    w.check().execute(&Actor::LocalOperator).await.unwrap();
    w.changelog().execute().await;
    *value.lock().unwrap() = Some("tok-2".into());
    w.check().execute(&Actor::LocalOperator).await.unwrap();
    *value.lock().unwrap() = None;
    w.check().execute(&Actor::LocalOperator).await.unwrap();
    assert_eq!(
        *w.source.tokens.lock().unwrap(),
        vec![
            Some("tok-1".to_string()),
            Some("tok-1".to_string()),
            Some("tok-2".to_string()),
            None
        ],
        "latest, the notes, then each check asks again; nothing: no header"
    );
}

#[tokio::test]
async fn without_a_credential_nothing_is_presented() {
    let w = world_with(
        channel_options(
            Some("http://127.0.0.1:9/releases/latest"),
            Some("acme-upgrade"),
        ),
        true,
        source_ok("0.4.0"),
        Journal::default(),
        InstallMethod::Script,
        Some(0),
    );
    w.check().execute(&Actor::LocalOperator).await.unwrap();
    w.apply().execute(&Actor::LocalOperator).await.unwrap();
    assert_eq!(*w.source.tokens.lock().unwrap(), vec![None]);
    assert_eq!(*w.executor.tokens.lock().unwrap(), vec![None]);
    assert!(!w.executor.plans.lock().unwrap()[0].release_token_for_command_only);
}

#[tokio::test]
async fn the_update_command_receives_the_binarys_identifier_outside_the_plan_and_the_record() {
    let value = Arc::new(Mutex::new(Some("tok-secret-1".to_string())));
    let w = world_with(
        credential_options(&value, declared),
        true,
        source_ok("0.4.0"),
        Journal::default(),
        InstallMethod::Script,
        Some(0),
    );
    w.check().execute(&Actor::LocalOperator).await.unwrap();
    *value.lock().unwrap() = Some("tok-secret-2".into());
    let started = w.apply().execute(&Actor::LocalOperator).await.unwrap();
    assert_eq!(
        *w.executor.tokens.lock().unwrap(),
        vec![Some("tok-secret-2".to_string())],
        "asked again at apply time"
    );
    let plan = w.executor.plans.lock().unwrap()[0].clone();
    assert!(plan.release_token_for_command_only);
    let attempt = w.journal.attempt.lock().unwrap().clone();
    let status = w.status().execute().await;
    let shown = format!("{plan:?} {started:?} {attempt:?} {status:?}");
    assert!(!shown.contains("tok-secret"), "{shown}");
}

#[tokio::test]
async fn the_operators_identifier_reaches_the_command_and_stays_with_the_daemon() {
    let value = Arc::new(Mutex::new(Some("tok-op".to_string())));
    let w = world_with(
        credential_options(&value, operator),
        true,
        source_ok("0.4.0"),
        Journal::default(),
        InstallMethod::Script,
        Some(0),
    );
    w.check().execute(&Actor::LocalOperator).await.unwrap();
    w.apply().execute(&Actor::LocalOperator).await.unwrap();
    assert_eq!(
        *w.source.tokens.lock().unwrap(),
        vec![Some("tok-op".to_string())]
    );
    assert_eq!(
        *w.executor.tokens.lock().unwrap(),
        vec![Some("tok-op".to_string())]
    );
    assert!(!w.executor.plans.lock().unwrap()[0].release_token_for_command_only);
}
