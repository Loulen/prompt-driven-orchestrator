//! The sessions the extensions are told about (CORE-61): who started each live
//! session, who takes control of its terminal, and when it ends.
//!
//! A session ends in many ways (its work is done, it is stopped, reaped by the
//! sweep, its harness exits, its tmux server dies), and most of them do not go
//! through the daemon. So the end is **observed**: [`SessionLedger::reconcile`]
//! compares the sessions the ledger knows with those tmux still has, and every
//! session gone is announced ended. The composition root runs it every
//! [`RECONCILE_INTERVAL`].
//!
//! A session of a Run is announced on the bus (hence `/ws`, to the connections
//! that may read the Run, and every extension) once an extension is loaded;
//! the library assistant's, which belongs to no Run, only to the extensions.
//! Without an extension, nothing is announced anywhere, and the ledger keeps
//! nothing: it never lists tmux.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pdo_extension_api::{SessionChange, SessionEvent, SessionRef};
use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::Actor;
use tracing::warn;

use crate::application::ports::Tmux;
use crate::domain::names::session_ref;

/// How often the ledger looks for the sessions that ended.
pub const RECONCILE_INTERVAL: Duration = Duration::from_secs(2);

struct Entry {
    /// Which start of the session this is: a session killed and started again
    /// under the same name between two reconciliations is two sessions.
    generation: u64,
    session: SessionRef,
    started_by: Option<Actor>,
}

#[derive(Default)]
struct Live {
    next: u64,
    sessions: HashMap<String, Entry>,
}

/// The live sessions, and who started each.
pub struct SessionLedger {
    tmux: Arc<dyn Tmux>,
    host: Arc<dyn ExtensionHost>,
    live: Mutex<Live>,
}

impl SessionLedger {
    pub fn new(tmux: Arc<dyn Tmux>, host: Arc<dyn ExtensionHost>) -> Self {
        Self {
            tmux,
            host,
            live: Mutex::new(Live::default()),
        }
    }

    /// `session` just came up, started by `started_by`: announce it. A session
    /// of the same name the ledger still knew ended before it.
    pub fn started(&self, session: SessionRef, started_by: Option<Actor>) {
        if !self.listened() {
            return;
        }
        let replaced = {
            let mut live = self.lock();
            live.next += 1;
            let entry = Entry {
                generation: live.next,
                session: session.clone(),
                started_by: started_by.clone(),
            };
            live.sessions.insert(session.id.clone(), entry)
        };
        if let Some(old) = replaced {
            self.announce(SessionChange::Ended, old.started_by, old.session);
        }
        self.announce(SessionChange::Started, started_by, session);
    }

    /// [`Self::started`], for the session named `name`; nothing for a name PDO
    /// does not give.
    pub fn started_named(&self, name: &str, started_by: Option<Actor>) {
        match session_ref(name) {
            Some(session) => self.started(session, started_by),
            None => warn!("session {name}: not a name PDO gives, its start is not announced"),
        }
    }

    /// `actor` took control of the terminal of the session `name`: it attached
    /// and drives it alone, or took the hand from the pilot.
    pub fn taken_over(&self, actor: &Actor, name: &str) {
        if let Some(session) = session_ref(name) {
            self.announce(SessionChange::TakenOver, Some(actor.clone()), session);
        }
    }

    /// Know the sessions alive now that the ledger does not know (a previous
    /// daemon started them), so their end is announced too, with nobody known
    /// as who started them. Blocking: it lists tmux.
    pub fn adopt_live(&self) {
        if !self.listened() {
            return;
        }
        let alive = self.tmux.list_pdo_sessions();
        let mut live = self.lock();
        for name in alive {
            if live.sessions.contains_key(&name) {
                continue;
            }
            if let Some(session) = session_ref(&name) {
                live.next += 1;
                let entry = Entry {
                    generation: live.next,
                    session,
                    started_by: None,
                };
                live.sessions.insert(name, entry);
            }
        }
    }

    /// Announce the end of every known session tmux no longer has. Blocking:
    /// it lists tmux, and only when the ledger knows a session.
    ///
    /// The ledger is read **before** tmux: a session is known only once it
    /// came up, so a known session missing from the list that follows is gone,
    /// never one not born yet. A session the list misses is checked once more
    /// on its own, so a failed list announces nothing.
    pub fn reconcile(&self) {
        let known: Vec<(String, u64)> = self
            .lock()
            .sessions
            .iter()
            .map(|(name, entry)| (name.clone(), entry.generation))
            .collect();
        if known.is_empty() {
            return;
        }
        let alive = self.tmux.list_pdo_sessions();
        for (name, generation) in known {
            if alive.contains(&name) || self.tmux.session_exists(&name) {
                continue;
            }
            let ended = {
                let mut live = self.lock();
                match live.sessions.get(&name) {
                    Some(entry) if entry.generation == generation => live.sessions.remove(&name),
                    _ => None,
                }
            };
            if let Some(entry) = ended {
                self.announce(SessionChange::Ended, entry.started_by, entry.session);
            }
        }
    }

    /// Who started the live session `name`, as far as the ledger knows.
    pub fn started_by(&self, name: &str) -> Option<Actor> {
        self.lock()
            .sessions
            .get(name)
            .and_then(|entry| entry.started_by.clone())
    }

    /// [`Self::reconcile`] every `every`, off the async runtime's threads, for
    /// as long as the daemon runs.
    pub async fn watch(self: Arc<Self>, every: Duration) {
        let mut tick = tokio::time::interval(every);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            let ledger = self.clone();
            if let Err(e) = tokio::task::spawn_blocking(move || ledger.reconcile()).await {
                warn!("the reconciliation of the sessions panicked: {e}");
            }
        }
    }

    /// Whether an extension is there to be told.
    fn listened(&self) -> bool {
        !self.host.installed().is_empty()
    }

    fn announce(&self, change: SessionChange, actor: Option<Actor>, session: SessionRef) {
        let in_a_run = session.run_id.is_some();
        let event = SessionEvent::new(change, actor, session).to_event();
        if in_a_run {
            self.host.announce(event);
        } else {
            self.host.publish(&event);
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Live> {
        match self.live.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}
