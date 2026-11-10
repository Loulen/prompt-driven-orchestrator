//! The library assistant in a shared draft (ADR-0088, amends ADR-0051): it
//! reads the draft of the template its Actor has open and saves into it, as a
//! participant of that Actor, live for everyone. It never writes the file: a
//! human saves the draft as a version.
//!
//! The assistant sends the whole template. What lands in the draft is what it
//! changed since it last read (or saved) the draft, field by field: what a
//! participant changed meanwhile, elsewhere, stays.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Weak};

use pdo_kernel::Actor;
use serde_json::Value;
use tracing::warn;

use super::room::Room;
use crate::application::editor_json::editor_json;
use crate::application::error::PipelineError;
use crate::application::pipelines::Pipelines;
use crate::application::queries::PipelineDetail;
use crate::domain::draft::changes::changed_fields;
use crate::domain::draft::notice::DraftNotice;
use crate::domain::draft::presence::{
    DraftParticipant, PresenceSelection, PresenceState, SelectionKind,
};
use crate::domain::draft::DraftContent;
use crate::domain::pipeline::parse_pipeline;
use crate::domain::version::PipelineVersionRef;

/// The draft as the library assistant reads it: the template's YAML and the
/// prompts of its nodes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssistantDraft {
    pub yaml: String,
    pub prompts: BTreeMap<String, String>,
}

/// Where an assistant's save landed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssistantSaved {
    /// What changed in the draft.
    pub fields: Vec<String>,
    /// The file does not read, so it has no draft: the save was a version,
    /// as the editor's is on such a file.
    pub version: Option<PipelineVersionRef>,
}

/// Where an assistant sits: the draft, and its connection there.
pub(super) type AssistantSeat = (Weak<Room>, u64);

impl Pipelines {
    /// The draft of `id` as `actor`'s library assistant reads it: the file
    /// itself while the draft has no change, else the draft written as YAML.
    pub async fn assistant_read(
        &self,
        actor: &Actor,
        id: &str,
    ) -> Result<AssistantDraft, PipelineError> {
        let _seating = self.drafts.seating.lock().await;
        let room = match self.open_draft(id).await {
            Ok(room) => room,
            // No draft for a file that does not read: the file itself.
            Err(PipelineError::Invalid(_)) => {
                let yaml = self.registry.read(id).ok_or(PipelineError::NotFound)?;
                let prompts = self.registry.prompts(id).into_iter().collect();
                self.assistant_bases()
                    .remove(&(actor.clone(), id.to_string()));
                return Ok(AssistantDraft { yaml, prompts });
            }
            Err(e) => return Err(e),
        };
        let read = self.read_for_assistant(id, &room);
        self.close_if_empty(&room);
        let (yaml, base) = read?;
        let prompts = base.prompts.clone();
        self.assistant_bases()
            .insert((actor.clone(), id.to_string()), base);
        Ok(AssistantDraft { yaml, prompts })
    }

    fn read_for_assistant(
        &self,
        id: &str,
        room: &Room,
    ) -> Result<(String, DraftContent), PipelineError> {
        if room.changes() == 0 {
            let yaml = self.registry.read(id).ok_or(PipelineError::NotFound)?;
            return Ok((yaml, room.baseline()));
        }
        let content = room.content();
        let yaml = serde_yaml::to_string(&content.pipeline)
            .map_err(|e| PipelineError::BadRequest(e.to_string()))?;
        // What it reads back the same way: its save then differs from it by
        // its own changes only.
        let base = match self.assistant_content(id, &yaml, content.prompts.clone()) {
            Ok(base) => base,
            Err(e) => {
                warn!("draft of pipeline {id}: does not read back for the assistant: {e:?}");
                content
            }
        };
        Ok((yaml, base))
    }

    /// Save `yaml` and `prompts` (by node id; a node left out keeps its
    /// prompt) into the draft of `id`, as `actor`'s library assistant, whom
    /// the caller asked the Autorisation d'opération about. The fields that
    /// changed, which the participants are told.
    ///
    /// A file that does not read has no draft (the editor edits it without
    /// one): the save is then a version of `actor`, as the editor's would be.
    pub async fn assistant_save(
        &self,
        actor: &Actor,
        id: &str,
        yaml: &str,
        prompts: &HashMap<String, String>,
    ) -> Result<AssistantSaved, PipelineError> {
        let seating = self.drafts.seating.lock().await;
        let room = match self.open_draft(id).await {
            Ok(room) => room,
            Err(PipelineError::Invalid(_)) => {
                drop(seating);
                parse_pipeline(yaml)?;
                let saved = self.save(actor, id, yaml, prompts).await?;
                return Ok(AssistantSaved {
                    fields: Vec::new(),
                    version: Some(saved.version),
                });
            }
            Err(e) => return Err(e),
        };
        let saved = self
            .save_as_assistant(actor, id, &room, yaml, prompts)
            .await;
        self.close_if_empty(&room);
        Ok(AssistantSaved {
            fields: saved?,
            version: None,
        })
    }

    async fn save_as_assistant(
        &self,
        actor: &Actor,
        id: &str,
        room: &Arc<Room>,
        yaml: &str,
        prompts: &HashMap<String, String>,
    ) -> Result<Vec<String>, PipelineError> {
        let key = (actor.clone(), id.to_string());
        let base = match self.assistant_bases().get(&key) {
            Some(base) => base.clone(),
            None => room.baseline(),
        };
        let mut merged = base.prompts.clone();
        merged.extend(
            prompts
                .iter()
                .map(|(node, text)| (node.clone(), text.clone())),
        );
        let target = self.assistant_content(id, yaml, merged)?;
        let fields = changed_fields(&base, &target);
        self.assistant_bases().insert(key, target.clone());
        if fields.is_empty() {
            return Ok(fields);
        }
        let connection = self.seat_assistant(actor, room);
        room.present(
            connection,
            PresenceState {
                selection: first_changed_node(&base, &target).map(|id| PresenceSelection {
                    kind: SelectionKind::Node,
                    id,
                }),
                typing: None,
            }
            .bounded(),
        );
        // Nothing to tell when the draft held all of it already.
        if room.change_as(connection, &base, &target) {
            self.drafts.persist(room).await;
            self.announce(room);
            room.notify(DraftNotice::Assistant {
                actor: actor.clone(),
                display: self.host().display_name(actor),
                fields: fields.clone(),
            });
        }
        Ok(fields)
    }

    /// `actor`'s library assistant left the pipeline `id` (its focus moved,
    /// it closed): it leaves the draft of `id` if it sits there. What it read
    /// is kept for when it comes back.
    pub async fn assistant_left(&self, actor: &Actor, id: &str) {
        let _seating = self.drafts.seating.lock().await;
        let seat = {
            let mut seats = self.assistant_seats();
            let there = seats
                .get(actor)
                .and_then(|(room, _)| room.upgrade())
                .is_none_or(|room| room.id() == id);
            if there {
                seats.remove(actor)
            } else {
                None
            }
        };
        if let Some((room, connection)) = seat {
            if let Some(room) = room.upgrade() {
                self.unseat_assistant(&room, connection);
            }
        }
    }

    /// The connection `actor`'s assistant has in `room`, seated there (and
    /// out of any other draft) the first time. The caller holds `seating`.
    fn seat_assistant(&self, actor: &Actor, room: &Arc<Room>) -> u64 {
        let seated = self.assistant_seats().get(actor).cloned();
        if let Some((previous, connection)) = seated {
            match previous.upgrade() {
                Some(previous) if Arc::ptr_eq(&previous, room) && room.is_seated(connection) => {
                    return connection;
                }
                Some(previous) => self.unseat_assistant(&previous, connection),
                None => {}
            }
        }
        let connection = self
            .drafts
            .next_connection
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        room.seat(
            connection,
            DraftParticipant {
                connection: connection.to_string(),
                actor: actor.clone(),
                display: self.host().display_name(actor),
                can_write: true,
                assistant: true,
                presence: PresenceState::default(),
            },
        );
        self.assistant_seats()
            .insert(actor.clone(), (Arc::downgrade(room), connection));
        self.announce(room);
        connection
    }

    fn unseat_assistant(&self, room: &Arc<Room>, connection: u64) {
        if !room.is_seated(connection) {
            return;
        }
        room.unseat(connection);
        self.announce(room);
        self.close_if_empty(room);
    }

    /// `yaml` and `prompts` as the draft of `id` holds them; the prompts of
    /// nodes the YAML does not define are dropped.
    fn assistant_content(
        &self,
        id: &str,
        yaml: &str,
        mut prompts: BTreeMap<String, String>,
    ) -> Result<DraftContent, PipelineError> {
        let parsed = parse_pipeline(yaml)?;
        prompts.retain(|node, _| parsed.pipeline.nodes.iter().any(|n| &n.id == node));
        let detail = PipelineDetail {
            id: id.to_string(),
            path: self.registry.path_of(id),
            yaml: yaml.to_string(),
            pipeline: parsed.pipeline,
            prompts: HashMap::new(),
            diagnostics: Vec::new(),
        };
        let pipeline =
            editor_json(&detail).map_err(|e| PipelineError::BadRequest(e.to_string()))?;
        Ok(DraftContent::new(pipeline, prompts))
    }

    fn assistant_bases(&self) -> std::sync::MutexGuard<'_, HashMap<(Actor, String), DraftContent>> {
        self.drafts
            .assistant_bases
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn assistant_seats(&self) -> std::sync::MutexGuard<'_, HashMap<Actor, AssistantSeat>> {
        self.drafts
            .assistant_seats
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// The first node of `after` that differs from `before` (or whose prompt
/// does): where the assistant shows on the canvas.
fn first_changed_node(before: &DraftContent, after: &DraftContent) -> Option<String> {
    let nodes = |content: &DraftContent| -> Vec<Value> {
        content.pipeline["nodes"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    };
    let old = nodes(before);
    nodes(after).into_iter().find_map(|node| {
        let id = node["id"].as_str()?.to_string();
        let was = old.iter().find(|n| n["id"] == node["id"]);
        let changed = was != Some(&node) || before.prompts.get(&id) != after.prompts.get(&id);
        changed.then_some(id)
    })
}
