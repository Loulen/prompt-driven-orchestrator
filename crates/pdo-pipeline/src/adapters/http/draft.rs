//! `WS /pipelines/{id}/draft`: the shared draft of a pipeline (ADR-0088).
//!
//! Binary frames carry the replicated document, their first byte saying
//! what follows (the frontend's `pipeline/api/draftSocket.ts` speaks the
//! same):
//!
//! - [`SYNC_REQUEST`] + a state vector: « here is what I know ». The daemon
//!   answers [`SYNC_REPLY`] with what the peer misses, and asks the same of
//!   it, so the changes a client made offline come back.
//! - [`SYNC_REPLY`] + an update: what the peer missed.
//! - [`UPDATE`] + an update: a change, passed to every other participant.
//!
//! Text frames are JSON: [`DraftServerMessage`] from the daemon (the
//! welcome, the participants, the changes, the heartbeat every
//! [`HEARTBEAT`], a rename, a deletion, a notice), [`DraftClientMessage`] from the
//! client (its presence). A participant the Autorisation d'opération does
//! not let update the pipeline reads: its updates are dropped.
//!
//! The client's Close frame is answered before the connection ends (ADR-0083
//! §3).

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Path as AxumPath, State, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Extension;
use futures_util::SinkExt;
use pdo_kernel::Actor;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast::error::RecvError;
use tracing::{debug, info, warn};
use ts_rs::TS;

use crate::application::drafts::{DraftJoin, DraftMessage};
use crate::application::Pipelines;
use crate::domain::draft::notice::DraftNotice;
use crate::domain::draft::presence::{DraftParticipant, PresenceState};

/// « Here is what I know »: a state vector follows.
pub const SYNC_REQUEST: u8 = 0;
/// What the peer missed: an update follows.
pub const SYNC_REPLY: u8 = 1;
/// A change: an update follows.
pub const UPDATE: u8 = 2;

/// How often the daemon beats on the socket (ADR-0083 §5).
pub const HEARTBEAT: Duration = Duration::from_secs(5);

/// A client heard of for this long is gone (a half-open connection after a
/// sleep fires no Close): its presence ends. The client beats every 5 s.
pub const SILENCE: Duration = Duration::from_secs(20);

/// The largest frame a client may send: a whole draft fits many times over.
const MAX_FRAME: usize = 8 * 1024 * 1024;

/// Whether a WebSocket upgrade comes from an origin the daemon accepts.
pub type OriginCheck = Arc<dyn Fn(&HeaderMap) -> bool + Send + Sync>;

/// What the daemon says on a draft's socket.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum DraftServerMessage {
    /// The first message: who the connection is in the draft. A client
    /// holding a copy of another `generation` drops it.
    Welcome {
        connection: String,
        generation: String,
        can_write: bool,
    },
    Participants {
        participants: Vec<DraftParticipant>,
    },
    /// The draft's changes since the file; 0 is nothing to save.
    Status {
        changes: u32,
    },
    /// A rename moved the pipeline to `to`: open the draft there.
    Moved {
        to: String,
    },
    /// The pipeline was deleted.
    Removed,
    /// The draft cannot open (the file no longer reads): the editor edits
    /// without it.
    Unavailable {
        message: String,
    },
    /// Someone outside the canvas changed the draft: a file written outside
    /// PDO, the library assistant.
    Notice {
        notice: DraftNotice,
    },
    Heartbeat,
}

/// What a client says on a draft's socket.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum DraftClientMessage {
    Presence {
        presence: PresenceState,
    },
    /// « Still here », every 5 s.
    Heartbeat,
}

/// The state of the draft's route.
#[derive(Clone)]
pub(super) struct DraftRoute {
    pipelines: Arc<Pipelines>,
    origin: OriginCheck,
}

impl DraftRoute {
    pub(super) fn new(pipelines: Arc<Pipelines>, origin: OriginCheck) -> Self {
        Self { pipelines, origin }
    }
}

pub(super) async fn upgrade(
    State(route): State<DraftRoute>,
    AxumPath(pipeline_id): AxumPath<String>,
    Extension(actor): Extension<Actor>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    if !(route.origin)(&headers) {
        warn!("WS /pipelines/{pipeline_id}/draft refused: origin not allowed");
        return (StatusCode::FORBIDDEN, "Origin not allowed").into_response();
    }
    if !route.pipelines.exists(&pipeline_id) {
        return super::not_found();
    }
    ws.max_message_size(MAX_FRAME)
        .on_upgrade(move |socket| async move {
            let pipelines = route.pipelines;
            match pipelines.join_draft(&actor, &pipeline_id).await {
                Ok(join) => {
                    let room = join.room.clone();
                    let connection = join.connection;
                    serve(socket, &pipelines, join).await;
                    pipelines.leave_draft(&room, connection).await;
                }
                Err(e) => {
                    warn!("draft of pipeline {pipeline_id} could not open: {e:?}");
                    let mut socket = socket;
                    let message = DraftServerMessage::Unavailable {
                        message: format!("{e:?}"),
                    };
                    let _ = socket.send(text(&message)).await;
                    close(socket).await;
                }
            }
        })
}

async fn close(mut socket: WebSocket) {
    let _ = socket.send(Message::Close(None)).await;
}

fn text(message: &DraftServerMessage) -> Message {
    Message::Text(serde_json::to_string(message).unwrap_or_default().into())
}

fn binary(tag: u8, payload: &[u8]) -> Message {
    let mut frame = Vec::with_capacity(payload.len() + 1);
    frame.push(tag);
    frame.extend_from_slice(payload);
    Message::Binary(frame.into())
}

/// Serve one participant until either side ends.
async fn serve(mut socket: WebSocket, pipelines: &Pipelines, join: DraftJoin) {
    let DraftJoin {
        room,
        connection,
        can_write,
        mut messages,
    } = join;
    let welcome = DraftServerMessage::Welcome {
        connection: connection.to_string(),
        generation: room.generation(),
        can_write,
    };
    let summary = room.summary();
    let greeting = [
        text(&welcome),
        text(&DraftServerMessage::Participants {
            participants: summary.participants,
        }),
        text(&DraftServerMessage::Status {
            changes: summary.changes,
        }),
    ];
    for message in greeting {
        if socket.send(message).await.is_err() {
            return;
        }
    }
    let mut heartbeat = tokio::time::interval(HEARTBEAT);
    let mut heard_at = Instant::now();
    loop {
        let outgoing = tokio::select! {
            _ = heartbeat.tick() => {
                if heard_at.elapsed() > SILENCE {
                    close(socket).await;
                    return;
                }
                vec![text(&DraftServerMessage::Heartbeat)]
            }
            incoming = socket.recv() => {
                heard_at = Instant::now();
                match incoming {
                Some(Ok(Message::Close(_))) => {
                    // The answer is queued by the protocol; flushing sends it.
                    let _ = socket.flush().await;
                    return;
                }
                Some(Ok(Message::Binary(frame))) => {
                    received(pipelines, &room, connection, can_write, &frame).await
                }
                Some(Ok(Message::Text(body))) => {
                    if let Ok(DraftClientMessage::Presence { presence }) = serde_json::from_str(&body) {
                        room.present(connection, presence.bounded());
                    }
                    Vec::new()
                }
                Some(Ok(_)) => Vec::new(),
                None | Some(Err(_)) => return,
                }
            }
            heard = messages.recv() => match heard {
                Ok(DraftMessage::Update { from, update }) if from != connection => {
                    vec![binary(UPDATE, &update)]
                }
                Ok(DraftMessage::Update { .. }) => Vec::new(),
                Ok(DraftMessage::Participants(participants)) => {
                    vec![text(&DraftServerMessage::Participants { participants })]
                }
                Ok(DraftMessage::Status { changes }) => {
                    vec![text(&DraftServerMessage::Status { changes })]
                }
                Ok(DraftMessage::Notice(notice)) => {
                    vec![text(&DraftServerMessage::Notice { notice })]
                }
                Ok(DraftMessage::Moved { to }) => {
                    let _ = socket.send(text(&DraftServerMessage::Moved { to })).await;
                    close(socket).await;
                    return;
                }
                Ok(DraftMessage::Removed) => {
                    let _ = socket.send(text(&DraftServerMessage::Removed)).await;
                    close(socket).await;
                    return;
                }
                // Behind the others: the whole draft again.
                Err(RecvError::Lagged(_)) => vec![binary(SYNC_REPLY, &room.encode())],
                Err(RecvError::Closed) => return,
            },
        };
        for message in outgoing {
            if socket.send(message).await.is_err() {
                return;
            }
        }
    }
}

/// A binary frame from the client; what to answer.
async fn received(
    pipelines: &Pipelines,
    room: &crate::application::drafts::Room,
    connection: u64,
    can_write: bool,
    frame: &[u8],
) -> Vec<Message> {
    let Some((&tag, payload)) = frame.split_first() else {
        return Vec::new();
    };
    match tag {
        SYNC_REQUEST => match room.missing_for(payload) {
            Ok(missing) => vec![
                binary(SYNC_REPLY, &missing),
                binary(SYNC_REQUEST, &room.state_vector()),
            ],
            Err(e) => {
                debug!("draft {}: bad state vector: {e}", room.id());
                Vec::new()
            }
        },
        SYNC_REPLY | UPDATE if can_write => {
            if let Err(e) = pipelines.edit_draft(room, connection, payload).await {
                info!("draft {}: update refused: {e}", room.id());
            }
            Vec::new()
        }
        _ => Vec::new(),
    }
}
