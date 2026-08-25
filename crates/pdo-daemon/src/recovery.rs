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
//! branch buried in the command dispatcher. The dispatcher ([`crate::run_command`])
//! reads `can_resume()` off the frozen harness descriptor and asks this module
//! which mechanism to run.

/// How a `recover_node` command will recover an `Interrupted` node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecoveryMechanism {
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
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            RecoveryMechanism::Reattach => "reattach",
            RecoveryMechanism::RestartWithArtifacts => "restart_with_artifacts",
        }
    }
}

/// Pick the recovery mechanism for a node whose frozen harness `can_resume` (or
/// not) — ADR-0049 §3.
///
/// The whole rule: re-attach when the harness can resume, else fall back to
/// restart-with-artifacts. The fallback is **automatic** — it is not a second
/// human decision — which is why it lives here and not in a UI.
pub(crate) fn choose_recovery(harness_can_resume: bool) -> RecoveryMechanism {
    if harness_can_resume {
        RecoveryMechanism::Reattach
    } else {
        RecoveryMechanism::RestartWithArtifacts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ADR-0049 §3: the optimal path is re-attach, and it is conditioned on the
    /// harness declaring a resume capability (ADR-0045).
    #[test]
    fn a_resuming_harness_reattaches() {
        assert_eq!(choose_recovery(true), RecoveryMechanism::Reattach);
    }

    /// The automatic fallback: a harness that cannot resume never strands the
    /// node — it restarts with the partial artifacts fed back as input.
    #[test]
    fn a_non_resuming_harness_falls_back_to_restart_with_artifacts() {
        assert_eq!(
            choose_recovery(false),
            RecoveryMechanism::RestartWithArtifacts
        );
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
