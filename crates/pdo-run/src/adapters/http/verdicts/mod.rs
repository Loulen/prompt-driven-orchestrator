//! The one HTTP projection of each verdict a Run's routes answer with: the
//! domain says what happened, this says with which status and which body.

pub mod completion;
pub mod restart;
pub mod retry;

pub use completion::refusal_response;
pub use restart::restart_response;
pub use retry::retry_response;
