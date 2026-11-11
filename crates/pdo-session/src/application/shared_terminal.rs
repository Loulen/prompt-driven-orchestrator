//! The shared terminal's presence registry (#867, ADR-0075): per tmux session,
//! the live PTY-bridge connections (their poste, their arrival order, their
//! last size) and the pilot poste. It decides roles; the bridge applies them —
//! a spectator's client carries `ignore-size`, flipped live
//! ([`ignore_size`](super::ignore_size)), and its input frames are dropped. It
//! also answers who pilots a terminal, for the gestures that write into it
//! without going through the bridge.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use pdo_kernel::Actor;
use tokio::sync::{mpsc, watch};

use crate::domain::shared_terminal::{ConnId, Role, RoleMsg, TermSize, INITIAL_SIZE};

/// What the bridge needs to act on a connection's role.
pub struct Membership {
    pub id: ConnId,
    /// `true` while the connection is a spectator: the bridge drops its input.
    pub spectator: Arc<AtomicBool>,
    /// Whether the connection's tmux client must carry `ignore-size`. The bridge
    /// reads it once to attach, then follows its changes (see
    /// [`follow_ignore_size`]).
    pub ignore_size: watch::Receiver<bool>,
    /// Whether joining put the hand on this connection's poste: the first to
    /// attach (a solo terminal), never a spectator, nor a second tab of the
    /// poste that already drove (CORE-61).
    pub took_the_hand: bool,
}

/// The pilot of a shared terminal: its poste, and the Actor of its latest
/// connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pilot {
    pub poste: String,
    pub actor: Actor,
}

struct Connection {
    id: ConnId,
    poste: String,
    actor: Actor,
    /// Arrival order, daemon-wide and monotonic.
    arrived: u64,
    size: TermSize,
    /// When `size` last changed, on the same clock as `arrived`.
    resized: u64,
    role_tx: mpsc::UnboundedSender<String>,
    last_sent: Option<RoleMsg>,
    spectator: Arc<AtomicBool>,
    ignore_size: watch::Sender<bool>,
    /// The sentence of the refusal that keeps it from ever driving the
    /// terminal; `None` when it may.
    read_only: Option<String>,
}

impl Connection {
    fn may_drive(&self) -> bool {
        self.read_only.is_none()
    }
}

#[derive(Default)]
struct SessionPresence {
    connections: Vec<Connection>,
    /// `None` while the terminal is solo.
    pilot: Option<String>,
}

impl SessionPresence {
    /// Postes present, first arrived first. A poste arrives with its oldest live
    /// connection: a tab that closes and reopens keeps the poste's rank as long
    /// as another tab of it stayed.
    fn postes(&self) -> Vec<&str> {
        let mut firsts: Vec<(&str, u64)> = Vec::new();
        for c in &self.connections {
            match firsts.iter_mut().find(|(p, _)| *p == c.poste) {
                Some(entry) => entry.1 = entry.1.min(c.arrived),
                None => firsts.push((c.poste.as_str(), c.arrived)),
            }
        }
        firsts.sort_by_key(|(_, arrived)| *arrived);
        firsts.into_iter().map(|(p, _)| p).collect()
    }

    /// The postes that may drive, first arrived first: those with a
    /// connection that is not read-only.
    fn drivers(&self) -> Vec<&str> {
        self.postes()
            .into_iter()
            .filter(|poste| {
                self.connections
                    .iter()
                    .any(|c| c.poste == *poste && c.may_drive())
            })
            .collect()
    }

    /// The latest connection of `poste` that may drive.
    fn latest_driving(&self, poste: &str) -> Option<&Connection> {
        self.connections
            .iter()
            .filter(|c| c.poste == poste && c.may_drive())
            .max_by_key(|c| c.arrived)
    }

    /// The pilot, with the Actor of its latest connection; `None` while solo.
    fn pilot(&self) -> Option<Pilot> {
        let poste = self.pilot.as_ref()?;
        let latest = self.latest_driving(poste)?;
        Some(Pilot {
            poste: poste.clone(),
            actor: latest.actor.clone(),
        })
    }

    /// The poste whose keystrokes reach the pane: the pilot, or the one poste
    /// of a solo terminal when it may drive.
    fn driver(&self) -> Option<String> {
        if self.pilot.is_some() {
            return self.pilot.clone();
        }
        match (self.postes().as_slice(), self.drivers().as_slice()) {
            ([only], [driver]) if only == driver => Some(only.to_string()),
            _ => None,
        }
    }

    /// Re-derive the pilot and push to every connection what changed for it.
    fn settle(&mut self) {
        let postes: Vec<String> = self.postes().into_iter().map(str::to_string).collect();
        let drivers: Vec<String> = self.drivers().into_iter().map(str::to_string).collect();
        if postes.len() <= 1 {
            self.pilot = None;
        } else if !self
            .pilot
            .as_ref()
            .is_some_and(|p| drivers.iter().any(|q| q == p))
        {
            // The pilot left (or there was none yet): the hand goes to the first
            // arrived of the postes still here that may drive; none among
            // read-only postes only.
            self.pilot = drivers.first().cloned();
        }

        // The poste whose grid the others render: the pilot; among read-only
        // postes only, the first arrived; none while solo.
        let leader = match &self.pilot {
            Some(pilot) => Some(pilot.clone()),
            None if postes.len() > 1 => postes.first().cloned(),
            None => None,
        };
        let piloted = self.pilot.is_some();
        let leader_size = leader.as_ref().and_then(|leader| {
            self.connections
                .iter()
                .filter(|c| &c.poste == leader && (c.may_drive() || !piloted))
                .max_by_key(|c| c.resized)
                .map(|c| c.size)
        });
        let spectators = postes.len().saturating_sub(1);

        for c in &mut self.connections {
            let read_only = c.read_only.clone();
            let msg = match (&leader, leader_size) {
                (Some(pilot), _) if *pilot == c.poste && piloted && read_only.is_none() => {
                    RoleMsg::Pilot { spectators }
                }
                (Some(leader), _) if *leader == c.poste && !piloted => RoleMsg::Solo { read_only },
                (Some(_), Some(size)) => RoleMsg::Spectator {
                    pilot: size,
                    read_only,
                },
                _ => RoleMsg::Solo { read_only },
            };
            let spectator = msg.role() == Role::Spectator;
            // A read-only connection's keystrokes are dropped, whatever its role.
            c.spectator
                .store(spectator || !c.may_drive(), Ordering::SeqCst);
            c.ignore_size.send_if_modified(|current| {
                let changed = *current != spectator;
                *current = spectator;
                changed
            });
            if c.last_sent.as_ref() != Some(&msg) {
                // A closed receiver means the socket is going away; its `leave`
                // is on its way.
                let _ = c.role_tx.send(msg.to_frame());
                c.last_sent = Some(msg);
            }
        }
    }
}

#[derive(Default)]
struct Inner {
    clock: u64,
    sessions: HashMap<String, SessionPresence>,
}

impl Inner {
    fn tick(&mut self) -> u64 {
        self.clock += 1;
        self.clock
    }
}

/// Per-daemon presence registry of the terminal sockets (one per daemon, in the session context).
#[derive(Default)]
pub struct SharedTerminalRegistry {
    inner: Mutex<Inner>,
}

impl SharedTerminalRegistry {
    /// Register a new connection of `actor` on `session`. `poste: None` (no or
    /// invalid query parameter) makes the socket a poste of its own. Role
    /// frames for this connection are sent on `role_tx`, starting with its
    /// first role.
    pub fn join(
        &self,
        session: &str,
        poste: Option<String>,
        actor: Actor,
        role_tx: mpsc::UnboundedSender<String>,
    ) -> Membership {
        self.enter(session, poste, actor, role_tx, None)
    }

    /// Register a read-only connection of `actor` on `session`, as
    /// [`Self::join`] does: it may read the session but not attach to it, and
    /// `why` is the sentence of that refusal. It never drives the terminal nor
    /// takes control of it; its keystrokes are dropped.
    pub fn join_read_only(
        &self,
        session: &str,
        poste: Option<String>,
        actor: Actor,
        role_tx: mpsc::UnboundedSender<String>,
        why: String,
    ) -> Membership {
        self.enter(session, poste, actor, role_tx, Some(why))
    }

    fn enter(
        &self,
        session: &str,
        poste: Option<String>,
        actor: Actor,
        role_tx: mpsc::UnboundedSender<String>,
        read_only: Option<String>,
    ) -> Membership {
        let mut inner = self.inner.lock().unwrap();
        let id = inner.tick();
        let poste = poste.unwrap_or_else(|| format!("anonymous-socket-{id}"));
        let spectator = Arc::new(AtomicBool::new(false));
        let (ignore_tx, ignore_rx) = watch::channel(false);
        let presence = inner.sessions.entry(session.to_string()).or_default();
        let driver_before = presence.driver();
        presence.connections.push(Connection {
            id,
            poste: poste.clone(),
            actor,
            arrived: id,
            size: INITIAL_SIZE,
            resized: 0,
            role_tx,
            last_sent: None,
            spectator: spectator.clone(),
            ignore_size: ignore_tx,
            read_only,
        });
        presence.settle();
        let driver_after = presence.driver();
        Membership {
            id,
            spectator,
            ignore_size: ignore_rx,
            took_the_hand: driver_after.as_ref() == Some(&poste) && driver_before != driver_after,
        }
    }

    /// Who pilots `session`: `None` while no one shows it, or while a single
    /// poste does (solo).
    pub fn pilot(&self, session: &str) -> Option<Pilot> {
        self.inner.lock().unwrap().sessions.get(session)?.pilot()
    }

    /// Who drives `session`: the Actor of the latest connection of the poste
    /// whose keystrokes reach the pane (the pilot, or the one poste of a solo
    /// terminal), never a read-only one; `None` while no one does.
    pub fn driver(&self, session: &str) -> Option<Actor> {
        let inner = self.inner.lock().unwrap();
        let presence = inner.sessions.get(session)?;
        let poste = presence.driver()?;
        presence.latest_driving(&poste).map(|c| c.actor.clone())
    }

    /// The pilot of `session` when `poste` watches it as a spectator; `None`
    /// when `poste` pilots it, is alone on it, or does not show it.
    pub fn spectated_pilot(&self, session: &str, poste: &str) -> Option<Pilot> {
        let inner = self.inner.lock().unwrap();
        let presence = inner.sessions.get(session)?;
        if !presence.connections.iter().any(|c| c.poste == poste) {
            return None;
        }
        presence.pilot().filter(|pilot| pilot.poste != poste)
    }

    /// The connection resized its PTY. Only a pilot's size matters: spectators
    /// are told the new grid.
    pub fn resize(&self, session: &str, id: ConnId, size: TermSize) {
        let mut inner = self.inner.lock().unwrap();
        let now = inner.tick();
        let Some(presence) = inner.sessions.get_mut(session) else {
            return;
        };
        let Some(conn) = presence.connections.iter_mut().find(|c| c.id == id) else {
            return;
        };
        conn.size = size;
        conn.resized = now;
        presence.settle();
    }

    /// A spectator takes control (#870): its poste becomes the pilot. Returns
    /// `false` (and changes nothing) when the connection is not a spectator —
    /// the pilot's own request, a solo terminal, an unknown connection — or
    /// is read-only.
    pub fn take_control(&self, session: &str, id: ConnId) -> bool {
        let mut inner = self.inner.lock().unwrap();
        let Some(presence) = inner.sessions.get_mut(session) else {
            return false;
        };
        let Some(conn) = presence
            .connections
            .iter()
            .find(|c| c.id == id && c.may_drive())
        else {
            return false;
        };
        let spectator = presence
            .pilot
            .as_ref()
            .is_some_and(|pilot| *pilot != conn.poste);
        if !spectator {
            return false;
        }
        presence.pilot = Some(conn.poste.clone());
        presence.settle();
        true
    }

    /// The connection closed. The hand passes on if it was the pilot's last one.
    pub fn leave(&self, session: &str, id: ConnId) {
        let mut inner = self.inner.lock().unwrap();
        let Some(presence) = inner.sessions.get_mut(session) else {
            return;
        };
        presence.connections.retain(|c| c.id != id);
        if presence.connections.is_empty() {
            inner.sessions.remove(session);
        } else {
            presence.settle();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Probe {
        m: Membership,
        rx: mpsc::UnboundedReceiver<String>,
    }

    impl Probe {
        fn frames(&mut self) -> Vec<serde_json::Value> {
            let mut out = Vec::new();
            while let Ok(f) = self.rx.try_recv() {
                out.push(serde_json::from_str(&f).unwrap());
            }
            out
        }
        fn last(&mut self) -> Option<serde_json::Value> {
            self.frames().pop()
        }
        fn spectator(&self) -> bool {
            self.m.spectator.load(Ordering::SeqCst)
        }
        fn ignore_size(&self) -> bool {
            *self.m.ignore_size.borrow()
        }
    }

    fn join(reg: &SharedTerminalRegistry, poste: Option<&str>) -> Probe {
        let (tx, rx) = mpsc::unbounded_channel();
        let m = reg.join("s", poste.map(str::to_string), Actor::LocalOperator, tx);
        Probe { m, rx }
    }

    fn size(cols: u16, rows: u16) -> TermSize {
        TermSize { cols, rows }
    }

    #[test]
    fn a_lone_poste_is_solo() {
        let reg = SharedTerminalRegistry::default();
        let mut a = join(&reg, Some("A"));
        assert_eq!(a.last().unwrap()["role"], "solo");
        assert!(!a.spectator());
        assert!(!a.ignore_size());
    }

    fn join_as(reg: &SharedTerminalRegistry, poste: &str, actor: Actor) -> Probe {
        let (tx, rx) = mpsc::unbounded_channel();
        let m = reg.join("s", Some(poste.to_string()), actor, tx);
        Probe { m, rx }
    }

    fn named(id: &str) -> Actor {
        Actor::Identified {
            namespace: "test".into(),
            id: id.into(),
        }
    }

    #[test]
    fn the_driver_is_the_lone_poste_then_the_pilot_and_nobody_once_all_left() {
        let reg = SharedTerminalRegistry::default();
        assert_eq!(reg.driver("s"), None, "nobody shows it");
        let alice = join_as(&reg, "A", named("alice"));
        assert_eq!(reg.driver("s"), Some(named("alice")), "solo: its one poste");
        let bob = join_as(&reg, "B", named("bob"));
        assert_eq!(
            reg.driver("s"),
            Some(named("alice")),
            "the first arrived pilots"
        );
        assert!(reg.take_control("s", bob.m.id));
        assert_eq!(reg.driver("s"), Some(named("bob")));
        reg.leave("s", bob.m.id);
        assert_eq!(reg.driver("s"), Some(named("alice")));
        reg.leave("s", alice.m.id);
        assert_eq!(reg.driver("s"), None);
    }

    #[test]
    fn second_poste_is_spectator_of_the_first_arrived() {
        let reg = SharedTerminalRegistry::default();
        let mut a = join(&reg, Some("A"));
        reg.resize("s", a.m.id, size(200, 50));
        let mut b = join(&reg, Some("B"));

        let fa = a.last().unwrap();
        assert_eq!(fa["role"], "pilot");
        assert_eq!(fa["spectators"], 1);
        let fb = b.last().unwrap();
        assert_eq!(fb["role"], "spectator");
        assert_eq!(fb["pilot"]["cols"], 200);
        assert!(b.spectator() && b.ignore_size());
        assert!(!a.spectator() && !a.ignore_size());
    }

    #[test]
    fn spectator_hears_pilot_resizes_only() {
        let reg = SharedTerminalRegistry::default();
        let mut a = join(&reg, Some("A"));
        let mut b = join(&reg, Some("B"));
        a.frames();
        b.frames();

        reg.resize("s", b.m.id, size(90, 30));
        assert!(a.frames().is_empty(), "a spectator resize changes nothing");
        assert!(b.frames().is_empty());

        reg.resize("s", a.m.id, size(150, 40));
        assert!(a.frames().is_empty(), "the pilot's own role is unchanged");
        let fb = b.last().unwrap();
        assert_eq!(fb["pilot"]["cols"], 150);
        assert_eq!(fb["pilot"]["rows"], 40);
    }

    #[test]
    fn pilot_leaving_hands_over_to_first_arrived_then_solo() {
        let reg = SharedTerminalRegistry::default();
        let a = join(&reg, Some("A"));
        let mut b = join(&reg, Some("B"));
        let mut c = join(&reg, Some("C"));
        reg.resize("s", b.m.id, size(90, 30));

        reg.leave("s", a.m.id);
        let fb = b.last().unwrap();
        assert_eq!(fb["role"], "pilot");
        assert_eq!(fb["spectators"], 1);
        let fc = c.last().unwrap();
        assert_eq!(fc["role"], "spectator");
        assert_eq!(fc["pilot"]["cols"], 90);
        assert!(!b.ignore_size());

        reg.leave("s", b.m.id);
        assert_eq!(c.last().unwrap()["role"], "solo");
        assert!(!c.spectator() && !c.ignore_size());
    }

    #[test]
    fn two_sockets_of_the_same_poste_stay_solo() {
        let reg = SharedTerminalRegistry::default();
        let mut a1 = join(&reg, Some("A"));
        let mut a2 = join(&reg, Some("A"));
        assert_eq!(a1.last().unwrap()["role"], "solo");
        assert_eq!(a2.last().unwrap()["role"], "solo");
        assert!(!a2.spectator());
    }

    #[test]
    fn every_tab_of_the_pilot_poste_pilots() {
        let reg = SharedTerminalRegistry::default();
        let mut a1 = join(&reg, Some("A"));
        let mut b = join(&reg, Some("B"));
        let mut a2 = join(&reg, Some("A"));
        assert_eq!(a1.last().unwrap()["role"], "pilot");
        assert_eq!(a2.last().unwrap()["role"], "pilot");
        assert_eq!(b.last().unwrap()["role"], "spectator");
        // The pilot's latest resize is the grid shown to the spectator.
        reg.resize("s", a2.m.id, size(100, 20));
        assert_eq!(b.last().unwrap()["pilot"]["cols"], 100);
        // One tab of the pilot leaving keeps the hand with the poste.
        reg.leave("s", a1.m.id);
        assert!(b.last().is_none());
        assert_eq!(a2.frames().len(), 0);
    }

    #[test]
    fn sockets_without_poste_are_postes_of_their_own() {
        let reg = SharedTerminalRegistry::default();
        let mut x = join(&reg, None);
        assert_eq!(x.last().unwrap()["role"], "solo");
        let mut y = join(&reg, None);
        assert_eq!(x.last().unwrap()["role"], "pilot");
        assert_eq!(y.last().unwrap()["role"], "spectator");
    }

    #[test]
    fn a_spectator_takes_control_and_the_former_pilot_watches() {
        let reg = SharedTerminalRegistry::default();
        let mut a = join(&reg, Some("A"));
        reg.resize("s", a.m.id, size(200, 50));
        let mut b = join(&reg, Some("B"));
        reg.resize("s", b.m.id, size(90, 30));
        a.frames();
        b.frames();

        assert!(reg.take_control("s", b.m.id));
        let fb = b.last().unwrap();
        assert_eq!(fb["role"], "pilot");
        assert_eq!(fb["spectators"], 1);
        let fa = a.last().unwrap();
        assert_eq!(fa["role"], "spectator");
        assert_eq!(fa["pilot"]["cols"], 90);
        assert!(!b.spectator() && !b.ignore_size());
        assert!(a.spectator() && a.ignore_size());

        // The new pilot's resizes are the grid the former one now follows.
        reg.resize("s", b.m.id, size(100, 35));
        assert_eq!(a.last().unwrap()["pilot"]["cols"], 100);
        // And the hand can go back.
        assert!(reg.take_control("s", a.m.id));
        assert_eq!(a.last().unwrap()["role"], "pilot");
        assert_eq!(b.last().unwrap()["role"], "spectator");
    }

    #[test]
    fn take_control_from_the_pilot_or_solo_changes_nothing() {
        let reg = SharedTerminalRegistry::default();
        let mut a = join(&reg, Some("A"));
        a.frames();
        assert!(!reg.take_control("s", a.m.id), "solo");
        assert!(a.frames().is_empty());

        let mut b = join(&reg, Some("B"));
        a.frames();
        b.frames();
        assert!(!reg.take_control("s", a.m.id), "already the pilot");
        assert!(a.frames().is_empty() && b.frames().is_empty());
        assert!(b.spectator());
        assert!(!reg.take_control("s", 999), "unknown connection");
        assert!(!reg.take_control("other", b.m.id), "unknown session");
    }

    #[test]
    fn taking_control_keeps_the_hand_when_a_third_poste_arrives_and_the_pilot_leaves() {
        let reg = SharedTerminalRegistry::default();
        let mut a = join(&reg, Some("A"));
        reg.resize("s", a.m.id, size(120, 40));
        let b = join(&reg, Some("B"));
        reg.resize("s", b.m.id, size(90, 30));
        assert!(reg.take_control("s", b.m.id));
        let mut c = join(&reg, Some("C"));
        assert_eq!(c.last().unwrap()["pilot"]["cols"], 90);
        // B leaves: the hand goes to the first arrived of the rest, A.
        reg.leave("s", b.m.id);
        assert_eq!(a.last().unwrap()["role"], "pilot");
        let fc = c.last().unwrap();
        assert_eq!(fc["role"], "spectator");
        assert_eq!(fc["pilot"]["cols"], 120);
        assert!(c.spectator() && !a.spectator());
    }

    #[test]
    fn every_tab_of_the_poste_that_takes_control_pilots() {
        let reg = SharedTerminalRegistry::default();
        let mut a = join(&reg, Some("A"));
        let mut b1 = join(&reg, Some("B"));
        let mut b2 = join(&reg, Some("B"));
        assert!(reg.take_control("s", b2.m.id));
        assert_eq!(b1.last().unwrap()["role"], "pilot");
        assert_eq!(b2.last().unwrap()["role"], "pilot");
        assert_eq!(a.last().unwrap()["role"], "spectator");
        assert!(!b1.spectator() && !b1.ignore_size());
    }

    #[test]
    fn taking_control_of_one_session_leaves_the_other_alone() {
        let reg = SharedTerminalRegistry::default();
        let mut a = join(&reg, Some("A"));
        let b = join(&reg, Some("B"));
        let (ta, mut rxa) = mpsc::unbounded_channel();
        let other_a = reg.join("other", Some("A".into()), Actor::LocalOperator, ta);
        let (tb, mut rxb) = mpsc::unbounded_channel();
        let other_b = reg.join("other", Some("B".into()), Actor::LocalOperator, tb);
        while rxa.try_recv().is_ok() {}
        while rxb.try_recv().is_ok() {}

        assert!(reg.take_control("s", b.m.id));
        assert_eq!(a.last().unwrap()["role"], "spectator");
        assert!(rxa.try_recv().is_err() && rxb.try_recv().is_err());
        assert!(!other_a.spectator.load(Ordering::SeqCst));
        assert!(other_b.spectator.load(Ordering::SeqCst));
    }

    #[test]
    fn sessions_are_independent() {
        let reg = SharedTerminalRegistry::default();
        let mut a = join(&reg, Some("A"));
        let (tx, mut rx) = mpsc::unbounded_channel();
        reg.join("other", Some("B".into()), Actor::LocalOperator, tx);
        assert_eq!(a.last().unwrap()["role"], "solo");
        let f: serde_json::Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
        assert_eq!(f["role"], "solo");
    }

    #[test]
    fn only_the_first_to_attach_takes_the_hand_on_joining() {
        let reg = SharedTerminalRegistry::default();
        let a = join(&reg, Some("A"));
        assert!(a.m.took_the_hand, "a solo terminal is driven by its poste");
        let second_tab = join(&reg, Some("A"));
        assert!(!second_tab.m.took_the_hand, "the poste already drove");
        let b = join(&reg, Some("B"));
        assert!(!b.m.took_the_hand, "a spectator takes nothing");
        let c = join(&reg, None);
        assert!(!c.m.took_the_hand, "a third poste is a spectator too");
    }

    fn alice() -> Actor {
        Actor::Identified {
            namespace: "test".into(),
            id: "alice".into(),
        }
    }

    #[test]
    fn the_registry_says_who_pilots_a_terminal() {
        let reg = SharedTerminalRegistry::default();
        assert_eq!(reg.pilot("s"), None, "no one shows it");
        let (tx, _rx_a) = mpsc::unbounded_channel();
        let a = reg.join("s", Some("A".into()), alice(), tx);
        assert_eq!(reg.pilot("s"), None, "solo: no pilot");
        let b = join(&reg, Some("B"));
        let pilot = reg.pilot("s").unwrap();
        assert_eq!(pilot.poste, "A");
        assert_eq!(pilot.actor, alice());

        assert!(reg.take_control("s", b.m.id));
        assert_eq!(reg.pilot("s").unwrap().poste, "B");
        assert_eq!(reg.pilot("s").unwrap().actor, Actor::LocalOperator);
        reg.leave("s", a.id);
        assert_eq!(reg.pilot("s"), None, "back to solo");
    }

    #[test]
    fn only_a_poste_watching_as_spectator_has_a_spectated_pilot() {
        let reg = SharedTerminalRegistry::default();
        let (tx, _rx_a) = mpsc::unbounded_channel();
        let _a = reg.join("s", Some("A".into()), alice(), tx);
        assert_eq!(reg.spectated_pilot("s", "A"), None, "alone: solo");
        let _b = join(&reg, Some("B"));
        let pilot = reg.spectated_pilot("s", "B").unwrap();
        assert_eq!(pilot.poste, "A");
        assert_eq!(pilot.actor, alice());
        assert_eq!(reg.spectated_pilot("s", "A"), None, "the pilot itself");
        assert_eq!(
            reg.spectated_pilot("s", "C"),
            None,
            "a poste not showing it"
        );
        assert_eq!(reg.spectated_pilot("other", "B"), None, "another session");
    }

    const WHY: &str =
        "You can't attach this session here. Ask an administrator of the test extension.";

    fn watch(reg: &SharedTerminalRegistry, poste: &str) -> Probe {
        let (tx, rx) = mpsc::unbounded_channel();
        let m = reg.join_read_only("s", Some(poste.to_string()), named("rita"), tx, WHY.into());
        Probe { m, rx }
    }

    #[test]
    fn a_reader_alone_sizes_the_terminal_but_never_drives_it() {
        let reg = SharedTerminalRegistry::default();
        let mut w = watch(&reg, "W");
        let frame = w.last().unwrap();
        assert_eq!(frame["role"], "solo");
        assert_eq!(frame["read_only"], WHY);
        assert!(w.spectator(), "its keystrokes are dropped");
        assert!(!w.ignore_size(), "alone, its grid is the terminal's");
        assert!(!w.m.took_the_hand);
        assert_eq!(reg.driver("s"), None);
        assert_eq!(reg.pilot("s"), None);
        assert!(!reg.take_control("s", w.m.id));
    }

    #[test]
    fn a_reader_beside_a_pilot_watches_and_cannot_take_control() {
        let reg = SharedTerminalRegistry::default();
        let mut a = join_as(&reg, "A", named("alice"));
        reg.resize("s", a.m.id, size(120, 40));
        let mut w = watch(&reg, "W");
        let fa = a.last().unwrap();
        assert_eq!(fa["role"], "pilot");
        assert_eq!(fa["spectators"], 1);
        assert!(fa.get("read_only").is_none(), "{fa}");
        let fw = w.last().unwrap();
        assert_eq!(fw["role"], "spectator");
        assert_eq!(fw["pilot"]["cols"], 120);
        assert_eq!(fw["read_only"], WHY);
        assert!(w.spectator() && w.ignore_size());
        assert!(!reg.take_control("s", w.m.id));
        assert_eq!(reg.driver("s"), Some(named("alice")));
        assert!(w.last().is_none(), "nothing changed");
    }

    #[test]
    fn a_poste_that_may_drive_takes_the_hand_from_a_reader_there_first() {
        let reg = SharedTerminalRegistry::default();
        let mut w = watch(&reg, "W");
        let a = join_as(&reg, "A", named("alice"));
        assert!(a.m.took_the_hand);
        reg.resize("s", a.m.id, size(100, 30));
        assert_eq!(reg.driver("s"), Some(named("alice")));
        let fw = w.last().unwrap();
        assert_eq!(fw["role"], "spectator");
        assert_eq!(fw["pilot"]["cols"], 100);
        assert_eq!(fw["read_only"], WHY);
        assert!(w.ignore_size());
    }

    #[test]
    fn the_hand_never_passes_to_a_reader() {
        let reg = SharedTerminalRegistry::default();
        let a = join_as(&reg, "A", named("alice"));
        let mut w = watch(&reg, "W");
        let mut b = join_as(&reg, "B", named("bob"));
        reg.resize("s", b.m.id, size(90, 30));
        reg.leave("s", a.m.id);
        assert_eq!(b.last().unwrap()["role"], "pilot", "B, though W came first");
        assert_eq!(w.last().unwrap()["pilot"]["cols"], 90);
        assert_eq!(reg.driver("s"), Some(named("bob")));
        reg.leave("s", b.m.id);
        let fw = w.last().unwrap();
        assert_eq!(fw["role"], "solo");
        assert_eq!(fw["read_only"], WHY);
        assert!(w.spectator() && !w.ignore_size());
        assert_eq!(reg.driver("s"), None);
    }

    #[test]
    fn readers_alone_follow_the_first_arrived() {
        let reg = SharedTerminalRegistry::default();
        let mut v = watch(&reg, "V");
        reg.resize("s", v.m.id, size(110, 33));
        let mut w = watch(&reg, "W");
        assert_eq!(v.last().unwrap()["role"], "solo");
        let fw = w.last().unwrap();
        assert_eq!(fw["role"], "spectator");
        assert_eq!(fw["pilot"]["cols"], 110);
        assert!(!v.ignore_size() && w.ignore_size());
        assert!(v.spectator() && w.spectator());
        assert_eq!(reg.driver("s"), None);
        assert_eq!(reg.pilot("s"), None);
    }
}
