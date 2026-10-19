//! The shared drafts of the registry's pipelines (ADR-0088): each pipeline
//! someone opens is edited in one draft, held here and kept by the store, that
//! every participant changes at once and sees change.
//!
//! Joining asks the Autorisation d'opération whether the Acteur may update the
//! pipeline, as a question (nothing recorded): refused, it joins to
//! read, and its updates are refused. A file written (a save from elsewhere,
//! an overwrite, a rename, an edit outside PDO) lands in the draft field by
//! field.

mod room;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use tokio::sync::broadcast;
use tracing::warn;

use room::RoomState;
pub use room::{DraftMessage, Room};

use super::editor_json::editor_json;
use super::error::PipelineError;
use super::pipelines::Pipelines;
use super::ports::{DraftDocuments, DraftStore};
use crate::domain::draft::presence::{DraftParticipant, PresenceState};
use crate::domain::draft::{DraftContent, DraftSummary};

/// The open drafts, and where they are kept.
pub struct Drafts {
    store: Arc<dyn DraftStore>,
    documents: Arc<dyn DraftDocuments>,
    rooms: Mutex<HashMap<String, Arc<Room>>>,
    /// Opening a draft, seating a participant and closing a draft after its
    /// last one, one at a time: two first participants share one draft, and a
    /// newcomer never sits in a draft that is closing.
    seating: tokio::sync::Mutex<()>,
    next_connection: AtomicU64,
}

impl Drafts {
    pub fn new(store: Arc<dyn DraftStore>, documents: Arc<dyn DraftDocuments>) -> Self {
        Self {
            store,
            documents,
            rooms: Mutex::new(HashMap::new()),
            seating: tokio::sync::Mutex::new(()),
            next_connection: AtomicU64::new(room::DAEMON + 1),
        }
    }

    fn rooms(&self) -> std::sync::MutexGuard<'_, HashMap<String, Arc<Room>>> {
        self.rooms
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn loaded(&self, id: &str) -> Option<Arc<Room>> {
        self.rooms().get(id).cloned()
    }

    async fn persist(&self, room: &Room) {
        let _one_at_a_time = room.persist_lock().lock().await;
        // A deleted pipeline's draft is not written back.
        if room.is_removed() {
            return;
        }
        let (id, stored) = room.stored();
        if let Err(e) = self.store.save(&id, &stored).await {
            warn!("draft of pipeline {id}: not kept: {e}");
        }
    }
}

/// A participant seated in a draft.
pub struct DraftJoin {
    pub room: Arc<Room>,
    pub connection: u64,
    pub can_write: bool,
    pub color: u8,
    /// What the participant hears, from before it was seated on.
    pub messages: broadcast::Receiver<DraftMessage>,
}

impl Pipelines {
    /// Seat `actor` in the draft of `id`, opening it from the store, or from
    /// the file the first time.
    pub async fn join_draft(&self, actor: &Actor, id: &str) -> Result<DraftJoin, PipelineError> {
        let _seating = self.drafts.seating.lock().await;
        let room = self.open_draft(id).await?;
        let update = Operation::new(ResourceKind::Pipeline, Verb::Update).on(id);
        let can_write = self.host().authorize(actor, &update).is_allowed();
        let messages = room.subscribe();
        let connection = self.drafts.next_connection.fetch_add(1, Ordering::Relaxed);
        let participant = DraftParticipant {
            connection: connection.to_string(),
            actor: actor.clone(),
            display: self.host().display_name(actor),
            color: 0,
            can_write,
            presence: PresenceState::default(),
        };
        let color = room.seat(connection, participant);
        self.announce(&room);
        Ok(DraftJoin {
            room,
            connection,
            can_write,
            color,
            messages,
        })
    }

    /// `connection` left the draft; the draft closes with its last
    /// participant (the store keeps it).
    pub async fn leave_draft(&self, room: &Arc<Room>, connection: u64) {
        room.unseat(connection);
        self.announce(room);
        let _seating = self.drafts.seating.lock().await;
        if room.has_participants() {
            return;
        }
        self.drafts.persist(room).await;
        self.close_if_empty(room);
    }

    /// `update` from `connection`, kept, then passed to the others.
    ///
    /// Kept before it is passed on: what a participant saw survives a
    /// restart of the daemon.
    pub async fn edit_draft(
        &self,
        room: &Room,
        connection: u64,
        update: &[u8],
    ) -> Result<(), String> {
        let moved = room.apply(update)?;
        self.drafts.persist(room).await;
        room.pass_on(connection, update, moved);
        if moved {
            self.announce(room);
        }
        Ok(())
    }

    /// The file of `id` was written: a draft of it follows. Nothing for a
    /// pipeline nobody ever opened.
    pub async fn draft_file_changed(&self, id: &str) {
        let room = match self.drafts.loaded(id) {
            Some(room) => room,
            None => match self.drafts.store.load(id).await {
                Ok(Some(_)) => {
                    let _seating = self.drafts.seating.lock().await;
                    match self.open_draft(id).await {
                        // Opening followed the file already.
                        Ok(room) => {
                            self.announce(&room);
                            return self.close_if_empty(&room);
                        }
                        Err(_) => return,
                    }
                }
                _ => return,
            },
        };
        let Ok(file) = self.draft_file(id) else {
            return;
        };
        if room.follow(&file) {
            self.drafts.persist(&room).await;
            self.announce(&room);
        }
    }

    /// The draft of `id` was saved as the pipeline `saved_as`, holding
    /// `saved` (what the editor serialized). A change made meanwhile by
    /// someone else still counts; what the file normalized lands in the draft.
    pub(crate) async fn draft_saved(&self, id: &str, saved_as: &str, saved: &DraftContent) {
        if id != saved_as {
            self.draft_moved(id, saved_as, false).await;
        }
        let Some(room) = self.drafts.loaded(saved_as) else {
            return;
        };
        room.saved(saved);
        self.drafts.persist(&room).await;
        self.draft_file_changed(saved_as).await;
        self.announce(&room);
    }

    /// A rename moved `from` to `to`: its draft follows, and takes the new
    /// file in when `follow`.
    pub(crate) async fn draft_moved(&self, from: &str, to: &str, follow: bool) {
        if let Err(e) = self.drafts.store.carry(from, to).await {
            warn!("draft of pipeline {from}: not moved to {to}: {e}");
        }
        let room = self.drafts.rooms().remove(from);
        if let Some(room) = room {
            room.moved(to);
            self.drafts.rooms().insert(to.to_string(), room);
        }
        if follow {
            self.draft_file_changed(to).await;
        }
    }

    /// The pipeline `id` was deleted: so is its draft.
    pub(crate) async fn draft_removed(&self, id: &str) {
        if let Some(room) = self.drafts.rooms().remove(id) {
            room.removed();
        }
        if let Err(e) = self.drafts.store.remove(id).await {
            warn!("draft of pipeline {id}: not removed: {e}");
        }
    }

    /// The drafts the pipeline list shows, by pipeline id: the open ones with
    /// their participants, the kept ones with their changes.
    pub async fn draft_summaries(&self) -> HashMap<String, DraftSummary> {
        let mut summaries: HashMap<String, DraftSummary> = self
            .drafts
            .store
            .changes()
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|(_, changes)| *changes > 0)
            .map(|(id, changes)| {
                (
                    id,
                    DraftSummary {
                        changes,
                        participants: Vec::new(),
                    },
                )
            })
            .collect();
        let open: Vec<Arc<Room>> = self.drafts.rooms().values().cloned().collect();
        for room in open {
            summaries.insert(room.id(), room.summary());
        }
        summaries
    }

    /// The file of `id` as a draft holds it.
    fn draft_file(&self, id: &str) -> Result<DraftContent, PipelineError> {
        let detail = self.detail(id)?;
        let pipeline =
            editor_json(&detail).map_err(|e| PipelineError::BadRequest(e.to_string()))?;
        Ok(DraftContent::new(pipeline, detail.prompts))
    }

    /// The draft of `id`, opened if needed. The caller holds `seating`.
    async fn open_draft(&self, id: &str) -> Result<Arc<Room>, PipelineError> {
        if let Some(room) = self.drafts.loaded(id) {
            return Ok(room);
        }
        let file = self.draft_file(id)?;
        let stored = self
            .drafts
            .store
            .load(id)
            .await
            .map_err(PipelineError::Storage)?;
        let kept = stored.and_then(|stored| match self.drafts.documents.load(&stored.state) {
            Ok(doc) => Some((stored, doc)),
            Err(e) => {
                warn!("draft of pipeline {id}: unreadable, started again from the file: {e}");
                None
            }
        });
        let room = match kept {
            Some((stored, doc)) => {
                let room = Room::new(RoomState {
                    id: id.to_string(),
                    generation: stored.generation,
                    doc,
                    baseline: stored.baseline,
                    changes: stored.changes,
                    participants: Default::default(),
                });
                // Written while nobody had it open (or the daemon was down).
                room.follow(&file);
                room
            }
            None => Room::new(RoomState {
                id: id.to_string(),
                generation: uuid::Uuid::new_v4().simple().to_string(),
                doc: self.drafts.documents.create(&file),
                baseline: file,
                changes: 0,
                participants: Default::default(),
            }),
        };
        let room = Arc::new(room);
        self.drafts.persist(&room).await;
        self.drafts.rooms().insert(id.to_string(), room.clone());
        Ok(room)
    }

    fn close_if_empty(&self, room: &Arc<Room>) {
        let mut rooms = self.drafts.rooms();
        if !room.has_participants() && rooms.get(&room.id()).is_some_and(|r| Arc::ptr_eq(r, room)) {
            rooms.remove(&room.id());
        }
    }

    /// Tell the pipeline lists who has the draft open and its changes.
    fn announce(&self, room: &Room) {
        self.events.draft_changed(&room.id(), &room.summary());
    }
}
