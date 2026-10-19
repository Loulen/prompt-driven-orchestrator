//! The use cases of the settings context, one per file, and the ports they
//! need (`ports.rs`). Every mutation — an edit of the instance settings, the
//! create, edit and delete of an agentic profile — builds an Operation and goes
//! through the Autorisation d'opération before it touches anything. The boot's
//! own reads (the retired settings' warning, the catalogues' warm-up) are
//! transitions of the daemon and never ask.

pub mod agent_profiles;
pub mod boot;
pub mod ports;
pub mod read_settings;
pub mod refresh_catalogue;
pub mod settings;
pub mod update_settings;

use pdo_kernel::authorization::Denied;

use crate::domain::agent_profile::AgentProfileError;

/// Why a use case of the context did not go through. Each variant maps to one
/// status; its message is the body's `error`, byte for byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsError {
    /// An extension refused the Operation (`403 operation_refused`).
    Refused(Denied),
    /// The request is invalid (`400`).
    Invalid(String),
    /// No such agentic profile (`404`).
    NotFound(String),
    /// Another agentic profile carries the name (`409`).
    Conflict(String),
    /// The storage failed (`500`); the message says what failed.
    Storage(String),
}

impl From<Denied> for SettingsError {
    fn from(denied: Denied) -> Self {
        SettingsError::Refused(denied)
    }
}

impl From<AgentProfileError> for SettingsError {
    fn from(error: AgentProfileError) -> Self {
        let message = error.to_string();
        match error {
            AgentProfileError::NotFound => SettingsError::NotFound(message),
            AgentProfileError::DuplicateName { .. } => SettingsError::Conflict(message),
            AgentProfileError::EmptyName
            | AgentProfileError::EmptyHarness
            | AgentProfileError::DefaultUndeletable => SettingsError::Invalid(message),
            AgentProfileError::Storage(_) => SettingsError::Storage(message),
        }
    }
}
