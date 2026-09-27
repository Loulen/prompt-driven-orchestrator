//! The orphan sweep: inventory first, owners second, kill last.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use pdo_session::application::assistant::FocusCell;
use pdo_session::application::sweep::{ReaperGauge, SweepOrphans};
use pdo_session::domain::libassist::LibassistFocus;
use pdo_session::domain::names::{
    manager_session_name, node_session_name, shell_session_name, LIBASSIST_SESSION_NAME,
};
use pdo_session::domain::reaper::{NodeRunInfo, SweepTtls};

use crate::fakes::{FakeTmux, Owners};

const RUN: &str = "20260731-153057-553fcb3";

fn ttls() -> SweepTtls {
    SweepTtls {
        node_run: Duration::from_secs(3600),
        libassist_idle: Duration::from_secs(120),
    }
}

fn owners(facts: &[((&str, &str), NodeRunInfo)], unreadable: bool) -> Arc<Owners> {
    Arc::new(Owners {
        facts: facts
            .iter()
            .map(|((r, n), i)| ((r.to_string(), n.to_string()), i.clone()))
            .collect::<HashMap<_, _>>(),
        unreadable,
        ttls: ttls(),
        asked: Default::default(),
    })
}

fn live() -> NodeRunInfo {
    NodeRunInfo {
        completed_at: None,
        is_archived: false,
    }
}

fn sweep(tmux: Arc<FakeTmux>, owners: Arc<Owners>, focus: Arc<FocusCell>) -> SweepOrphans {
    SweepOrphans::new(tmux, owners, focus, Arc::new(ReaperGauge::default()))
}

#[tokio::test]
async fn owned_sessions_survive_and_orphans_are_killed() {
    let node = node_session_name(RUN, "alpha", 1);
    let ghost = node_session_name(RUN, "ghost", 1);
    let manager = manager_session_name(RUN);
    let shell = shell_session_name(RUN);
    let tmux = FakeTmux::with_sessions(&[&node, &ghost, &manager, &shell, "pdo-garbage"]);
    let owners = owners(
        &[
            ((RUN, "alpha"), live()),
            ((RUN, "__manager__"), live()),
            (
                (RUN, "__shell__"),
                NodeRunInfo {
                    completed_at: None,
                    is_archived: true,
                },
            ),
        ],
        false,
    );
    let tally = sweep(tmux.clone(), owners.clone(), Arc::new(FocusCell::default()))
        .run()
        .await
        .unwrap();
    assert!(tmux.has(&node) && tmux.has(&manager));
    assert!(!tmux.has(&ghost) && !tmux.has(&shell) && !tmux.has("pdo-garbage"));
    assert_eq!(tally.killed, 3);
    assert_eq!(
        tally.killed_for_absent_run, 2,
        "the ghost and the garbage name"
    );
    // The manager and the shell resolve against their Run, under fixed ids.
    let asked = owners.asked.lock().unwrap().clone();
    assert!(asked.contains(&(RUN.to_string(), "__manager__".to_string())));
    assert!(asked.contains(&(RUN.to_string(), "__shell__".to_string())));
}

/// A storage error must never read a live run as "absent": the pass aborts
/// having killed nothing.
#[tokio::test]
async fn an_unreadable_owner_aborts_the_pass_before_any_kill() {
    let node = node_session_name(RUN, "alpha", 1);
    let tmux = FakeTmux::with_sessions(&[&node, "pdo-garbage"]);
    let result = sweep(
        tmux.clone(),
        owners(&[], true),
        Arc::new(FocusCell::default()),
    )
    .run()
    .await;
    assert!(result.is_err());
    assert!(tmux.killed.lock().unwrap().is_empty());
}

#[tokio::test]
async fn the_library_assistant_is_judged_on_presence() {
    let tmux = FakeTmux::with_sessions(&[LIBASSIST_SESSION_NAME]);
    let focus = Arc::new(FocusCell::default());
    focus.set(Some(LibassistFocus {
        pipeline_id: "review".into(),
        path: None,
        at: chrono::Utc::now(),
    }));
    sweep(tmux.clone(), owners(&[], false), focus.clone())
        .run()
        .await
        .unwrap();
    assert!(tmux.has(LIBASSIST_SESSION_NAME), "a fresh focus keeps it");

    focus.set(None);
    tmux.attached
        .lock()
        .unwrap()
        .insert(LIBASSIST_SESSION_NAME.into());
    sweep(tmux.clone(), owners(&[], false), focus.clone())
        .run()
        .await
        .unwrap();
    assert!(
        tmux.has(LIBASSIST_SESSION_NAME),
        "an attached terminal keeps it"
    );

    tmux.attached.lock().unwrap().clear();
    let tally = sweep(tmux.clone(), owners(&[], false), focus)
        .run()
        .await
        .unwrap();
    assert!(!tmux.has(LIBASSIST_SESSION_NAME), "nobody there: reaped");
    assert_eq!(tally.killed_for_absent_run, 0, "idleness is housekeeping");
}
