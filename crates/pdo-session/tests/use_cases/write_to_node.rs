//! A text written into the session of a Node that waits for a person, on
//! behalf of an Actor (API 0.23): pasted, submitted, the Node resumed, the
//! Actor announced as taking control; refused with a stable reason when the
//! Node does not wait or the Actor may not take control.

use std::sync::Arc;

use pdo_extension_api::{NodeWrite, NodeWritten, NODE_WRITE_MAX_BYTES};
use pdo_kernel::{Actor, ResourceKind, Verb};
use pdo_session::application::ports::TerminalGesture;
use pdo_session::application::session_ledger::SessionLedger;
use pdo_session::application::write_to_node::WriteToNode;
use pdo_session::domain::declared_wait::{
    AwaitingMarker, IterationFacts, NodeWaitFacts, WaitFacts,
};
use pdo_session::domain::names::node_session_name;

use crate::fakes::{FakeTmux, Host, InP1, Waits, RUN_IN_P1};

const RUN: &str = RUN_IN_P1;
const NODE: &str = "worker";

fn bob() -> Actor {
    Actor::Identified {
        namespace: "test".into(),
        id: "bob".into(),
    }
}

/// A Node at its iteration `iter`, in a live Run, waiting on `cause` (none:
/// running).
fn facts(iter: i64, cause: Option<&str>) -> WaitFacts {
    WaitFacts {
        run_live: true,
        defined: true,
        is_script: false,
        node: Some(NodeWaitFacts {
            iter,
            awaiting: cause.is_some(),
            marker: cause.map(|cause| AwaitingMarker {
                cause: cause.into(),
                message: Some("Which retention?".into()),
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

fn waiting(iter: i64) -> WaitFacts {
    facts(iter, Some("declared"))
}

struct World {
    waits: Arc<Waits>,
    tmux: Arc<FakeTmux>,
    host: Arc<Host>,
}

impl World {
    fn new(facts: WaitFacts, host: Arc<Host>) -> Self {
        let iter = facts.node.as_ref().map(|n| n.iter).unwrap_or(1);
        Self {
            waits: Waits::with(RUN, NODE, facts),
            tmux: FakeTmux::with_sessions(&[&node_session_name(RUN, NODE, iter)]),
            host,
        }
    }

    fn write_to_node(&self) -> WriteToNode {
        let ledger = Arc::new(SessionLedger::new(self.tmux.clone(), self.host.clone()));
        WriteToNode::new(
            self.waits.clone(),
            self.tmux.clone(),
            self.host.clone(),
            ledger,
        )
        .with_projects(Arc::new(InP1))
    }

    async fn write(&self, actor: Actor, write: NodeWrite) -> Result<NodeWritten, (u16, String)> {
        self.write_to_node()
            .execute(&actor, write)
            .await
            .map_err(|refusal| (refusal.status, refusal.reason))
    }

    /// What reached the panes: the pastes, then the Enters.
    fn written(&self) -> (Vec<(String, String)>, Vec<String>) {
        (
            self.tmux.pasted_without_enter.lock().unwrap().clone(),
            self.tmux.entered.lock().unwrap().clone(),
        )
    }

    fn nothing_written(&self) -> bool {
        let (pasted, entered) = self.written();
        pasted.is_empty()
            && entered.is_empty()
            && self.waits.resumed.lock().unwrap().is_empty()
            && self.host.events().is_empty()
    }
}

#[tokio::test]
async fn the_text_is_typed_and_submitted_and_the_node_resumes_with_the_actor() {
    let world = World::new(waiting(2), Host::allowing());
    let session = node_session_name(RUN, NODE, 2);

    let written = world
        .write(bob(), NodeWrite::new(RUN, NODE, "continue"))
        .await
        .unwrap();

    assert_eq!(written, NodeWritten::new(RUN, NODE, 2, session.clone()));
    assert_eq!(
        world.written(),
        (
            vec![(session.clone(), "continue".to_string())],
            vec![session.clone()]
        )
    );
    let resumed = world.waits.resumed.lock().unwrap().clone();
    assert_eq!(
        resumed,
        vec![(
            RUN.to_string(),
            NODE.to_string(),
            2,
            serde_json::json!({
                "source": "extension",
                "actor": { "kind": "identified", "namespace": "test", "id": "bob" },
            })
        )]
    );
    // The Run's log keeps Bob's take-over, as the announcement says.
    let gestures = world.waits.gestures.lock().unwrap().clone();
    assert_eq!(gestures.len(), 1, "{gestures:?}");
    assert_eq!(gestures[0].3, TerminalGesture::TakenOver);
    assert_eq!(gestures[0].4["actor"]["id"], "bob");
    // Bob is announced as taking control, as when he takes the hand.
    let events = world.host.events();
    assert_eq!(events.len(), 1, "{events:#?}");
    let (on_the_bus, event) = &events[0];
    assert!(on_the_bus);
    assert_eq!(event["type"], "session_taken_over");
    assert_eq!(event["actor"]["id"], "bob");
    assert_eq!(event["session"]["id"], session.as_str());
    assert_eq!(event["session"]["node_id"], NODE);
    // The right asked is the one of taking control, on the session, in the
    // Projets of its Run.
    let asked = world.host.asked();
    assert_eq!(asked.len(), 1, "{asked:#?}");
    assert_eq!(asked[0].resource, ResourceKind::Session);
    assert_eq!(asked[0].verb, Verb::Action("take_control".into()));
    assert_eq!(asked[0].target.as_deref(), Some(session.as_str()));
    assert_eq!(asked[0].projects, ["p1"]);
}

#[tokio::test]
async fn a_completion_not_released_waits_for_a_person_too() {
    let world = World::new(facts(1, Some("completion_not_released")), Host::allowing());
    let written = world.write(bob(), NodeWrite::new(RUN, NODE, "ok")).await;
    assert!(written.is_ok(), "{written:?}");
}

#[tokio::test]
async fn a_text_on_several_lines_is_submitted_once_on_one_line() {
    let world = World::new(waiting(1), Host::allowing());
    world
        .write(bob(), NodeWrite::new(RUN, NODE, "first\nsecond\r\nthird"))
        .await
        .unwrap();
    let (pasted, entered) = world.written();
    assert_eq!(pasted[0].1, "first second third");
    assert_eq!(entered.len(), 1);
}

#[tokio::test]
async fn a_node_that_does_not_wait_is_refused_and_nothing_is_written() {
    let mut ended = waiting(1);
    ended.run_live = false;
    let cases = [
        ("running", facts(1, None), None),
        ("run ended", ended, None),
        (
            "waits on its children",
            facts(1, Some("children_pending")),
            None,
        ),
        ("another iteration", waiting(2), Some(1)),
    ];
    for (case, facts, iter) in cases {
        let world = World::new(facts, Host::allowing());
        let mut write = NodeWrite::new(RUN, NODE, "continue");
        if let Some(iter) = iter {
            write = write.at_iter(iter);
        }
        let refused = world
            .write_to_node()
            .execute(&bob(), write)
            .await
            .unwrap_err();
        assert_eq!(
            (
                refused.status,
                refused.reason.as_str(),
                refused.message.as_str()
            ),
            (409, "not_waiting", "the node no longer waits"),
            "{case}"
        );
        assert!(world.nothing_written(), "{case}");
    }
}

#[tokio::test]
async fn a_node_whose_pane_is_gone_does_not_wait() {
    let world = World::new(waiting(1), Host::allowing());
    world.tmux.sessions.lock().unwrap().clear();
    let refused = world
        .write(bob(), NodeWrite::new(RUN, NODE, "continue"))
        .await;
    assert_eq!(refused, Err((409, "not_waiting".to_string())));
    assert!(world.nothing_written());
}

#[tokio::test]
async fn an_unknown_run_or_node_is_refused_as_not_waiting() {
    let world = World::new(waiting(1), Host::allowing());
    for (run, node) in [("20990101-000000-0000000", NODE), (RUN, "ghost")] {
        let refused = world
            .write(bob(), NodeWrite::new(run, node, "continue"))
            .await;
        assert_eq!(
            refused,
            Err((404, "not_waiting".to_string())),
            "{run}/{node}"
        );
    }
    // A node the pipeline defines but that never started holds no session.
    let never_started = WaitFacts {
        node: None,
        ..waiting(1)
    };
    let world = World::new(never_started, Host::allowing());
    let refused = world
        .write(bob(), NodeWrite::new(RUN, NODE, "continue"))
        .await;
    assert_eq!(refused, Err((409, "not_waiting".to_string())));
    assert!(world.nothing_written());
}

#[tokio::test]
async fn an_actor_who_may_not_take_control_is_refused_and_nothing_is_written() {
    let world = World::new(waiting(1), Host::refusing());
    let refused = world
        .write_to_node()
        .execute(&bob(), NodeWrite::new(RUN, NODE, "continue"))
        .await
        .unwrap_err();
    assert_eq!((refused.status, refused.reason.as_str()), (403, "refused"));
    assert!(refused.message.contains("admins only"), "{refused}");
    assert!(world.nothing_written());

    // A refused Actor learns nothing of which Nodes exist.
    let refused = world
        .write(bob(), NodeWrite::new(RUN, "ghost", "continue"))
        .await;
    assert_eq!(refused, Err((403, "refused".to_string())));
}

#[tokio::test]
async fn a_blank_or_too_long_text_is_refused() {
    let world = World::new(waiting(1), Host::allowing());
    for text in ["  \n ".to_string(), "x".repeat(NODE_WRITE_MAX_BYTES + 1)] {
        let refused = world.write(bob(), NodeWrite::new(RUN, NODE, text)).await;
        assert_eq!(refused, Err((400, "invalid_text".to_string())));
    }
    assert!(world.nothing_written());
}

#[tokio::test]
async fn a_paste_or_an_enter_the_terminal_refuses_leaves_the_node_waiting() {
    let world = World::new(waiting(1), Host::allowing());
    *world.tmux.refuse_paste.lock().unwrap() = true;
    let refused = world
        .write(bob(), NodeWrite::new(RUN, NODE, "continue"))
        .await;
    assert_eq!(refused, Err((502, "write_failed".to_string())));
    assert!(world.nothing_written());

    let world = World::new(waiting(1), Host::allowing());
    *world.tmux.refuse_enter.lock().unwrap() = true;
    let refused = world
        .write(bob(), NodeWrite::new(RUN, NODE, "continue"))
        .await;
    assert_eq!(refused, Err((502, "write_failed".to_string())));
    assert!(world.waits.resumed.lock().unwrap().is_empty());
    assert!(world.host.events().is_empty());
}

#[tokio::test]
async fn of_two_writes_at_once_only_the_first_lands() {
    let world = World::new(waiting(1), Host::allowing());
    let use_case = world.write_to_node();
    let bob = bob();
    let (a, b) = tokio::join!(
        use_case.execute(&bob, NodeWrite::new(RUN, NODE, "first")),
        use_case.execute(&Actor::LocalOperator, NodeWrite::new(RUN, NODE, "second")),
    );
    let landed = [a.is_ok(), b.is_ok()];
    assert_eq!(landed.iter().filter(|ok| **ok).count(), 1, "{a:?} {b:?}");
    assert!(a.err().or(b.err()).unwrap().is_not_waiting());
    assert_eq!(world.written().0.len(), 1);
}
