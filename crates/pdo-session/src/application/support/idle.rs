//! The idle watch of the Agents de support: each live agent's clock, started
//! over by its person's input; « Still there? » once the instance's idle
//! timeout runs out; the agent closed when nobody answers. And what changed
//! of an agent, told on the bus for every tab of its Actor.

use std::collections::HashSet;
use std::sync::atomic::Ordering;
use std::time::{Duration, SystemTime};

use chrono::{DateTime, Utc};
use pdo_kernel::authorization::authorize;
use pdo_kernel::event_bus::guarded;
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use tracing::{info, warn};

use super::{Support, SupportError, ASK};
use crate::domain::names::{support_session_name_of, SUPPORT_SESSION_PREFIX};
use crate::domain::support_idle::{
    is_terminal_reply, rfc3339, IdleStep, Idleness, SupportChange, SUPPORT_AGENT_CHANGED,
};

/// How often the watch looks at the agents.
pub const IDLE_WATCH_INTERVAL: Duration = Duration::from_secs(5);

/// What the watch knows of one live agent.
pub(super) struct Watched {
    pub(super) idleness: Idleness,
    /// The last output seen: a later one is a new answer.
    pub(super) last_output: Option<SystemTime>,
    /// The timeout « Still there? » was asked after, in seconds.
    pub(super) asked_after_secs: Option<u32>,
    /// Which start of a clock this is: a pass of the watch that listed the
    /// sessions before it started does not take it for gone.
    started: u64,
}

impl Watched {
    fn new(at: DateTime<Utc>, last_output: Option<SystemTime>, started: u64) -> Self {
        Self {
            idleness: Idleness::new(at),
            last_output,
            asked_after_secs: None,
            started,
        }
    }
}

impl Support {
    pub(super) fn watched(
        &self,
    ) -> std::sync::MutexGuard<'_, std::collections::HashMap<String, Watched>> {
        match self.idle.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    /// The agent `session` opened now: its clock starts, and what it wrote so
    /// far is no new answer.
    pub(super) fn start_clock(&self, session: &str) {
        let started = self.starts.fetch_add(1, Ordering::SeqCst) + 1;
        let watched = Watched::new(self.clock.now(), self.tmux.last_output_at(session), started);
        self.watched().insert(session.to_string(), watched);
        self.tell(session, SupportChange::Opened);
    }

    /// End the agent `session`, whoever asks: its tmux session, its clock,
    /// the agent its record names (its Actor's last choice stays). Whether it
    /// was alive.
    pub(super) fn end(&self, session: &str) -> bool {
        let existed = self.tmux.session_exists(session);
        self.tmux.kill(session);
        self.watched().remove(session);
        let mut record = self.record(session);
        if record.agent.take().is_some() {
            if let Err(e) = self.write_record(session, &record) {
                warn!("Support agent {session}: its record was not written: {e}");
            }
        }
        if existed && self.tmux.session_exists(session) {
            warn!("Failed to close support agent {session}: the tmux session is still alive");
        }
        existed
    }

    /// Its person typed into the terminal of `session`: when that is an Agent
    /// de support, its clock starts over. Any other session, nothing.
    /// `bytes` the terminal sends on its own (focus reports, answers to the
    /// harness's queries) are nobody's input.
    pub fn note_input(&self, session: &str, bytes: &[u8]) {
        if session.starts_with(SUPPORT_SESSION_PREFIX) && !is_terminal_reply(bytes) {
            self.input(session);
        }
    }

    /// Input reached `session` now. When it answered « Still there? », every
    /// tab hears it.
    pub(super) fn input(&self, session: &str) {
        let now = self.clock.now();
        // Told under the lock: a pass of the watch asking at the same moment
        // tells its question before this answer, never after.
        let mut watched = self.watched();
        let entry = watched
            .entry(session.to_string())
            .or_insert_with(|| Watched::new(now, None, 0));
        entry.asked_after_secs = None;
        if entry.idleness.input(now) {
            self.tell(session, SupportChange::Active);
        }
    }

    /// « Keep »: the agent of `actor` stays, its clock starts over. An input
    /// from its person, so the Operation of a question: `session` × `ask` on
    /// its session.
    pub fn keep(&self, actor: &Actor) -> Result<String, SupportError> {
        let session = support_session_name_of(actor);
        let operation =
            Operation::new(ResourceKind::Session, Verb::Action(ASK.into())).on(session.clone());
        authorize(self.host.as_ref(), actor, &operation).map_err(SupportError::Refused)?;
        if !self.tmux.session_exists(&session) {
            return Err(SupportError::NotOpen);
        }
        self.input(&session);
        Ok(session)
    }

    /// One pass of the watch: the live agents' clocks at the instance's idle
    /// timeout. An agent that outlived a daemon restart starts its clock now;
    /// one gone on its own is told closed; one idle asks « Still there? »; one
    /// that went unanswered closes; one that wrote since the last pass is told
    /// answered.
    pub async fn watch_idle_once(&self) {
        let timeout = self.agents.idle_timeout().await;
        let timeout_secs = u32::try_from(timeout.as_secs()).unwrap_or(u32::MAX);
        // The clocks started from here on belong to sessions the list below
        // may not hold yet.
        let listed_after = self.starts.load(Ordering::SeqCst);
        let live: HashSet<String> = self
            .tmux
            .list_pdo_sessions()
            .into_iter()
            .filter(|name| name.starts_with(SUPPORT_SESSION_PREFIX))
            .collect();
        let outputs: Vec<(String, Option<SystemTime>)> = live
            .iter()
            .map(|name| (name.clone(), self.tmux.last_output_at(name)))
            .collect();
        let now = self.clock.now();
        let mut changes = Vec::new();
        let mut to_close = Vec::new();
        {
            let mut watched = self.watched();
            let gone: Vec<String> = watched
                .iter()
                .filter(|(name, w)| !live.contains(*name) && w.started <= listed_after)
                .map(|(name, _)| name.clone())
                .collect();
            for name in gone {
                watched.remove(&name);
                changes.push((name, SupportChange::Closed { idle: false }));
            }
            for (name, output) in outputs {
                let entry = watched
                    .entry(name.clone())
                    .or_insert_with(|| Watched::new(now, output, 0));
                if let Some(output) = output {
                    if entry.last_output.is_some_and(|seen| output > seen) {
                        changes.push((
                            name.clone(),
                            SupportChange::Answered {
                                last_output_at: rfc3339(output.into()),
                            },
                        ));
                    }
                    if entry.last_output.is_none_or(|seen| output > seen) {
                        entry.last_output = Some(output);
                    }
                }
                match entry.idleness.step(now, timeout) {
                    IdleStep::Ask { closes_at } => {
                        entry.asked_after_secs = Some(timeout_secs);
                        changes.push((
                            name,
                            SupportChange::Idle {
                                closes_at: rfc3339(closes_at),
                                closes_in_secs: entry.idleness.closes_in_secs(now).unwrap_or(0),
                                idle_timeout_secs: timeout_secs,
                            },
                        ));
                    }
                    IdleStep::Close => to_close.push(name),
                    IdleStep::Wait => {}
                }
            }
            // Told under the lock: an input that answers the question at the
            // same moment tells its answer after it.
            for (name, change) in changes {
                self.tell(&name, change);
            }
        }
        for name in to_close {
            // A Keep or a keystroke may have answered since: then it stays.
            if !self.still_unanswered(&name) {
                continue;
            }
            if self.end(&name) {
                info!("Closed support agent {name}: nobody answered « Still there? »");
            }
            self.tell(&name, SupportChange::Closed { idle: true });
        }
    }

    /// Whether « Still there? » is still asked of `session` and its time ran
    /// out; then its clock is dropped, so no input can answer it any more.
    fn still_unanswered(&self, session: &str) -> bool {
        let now = self.clock.now();
        let mut watched = self.watched();
        let due = watched
            .get(session)
            .and_then(|w| w.idleness.closes_at())
            .is_some_and(|at| now >= at);
        if due {
            watched.remove(session);
        }
        due
    }

    /// Run the watch every `every`, for as long as the daemon serves.
    pub async fn watch_idle(&self, every: Duration) {
        let mut tick = tokio::time::interval(every);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            self.watch_idle_once().await;
        }
    }

    /// Tell what changed of the agent `session`, on the bus: `/ws` relays it
    /// to the connections that may read its session.
    pub(super) fn tell(&self, session: &str, change: SupportChange) {
        let read = Operation::new(ResourceKind::Session, Verb::Read).on(session);
        let message = serde_json::json!({
            "type": SUPPORT_AGENT_CHANGED,
            "support_session": session,
            "support": change,
        });
        self.events.publish(guarded(message, &read));
    }
}
