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
use pdo_session::application::declared_wait::{LiftWait, TypedInput};
use pdo_session::application::hosted::{Hosted, HostedPorts, HostedStore, HostedWaitError};
use pdo_session::application::session_ledger::SessionLedger;
use pdo_session::application::sweep::{ReaperGauge, SweepOrphans};
use pdo_session::domain::reaper::SweepTtls;

use crate::fakes::{Cap, FakeTmux, Harnesses, Host, HostedRecords, Owners, Places};

const EXT: &str = "fake";

struct World {
    hosted: Arc<Hosted>,
    typed: Arc<TypedInput>,
    tmux: Arc<FakeTmux>,
    host: Arc<Host>,
    ledger: Arc<SessionLedger>,
    store: Arc<HostedRecords>,
    cap: Arc<Cap>,
    harnesses: Arc<Harnesses>,
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
        let typed = Arc::new(TypedInput::default());
        let harnesses = Arc::new(Harnesses::default());
        let hosted = Arc::new(
            Hosted::new(
                HostedPorts {
                    store: store.clone(),
                    places: Arc::new(Places {
                        home: Some(home.path().to_path_buf()),
                        repo: Some(repo.path().to_path_buf()),
                    }),
                    harness: harnesses.clone(),
                    admission: cap.clone(),
                },
                tmux.clone(),
                host.clone(),
                ledger.clone(),
                6172,
                None,
            )
            .with_typed(typed.clone()),
        );
        Self {
            hosted,
            typed,
            tmux,
            host,
            ledger,
            store,
            cap,
            harnesses,
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

    /// The script of the last launch of `id`.
    fn last_script_of(&self, id: &str) -> String {
        let created = self.tmux.created.lock().unwrap();
        let (_, _, script) = created
            .iter()
            .rev()
            .find(|(name, _, _)| name == id)
            .unwrap();
        script.clone()
    }

    fn launches_of(&self, id: &str) -> usize {
        let created = self.tmux.created.lock().unwrap();
        created.iter().filter(|(name, _, _)| name == id).count()
    }

    /// The Terminals' Enter rule over these Sessions hébergées.
    fn lift(&self) -> LiftWait {
        LiftWait::new(
            Arc::new(crate::fakes::Waits::default()),
            self.typed.clone(),
            self.host.clone(),
        )
        .with_hosted(self.hosted.clone())
    }

    /// Open `open` and end it (terminated).
    async fn ended(&self, open: OpenSession) -> pdo_extension_api::HostedSession {
        let key = open.key.clone();
        self.hosted.open(EXT, &alice(), open).await.unwrap();
        self.hosted.terminate(EXT, &alice(), &key).await.unwrap()
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

fn bob() -> Actor {
    Actor::Identified {
        namespace: "test".into(),
        id: "bob".into(),
    }
}

#[tokio::test]
async fn a_relaunched_agent_resumes_its_conversation_with_who_relaunched_it() {
    let w = World::new();
    let open = OpenSession::agent("a")
        .in_project("p1")
        .with_config(HostedConfig::default().with_prompt("Bonjour"));
    let ended = w.ended(open).await;
    let conversation = w.store.load()[0].conversation_id.clone().unwrap();
    assert!(w.last_script_of(&ended.id).contains(&conversation));
    std::fs::remove_file(w.store.state_dir(&ended.id).join("primer.md")).unwrap();

    let session = w.hosted.relaunch(EXT, &bob(), "a").await.unwrap();

    assert_eq!(session.state, HostedState::Running);
    assert_eq!(session.end_reason, None);
    assert_eq!(session.started_by, Some(bob()));
    assert_eq!(session.id, ended.id, "the same session");
    let script = w.last_script_of(&session.id);
    assert!(
        script.contains(&format!("--resume '\\''{conversation}'\\''")),
        "{script}"
    );
    assert!(!script.contains("--session-id"), "{script}");
    assert!(
        !script.contains("primer.md"),
        "nothing is read again: {script}"
    );
    assert!(
        !w.store.state_dir(&session.id).join("primer.md").exists(),
        "no primer written for a resumed conversation"
    );
    assert_eq!(w.store.load()[0].conversation_id, Some(conversation));
    let relaunch = w.host.asked().last().unwrap().clone();
    assert_eq!(relaunch.verb, Verb::Action("relaunch".into()));
    assert_eq!(relaunch.target.as_deref(), Some(session.id.as_str()));
    assert_eq!(relaunch.projects, ["p1"]);
    let launches = w.host.launches.lock().unwrap().clone();
    assert_eq!(
        launches.last().unwrap().started_by,
        Some(bob()),
        "the Contribution au lancement is asked for who relaunches"
    );
    assert_eq!(
        w.changes()[w.changes().len() - 2..],
        [(HostedState::Starting, None), (HostedState::Running, None)]
    );
    assert_eq!(w.ledger.started_by(&session.id), Some(bob()));
    assert_eq!(w.ledger.hosted_live(), 1);
}

#[tokio::test]
async fn an_agent_nobody_spoke_to_relaunches_on_a_new_conversation() {
    let w = World::new();
    let ended = w
        .ended(OpenSession::agent("a").with_config(HostedConfig::default().with_prompt("Un")))
        .await;
    let before = w.store.load()[0].conversation_id.clone().unwrap();
    w.harnesses.unwritten.lock().unwrap().push(before.clone());
    w.hosted.relaunch(EXT, &alice(), "a").await.unwrap();
    let after = w.store.load()[0].conversation_id.clone().unwrap();
    assert_ne!(after, before);
    let script = w.last_script_of(&ended.id);
    assert!(!script.contains("--resume"), "{script}");
    assert!(script.contains(&after), "{script}");
    assert!(script.contains("primer.md"), "its prompt is read: {script}");
}

#[tokio::test]
async fn a_relaunched_shell_is_new_and_only_an_ended_session_relaunches() {
    let w = World::new();
    let shell = w
        .hosted
        .open(EXT, &alice(), OpenSession::shell("s"))
        .await
        .unwrap();
    let refused = w.hosted.relaunch(EXT, &alice(), "s").await.unwrap_err();
    assert_eq!(refused.status, 409, "{refused}");
    assert!(refused.message.contains("not ended"), "{refused}");
    assert_eq!(
        w.hosted
            .relaunch(EXT, &alice(), "never")
            .await
            .unwrap_err()
            .status,
        404
    );

    w.hosted.terminate(EXT, &alice(), "s").await.unwrap();
    let again = w.hosted.relaunch(EXT, &alice(), "s").await.unwrap();
    assert_eq!(again.state, HostedState::Running);
    assert_eq!(w.launches_of(&shell.id), 2);
    assert!(w.last_script_of(&shell.id).contains("bash -i"));
    assert!(w.tmux.has(&shell.id));
}

#[tokio::test]
async fn a_full_cap_or_a_refused_operation_relaunches_nothing() {
    let store = Arc::new(HostedRecords::default());
    let tmux = Arc::new(FakeTmux::default());
    let w = World::over(store.clone(), tmux.clone(), Host::allowing());
    let ended = w.ended(OpenSession::agent("a")).await;

    *w.cap.full.lock().unwrap() = true;
    let refused = w.hosted.relaunch(EXT, &alice(), "a").await.unwrap_err();
    assert_eq!(refused.status, 409);
    assert!(
        refused.message.contains("session cap reached (2/2"),
        "{refused}"
    );
    assert!(refused.message.contains("relaunched"), "{refused}");
    assert_eq!(w.launches_of(&ended.id), 1);
    assert_eq!(
        w.hosted.registry().find(EXT, "a").unwrap().session.state,
        HostedState::Ended
    );

    let refusing = World::over(store, tmux, Host::refusing());
    let refused = refusing
        .hosted
        .relaunch(EXT, &alice(), "a")
        .await
        .unwrap_err();
    assert_eq!(refused.status, 403);
    assert_eq!(refusing.launches_of(&ended.id), 1);
}

#[tokio::test]
async fn a_configuration_applies_at_the_next_relaunch_and_not_before() {
    let w = World::new();
    let open = OpenSession::agent("a").with_config(HostedConfig::default().with_prompt("Un"));
    let session = w.hosted.open(EXT, &alice(), open).await.unwrap();
    let first = w.last_script_of(&session.id);

    let configured = w
        .hosted
        .configure(
            EXT,
            &alice(),
            "a",
            HostedConfig::default()
                .with_model("opus")
                .with_effort("high")
                .with_prompt("Deux"),
        )
        .await
        .unwrap();
    assert_eq!(configured.config.model.as_deref(), Some("opus"));
    assert_eq!(configured.state, HostedState::Running, "still running");
    assert_eq!(w.launches_of(&session.id), 1, "not relaunched");
    assert_eq!(w.last_script_of(&session.id), first);
    assert_eq!(
        w.host.asked().last().unwrap().verb,
        Verb::Action("configure".into())
    );
    assert_eq!(
        w.hosted
            .registry()
            .find(EXT, "a")
            .unwrap()
            .session
            .config
            .prompt
            .as_deref(),
        Some("Deux"),
        "recorded"
    );
    assert_eq!(
        w.changes().last().map(|c| c.0),
        Some(HostedState::Running),
        "the change is published"
    );

    w.hosted.terminate(EXT, &alice(), "a").await.unwrap();
    w.hosted.relaunch(EXT, &alice(), "a").await.unwrap();
    let script = w.last_script_of(&session.id);
    assert!(script.contains("--resume"), "{script}");
    assert!(script.contains("opus"), "the new model: {script}");
    assert!(script.contains("high"), "the new effort: {script}");
}

#[tokio::test]
async fn another_harness_starts_a_new_conversation_with_the_new_prompt() {
    let w = World::new();
    let ended = w.ended(OpenSession::agent("a")).await;
    let before = w.store.load()[0].conversation_id.clone().unwrap();
    // Its conversation ran on another harness than the one it relaunches on.
    w.hosted.registry().update(&ended.id, |r| {
        r.harness = Some("elsewhere".into());
        true
    });
    w.hosted
        .configure(
            EXT,
            &alice(),
            "a",
            HostedConfig::default()
                .on_harness("claude")
                .with_prompt("Deux"),
        )
        .await
        .unwrap();
    w.hosted.relaunch(EXT, &alice(), "a").await.unwrap();
    let after = w.store.load()[0].conversation_id.clone().unwrap();
    assert_ne!(after, before, "a new conversation");
    let script = w.last_script_of(&ended.id);
    assert!(script.contains("--session-id"), "{script}");
    assert!(script.contains(&after), "{script}");
    assert!(!script.contains("--resume"), "{script}");
    assert_eq!(
        std::fs::read_to_string(w.store.state_dir(&ended.id).join("primer.md")).unwrap(),
        "Deux"
    );
    assert_eq!(w.store.load()[0].harness.as_deref(), Some("claude"));
}

#[tokio::test]
async fn a_shell_has_no_configuration_and_an_unknown_harness_is_refused() {
    let w = World::new();
    w.hosted
        .open(EXT, &alice(), OpenSession::shell("s"))
        .await
        .unwrap();
    let refused = w
        .hosted
        .configure(EXT, &alice(), "s", HostedConfig::default().with_model("x"))
        .await
        .unwrap_err();
    assert_eq!(refused.status, 400, "{refused}");
    w.hosted
        .open(EXT, &alice(), OpenSession::agent("a"))
        .await
        .unwrap();
    let refused = w
        .hosted
        .configure(
            EXT,
            &alice(),
            "a",
            HostedConfig::default().on_harness("nope"),
        )
        .await
        .unwrap_err();
    assert_eq!(refused.status, 400, "{refused}");
    assert!(refused.message.contains("unknown harness"), "{refused}");
    assert_eq!(
        w.hosted.registry().find(EXT, "a").unwrap().session.config,
        HostedConfig::default()
    );
}

#[tokio::test]
async fn a_text_is_laid_in_the_input_without_enter_and_the_gesture_is_an_operation() {
    let w = World::new();
    let session = w
        .hosted
        .open(EXT, &alice(), OpenSession::agent("a").in_project("p1"))
        .await
        .unwrap();
    let answered = w
        .hosted
        .type_text(EXT, &alice(), "a", "bonjour")
        .await
        .unwrap();
    assert_eq!(answered.id, session.id);
    assert_eq!(
        *w.tmux.pasted_without_enter.lock().unwrap(),
        [(session.id.clone(), "bonjour".to_string())]
    );
    assert!(w.tmux.typed.lock().unwrap().is_empty(), "no Enter");
    let typed = w.host.asked().last().unwrap().clone();
    assert_eq!(typed.verb, Verb::Action("type".into()));
    assert_eq!(typed.projects, ["p1"]);

    assert_eq!(
        w.hosted
            .type_text(EXT, &alice(), "a", "")
            .await
            .unwrap_err()
            .status,
        400
    );
    *w.tmux.refuse_paste.lock().unwrap() = true;
    assert_eq!(
        w.hosted
            .type_text(EXT, &alice(), "a", "x")
            .await
            .unwrap_err()
            .status,
        409
    );
    w.hosted.terminate(EXT, &alice(), "a").await.unwrap();
    let refused = w
        .hosted
        .type_text(EXT, &alice(), "a", "x")
        .await
        .unwrap_err();
    assert_eq!(refused.status, 409);
    assert!(refused.message.contains("not running"), "{refused}");
}

#[tokio::test]
async fn a_refused_text_is_not_laid() {
    let store = Arc::new(HostedRecords::default());
    let tmux = Arc::new(FakeTmux::default());
    let w = World::over(store.clone(), tmux.clone(), Host::allowing());
    w.hosted
        .open(EXT, &alice(), OpenSession::shell("s"))
        .await
        .unwrap();
    let refusing = World::over(store, tmux.clone(), Host::refusing());
    let refused = refusing
        .hosted
        .type_text(EXT, &alice(), "s", "rm -rf")
        .await
        .unwrap_err();
    assert_eq!(refused.status, 403);
    assert!(tmux.pasted_without_enter.lock().unwrap().is_empty());
}

#[tokio::test]
async fn the_artefacts_live_are_listed_read_published_kept_and_forgotten() {
    let w = World::new();
    let session = w
        .hosted
        .open(EXT, &alice(), OpenSession::agent("a"))
        .await
        .unwrap();
    let live = w.store.state_dir(&session.id).join("live");
    let script = w.last_script_of(&session.id);
    assert!(script.contains("PDO_LIVE_DIR"), "{script}");
    assert!(script.contains(&*live.to_string_lossy()), "{script}");
    assert!(live.is_dir(), "created before the launch");
    assert!(w.hosted.live_artifacts(EXT, "a").is_empty());

    std::fs::create_dir_all(live.join("notes")).unwrap();
    std::fs::write(live.join("notes/plan.md"), "# Plan").unwrap();
    std::fs::write(live.join("progress.md"), "50 %").unwrap();
    w.hosted.reconcile();
    let listed = w.hosted.live_artifacts(EXT, "a");
    let paths: Vec<&str> = listed.iter().map(|a| a.path.as_str()).collect();
    assert_eq!(paths, ["notes/plan.md", "progress.md"]);
    assert_eq!(listed[1].size, 4);
    assert_eq!(
        w.hosted.live_artifact(EXT, "a", "notes/plan.md").unwrap(),
        b"# Plan"
    );
    let published = |w: &World| {
        w.host
            .events()
            .into_iter()
            .filter(|(_, e)| e["type"] == "hosted_live_artifacts_changed")
            .collect::<Vec<_>>()
    };
    let events = published(&w);
    assert_eq!(events.len(), 1);
    assert!(events[0].0, "on the bus, hence /ws");
    assert_eq!(events[0].1["key"], "a");
    assert_eq!(events[0].1["artifacts"][0]["path"], "notes/plan.md");
    w.hosted.reconcile();
    assert_eq!(published(&w).len(), 1, "nothing changed, nothing published");
    std::fs::write(live.join("progress.md"), "100 %").unwrap();
    w.hosted.reconcile();
    assert_eq!(published(&w).len(), 2);

    for bad in ["../session.json", "/etc/passwd"] {
        assert_eq!(
            w.hosted.live_artifact(EXT, "a", bad).unwrap_err().status,
            400,
            "{bad}"
        );
    }
    assert_eq!(
        w.hosted
            .live_artifact(EXT, "a", "nope.md")
            .unwrap_err()
            .status,
        404
    );
    assert_eq!(
        w.hosted
            .live_artifact(EXT, "never", "a")
            .unwrap_err()
            .status,
        404
    );

    w.hosted.terminate(EXT, &alice(), "a").await.unwrap();
    w.hosted.relaunch(EXT, &alice(), "a").await.unwrap();
    assert_eq!(
        w.hosted.live_artifacts(EXT, "a").len(),
        2,
        "kept across a relaunch"
    );

    let refused = w.hosted.forget(EXT, &alice(), "a").await.unwrap_err();
    assert_eq!(refused.status, 409, "a live session is not forgotten");
    w.hosted.terminate(EXT, &alice(), "a").await.unwrap();
    let forgotten = w.hosted.forget(EXT, &alice(), "a").await.unwrap();
    assert_eq!(forgotten.id, session.id);
    assert_eq!(
        w.host.asked().last().unwrap().verb,
        Verb::Action("forget".into())
    );
    assert!(w.hosted.registry().find(EXT, "a").is_none());
    assert!(w.store.load().is_empty());
    assert!(
        !w.store.state_dir(&session.id).exists(),
        "its state is erased"
    );
    assert!(w.hosted.live_artifacts(EXT, "a").is_empty());
    assert!(w
        .host
        .events()
        .iter()
        .any(|(_, e)| e["type"] == "hosted_session_forgotten" && e["session"]["key"] == "a"));
    assert_eq!(
        w.hosted
            .forget(EXT, &alice(), "a")
            .await
            .unwrap_err()
            .status,
        404
    );
    let reopened = w.hosted.open(EXT, &alice(), OpenSession::agent("a")).await;
    assert!(reopened.is_ok(), "its key is free again");
}

#[tokio::test]
async fn a_declared_wait_is_published_and_lifted_by_an_enter_after_typing() {
    let w = World::new();
    let session = w
        .hosted
        .open(EXT, &alice(), OpenSession::agent("a").in_project("p1"))
        .await
        .unwrap();
    let declared = w
        .hosted
        .declare_wait(&alice(), &session.id, Some("Which DB?"))
        .unwrap();
    assert_eq!(
        declared,
        pdo_session::domain::declared_wait::DeclareOutcome::Declared
    );
    let record = w.hosted.registry().find(EXT, "a").unwrap().session;
    assert_eq!(record.state, HostedState::AwaitingUser);
    assert_eq!(record.wait_message.as_deref(), Some("Which DB?"));
    assert_eq!(w.changes().last().unwrap().0, HostedState::AwaitingUser);
    let asked = w.host.asked().last().unwrap().clone();
    assert_eq!(asked.verb, Verb::Action("wait_user".into()));
    assert_eq!(asked.projects, ["p1"]);
    assert_eq!(
        w.hosted
            .declare_wait(&alice(), &session.id, Some("Which DB?"))
            .unwrap(),
        pdo_session::domain::declared_wait::DeclareOutcome::Noop
    );

    let lift = w.lift();
    lift.note_input(&bob(), &session.id, b"\r").await;
    assert_eq!(
        w.hosted.registry().find(EXT, "a").unwrap().session.state,
        HostedState::AwaitingUser,
        "a bare Enter types nothing"
    );
    lift.note_input(&bob(), &session.id, b"postgres").await;
    lift.note_input(&bob(), &session.id, b"\r").await;
    let record = w.hosted.registry().find(EXT, "a").unwrap().session;
    assert_eq!(record.state, HostedState::Running);
    assert_eq!(record.wait_message, None);
    assert_eq!(w.changes().last().unwrap().0, HostedState::Running);
    assert_eq!(
        w.host.asked().last().unwrap().verb,
        Verb::Action("resume".into())
    );

    // A text laid by the extension counts as typed: its Enter lifts.
    w.hosted.declare_wait(&alice(), &session.id, None).unwrap();
    w.hosted.type_text(EXT, &alice(), "a", "oui").await.unwrap();
    lift.note_input(&bob(), &session.id, b"\r").await;
    assert_eq!(
        w.hosted.registry().find(EXT, "a").unwrap().session.state,
        HostedState::Running
    );
}

#[tokio::test]
async fn a_wait_needs_a_running_hosted_session() {
    let w = World::new();
    assert_eq!(
        w.hosted
            .declare_wait(&alice(), "pdo-hosted-nobody", None)
            .unwrap_err(),
        HostedWaitError::NotFound
    );
    let session = w
        .hosted
        .open(EXT, &alice(), OpenSession::agent("a"))
        .await
        .unwrap();
    w.tmux.sessions.lock().unwrap().remove(&session.id);
    assert_eq!(
        w.hosted
            .declare_wait(&alice(), &session.id, None)
            .unwrap_err(),
        HostedWaitError::NotRunning,
        "its process is gone"
    );
    w.hosted.reconcile();
    assert_eq!(
        w.hosted
            .declare_wait(&alice(), &session.id, None)
            .unwrap_err(),
        HostedWaitError::NotRunning
    );
    let refusing = World::over(w.store.clone(), w.tmux.clone(), Host::refusing());
    let other = refusing
        .hosted
        .declare_wait(&alice(), &session.id, None)
        .unwrap_err();
    assert!(matches!(other, HostedWaitError::Refused(_)), "{other:?}");
}

#[tokio::test]
async fn an_awaiting_session_is_taken_back_awaiting_after_a_restart() {
    let w = World::new();
    let session = w
        .hosted
        .open(EXT, &alice(), OpenSession::agent("a"))
        .await
        .unwrap();
    w.hosted
        .declare_wait(&alice(), &session.id, Some("?"))
        .unwrap();
    let after = w.restarted();
    after.hosted.restore();
    assert_eq!(
        after
            .hosted
            .registry()
            .find(EXT, "a")
            .unwrap()
            .session
            .state,
        HostedState::AwaitingUser
    );
}

#[tokio::test]
async fn the_sessions_of_an_absent_extension_are_terminated_at_restart_and_kept() {
    let w = World::new();
    let gone = w
        .hosted
        .open("uninstalled", &alice(), OpenSession::agent("a"))
        .await
        .unwrap();
    let kept = w
        .hosted
        .open(EXT, &alice(), OpenSession::shell("s"))
        .await
        .unwrap();
    let after = w.restarted();
    after.hosted.restore();
    let record = after
        .hosted
        .registry()
        .find("uninstalled", "a")
        .unwrap()
        .session;
    assert_eq!(record.state, HostedState::Ended);
    assert_eq!(record.end_reason, Some(EndReason::Terminated));
    assert!(!after.tmux.has(&gone.id), "its process is killed");
    assert_eq!(
        after
            .hosted
            .registry()
            .find(EXT, "s")
            .unwrap()
            .session
            .state,
        HostedState::Running
    );
    assert!(after.tmux.has(&kept.id));
    assert_eq!(after.ledger.hosted_live(), 1);
}
