//! Choosing how to recover an `Interrupted` node (#599, ADR-0049 §3).
//!
//! "La session est morte, pas le travail." An infra incident parks a node
//! `Interrupted` (ADR-0049); a human then recovers it by one of two mechanisms:
//!
//! - **Re-attach** the SAME agent session in the existing sub-worktree, on the
//!   harness's resume tail (`claude --continue`) — the optimal path, because the
//!   agent's whole conversation survives. It is **conditioned on a declared
//!   harness capability** (ADR-0045): only a harness whose descriptor carries a
//!   resume tail (`HarnessDescriptor::can_resume()`) can be continued.
//! - **Restart with the partial artifacts fed back in as input** — a FRESH agent,
//!   handed the interrupted node's partial output as context, never writing over
//!   it. Harness-agnostic, so it is the **default** and the **automatic fallback**
//!   when the harness cannot resume.
//!
//! This module is the pure decision — no I/O, no clock — so the fallback rule
//! (ADR-0049 §3: "repli automatique sur restart-avec-artefacts si le harnais ne
//! sait pas reprendre") is a one-line function a unit test pins, rather than a
//! branch buried in the command dispatcher. The dispatcher (`POST /runs/{id}/commands`)
//! reads `can_resume()` off the frozen harness descriptor and asks this module
//! which mechanism to run.

/// How a `recover_node` command will recover an `Interrupted` node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryMechanism {
    /// Re-attach the existing session in place (`claude --continue`). The optimal
    /// path — chosen only when the harness declares a resume tail (ADR-0045).
    Reattach,
    /// Spawn a fresh agent with the partial artifacts fed back as input. The
    /// default, and the automatic fallback when the harness cannot resume.
    RestartWithArtifacts,
}

impl RecoveryMechanism {
    /// The stable wire token a `recover_node` response reports, so a client can
    /// tell which mechanism actually ran (and, in particular, observe the
    /// automatic fallback).
    pub fn as_str(self) -> &'static str {
        match self {
            RecoveryMechanism::Reattach => "reattach",
            RecoveryMechanism::RestartWithArtifacts => "restart_with_artifacts",
        }
    }
}

/// What PDO can tell about the conversation a re-attach would resume.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Conversation {
    /// The harness's transcript of this iteration is on disk.
    OnDisk,
    /// The harness writes a transcript PDO can find, and there is none for this
    /// iteration: the session died before the agent wrote its first turn, or its
    /// store is gone. `claude --resume` answers « No conversation found » and
    /// exits, so a re-attach would only kill the session again.
    Missing,
    /// PDO reads no transcript for this harness: it cannot tell.
    Unknown,
}

/// Why a harness that can resume was restarted instead — said in the
/// `recover_node` response, so the fallback is never silent.
pub const NO_CONVERSATION_TO_RESUME: &str =
    "no conversation to resume: the session died before its agent wrote one, so the node \
     starts fresh with its partial artifacts";

/// Pick the recovery mechanism for a node whose frozen harness `can_resume` (or
/// not) — ADR-0049 §3. The fallback is **automatic** — it is not a second human
/// decision — which is why it lives here and not in a UI.
///
/// A harness that can resume is only re-attached when there is something to
/// resume: a conversation PDO knows is missing falls back to the restart too.
pub fn choose_recovery(harness_can_resume: bool, conversation: Conversation) -> RecoveryMechanism {
    if harness_can_resume && conversation != Conversation::Missing {
        RecoveryMechanism::Reattach
    } else {
        RecoveryMechanism::RestartWithArtifacts
    }
}

/// The sentence a fallback carries, when it is not the harness's own limit.
pub fn fallback_reason(
    harness_can_resume: bool,
    conversation: Conversation,
) -> Option<&'static str> {
    (harness_can_resume && conversation == Conversation::Missing)
        .then_some(NO_CONVERSATION_TO_RESUME)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_resuming_harness_reattaches() {
        assert_eq!(
            choose_recovery(true, Conversation::OnDisk),
            RecoveryMechanism::Reattach
        );
        // A harness PDO reads no transcript for: nothing says the resume would fail.
        assert_eq!(
            choose_recovery(true, Conversation::Unknown),
            RecoveryMechanism::Reattach
        );
    }

    #[test]
    fn a_non_resuming_harness_falls_back_to_restart_with_artifacts() {
        assert_eq!(
            choose_recovery(false, Conversation::OnDisk),
            RecoveryMechanism::RestartWithArtifacts
        );
    }

    #[test]
    fn a_resuming_harness_with_no_conversation_restarts_and_says_why() {
        assert_eq!(
            choose_recovery(true, Conversation::Missing),
            RecoveryMechanism::RestartWithArtifacts
        );
        assert_eq!(
            fallback_reason(true, Conversation::Missing),
            Some(NO_CONVERSATION_TO_RESUME)
        );
        assert_eq!(fallback_reason(true, Conversation::OnDisk), None);
        assert_eq!(fallback_reason(false, Conversation::Missing), None);
    }

    #[test]
    fn wire_tokens_are_stable() {
        assert_eq!(RecoveryMechanism::Reattach.as_str(), "reattach");
        assert_eq!(
            RecoveryMechanism::RestartWithArtifacts.as_str(),
            "restart_with_artifacts"
        );
    }
}
