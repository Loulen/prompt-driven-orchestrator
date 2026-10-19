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
        self.mount_over(Files::default())
    }

    fn mount_over(&self, files: Files) -> MountPage {
        MountPage::new(
            self.pages.clone(),
            Arc::new(files),
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
        guide: None,
        hint: ActorHint::Cli,
    }
}

fn guided(guide: &str) -> MountPageCommand {
    MountPageCommand {
        guide: Some(guide.into()),
        ..command("demo", "/srv/demo")
    }
}

/// A world where Run `r1` has the nodes `n1` and `design`, and the page has
/// `index.html` and `sub/b.html`.
fn guided_world() -> (World, Files) {
    let runs = FakeRuns {
        live_sessions: vec![("r1".into(), "n1".into())],
        nodes: [(
            "r1".to_string(),
            vec!["n1".to_string(), "design".to_string()],
        )]
        .into_iter()
        .collect(),
        ..FakeRuns::default()
    };
    let files = Files(
        [
            ("/srv/demo/index.html".to_string(), b"i".to_vec()),
            ("/srv/demo/sub/b.html".to_string(), b"b".to_vec()),
        ]
        .into_iter()
        .collect(),
    );
    (World::new(runs, Host::allowing()), files)
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

#[tokio::test]
async fn a_guide_is_mounted_with_its_page_and_its_author() {
    let (w, files) = guided_world();
    let guide = "\
- title: Start with option A
  text: The chosen snap.
  link: '#snap-a'
- title: Where it plugs
  link: '#/runs/r1/nodes/design'
- title: Before
  link: '#/runs/r1/nodes/design/artifacts/before'
- title: The code
  link: /runs/r1/review?file=src%2Fa.ts
- title: The other page
  link: sub/b.html#x
";
    let mount = w
        .mount_over(files)
        .execute(&agent(), guided(guide))
        .await
        .unwrap();
    let entries = mount.guide.expect("a guide");
    assert_eq!(entries.len(), 5);
    assert_eq!(entries[0].title, "Start with option A");
    assert_eq!(entries[0].text, "The chosen snap.");
    assert_eq!(entries[3].link, "/runs/r1/review?file=src%2Fa.ts");
    assert_eq!(mount.node_id.as_deref(), Some("n1"));
    let stored = w.pages.0.lock().unwrap()["demo"].clone();
    assert_eq!(stored.guide.unwrap().len(), 5);
}

#[tokio::test]
async fn an_invalid_guide_is_refused_with_its_reason_and_nothing_is_mounted() {
    for (guide, reason) in [
        ("not: a list", "guide is not a list"),
        (
            "- title: t\n  link: 'https://x'\n",
            "guide entry 1: link `https://x` leads nowhere",
        ),
        (
            "- title: t\n  link: '#/runs/r9'\n",
            "guide entry 1: Run `r9` not found",
        ),
        (
            "- title: t\n  link: '#a'\n- title: u\n  link: '#/runs/r1/nodes/ghost'\n",
            "guide entry 2: node `ghost` not found in Run `r1`",
        ),
        (
            "- title: t\n  link: missing.html\n",
            "guide entry 1: `missing.html` is not a file of the page",
        ),
        (
            "- title: t\n  link: /runs/r9/review\n",
            "guide entry 1: Run `r9` not found",
        ),
    ] {
        let (w, files) = guided_world();
        let err = w
            .mount_over(files)
            .execute(&agent(), guided(guide))
            .await
            .unwrap_err();
        match err {
            MountPageError::Invalid(MountError::Invalid(message)) => {
                assert!(
                    message.starts_with(reason),
                    "{message:?} should start with {reason:?}"
                )
            }
            other => panic!("expected a refusal for {guide:?}, got {other:?}"),
        }
        assert!(
            w.pages.0.lock().unwrap().is_empty(),
            "nothing mounted for {guide:?}"
        );
        assert!(w.runs.commands.lock().unwrap().is_empty());
    }
}

#[tokio::test]
async fn a_page_without_a_guide_has_none() {
    let w = World::allowing();
    let mount = w
        .mount()
        .execute(&Actor::LocalOperator, command("demo", "/srv/demo"))
        .await
        .unwrap();
    assert_eq!((mount.guide, mount.node_id), (None, None));
}
