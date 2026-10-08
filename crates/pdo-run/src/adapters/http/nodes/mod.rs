//! The node routes of a Run: the agent's completion, failure and skip, the
//! canvas buttons (start, stop, retry), the files a user imports into a node
//! with a live session, and what a node reads (its prompt, its inputs and
//! outputs, its diff).

pub(super) mod gestures;
pub mod import;
pub(super) mod read;
pub mod requests;
