//! The orphan sweep's verdicts (ADR-0038), pure: which live `pdo-*` session is
//! an orphan, given the tmux inventory and its owners' facts. No tmux, no
//! database, no clock inside — the application's sweep gathers the facts, then
//! applies the decisions.

mod kill_reason;

use std::time::Duration;

pub use kill_reason::KillReason;

use crate::domain::names::ParsedSession;

/// Information the reaper needs about a NodeRun to decide whether to reap.
///
/// A *facts* type on purpose (#485, ADR-0038): this deep module has no
/// dependency on the Run's event log, so the caller projects the run and hands
/// the two facts down. Keeping the coupling this way round is what lets
/// [`decide_sweep`] stay pure and unit-testable without a database.
#[derive(Debug, Clone, PartialEq)]
pub struct NodeRunInfo {
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub is_archived: bool,
}

// The orphan sweep runs **at daemon boot AND on the periodic reaper loop**
// (`DEFAULT_REAPER_INTERVAL`). That distinction is load-bearing (#485,
// ADR-0038): only the periodic pass runs against live spawns, so only it can
// race one. The boot pass cannot — it runs before the router is built, so no
// request and no scheduler tick is concurrent with it.

/// One live `pdo-*` session, plus everything the reaper knows about its owner.
///
/// **The inventory is an input, never something the sweep fetches for itself
/// (#485, ADR-0038).** If the sweep listed the tmux sessions itself, the order
/// of its two observations — the tmux inventory and the event-log read — could not
/// be guaranteed by the caller; reading the log first kills a session born between
/// the two ~150 ms after its own spawn. Keying the input *per session* makes the
/// correct order the path of least resistance: `info` cannot be filled without
/// already holding the names.
#[derive(Debug, Clone)]
pub struct SweepInput {
    pub session_name: String,
    /// `None` = [`parse_session_name`](crate::domain::names::parse_session_name) refused the name (unconditional-kill arm).
    pub parsed: Option<ParsedSession>,
    /// Its owner's facts, resolved **after** the inventory. `None` = absent from
    /// the event log.
    pub info: Option<NodeRunInfo>,
    /// Whether a tmux client is attached to this session (#594) — filled from
    /// [`Tmux::is_attached`](crate::application::ports::Tmux::is_attached) for [`ParsedSession::LibAssist`] only, `false` elsewhere.
    /// The other arms have their own owner to key on and never consult it.
    pub attached: bool,
    /// How long ago the UI last declared an open edit view (#594) — filled for
    /// [`ParsedSession::LibAssist`] only. `None` means **never declared**, which
    /// is what a browser reload leaves behind: it is an idle verdict, not a fresh
    /// one. Same clock as `now`, resolved by the caller in the same pass.
    pub focus_age: Option<Duration>,
}

/// The TTLs the sweep arbitrates on. Named fields rather than two positional
/// `Duration`s: they differ by an order of magnitude and by *meaning* (a
/// completed node's session vs a library assistant nobody is using), so swapping
/// them at a call site would compile and then reap the wrong thing quietly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SweepTtls {
    /// A completed NodeRun's session, the only pre-#594 TTL. See [`reaper_ttl_with`](crate::adapters::env::reaper_ttl_with).
    pub node_run: Duration,
    /// The shared library assistant with nobody attached and no fresh focus.
    /// See [`libassist_idle_ttl_with`](crate::adapters::env::libassist_idle_ttl_with).
    pub libassist_idle: Duration,
}

/// What the sweep decided about one live session. One per input, `Keep`
/// included, so a test can assert the *absence* of a kill as strongly as its
/// presence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SweepDecision {
    pub session_name: String,
    pub verdict: SweepVerdict,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SweepVerdict {
    Keep,
    Kill(KillReason),
}

/// What one sweep pass actually killed, for the `reaper` gauge on
/// `GET /sessions` (#485). Strictly the **pass's own** tally — this type stays
/// per-pass and stateless; the caller is what accumulates it into the two
/// since-boot counters it publishes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SweepTally {
    pub killed: i64,
    pub killed_for_absent_run: i64,
}

/// Decide, for every live session, whether it is an orphan — **pure**.
///
/// ADR-0009 layer 1: no tmux, no DB, no clock. `now` and `ttls` are injected so
/// the TTL arms are deterministic in a test, and every input yields a decision
/// (`Keep` included) so a test can pin "this session survives" as hard as
/// "this one dies".
///
/// An orphan is a `pdo-*` session whose owner is:
/// - archived,
/// - absent from the event log,
/// - or a NodeRun that completed more than `ttls.node_run` ago (NodeRun only —
///   #316/#458).
///
/// The library assistant owns no Run and is judged on human presence instead
/// (#594) — see the `LibAssist` arm of [`decide_one`].
///
/// **Correctness precondition, and this function cannot enforce it (#485,
/// ADR-0038):** `info` must have been resolved from a log read that happened
/// *after* the inventory that produced `session_name`. The log only grows, so an
/// absence observed after the inventory implies absence *at* the inventory —
/// which is what makes the `Absent` verdicts sound. The reverse order has no
/// symmetric proof.
pub fn decide_sweep(
    inputs: &[SweepInput],
    ttls: SweepTtls,
    now: chrono::DateTime<chrono::Utc>,
) -> Vec<SweepDecision> {
    inputs
        .iter()
        .map(|input| SweepDecision {
            session_name: input.session_name.clone(),
            verdict: decide_one(input, ttls, now),
        })
        .collect()
}

fn decide_one(
    input: &SweepInput,
    ttls: SweepTtls,
    now: chrono::DateTime<chrono::Utc>,
) -> SweepVerdict {
    let parsed = match &input.parsed {
        Some(p) => p,
        None => {
            return SweepVerdict::Kill(KillReason::UnrecognisedName {
                session_name: input.session_name.clone(),
            })
        }
    };

    match parsed {
        ParsedSession::Manager { run_id } => match &input.info {
            None => SweepVerdict::Kill(KillReason::ManagerRunAbsent {
                run_id: run_id.clone(),
            }),
            Some(info) if info.is_archived => SweepVerdict::Kill(KillReason::ManagerRunArchived {
                run_id: run_id.clone(),
            }),
            // No TTL arm (#458): a manager outlives its run's completion.
            _ => SweepVerdict::Keep,
        },
        ParsedSession::Shell { run_id } => match &input.info {
            // #316: mirror the Manager arm — reap iff the run is absent or
            // archived, NEVER on a TTL (an interactive shell must not be
            // yanked from a user who stepped away). The `__shell__` lookup
            // branch supplies the run's archived flag.
            None => SweepVerdict::Kill(KillReason::ShellRunAbsent {
                run_id: run_id.clone(),
            }),
            Some(info) if info.is_archived => SweepVerdict::Kill(KillReason::ShellRunArchived {
                run_id: run_id.clone(),
            }),
            _ => SweepVerdict::Keep,
        },
        // #594 / ADR-0051 §4. Don't make this an unconditional `Keep`: the
        // assistant owns no Run, so absence/archived say nothing about it, and the
        // explicit `DELETE` is not enough — React runs no effect cleanup when the
        // document unloads, so a reload or a closed tab leaks the session forever.
        //
        // Three verdicts on human presence, in this order, and the order matters:
        //   1. attached — someone has a terminal open on it. Never yank that, even
        //      if the focus is stale (a user reading the pane declares no focus).
        //   2. fresh focus — an edit view is open (the UI re-declares every 20 s),
        //      even with the Assistant tab hidden. That is the owner's point 2:
        //      "do not reap while I am editing". The info panel closes by itself on
        //      every tab switch (#385), so attachment alone cannot answer this.
        //   3. otherwise idle — nobody attached, nobody editing. `None` (no focus
        //      ever declared) belongs here, not in `Keep`: it is exactly the state
        //      a reload leaves behind.
        // `info` is always `None` for this variant (the caller does no run lookup).
        ParsedSession::LibAssist => {
            if input.attached {
                return SweepVerdict::Keep;
            }
            match input.focus_age {
                Some(age) if age < ttls.libassist_idle => SweepVerdict::Keep,
                other => SweepVerdict::Kill(KillReason::LibAssistIdle {
                    session_name: input.session_name.clone(),
                    idle_secs: other.map(|a| a.as_secs() as i64),
                }),
            }
        }
        ParsedSession::NodeRun {
            run_id,
            node_id,
            iter: _,
        } => match &input.info {
            None => SweepVerdict::Kill(KillReason::NodeRunAbsent {
                run_id: run_id.clone(),
                node_id: node_id.clone(),
            }),
            Some(info) if info.is_archived => SweepVerdict::Kill(KillReason::NodeRunArchived {
                run_id: run_id.clone(),
                node_id: node_id.clone(),
            }),
            Some(NodeRunInfo {
                completed_at: Some(completed),
                ..
            }) => {
                let age = now.signed_duration_since(*completed);
                if age
                    > chrono::Duration::from_std(ttls.node_run)
                        .unwrap_or(chrono::Duration::hours(1))
                {
                    SweepVerdict::Kill(KillReason::NodeRunStale {
                        session_name: input.session_name.clone(),
                        age_secs: age.num_seconds(),
                    })
                } else {
                    SweepVerdict::Keep
                }
            }
            _ => SweepVerdict::Keep, // still running or not yet completed
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::names::{
        manager_session_name, node_session_name, parse_session_name, shell_session_name,
        LIBASSIST_SESSION_NAME,
    };

    const RID: &str = "20260731-153057-553fcb3";

    fn at(iso: &str) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::parse_from_rfc3339(iso)
            .unwrap()
            .with_timezone(&chrono::Utc)
    }

    /// One input, already resolved: the session name plus its owner's facts.
    /// Detached with no focus — the presence fields only matter to the LibAssist
    /// arm, which has its own builder below.
    fn input(session_name: &str, info: Option<NodeRunInfo>) -> SweepInput {
        SweepInput {
            session_name: session_name.to_string(),
            parsed: parse_session_name(session_name),
            info,
            attached: false,
            focus_age: None,
        }
    }

    fn live() -> Option<NodeRunInfo> {
        Some(NodeRunInfo {
            completed_at: None,
            is_archived: false,
        })
    }

    /// The TTLs used by every sweep unit test: the historical 1 h NodeRun TTL,
    /// and 120 s of assistant idleness.
    const TTLS: SweepTtls = SweepTtls {
        node_run: Duration::from_secs(3600),
        libassist_idle: Duration::from_secs(120),
    };

    fn verdict(inputs: &[SweepInput], now: chrono::DateTime<chrono::Utc>) -> SweepVerdict {
        let decisions = decide_sweep(inputs, TTLS, now);
        assert_eq!(decisions.len(), inputs.len(), "one decision per input");
        decisions[0].verdict.clone()
    }

    /// **The test #485 asks for.** A node whose reservation IS visible in the log
    /// survives — reproduced with no dependency on timing at all, because the
    /// snapshot is a parameter.
    #[test]
    fn young_session_of_a_live_node_survives() {
        let name = node_session_name(RID, "alpha", 1);
        assert_eq!(
            verdict(&[input(&name, live())], at("2026-07-31T15:32:19Z")),
            SweepVerdict::Keep
        );
    }

    /// **The twin, and it is non-negotiable.** Without it, a "fix" that simply
    /// neutralises the sweep passes the test above and lets sessions pile up
    /// toward the ~30-session tmux collapse point (#77/#78).
    #[test]
    fn genuinely_absent_node_is_still_killed() {
        let name = node_session_name(RID, "alpha", 1);
        assert_eq!(
            verdict(&[input(&name, None)], at("2026-07-31T15:32:19Z")),
            SweepVerdict::Kill(KillReason::NodeRunAbsent {
                run_id: RID.into(),
                node_id: "alpha".into(),
            })
        );
    }

    #[test]
    fn archived_run_is_killed_on_every_arm() {
        let archived = || {
            Some(NodeRunInfo {
                completed_at: None,
                is_archived: true,
            })
        };
        let now = at("2026-07-31T15:32:19Z");

        let node = node_session_name(RID, "alpha", 1);
        assert_eq!(
            verdict(&[input(&node, archived())], now),
            SweepVerdict::Kill(KillReason::NodeRunArchived {
                run_id: RID.into(),
                node_id: "alpha".into(),
            })
        );
        assert_eq!(
            verdict(&[input(&manager_session_name(RID), archived())], now),
            SweepVerdict::Kill(KillReason::ManagerRunArchived { run_id: RID.into() })
        );
        assert_eq!(
            verdict(&[input(&shell_session_name(RID), archived())], now),
            SweepVerdict::Kill(KillReason::ShellRunArchived { run_id: RID.into() })
        );
    }

    #[test]
    fn absent_run_is_killed_on_manager_and_shell_arms_too() {
        let now = at("2026-07-31T15:32:19Z");
        assert_eq!(
            verdict(&[input(&manager_session_name(RID), None)], now),
            SweepVerdict::Kill(KillReason::ManagerRunAbsent { run_id: RID.into() })
        );
        assert_eq!(
            verdict(&[input(&shell_session_name(RID), None)], now),
            SweepVerdict::Kill(KillReason::ShellRunAbsent { run_id: RID.into() })
        );
    }

    /// A library-assistant input: `info` is always `None` (it owns no Run), so
    /// the only facts are the two presence signals.
    fn libassist_input(attached: bool, focus_age: Option<Duration>) -> SweepInput {
        SweepInput {
            session_name: LIBASSIST_SESSION_NAME.to_string(),
            parsed: parse_session_name(LIBASSIST_SESSION_NAME),
            info: None,
            attached,
            focus_age,
        }
    }

    /// #594 / ADR-0051 §4: presence, not the owning Run, decides the assistant's
    /// fate. The three verdicts are checked in their order of precedence, plus the
    /// boundary.
    #[test]
    fn library_assistant_is_kept_while_someone_is_there() {
        let now = at("2026-07-31T15:32:19Z");
        let ttl = TTLS.libassist_idle;

        // 1. Attached wins over everything: a stale focus does not kill a session
        //    someone has a terminal open on.
        assert_eq!(
            verdict(
                &[libassist_input(true, Some(ttl + Duration::from_secs(60)))],
                now
            ),
            SweepVerdict::Keep,
            "an attached terminal is never yanked"
        );
        assert_eq!(
            verdict(&[libassist_input(true, None)], now),
            SweepVerdict::Keep
        );

        // 2. Detached but the user is editing (the UI re-declared its focus within
        //    the TTL) — the owner's point 2, and the reason attachment alone is not
        //    the oracle: the info panel closes on every edit-tab switch (#385).
        assert_eq!(
            verdict(
                &[libassist_input(false, Some(ttl - Duration::from_secs(1)))],
                now
            ),
            SweepVerdict::Keep,
            "a fresh focus keeps the session even with no terminal attached"
        );
    }

    /// The twin, non-negotiable half: without it a "fix" that simply keeps the
    /// assistant forever passes the test above and restores the unbounded leak.
    #[test]
    fn library_assistant_is_killed_once_nobody_is_there() {
        let now = at("2026-07-31T15:32:19Z");
        let ttl = TTLS.libassist_idle;

        // Detached, focus expired.
        assert_eq!(
            verdict(
                &[libassist_input(false, Some(ttl + Duration::from_secs(1)))],
                now
            ),
            SweepVerdict::Kill(KillReason::LibAssistIdle {
                session_name: LIBASSIST_SESSION_NAME.into(),
                idle_secs: Some(121),
            })
        );

        // Detached, focus NEVER declared — the browser-reload case (#594): the
        // React cleanup never runs on unload, so no `DELETE` is ever sent and the
        // reloaded page opens no edit tab. `None` must reap, not keep.
        assert_eq!(
            verdict(&[libassist_input(false, None)], now),
            SweepVerdict::Kill(KillReason::LibAssistIdle {
                session_name: LIBASSIST_SESSION_NAME.into(),
                idle_secs: None,
            })
        );
    }

    /// The idle boundary is `<`, strictly: `age == ttl` reaps. Opposite of the
    /// NodeRun arm's `>` (where `age == ttl` keeps) — deliberately, because here
    /// the freshness claim is what must be proven, not the staleness.
    #[test]
    fn libassist_idle_boundary_is_strict() {
        let now = at("2026-07-31T15:32:19Z");
        assert_eq!(
            verdict(&[libassist_input(false, Some(TTLS.libassist_idle))], now),
            SweepVerdict::Kill(KillReason::LibAssistIdle {
                session_name: LIBASSIST_SESSION_NAME.into(),
                idle_secs: Some(120),
            })
        );
    }

    /// An idle assistant is routine housekeeping, not an absence verdict: it must
    /// stay `info!` and out of the `killed_for_absent_run` gauge, or every reload
    /// would train the operator to ignore a permanent warning stream.
    #[test]
    fn libassist_idle_is_not_an_absence_verdict() {
        assert!(!KillReason::LibAssistIdle {
            session_name: LIBASSIST_SESSION_NAME.into(),
            idle_secs: Some(300),
        }
        .is_absence());
    }

    /// An unparseable name is killed without consulting `info` at all — the same
    /// unconditional arm as before #485. A private socket makes this the right
    /// default (#86); the round-trip test above is what guards against our *own*
    /// formatters drifting into it.
    #[test]
    fn unrecognised_name_is_killed_without_a_lookup() {
        let decisions = decide_sweep(
            &[SweepInput {
                session_name: "pdo-ceci-nest-pas-un-nom".into(),
                parsed: None,
                info: live(),
                attached: true,
                focus_age: Some(Duration::ZERO),
            }],
            TTLS,
            at("2026-07-31T15:32:19Z"),
        );
        assert_eq!(
            decisions[0].verdict,
            SweepVerdict::Kill(KillReason::UnrecognisedName {
                session_name: "pdo-ceci-nest-pas-un-nom".into(),
            })
        );
    }

    /// The TTL comparison is `>`, strictly — `age == ttl` keeps. Deterministic
    /// only because `now` is injected.
    #[test]
    fn ttl_boundary_is_strict() {
        let name = node_session_name(RID, "alpha", 1);
        let completed = at("2026-07-31T12:00:00Z");
        let done = |completed| {
            Some(NodeRunInfo {
                completed_at: Some(completed),
                is_archived: false,
            })
        };

        // age == ttl (3600 s) → Keep.
        assert_eq!(
            verdict(&[input(&name, done(completed))], at("2026-07-31T13:00:00Z")),
            SweepVerdict::Keep
        );
        // age == ttl + 1 s → Kill.
        assert_eq!(
            verdict(&[input(&name, done(completed))], at("2026-07-31T13:00:01Z")),
            SweepVerdict::Kill(KillReason::NodeRunStale {
                session_name: name.clone(),
                age_secs: 3601,
            })
        );
    }

    /// An out-of-range `ttl` falls back to one hour, not to "kill everything"
    /// (`chrono::Duration::from_std(..).unwrap_or(hours(1))`).
    #[test]
    fn ttl_out_of_chrono_range_falls_back_to_one_hour() {
        let name = node_session_name(RID, "alpha", 1);
        let info = Some(NodeRunInfo {
            completed_at: Some(at("2026-07-31T12:00:00Z")),
            is_archived: false,
        });
        let absurd = Duration::from_secs(u64::MAX);

        let ttls = SweepTtls {
            node_run: absurd,
            ..TTLS
        };

        // 30 min old, 1 h fallback → Keep.
        let keep = decide_sweep(
            &[input(&name, info.clone())],
            ttls,
            at("2026-07-31T12:30:00Z"),
        );
        assert_eq!(keep[0].verdict, SweepVerdict::Keep);

        // 2 h old → Kill, so the fallback is 1 h and not "never".
        let kill = decide_sweep(&[input(&name, info)], ttls, at("2026-07-31T14:00:00Z"));
        assert!(matches!(
            kill[0].verdict,
            SweepVerdict::Kill(KillReason::NodeRunStale { .. })
        ));
    }

    /// Manager (#458) and Shell (#316) have **no** TTL arm: a long-completed run
    /// keeps both. Do not "unify" the three arms.
    #[test]
    fn manager_and_shell_have_no_ttl_arm() {
        let long_done = || {
            Some(NodeRunInfo {
                completed_at: Some(at("2020-01-01T00:00:00Z")),
                is_archived: false,
            })
        };
        let now = at("2026-07-31T15:32:19Z");

        assert_eq!(
            verdict(&[input(&manager_session_name(RID), long_done())], now),
            SweepVerdict::Keep
        );
        assert_eq!(
            verdict(&[input(&shell_session_name(RID), long_done())], now),
            SweepVerdict::Keep
        );
    }

    /// The nine kill messages, byte for byte. The whole #485 investigation was a
    /// `journalctl | grep "Orphan sweep: killing session for absent run"`, so
    /// these strings are a contract, not cosmetics. Note the two irregularities a
    /// composed formatter would smooth over: the NodeRun arms have no "node" word,
    /// and the stale arm keys off the session name.
    #[test]
    fn kill_reason_messages_are_verbatim() {
        assert_eq!(
            KillReason::UnrecognisedName {
                session_name: "pdo-weird".into()
            }
            .to_string(),
            "Orphan sweep: killing unrecognised session pdo-weird"
        );
        assert_eq!(
            KillReason::ManagerRunAbsent { run_id: RID.into() }.to_string(),
            "Orphan sweep: killing manager session for absent run 20260731-153057-553fcb3"
        );
        assert_eq!(
            KillReason::ManagerRunArchived { run_id: RID.into() }.to_string(),
            "Orphan sweep: killing manager session for archived run 20260731-153057-553fcb3"
        );
        assert_eq!(
            KillReason::ShellRunAbsent { run_id: RID.into() }.to_string(),
            "Orphan sweep: killing shell session for absent run 20260731-153057-553fcb3"
        );
        assert_eq!(
            KillReason::ShellRunArchived { run_id: RID.into() }.to_string(),
            "Orphan sweep: killing shell session for archived run 20260731-153057-553fcb3"
        );
        assert_eq!(
            KillReason::NodeRunAbsent {
                run_id: RID.into(),
                node_id: "alpha".into()
            }
            .to_string(),
            "Orphan sweep: killing session for absent run 20260731-153057-553fcb3/alpha"
        );
        assert_eq!(
            KillReason::NodeRunArchived {
                run_id: RID.into(),
                node_id: "alpha".into()
            }
            .to_string(),
            "Orphan sweep: killing session for archived run 20260731-153057-553fcb3/alpha"
        );
        assert_eq!(
            KillReason::NodeRunStale {
                session_name: "pdo-20260731-153057-553fcb3-alpha-iter-1".into(),
                age_secs: 7200,
            }
            .to_string(),
            "Orphan sweep: killing stale session pdo-20260731-153057-553fcb3-alpha-iter-1 \
             (completed 7200s ago)"
        );
        // #594 — two renderings, because "they stopped editing 300 s ago" and "no
        // tab ever declared a focus" are different operator stories. The second is
        // the reload/close case that had no reap at all before this issue.
        assert_eq!(
            KillReason::LibAssistIdle {
                session_name: LIBASSIST_SESSION_NAME.into(),
                idle_secs: Some(300),
            }
            .to_string(),
            "Orphan sweep: killing idle library assistant pdo-libassist-shared \
             (no edit view for 300s)"
        );
        assert_eq!(
            KillReason::LibAssistIdle {
                session_name: LIBASSIST_SESSION_NAME.into(),
                idle_secs: None,
            }
            .to_string(),
            "Orphan sweep: killing idle library assistant pdo-libassist-shared \
             (no edit view ever declared)"
        );
    }

    /// The level split feeding both `apply_sweep`'s `warn!`/`info!` choice and the
    /// `killed_for_absent_run` gauge: absence (incl. an unparseable name) is the
    /// "can no longer happen" class; archived/stale is nominal housekeeping.
    #[test]
    fn only_absence_verdicts_are_flagged_as_abnormal() {
        for reason in [
            KillReason::UnrecognisedName {
                session_name: "x".into(),
            },
            KillReason::ManagerRunAbsent { run_id: RID.into() },
            KillReason::ShellRunAbsent { run_id: RID.into() },
            KillReason::NodeRunAbsent {
                run_id: RID.into(),
                node_id: "alpha".into(),
            },
        ] {
            assert!(reason.is_absence(), "{reason} should be an absence verdict");
        }
        for reason in [
            KillReason::ManagerRunArchived { run_id: RID.into() },
            KillReason::ShellRunArchived { run_id: RID.into() },
            KillReason::NodeRunArchived {
                run_id: RID.into(),
                node_id: "alpha".into(),
            },
            KillReason::NodeRunStale {
                session_name: "x".into(),
                age_secs: 1,
            },
            KillReason::LibAssistIdle {
                session_name: "x".into(),
                idle_secs: Some(1),
            },
        ] {
            assert!(!reason.is_absence(), "{reason} is routine housekeeping");
        }
    }

    /// `decide_sweep` yields one decision per input, in input order, mixing
    /// verdicts — the property that lets a caller tally kills and a test assert an
    /// absence of kills.
    #[test]
    fn decide_sweep_returns_one_decision_per_input_in_order() {
        let live_node = node_session_name(RID, "alpha", 1);
        let dead_node = node_session_name(RID, "beta", 1);
        let decisions = decide_sweep(
            &[
                input(&live_node, live()),
                input(&dead_node, None),
                input(&manager_session_name(RID), live()),
            ],
            TTLS,
            at("2026-07-31T15:32:19Z"),
        );

        assert_eq!(
            decisions
                .iter()
                .map(|d| d.session_name.as_str())
                .collect::<Vec<_>>(),
            vec![
                live_node.as_str(),
                dead_node.as_str(),
                &manager_session_name(RID)
            ]
        );
        assert_eq!(decisions[0].verdict, SweepVerdict::Keep);
        assert!(matches!(decisions[1].verdict, SweepVerdict::Kill(_)));
        assert_eq!(decisions[2].verdict, SweepVerdict::Keep);
    }
}
