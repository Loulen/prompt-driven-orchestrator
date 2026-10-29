//! The Sessions hébergées (ADR-0091): open, terminate, the exit of their
//! process, a restart of the daemon, the sweep and the Cap, against tmux in
//! memory.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use pdo_extension_api::{
    EndReason, HostedConfig, HostedFolder, HostedSessionChanged, HostedState, OpenSession,
    SessionKind,
};
use pdo_kernel::extension_host::NodeContribution;
use pdo_kernel::{Actor, ResourceKind, Verb};
use pdo_session::application::assistant::FocusCell;
use pdo_session::application::hosted::{Hosted, HostedPorts, HostedStore};
use pdo_session::application::session_ledger::SessionLedger;
use pdo_session::application::sweep::{ReaperGauge, SweepOrphans};
use pdo_session::domain::reaper::SweepTtls;

use crate::fakes::{Cap, FakeTmux, Harnesses, Host, HostedRecords, Owners, Places};

const EXT: &str = "fake";

struct World {
    hosted: Arc<Hosted>,
    tmux: Arc<FakeTmux>,
    host: Arc<Host>,
    ledger: Arc<SessionLedger>,
    store: Arc<HostedRecords>,
    cap: Arc<Cap>,
    home: tempfile::TempDir,
    repo: tempfile::TempDir,
}

impl World {
    fn new() -> Self {
        Self::over(
            Arc::new(HostedRecords::default()),
            Arc::new(FakeTmux::default()),
            Host::allowing(),
        )
    }

    fn over(store: Arc<HostedRecords>, tmux: Arc<FakeTmux>, host: Arc<Host>) -> Self {
        let home = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        let ledger = Arc::new(SessionLedger::new(tmux.clone(), host.clone()));
        let cap = Arc::new(Cap::default());
        let hosted = Arc::new(Hosted::new(
            HostedPorts {
                store: store.clone(),
                places: Arc::new(Places {
                    home: Some(home.path().to_path_buf()),
                    repo: Some(repo.path().to_path_buf()),
                }),
                harness: Arc::new(Harnesses),
                admission: cap.clone(),
            },
            tmux.clone(),
            host.clone(),
            ledger.clone(),
            6172,
            None,
        ));
        Self {
            hosted,
            tmux,
            host,
            ledger,
            store,
            cap,
            home,
            repo,
        }
    }

    /// The same records and tmux, under a daemon that just started.
    fn restarted(&self) -> Self {
        let mut world = Self::over(self.store.clone(), self.tmux.clone(), self.host.clone());
        world.home = tempfile::tempdir().unwrap();
        world
    }

    fn script_of(&self, id: &str) -> (PathBuf, String) {
        let created = self.tmux.created.lock().unwrap();
        let (_, dir, script) = created.iter().find(|(name, _, _)| name == id).unwrap();
        (dir.clone(), script.clone())
    }

    fn changes(&self) -> Vec<(HostedState, Option<EndReason>)> {
        self.host
            .events()
            .iter()
            .filter_map(|(_, event)| HostedSessionChanged::from_event(event))
            .map(|change| (change.session.state, change.session.end_reason))
            .collect()
    }
}

fn alice() -> Actor {
    Actor::Identified {
        namespace: "test".into(),
        id: "alice".into(),
    }
}

fn in_folder(open: OpenSession, dir: &Path) -> OpenSession {
    open.in_folder(HostedFolder::Path {
        path: dir.to_string_lossy().into_owned(),
    })
}

#[tokio::test]
async fn an_agent_opens_running_without_a_byte_in_its_folder_nor_a_run() {
    let w = World::new();
    *w.host.contribution.lock().unwrap() = NodeContribution {
        env: [("FIXTURE_HELLO".to_string(), "monde".to_string())].into(),
        preamble: Some("Ligne de fixture".into()),
    };
    let folder = tempfile::tempdir().unwrap();
    let open = in_folder(OpenSession::agent("n1"), folder.path())
        .in_project("p1")
        .with_config(HostedConfig::default().with_prompt("Bonjour"));

    let session = w.hosted.open(EXT, &alice(), open).await.unwrap();

    assert_eq!(session.state, HostedState::Running);
    assert_eq!(session.key, "n1");
    assert_eq!(session.extension, EXT);
    assert_eq!(session.started_by, Some(alice()));
    assert!(session.id.starts_with("pdo-hosted-"), "{}", session.id);
    assert_eq!(
        std::fs::read_dir(folder.path()).unwrap().count(),
        0,
        "the core writes nothing into the session's folder"
    );
    let (dir, script) = w.script_of(&session.id);
    assert_eq!(dir, folder.path());
    assert!(script.contains("PDO_SESSION_ID"), "{script}");
    assert!(script.contains(&session.id), "{script}");
    assert!(script.contains("PDO_DAEMON_URL"), "{script}");
    assert!(!script.contains("export PDO_RUN_ID"), "{script}");
    assert!(!script.contains("export PDO_NODE_ID"), "{script}");
    assert!(script.contains("FIXTURE_HELLO"), "{script}");
    let primer = w.store.state_dir(&session.id).join("primer.md");
    assert_eq!(
        std::fs::read_to_string(primer).unwrap(),
        "Ligne de fixture\n\nBonjour",
        "the extensions' text, then the prompt, and nothing of the core"
    );

    let launches = w.host.launches.lock().unwrap().clone();
    assert_eq!(launches.len(), 1);
    assert_eq!(launches[0].session.kind, SessionKind::Hosted);
    assert_eq!(launches[0].session.extension.as_deref(), Some(EXT));
    assert_eq!(launches[0].session.key.as_deref(), Some("n1"));
    assert_eq!(launches[0].started_by, Some(alice()));
    assert_eq!(launches[0].projects, ["p1"]);

    let asked = w.host.asked();
    assert_eq!(asked[0].resource, ResourceKind::Session);
    assert_eq!(asked[0].verb, Verb::Action("open".into()));
    assert_eq!(asked[0].projects, ["p1"]);
    assert_eq!(
        w.changes(),
        [(HostedState::Starting, None), (HostedState::Running, None)]
    );
    let events = w.host.events();
    assert!(
        events
            .iter()
            .filter(|(_, e)| e["type"] == "hosted_session_changed")
            .all(|(on_bus, _)| *on_bus),
        "the changes ride the bus, hence /ws"
    );
    let started = events
        .iter()
        .find(|(_, e)| e["type"] == "session_started")
        .unwrap();
    assert_eq!(started.1["session"]["kind"], "hosted");
    assert_eq!(started.1["session"]["key"], "n1");
    assert_eq!(started.1["actor"]["id"], "alice");
    assert_eq!(w.ledger.hosted_live(), 1);
    assert_eq!(w.hosted.registry().list(EXT), vec![session]);
}

#[tokio::test]
async fn a_shell_gets_the_contribution_too_and_runs_bash_in_the_home_folder() {
    let w = World::new();
    *w.host.contribution.lock().unwrap() = NodeContribution {
        env: [("PDO_TOKEN".to_string(), "t-alice".to_string())].into(),
        preamble: None,
    };
    let session = w
        .hosted
        .open(EXT, &alice(), OpenSession::shell("sh"))
        .await
        .unwrap();
    let (dir, script) = w.script_of(&session.id);
    assert_eq!(dir, w.home.path());
    assert!(script.contains("bash -i"), "{script}");
    assert!(script.contains("t-alice"), "{script}");
    assert!(!w.store.state_dir(&session.id).join("primer.md").exists());
    assert_eq!(w.host.launches.lock().unwrap()[0].started_by, Some(alice()));
}

#[tokio::test]
async fn the_primary_repository_of_the_projet_is_a_folder_and_nothing_else_resolves_silently() {
    let w = World::new();
    let session = w
        .hosted
        .open(
            EXT,
            &alice(),
            OpenSession::shell("r")
                .in_project("p1")
                .in_folder(HostedFolder::ProjectRepo),
        )
        .await
        .unwrap();
    assert_eq!(session.cwd, w.repo.path().to_string_lossy());

    for (open, words) in [
        (
            OpenSession::shell("a").in_folder(HostedFolder::ProjectRepo),
            "needs the session's Projet",
        ),
        (
            OpenSession::shell("b")
                .in_project("p9")
                .in_folder(HostedFolder::ProjectRepo),
            "has no primary repository",
        ),
        (
            OpenSession::shell("c").in_folder(HostedFolder::Path {
                path: "relative".into(),
            }),
            "not an absolute path",
        ),
        (
            in_folder(OpenSession::shell("d"), Path::new("/nowhere/at/all")),
            "does not exist",
        ),
        (
            OpenSession::agent("e").with_config(HostedConfig::default().on_harness("nope")),
            "unknown harness `nope`",
        ),
        (OpenSession::shell(" "), "needs a key"),
    ] {
        let refused = w.hosted.open(EXT, &alice(), open).await.unwrap_err();
        assert_eq!(refused.status, 400, "{refused}");
        assert!(refused.message.contains(words), "{refused}");
    }
    assert_eq!(w.tmux.created.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn a_refused_open_opens_nothing() {
    let w = World::over(
        Arc::new(HostedRecords::default()),
        Arc::new(FakeTmux::default()),
        Host::refusing(),
    );
    let refused = w
        .hosted
        .open(EXT, &alice(), OpenSession::shell("s"))
        .await
        .unwrap_err();
    assert_eq!(refused.status, 403);
    assert!(refused.message.contains("admins only"), "{refused}");
    assert!(w.tmux.created.lock().unwrap().is_empty());
    assert!(w.hosted.registry().all().is_empty());
    assert!(w.host.events().is_empty());
}

#[tokio::test]
async fn a_full_cap_refuses_at_once_and_a_taken_key_is_refused() {
    let w = World::new();
    w.hosted
        .open(EXT, &alice(), OpenSession::shell("s"))
        .await
        .unwrap();
    let taken = w
        .hosted
        .open(EXT, &alice(), OpenSession::shell("s"))
        .await
        .unwrap_err();
    assert_eq!(taken.status, 409);
    assert!(taken.message.contains("exists already"), "{taken}");
    let other = w
        .hosted
        .open("other", &alice(), OpenSession::shell("s"))
        .await;
    assert!(other.is_ok(), "a key is the extension's own");

    *w.cap.full.lock().unwrap() = true;
    let refused = w
        .hosted
        .open(EXT, &alice(), OpenSession::agent("more"))
        .await
        .unwrap_err();
    assert_eq!(refused.status, 409);
    assert!(
        refused.message.contains("session cap reached (2/2"),
        "{refused}"
    );
    assert_eq!(w.tmux.created.lock().unwrap().len(), 2);
    assert_eq!(w.hosted.registry().all().len(), 2);
}

#[tokio::test]
async fn a_launch_that_fails_leaves_no_record_and_gives_its_slot_back() {
    let w = World::new();
    *w.tmux.fail_next_spawn.lock().unwrap() = Some("no server".into());
    let refused = w
        .hosted
        .open(EXT, &alice(), OpenSession::shell("s"))
        .await
        .unwrap_err();
    assert_eq!(refused.status, 500);
    assert!(refused.message.contains("no server"), "{refused}");
    assert!(w.hosted.registry().all().is_empty());
    assert_eq!(w.ledger.hosted_live(), 0);
    assert_eq!(*w.cap.released.lock().unwrap(), 1);
    assert_eq!(
        w.changes(),
        [
            (HostedState::Starting, None),
            (HostedState::Ended, Some(EndReason::Exited))
        ]
    );
}

#[tokio::test]
async fn terminate_ends_it_kills_its_session_and_keeps_its_record() {
    let w = World::new();
    let session = w
        .hosted
        .open(EXT, &alice(), OpenSession::shell("s").in_project("p1"))
        .await
        .unwrap();
    let ended = w.hosted.terminate(EXT, &alice(), "s").await.unwrap();
    assert_eq!(ended.state, HostedState::Ended);
    assert_eq!(ended.end_reason, Some(EndReason::Terminated));
    assert!(!w.tmux.has(&session.id));
    assert_eq!(w.ledger.hosted_live(), 0);
    assert_eq!(*w.cap.released.lock().unwrap(), 1);
    let asked = w.host.asked();
    let terminate = asked.last().unwrap();
    assert_eq!(terminate.verb, Verb::Action("terminate".into()));
    assert_eq!(terminate.target.as_deref(), Some(session.id.as_str()));
    assert_eq!(terminate.projects, ["p1"]);
    assert_eq!(
        w.changes().last(),
        Some(&(HostedState::Ended, Some(EndReason::Terminated)))
    );
    assert_eq!(w.hosted.registry().find(EXT, "s").unwrap().session, ended);
    assert_eq!(
        w.store.load()[0].session.state,
        HostedState::Ended,
        "stored"
    );

    let again = w.hosted.terminate(EXT, &alice(), "s").await.unwrap();
    assert_eq!(again, ended, "an ended session is answered as it is");
    assert_eq!(
        w.hosted
            .terminate(EXT, &alice(), "never")
            .await
            .unwrap_err()
            .status,
        404
    );
}

#[tokio::test]
async fn a_refused_terminate_leaves_it_running() {
    let store = Arc::new(HostedRecords::default());
    let tmux = Arc::new(FakeTmux::default());
    let w = World::over(store.clone(), tmux.clone(), Host::allowing());
    let session = w
        .hosted
        .open(EXT, &alice(), OpenSession::shell("s"))
        .await
        .unwrap();
    let refusing = World::over(store, tmux.clone(), Host::refusing());
    let refused = refusing
        .hosted
        .terminate(EXT, &alice(), "s")
        .await
        .unwrap_err();
    assert_eq!(refused.status, 403);
    assert!(tmux.has(&session.id));
}

#[tokio::test]
async fn a_session_whose_process_exits_ends_exited() {
    let w = World::new();
    let session = w
        .hosted
        .open(EXT, &alice(), OpenSession::agent("a"))
        .await
        .unwrap();
    w.hosted.reconcile();
    assert_eq!(
        w.hosted.registry().find(EXT, "a").unwrap().session.state,
        HostedState::Running,
        "alive: nothing changes"
    );
    w.tmux.sessions.lock().unwrap().remove(&session.id);
    w.hosted.reconcile();
    let ended = w.hosted.registry().find(EXT, "a").unwrap().session;
    assert_eq!(ended.state, HostedState::Ended);
    assert_eq!(ended.end_reason, Some(EndReason::Exited));
    assert_eq!(*w.cap.released.lock().unwrap(), 1);
    w.hosted.reconcile();
    assert_eq!(*w.cap.released.lock().unwrap(), 1, "ended once");
}

#[tokio::test]
async fn a_restart_takes_back_the_live_sessions_with_their_starter() {
    let w = World::new();
    let alive = w
        .hosted
        .open(EXT, &alice(), OpenSession::agent("alive"))
        .await
        .unwrap();
    let gone = w
        .hosted
        .open(EXT, &alice(), OpenSession::shell("gone"))
        .await
        .unwrap();
    w.tmux.sessions.lock().unwrap().remove(&gone.id);

    let after = w.restarted();
    assert_eq!(after.ledger.hosted_live(), 2, "counted as soon as loaded");
    after.hosted.restore();
    let alive_now = after.hosted.registry().find(EXT, "alive").unwrap().session;
    assert_eq!(alive_now.state, HostedState::Running);
    assert_eq!(alive_now.started_by, Some(alice()));
    assert_eq!(after.ledger.started_by(&alive.id), Some(alice()));
    let gone_now = after.hosted.registry().find(EXT, "gone").unwrap().session;
    assert_eq!(gone_now.state, HostedState::Ended);
    assert_eq!(gone_now.end_reason, Some(EndReason::Exited));
    assert_eq!(after.ledger.hosted_live(), 1);
}

#[tokio::test]
async fn the_sweep_keeps_a_recorded_session_and_kills_any_other_of_the_prefix() {
    let w = World::new();
    let live = w
        .hosted
        .open(EXT, &alice(), OpenSession::agent("live"))
        .await
        .unwrap();
    let ended = w
        .hosted
        .open(EXT, &alice(), OpenSession::shell("ended"))
        .await
        .unwrap();
    w.hosted.terminate(EXT, &alice(), "ended").await.unwrap();
    // Still in tmux though ended, and one nobody recorded.
    for name in [ended.id.as_str(), "pdo-hosted-stray"] {
        w.tmux
            .sessions
            .lock()
            .unwrap()
            .insert(name.to_string(), String::new());
    }
    let sweep = SweepOrphans::new(
        w.tmux.clone(),
        Arc::new(Owners {
            facts: Default::default(),
            unreadable: false,
            ttls: SweepTtls {
                node_run: std::time::Duration::ZERO,
                libassist_idle: std::time::Duration::ZERO,
            },
            asked: Default::default(),
        }),
        Arc::new(FocusCell::default()),
        Arc::new(ReaperGauge::default()),
    )
    .with_hosted(w.hosted.registry().clone());
    let tally = sweep
        .run_with(SweepTtls {
            node_run: std::time::Duration::ZERO,
            libassist_idle: std::time::Duration::ZERO,
        })
        .await
        .unwrap();
    assert_eq!(tally.killed, 2);
    assert!(
        w.tmux.has(&live.id),
        "a recorded live session is never swept"
    );
    assert!(!w.tmux.has(&ended.id));
    assert!(!w.tmux.has("pdo-hosted-stray"));
}

#[tokio::test]
async fn terminated_while_it_is_launched_it_stays_ended_and_its_process_is_killed() {
    use futures_util::FutureExt;
    let w = World::new();
    let hosted = w.hosted.clone();
    *w.tmux.on_next_spawn.lock().unwrap() = Some(Box::new(move || {
        let ended = hosted
            .terminate(EXT, &alice(), "s")
            .now_or_never()
            .unwrap()
            .unwrap();
        assert_eq!(ended.end_reason, Some(EndReason::Terminated));
    }));
    let session = w
        .hosted
        .open(EXT, &alice(), OpenSession::shell("s"))
        .await
        .unwrap();
    assert_eq!(session.state, HostedState::Ended);
    assert_eq!(session.end_reason, Some(EndReason::Terminated));
    assert!(!w.tmux.has(&session.id), "its process is killed");
    assert_eq!(w.ledger.hosted_live(), 0);
    assert_eq!(
        *w.cap.released.lock().unwrap(),
        1,
        "its slot given back once"
    );
    assert_eq!(
        w.changes(),
        [
            (HostedState::Starting, None),
            (HostedState::Ended, Some(EndReason::Terminated))
        ],
        "never `running` after `ended`"
    );
}

#[tokio::test]
async fn a_session_that_exited_keeps_its_reason_when_it_is_terminated() {
    let w = World::new();
    let session = w
        .hosted
        .open(EXT, &alice(), OpenSession::shell("s"))
        .await
        .unwrap();
    w.tmux.sessions.lock().unwrap().remove(&session.id);
    w.hosted.reconcile();
    let ended = w.hosted.terminate(EXT, &alice(), "s").await.unwrap();
    assert_eq!(ended.end_reason, Some(EndReason::Exited));
    assert_eq!(*w.cap.released.lock().unwrap(), 1);
    let ends = w
        .changes()
        .into_iter()
        .filter(|(state, _)| *state == HostedState::Ended)
        .count();
    assert_eq!(ends, 1, "ended once");
}
