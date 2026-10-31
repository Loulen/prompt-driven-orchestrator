//! One open draft: its replicated document, the file it started from, its
//! participants, and the channel that carries what each one must hear.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use tokio::sync::broadcast;

use crate::application::ports::{DraftDocument, StoredDraft};
use crate::domain::draft::changes::{change_count, changed_fields};
use crate::domain::draft::notice::DraftNotice;
use crate::domain::draft::presence::{DraftParticipant, PresenceState};
use crate::domain::draft::{DraftContent, DraftSummary};

/// How many messages a slow participant may lag behind before it resyncs.
const CHANNEL: usize = 512;

/// The connection the daemon itself changes the draft as (a file written).
pub const DAEMON: u64 = 0;

/// What the participants of a draft hear.
#[derive(Debug, Clone)]
pub enum DraftMessage {
    /// An update of the document, from the connection `from` (which does not
    /// hear its own).
    Update { from: u64, update: Arc<Vec<u8>> },
    /// Who is there now.
    Participants(Vec<DraftParticipant>),
    /// The changes since the file.
    Status { changes: u32 },
    /// A rename moved the pipeline: the draft lives under `to` now.
    Moved { to: String },
    /// The pipeline was deleted.
    Removed,
    /// Someone outside the canvas changed the draft.
    Notice(DraftNotice),
}

pub(super) struct RoomState {
    pub id: String,
    pub generation: String,
    pub doc: Box<dyn DraftDocument>,
    pub baseline: DraftContent,
    pub changes: u32,
    pub nodes: u32,
    pub participants: BTreeMap<u64, DraftParticipant>,
}

/// An open draft.
pub struct Room {
    state: Mutex<RoomState>,
    /// Writes to the store one at a time, each with the latest state.
    persist: tokio::sync::Mutex<()>,
    /// The pipeline was deleted: nothing of the room is kept any more.
    removed: AtomicBool,
    tx: broadcast::Sender<DraftMessage>,
}

impl Room {
    pub(super) fn new(state: RoomState) -> Self {
        let (tx, _) = broadcast::channel(CHANNEL);
        Self {
            state: Mutex::new(state),
            persist: tokio::sync::Mutex::new(()),
            removed: AtomicBool::new(false),
            tx,
        }
    }

    pub(super) fn lock(&self) -> MutexGuard<'_, RoomState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub(super) fn persist_lock(&self) -> &tokio::sync::Mutex<()> {
        &self.persist
    }

    /// Listen to the draft.
    pub fn subscribe(&self) -> broadcast::Receiver<DraftMessage> {
        self.tx.subscribe()
    }

    pub(super) fn send(&self, message: DraftMessage) {
        let _ = self.tx.send(message);
    }

    /// The pipeline id the draft lives under now.
    pub fn id(&self) -> String {
        self.lock().id.clone()
    }

    pub fn generation(&self) -> String {
        self.lock().generation.clone()
    }

    /// Everything a newcomer with nothing misses.
    pub fn encode(&self) -> Vec<u8> {
        self.lock().doc.encode()
    }

    pub fn state_vector(&self) -> Vec<u8> {
        self.lock().doc.state_vector()
    }

    /// What a participant with `state_vector` misses.
    pub fn missing_for(&self, state_vector: &[u8]) -> Result<Vec<u8>, String> {
        self.lock().doc.missing_for(state_vector)
    }

    pub fn changes(&self) -> u32 {
        self.lock().changes
    }

    pub fn summary(&self) -> DraftSummary {
        let state = self.lock();
        DraftSummary {
            changes: state.changes,
            nodes: state.nodes,
            participants: state.participants.values().cloned().collect(),
        }
    }

    /// Whether anyone but an assistant has it open: an assistant alone does
    /// not keep a draft open.
    pub(super) fn has_participants(&self) -> bool {
        self.lock().participants.values().any(|p| !p.assistant)
    }

    pub(super) fn is_seated(&self, connection: u64) -> bool {
        self.lock().participants.contains_key(&connection)
    }

    /// What the draft holds now.
    pub(super) fn content(&self) -> DraftContent {
        self.lock().doc.content()
    }

    /// The file the draft counts its changes from.
    pub(super) fn baseline(&self) -> DraftContent {
        self.lock().baseline.clone()
    }

    /// Seat a participant.
    pub(super) fn seat(&self, connection: u64, participant: DraftParticipant) {
        let participants = {
            let mut state = self.lock();
            state.participants.insert(connection, participant);
            state.participants.values().cloned().collect()
        };
        self.send(DraftMessage::Participants(participants));
    }

    pub(super) fn unseat(&self, connection: u64) {
        let participants = {
            let mut state = self.lock();
            state.participants.remove(&connection);
            state.participants.values().cloned().collect()
        };
        self.send(DraftMessage::Participants(participants));
    }

    /// What `connection` says of itself.
    pub fn present(&self, connection: u64, presence: PresenceState) {
        let participants = {
            let mut state = self.lock();
            let Some(participant) = state.participants.get_mut(&connection) else {
                return;
            };
            if participant.presence == presence {
                return;
            }
            participant.presence = presence;
            state.participants.values().cloned().collect()
        };
        self.send(DraftMessage::Participants(participants));
    }

    /// Apply `update`; whether the changes moved. Nobody hears of it until
    /// [`Room::pass_on`].
    pub(super) fn apply(&self, update: &[u8]) -> Result<bool, String> {
        let mut state = self.lock();
        state.doc.apply_update(update)?;
        Ok(recount(&mut state))
    }

    /// Pass `update` from `connection` on to the others, with the changes
    /// when they `moved`.
    pub(super) fn pass_on(&self, connection: u64, update: &[u8], moved: bool) {
        self.send(DraftMessage::Update {
            from: connection,
            update: Arc::new(update.to_vec()),
        });
        if moved {
            self.send(DraftMessage::Status {
                changes: self.changes(),
            });
        }
    }

    /// The file is now `file`: what changed in it since the draft's baseline
    /// lands in the draft, field by field, and it becomes the baseline. The
    /// fields that changed in the file, `None` when it did not.
    pub(super) fn follow(&self, file: &DraftContent) -> Option<Vec<String>> {
        let (update, fields) = {
            let mut state = self.lock();
            if &state.baseline == file {
                return None;
            }
            let fields = changed_fields(&state.baseline, file);
            let update = state.doc.change(&state.baseline.clone(), file);
            state.baseline = file.clone();
            recount(&mut state);
            (update, fields)
        };
        self.send(DraftMessage::Update {
            from: DAEMON,
            update: Arc::new(update),
        });
        self.send(DraftMessage::Status {
            changes: self.changes(),
        });
        Some(fields)
    }

    /// `connection` changes the draft from `from` to `to`, field by field:
    /// what someone else changed meanwhile, elsewhere, stays. Passed on to
    /// the others. Whether the document changed.
    pub(super) fn change_as(
        &self,
        connection: u64,
        from: &DraftContent,
        to: &DraftContent,
    ) -> bool {
        let (update, moved) = {
            let mut state = self.lock();
            let before = state.doc.state_vector();
            let update = state.doc.change(from, to);
            if state.doc.state_vector() == before {
                return false;
            }
            (update, recount(&mut state))
        };
        self.pass_on(connection, &update, moved);
        true
    }

    /// Tell every participant.
    pub(super) fn notify(&self, notice: DraftNotice) {
        self.send(DraftMessage::Notice(notice));
    }

    /// The draft was saved as `saved` (what the editor serialized): that is
    /// the file now. What changed in the draft since still counts.
    pub(super) fn saved(&self, saved: &DraftContent) {
        let changes = {
            let mut state = self.lock();
            state.baseline = saved.clone().normalized();
            recount(&mut state);
            state.changes
        };
        self.send(DraftMessage::Status { changes });
    }

    /// Count the changes and the nodes again.
    pub(super) fn recount(&self) {
        recount(&mut self.lock());
    }

    pub(super) fn moved(&self, to: &str) {
        self.lock().id = to.to_string();
        self.send(DraftMessage::Moved { to: to.to_string() });
    }

    pub(super) fn removed(&self) {
        self.removed.store(true, Ordering::SeqCst);
        self.send(DraftMessage::Removed);
    }

    pub(super) fn is_removed(&self) -> bool {
        self.removed.load(Ordering::SeqCst)
    }

    /// The draft as the store keeps it.
    pub(super) fn stored(&self) -> (String, StoredDraft) {
        let state = self.lock();
        (
            state.id.clone(),
            StoredDraft {
                generation: state.generation.clone(),
                state: state.doc.encode(),
                baseline: state.baseline.clone(),
                changes: state.changes,
                nodes: state.nodes,
            },
        )
    }
}

/// Count the changes and the nodes again; whether they moved.
fn recount(state: &mut RoomState) -> bool {
    let content = state.doc.content();
    let changes = change_count(&state.baseline, &content);
    let nodes = content.node_count();
    let moved = changes != state.changes || nodes != state.nodes;
    state.changes = changes;
    state.nodes = nodes;
    moved
}
