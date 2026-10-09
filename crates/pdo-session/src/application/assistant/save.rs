//! Persist the template the UI has open — **the focus names the target, the
//! assistant does not** (ADR-0051).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use pdo_kernel::authorization::{authorize, Denied};
use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use tracing::info;

use super::FocusCell;
use crate::application::ports::{TemplateSaveError, Templates};
use crate::domain::names::libassist_session_name_of;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveFocusedError {
    Refused(Denied),
    /// No template is open in the UI: saving into a guess is what this
    /// mechanism exists to prevent.
    NoFocus,
    Save(TemplateSaveError),
}

/// The template written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Saved {
    pub pipeline_id: String,
    pub path: PathBuf,
}

/// `POST /sessions/libassist/save`, the assistant's one write path.
///
/// Don't reintroduce a scope argument. `POST /library/pipelines` reads `scope` in
/// the *library store's* vocabulary (`.pdo/library/pipelines/`) while an edit tab's
/// `repo`/`user` mean `.pdo/pipelines/`: the two words look identical and point at
/// different trees, so a faithful assistant wrote a duplicate into the wrong store,
/// left the edited file untouched, and reported success. Taking no argument removes
/// the class of bug — the daemon already resolved the path at focus time, and it is
/// the only party that knows both vocabularies.
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

    /// Save into the template `actor`'s focus names: the assistant calls it
    /// with its Actor's identity, so it writes what its own Actor has open.
    pub fn execute(
        &self,
        actor: &Actor,
        yaml: &str,
        prompts: &HashMap<String, String>,
    ) -> Result<Saved, SaveFocusedError> {
        let focus = self
            .focus
            .get(&libassist_session_name_of(actor))
            .ok_or(SaveFocusedError::NoFocus)?;
        let id = focus.pipeline_id;
        let operation = Operation::new(ResourceKind::Pipeline, Verb::Update).on(id.clone());
        authorize(self.host.as_ref(), actor, &operation).map_err(SaveFocusedError::Refused)?;
        let path = self
            .templates
            .save(&id, yaml, prompts)
            .map_err(SaveFocusedError::Save)?;
        info!(
            "Library assistant saved template {id} at {}",
            path.display()
        );
        Ok(Saved {
            pipeline_id: id,
            path,
        })
    }
}
