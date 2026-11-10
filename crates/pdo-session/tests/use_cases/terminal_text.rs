//! « Copy & send to terminal »: a text into a node's input, without Enter, that
//! counts as typed for the declared wait.

use std::sync::Arc;

use pdo_kernel::{Actor, ResourceKind, Verb};
use pdo_session::application::declared_wait::{LiftWait, TypedInput};
use pdo_session::application::shared_terminal::SharedTerminalRegistry;
use pdo_session::application::terminal_text::{
    SendTerminalText, SendTerminalTextCommand, SendTextError, TERMINAL_TEXT_MAX_BYTES,
};
use pdo_session::domain::declared_wait::{
    AwaitingMarker, IterationFacts, NodeWaitFacts, WaitFacts, WaitRefusal,
};
use pdo_session::domain::names::node_session_name;
use tokio::sync::mpsc;

use crate::fakes::{FakeTmux, Host, Waits};

const RUN: &str = "20261008-100000-a11ce01";
const NODE: &str = "worker";

fn facts(run_live: bool, is_script: bool, iter: i64, awaiting: bool) -> WaitFacts {
    WaitFacts {
        run_live,
        defined: true,
        is_script,
        node: Some(NodeWaitFacts {
            iter,
            awaiting,
            marker: awaiting.then(|| AwaitingMarker {
                cause: "declared".into(),
                message: Some("Which contract?".into()),
            }),
            iterations: (1..=iter)
                .map(|i| IterationFacts {
                    iter: i,
                    holds_session: i == iter,
                })
                .collect(),
        }),
    }
}

struct World {
    waits: Arc<Waits>,
    tmux: Arc<FakeTmux>,
    host: Arc<Host>,
    typed: Arc<TypedInput>,
    terminals: Arc<SharedTerminalRegistry>,
}

impl World {
    fn new(facts: WaitFacts, host: Arc<Host>) -> Self {
        let iter = facts.node.as_ref().map(|n| n.iter).unwrap_or(1);
        Self {
            waits: Waits::with(RUN, NODE, facts),
            tmux: FakeTmux::with_sessions(&[&node_session_name(RUN, NODE, iter)]),
            host,
            typed: Arc::new(TypedInput::default()),
            terminals: Arc::default(),
        }
    }

    /// `poste` shows the terminal of the node's iteration `iter`, as `actor`;
    /// the receiver keeps its connection's role frames.
    fn show(&self, iter: i64, poste: &str, actor: Actor) -> mpsc::UnboundedReceiver<String> {
        let (tx, rx) = mpsc::unbounded_channel();
        self.terminals.join(
            &node_session_name(RUN, NODE, iter),
            Some(poste.into()),
            actor,
            tx,
        );
        rx
    }

    fn send(&self) -> SendTerminalText {
        SendTerminalText::new(
            self.waits.clone(),
            self.tmux.clone(),
            self.typed.clone(),
            self.host.clone(),
        )
        .with_terminals(self.terminals.clone())
    }

    fn lift(&self) -> LiftWait {
        LiftWait::new(self.waits.clone(), self.typed.clone(), self.host.clone())
    }

    fn pasted(&self) -> Vec<(String, String)> {
        self.tmux.pasted_without_enter.lock().unwrap().clone()
    }
}

fn command(text: &str, iter: Option<i64>) -> SendTerminalTextCommand {
    SendTerminalTextCommand {
        run_id: RUN.into(),
        node_id: NODE.into(),
        iter,
        text: text.into(),
        poste: None,
    }
}

#[tokio::test]
async fn the_text_is_pasted_without_enter_into_the_current_iteration() {
    let world = World::new(facts(true, false, 2, false), Host::allowing());
    let iter = world
        .send()
        .execute(&Actor::LocalOperator, command("I imported a file", None))
        .await;
    assert_eq!(iter, Ok(2));
    assert_eq!(
        world.pasted(),
        vec![(
            node_session_name(RUN, NODE, 2),
            "I imported a file".to_string()
        )]
    );
    // Never the paste that submits.
    assert!(world.tmux.pasted.lock().unwrap().is_empty());
    let asked = world.host.asked();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].resource, ResourceKind::Node);
    assert_eq!(asked[0].verb, Verb::Action("send_text".into()));
    assert_eq!(
        asked[0].target.as_deref(),
        Some(format!("{RUN}/{NODE}").as_str())
    );
}

#[tokio::test]
async fn a_refused_operation_writes_nothing() {
    let world = World::new(facts(true, false, 1, false), Host::refusing());
    let outcome = world
        .send()
        .execute(&Actor::LocalOperator, command("hello", None))
        .await;
    assert!(matches!(outcome, Err(SendTextError::Refused(d)) if d.reason == "admins only"));
    assert!(world.pasted().is_empty());
}

#[tokio::test]
async fn an_empty_or_too_long_text_is_refused() {
    let world = World::new(facts(true, false, 1, false), Host::allowing());
    let send = world.send();
    assert_eq!(
        send.execute(&Actor::LocalOperator, command("  \n ", None))
            .await,
        Err(SendTextError::Empty)
    );
    let long = "x".repeat(TERMINAL_TEXT_MAX_BYTES + 1);
    assert_eq!(
        send.execute(&Actor::LocalOperator, command(&long, None))
            .await,
        Err(SendTextError::TooLong)
    );
    let exact = "x".repeat(TERMINAL_TEXT_MAX_BYTES);
    assert_eq!(
        send.execute(&Actor::LocalOperator, command(&exact, None))
            .await,
        Ok(1)
    );
}

#[tokio::test]
async fn a_node_without_a_live_session_refuses_by_name() {
    let cases = [
        (facts(false, false, 1, false), None, WaitRefusal::RunNotLive),
        (facts(true, true, 1, false), None, WaitRefusal::NodeIsScript),
        // An older iteration holds no session.
        (
            facts(true, false, 2, false),
            Some(1),
            WaitRefusal::NodeSessionNotLive,
        ),
    ];
    for (facts, iter, expected) in cases {
        let world = World::new(facts, Host::allowing());
        let outcome = world
            .send()
            .execute(&Actor::LocalOperator, command("hello", iter))
            .await;
        assert!(
            matches!(&outcome, Err(SendTextError::Session(refusal, _)) if *refusal == expected),
            "{outcome:?}"
        );
        assert!(world.pasted().is_empty());
    }

    // The projection says running, tmux says the pane is gone.
    let world = World::new(facts(true, false, 1, false), Host::allowing());
    world.tmux.sessions.lock().unwrap().clear();
    let outcome = world
        .send()
        .execute(&Actor::LocalOperator, command("hello", None))
        .await;
    assert_eq!(
        outcome,
        Err(SendTextError::Session(WaitRefusal::NodeSessionNotLive, 1))
    );

    // A node the Run does not know.
    let world = World::new(facts(true, false, 1, false), Host::allowing());
    let outcome = world
        .send()
        .execute(
            &Actor::LocalOperator,
            SendTerminalTextCommand {
                node_id: "ghost".into(),
                ..command("hello", None)
            },
        )
        .await;
    assert_eq!(
        outcome,
        Err(SendTextError::Session(WaitRefusal::RunNotFound, 1))
    );
}

#[tokio::test]
async fn a_refused_paste_is_named_with_its_session() {
    let world = World::new(facts(true, false, 1, false), Host::allowing());
    *world.tmux.refuse_paste.lock().unwrap() = true;
    let outcome = world
        .send()
        .execute(&Actor::LocalOperator, command("hello", None))
        .await;
    assert_eq!(
        outcome,
        Err(SendTextError::PasteFailed(node_session_name(RUN, NODE, 1)))
    );
}

#[tokio::test]
async fn the_enter_that_follows_the_text_lifts_the_declared_wait() {
    let world = World::new(facts(true, false, 1, true), Host::allowing());
    let session = node_session_name(RUN, NODE, 1);
    world
        .send()
        .execute(&Actor::LocalOperator, command("I imported a file", None))
        .await
        .unwrap();
    // No byte crossed the PTY bridge: the pasted text counts as typed.
    world
        .lift()
        .note_input(&Actor::LocalOperator, &session, b"\r")
        .await;
    let resumed = world.waits.resumed.lock().unwrap().clone();
    assert_eq!(
        resumed,
        vec![(
            RUN.to_string(),
            NODE.to_string(),
            1,
            serde_json::json!({ "source": "pty_enter", "actor": { "kind": "local_operator" } })
        )]
    );
}

#[tokio::test]
async fn a_line_break_never_reaches_the_pane_as_an_enter() {
    let world = World::new(facts(true, false, 1, false), Host::allowing());
    world
        .send()
        .execute(
            &Actor::LocalOperator,
            command("first\r\nsecond\nthird\r", None),
        )
        .await
        .unwrap();
    assert_eq!(world.pasted()[0].1, "first second third ");
}

fn from(poste: &str, text: &str) -> SendTerminalTextCommand {
    SendTerminalTextCommand {
        poste: Some(poste.into()),
        ..command(text, None)
    }
}

fn alice() -> Actor {
    Actor::Identified {
        namespace: "test".into(),
        id: "alice".into(),
    }
}

#[tokio::test]
async fn a_spectator_poste_writes_nothing_and_learns_who_pilots() {
    let world = World::new(facts(true, false, 1, false), Host::allowing());
    let _a = world.show(1, "A", alice());
    let _b = world.show(1, "B", Actor::LocalOperator);
    let outcome = world
        .send()
        .execute(&Actor::LocalOperator, from("B", "hello"))
        .await;
    match outcome {
        Err(SendTextError::NotPilot(pilot)) => {
            assert_eq!(pilot.poste, "A");
            assert_eq!(pilot.actor, alice());
        }
        other => panic!("{other:?}"),
    }
    assert!(world.pasted().is_empty(), "the input is unchanged");
}

#[tokio::test]
async fn the_pilot_or_a_lone_poste_pastes_without_enter() {
    let world = World::new(facts(true, false, 1, false), Host::allowing());
    let _a = world.show(1, "A", alice());
    let send = world.send();
    assert_eq!(
        send.execute(&Actor::LocalOperator, from("A", "solo")).await,
        Ok(1)
    );
    let _b = world.show(1, "B", Actor::LocalOperator);
    assert_eq!(
        send.execute(&Actor::LocalOperator, from("A", "pilot"))
            .await,
        Ok(1)
    );
    let pasted: Vec<String> = world.pasted().into_iter().map(|(_, t)| t).collect();
    assert_eq!(pasted, vec!["solo", "pilot"]);
    assert!(
        world.tmux.pasted.lock().unwrap().is_empty(),
        "never an Enter"
    );
}

#[tokio::test]
async fn a_request_from_no_poste_passes_even_with_a_pilot() {
    let world = World::new(facts(true, false, 1, false), Host::allowing());
    let _a = world.show(1, "A", alice());
    let _b = world.show(1, "B", Actor::LocalOperator);
    assert_eq!(
        world
            .send()
            .execute(&Actor::LocalOperator, command("from the CLI", None))
            .await,
        Ok(1)
    );
    assert_eq!(world.pasted().len(), 1);
}

#[tokio::test]
async fn a_poste_that_does_not_show_this_terminal_writes() {
    let world = World::new(facts(true, false, 1, false), Host::allowing());
    let _a = world.show(1, "A", alice());
    let _b = world.show(1, "B", Actor::LocalOperator);
    let send = world.send();
    // No terminal of this node open on poste C: as before.
    assert_eq!(
        send.execute(&Actor::LocalOperator, from("C", "elsewhere"))
            .await,
        Ok(1)
    );
    // A spectator's role on another node's terminal does not count here.
    let world2 = World::new(facts(true, false, 2, false), Host::allowing());
    let _a1 = world2.show(1, "A", alice());
    let _b1 = world2.show(1, "B", Actor::LocalOperator);
    assert_eq!(
        world2
            .send()
            .execute(&Actor::LocalOperator, from("B", "iter 2"))
            .await,
        Ok(2)
    );
}
