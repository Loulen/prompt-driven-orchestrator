//! The node routes of a Run: the agent's completion, failure and skip, the
//! canvas buttons (start, stop, retry), and what a node reads (its prompt,
//! its inputs and outputs, its diff).

pub(super) mod gestures;
pub(super) mod read;
pub mod requests;
