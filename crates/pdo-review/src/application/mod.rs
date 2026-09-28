//! The use cases of the review, over its ports.
//!
//! The reads — a Run's raw diff ([`run_diff`]), its structured diff
//! ([`structured_diff`]), its comments ([`list_comments`]) — and the gestures
//! on the comments: send a batch ([`send_comments`]), reply ([`reply`]),
//! resolve or reopen ([`decide`]). Every gesture receives the Acteur
//! `identify` set and asks the Autorisation d'opération before it reads the
//! Run ([`operations`]).

mod comment_events;
pub mod decide;
pub mod failure;
pub mod list_comments;
pub mod operations;
pub mod ports;
pub mod reply;
pub mod review;
pub mod run_diff;
pub mod send_comments;
pub mod structured_diff;
