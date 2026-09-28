//! The pure rules of the review of a Run. Nothing here does I/O.
//!
//! - [`diff`]: a Run's diff as data (files → hunks → lines) and the parser of
//!   the unified patch.
//! - [`comments`]: the review comments — their ids, their excerpt, the author
//!   of a reply, the list's filter, the re-map of an anchor, what a reply or a
//!   decision changes, and the one message a batch hands the manager.

pub mod comments;
pub mod diff;
