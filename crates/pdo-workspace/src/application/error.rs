//! Why a workspace use case did not happen.

use pdo_kernel::authorization::Denied;

use super::ports::StorageError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceError {
    /// An extension refused the Operation.
    Denied(Denied),
    /// The request named something that does not make sense (→ 400).
    Invalid(String),
    /// The entity it targets does not exist (→ 404).
    NotFound,
    /// A store failed (→ 500).
    Storage(StorageError),
    /// Git could not read the repository (→ 500, the message is git's).
    Git(String),
}

impl From<Denied> for WorkspaceError {
    fn from(denied: Denied) -> Self {
        WorkspaceError::Denied(denied)
    }
}

impl From<StorageError> for WorkspaceError {
    fn from(error: StorageError) -> Self {
        WorkspaceError::Storage(error)
    }
}

pub type WorkspaceResult<T> = Result<T, WorkspaceError>;
