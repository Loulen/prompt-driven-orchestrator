//! The OS terminal pop-out.

use std::sync::Arc;

use pdo_kernel::{Actor, Verb};
use pdo_session::application::attach::{AttachError, AttachTerminal, Attached};
use pdo_session::domain::names::manager_session_name;

use crate::fakes::{FakeTmux, Host, Window};

#[tokio::test]
async fn a_session_pops_out_in_a_terminal_window() {
    let tmux = Arc::new(FakeTmux::default());
    let window = Arc::new(Window::default());
    let host = Host::allowing();
    let uc = AttachTerminal::new(tmux, window.clone(), host.clone());
    assert_eq!(
        uc.session(&Actor::LocalOperator, "pdo-shell-r1").await,
        Ok(Attached {
            session: "pdo-shell-r1".into(),
            terminal: "kitty".into()
        })
    );
    assert_eq!(
        window.opened.lock().unwrap().as_slice(),
        [(
            "pdo-shell-r1".to_string(),
            "tmux attach -t pdo-shell-r1".to_string()
        )]
    );
    assert_eq!(host.asked()[0].verb, Verb::Action("attach".into()));
}

#[tokio::test]
async fn the_manager_must_be_running_to_pop_out() {
    let window = Arc::new(Window::default());
    let uc = AttachTerminal::new(
        Arc::new(FakeTmux::default()),
        window.clone(),
        Host::allowing(),
    );
    assert_eq!(
        uc.manager(&Actor::LocalOperator, "r1").await,
        Err(AttachError::NoManager {
            session: "pdo-mgr-r1".into()
        })
    );
    let uc = AttachTerminal::new(
        FakeTmux::with_sessions(&[&manager_session_name("r1")]),
        window,
        Host::allowing(),
    );
    assert!(uc.manager(&Actor::LocalOperator, "r1").await.is_ok());
}

#[tokio::test]
async fn a_refusal_or_a_missing_display_is_named() {
    let window = Arc::new(Window::default());
    let uc = AttachTerminal::new(
        Arc::new(FakeTmux::default()),
        window.clone(),
        Host::refusing(),
    );
    assert!(matches!(
        uc.session(&Actor::LocalOperator, "s").await,
        Err(AttachError::Refused(_))
    ));
    assert!(window.opened.lock().unwrap().is_empty());
    let uc = AttachTerminal::new(
        Arc::new(FakeTmux::default()),
        Arc::new(Window {
            fail: true,
            ..Default::default()
        }),
        Host::allowing(),
    );
    assert_eq!(
        uc.session(&Actor::LocalOperator, "s").await,
        Err(AttachError::Terminal("failed to attach: no display".into()))
    );
}
