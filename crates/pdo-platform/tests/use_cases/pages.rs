use std::path::PathBuf;
use std::sync::Arc;

use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use pdo_platform::application::mount_page::{MountPage, MountPageCommand, MountPageError};
use pdo_platform::application::page_journal::PageMountLock;
use pdo_platform::application::place_older_pages::PlaceOlderPages;
use pdo_platform::application::place_page_mount::PlacePageMount;
use pdo_platform::application::read_pages::{ReadPageError, ReadPages};
use pdo_platform::application::record_audit::RecordAudit;
use pdo_platform::application::release_run_pages::ReleaseRunPages;
use pdo_platform::application::unmount_page::{UnmountPage, UnmountPageError};
use pdo_platform::domain::audit::ActorHint;
use pdo_platform::domain::page_mount::{MountError, PageMount};

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
            Arc::new(self.place()),
        )
    }

    fn place(&self) -> PlacePageMount {
        PlacePageMount::new(self.pages.clone(), self.runs.clone())
    }

    fn unmount(&self) -> UnmountPage {
        UnmountPage::new(
            self.pages.clone(),
            self.runs.clone(),
            RecordAudit::new(self.audit.clone()),
            self.host.clone() as Arc<dyn ExtensionHost>,
            self.lock.clone(),
            Arc::new(self.place()),
        )
    }
}

fn command(name: &str, directory: &str) -> MountPageCommand {
    MountPageCommand {
        name: name.into(),
        directory: PathBuf::from(directory),
        guide: None,
        hint: ActorHint::Cli,
        project_id: None,
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

/// A world where the session `n1` of Run `r1` is live, `r1` in Projet `p1`.
fn world_in_p1() -> World {
    let runs = FakeRuns {
        live_sessions: vec![("r1".into(), "n1".into())],
        projects: [("r1".to_string(), vec!["p1".to_string()])]
            .into_iter()
            .collect(),
        ..FakeRuns::default()
    };
    World::new(runs, Host::allowing())
}

fn asked(w: &World, verb: Verb) -> Operation {
    w.host
        .asked
        .lock()
        .unwrap()
        .iter()
        .find(|o| o.verb == verb)
        .cloned()
        .unwrap()
}

#[tokio::test]
async fn an_agent_mount_is_asked_in_the_projet_of_its_run() {
    let w = world_in_p1();
    w.mount()
        .execute(&agent(), command("demo", "/srv/demo"))
        .await
        .unwrap();
    assert_eq!(asked(&w, Verb::Create).projects, ["p1"]);
}

#[tokio::test]
async fn a_mount_outside_any_run_is_asked_in_no_projet() {
    let w = world_in_p1();
    w.mount()
        .execute(&Actor::LocalOperator, command("demo", "/srv/demo"))
        .await
        .unwrap();
    assert!(asked(&w, Verb::Create).is_outside_projects());
}

#[tokio::test]
async fn the_unmount_of_a_runs_mount_is_asked_in_the_projet_of_that_run() {
    let w = world_in_p1();
    w.mount()
        .execute(&agent(), command("demo", "/srv/demo"))
        .await
        .unwrap();
    w.unmount()
        .execute(&Actor::LocalOperator, "demo", ActorHint::Cli)
        .await
        .unwrap();
    let delete = asked(&w, Verb::Delete);
    assert_eq!(delete.target.as_deref(), Some("demo"));
    assert_eq!(delete.projects, ["p1"]);
}

#[tokio::test]
async fn the_unmount_of_a_mount_outside_any_run_is_asked_in_no_projet() {
    let w = world_in_p1();
    w.mount()
        .execute(&Actor::LocalOperator, command("demo", "/srv/demo"))
        .await
        .unwrap();
    w.unmount()
        .execute(&agent(), "demo", ActorHint::Cli)
        .await
        .unwrap();
    assert!(asked(&w, Verb::Delete).is_outside_projects());
}

#[tokio::test]
async fn a_page_mount_question_is_placed_where_the_gesture_would_be() {
    let w = world_in_p1();
    w.mount()
        .execute(&agent(), command("demo", "/srv/demo"))
        .await
        .unwrap();
    let place = w.place();
    let create = Operation::new(ResourceKind::PageMount, Verb::Create);
    assert_eq!(
        place.execute(&agent(), create.clone()).await.projects,
        ["p1"]
    );
    assert!(place
        .execute(&Actor::LocalOperator, create)
        .await
        .is_outside_projects());
    let delete = Operation::new(ResourceKind::PageMount, Verb::Delete);
    assert_eq!(
        place
            .execute(&Actor::LocalOperator, delete.clone().on("demo"))
            .await
            .projects,
        ["p1"]
    );
    assert!(place
        .execute(&Actor::LocalOperator, delete.on("ghost"))
        .await
        .is_outside_projects());
    // An Operation that names its Projets, or is on another resource, is left alone.
    let named = Operation::new(ResourceKind::PageMount, Verb::Create).in_projects(["p9"]);
    assert_eq!(place.execute(&agent(), named).await.projects, ["p9"]);
    let other = Operation::new(ResourceKind::Project, Verb::Create);
    assert!(place.execute(&agent(), other).await.is_outside_projects());
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
    assert!(
        !w.host.asked().iter().any(|o| o.verb == Verb::Delete),
        "its owner unmounts it without asking"
    );
    assert_eq!(
        w.unmount().execute(&op, "demo", ActorHint::Ui).await,
        Err(UnmountPageError::NotFound("demo".into()))
    );
    let delete = asked(&w, Verb::Delete);
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
    let read = ReadPages::new(w.pages.clone(), Arc::new(files), w.host.clone());
    let op = Actor::LocalOperator;
    assert_eq!(
        read.file(&op, "demo", "a.html")
            .await
            .unwrap()
            .unwrap()
            .contents,
        b"a"
    );
    assert!(read
        .file(&op, "demo", "../demo/a.html")
        .await
        .unwrap()
        .is_none());
    assert!(read.file(&op, "nope", "a.html").await.unwrap().is_none());
    assert_eq!(read.list(&op).await.unwrap().len(), 1);
}

fn named(id: &str) -> Actor {
    Actor::Identified {
        namespace: "test".into(),
        id: id.into(),
    }
}

/// A world where Run `r1`, created by alice, is in Projet `p1`, and the
/// extension refuses `refused` to anyone but alice.
fn world_of_alice(refused: Verb) -> World {
    let runs = FakeRuns {
        live_sessions: vec![("r1".into(), "n1".into())],
        projects: [("r1".to_string(), vec!["p1".to_string()])]
            .into_iter()
            .collect(),
        creators: [("r1".to_string(), named("alice"))].into_iter().collect(),
        ..FakeRuns::default()
    };
    let host = Host::refusing_when(move |actor, operation| {
        operation.resource == ResourceKind::PageMount
            && operation.verb == refused
            && *actor != named("alice")
    });
    World::new(runs, host)
}

#[tokio::test]
async fn an_agent_mount_is_owned_by_who_created_its_run_in_its_projet() {
    let w = world_of_alice(Verb::Delete);
    let mount = w
        .mount()
        .execute(&agent(), command("demo", "/srv/demo"))
        .await
        .unwrap();
    assert_eq!(mount.owner, Some(named("alice")));
    assert_eq!(mount.project_id.as_deref(), Some("p1"));
    assert_eq!(w.pages.0.lock().unwrap()["demo"], mount);
    // A node names no other Projet than its Run's.
    let elsewhere = MountPageCommand {
        project_id: Some("p2".into()),
        ..command("other", "/srv/demo")
    };
    assert!(matches!(
        w.mount().execute(&agent(), elsewhere).await,
        Err(MountPageError::Invalid(MountError::Invalid(m))) if m.contains("Projet of its Run")
    ));
    let same = MountPageCommand {
        project_id: Some("p1".into()),
        ..command("same", "/srv/demo")
    };
    assert!(w.mount().execute(&agent(), same).await.is_ok());
}

#[tokio::test]
async fn a_person_mounts_in_the_projet_they_name_and_owns_the_page() {
    let w = world_in_p1();
    let bob = named("bob");
    let in_p2 = MountPageCommand {
        project_id: Some("p2".into()),
        ..command("demo", "/srv/demo")
    };
    let mount = w.mount().execute(&bob, in_p2).await.unwrap();
    assert_eq!(mount.owner, Some(bob.clone()));
    assert_eq!(mount.project_id.as_deref(), Some("p2"));
    assert_eq!(asked(&w, Verb::Create).projects, ["p2"]);
    let alone = w
        .mount()
        .execute(&bob, command("alone", "/srv/demo"))
        .await
        .unwrap();
    assert_eq!(alone.project_id, None);
}

#[tokio::test]
async fn a_page_is_read_in_its_projet_and_listed_only_to_who_may_read_it() {
    let w = world_of_alice(Verb::Read);
    w.mount()
        .execute(&agent(), command("demo", "/srv/demo"))
        .await
        .unwrap();
    let files = Files(
        [("/srv/demo/index.html".to_string(), b"i".to_vec())]
            .into_iter()
            .collect(),
    );
    let read = ReadPages::new(w.pages.clone(), Arc::new(files), w.host.clone());

    let alice = named("alice");
    assert_eq!(read.list(&alice).await.unwrap().len(), 1);
    assert!(read.get(&alice, "demo").await.unwrap().is_some());
    assert!(read
        .file(&alice, "demo", "index.html")
        .await
        .unwrap()
        .is_some());
    let question = w
        .host
        .asked()
        .into_iter()
        .rfind(|o| o.verb == Verb::Read)
        .unwrap();
    assert_eq!(question.target.as_deref(), Some("demo"));
    assert_eq!(question.projects, ["p1"]);

    let olga = named("olga");
    assert!(read.list(&olga).await.unwrap().is_empty());
    assert!(matches!(
        read.get(&olga, "demo").await,
        Err(ReadPageError::Refused(denied)) if denied.sentence() == "You can't."
    ));
    assert!(matches!(
        read.file(&olga, "demo", "index.html").await,
        Err(ReadPageError::Refused(_))
    ));
    assert_eq!(
        read.get(&olga, "ghost").await,
        Ok(None),
        "unknown: no question"
    );
}

#[tokio::test]
async fn its_owner_unmounts_a_page_and_anyone_else_is_judged_in_its_projet() {
    let w = world_of_alice(Verb::Delete);
    for name in ["mine", "yours"] {
        w.mount()
            .execute(&agent(), command(name, "/srv/demo"))
            .await
            .unwrap();
    }
    // Bob may not delete pages in p1: refused, the page stays.
    let refused = w
        .unmount()
        .execute(&named("bob"), "mine", ActorHint::Ui)
        .await;
    assert!(matches!(refused, Err(UnmountPageError::Refused(_))));
    assert!(w.pages.0.lock().unwrap().contains_key("mine"));
    assert_eq!(asked(&w, Verb::Delete).projects, ["p1"]);
    // Alice owns it: she unmounts it without being asked.
    let asked_before = w.host.asked().len();
    w.unmount()
        .execute(&named("alice"), "mine", ActorHint::Ui)
        .await
        .unwrap();
    assert_eq!(w.host.asked().len(), asked_before, "the owner is not asked");
    let delete = Operation::new(ResourceKind::PageMount, Verb::Delete).on("yours");
    assert!(w.place().owned_by(&named("alice"), &delete).await);
    assert!(!w.place().owned_by(&named("bob"), &delete).await);
    // An agent of its Run is no owner: it is asked, in the page's Projet,
    // and refused like anyone but alice.
    let refused = w.unmount().execute(&agent(), "yours", ActorHint::Cli).await;
    assert!(matches!(refused, Err(UnmountPageError::Refused(_))));
    assert_eq!(w.host.asked().len(), asked_before + 1);
    assert_eq!(w.host.asked().last().unwrap().projects, ["p1"]);
    let left: Vec<String> = w.pages.0.lock().unwrap().keys().cloned().collect();
    assert_eq!(left, ["yours"]);
}

#[tokio::test]
async fn older_pages_take_the_projet_and_the_creator_of_their_run() {
    let w = world_of_alice(Verb::Delete);
    let older = |name: &str, run_id: Option<&str>| PageMount {
        name: name.into(),
        directory: "/srv/demo".into(),
        run_id: run_id.map(str::to_string),
        node_id: None,
        created_at: "then".into(),
        guide: None,
        owner: None,
        project_id: None,
    };
    {
        let mut pages = w.pages.0.lock().unwrap();
        for mount in [
            older("of-r1", Some("r1")),
            older("of-a-gone-run", Some("r9")),
            older("alone", None),
        ] {
            pages.insert(mount.name.clone(), mount);
        }
        let mut placed = older("placed", Some("r1"));
        placed.project_id = Some("p7".into());
        pages.insert("placed".into(), placed);
    }
    let place = PlaceOlderPages::new(w.pages.clone(), w.runs.clone());
    assert_eq!(place.execute().await.unwrap(), 1);
    let pages = w.pages.0.lock().unwrap().clone();
    assert_eq!(pages["of-r1"].owner, Some(named("alice")));
    assert_eq!(pages["of-r1"].project_id.as_deref(), Some("p1"));
    for unplaced in ["of-a-gone-run", "alone"] {
        assert_eq!(pages[unplaced].owner, None, "{unplaced}");
        assert_eq!(pages[unplaced].project_id, None, "{unplaced}");
    }
    assert_eq!(
        pages["placed"].project_id.as_deref(),
        Some("p7"),
        "left alone"
    );
    assert_eq!(pages["placed"].owner, None);
    drop(pages);
    assert_eq!(place.execute().await.unwrap(), 0, "once");
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
