//! Liveness sweep policy for the nodes of a live Run.
//!
//! **Session death is the only verdict of death (#469, ADR-0032).** For an agent
//! node the death of the agent *is* the death of the tmux session by
//! construction: `pdo_session::domain::launch::wrap_with_env` emits the bash
//! script `<exports> && <tail>` the session runs and `build_agent_tail` emits
//! `exec claude …`, so the `claude` process **is** the pane leader of the
//! session's only window. It exits → the pane dies → the session dies →
//! `session_alive == false`. Same in a sandbox: the pane carries the
//! `docker exec` client, which returns as soon as claude exits inside the
//! container. `remain-on-exit` is never armed.
//!
//! That fact is why there is no idle threshold here any more. A transcript-mtime
//! proxy adds nothing to death detection — what it catches *beyond*
//! [`Detection::SessionDied`] is exclusively the agent that is **alive but
//! silent**, i.e. "wedged" or "not progressing", and a `docker build` or a
//! `cargo test --workspace` is indistinguishable from either. Measured on a real
//! 679-record transcript of a healthy node: five silent gaps of 155 s, 185 s,
//! 214 s, 270 s and 291 s. The old 120 s threshold was not mis-calibrated, it was
//! structurally incapable, and one false positive cost a whole Run.
//!
//! What the sweep still does, beyond death:
//! - **usage-limit menu** (#290): observability only, the node stays `Running`;
//! - **turn-end auto-completion** (#469 §2): opt-in, and keyed on a *constated
//!   end of turn* ([`parse_turn_state`]) — never on a duration.

use crate::domain::event;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::time::{Duration, SystemTime};

use crate::domain::event::EventKind;

use crate::application::node::outputs as outputs_validator;
use pdo_pipeline::domain::pipeline;

pub use super::assess::*;
pub use super::diagnostics::*;

/// How often the background sweep wakes up. Surfaced by `GET /stale/health`
/// (#251) and mirrors `trigger_scheduler::TICK_INTERVAL_SECS`.
///
/// Since #469 this is the sweep's *only* time constant on the liveness path:
/// there is no idle age that "counts as stale" for it to be compared against.
pub const STALE_TICK_INTERVAL_SECS: u64 = 30;

/// How many trailing bytes of a node's Claude Code transcript the turn-end probe
/// reads (#469 §2). Never the whole file: a long node's `.jsonl` runs to
/// megabytes and the sweep visits every live node every
/// [`STALE_TICK_INTERVAL_SECS`].
///
/// A window that clips mid-record is a *designed-for* case, not a bug:
/// [`parse_turn_state`] skips every unparseable line and answers
/// [`TurnState::Unknown`] when nothing substantial survives — and `Unknown`
/// behaves as "at work", so a clipped read can only ever be conservative.
pub const TRANSCRIPT_TAIL_BYTES: u64 = 256 * 1024;

/// Anti-bounce window before a constated end of turn may be acted on (#469 §2).
///
/// The transcript mtime survives the removal of the idle threshold in **this one
/// role and no other**: it is not an oracle for "is the agent alive", it only
/// keeps the sweep from racing a write still in flight (a final `assistant`
/// record whose successor is being appended as we read). A clock-skewed *future*
/// mtime counts as not-quiet — conservative by construction.
pub const TURN_END_QUIET_PERIOD: Duration = Duration::from_secs(60);

/// Env seam for the turn-end auto-completion setting (#469 §4, ADR-0015).
/// Middle tier of `stored → env → default(false)`; the resolver is
/// [`autocomplete_turn_end_with`].
pub const AUTOCOMPLETE_TURN_END_ENV: &str = "PDO_AUTOCOMPLETE_TURN_END";

/// Built-in default for turn-end auto-completion: **off**.
///
/// Load-bearing (ADR-0012, autonomy is earned): a terminal action the runtime
/// initiates on its own must be opted into. With it off, the sweep performs
/// exactly one `session_exists` per live node and reads no transcript at all —
/// strictly cheaper than the pre-#469 path, which paid a `read_dir` plus an
/// outputs validation per node per tick.
pub const AUTOCOMPLETE_TURN_END_DEFAULT: bool = false;

/// Parse a stored/env boolean flag. `None` for anything unrecognised, so a typo
/// falls through to the next precedence tier instead of silently meaning `false`.
pub fn parse_bool_setting(raw: &str) -> Option<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

/// The `env` tier of the turn-end auto-completion setting (#469 §4).
pub fn env_autocomplete_turn_end() -> Option<bool> {
    std::env::var(AUTOCOMPLETE_TURN_END_ENV)
        .ok()
        .as_deref()
        .and_then(parse_bool_setting)
}

/// Resolve turn-end auto-completion: `stored → env → default(false)` (#469 §4,
/// ADR-0015).
///
/// `stored` is the raw `instance_config.autocomplete_turn_end` column: `Some(0)`
/// is a stored **off** and wins over the env, exactly like a stored `0` would for
/// any other knob; only SQL `NULL` (`None`) falls through.
pub fn autocomplete_turn_end_with(stored: Option<i64>) -> bool {
    match stored {
        Some(v) => v != 0,
        None => env_autocomplete_turn_end().unwrap_or(AUTOCOMPLETE_TURN_END_DEFAULT),
    }
}

/// What the sweep concluded about one live node.
///
/// There is deliberately **no `Stale` variant** (#469, ADR-0032): a verdict that
/// meant "alive but idle past N seconds" produced false positives that were
/// terminal *and* irrecoverable — the node latched out of the probe set, its
/// later `node_done` was refused by the completion guard, and
/// `reconcile_run_level_stall` failed the whole Run within the same sweep. The
/// `NodeStale` **event** and `NodeStatus::Stale` survive for historical Runs (the
/// log is append-only), but nothing in the daemon emits them any more.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Detection {
    /// The node's tmux session is gone. The **only** verdict of death.
    SessionDied,
    /// The agent is alive and has visibly finished its turn with valid outputs,
    /// and turn-end auto-completion is enabled (#469 §2). Produced by
    /// [`assess_node`], never by [`decide`]: it needs two probes past liveness.
    TurnEnded,
    /// Nothing to do. Includes every "alive but not progressing" shape —
    /// mid-tool-call, wedged on an interactive prompt, API retries exhausted.
    Ok,
}

/// Pure liveness decision: session alive or not (#469 §1).
///
/// This *is* the whole of it now. `session_alive == false` is the single
/// authority on death (see the module docs for why the double `exec` makes the
/// agent's exit and the session's death the same event); everything else the
/// sweep does hangs off [`Detection::Ok`] in [`assess_node`].
pub fn decide(session_alive: bool) -> Detection {
    if session_alive {
        Detection::Ok
    } else {
        Detection::SessionDied
    }
}

/// Whether the transcript has been quiet long enough for a constated end of turn
/// to be acted on (#469 §2 anti-bounce). See [`TURN_END_QUIET_PERIOD`] for why
/// the mtime survives *only* in this role.
pub(super) fn quiet_long_enough(mtime: SystemTime, now: SystemTime) -> bool {
    now.duration_since(mtime).unwrap_or(Duration::ZERO) >= TURN_END_QUIET_PERIOD
}

/// The trailing slice of a node's transcript plus the mtime of that same file
/// (#469 §2).
///
/// One probe returning both, deliberately: the anti-bounce
/// ([`TURN_END_QUIET_PERIOD`]) must be evaluated against the *file we just
/// tailed*. A second `jsonl_mtime()` probe could resolve a different newest
/// `.jsonl` between the two calls and let a stale quiet-check greenlight a fresh
/// tail. It is also what makes "setting off ⇒ zero transcript I/O" provable: one
/// method, never called.
#[derive(Debug, Clone)]
pub struct TranscriptTail {
    /// Last [`TRANSCRIPT_TAIL_BYTES`] bytes, lossily decoded. May begin
    /// mid-record — [`parse_turn_state`] is built for that.
    pub text: String,
    /// mtime of the tailed file. The anti-bounce clock, **not** an activity
    /// oracle: Claude Code writes untimestamped metadata records that bump it
    /// without any agent activity behind them.
    pub mtime: SystemTime,
}

/// Read the last [`TRANSCRIPT_TAIL_BYTES`] of `path` together with its mtime.
///
/// `None` on any I/O failure (absent file, unreadable) — which
/// [`assess_node`] treats as "no signal", i.e. leave the node alone. Seeks rather
/// than reading the file whole, so a multi-megabyte transcript costs one
/// bounded read per live node per sweep.
pub fn read_transcript_tail(path: &Path) -> Option<TranscriptTail> {
    let mut file = std::fs::File::open(path).ok()?;
    let meta = file.metadata().ok()?;
    let mtime = meta.modified().ok()?;
    let len = meta.len();
    let start = len.saturating_sub(TRANSCRIPT_TAIL_BYTES);
    if start > 0 {
        file.seek(SeekFrom::Start(start)).ok()?;
    }
    let mut buf = Vec::with_capacity((len - start) as usize);
    file.take(TRANSCRIPT_TAIL_BYTES)
        .read_to_end(&mut buf)
        .ok()?;
    Some(TranscriptTail {
        // Lossy: the seek can land mid-codepoint, and a replacement char only
        // ever breaks the JSON of the leading partial record, which is skipped.
        text: String::from_utf8_lossy(&buf).into_owned(),
        mtime,
    })
}

/// Validate outputs for a node using the pipeline definition.
///
/// Since #469 this is consulted **only** behind a constated
/// [`TurnState::TurnEnded`] — never behind a duration. It is the second of the
/// two independent guards on auto-completion: it is what stops an agent that
/// ended its turn to *ask a question* from being completed, since its outputs
/// are still incomplete.
pub fn validate_outputs(
    pipeline_path: &Path,
    node_id: &str,
    iter: i64,
    artifacts_dir: &Path,
) -> bool {
    let yaml = match std::fs::read_to_string(pipeline_path) {
        Ok(y) => y,
        Err(_) => return false,
    };
    let pipeline_def = match pipeline::parse_pipeline(&yaml) {
        Ok(p) => p.pipeline,
        Err(_) => return false,
    };

    outputs_validator::validate(&pipeline_def, node_id, iter, artifacts_dir).is_ok()
}

/// Build events for a detection result.
///
/// Only [`Detection::SessionDied`] has an event of its own here.
/// [`Detection::TurnEnded`] deliberately produces **none** (#469 §3): appending a
/// `NodeAutoCompleted` straight into the log was the defect of the old design —
/// on an isolated node it would record a `Completed` whose commit
/// stayed on the `pdo/sub-…` branch, with the downstream receiving nothing. Its
/// terminal event is appended by the *shared node-completion body*, past the
/// forgotten-run refusal, the completion guard and
/// `commit_and_merge_sub_worktree_inner`.
pub fn detection_events(
    detection: &Detection,
    run_id: &str,
    node_id: &str,
    iter: i64,
) -> Vec<event::Event> {
    // The session-died cause names the dead tmux session (#213 AC1) so the
    // incident is self-explanatory in the UI/log. Since résilience (ADR-0049)
    // this is a `NodeInterrupted`, not a `NodeFailed`: "la session est morte,
    // pas le travail" — the run parks `AwaitingUser` (derived in `finalize`),
    // never `Failed`, and a human resumes or restarts it.
    let (kind, reason) = match detection {
        Detection::Ok | Detection::TurnEnded => return vec![],
        Detection::SessionDied => {
            let session = pdo_session::domain::names::node_session_name(run_id, node_id, iter);
            (
                EventKind::NodeInterrupted,
                format!("session_died: tmux session {session} no longer exists"),
            )
        }
    };

    vec![event::Event {
        id: None,
        run_id: run_id.to_string(),
        ts: pdo_kernel::clock::now_iso(),
        kind,
        node_id: Some(node_id.to_string()),
        iter: Some(iter),
        payload: Some(serde_json::json!({ "reason": reason })),
    }]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::event::Event;
    use crate::domain::projection::project;
    use std::time::{Duration, SystemTime};

    // Claude transcript tails, cut from a real node (the harness owns them and
    // their parser, `pdo_harness::claude_transcript`).
    const FIXTURE_TURN_ENDED: &str =
        include_str!("../../../../pdo-harness/tests/fixtures/turn_state/turn_ended.jsonl");
    const FIXTURE_IN_TOOL_CALL: &str =
        include_str!("../../../../pdo-harness/tests/fixtures/turn_state/in_tool_call.jsonl");
    const FIXTURE_AWAITING_ASSISTANT: &str =
        include_str!("../../../../pdo-harness/tests/fixtures/turn_state/awaiting_assistant.jsonl");
    const FIXTURE_METADATA_ONLY: &str =
        include_str!("../../../../pdo-harness/tests/fixtures/turn_state/metadata_only.jsonl");

    #[test]
    fn dead_session_returns_session_died() {
        assert_eq!(decide(false), Detection::SessionDied);
    }

    #[test]
    fn live_session_returns_ok() {
        assert_eq!(decide(true), Detection::Ok);
    }

    #[test]
    fn decide_never_produces_turn_ended() {
        // `TurnEnded` needs two probes past liveness, so it is `assess_node`'s to
        // produce — never the pure liveness verdict's. Guards against someone
        // "simplifying" the two into one and reintroducing a duration.
        for alive in [true, false] {
            assert_ne!(decide(alive), Detection::TurnEnded);
        }
    }

    #[test]
    fn quiet_window_boundaries() {
        let now = SystemTime::now();
        assert!(!quiet_long_enough(now, now), "a write just landed");
        assert!(!quiet_long_enough(
            now - (TURN_END_QUIET_PERIOD - Duration::from_secs(1)),
            now
        ));
        assert!(quiet_long_enough(now - TURN_END_QUIET_PERIOD, now));
        assert!(quiet_long_enough(now - Duration::from_secs(3600), now));
    }

    #[test]
    fn a_future_mtime_is_never_quiet() {
        // Clock skew must not greenlight a completion.
        let now = SystemTime::now();
        assert!(!quiet_long_enough(now + Duration::from_secs(600), now));
    }

    // --- the setting resolver (#469 §4, ADR-0015 stored → env → default) ---

    /// ADR-0012: a terminal action the runtime initiates is earned, not given, so
    /// the built-in default must stay OFF. A compile-time guard — flipping the
    /// constant fails the build rather than a test run.
    const _: () = assert!(!AUTOCOMPLETE_TURN_END_DEFAULT);

    #[test]
    fn stored_wins_over_env_in_both_directions() {
        // A stored 0 is a stored OFF and wins, exactly like a stored 1 wins —
        // that is what makes the checkbox authoritative over the env var.
        assert!(autocomplete_turn_end_with(Some(1)));
        assert!(!autocomplete_turn_end_with(Some(0)));
        // Any non-zero is truthy: the column is written 0/1, but a hand-edited DB
        // must not become a third state.
        assert!(autocomplete_turn_end_with(Some(7)));
    }

    #[test]
    fn bool_setting_parses_the_usual_spellings_and_rejects_junk() {
        for on in ["1", "true", "TRUE", " yes ", "on"] {
            assert_eq!(parse_bool_setting(on), Some(true), "{on:?}");
        }
        for off in ["0", "false", "No", "off"] {
            assert_eq!(parse_bool_setting(off), Some(false), "{off:?}");
        }
        // Junk yields None so it falls through to the next tier instead of
        // silently meaning `false`.
        for junk in ["", "maybe", "2x", "oui"] {
            assert_eq!(parse_bool_setting(junk), None, "{junk:?}");
        }
    }

    // --- read_transcript_tail (filesystem) ---

    #[test]
    fn transcript_tail_reads_the_last_bytes_and_the_mtime() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("session.jsonl");
        // Comfortably larger than the window, so the read must seek.
        let filler = "x".repeat(TRANSCRIPT_TAIL_BYTES as usize);
        std::fs::write(&path, format!("{filler}TAIL-MARKER\n")).unwrap();
        let back_dated = SystemTime::now() - Duration::from_secs(300);
        filetime::set_file_mtime(&path, filetime::FileTime::from_system_time(back_dated)).unwrap();

        let tail = read_transcript_tail(&path).expect("tail must read");
        assert!(
            tail.text.ends_with("TAIL-MARKER\n"),
            "must read the END of the file"
        );
        assert!(
            tail.text.len() as u64 <= TRANSCRIPT_TAIL_BYTES,
            "must never exceed the window: {} bytes",
            tail.text.len()
        );
        assert!(
            tail.mtime <= SystemTime::now() - Duration::from_secs(299),
            "mtime must come from the same stat as the read"
        );
    }

    #[test]
    fn transcript_tail_reads_a_short_file_whole() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("session.jsonl");
        std::fs::write(&path, "{}\n").unwrap();
        assert_eq!(read_transcript_tail(&path).unwrap().text, "{}\n");
    }

    #[test]
    fn transcript_tail_of_a_missing_file_is_none() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(read_transcript_tail(&tmp.path().join("nope.jsonl")).is_none());
    }

    // --- detection_events ---

    #[test]
    fn events_ok_is_empty() {
        assert!(detection_events(&Detection::Ok, "r", "n", 1).is_empty());
    }

    #[test]
    fn events_turn_ended_is_empty() {
        // #469 §3: the terminal `NodeAutoCompleted` belongs to the SHARED
        // node-completion body (which merges the sub-worktree first), never to a
        // bare append from the sweep.
        assert!(detection_events(&Detection::TurnEnded, "r", "n", 1).is_empty());
    }

    #[test]
    fn events_session_died() {
        let events = detection_events(&Detection::SessionDied, "run1", "node1", 1);
        assert_eq!(events.len(), 1);
        // ADR-0049: session death is an infra incident → `NodeInterrupted`, not
        // `NodeFailed`.
        assert_eq!(events[0].kind, EventKind::NodeInterrupted);
        assert_eq!(events[0].node_id.as_deref(), Some("node1"));
        let payload = events[0].payload.as_ref().unwrap();
        // #213 AC1: the failure cause must name the dead tmux session so an
        // operator inspecting the run can tell exactly which session vanished.
        let reason = payload["reason"].as_str().unwrap();
        assert!(
            reason.contains("pdo-run1-node1-iter-1"),
            "session-died cause {reason:?} must name the dead session"
        );
    }

    #[test]
    fn no_detection_can_emit_node_stale() {
        // AC2 (pure half): `NodeStale` has no producer left. The variant itself
        // stays — the log is append-only and historical Runs carry it — but
        // nothing in the daemon writes one.
        for detection in [Detection::Ok, Detection::TurnEnded, Detection::SessionDied] {
            for event in detection_events(&detection, "r", "n", 1) {
                assert_ne!(
                    event.kind,
                    EventKind::NodeStale,
                    "no detection may emit NodeStale any more (#469 §1)"
                );
                assert_ne!(
                    event.kind,
                    EventKind::NodeAutoCompleted,
                    "auto-completion goes through the shared completion body (#469 §3)"
                );
            }
        }
    }

    // --- #234 session-death diagnostics ---

    #[test]
    fn diagnostics_to_json_carries_all_fields() {
        let diag = SessionDeathDiagnostics {
            tmux_server_alive: Some(false),
            mem_available_kb: Some(123),
            swap_free_kb: Some(456),
            correlated_deaths: 2,
            harness_error: Some("model failure after retries".to_string()),
        };
        let json = diag.to_json();
        assert_eq!(json["tmux_server_alive"], serde_json::json!(false));
        assert_eq!(json["mem_available_kb"], serde_json::json!(123));
        assert_eq!(json["swap_free_kb"], serde_json::json!(456));
        assert_eq!(json["correlated_deaths"], serde_json::json!(2));
        assert_eq!(
            json["harness_error"],
            serde_json::json!("model failure after retries")
        );
    }

    #[test]
    fn diagnostics_to_json_none_probes_serialize_to_null() {
        // A failed probe must be distinguishable from a real value, so `None`
        // fields serialise to JSON `null` rather than being dropped.
        let json = SessionDeathDiagnostics::default().to_json();
        assert!(json["tmux_server_alive"].is_null());
        assert!(json["mem_available_kb"].is_null());
        assert!(json["swap_free_kb"].is_null());
        assert_eq!(json["correlated_deaths"], serde_json::json!(0));
    }

    #[test]
    fn attach_diagnostics_enriches_session_died_payload_alongside_reason() {
        let mut events = detection_events(&Detection::SessionDied, "run1", "node1", 1);
        let diag = SessionDeathDiagnostics {
            tmux_server_alive: Some(false),
            mem_available_kb: Some(2048),
            swap_free_kb: Some(0),
            correlated_deaths: 1,
            harness_error: None,
        };
        attach_diagnostics(&mut events, &diag);

        let payload = events[0].payload.as_ref().unwrap();
        // The original symptom is preserved …
        assert!(payload["reason"].as_str().unwrap().contains("session_died"));
        // … and the diagnostics sit alongside it.
        assert_eq!(
            payload["diagnostics"]["tmux_server_alive"],
            serde_json::json!(false)
        );
        assert_eq!(
            payload["diagnostics"]["correlated_deaths"],
            serde_json::json!(1)
        );
        assert_eq!(
            payload["diagnostics"]["mem_available_kb"],
            serde_json::json!(2048)
        );
    }

    #[test]
    fn attach_diagnostics_is_noop_on_empty_events() {
        // Detection::Ok yields no events — attaching must not panic.
        let mut events = detection_events(&Detection::Ok, "run1", "node1", 1);
        attach_diagnostics(&mut events, &SessionDeathDiagnostics::default());
        assert!(events.is_empty());
    }

    #[test]
    fn parse_meminfo_extracts_available_and_swap() {
        let contents = "\
MemTotal:       16384000 kB
MemFree:          512000 kB
MemAvailable:    8192000 kB
SwapTotal:       2048000 kB
SwapFree:         204800 kB
";
        let (mem, swap) = parse_meminfo(contents);
        assert_eq!(mem, Some(8192000));
        assert_eq!(swap, Some(204800));
    }

    #[test]
    fn parse_meminfo_missing_fields_return_none() {
        // No MemAvailable / SwapFree lines (e.g. an ancient kernel) → None,
        // not a wrong value picked up from a similarly-named line.
        let contents = "MemTotal:  16384000 kB\nMemFree:  512000 kB\nSwapTotal:  2048000 kB\n";
        assert_eq!(parse_meminfo(contents), (None, None));
    }

    #[test]
    fn parse_meminfo_ignores_malformed_values() {
        let contents = "MemAvailable:  notanumber kB\nSwapFree:\n";
        assert_eq!(parse_meminfo(contents), (None, None));
    }

    #[test]
    fn count_correlated_deaths_excludes_self_and_counts_dead_peers() {
        let running = vec![
            ("a".to_string(), 1),
            ("b".to_string(), 1),
            ("c".to_string(), 2),
        ];
        // Self ("a", 1) is excluded even though the predicate would call it
        // dead; "b" is dead, "c" is alive → exactly one correlated death.
        let dead = |id: &str, _it: i64| id != "c";
        assert_eq!(count_correlated_deaths(&running, ("a", 1), dead), 1);
    }

    #[test]
    fn count_correlated_deaths_zero_when_peers_alive() {
        let running = vec![("a".to_string(), 1), ("b".to_string(), 1)];
        assert_eq!(count_correlated_deaths(&running, ("a", 1), |_, _| false), 0);
    }

    #[test]
    fn count_correlated_deaths_distinguishes_iter() {
        // Same node id, different iter, must be treated as a distinct peer and
        // counted — not collapsed onto self.
        let running = vec![("a".to_string(), 1), ("a".to_string(), 2)];
        assert_eq!(count_correlated_deaths(&running, ("a", 1), |_, _| true), 1);
    }

    // --- running_nodes ---

    #[test]
    fn running_nodes_filters_correctly() {
        use crate::domain::event::Event;
        use crate::domain::event::EventKind;
        use crate::domain::projection::project;

        fn make_event(kind: EventKind, node_id: Option<&str>, iter: Option<i64>) -> Event {
            let payload = if kind == EventKind::RunStarted {
                Some(serde_json::json!({ "pipeline_name": "test" }))
            } else {
                None
            };
            Event {
                id: None,
                run_id: "test-run".to_string(),
                ts: pdo_kernel::clock::now_iso(),
                kind,
                node_id: node_id.map(String::from),
                iter,
                payload,
            }
        }

        let events = vec![
            make_event(EventKind::RunStarted, None, None),
            make_event(EventKind::NodeStarted, Some("worker"), Some(1)),
            make_event(EventKind::NodeStarted, Some("planner"), Some(1)),
            make_event(EventKind::NodeCompleted, Some("planner"), Some(1)),
        ];

        let state = project(&events).unwrap();
        let running = running_nodes(&state);
        assert_eq!(running.len(), 1);
        assert_eq!(running[0].0, "worker");
        assert_eq!(running[0].1, 1);
    }
    #[test]
    fn a_node_waiting_for_its_user_in_its_session_is_probed_for_death_only() {
        // `pdo wait-user` and a refused unreleased completion keep the agent in
        // its session: its death must not go unseen. An orchestrator parked on
        // its children has no session to probe.
        let mut events = vec![Event {
            id: None,
            run_id: "r".into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({ "pipeline_name": "test" })),
        }];
        let node_event = |kind: EventKind, node: &str, payload: Option<serde_json::Value>| Event {
            id: None,
            run_id: "r".into(),
            ts: pdo_kernel::clock::now_iso(),
            kind,
            node_id: Some(node.into()),
            iter: Some(1),
            payload,
        };
        for node in ["worker", "asker", "parent"] {
            events.push(node_event(EventKind::NodeStarted, node, None));
        }
        events.push(node_event(
            EventKind::NodeAwaitingUser,
            "asker",
            Some(serde_json::json!({ "cause": "declared", "message": "Colour?" })),
        ));
        events.push(node_event(
            EventKind::NodeAwaitingUser,
            "parent",
            Some(serde_json::json!({ "reason": "children_pending" })),
        ));
        let state = project(&events).unwrap();

        let mut probed = session_holding_nodes(&state);
        probed.sort();
        assert_eq!(
            probed,
            vec![
                ("asker".to_string(), 1, true),
                ("worker".to_string(), 1, false),
            ]
        );
    }

    // --- validate_outputs (integration with outputs_validator) ---

    #[test]
    fn validate_outputs_with_no_declared_outputs() {
        let tmp = tempfile::tempdir().unwrap();
        let pipeline_path = tmp.path().join("pipeline.yaml");
        std::fs::write(
            &pipeline_path,
            "name: test\nnodes:\n  - id: start\n    name: Start\n    type: start\n    inputs: []\n    outputs:\n      - name: user_prompt\n  - id: worker\n    name: Worker\n    type: agent\n    isolated_worktree: false\n    inputs:\n      - name: task\n    outputs: []\n  - id: end\n    name: End\n    type: end\n    inputs:\n      - name: result\n    outputs: []\nedges:\n  - source: { node: start, port: user_prompt }\n    target: { node: worker, port: task }\n",
        )
        .unwrap();

        let artifacts_dir = tmp.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();

        assert!(validate_outputs(
            &pipeline_path,
            "worker",
            1,
            &artifacts_dir
        ));
    }

    #[test]
    fn validate_outputs_with_missing_artifact() {
        let tmp = tempfile::tempdir().unwrap();
        let pipeline_path = tmp.path().join("pipeline.yaml");
        std::fs::write(
            &pipeline_path,
            "name: test\nnodes:\n  - id: start\n    name: Start\n    type: start\n    inputs: []\n    outputs:\n      - name: user_prompt\n  - id: worker\n    name: Worker\n    type: agent\n    isolated_worktree: false\n    inputs:\n      - name: task\n    outputs:\n      - name: report\n  - id: end\n    name: End\n    type: end\n    inputs:\n      - name: result\n    outputs: []\nedges:\n  - source: { node: start, port: user_prompt }\n    target: { node: worker, port: task }\n",
        )
        .unwrap();

        let artifacts_dir = tmp.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();

        assert!(!validate_outputs(
            &pipeline_path,
            "worker",
            1,
            &artifacts_dir
        ));
    }

    // --- assess_node (#469: the whole sweep policy, with fake I/O) ---

    /// A fully controllable [`NodeProbes`] fake. The two counters are the point:
    /// they prove the *short-circuits*, i.e. that a healthy node with the setting
    /// off pays for no transcript read and no outputs validation at all.
    struct FakeProbes {
        session_alive: bool,
        tail: Option<TranscriptTail>,
        outputs_valid: bool,
        pane: Option<String>,
        diagnostics: SessionDeathDiagnostics,
        tail_calls: std::cell::Cell<usize>,
        validate_calls: std::cell::Cell<usize>,
    }

    impl FakeProbes {
        fn alive() -> Self {
            Self {
                session_alive: true,
                tail: None,
                outputs_valid: false,
                pane: None,
                diagnostics: SessionDeathDiagnostics::default(),
                tail_calls: std::cell::Cell::new(0),
                validate_calls: std::cell::Cell::new(0),
            }
        }

        /// Alive, transcript = `text` last written `quiet` ago, outputs `valid`.
        fn with_tail(text: &str, quiet: Duration, valid: bool) -> Self {
            Self {
                tail: Some(TranscriptTail {
                    text: text.to_string(),
                    mtime: SystemTime::now() - quiet,
                }),
                outputs_valid: valid,
                ..Self::alive()
            }
        }

        /// The shape this issue exists for: an agent that finished its turn,
        /// wrote valid outputs, and never called `pdo complete`.
        fn finished_turn() -> Self {
            Self::with_tail(
                FIXTURE_TURN_ENDED,
                TURN_END_QUIET_PERIOD + Duration::from_secs(5),
                true,
            )
        }
    }

    impl NodeProbes for FakeProbes {
        fn session_alive(&self) -> bool {
            self.session_alive
        }
        fn transcript_tail(&self) -> Option<TranscriptTail> {
            self.tail_calls.set(self.tail_calls.get() + 1);
            self.tail.clone()
        }
        fn outputs_valid(&self) -> bool {
            self.validate_calls.set(self.validate_calls.get() + 1);
            self.outputs_valid
        }
        fn capture_pane(&self) -> Option<String> {
            self.pane.clone()
        }
        fn session_death_diagnostics(&self) -> SessionDeathDiagnostics {
            self.diagnostics.clone()
        }
    }

    fn assess(probes: &FakeProbes, autocomplete: bool) -> Assessment {
        // The existing suite is about the `claude` sweep, which has both
        // capabilities and whose JSONL parser the `FakeProbes` fixtures feed;
        // #613's capability gating is exercised by the dedicated tests below on a
        // data-declared harness (`opencode`, which has neither capability).
        assess_node(
            probes,
            &[],
            "run1",
            "worker",
            1,
            SystemTime::now(),
            autocomplete,
            pdo_harness::registry::CLAUDE,
        )
    }

    // --- session death: the only verdict of death ---

    #[test]
    fn assess_dead_session_fails_with_diagnostics() {
        let probes = FakeProbes {
            session_alive: false,
            outputs_valid: true, // must be ignored: a dead session wins
            diagnostics: SessionDeathDiagnostics {
                tmux_server_alive: Some(false),
                correlated_deaths: 2,
                ..Default::default()
            },
            ..FakeProbes::finished_turn()
        };
        let a = assess(&probes, true);
        assert_eq!(a.detection, Detection::SessionDied);
        assert_eq!(a.events.len(), 1);
        // ADR-0049: session death → `NodeInterrupted`, not `NodeFailed`.
        assert_eq!(a.events[0].kind, EventKind::NodeInterrupted);
        // #234: diagnostics folded into the failure payload AND surfaced for the
        // sweep's structured log.
        assert_eq!(
            a.events[0].payload.as_ref().unwrap()["diagnostics"]["correlated_deaths"],
            serde_json::json!(2)
        );
        assert_eq!(a.session_death_diagnostics.unwrap().correlated_deaths, 2);
        assert!(!a.blocked_on_limit);
    }

    #[test]
    fn assess_dead_session_probes_no_transcript() {
        // Death short-circuits everything: no tail read, no outputs validation,
        // no pane capture. `claude`'s exit IS its verdict, so no journal is read.
        let probes = FakeProbes {
            session_alive: false,
            ..FakeProbes::finished_turn()
        };
        assert_eq!(assess(&probes, true).detection, Detection::SessionDied);
        assert_eq!(probes.tail_calls.get(), 0);
        assert_eq!(probes.validate_calls.get(), 0);
    }

    // --- #615: copilot's journal is the verdict, not its exit code (ADR-0052) ---

    const COPILOT_HARD_ERROR: &str = concat!(
        r#"{"type":"assistant.turn_start","data":{"turnId":"0"}}"#,
        "\n",
        r#"{"type":"session.error","data":{"errorType":"query","message":"Failed to get response from the AI model; retried 5 times"}}"#,
        "\n"
    );
    const COPILOT_TURN_ENDED: &str = concat!(
        r#"{"type":"assistant.turn_start","data":{"turnId":"0"}}"#,
        "\n",
        r#"{"type":"assistant.turn_end","data":{"turnId":"0"}}"#,
        "\n"
    );

    #[test]
    fn assess_dead_copilot_session_names_the_journal_error() {
        // AC (#615): a hard error the harness EXITED 0 on is recognised from the
        // journal, not the exit code. The session died; PDO reads the tail copilot
        // left and names the failure in the diagnostics.
        let probes = FakeProbes {
            session_alive: false,
            ..FakeProbes::with_tail(COPILOT_HARD_ERROR, Duration::from_secs(1), false)
        };
        let a = assess_harness(&probes, true, pdo_harness::registry::COPILOT);
        assert_eq!(a.detection, Detection::SessionDied);
        let err = a
            .session_death_diagnostics
            .as_ref()
            .unwrap()
            .harness_error
            .as_deref()
            .expect("the journal error is named");
        assert!(err.contains("Failed to get response from the AI model"));
        // And it rides in the NodeInterrupted payload alongside the symptom.
        assert_eq!(
            a.events[0].payload.as_ref().unwrap()["diagnostics"]["harness_error"],
            serde_json::json!(err)
        );
    }

    #[test]
    fn assess_copilot_errored_turn_is_not_auto_completed() {
        // A copilot node whose journal trails on a hard error must NOT be auto-
        // completed as a finished turn — even with the setting on and outputs valid.
        let probes = FakeProbes::with_tail(
            COPILOT_HARD_ERROR,
            TURN_END_QUIET_PERIOD + Duration::from_secs(5),
            true,
        );
        let a = assess_harness(&probes, true, pdo_harness::registry::COPILOT);
        assert_ne!(
            a.detection,
            Detection::TurnEnded,
            "an errored turn is not ended"
        );
    }

    #[test]
    fn assess_copilot_finished_turn_is_auto_completed() {
        // The positive control: a real copilot turn-end (its own event shape) with
        // valid outputs auto-completes — dispatched to copilot's journal parser.
        let probes = FakeProbes::with_tail(
            COPILOT_TURN_ENDED,
            TURN_END_QUIET_PERIOD + Duration::from_secs(5),
            true,
        );
        let a = assess_harness(&probes, true, pdo_harness::registry::COPILOT);
        assert_eq!(a.detection, Detection::TurnEnded);
    }

    // --- setting OFF: the default path is one liveness probe (#469 §4, AC8) ---

    #[test]
    fn assess_with_setting_off_never_reads_the_transcript() {
        // AC8, at the I/O seam: the node HAS finished its turn with valid
        // outputs, and with the box unchecked the sweep does not even look.
        let probes = FakeProbes::finished_turn();
        let a = assess(&probes, false);
        assert_eq!(a.detection, Detection::Ok);
        assert!(a.events.is_empty());
        assert_eq!(
            probes.tail_calls.get(),
            0,
            "transcript_tail must NOT be called when the setting is off"
        );
        assert_eq!(
            probes.validate_calls.get(),
            0,
            "outputs must NOT be validated when the setting is off"
        );
    }

    #[test]
    fn assess_healthy_node_is_silent() {
        let probes = FakeProbes {
            pane: Some("● Running: cargo test".to_string()),
            ..FakeProbes::alive()
        };
        let a = assess(&probes, true);
        assert_eq!(a.detection, Detection::Ok);
        assert!(!a.blocked_on_limit);
        assert!(a.events.is_empty());
    }

    // --- setting ON: the two independent guards (#469 §2) ---

    #[test]
    fn assess_finished_turn_with_valid_outputs_is_turn_ended() {
        let probes = FakeProbes::finished_turn();
        let a = assess(&probes, true);
        assert_eq!(a.detection, Detection::TurnEnded);
        // The sweep, not `assess_node`, owns the terminal event (#469 §3).
        assert!(
            a.events.is_empty(),
            "TurnEnded must emit no event of its own"
        );
        assert_eq!(probes.validate_calls.get(), 1);
    }

    #[test]
    fn assess_mid_tool_call_is_never_completed_even_with_valid_outputs() {
        // The core regression guard, and the reason a duration cannot be the
        // signal: this transcript has been silent through a 214 s `docker build`
        // and its outputs already validate. Completing it would put a second
        // writer on the node's worktree.
        let probes = FakeProbes::with_tail(FIXTURE_IN_TOOL_CALL, Duration::from_secs(3600), true);
        assert_eq!(assess(&probes, true).detection, Detection::Ok);
        assert_eq!(
            probes.validate_calls.get(),
            0,
            "a pending tool call must short-circuit before the outputs guard"
        );
    }

    #[test]
    fn assess_awaiting_assistant_is_never_completed() {
        // #251: API retries exhausted mid-turn. Silent, alive, outputs valid —
        // and still not finished.
        let probes =
            FakeProbes::with_tail(FIXTURE_AWAITING_ASSISTANT, Duration::from_secs(3600), true);
        assert_eq!(assess(&probes, true).detection, Detection::Ok);
    }

    #[test]
    fn assess_unknown_turn_state_is_never_completed() {
        // Signal absent ⇒ touch nothing. Fail-safe by construction.
        let probes = FakeProbes::with_tail(FIXTURE_METADATA_ONLY, Duration::from_secs(3600), true);
        assert_eq!(assess(&probes, true).detection, Detection::Ok);
    }

    #[test]
    fn assess_finished_turn_with_incomplete_outputs_is_not_completed() {
        // The second guard on its own: an agent that ends its turn to ask a
        // question has an ended turn and unfinished work.
        let probes = FakeProbes::with_tail(
            FIXTURE_TURN_ENDED,
            TURN_END_QUIET_PERIOD + Duration::from_secs(5),
            false,
        );
        assert_eq!(assess(&probes, true).detection, Detection::Ok);
        assert_eq!(probes.validate_calls.get(), 1, "the outputs guard did run");
    }

    #[test]
    fn assess_respects_the_anti_bounce_window() {
        // A turn that ended one second ago is not acted on: the successor record
        // may be landing as we read.
        let probes = FakeProbes::with_tail(FIXTURE_TURN_ENDED, Duration::from_secs(1), true);
        assert_eq!(assess(&probes, true).detection, Detection::Ok);
        assert_eq!(
            probes.validate_calls.get(),
            0,
            "the anti-bounce must short-circuit before the outputs guard"
        );
    }

    #[test]
    fn assess_no_transcript_is_ok() {
        // A `script` node (ADR-0017) has no `claude`, hence no transcript at all
        // — it self-signals and must never be touched here.
        let probes = FakeProbes {
            tail: None,
            outputs_valid: true,
            ..FakeProbes::alive()
        };
        assert_eq!(assess(&probes, true).detection, Detection::Ok);
        assert_eq!(probes.tail_calls.get(), 1);
        assert_eq!(probes.validate_calls.get(), 0);
    }

    // --- usage-limit menu (#290), unchanged by #469 ---

    #[test]
    fn assess_usage_limit_menu_flags_blocked_and_emits_once() {
        let probes = FakeProbes {
            pane: Some("❯ 1. Stop and wait for limit to reset".to_string()),
            ..FakeProbes::alive()
        };
        let a = assess(&probes, true);
        assert_eq!(a.detection, Detection::Ok);
        assert!(
            a.blocked_on_limit,
            "usage-limit menu must set the gauge flag"
        );
        assert_eq!(a.events.len(), 1);
        assert_eq!(a.events[0].kind, EventKind::NodeBlockedOnLimit);
    }

    #[test]
    fn assess_usage_limit_gauge_set_but_event_deduped() {
        // On a subsequent sweep the menu is still up: the gauge still counts it,
        // but the event is not re-emitted (rising-edge dedup).
        let probes = FakeProbes {
            pane: Some("Stop and wait for limit to reset".to_string()),
            ..FakeProbes::alive()
        };
        let prior = vec![event::Event {
            id: None,
            run_id: "run1".to_string(),
            ts: pdo_kernel::clock::now_iso(),
            kind: EventKind::NodeBlockedOnLimit,
            node_id: Some("worker".to_string()),
            iter: Some(1),
            payload: None,
        }];
        let a = assess_node(
            &probes,
            &prior,
            "run1",
            "worker",
            1,
            SystemTime::now(),
            true,
            pdo_harness::registry::CLAUDE,
        );
        assert!(
            a.blocked_on_limit,
            "gauge counts every sweep the menu is up"
        );
        assert!(
            a.events.is_empty(),
            "the blocked event is emitted only once"
        );
    }

    #[test]
    fn a_blocked_node_that_finished_its_turn_still_reports_the_menu() {
        // Turn-end and the #290 marker are orthogonal: flipping the detection to
        // `TurnEnded` must not swallow the informational event or the gauge.
        let probes = FakeProbes {
            pane: Some("Stop and wait for limit to reset".to_string()),
            ..FakeProbes::finished_turn()
        };
        let a = assess(&probes, true);
        assert_eq!(a.detection, Detection::TurnEnded);
        assert!(a.blocked_on_limit);
        assert_eq!(a.events.len(), 1);
        assert_eq!(a.events[0].kind, EventKind::NodeBlockedOnLimit);
    }

    // --- #553/#613: capability gating — a data-declared harness runs no probe ---

    fn assess_harness(probes: &FakeProbes, autocomplete: bool, harness: &str) -> Assessment {
        assess_node(
            probes,
            &[],
            "run1",
            "worker",
            1,
            SystemTime::now(),
            autocomplete,
            harness,
        )
    }

    /// A data-declared harness the sweep carries no code for: neither capability,
    /// no transcript resolution, no pane anchor. `opencode` is the embedded example.
    const DATA_DECLARED: &str = pdo_harness::registry::OPENCODE;

    #[test]
    fn a_harness_without_the_turn_end_capability_is_never_auto_completed() {
        // The node HAS finished its turn with valid outputs and the setting is ON —
        // but its harness has no turn-end substrate, so the sweep must not complete
        // it, and must not even read a transcript (the substrate is not claude's).
        let probes = FakeProbes::finished_turn();
        let a = assess_harness(&probes, true, DATA_DECLARED);
        assert_eq!(
            a.detection,
            Detection::Ok,
            "no auto-completion without the capability"
        );
        assert_eq!(
            probes.tail_calls.get(),
            0,
            "no transcript read for an un-instrumented harness"
        );
        assert_eq!(probes.validate_calls.get(), 0);
    }

    #[test]
    fn a_harness_with_the_turn_end_capability_still_auto_completes() {
        // The control: with the capability present (and the setting on) the same
        // finished turn IS completed — the gate is the capability, nothing else.
        let probes = FakeProbes::finished_turn();
        let a = assess_harness(&probes, true, pdo_harness::registry::CLAUDE);
        assert_eq!(a.detection, Detection::TurnEnded);
    }

    #[test]
    fn a_harness_without_the_usage_limit_capability_is_never_flagged_blocked() {
        // The pane shows what WOULD be a usage-limit menu, but this harness has no
        // such anchor — so the probe short-circuits and the node is not flagged.
        let probes = FakeProbes {
            pane: Some("❯ 1. Stop and wait for limit to reset".to_string()),
            ..FakeProbes::alive()
        };
        let a = assess_harness(&probes, true, DATA_DECLARED);
        assert_eq!(a.detection, Detection::Ok);
        assert!(
            !a.blocked_on_limit,
            "no menu probe runs without the usage-limit capability"
        );
        assert!(a.events.is_empty());
    }
}
