//! One live node, assessed: what its probes say (session, pane, transcript,
//! outputs) and which events the sweep records for it.

use std::time::{Duration, SystemTime};

use super::detector::*;
use crate::domain::event::{self, EventKind};

/// Injected I/O for [`assess_node`]. The impure sweep layer ([`crate::lib`])
/// implements this against tmux + the filesystem for one running node; unit
/// tests supply a fake so the whole probe → gate → decide → events →
/// diagnostics → dedup pipeline runs without a daemon.
///
/// Every method is a side-effect-free *read*: the reap/spawn side effects stay
/// in the sweep, keyed off [`Assessment::detection`].
pub trait NodeProbes {
    /// Is the node's tmux session still alive? The one probe on the default path
    /// — see [`decide`].
    fn session_alive(&self) -> bool;

    /// Trailing slice + mtime of *this node's* Claude Code transcript, or `None`
    /// when nothing resolves (#469 §2).
    ///
    /// #473: the implementation resolves by the session id PDO pinned at spawn
    /// (`pdo_harness::adapters::transcripts::session_jsonl_by_id`) — the transcript named `<uuid>.jsonl`, this node's
    /// own — and only falls back to the newest-mtime pick (`find_session_jsonl`)
    /// for a pre-#473 node with no recorded id. Without that, a
    /// non-isolated node shares its cwd (the Run worktree) with the
    /// manager's `claude`, so the newest `.jsonl` was usually the manager's and the
    /// turn-end verdict was read off the wrong conversation.
    ///
    /// Called **only** when turn-end auto-completion is enabled. With the setting
    /// off this is never invoked, which is the whole of "unchecked ⇒ no transcript
    /// read" and is asserted directly through this seam.
    fn transcript_tail(&self) -> Option<TranscriptTail>;

    /// Do the node's declared outputs validate against the pipeline? The second
    /// of the two independent auto-completion guards, consulted **only** behind a
    /// constated [`TurnState::TurnEnded`] — so a healthy node never pays for it.
    fn outputs_valid(&self) -> bool;

    /// Best-effort capture of the node's tmux pane, for the usage-limit menu
    /// probe (#290). Only called on the `Ok` path (an alive node).
    fn capture_pane(&self) -> Option<String>;

    /// Best-effort session-death forensics (#234). Gathered lazily — only when
    /// the session is found dead — so no tmux/proc I/O runs on a healthy node.
    fn session_death_diagnostics(&self) -> SessionDeathDiagnostics;

    /// When tmux closed the node's session, if it wrote it down. Read only on
    /// the `SessionDied` path: the sweep sees a death up to a tick late, and the
    /// early-death hint ([`EARLY_DEATH`]) measures the session's real lifetime.
    fn session_closed_at(&self) -> Option<SystemTime>;
}

/// Outcome of [`assess_node`]: the raw detection plus the events to append and
/// the observability the sweep needs. The impure caller appends
/// [`Self::events`] and runs any reap/spawn side effects keyed off
/// [`Self::detection`].
///
/// Not `PartialEq`/`Eq`: [`event::Event`]'s payload is a
/// `serde_json::Value` (no `Eq`), and callers/tests inspect the fields
/// individually rather than compare a whole `Assessment`.
#[derive(Debug, Clone)]
pub struct Assessment {
    /// The verdict the sweep drives its side effects off: `SessionDied` reaps,
    /// `TurnEnded` runs the shared node-completion body (#469 §3), `Ok` does
    /// nothing.
    pub detection: Detection,
    /// Events to append. A `SessionDied` failure already carries its
    /// diagnostics; a usage-limit menu carries a (deduped) `NodeBlockedOnLimit`.
    /// Empty for a nominal `Ok` node — and for `TurnEnded`, whose terminal event
    /// belongs to the shared completion body, not to the sweep.
    pub events: Vec<event::Event>,
    /// True when the node is alive but its pane shows Claude Code's usage-limit
    /// menu — feeds the per-sweep `blocked_on_limit` gauge (#290). Set on every
    /// sweep the menu is visible, independent of event dedup.
    pub blocked_on_limit: bool,
    /// The session-death forensics gathered on the `SessionDied` path (`None`
    /// otherwise), surfaced so the sweep can log the structured fields (#234)
    /// without re-running the probe or re-parsing the event payload.
    pub session_death_diagnostics: Option<SessionDeathDiagnostics>,
}

/// True when an event of `kind` for `(node_id, iter)` already exists in
/// `prior_events` — the rising-edge de-dup key for the informational
/// `NodeBlockedOnLimit` marker. Pure over the event log snapshot the sweep
/// already loaded, so a held condition emits one event, not one per ~30 s sweep
/// tick, and the dedup survives a daemon restart.
fn episode_has_event(
    prior_events: &[event::Event],
    kind: &EventKind,
    node_id: &str,
    iter: i64,
) -> bool {
    prior_events
        .iter()
        .any(|e| &e.kind == kind && e.node_id.as_deref() == Some(node_id) && e.iter == Some(iter))
}

/// A session that dies this soon after its start never got to work: almost
/// always the harness refused to start — not logged in, a model it does not
/// know (FP of CORE-227: an `opencode` with no login exits in four seconds).
const EARLY_DEATH: Duration = Duration::from_secs(30);

/// The grain of [`NodeProbes::session_closed_at`]: tmux writes it down in
/// whole seconds.
const CLOSED_AT_GRAIN: Duration = Duration::from_secs(1);

/// How long a dead session lived.
#[derive(Debug, Clone, Copy)]
enum Lifetime {
    /// From its `NodeStarted` to the moment tmux closed it.
    Measured(Duration),
    /// No closing time: at most this, from its `NodeStarted` to the sweep.
    AtMost(Duration),
}

impl Lifetime {
    /// The lifetime that decides the hint: a bound counts as its own length,
    /// so a death the sweep caught past [`EARLY_DEATH`] with no closing time
    /// gets no hint rather than a wrong one.
    fn length(self) -> Duration {
        match self {
            Self::Measured(d) | Self::AtMost(d) => d,
        }
    }

    /// Always a bound, in whole seconds rounded up, so it is never « 0 s » for
    /// a session that lived. A closing time is rounded down to its second
    /// ([`CLOSED_AT_GRAIN`]): the session closed before the next one.
    fn told(self) -> String {
        let bound = match self {
            Self::Measured(d) => d + CLOSED_AT_GRAIN,
            Self::AtMost(d) => d,
        };
        let secs = bound.as_secs() + u64::from(bound.subsec_nanos() > 0);
        format!("it ended within {secs} s of its start")
    }
}

/// How long the session of `node_id` iter `iter` lived, from its `NodeStarted`
/// in `prior_events` to `closed_at` (when tmux closed it), else to `now`;
/// `None` without a readable start.
fn lifetime(
    prior_events: &[event::Event],
    node_id: &str,
    iter: i64,
    closed_at: Option<SystemTime>,
    now: SystemTime,
) -> Option<Lifetime> {
    let started = prior_events.iter().rev().find(|e| {
        e.kind == EventKind::NodeStarted
            && e.node_id.as_deref() == Some(node_id)
            && e.iter == Some(iter)
    })?;
    let started: SystemTime = chrono::DateTime::parse_from_rfc3339(&started.ts)
        .ok()?
        .with_timezone(&chrono::Utc)
        .into();
    // The closing time is in whole seconds, so a death within the second of
    // the start may read up to one second before it. Earlier still is another
    // iteration's session of the same name: it says nothing of this one.
    match closed_at.filter(|closed| *closed + CLOSED_AT_GRAIN >= started) {
        Some(closed) => Some(Lifetime::Measured(
            closed.duration_since(started).unwrap_or_default(),
        )),
        None => now.duration_since(started).ok().map(Lifetime::AtMost),
    }
}

/// The reason of a session-death event, told when the session died within
/// [`EARLY_DEATH`] of its start: the harness never started, and what to check.
fn tell_early_death(events: &mut [event::Event], lived: Lifetime) {
    if lived.length() >= EARLY_DEATH {
        return;
    }
    for event in events.iter_mut() {
        let Some(obj) = event.payload.as_mut().and_then(|p| p.as_object_mut()) else {
            continue;
        };
        if let Some(reason) = obj.get("reason").and_then(|r| r.as_str()) {
            let told = format!(
                "{reason} · {}: check that its harness is logged in and knows the \
                 node's model",
                lived.told()
            );
            obj.insert("reason".to_string(), serde_json::Value::String(told));
        }
    }
}

fn informational_event(
    kind: EventKind,
    run_id: &str,
    node_id: &str,
    iter: i64,
    payload: serde_json::Value,
) -> event::Event {
    event::Event {
        id: None,
        run_id: run_id.to_string(),
        ts: pdo_kernel::clock::now_iso(),
        kind,
        node_id: Some(node_id.to_string()),
        iter: Some(iter),
        payload: Some(payload),
    }
}

/// The liveness-sweep policy for a single running node, with all I/O injected
/// via `probes`. This is the one place the whole pipeline lives, so
/// [`crate::lib`]'s sweep is reduced to a loop that builds a [`NodeProbes`]
/// adapter, calls this, appends [`Assessment::events`], and runs the
/// reap / complete side effects keyed off [`Assessment::detection`].
///
/// `autocomplete_turn_end` is the resolved instance setting
/// ([`autocomplete_turn_end_with`]), read once per sweep by the caller. When it
/// is `false` this function **short-circuits at the head of the `Ok` path**:
/// neither [`NodeProbes::transcript_tail`] nor [`NodeProbes::outputs_valid`] is
/// invoked, so the default path costs one `session_exists` plus the #290 pane
/// capture and nothing else.
///
/// The two guards on `TurnEnded` are independent and both mandatory:
/// [`TurnState::TurnEnded`] (the agent visibly finished) **and** valid outputs
/// (it finished *the work*, not just its turn — an agent that stopped to ask a
/// question fails the second). The anti-bounce
/// ([`TURN_END_QUIET_PERIOD`]) sits in front of both so a write in flight is
/// never raced.
///
/// Note on #290: a node wedged on the usage-limit menu cannot be auto-completed
/// here even though it is silent — the limit is hit while *requesting* the next
/// assistant message, so its last substantial record is a `user`/`tool_result`
/// and [`parse_turn_state`] answers [`TurnState::AwaitingAssistant`]. "Blocked"
/// is never "finished", by construction rather than by a third guard.
///
/// `prior_events` is the run's event-log snapshot, used for the rising-edge
/// de-dup of `NodeBlockedOnLimit` (see [`episode_has_event`]) and, on a dead
/// session, for how long it lived (see [`EARLY_DEATH`]).
// The probe set + policy toggles are all distinct facts a sweep tick needs; the
// same posture as the four session-launch builders (`pdo_session`) that allow this lint.
#[allow(clippy::too_many_arguments)]
pub fn assess_node(
    probes: &impl NodeProbes,
    prior_events: &[event::Event],
    run_id: &str,
    node_id: &str,
    iter: i64,
    now: SystemTime,
    autocomplete_turn_end: bool,
    harness: &str,
) -> Assessment {
    // #613/ADR-0051: the two probes are dispatch points, not presence guards. The
    // gate reads the resolved harness's declared capabilities, and the detection
    // itself is dispatched to that harness's implementation — `claude`'s JSONL
    // parser / pane anchor for `claude`, a data-declared harness's own (or nothing).
    let caps = pdo_harness::probes::capabilities(harness);
    let detection = decide(probes.session_alive());

    if detection == Detection::SessionDied {
        let mut events = detection_events(&detection, run_id, node_id, iter);
        let mut diag = probes.session_death_diagnostics();
        // #615/ADR-0052: a harness that exits 0 on a hard failure (`copilot`) leaves
        // the verdict in its journal, not its exit code. Only such a harness pays a
        // tail read on death (gated on `exit_code_is_verdict` being false) — so
        // `claude`, whose death is its own signal, keeps short-circuiting every
        // probe. When the tail trails on a hard error, name it in the death
        // diagnostics, so the `NodeFailed` payload says WHY, not just "session died".
        if !pdo_harness::probes::exit_code_is_verdict(harness) && diag.harness_error.is_none() {
            diag.harness_error = probes
                .transcript_tail()
                .and_then(|tail| pdo_harness::probes::hard_error(harness, &tail.text));
        }
        attach_diagnostics(&mut events, &diag);
        let closed_at = probes.session_closed_at();
        if let Some(lived) = lifetime(prior_events, node_id, iter, closed_at, now) {
            tell_early_death(&mut events, lived);
        }
        return Assessment {
            detection,
            events,
            blocked_on_limit: false,
            session_death_diagnostics: Some(diag),
        };
    }

    // Alive, but maybe wedged on Claude Code's usage-limit menu (#290):
    // observability only — the node keeps running. The gauge counts every sweep
    // the menu is visible; the event is emitted once per (node, iter) episode.
    //
    // #553: the menu anchor is proper to a harness. Gate the probe on the
    // capability — a harness without it never has its pane captured for a menu
    // whose wording belongs to another harness (ANDed first, so no pane I/O runs).
    let blocked_on_limit = caps.usage_limit
        && probes
            .capture_pane()
            .is_some_and(|pane| pdo_harness::probes::usage_limit_shown(harness, &pane));
    let events = if blocked_on_limit
        && !episode_has_event(prior_events, &EventKind::NodeBlockedOnLimit, node_id, iter)
    {
        vec![informational_event(
            EventKind::NodeBlockedOnLimit,
            run_id,
            node_id,
            iter,
            serde_json::json!({ "signal": "usage_limit_menu" }),
        )]
    } else {
        vec![]
    };

    // #469 §2: turn-end auto-completion. Opt-in, and the ONLY path here that can
    // end a live node's iteration. `claude --dangerously-skip-permissions
    // "<prompt>"` does not exit at the end of a turn — it stays in the REPL — so
    // an agent that finished without calling `pdo complete` is alive and
    // motionless, and this is its positive signature.
    //
    // #553: the turn-end substrate is a capability. Gate the probe on it — a
    // harness without it is never auto-completed on an invented heuristic (its
    // store is not the claude JSONL `parse_turn_state` reads). ANDed BEFORE the
    // transcript read, so an un-instrumented harness pays no transcript I/O.
    let turn_ended = autocomplete_turn_end
        && caps.turn_end
        && probes.transcript_tail().is_some_and(|tail| {
            quiet_long_enough(tail.mtime, now)
                && pdo_harness::probes::turn_ended(harness, &tail.text)
        })
        && probes.outputs_valid();

    Assessment {
        detection: if turn_ended {
            Detection::TurnEnded
        } else {
            detection
        },
        events,
        blocked_on_limit,
        session_death_diagnostics: None,
    }
}
