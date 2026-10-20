//! What the participants of a draft are told when someone outside the canvas
//! changed it (ADR-0088): a file written outside PDO, kept as a version and
//! merged in, or the library assistant's save.

use pdo_extension_api::ActorDisplay;
use pdo_kernel::Actor;
use serde::Serialize;
use ts_rs::TS;

use crate::domain::version::PipelineVersionRef;

/// A change of the draft no participant made on the canvas.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum DraftNotice {
    /// `file` was written outside PDO: kept as `version`, then merged into
    /// the draft, where `fields` changed.
    ExternalEdit {
        file: String,
        version: PipelineVersionRef,
        fields: Vec<String>,
    },
    /// The library assistant of `actor` saved into the draft, where `fields`
    /// changed.
    Assistant {
        actor: Actor,
        display: Option<ActorDisplay>,
        fields: Vec<String>,
    },
}
