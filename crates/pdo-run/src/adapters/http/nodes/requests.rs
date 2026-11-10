//! The bodies and queries of the node routes.

use serde::Deserialize;

#[derive(Deserialize)]
pub struct NodeDoneRequest {
    #[serde(default)]
    pub iter: Option<i64>,
    /// `true` when the completion came from the injected `Stop` hook on turn end
    /// rather than from the agent. Selects [`CompletionSource::StopHook`] →
    /// `NodeAutoCompleted`. Keep `#[serde(default)]`: an older `pdo complete`
    /// sends the legacy body (`{ "iter": … }`) and must deserialise as `false`.
    #[serde(default)]
    pub auto: bool,
}

#[derive(Deserialize)]
pub struct ReleaseNodeCompletionRequest {
    #[serde(default)]
    pub iter: Option<i64>,
}

#[derive(Deserialize)]
pub struct NodeFailRequest {
    pub reason: String,
    #[serde(default)]
    pub iter: Option<i64>,
}

#[derive(Deserialize)]
pub struct NodeSkipRequest {
    pub reason: String,
    #[serde(default)]
    pub iter: Option<i64>,
}

#[derive(Deserialize)]
pub struct IterQuery {
    #[serde(default = "default_iter")]
    pub iter: i64,
}

/// The query of a transcript page: the iteration, the first entry, how many.
#[derive(Deserialize)]
pub struct TranscriptQuery {
    #[serde(default = "default_iter")]
    pub iter: i64,
    #[serde(default)]
    pub after: usize,
    #[serde(default = "default_transcript_limit")]
    pub limit: usize,
}

pub fn default_transcript_limit() -> usize {
    50
}

pub fn default_iter() -> i64 {
    1
}
