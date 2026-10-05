//! The Projets of the session context's Operations (CORE-3): an Operation on
//! a Run's session, shell or node is in the Projets of that Run; the library
//! assistant's in none.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use pdo_kernel::Actor;
use pdo_session::application::attach::AttachTerminal;
use pdo_session::application::declared_wait::{DeclareWait, DeclareWaitCommand, TypedInput};
use pdo_session::application::open_shell::OpenShell;
use pdo_session::application::ports::projects_of_session;
use pdo_session::application::terminal::Terminals;
use pdo_session::domain::names::{
    manager_session_name, node_session_name, shell_session_name, LIBASSIST_SESSION_NAME,
};

use crate::fakes::{FakeTmux, Host, InP1, Shells, Waits, Window};

/// A Run in `p1` ([`InP1`]), and one in no Projet.
const RUN: &str = crate::fakes::RUN_IN_P1;
const OTHER: &str = "20260912-100000-0ther00";

fn p1() -> Vec<String> {
    vec!["p1".to_string()]
}

#[tokio::test]
async fn a_session_is_in_the_projets_of_its_run() {
    for session in [
        node_session_name(RUN, "worker", 2),
        manager_session_name(RUN),
        shell_session_name(RUN),
    ] {
        assert_eq!(
            projects_of_session(&InP1, &session).await,
            p1(),
            "{session}"
        );
    }
    assert!(projects_of_session(&InP1, &manager_session_name(OTHER))
        .await
        .is_empty());
    assert!(projects_of_session(&InP1, LIBASSIST_SESSION_NAME)
        .await
        .is_empty());
    assert!(projects_of_session(&InP1, "not-pdo").await.is_empty());
}

#[tokio::test]
async fn attaching_a_window_or_a_terminal_and_taking_control_are_in_the_runs_projets() {
    let host = Host::refusing();
    let attach = AttachTerminal::new(
        Arc::new(FakeTmux::default()),
        Arc::new(Window::default()),
        host.clone(),
    )
    .with_projects(Arc::new(InP1));
    let _ = attach
        .session(&Actor::LocalOperator, &node_session_name(RUN, "worker", 1))
        .await;
    let _ = attach.manager(&Actor::LocalOperator, RUN).await;
    let _ = attach
        .session(&Actor::LocalOperator, LIBASSIST_SESSION_NAME)
        .await;

    let terminals = Terminals::new(
        Arc::new(FakeTmux::default()),
        pdo_session::application::declared_wait::LiftWait::new(
            Arc::new(Waits::default()),
            Arc::new(TypedInput::default()),
            host.clone(),
        ),
        host.clone(),
    )
    .with_projects(Arc::new(InP1));
    let session = manager_session_name(RUN);
    assert!(terminals
        .authorize_attach(&Actor::LocalOperator, &session)
        .await
        .is_err());
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let joined = terminals.registry().join(&session, None, tx);
    assert!(
        !terminals
            .take_control(&Actor::LocalOperator, &session, joined.id)
            .await
    );

    let projects: Vec<Vec<String>> = host.asked().into_iter().map(|op| op.projects).collect();
    assert_eq!(projects, vec![p1(), p1(), vec![], p1(), p1()]);
}

#[tokio::test]
async fn opening_a_run_shell_and_declaring_a_wait_are_in_the_runs_projets() {
    let host = Host::refusing();
    let shells = Arc::new(Shells {
        eligible: true,
        worktree: PathBuf::from("/repo/.pdo/runs/run/worktree"),
        sandboxed: false,
        lookup: None,
        sandbox_error: None,
        readied: Mutex::new(Vec::new()),
    });
    let shell = OpenShell::new(shells, Arc::new(FakeTmux::default()), host.clone(), 6172)
        .with_projects(Arc::new(InP1));
    assert!(shell.execute(&Actor::LocalOperator, RUN).await.is_err());

    let declare = DeclareWait::new(
        Arc::new(Waits::default()),
        Arc::new(FakeTmux::default()),
        Arc::new(TypedInput::default()),
        host.clone(),
    )
    .with_projects(Arc::new(InP1));
    let command = DeclareWaitCommand {
        run_id: RUN.into(),
        node_id: "worker".into(),
        iter: 1,
        message: None,
    };
    assert!(declare
        .execute(&Actor::LocalOperator, command)
        .await
        .is_err());

    let projects: Vec<Vec<String>> = host.asked().into_iter().map(|op| op.projects).collect();
    assert_eq!(projects, vec![p1(), p1()]);
}
