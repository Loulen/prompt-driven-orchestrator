//! Read and save the template the UI has open — **the focus names the target,
//! the assistant does not** (ADR-0051) — through its shared draft (ADR-0088):
//! the assistant never writes the file, a human saves the draft as a version.

use std::collections::HashMap;
use std::sync::Arc;

use pdo_kernel::authorization::{authorize, Denied};
use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use tracing::info;

use super::FocusCell;
use crate::application::ports::{TemplateDraft, TemplateSaveError, TemplateSaved, Templates};
use crate::domain::names::libassist_session_name_of;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveFocusedError {
    Refused(Denied),
    /// No template is open in the UI: saving into a guess is what this
    /// mechanism exists to prevent.
    NoFocus,
    Save(TemplateSaveError),
}

/// The template saved into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Saved {
    pub pipeline_id: String,
    pub saved: TemplateSaved,
}

/// The template read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Read {
    pub pipeline_id: String,
    pub draft: TemplateDraft,
}

/// `POST /sessions/libassist/save`, the assistant's one write path, and
/// `GET /sessions/libassist/draft`, what it reads before.
///
/// Don't reintroduce a scope argument. `POST /library/pipelines` reads `scope` in
/// the *library store's* vocabulary (`.pdo/library/pipelines/`) while an edit tab's
/// `repo`/`user` mean `.pdo/pipelines/`: the two words look identical and point at
/// different trees, so a faithful assistant wrote a duplicate into the wrong store,
/// left the edited template untouched, and reported success. Taking no argument
/// removes the class of bug — the daemon already resolved the template at focus
/// time, and it is the only party that knows both vocabularies.
pub struct SaveFocused {
    focus: Arc<FocusCell>,
    templates: Arc<dyn Templates>,
    host: Arc<dyn ExtensionHost>,
}

impl SaveFocused {
    pub fn new(
        focus: Arc<FocusCell>,
        templates: Arc<dyn Templates>,
        host: Arc<dyn ExtensionHost>,
    ) -> Self {
        Self {
            focus,
            templates,
            host,
        }
    }

    /// Save into the draft of the template `actor`'s focus names: the
    /// assistant calls it with its Actor's identity, so it writes what its own
    /// Actor has open, as that Actor.
    pub async fn execute(
        &self,
        actor: &Actor,
        yaml: &str,
        prompts: &HashMap<String, String>,
    ) -> Result<Saved, SaveFocusedError> {
        let id = self.focused(actor, Verb::Update)?;
        let saved = self
            .templates
            .save(actor, &id, yaml, prompts)
            .await
            .map_err(SaveFocusedError::Save)?;
        match saved.version {
            Some(n) => info!("Library assistant saved {id} as v{n}: its file has no draft"),
            None => info!(
                "Library assistant saved into the draft of {id}: {:?}",
                saved.fields
            ),
        }
        Ok(Saved {
            pipeline_id: id,
            saved,
        })
    }

    /// The draft of the template `actor`'s focus names, as its assistant reads
    /// it.
    pub async fn read(&self, actor: &Actor) -> Result<Read, SaveFocusedError> {
        let id = self.focused(actor, Verb::Read)?;
        let draft = self
            .templates
            .read(actor, &id)
            .await
            .map_err(SaveFocusedError::Save)?;
        Ok(Read {
            pipeline_id: id,
            draft,
        })
    }

    /// The template `actor`'s focus names, once the Autorisation d'opération
    /// let `actor` `verb` it.
    fn focused(&self, actor: &Actor, verb: Verb) -> Result<String, SaveFocusedError> {
        let focus = self
            .focus
            .get(&libassist_session_name_of(actor))
            .ok_or(SaveFocusedError::NoFocus)?;
        let id = focus.pipeline_id;
        let operation = Operation::new(ResourceKind::Pipeline, verb).on(id.clone());
        authorize(self.host.as_ref(), actor, &operation).map_err(SaveFocusedError::Refused)?;
        Ok(id)
    }
}
