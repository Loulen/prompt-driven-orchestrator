//! The idle watch of the Agent de support, on an injected clock: its person's
//! input starts the clock over; once the idle timeout runs out, « Still
//! there? » goes to every tab; « Keep » starts the clock over; unanswered,
//! the agent closes 60 s later, with no tab open as well.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use chrono::{DateTime, Utc};
use pdo_harness::domain::agent_choice::AgentChoice;
use pdo_kernel::event_bus::{BroadcastEventBus, EventBus};
use pdo_kernel::Verb;
use pdo_session::application::ports::BoxFuture;
use pdo_session::application::support::{
    ChoiceRefusal, ResolvedAgent, Support, SupportAgents, SupportClock, SupportError,
};
use pdo_session::domain::names::support_session_name_of;
use serde_json::{json, Value};
use tokio::sync::broadcast;

use crate::fakes::{FakeTmux, Host};
use crate::support::{alice, deep, support, Agents};

/// A clock the test moves by hand.
struct Clock(Mutex<DateTime<Utc>>);

impl Clock {
    fn at(secs: i64) -> Arc<Self> {
        Arc::new(Self(Mutex::new(t(secs))))
    }

    fn set(&self, secs: i64) {
        *self.0.lock().unwrap() = t(secs);
    }
}

impl SupportClock for Clock {
    fn now(&self) -> DateTime<Utc> {
        *self.0.lock().unwrap()
    }
}

/// The moment `secs` seconds after the test's origin.
fn t(secs: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(1_800_000_000 + secs, 0).unwrap()
}

fn rfc(secs: i64) -> String {
    t(secs).to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// The agents of the support tests, with an idle timeout of a minute: the
/// instance's setting, as the port reads it.
struct OneMinute;

impl SupportAgents for OneMinute {
    fn resolve<'a>(
        &'a self,
        choice: &'a AgentChoice,
    ) -> BoxFuture<'a, Result<ResolvedAgent, ChoiceRefusal>> {
        Agents.resolve(choice)
    }

    fn idle_timeout(&self) -> BoxFuture<'_, Duration> {
        Box::pin(async { Duration::from_secs(60) })
    }
}

struct Watch {
    uc: Arc<Support>,
    tmux: Arc<FakeTmux>,
    clock: Arc<Clock>,
    host: Arc<Host>,
    events: broadcast::Receiver<Value>,
    session: String,
    _root: tempfile::TempDir,
}

impl Watch {
    /// An agent at the default timeout (30 min), opened at 0 when `open`.
    async fn new(open: bool) -> Self {
        Self::over(support, open).await
    }

    async fn over(
        build: fn(&Path, Arc<FakeTmux>, Arc<Host>, Option<&str>) -> Support,
        open: bool,
    ) -> Self {
        let root = tempfile::tempdir().unwrap();
        let tmux = Arc::new(FakeTmux::default());
        let host = Host::allowing();
        let clock = Clock::at(0);
        let bus = Arc::new(BroadcastEventBus::new(64));
        let events = bus.subscribe();
        let uc = Arc::new(
            build(root.path(), tmux.clone(), host.clone(), None)
                .with_clock(clock.clone())
                .with_events(bus),
        );
        if open {
            uc.open(&alice(), &deep(), None, false).await.unwrap();
        }
        Self {
            uc,
            tmux,
            clock,
            host,
            events,
            session: support_session_name_of(&alice()),
            _root: root,
        }
    }

    /// One pass of the watch at `secs`.
    async fn tick(&self, secs: i64) {
        self.clock.set(secs);
        self.uc.watch_idle_once().await;
    }

    /// The changes told since the last call, without their envelope.
    fn changes(&mut self) -> Vec<Value> {
        let mut changes = Vec::new();
        while let Ok(message) = self.events.try_recv() {
            assert_eq!(message["type"], "support_agent_changed");
            assert_eq!(message["support_session"], self.session.as_str());
            changes.push(message["support"].clone());
        }
        changes
    }

    fn alive(&self) -> bool {
        self.tmux.has(&self.session)
    }
}

#[tokio::test]
async fn unanswered_still_there_closes_the_agent_60_s_later() {
    let mut w = Watch::new(true).await;
    assert_eq!(w.changes(), [json!({ "change": "opened" })]);

    w.tick(1799).await;
    assert_eq!(w.changes(), Vec::<Value>::new());
    assert_eq!(w.uc.status(&alice()).still_there, None);

    w.tick(1800).await;
    assert_eq!(
        w.changes(),
        [json!({
            "change": "idle",
            "closes_at": rfc(1860),
            "closes_in_secs": 60,
            "idle_timeout_secs": 1800,
        })]
    );
    let asked = w.uc.status(&alice()).still_there.unwrap();
    assert_eq!(asked.closes_at, rfc(1860));
    assert_eq!(asked.idle_timeout_secs, 1800);
    w.clock.set(1845);
    assert_eq!(
        w.uc.status(&alice()).still_there.unwrap().closes_in_secs,
        15
    );

    // No tab answers, none needs to be open: the daemon closes it.
    w.tick(1859).await;
    assert!(w.alive());
    w.tick(1860).await;
    assert!(!w.alive());
    assert_eq!(w.changes(), [json!({ "change": "closed", "idle": true })]);
    let status = w.uc.status(&alice());
    assert!(!status.alive);
    assert_eq!(status.agent, None);
    assert_eq!(status.last_choice, Some(deep()), "the last choice stays");
}

#[tokio::test]
async fn a_keystroke_or_a_question_starts_the_clock_over() {
    let mut w = Watch::new(true).await;
    w.tick(1000).await;
    w.uc.note_input(&w.session, b"x");
    assert_eq!(w.uc.status(&alice()).last_input_at, Some(rfc(1000)));
    // Another session's keystroke is none of its business.
    w.clock.set(1500);
    w.uc.note_input("pdo-20260101-000000-abc123-node-iter-1", b"x");
    w.tick(1800).await;
    w.tick(2799).await;
    assert!(w.uc.status(&alice()).still_there.is_none());

    w.clock.set(2700);
    w.uc.ask(&alice(), "and now?").unwrap();
    w.tick(4499).await;
    assert!(w.uc.status(&alice()).still_there.is_none());
    w.tick(4500).await;
    assert!(w.uc.status(&alice()).still_there.is_some());
    w.changes();
    // A keystroke answers « Still there? » as Keep does.
    w.clock.set(4510);
    w.uc.note_input(&w.session, b"x");
    assert_eq!(w.changes(), [json!({ "change": "active" })]);
    w.tick(4570).await;
    assert!(w.alive());
}

#[tokio::test]
async fn keep_answers_and_the_clock_starts_over() {
    let mut w = Watch::new(true).await;
    w.tick(1800).await;
    w.changes();

    w.clock.set(1820);
    assert_eq!(w.uc.keep(&alice()), Ok(w.session.clone()));
    assert_eq!(w.changes(), [json!({ "change": "active" })]);
    let status = w.uc.status(&alice());
    assert_eq!(status.still_there, None);
    assert_eq!(status.last_input_at, Some(rfc(1820)));

    w.tick(1860).await;
    assert!(w.alive(), "kept: it stays");
    w.tick(3619).await;
    assert_eq!(w.changes(), Vec::<Value>::new());
    w.tick(3620).await;
    assert_eq!(w.changes()[0]["change"], "idle");

    let keep = w
        .host
        .asked()
        .into_iter()
        .rfind(|o| o.verb == Verb::Action("ask".into()))
        .unwrap();
    assert_eq!(keep.target.as_deref(), Some(w.session.as_str()));
}

#[tokio::test]
async fn keep_needs_an_open_agent_and_the_right_to_ask_it() {
    let w = Watch::new(false).await;
    assert_eq!(w.uc.keep(&alice()), Err(SupportError::NotOpen));

    let root = tempfile::tempdir().unwrap();
    let refused = support(root.path(), w.tmux.clone(), Host::refusing(), None);
    assert!(matches!(
        refused.keep(&alice()),
        Err(SupportError::Refused(_))
    ));
}

#[tokio::test]
async fn the_timeout_is_the_instance_setting() {
    let mut w = Watch::over(
        |root, tmux, host, seam| {
            Support::new(
                tmux,
                Arc::new(OneMinute),
                host,
                pdo_session::application::support::SupportLaunch {
                    root: root.to_path_buf(),
                    daemon_port: 6172,
                    tmux_cmd_override: seam.map(String::from),
                },
            )
        },
        true,
    )
    .await;
    w.changes();
    w.tick(59).await;
    assert_eq!(w.changes(), Vec::<Value>::new());
    w.tick(60).await;
    assert_eq!(
        w.changes(),
        [json!({
            "change": "idle",
            "closes_at": rfc(120),
            "closes_in_secs": 60,
            "idle_timeout_secs": 60,
        })]
    );
}

#[tokio::test]
async fn an_agent_that_outlived_a_restart_starts_its_clock_when_first_seen() {
    let mut w = Watch::new(false).await;
    w.tmux
        .sessions
        .lock()
        .unwrap()
        .insert(w.session.clone(), "pane".into());
    w.tick(500).await;
    assert_eq!(w.uc.status(&alice()).last_input_at, Some(rfc(500)));
    w.tick(2299).await;
    assert_eq!(w.changes(), Vec::<Value>::new());
    w.tick(2300).await;
    assert_eq!(w.changes()[0]["change"], "idle");
}

#[tokio::test]
async fn an_agent_gone_on_its_own_is_told_closed() {
    let mut w = Watch::new(true).await;
    w.tick(10).await;
    w.changes();
    w.tmux.sessions.lock().unwrap().remove(&w.session);
    w.tick(20).await;
    assert_eq!(w.changes(), [json!({ "change": "closed", "idle": false })]);
    w.tick(30).await;
    assert_eq!(w.changes(), Vec::<Value>::new(), "told once");
}

#[tokio::test]
async fn closing_it_tells_every_tab() {
    let mut w = Watch::new(true).await;
    w.changes();
    w.uc.close(&alice()).unwrap();
    assert_eq!(w.changes(), [json!({ "change": "closed", "idle": false })]);
}

#[tokio::test]
async fn what_it_writes_after_it_was_last_seen_is_a_new_answer() {
    let at = |secs: u64| SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000 + secs);
    let mut w = Watch::new(false).await;
    w.tmux
        .outputs
        .lock()
        .unwrap()
        .insert(w.session.clone(), at(0));
    w.uc.open(&alice(), &deep(), None, false).await.unwrap();
    w.tick(5).await;
    assert_eq!(
        w.changes(),
        [json!({ "change": "opened" })],
        "what it wrote before is no answer"
    );

    w.tmux
        .outputs
        .lock()
        .unwrap()
        .insert(w.session.clone(), at(7));
    w.tick(10).await;
    assert_eq!(
        w.changes(),
        [json!({ "change": "answered", "last_output_at": rfc(7) })]
    );
    assert_eq!(w.uc.status(&alice()).last_output_at, Some(rfc(7)));
    w.tick(15).await;
    assert_eq!(w.changes(), Vec::<Value>::new(), "told once");
}

#[tokio::test]
async fn a_change_reaches_only_who_may_read_its_session() {
    let root = tempfile::tempdir().unwrap();
    let bus = Arc::new(BroadcastEventBus::new(8));
    let mut events = bus.subscribe();
    let uc = support(
        root.path(),
        Arc::new(FakeTmux::default()),
        Host::allowing(),
        None,
    )
    .with_events(bus);
    uc.open(&alice(), &deep(), None, false).await.unwrap();
    let message = events.try_recv().unwrap();
    let session = support_session_name_of(&alice());
    assert_eq!(message["read_guard"]["resource"], "session");
    assert_eq!(message["read_guard"]["verb"], "read");
    assert_eq!(message["read_guard"]["target"], session.as_str());
}

#[tokio::test]
async fn a_focus_report_or_an_answer_to_the_harness_is_no_input() {
    let w = Watch::new(true).await;
    w.clock.set(1000);
    w.uc.note_input(&w.session, b"\x1b[I");
    w.uc.note_input(&w.session, b"\x1b[12;40R");
    assert_eq!(w.uc.status(&alice()).last_input_at, Some(rfc(0)));
    w.uc.note_input(&w.session, b"\x1b[A");
    assert_eq!(w.uc.status(&alice()).last_input_at, Some(rfc(1000)));
}

#[tokio::test(flavor = "multi_thread")]
async fn an_agent_opened_while_a_pass_lists_the_sessions_is_not_taken_for_gone() {
    let mut w = Watch::new(false).await;
    let uc = w.uc.clone();
    *w.tmux.on_next_list.lock().unwrap() = Some(Box::new(move || {
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(uc.open(&alice(), &deep(), None, false))
                .unwrap();
        });
    }));
    w.tick(1).await;
    assert_eq!(
        w.changes(),
        [json!({ "change": "opened" })],
        "never told closed"
    );
    assert!(
        w.uc.status(&alice()).last_input_at.is_some(),
        "its clock runs"
    );
    w.tick(6).await;
    assert_eq!(w.changes(), Vec::<Value>::new());
}

/// A bus that reads, as each change is told, whether the record names an
/// agent: what a tab reading the agent then would find.
struct RecordAtTelling {
    record: std::path::PathBuf,
    named: Mutex<Vec<bool>>,
    tx: broadcast::Sender<Value>,
}

impl EventBus for RecordAtTelling {
    fn publish(&self, _event: Value) {
        let named = std::fs::read_to_string(&self.record)
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .is_some_and(|record| !record["agent"].is_null());
        self.named.lock().unwrap().push(named);
    }

    fn subscribe(&self) -> broadcast::Receiver<Value> {
        self.tx.subscribe()
    }
}

#[tokio::test]
async fn a_tab_told_it_opened_reads_its_agent() {
    let root = tempfile::tempdir().unwrap();
    let session = support_session_name_of(&alice());
    let bus = Arc::new(RecordAtTelling {
        record: root.path().join(&session).join(".support.json"),
        named: Mutex::default(),
        tx: broadcast::channel(1).0,
    });
    let uc = support(
        root.path(),
        Arc::new(FakeTmux::default()),
        Host::allowing(),
        None,
    )
    .with_events(bus.clone());
    uc.open(&alice(), &deep(), None, false).await.unwrap();
    assert_eq!(*bus.named.lock().unwrap(), [true]);
}
