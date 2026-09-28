//! The Operations the gestures on review comments ask the Autorisation
//! d'opération about (ADR-0081): a send creates comments on a Run, a reply,
//! a resolution or a reopening acts on one comment, targeted `<run>/<comment>`.

use pdo_kernel::{Operation, ResourceKind, Verb};

/// Sending a batch of drafts on the Run `run_id`.
pub fn send(run_id: &str) -> Operation {
    Operation::new(ResourceKind::ReviewComment, Verb::Create).on(run_id)
}

/// `action` on the comment `comment_id` of `run_id`.
pub fn on_comment(action: &str, run_id: &str, comment_id: &str) -> Operation {
    Operation::new(
        ResourceKind::ReviewComment,
        Verb::Action(action.to_string()),
    )
    .on(format!("{run_id}/{comment_id}"))
}
