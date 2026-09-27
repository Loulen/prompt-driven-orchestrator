//! The scheduler (ADR-0009, layer 3): the one interpreter of the evaluators'
//! actions, and the drivers that feed it (a Run's advance, a node's
//! completion, the retry of waiting nodes, the run-level stall). A transition
//! of the Run: nothing here asks the Autorisation d'opération (D10).

pub mod advance;
mod children_binding;
mod completion_head;
pub mod frontmatter;
pub mod interpreter;
pub mod node_completion;
pub mod stall;
