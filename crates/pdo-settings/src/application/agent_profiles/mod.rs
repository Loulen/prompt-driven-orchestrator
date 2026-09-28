//! The agentic profiles: the reads, and one use case per mutation — create
//! (`agent_profile` × `create`), edit (× `update` on the profile), delete
//! (× `delete` on the profile) — each asking the Autorisation d'opération
//! before it reads anything.

pub mod create;
pub mod delete;
pub mod read;
pub mod referents;
pub mod update;

use pdo_kernel::{Operation, ResourceKind, Verb};

use crate::application::ports::AgentProfileStore;
use crate::domain::agent_profile::{name_clash, AgentProfileError, ProfileDraft};

/// The Operation of a gesture on the profiles, aimed at `id` when it names one.
fn operation(verb: Verb, id: Option<&str>) -> Operation {
    let operation = Operation::new(ResourceKind::AgentProfile, verb);
    match id {
        Some(id) => operation.on(id),
        None => operation,
    }
}

/// The request, normalised, once no other profile carries its name
/// case-insensitively (`editing` is the profile being edited, which may keep
/// its own name). A failed lookup reads as a missing profile, as it always has.
async fn draft(
    store: &dyn AgentProfileStore,
    request: &ProfileRequest,
    editing: Option<&str>,
) -> Result<ProfileDraft, AgentProfileError> {
    let draft = ProfileDraft::new(
        &request.name,
        &request.harness,
        request.model.as_deref(),
        request.effort.as_deref(),
    )?;
    let existing = store
        .find_by_name(&draft.name)
        .await
        .map_err(|_| AgentProfileError::NotFound)?;
    if let Some(clash) = existing.and_then(|existing| name_clash(&existing, editing)) {
        return Err(clash);
    }
    Ok(draft)
}

/// A profile as a create or an edit sends it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProfileRequest {
    pub name: String,
    pub harness: String,
    pub model: Option<String>,
    pub effort: Option<String>,
}
