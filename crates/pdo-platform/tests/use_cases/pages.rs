use std::path::PathBuf;
use std::sync::Arc;

use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::{Actor, ResourceKind, Verb};
use pdo_platform::application::mount_page::{MountPage, MountPageCommand, MountPageError};
use pdo_platform::application::page_journal::PageMountLock;
use pdo_platform::application::read_pages::ReadPages;
use pdo_platform::application::record_audit::RecordAudit;
use pdo_platform::application::release_run_pages::ReleaseRunPages;
use pdo_platform::application::unmount_page::{UnmountPage, UnmountPageError};
use pdo_platform::domain::audit::ActorHint;
use pdo_platform::domain::page_mount::MountError;

use crate::fakes::{Audit, FakeRuns, Files, Host, Pages};

struct World {
    pages: Arc<Pages>,
    runs: Arc<FakeRuns>,
    audit: Arc<Audit>,
    host: Arc<Host>,
    lock: PageMountLock,
}

impl World {
    fn new(runs: FakeRuns, host: Arc<Host>) -> Self {
        Self {
            pages: Arc::new(Pages::default()),
            runs: Arc::new(runs),
            audit: Arc::new(Audit::default()),
            host,
            lock: PageMountLock::default(),
        }
    }

    fn allowing() -> Self {
        Self::new(FakeRuns::default(), Host::allowing())
    }

    fn mount(&self) -> MountPage {
        MountPage::new(
            self.pages.clone(),
            Arc::new(Files::default()),
            self.runs.clone(),
            RecordAudit::new(self.audit.clone()),
            self.host.clone() as Arc<dyn ExtensionHost>,
            self.lock.clone(),
        )
    }

    fn unmount(&self) -> UnmountPage {
        UnmountPage::new(
            self.pages.clone(),
            self.runs.clone(),
            RecordAudit::new(self.audit.clone()),
            self.host.clone() as Arc<dyn ExtensionHost>,
            self.lock.clone(),
        )
    }
}

fn command(name: &str, directory: &str) -> MountPageCommand {
    MountPageCommand {
        name: name.into(),
        directory: PathBuf::from(directory),
        hint: ActorHint::Cli,
    }
}

fn agent() -> Actor {
    Actor::Agent {
        run_id: "r1".into(),
        node_id: "n1".into(),
    }
}

#[tokio::test]
async fn an_operator_mount_is_stored_and_audited() {
    let w = World::allowing();
    let mount = w
        .mount()
        .execute(&Actor::LocalOperator, command("demo", "/srv/demo"))
        .await
        .unwrap();
    assert_eq!(mount.directory, "/srv/demo");
    assert_eq!(mount.run_id, None);
    assert!(w.pages.0.lock().unwrap().contains_key("demo"));
    let rows = w.audit.rows.lock().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].action, "page_mount.mounted");
    assert_eq!(rows[0].actor_hint, "cli");
    assert_eq!(rows[0].target_kind.as_deref(), Some("page_mount"));
    assert_eq!(rows[0].target_id.as_deref(), Some("demo"));
    assert!(rows[0].before.is_none());
    assert_eq!(rows[0].after.as_ref().unwrap()["name"], "demo");
    let asked = w.host.asked.lock().unwrap();
    assert_eq!(asked[0].resource, ResourceKind::PageMount);
    assert_eq!(asked[0].verb, Verb::Create);
}

#[tokio::test]
async fn an_agent_mount_belongs_to_its_run_and_is_journaled_there() {
    let runs = FakeRuns {
        live_sessions: vec![("r1".into(), "n1".into())],
        ..FakeRuns::default()
    };
    let w = World::new(runs, Host::allowing());
    let mount = w
        .mount()
        .execute(&agent(), command("demo", "/srv/demo"))
        .await
        .unwrap();
    assert_eq!(mount.run_id.as_deref(), Some("r1"));
    assert!(w.audit.rows.lock().unwrap().is_empty(), "not audited");
    let commands = w.runs.commands.lock().unwrap();
    assert_eq!(commands.len(), 1);
    assert_eq!(
        (
            commands[0].0.as_str(),
            commands[0].1.as_str(),
            commands[0].2.as_str()
        ),
        ("r1", "n1", "page_mount.mounted")
    );
    assert_eq!(commands[0].3["name"], "demo");
}

#[tokio::test]
async fn a_stale_session_claim_is_refused_before_any_effect() {
    let w = World::allowing();
    let err = w
        .mount()
        .execute(&agent(), command("demo", "/srv/demo"))
        .await
        .unwrap_err();
    assert!(
        matches!(err, MountPageError::SessionRefused(m) if m.contains("session claim refused"))
    );
    assert!(w.pages.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn an_extension_refusal_stops_the_mount_before_any_effect() {
    let w = World::new(FakeRuns::default(), Host::refusing());
    let err = w
        .mount()
        .execute(&Actor::LocalOperator, command("demo", "/srv/demo"))
        .await
        .unwrap_err();
    match err {
        MountPageError::Refused(denied) => assert_eq!(denied.reason, "admins only"),
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert!(w.pages.0.lock().unwrap().is_empty());
    assert!(w.audit.rows.lock().unwrap().is_empty());
}

#[tokio::test]
async fn invalid_names_directories_and_duplicates_are_refused() {
    let w = World::allowing();
    let mount = w.mount();
    let op = Actor::LocalOperator;
    assert!(matches!(
        mount.execute(&op, command("Bad Name", "/srv")).await,
        Err(MountPageError::Invalid(MountError::Invalid(_)))
    ));
    assert!(matches!(
        mount.execute(&op, command("demo", "/missing")).await,
        Err(MountPageError::Invalid(MountError::Invalid(_)))
    ));
    assert!(matches!(
        mount.execute(&op, command("demo", "/repo/.pdo/artifacts/n1")).await,
        Err(MountPageError::Invalid(MountError::Invalid(m))) if m.contains(".pdo/artifacts")
    ));
    mount.execute(&op, command("demo", "/srv")).await.unwrap();
    assert_eq!(
        mount.execute(&op, command("demo", "/srv")).await,
        Err(MountPageError::Invalid(MountError::Duplicate(
            "demo".into()
        )))
    );
}

#[tokio::test]
async fn a_journal_failure_rolls_the_mount_back() {
    let runs = FakeRuns {
        live_sessions: vec![("r1".into(), "n1".into())],
        journal_fails: true,
        ..FakeRuns::default()
    };
    let w = World::new(runs, Host::allowing());
    let err = w
        .mount()
        .execute(&agent(), command("demo", "/srv/demo"))
        .await
        .unwrap_err();
    assert_eq!(err, MountPageError::Journal("event log is locked".into()));
    assert!(w.pages.0.lock().unwrap().is_empty(), "rolled back");
}

#[tokio::test]
async fn unmount_removes_audits_and_restores_on_journal_failure() {
    let w = World::allowing();
    let op = Actor::LocalOperator;
    w.mount()
        .execute(&op, command("demo", "/srv"))
        .await
        .unwrap();
    let removed = w
        .unmount()
        .execute(&op, "demo", ActorHint::Ui)
        .await
        .unwrap();
    assert_eq!(removed.name, "demo");
    let rows = w.audit.rows.lock().unwrap().clone();
    assert_eq!(rows[1].action, "page_mount.unmounted");
    assert_eq!(rows[1].actor_hint, "ui");
    assert!(rows[1].after.is_none());
    assert_eq!(
        w.unmount().execute(&op, "demo", ActorHint::Ui).await,
        Err(UnmountPageError::NotFound("demo".into()))
    );
    let delete = w
        .host
        .asked
        .lock()
        .unwrap()
        .iter()
        .find(|o| o.verb == Verb::Delete)
        .cloned()
        .unwrap();
    assert_eq!(delete.target.as_deref(), Some("demo"));

    let runs = FakeRuns {
        live_sessions: vec![("r1".into(), "n1".into())],
        journal_fails: true,
        ..FakeRuns::default()
    };
    let w = World::new(runs, Host::allowing());
    w.mount()
        .execute(&op, command("demo", "/srv"))
        .await
        .unwrap();
    let err = w
        .unmount()
        .execute(&agent(), "demo", ActorHint::Cli)
        .await
        .unwrap_err();
    assert!(matches!(err, UnmountPageError::Journal(_)));
    assert!(w.pages.0.lock().unwrap().contains_key("demo"), "restored");
}

#[tokio::test]
async fn archiving_a_run_releases_its_pages_only() {
    let runs = FakeRuns {
        live_sessions: vec![("r1".into(), "n1".into())],
        ..FakeRuns::default()
    };
    let w = World::new(runs, Host::allowing());
    w.mount()
        .execute(&agent(), command("mine", "/srv/a"))
        .await
        .unwrap();
    w.mount()
        .execute(&Actor::LocalOperator, command("theirs", "/srv/b"))
        .await
        .unwrap();
    let release = ReleaseRunPages::new(w.pages.clone(), w.lock.clone());
    let guard = release.hold().await;
    assert_eq!(release.execute("r1").await.unwrap(), 1);
    drop(guard);
    let names: Vec<String> = w.pages.0.lock().unwrap().keys().cloned().collect();
    assert_eq!(names, ["theirs"]);
}

#[tokio::test]
async fn files_are_read_beneath_their_mount_only() {
    let w = World::allowing();
    w.mount()
        .execute(&Actor::LocalOperator, command("demo", "/srv/demo"))
        .await
        .unwrap();
    let files = Files(
        [("/srv/demo/a.html".to_string(), b"a".to_vec())]
            .into_iter()
            .collect(),
    );
    let read = ReadPages::new(w.pages.clone(), Arc::new(files));
    assert_eq!(
        read.file("demo", "a.html").await.unwrap().unwrap().contents,
        b"a"
    );
    assert!(read.file("demo", "../demo/a.html").await.unwrap().is_none());
    assert!(read.file("nope", "a.html").await.unwrap().is_none());
    assert_eq!(read.list().await.unwrap().len(), 1);
}
