//! Why the sweep kills a session: the verbatim log line, and whether it is an
//! absence verdict or routine housekeeping.

/// Flat on purpose: two of the messages are irregular and a composed
/// `{kind} × {cause}` formatter would smooth them over silently. The NodeRun
/// arms say `killing session for absent run …` with **no** "node" word (unlike
/// manager/shell), and the stale arm keys off the session name rather than
/// `run_id`/`node_id`. `journalctl | grep "Orphan sweep: killing session for
/// absent run"` is how #485 was diagnosed in the first place, so these strings
/// are a contract — `kill_reason_messages_are_verbatim` pins all nine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KillReason {
    UnrecognisedName {
        session_name: String,
    },
    ManagerRunAbsent {
        run_id: String,
    },
    ManagerRunArchived {
        run_id: String,
    },
    ShellRunAbsent {
        run_id: String,
    },
    ShellRunArchived {
        run_id: String,
    },
    NodeRunAbsent {
        run_id: String,
        node_id: String,
    },
    NodeRunArchived {
        run_id: String,
        node_id: String,
    },
    /// The NodeRun TTL arm. Neither Manager (#458) nor Shell (#316) has one —
    /// do not "unify" them, and do not unify it with [`Self::LibAssistIdle`]
    /// either: that one keys on human presence, not on completion.
    NodeRunStale {
        session_name: String,
        age_secs: i64,
    },
    /// The library assistant with nobody attached and no fresh focus (#594,
    /// ADR-0051 §4). Replaces ADR-0048's unconditional `Keep`, which left a
    /// session leaked by a browser reload alive with **no** upper bound.
    LibAssistIdle {
        session_name: String,
        /// Seconds since the UI last declared an open edit view, or `None` when
        /// it never declared one — the trace a reload leaves, and the case that
        /// motivated the arm. Said in the message either way, so the journal
        /// distinguishes "they walked away" from "the tab is gone".
        idle_secs: Option<i64>,
    },
}

impl std::fmt::Display for KillReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KillReason::UnrecognisedName { session_name } => {
                write!(
                    f,
                    "Orphan sweep: killing unrecognised session {session_name}"
                )
            }
            KillReason::ManagerRunAbsent { run_id } => {
                write!(
                    f,
                    "Orphan sweep: killing manager session for absent run {run_id}"
                )
            }
            KillReason::ManagerRunArchived { run_id } => {
                write!(
                    f,
                    "Orphan sweep: killing manager session for archived run {run_id}"
                )
            }
            KillReason::ShellRunAbsent { run_id } => {
                write!(
                    f,
                    "Orphan sweep: killing shell session for absent run {run_id}"
                )
            }
            KillReason::ShellRunArchived { run_id } => {
                write!(
                    f,
                    "Orphan sweep: killing shell session for archived run {run_id}"
                )
            }
            KillReason::NodeRunAbsent { run_id, node_id } => {
                write!(
                    f,
                    "Orphan sweep: killing session for absent run {run_id}/{node_id}"
                )
            }
            KillReason::NodeRunArchived { run_id, node_id } => {
                write!(
                    f,
                    "Orphan sweep: killing session for archived run {run_id}/{node_id}"
                )
            }
            KillReason::NodeRunStale {
                session_name,
                age_secs,
            } => write!(
                f,
                "Orphan sweep: killing stale session {session_name} (completed {age_secs}s ago)"
            ),
            KillReason::LibAssistIdle {
                session_name,
                idle_secs: Some(secs),
            } => write!(
                f,
                "Orphan sweep: killing idle library assistant {session_name} \
                 (no edit view for {secs}s)"
            ),
            KillReason::LibAssistIdle {
                session_name,
                idle_secs: None,
            } => write!(
                f,
                "Orphan sweep: killing idle library assistant {session_name} \
                 (no edit view ever declared)"
            ),
        }
    }
}

impl KillReason {
    /// Whether this kill is an *absence* verdict (including an unparseable
    /// name) rather than routine housekeeping.
    ///
    /// After #485 an absence verdict on a **live** session is a "can no longer
    /// happen": it means either a genuinely leaked session or a reservation that
    /// never landed. Housekeeping (archived run, TTL-expired node) is nominal
    /// and happens every 60 s. The split drives both the log level in
    /// the sweep (`application::sweep`) and the `killed_for_absent_run` gauge on `GET /sessions`.
    pub fn is_absence(&self) -> bool {
        matches!(
            self,
            KillReason::UnrecognisedName { .. }
                | KillReason::ManagerRunAbsent { .. }
                | KillReason::ShellRunAbsent { .. }
                | KillReason::NodeRunAbsent { .. }
        )
    }
}
