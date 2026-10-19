//! One open draft: its replicated document, the file it started from, its
//! participants, and the channel that carries what each one must hear.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

use tokio::sync::broadcast;

use crate::application::ports::{DraftDocument, StoredDraft};
use crate::domain::draft::changes::change_count;
use crate::domain::draft::presence::{next_color, DraftParticipant, PresenceState};
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
}

pub(super) struct RoomState {
    pub id: String,
    pub generation: String,
    pub doc: Box<dyn DraftDocument>,
    pub baseline: DraftContent,
    pub changes: u32,
    pub participants: BTreeMap<u64, DraftParticipant>,
}

/// An open draft.
pub struct Room {
    state: Mutex<RoomState>,
    /// Writes to the store one at a time, each with the latest state.
    persist: tokio::sync::Mutex<()>,
    tx: broadcast::Sender<DraftMessage>,
}

impl Room {
    pub(super) fn new(state: RoomState) -> Self {
        let (tx, _) = broadcast::channel(CHANNEL);
        Self {
            state: Mutex::new(state),
            persist: tokio::sync::Mutex::new(()),
            tx,
        }
    }

    pub(super) fn lock(&self) -> MutexGuard<'_, RoomState> {
        self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
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
            participants: state.participants.values().cloned().collect(),
        }
    }

    pub(super) fn has_participants(&self) -> bool {
        !self.lock().participants.is_empty()
    }

    /// Seat a participant; its color.
    pub(super) fn seat(&self, connection: u64, mut participant: DraftParticipant) -> u8 {
        let participants = {
            let mut state = self.lock();
            participant.color = next_color(state.participants.values().map(|p| p.color));
            state.participants.insert(connection, participant.clone());
            state.participants.values().cloned().collect()
        };
        self.send(DraftMessage::Participants(participants));
        participant.color
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
    /// lands in the draft, field by field, and it becomes the baseline.
    /// Whether anything changed.
    pub(super) fn follow(&self, file: &DraftContent) -> bool {
        let update = {
            let mut state = self.lock();
            if &state.baseline == file {
                return false;
            }
            let update = state.doc.change(&state.baseline.clone(), file);
            state.baseline = file.clone();
            recount(&mut state);
            update
        };
        self.send(DraftMessage::Update {
            from: DAEMON,
            update: Arc::new(update),
        });
        self.send(DraftMessage::Status {
            changes: self.changes(),
        });
        true
    }

    /// The draft was saved as it is: it is the file now.
    pub(super) fn saved(&self) {
        {
            let mut state = self.lock();
            state.baseline = state.doc.content();
            state.changes = 0;
        }
        self.send(DraftMessage::Status { changes: 0 });
    }

    pub(super) fn moved(&self, to: &str) {
        self.lock().id = to.to_string();
        self.send(DraftMessage::Moved { to: to.to_string() });
    }

    pub(super) fn removed(&self) {
        self.send(DraftMessage::Removed);
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
            },
        )
    }
}

/// Count the changes again; whether they moved.
fn recount(state: &mut RoomState) -> bool {
    let changes = change_count(&state.baseline, &state.doc.content());
    let moved = changes != state.changes;
    state.changes = changes;
    moved
}
