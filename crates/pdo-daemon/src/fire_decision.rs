//! Pure firing decision for a Trigger tick.
//!
//! Given the relevant trigger state plus the observable world at tick time
//! (whether the trigger is due, whether its own previous Run is still live, and
//! the optional guard result), decide whether to fire a Run, skip this tick, or
//! reject (a misconfiguration surfaced at fire time).
//!
//! This is the routing brain of the scheduler, deliberately free of I/O so its
//! branch matrix (ADR-0012 / CONTEXT.md → *Trigger*) is exhaustively
//! unit-testable. The scheduler feeds it facts; it returns a verdict.

/// The overlap policy of a Trigger: what to do when the Trigger's own previous
/// Run is still live at fire time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlapPolicy {
    /// Skip this tick while the previous Run is live (the default).
    Skip,
    /// Allow a concurrent fire.
    Allow,
}

/// Outcome of running a guard command (slice #161 will populate this; the
/// cron-only scheduler always passes `None`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuardResult {
    /// Guard exited 0; its (possibly empty) stdout becomes the input source.
    Pass { stdout: String },
    /// Guard exited non-zero: no work to do, skip without error.
    Skip,
    /// Guard could not be evaluated (spawn error or timeout).
    Error { detail: String },
}

/// Inputs to a firing decision — the trigger facts and the world at tick time.
#[derive(Debug, Clone)]
pub struct FireInputs<'a> {
    pub enabled: bool,
    pub due: bool,
    pub overlap: OverlapPolicy,
    pub has_live_run: bool,
    /// `None` when the trigger has no guard (cron-only).
    pub guard: Option<GuardResult>,
    /// Static input template configured on the trigger (may be empty).
    pub input_template: &'a str,
    /// Whether the target pipeline requires a prompt (`prompt_required` flag).
    pub prompt_required: bool,
}

/// The verdict for a tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FireDecision {
    /// Spawn a Run with this resolved input.
    Fire { input: String },
    /// Do nothing this tick (also covers disabled / not-due) and, when a
    /// reason is present, record an audit row.
    Skip { reason: Option<SkipReason> },
    /// Refuse to fire: a misconfiguration. Recorded as an error outcome.
    Reject { reason: String },
}

/// Why a tick was skipped (drives the `trigger_fires` audit outcome).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkipReason {
    /// The Trigger's own previous Run is still active.
    OverlapPreviousRunLive,
    /// Guard exited non-zero (no work to do).
    GuardExitNonZero,
    /// Guard could not be evaluated (spawn error or timeout).
    GuardError { detail: String },
}

/// Decide what to do for a single Trigger tick.
pub fn decide(inputs: &FireInputs) -> FireDecision {
    // Disabled or not due: a silent no-op (no audit row).
    if !inputs.enabled || !inputs.due {
        return FireDecision::Skip { reason: None };
    }

    // Overlap policy: never stack on the Trigger's own live Run unless allowed.
    if inputs.has_live_run && inputs.overlap == OverlapPolicy::Skip {
        return FireDecision::Skip {
            reason: Some(SkipReason::OverlapPreviousRunLive),
        };
    }

    // Guard branches (slice #161). Cron-only triggers pass `None`.
    let guard_stdout = match &inputs.guard {
        Some(GuardResult::Skip) => {
            return FireDecision::Skip {
                reason: Some(SkipReason::GuardExitNonZero),
            };
        }
        Some(GuardResult::Error { detail }) => {
            return FireDecision::Skip {
                reason: Some(SkipReason::GuardError {
                    detail: detail.clone(),
                }),
            };
        }
        Some(GuardResult::Pass { stdout }) => Some(stdout.as_str()),
        None => None,
    };

    // Input resolution order: guard stdout (if non-empty) → input_template → none.
    let resolved_input = resolve_input(guard_stdout, inputs.input_template);

    // A prompt-required pipeline with no resolvable input is a misconfiguration.
    if resolved_input.trim().is_empty() && inputs.prompt_required {
        return FireDecision::Reject {
            reason: "this pipeline requires a prompt; add a guard, an input \
                     template, or mark the pipeline prompt-not-required"
                .to_string(),
        };
    }

    FireDecision::Fire {
        input: resolved_input,
    }
}

/// Resolve the Run input: guard stdout when present and non-empty, else the
/// static template. No merging in v1 (CONTEXT.md → *Trigger*).
fn resolve_input(guard_stdout: Option<&str>, template: &str) -> String {
    match guard_stdout {
        Some(s) if !s.trim().is_empty() => s.to_string(),
        _ => template.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> FireInputs<'static> {
        FireInputs {
            enabled: true,
            due: true,
            overlap: OverlapPolicy::Skip,
            has_live_run: false,
            guard: None,
            input_template: "do the thing",
            prompt_required: true,
        }
    }

    #[test]
    fn disabled_trigger_is_a_silent_noop() {
        let inputs = FireInputs {
            enabled: false,
            ..base()
        };
        assert_eq!(decide(&inputs), FireDecision::Skip { reason: None });
    }

    #[test]
    fn not_due_trigger_is_a_silent_noop() {
        let inputs = FireInputs {
            due: false,
            ..base()
        };
        assert_eq!(decide(&inputs), FireDecision::Skip { reason: None });
    }

    #[test]
    fn due_with_live_run_and_skip_policy_skips_with_overlap_reason() {
        let inputs = FireInputs {
            has_live_run: true,
            overlap: OverlapPolicy::Skip,
            ..base()
        };
        assert_eq!(
            decide(&inputs),
            FireDecision::Skip {
                reason: Some(SkipReason::OverlapPreviousRunLive)
            }
        );
    }

    #[test]
    fn due_with_live_run_and_allow_policy_fires() {
        let inputs = FireInputs {
            has_live_run: true,
            overlap: OverlapPolicy::Allow,
            ..base()
        };
        assert_eq!(
            decide(&inputs),
            FireDecision::Fire {
                input: "do the thing".to_string()
            }
        );
    }

    #[test]
    fn cron_only_due_trigger_fires_with_input_template() {
        // No guard, due, no live run: fire with the static template.
        let inputs = base();
        assert_eq!(
            decide(&inputs),
            FireDecision::Fire {
                input: "do the thing".to_string()
            }
        );
    }

    #[test]
    fn guard_exit_nonzero_skips_without_error() {
        let inputs = FireInputs {
            guard: Some(GuardResult::Skip),
            ..base()
        };
        assert_eq!(
            decide(&inputs),
            FireDecision::Skip {
                reason: Some(SkipReason::GuardExitNonZero)
            }
        );
    }

    #[test]
    fn guard_error_skips_with_error_outcome() {
        let inputs = FireInputs {
            guard: Some(GuardResult::Error {
                detail: "timeout".to_string(),
            }),
            ..base()
        };
        assert_eq!(
            decide(&inputs),
            FireDecision::Skip {
                reason: Some(SkipReason::GuardError {
                    detail: "timeout".to_string()
                })
            }
        );
    }

    #[test]
    fn guard_stdout_takes_precedence_over_input_template() {
        let inputs = FireInputs {
            guard: Some(GuardResult::Pass {
                stdout: "issues from guard".to_string(),
            }),
            input_template: "static template",
            ..base()
        };
        assert_eq!(
            decide(&inputs),
            FireDecision::Fire {
                input: "issues from guard".to_string()
            }
        );
    }

    #[test]
    fn empty_guard_stdout_falls_back_to_input_template() {
        let inputs = FireInputs {
            guard: Some(GuardResult::Pass {
                stdout: "   \n".to_string(),
            }),
            input_template: "static template",
            ..base()
        };
        assert_eq!(
            decide(&inputs),
            FireDecision::Fire {
                input: "static template".to_string()
            }
        );
    }

    #[test]
    fn empty_input_with_prompt_required_is_rejected() {
        let inputs = FireInputs {
            input_template: "",
            prompt_required: true,
            ..base()
        };
        match decide(&inputs) {
            FireDecision::Reject { reason } => assert!(reason.contains("requires a prompt")),
            other => panic!("expected Reject, got {other:?}"),
        }
    }

    #[test]
    fn empty_input_without_prompt_required_fires_with_empty_input() {
        let inputs = FireInputs {
            input_template: "",
            prompt_required: false,
            ..base()
        };
        assert_eq!(
            decide(&inputs),
            FireDecision::Fire {
                input: String::new()
            }
        );
    }
}
