//! Read the executor's journal of one attempt (#699).

use std::sync::Arc;

use super::update_context::UpdateContext;
use crate::domain::attempt::is_valid_attempt_id;

/// Why there is no log to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogError {
    /// The id is not filesystem-safe: refused before touching the disk.
    InvalidId,
    NoHome,
    /// No such attempt was journaled here.
    NotFound,
    Unreadable(String),
}

pub struct ReadAttemptLog {
    update: Arc<UpdateContext>,
}

impl ReadAttemptLog {
    pub fn new(update: Arc<UpdateContext>) -> Self {
        Self { update }
    }

    pub fn execute(&self, attempt_id: &str) -> Result<String, LogError> {
        if !is_valid_attempt_id(attempt_id) {
            return Err(LogError::InvalidId);
        }
        let journal = &self.update.ports.journal;
        if !journal.is_available() {
            return Err(LogError::NoHome);
        }
        match journal.read_log(attempt_id) {
            Ok(Some(text)) => Ok(text),
            Ok(None) => Err(LogError::NotFound),
            Err(e) => Err(LogError::Unreadable(e)),
        }
    }
}
