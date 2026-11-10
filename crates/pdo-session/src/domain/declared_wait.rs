//! The **declared wait** (#588 / ADR-0069 §1): a node is `awaiting_user` when its
//! agent says so — never inferred from a spawn or from a silence.
//!
//! Three doors into the wait, one out by keystroke:
//! - `pdo wait-user [--message "<question>"]` → cause `declared`; accepted on
//!   **any** node holding a live session, interactive or not (a node stuck on a
//!   question becomes visible, #290); refused by name on a `script` node or
//!   without a live session; idempotent (a repeat with the same message is a
//!   `noop`, a new message refreshes the banner).
//! - a refused `pdo complete` on an unreleased interactive node → the daemon
//!   declares the wait for the agent, cause `completion_not_released`.
//! - the orchestrator binding marker (ADR-0064) keeps its own `children_pending`.
//! - **Out**: an Enter key that crossed the UI's PTY bridge after at least one
//!   typed byte since the declaration ([`classify_input`]) appends `NodeResumed`;
//!   the completion release (ADR-0068) keeps its own event. Arrow keys, scroll,
//!   mouse reports and the runtime's own paste never lift a wait — the first two
//!   are escape sequences, the last never crosses the bridge.
//!
//! Pure: the facts come from the Run's projection, the decision is taken here.

/// The agent declared the wait (`pdo wait-user`).
pub const CAUSE_DECLARED: &str = "declared";

/// The daemon declared it on a refused unreleased completion (ADR-0068).
pub const CAUSE_COMPLETION_NOT_RELEASED: &str = "completion_not_released";

/// `--message` is capped so the banner never grows past one line (design
/// decision, 2026-09-12). Both the CLI and the daemon truncate — the daemon
/// defensively, for a client that is not `pdo`.
pub const MESSAGE_MAX_CHARS: usize = 100;

/// Truncate a wait message to [`MESSAGE_MAX_CHARS`] characters, with an ellipsis.
/// Whitespace is collapsed to one line first: the banner has one line to give.
pub fn truncate_message(raw: &str) -> String {
    let one_line: String = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() <= MESSAGE_MAX_CHARS {
        return one_line;
    }
    let mut out: String = one_line.chars().take(MESSAGE_MAX_CHARS - 1).collect();
    out.push('…');
    out
}

/// The daemon's sentence when a refused `pdo complete` declares the wait. The
/// banner (A3) shows its own words and a « Show Approve » action instead.
pub const COMPLETION_NOT_RELEASED_MESSAGE: &str =
    "Completion not released. Choose Auto-approve when ready in the Approve menu so the agent can finish, or click Approve to take the artifacts as they are.";

/// Why a wait could not be declared. Every variant is a named refusal; the CLI
/// maps them to exit 3 (refused, still your turn — nothing terminal happened).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WaitRefusal {
    RunNotFound,
    RunNotLive,
    NodeNotFound,
    /// A `script` node runs no agent: nobody there can be waiting on a human.
    NodeIsScript,
    NodeSessionNotLive,
}

impl WaitRefusal {
    pub fn slug(&self) -> &'static str {
        match self {
            Self::RunNotFound => "run_not_found",
            Self::RunNotLive => "run_not_live",
            Self::NodeNotFound => "node_not_found",
            Self::NodeIsScript => "node_is_script",
            Self::NodeSessionNotLive => "node_session_not_live",
        }
    }

    pub fn message(&self, run_id: &str, node_id: &str, iter: i64) -> String {
        match self {
            Self::RunNotFound => format!("run {run_id} does not exist"),
            Self::RunNotLive => format!("run {run_id} has no live session to wait on"),
            Self::NodeNotFound => format!("node {node_id} is not part of run {run_id}"),
            Self::NodeIsScript => {
                format!("node {node_id} is a script node: it runs no agent that could wait on you")
            }
            Self::NodeSessionNotLive => {
                format!("node {node_id} iter {iter} has no live session")
            }
        }
    }

    /// The same refusal for a gesture that speaks to the node's agent (a text
    /// sent to its input, a file imported for it) rather than a wait.
    pub fn gesture_message(&self, run_id: &str, node_id: &str, iter: i64) -> String {
        match self {
            Self::RunNotLive => format!("run {run_id} is not running"),
            Self::NodeIsScript => {
                format!("node {node_id} is a script node: no agent runs there")
            }
            _ => self.message(run_id, node_id, iter),
        }
    }

    /// Whether the refusal names something that does not exist (a `404`), rather
    /// than a state that forbids the wait (a `409`).
    pub fn is_not_found(&self) -> bool {
        matches!(self, Self::RunNotFound | Self::NodeNotFound)
    }
}

/// What one declaration did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeclareOutcome {
    /// The node was running: it is now awaiting, with this message.
    Declared,
    /// Already awaiting with the same cause and message: nothing written.
    Noop,
    /// Already awaiting; the message (or cause) changed, so the banner refreshes.
    Refreshed,
}

/// The wait a node currently shows, as the projection folded it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AwaitingMarker {
    pub cause: String,
    pub message: Option<String>,
}

/// One iteration of the node, as far as a wait cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IterationFacts {
    pub iter: i64,
    /// `Running` or `AwaitingUser`: the iteration holds a session.
    pub holds_session: bool,
}

/// A started node's facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeWaitFacts {
    /// Its latest iteration.
    pub iter: i64,
    /// Its status is `AwaitingUser`.
    pub awaiting: bool,
    /// The wait it shows, when the projection kept one.
    pub marker: Option<AwaitingMarker>,
    pub iterations: Vec<IterationFacts>,
}

/// What the Run's projection says about one node, for a wait.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaitFacts {
    /// The Run is live (running, awaiting, paused).
    pub run_live: bool,
    /// The pipeline defines the node.
    pub defined: bool,
    /// The node is a `script` node.
    pub is_script: bool,
    /// `None` while the node never started.
    pub node: Option<NodeWaitFacts>,
}

/// Whether `(node, iter)` may wait at all, before tmux is asked whether its
/// session is really up.
pub fn check_target(facts: Option<&WaitFacts>, iter: i64) -> Result<(), WaitRefusal> {
    let Some(facts) = facts else {
        return Err(WaitRefusal::RunNotFound);
    };
    if !facts.run_live {
        return Err(WaitRefusal::RunNotLive);
    }
    let Some(node) = facts.node.as_ref() else {
        // A defined-but-never-started node has no session either; say which.
        return Err(if facts.defined {
            WaitRefusal::NodeSessionNotLive
        } else {
            WaitRefusal::NodeNotFound
        });
    };
    if facts.is_script {
        return Err(WaitRefusal::NodeIsScript);
    }
    let Some(attempt) = node.iterations.iter().find(|a| a.iter == iter) else {
        return Err(WaitRefusal::NodeSessionNotLive);
    };
    if !attempt.holds_session || iter != node.iter {
        return Err(WaitRefusal::NodeSessionNotLive);
    }
    Ok(())
}

/// The iteration a gesture toward a node's agent addresses — `iter`, or the
/// node's current one when absent — once [`check_target`] accepts it. A
/// refusal names the iteration it was about. tmux still has to confirm the
/// session.
pub fn live_target(
    facts: Option<&WaitFacts>,
    iter: Option<i64>,
) -> Result<i64, (WaitRefusal, i64)> {
    let current = facts
        .and_then(|f| f.node.as_ref())
        .map(|node| node.iter)
        .unwrap_or(1);
    let iter = iter.unwrap_or(current);
    check_target(facts, iter).map_err(|refusal| (refusal, iter))?;
    Ok(iter)
}

/// What a declaration writes: nothing (`None`, a no-op), or the
/// `NodeAwaitingUser` payload and whether it refreshes a wait already shown.
pub fn plan_declaration(
    node: &NodeWaitFacts,
    cause: &str,
    message: Option<&str>,
) -> Option<(serde_json::Value, DeclareOutcome)> {
    // The cap is for the agent's `--message`; the daemon's own sentence on a
    // refused completion is authored to fit the banner and stays whole.
    let message = message
        .map(|m| {
            if cause == CAUSE_DECLARED {
                truncate_message(m)
            } else {
                m.trim().to_string()
            }
        })
        .filter(|m| !m.is_empty());
    if node.awaiting {
        if let Some(current) = node.marker.as_ref() {
            if current.cause == cause && current.message == message {
                return None;
            }
        }
    }
    let mut payload = serde_json::json!({ "cause": cause });
    if let Some(m) = &message {
        payload["message"] = serde_json::json!(m);
    }
    let outcome = if node.awaiting {
        DeclareOutcome::Refreshed
    } else {
        DeclareOutcome::Declared
    };
    Some((payload, outcome))
}

/// Whether a keystroke may lift the wait `(node, iter)` shows: the node awaits
/// on its latest iteration, on a declared or refused-completion cause (a marker
/// with no cause counts as declared). Never a `children_pending` marker (the
/// orchestrator gave its turn back, ADR-0064).
pub fn liftable(node: &NodeWaitFacts, iter: i64) -> bool {
    if node.iter != iter || !node.awaiting {
        return false;
    }
    node.marker
        .as_ref()
        .is_none_or(|a| a.cause == CAUSE_DECLARED || a.cause == CAUSE_COMPLETION_NOT_RELEASED)
}

/// What a chunk of PTY input from the browser amounts to, for the Enter rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct InputSignal {
    /// At least one **typed** byte: printable ASCII or a UTF-8 lead/continuation
    /// byte, outside any escape sequence. Arrow keys, mouse reports and scroll
    /// (all `ESC [ …`), Backspace, Tab and bare control bytes do not count.
    pub typed: bool,
    /// At least one Enter (`\r`, or `\n` for a client that sends it) outside any
    /// escape sequence.
    pub enter: bool,
}

/// Classify one binary frame from the PTY WebSocket. Escape sequences are
/// skipped whole: `ESC [ … <final 0x40..=0x7e>` (CSI, which covers arrows, mouse
/// reports, bracketed-paste markers, focus reports and the DA / cursor-position
/// replies), `ESC O <byte>` (SS3 keypad), the control strings `ESC ] …` (OSC),
/// `ESC P …` (DCS), `ESC _ …` (APC), `ESC ^ …` (PM) and `ESC X …` (SOS) up to
/// their `ESC \` or BEL terminator, plus a bare `ESC <byte>`.
///
/// The control strings matter because a terminal answers queries on its own:
/// when a client attaches, tmux asks the browser's xterm.js for its colours
/// (OSC 10/11) and its version (XTVERSION, a DCS reply), and xterm.js answers
/// through the same bridge as the keyboard. Read byte by byte, `rgb:1e1e/…`
/// would be a person typing, and opening a node's terminal would take the
/// floor (`node_typed`) with no key pressed. A string cut at the frame's end
/// swallows the rest of the frame.
pub fn classify_input(bytes: &[u8]) -> InputSignal {
    let mut signal = InputSignal::default();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == 0x1b {
            i += 1;
            match bytes.get(i) {
                Some(b'[') => {
                    i += 1;
                    while let Some(&c) = bytes.get(i) {
                        i += 1;
                        if (0x40..=0x7e).contains(&c) {
                            break;
                        }
                    }
                }
                Some(b'O') => i += 2,
                Some(b']' | b'P' | b'_' | b'^' | b'X') => i = skip_control_string(bytes, i + 1),
                Some(_) => i += 1,
                None => {}
            }
            continue;
        }
        if b == b'\r' || b == b'\n' {
            signal.enter = true;
        } else if (0x20..=0x7e).contains(&b) || b >= 0x80 {
            signal.typed = true;
        }
        i += 1;
    }
    signal
}

/// The index just past the control string whose body starts at `start`: past
/// its BEL, or past its `ESC \` (ST). An `ESC` followed by anything else ends
/// the string there, as a terminal would; no terminator swallows the frame.
fn skip_control_string(bytes: &[u8], start: usize) -> usize {
    let mut i = start;
    while let Some(&c) = bytes.get(i) {
        match c {
            0x07 => return i + 1,
            0x1b if bytes.get(i + 1) == Some(&b'\\') => return i + 2,
            0x1b => return i,
            _ => i += 1,
        }
    }
    bytes.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn message_is_collapsed_to_one_line_and_capped_with_an_ellipsis() {
        assert_eq!(truncate_message("  a\n b  "), "a b");
        let long = "x".repeat(150);
        let capped = truncate_message(&long);
        assert_eq!(capped.chars().count(), MESSAGE_MAX_CHARS);
        assert!(capped.ends_with('…'));
        let exact = "y".repeat(MESSAGE_MAX_CHARS);
        assert_eq!(truncate_message(&exact), exact);
    }

    #[test]
    fn typed_text_then_enter_is_both() {
        let s = classify_input(b"the strip\r");
        assert_eq!(
            s,
            InputSignal {
                typed: true,
                enter: true
            }
        );
    }

    #[test]
    fn a_bare_enter_is_not_typed() {
        assert_eq!(
            classify_input(b"\r"),
            InputSignal {
                typed: false,
                enter: true
            }
        );
        assert_eq!(
            classify_input(b"\n"),
            InputSignal {
                typed: false,
                enter: true
            }
        );
    }

    #[test]
    fn arrows_mouse_and_scroll_are_neither() {
        // Up arrow, left arrow (application mode), SGR mouse wheel report.
        for seq in [
            &b"\x1b[A"[..],
            b"\x1bOD",
            b"\x1b[<64;10;5M",
            b"\x1b[<65;10;5M",
        ] {
            assert_eq!(classify_input(seq), InputSignal::default(), "{seq:?}");
        }
    }

    /// A terminal answers queries on its own: tmux asks the browser's xterm.js
    /// for its colours, its version and its attributes when a client attaches,
    /// and xterm.js reports focus changes. None of those replies is a person
    /// typing, so merely opening a node's terminal never takes the floor.
    #[test]
    fn terminal_replies_are_neither() {
        for seq in [
            // OSC 10/11 colour replies, ended by ST or by BEL.
            &b"\x1b]10;rgb:d8d8/d8d8/d8d8\x1b\\"[..],
            b"\x1b]11;rgb:1e1e/1e1e/1e1e\x07",
            // DCS: XTVERSION and DECRQSS replies.
            b"\x1bP>|xterm.js(5.5.0)\x1b\\",
            b"\x1bP1$r0;1m\x1b\\",
            // APC, PM and SOS strings.
            b"\x1b_Gi=1;OK\x1b\\",
            b"\x1b^private\x1b\\",
            b"\x1bXstring\x1b\\",
            // DA1, DA2, a cursor position report, focus in and out.
            b"\x1b[?1;2c",
            b"\x1b[>0;276;0c",
            b"\x1b[12;40R",
            b"\x1b[I",
            b"\x1b[O",
        ] {
            assert_eq!(classify_input(seq), InputSignal::default(), "{seq:?}");
        }
        // Back to back, as one frame on attach.
        assert_eq!(
            classify_input(b"\x1b[?1;2c\x1b]11;rgb:0000/0000/0000\x1b\\\x1b[I"),
            InputSignal::default()
        );
    }

    /// A reply cut at the frame's end swallows the rest of the frame, never
    /// counts it as typed; the next frame is read afresh.
    #[test]
    fn an_unterminated_string_reply_is_not_typed() {
        assert_eq!(
            classify_input(b"\x1b]11;rgb:0000/00"),
            InputSignal::default()
        );
        assert_eq!(
            classify_input(b"after\r"),
            InputSignal {
                typed: true,
                enter: true
            }
        );
    }

    #[test]
    fn text_typed_after_a_reply_still_counts() {
        assert_eq!(
            classify_input(b"\x1b]11;rgb:0000/0000/0000\x07yes\r"),
            InputSignal {
                typed: true,
                enter: true
            }
        );
    }

    #[test]
    fn the_terminals_own_replies_are_neither() {
        // What xterm.js answers on its own when the program in the pane queries
        // it: a background colour (OSC 11, ended by ST or BEL), its version
        // (XTVERSION, a DCS string), a status report (DECRQSS), a device
        // attributes report (CSI). None of it was typed by a person.
        for seq in [
            &b"\x1b]11;rgb:1e1e/1e1e/1e1e\x1b\\"[..],
            b"\x1b]10;rgb:ffff/ffff/ffff\x07",
            b"\x1bP>|xterm.js(6.0.0)\x1b\\",
            b"\x1bP1$r0;1m\x1b\\",
            b"\x1b_Gi=1;OK\x1b\\",
            b"\x1b[?1;2c",
        ] {
            assert_eq!(classify_input(seq), InputSignal::default(), "{seq:?}");
        }
    }

    #[test]
    fn a_keystroke_after_a_terminal_reply_still_counts() {
        assert_eq!(
            classify_input(b"\x1b]11;rgb:0/0/0\x07y\r"),
            InputSignal {
                typed: true,
                enter: true
            }
        );
    }

    #[test]
    fn backspace_tab_and_control_bytes_are_not_typed() {
        assert_eq!(classify_input(b"\x7f\t\x03"), InputSignal::default());
    }

    #[test]
    fn utf8_counts_as_typed() {
        assert_eq!(
            classify_input("é".as_bytes()),
            InputSignal {
                typed: true,
                enter: false
            }
        );
    }

    #[test]
    fn bracketed_paste_text_counts_as_typed() {
        let s = classify_input(b"\x1b[200~pasted answer\x1b[201~");
        assert_eq!(
            s,
            InputSignal {
                typed: true,
                enter: false
            }
        );
    }

    #[test]
    fn refusal_slugs_are_stable() {
        assert_eq!(WaitRefusal::NodeIsScript.slug(), "node_is_script");
        assert_eq!(
            WaitRefusal::NodeSessionNotLive.slug(),
            "node_session_not_live"
        );
        assert_eq!(WaitRefusal::RunNotLive.slug(), "run_not_live");
    }

    fn started(iter: i64, awaiting: Option<AwaitingMarker>) -> NodeWaitFacts {
        NodeWaitFacts {
            iter,
            awaiting: awaiting.is_some(),
            marker: awaiting,
            iterations: vec![IterationFacts {
                iter,
                holds_session: true,
            }],
        }
    }

    fn facts(node: Option<NodeWaitFacts>) -> WaitFacts {
        WaitFacts {
            run_live: true,
            defined: true,
            is_script: false,
            node,
        }
    }

    #[test]
    fn a_wait_needs_a_live_run_and_a_node_holding_its_latest_session() {
        assert_eq!(check_target(None, 1), Err(WaitRefusal::RunNotFound));
        let mut dead = facts(Some(started(1, None)));
        dead.run_live = false;
        assert_eq!(check_target(Some(&dead), 1), Err(WaitRefusal::RunNotLive));
        let mut unknown = facts(None);
        unknown.defined = false;
        assert_eq!(
            check_target(Some(&unknown), 1),
            Err(WaitRefusal::NodeNotFound)
        );
        assert_eq!(
            check_target(Some(&facts(None)), 1),
            Err(WaitRefusal::NodeSessionNotLive),
            "a defined node that never started has no session"
        );
        let mut script = facts(Some(started(1, None)));
        script.is_script = true;
        assert_eq!(
            check_target(Some(&script), 1),
            Err(WaitRefusal::NodeIsScript)
        );
        let live = facts(Some(started(2, None)));
        assert_eq!(
            check_target(Some(&live), 1),
            Err(WaitRefusal::NodeSessionNotLive),
            "not the latest iteration"
        );
        assert_eq!(check_target(Some(&live), 2), Ok(()));
    }

    #[test]
    fn a_repeat_is_a_noop_and_a_new_message_refreshes() {
        let running = started(1, None);
        let (payload, outcome) =
            plan_declaration(&running, CAUSE_DECLARED, Some(" Which DB? ")).unwrap();
        assert_eq!(
            payload,
            serde_json::json!({"cause": "declared", "message": "Which DB?"})
        );
        assert_eq!(outcome, DeclareOutcome::Declared);

        let waiting = started(
            1,
            Some(AwaitingMarker {
                cause: CAUSE_DECLARED.into(),
                message: Some("Which DB?".into()),
            }),
        );
        assert_eq!(
            plan_declaration(&waiting, CAUSE_DECLARED, Some("Which DB?")),
            None
        );
        let (_, outcome) =
            plan_declaration(&waiting, CAUSE_DECLARED, Some("Which cache?")).unwrap();
        assert_eq!(outcome, DeclareOutcome::Refreshed);
        let (payload, _) = plan_declaration(&running, CAUSE_DECLARED, Some("   ")).unwrap();
        assert_eq!(
            payload,
            serde_json::json!({"cause": "declared"}),
            "a blank message is none"
        );
    }

    #[test]
    fn the_daemon_sentence_is_never_capped() {
        let (payload, _) = plan_declaration(
            &started(1, None),
            CAUSE_COMPLETION_NOT_RELEASED,
            Some(COMPLETION_NOT_RELEASED_MESSAGE),
        )
        .unwrap();
        assert_eq!(payload["message"], COMPLETION_NOT_RELEASED_MESSAGE);
        assert!(COMPLETION_NOT_RELEASED_MESSAGE.chars().count() > MESSAGE_MAX_CHARS);
    }

    #[test]
    fn only_a_declared_or_refused_completion_wait_is_lifted_by_a_keystroke() {
        let marker = |cause: &str| {
            Some(AwaitingMarker {
                cause: cause.into(),
                message: None,
            })
        };
        assert!(liftable(&started(1, marker(CAUSE_DECLARED)), 1));
        assert!(liftable(
            &started(1, marker(CAUSE_COMPLETION_NOT_RELEASED)),
            1
        ));
        assert!(!liftable(&started(1, marker("children_pending")), 1));
        assert!(
            !liftable(&started(2, marker(CAUSE_DECLARED)), 1),
            "an older iteration"
        );
        assert!(!liftable(&started(1, None), 1), "a running node");
        let mut no_cause = started(1, None);
        no_cause.awaiting = true;
        assert!(
            liftable(&no_cause, 1),
            "a wait with no cause counts as declared"
        );
    }
}
