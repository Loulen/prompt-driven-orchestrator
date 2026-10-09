//! Declare (or clear) which template the UI is editing — and, by the same token,
//! that a human is still there (ADR-0051 §2).

use std::sync::Arc;

use pdo_kernel::authorization::{authorize, Denied};
use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};

use super::FocusCell;
use crate::application::ports::Templates;
use crate::domain::libassist::{focus_json, LibassistFocus};
use crate::domain::names::libassist_session_name_of;

/// `PUT /sessions/libassist/focus`. The UI repeats it every 20 s while an edit
/// view is open, and the repetition is load-bearing twice over: it is how the
/// assistant learns the current template on the next message, and how the orphan
/// sweep knows not to reap a session out from under someone editing with the
/// Assistant tab hidden.
///
/// **Never spawns the assistant.** Declaring a focus is not asking for a REPL;
/// auto-spawn was ruled out by ADR-0048.
pub struct DeclareFocus {
    focus: Arc<FocusCell>,
    templates: Arc<dyn Templates>,
    host: Arc<dyn ExtensionHost>,
}

impl DeclareFocus {
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

    /// Declare the focus of `actor`'s assistant. Returns the focus as the UI
    /// reads it back.
    pub fn execute(
        &self,
        actor: &Actor,
        pipeline_id: Option<String>,
    ) -> Result<serde_json::Value, Denied> {
        let session = libassist_session_name_of(actor);
        let operation = Operation::new(ResourceKind::Session, Verb::Update).on(session.clone());
        authorize(self.host.as_ref(), actor, &operation)?;
        let Some(pipeline_id) = pipeline_id.filter(|id| !id.is_empty()) else {
            self.focus.set(&session, None);
            return Ok(serde_json::json!({ "ok": true, "pipeline_id": null }));
        };
        // The absolute path is the daemon's job, not the client's: "scope" means
        // `.pdo/pipelines/` on an edit tab but `.pdo/library/pipelines/` in the
        // library store, and a client building the path itself gets that wrong.
        // An id that resolves to no file is NOT an error: a template can be open
        // in the canvas and not yet saved.
        let path = self.templates.path_of(&pipeline_id);
        let focus = LibassistFocus {
            pipeline_id,
            path: path.exists().then_some(path),
            at: chrono::Utc::now(),
        };
        let body = focus_json(Some(&focus), focus.at);
        self.focus.set(&session, Some(focus));
        Ok(body)
    }
}
