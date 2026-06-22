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
    /// Allow a concurrent fire — unbounded, or capped by `max_concurrent` (#239).
    Allow,
}

/// Effective concurrency ceiling for one tick. `None` = unbounded (#239).
///
///   Skip        → Some(1)  (never stack on my own live run)
///   Allow+None  → None     (unbounded)
///   Allow+Some  → Some(m)  (m clamped to >= 1 defensively, so a stray 0 from a
///                           legacy/unvalidated row can never make a bounded-allow
///                           Trigger fire forever — `n < 0` is never true)
///
/// The scheduler's guard-gate and `decide` both route through here so the cap can
/// never drift between "should the guard run?" and "should we fire?".
pub fn overlap_ceiling(overlap: OverlapPolicy, max_concurrent: Option<usize>) -> Option<usize> {
    match overlap {
        OverlapPolicy::Skip => Some(1),
        OverlapPolicy::Allow => max_concurrent.map(|m| m.max(1)),
    }
}

/// Outcome of running a guard command. The guard is live (wired in `lib.rs`);
/// cron-only triggers (no guard) pass `None` to the decision core.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuardResult {
    /// Guard exited 0; its (possibly empty) stdout becomes the input source.
    Pass { stdout: String },
    /// Guard exited non-zero: no work to do, skip without error. Carries what the
    /// guard printed (tail-capped diagnostics) so the fire history can explain
    /// *why* it skipped (#244). `exit_code` is `None` when signal-killed.
    Skip {
        stdout: String,
        stderr: String,
        exit_code: Option<i32>,
    },
    /// Guard could not be evaluated (spawn error or timeout).
    Error { detail: String },
}

/// Inputs to a firing decision — the trigger facts and the world at tick time.
#[derive(Debug, Clone)]
pub struct FireInputs<'a> {
    pub enabled: bool,
    pub due: bool,
    pub overlap: OverlapPolicy,
    /// Count of this Trigger's *own* live Runs at tick time (#239). With
    /// `OverlapPolicy::Skip` any count >= 1 skips; with `Allow` it is compared
    /// against the `max_concurrent` ceiling.
    pub live_run_count: usize,
    /// Bounded-`allow` ceiling: max simultaneous live Runs (#239). `None` =
    /// unbounded. Ignored entirely unless `overlap == Allow`.
    pub max_concurrent: Option<usize>,
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
    /// The Trigger is at its bounded-`allow` ceiling: `live` >= `max` (#239).
    OverlapMaxConcurrentReached { live: usize, max: usize },
    /// Guard exited non-zero (no work to do). Carries the guard's captured
    /// stdout/stderr/exit code so the fire history can explain the skip (#244).
    GuardExitNonZero {
        stdout: String,
        stderr: String,
        exit_code: Option<i32>,
    },
    /// Guard could not be evaluated (spawn error or timeout).
    GuardError { detail: String },
}

/// Decide what to do for a single Trigger tick.
pub fn decide(inputs: &FireInputs) -> FireDecision {
    // Disabled or not due: a silent no-op (no audit row).
    if !inputs.enabled || !inputs.due {
        return FireDecision::Skip { reason: None };
    }

    // Overlap policy collapses to one effective ceiling (#239): `skip` ⇒ 1,
    // `allow+None` ⇒ unbounded, `allow+Some(m)` ⇒ m. Fire iff the count is below
    // the ceiling.
    if let Some(ceiling) = overlap_ceiling(inputs.overlap, inputs.max_concurrent) {
        if inputs.live_run_count >= ceiling {
            let reason = match inputs.overlap {
                OverlapPolicy::Skip => SkipReason::OverlapPreviousRunLive,
                OverlapPolicy::Allow => SkipReason::OverlapMaxConcurrentReached {
                    live: inputs.live_run_count,
                    max: ceiling,
                },
            };
            return FireDecision::Skip {
                reason: Some(reason),
            };
        }
    }

    // Guard branches. Cron-only triggers pass `None`.
    let guard_stdout = match &inputs.guard {
        Some(GuardResult::Skip {
            stdout,
            stderr,
            exit_code,
        }) => {
            return FireDecision::Skip {
                reason: Some(SkipReason::GuardExitNonZero {
                    stdout: stdout.clone(),
                    stderr: stderr.clone(),
                    exit_code: *exit_code,
                }),
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
            live_run_count: 0,
            max_concurrent: None,
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
            live_run_count: 1,
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
            live_run_count: 1,
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

    // --- #239: bounded-`allow` concurrency cap ---

    #[test]
    fn allow_unbounded_fires_regardless_of_count() {
        let inputs = FireInputs {
            overlap: OverlapPolicy::Allow,
            max_concurrent: None,
            live_run_count: 5,
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
    fn allow_bounded_fires_below_cap() {
        let inputs = FireInputs {
            overlap: OverlapPolicy::Allow,
            max_concurrent: Some(2),
            live_run_count: 1,
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
    fn allow_bounded_skips_at_cap() {
        let inputs = FireInputs {
            overlap: OverlapPolicy::Allow,
            max_concurrent: Some(2),
            live_run_count: 2,
            ..base()
        };
        assert_eq!(
            decide(&inputs),
            FireDecision::Skip {
                reason: Some(SkipReason::OverlapMaxConcurrentReached { live: 2, max: 2 })
            }
        );
    }

    #[test]
    fn allow_bounded_skips_above_cap() {
        let inputs = FireInputs {
            overlap: OverlapPolicy::Allow,
            max_concurrent: Some(2),
            live_run_count: 3,
            ..base()
        };
        assert_eq!(
            decide(&inputs),
            FireDecision::Skip {
                reason: Some(SkipReason::OverlapMaxConcurrentReached { live: 3, max: 2 })
            }
        );
    }

    #[test]
    fn skip_policy_ignores_max_concurrent() {
        // Regression: a stray `max_concurrent` is inert under the `skip` policy —
        // the ceiling is always 1, so any live run skips with the previous-run
        // reason (not the bounded-allow one).
        let inputs = FireInputs {
            overlap: OverlapPolicy::Skip,
            max_concurrent: Some(5),
            live_run_count: 1,
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
    fn overlap_ceiling_collapses_each_mode() {
        assert_eq!(overlap_ceiling(OverlapPolicy::Skip, None), Some(1));
        assert_eq!(overlap_ceiling(OverlapPolicy::Skip, Some(9)), Some(1));
        assert_eq!(overlap_ceiling(OverlapPolicy::Allow, None), None);
        assert_eq!(overlap_ceiling(OverlapPolicy::Allow, Some(3)), Some(3));
        // Defensive clamp: a stray 0 must never become an unfireable ceiling.
        assert_eq!(overlap_ceiling(OverlapPolicy::Allow, Some(0)), Some(1));
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
            guard: Some(GuardResult::Skip {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: Some(1),
            }),
            ..base()
        };
        assert_eq!(
            decide(&inputs),
            FireDecision::Skip {
                reason: Some(SkipReason::GuardExitNonZero {
                    stdout: String::new(),
                    stderr: String::new(),
                    exit_code: Some(1),
                })
            }
        );
    }

    #[test]
    fn guard_exit_nonzero_carries_captured_output_into_skip_reason() {
        // #244: a non-zero guard's stdout, stderr, and exit code flow through the
        // decision into the audit reason so the fire history can explain the skip.
        let inputs = FireInputs {
            guard: Some(GuardResult::Skip {
                stdout: "checked 0 issues".to_string(),
                stderr: "gh: no work to do".to_string(),
                exit_code: Some(7),
            }),
            ..base()
        };
        assert_eq!(
            decide(&inputs),
            FireDecision::Skip {
                reason: Some(SkipReason::GuardExitNonZero {
                    stdout: "checked 0 issues".to_string(),
                    stderr: "gh: no work to do".to_string(),
                    exit_code: Some(7),
                })
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
