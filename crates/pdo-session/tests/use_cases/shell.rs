//! The run shell: create-if-absent in a terminal Run's worktree.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use pdo_kernel::{Actor, ResourceKind, Verb};
use pdo_session::application::open_shell::{OpenShell, OpenShellError, OpenedShell};
use pdo_session::application::ports::RunLookup;
use pdo_session::domain::names::shell_session_name;

use crate::fakes::{FakeTmux, Host, Shells};

const RUN: &str = "r1";

fn shells(eligible: bool, sandboxed: bool) -> Shells {
    Shells {
        eligible,
        worktree: PathBuf::from("/repo/.pdo/runs/r1/worktree"),
        sandboxed,
        lookup: None,
        sandbox_error: None,
        readied: Mutex::new(Vec::new()),
    }
}

fn open(shells: Shells, tmux: Arc<FakeTmux>, host: Arc<Host>) -> (OpenShell, Arc<Shells>) {
    let shells = Arc::new(shells);
    (OpenShell::new(shells.clone(), tmux, host, 6172), shells)
}

#[tokio::test]
async fn a_terminal_run_gets_one_shell_then_reattaches_to_it() {
    let tmux = Arc::new(FakeTmux::default());
    let host = Host::allowing();
    let (uc, _) = open(shells(true, false), tmux.clone(), host.clone());
    let first = uc.execute(&Actor::LocalOperator, RUN).await.unwrap();
    assert_eq!(
        first,
        OpenedShell {
            session: shell_session_name(RUN),
            created: true
        }
    );
    let (name, dir, script) = tmux.created.lock().unwrap()[0].clone();
    assert_eq!(name, "pdo-shell-r1");
    assert_eq!(dir, PathBuf::from("/repo/.pdo/runs/r1/worktree"));
    assert!(script.contains("while true; do bash -i; sleep 0.2; done"));
    let second = uc.execute(&Actor::LocalOperator, RUN).await.unwrap();
    assert!(!second.created, "a second open re-attaches");
    assert_eq!(tmux.created.lock().unwrap().len(), 1);
    let asked = host.asked();
    assert_eq!(asked[0].resource, ResourceKind::Session);
    assert_eq!(asked[0].verb, Verb::Create);
    // CORE-61: the shell's start is announced once, by who opened it, on the
    // bus (it is a session of the Run).
    let events = host.events();
    assert_eq!(events.len(), 1);
    assert!(events[0].0);
    assert_eq!(events[0].1["type"], "session_started");
    assert_eq!(events[0].1["actor"]["kind"], "local_operator");
    assert_eq!(events[0].1["session"]["kind"], "shell");
    assert_eq!(events[0].1["session"]["run_id"], RUN);
}

#[tokio::test]
async fn a_live_or_archived_run_is_not_eligible() {
    let tmux = Arc::new(FakeTmux::default());
    let (uc, _) = open(shells(false, false), tmux.clone(), Host::allowing());
    assert_eq!(
        uc.execute(&Actor::LocalOperator, RUN).await,
        Err(OpenShellError::NotEligible)
    );
    assert!(tmux.created.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_refusal_or_an_unknown_run_opens_nothing() {
    let tmux = Arc::new(FakeTmux::default());
    let (uc, _) = open(shells(true, false), tmux.clone(), Host::refusing());
    assert!(matches!(
        uc.execute(&Actor::LocalOperator, RUN).await,
        Err(OpenShellError::Refused(_))
    ));
    let mut unknown = shells(true, false);
    unknown.lookup = Some(RunLookup::NotFound);
    let (uc, _) = open(unknown, tmux.clone(), Host::allowing());
    assert_eq!(
        uc.execute(&Actor::LocalOperator, RUN).await,
        Err(OpenShellError::Run(RunLookup::NotFound))
    );
    assert!(tmux.created.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_sandboxed_shell_enters_its_container_or_fails_loudly() {
    let tmux = Arc::new(FakeTmux::default());
    let (uc, shells_port) = open(shells(true, true), tmux.clone(), Host::allowing());
    uc.execute(&Actor::LocalOperator, RUN).await.unwrap();
    assert_eq!(shells_port.readied.lock().unwrap().as_slice(), ["r1"]);
    let (_, _, script) = tmux.created.lock().unwrap()[0].clone();
    assert!(
        script.contains("docker exec pdo-sbx-r1 bash -lc"),
        "{script}"
    );

    let tmux = Arc::new(FakeTmux::default());
    let mut broken = shells(true, true);
    broken.sandbox_error = Some("docker daemon unreachable".into());
    let (uc, _) = open(broken, tmux.clone(), Host::allowing());
    assert_eq!(
        uc.execute(&Actor::LocalOperator, RUN).await,
        Err(OpenShellError::Sandbox("docker daemon unreachable".into()))
    );
    assert!(
        tmux.created.lock().unwrap().is_empty(),
        "never a silent host bash"
    );
}

#[tokio::test]
async fn a_lost_race_is_a_reattach_and_a_real_failure_is_named() {
    let tmux = Arc::new(FakeTmux::default());
    *tmux.fail_next_spawn.lock().unwrap() = Some("tmux new-session failed: boom".into());
    let (uc, _) = open(shells(true, false), tmux.clone(), Host::allowing());
    assert_eq!(
        uc.execute(&Actor::LocalOperator, RUN).await,
        Err(OpenShellError::Spawn(
            "failed to open shell: tmux new-session failed: boom".into()
        ))
    );
}
