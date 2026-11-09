//! How a batch of sent comments reaches the Pipeline Manager (CORE-239;
//! CONTEXT.md « Envoi au manager »): the rules the delivery follows while it
//! watches the manager's screen. Nothing is ever typed into a prompt only a
//! person can answer (the agent's folder-trust check exits on a stray Enter),
//! and a manager that ends before answering is said on the thread instead of
//! leaving it « Awaiting manager reply » for ever.

pub use pdo_run::domain::review_comment::{ReviewDeliveryState, DELIVERY_AWAITING};

/// What the thread says while the manager's agent asks whether to trust the
/// Run's folder, and the way out.
pub const TRUST_PROMPT_REASON: &str = "The manager's agent asks whether to trust this folder. \
     Answer it in the Run's Manager tab (« Yes, I trust this folder »): the comments go out once \
     it is answered.";

/// What the thread says when the manager ended on that prompt.
pub const EXITED_AT_TRUST_PROMPT_REASON: &str = "The manager exited at its agent's folder-trust \
     prompt. Send again, then answer « Yes, I trust this folder » in the Run's Manager tab.";

/// The question an agent asks when it wants a folder trusted, lowercased:
/// `claude`'s « Quick safety check » / « Do you trust the files in this folder? »,
/// and the same question as other harnesses put it.
const TRUST_QUESTIONS: &[&str] = &[
    "do you trust the files in this folder",
    "quick safety check",
    "do you trust the contents of this directory",
    "confirm folder trust",
];

/// The answers such a prompt offers, lowercased. A question alone is not a
/// prompt: the conversation may quote one (a comment on this very file).
const TRUST_ANSWERS: &[&str] = &[
    "no, exit",
    "no, quit",
    "yes, proceed",
    "yes, continue",
    "yes, i trust this folder",
];

/// How many lines, from the bottom of a pane capture, the prompt is looked
/// for: an agent draws it at the bottom of its screen, while the capture
/// carries scrollback where an answered prompt or a quoted one may linger.
const PROMPT_LINES: usize = 20;

/// Why the screen of the manager's session cannot take a message: the prompt
/// it shows, as the thread names it. `None` when nothing blocks it. `capture`
/// is a raw pane capture (ANSI escapes kept, scrollback above the screen).
pub fn blocking_prompt(capture: &str) -> Option<&'static str> {
    let screen = normalized(&bottom(capture, PROMPT_LINES));
    let asks = TRUST_QUESTIONS.iter().any(|q| screen.contains(q));
    let offers = TRUST_ANSWERS.iter().any(|a| screen.contains(a));
    (asks && offers).then_some(TRUST_PROMPT_REASON)
}

/// The last `n` non-blank lines of `capture` (a pane pads its screen with
/// blank lines below what it drew).
fn bottom(capture: &str, n: usize) -> String {
    let lines: Vec<&str> = capture
        .lines()
        .filter(|l| !strip_escapes(l).trim().is_empty())
        .collect();
    lines[lines.len().saturating_sub(n)..].join(" ")
}

/// Escapes stripped, lowercased, whitespace collapsed: escapes and padding
/// cannot split a marker.
fn normalized(text: &str) -> String {
    strip_escapes(text)
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// `text` without its CSI escape sequences (`ESC [ … final`), nor the
/// character after a lone escape.
fn strip_escapes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
        } else if chars.next() == Some('[') {
            for next in chars.by_ref() {
                if matches!(next, '\u{40}'..='\u{7e}') {
                    break;
                }
            }
        }
    }
    out
}

/// What the delivery does after one look at the manager's session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Type the batch into the session now.
    Paste,
    /// Record where the batch stands: `state` is `awaiting`, `blocked` or
    /// `exited` (the wire words of `review_comment_delivery`).
    Record {
        state: &'static str,
        reason: Option<String>,
    },
    /// Nothing more to follow: the session is gone.
    Stop,
}

/// How many looks a batch waits for a running manager's screen to settle
/// before it is typed anyway (a manager mid-turn redraws all the time, and
/// queues what it is given).
const PATIENCE: u32 = 10;

/// The delivery of one batch, one look at a time.
#[derive(Debug, Clone)]
pub struct Watch {
    /// The batch still has to be typed into the session: the manager was
    /// already running (a manager the send starts reads it in its launch prompt).
    must_paste: bool,
    /// What the screen showed last time, when a prompt blocked it.
    blocked_by: Option<&'static str>,
    /// The screen at the previous look, to tell a drawn, settled screen from
    /// a harness still starting.
    last_screen: Option<String>,
    /// Looks spent waiting for the screen to settle.
    waited: u32,
}

impl Watch {
    /// `must_paste`: the manager was already running, the batch is typed into
    /// it once its screen can take it.
    pub fn new(must_paste: bool) -> Watch {
        Watch {
            must_paste,
            blocked_by: None,
            last_screen: None,
            waited: 0,
        }
    }

    /// One look at the manager's session: its screen, or `None` once it is gone.
    pub fn observe(&mut self, screen: Option<&str>) -> Vec<Step> {
        let Some(screen) = screen else {
            let reason = self
                .blocked_by
                .map(|_| EXITED_AT_TRUST_PROMPT_REASON.to_string());
            return vec![
                Step::Record {
                    state: ReviewDeliveryState::Exited.as_str(),
                    reason,
                },
                Step::Stop,
            ];
        };
        let mut steps = Vec::new();
        let drawn = normalized(screen);
        let settled = !drawn.is_empty() && self.last_screen.as_deref() == Some(drawn.as_str());
        self.last_screen = Some(drawn);
        match blocking_prompt(screen) {
            Some(reason) => {
                if self.blocked_by != Some(reason) {
                    steps.push(Step::Record {
                        state: ReviewDeliveryState::Blocked.as_str(),
                        reason: Some(reason.to_string()),
                    });
                }
                self.blocked_by = Some(reason);
            }
            None => {
                if self.blocked_by.take().is_some() {
                    steps.push(Step::Record {
                        state: DELIVERY_AWAITING,
                        reason: None,
                    });
                }
                // Typed only into a screen drawn and settled: a harness still
                // starting may not read its input yet, or draw a prompt next.
                if self.must_paste {
                    self.waited += 1;
                    if settled || self.waited > PATIENCE {
                        self.must_paste = false;
                        steps.push(Step::Paste);
                    }
                }
            }
        }
        steps
    }

    /// The batch is still to be handed over, or held by a prompt: the
    /// delivery keeps looking closely.
    pub fn in_flight(&self) -> bool {
        self.must_paste || self.blocked_by.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLAUDE_TRUST: &str =
        "╭──────────╮\n│ Quick safety check: Is this a project you created or one you trust? │\n\
        │ ❯ 1. No, exit │\n│   2. Yes, I trust this folder │\n╰──────────╯";
    const CLAUDE_READY: &str = "╭──────────╮\n│ >  │\n╰──────────╯\n  ? for shortcuts";

    fn blocked() -> Step {
        Step::Record {
            state: "blocked",
            reason: Some(TRUST_PROMPT_REASON.to_string()),
        }
    }

    #[test]
    fn the_trust_prompt_blocks_whatever_its_wording() {
        assert_eq!(blocking_prompt(CLAUDE_TRUST), Some(TRUST_PROMPT_REASON));
        assert!(
            blocking_prompt("Do you trust the files in this folder?\n> 1. Yes, proceed").is_some()
        );
        assert!(blocking_prompt(
            "DO YOU TRUST THE CONTENTS OF THIS DIRECTORY?\n1. Yes, continue\n2. No, quit"
        )
        .is_some());
        assert_eq!(blocking_prompt(CLAUDE_READY), None);
        assert_eq!(blocking_prompt(""), None);
    }

    #[test]
    fn colours_and_padding_do_not_hide_the_prompt() {
        let coloured = "\u{1b}[1mDo you \u{1b}[38;5;174mtrust\u{1b}[0m the   files\n in this folder?\n\u{1b}[2m❯ 1. Yes,\u{1b}[0m proceed\n  2. No, exit";
        assert!(blocking_prompt(coloured).is_some());
    }

    #[test]
    fn a_running_manager_gets_the_batch_once_its_screen_is_drawn_and_settled() {
        let mut watch = Watch::new(true);
        assert!(watch.in_flight());
        assert_eq!(watch.observe(Some("")), vec![], "nothing drawn yet");
        assert_eq!(
            watch.observe(Some("")),
            vec![],
            "a blank screen never settles"
        );
        assert_eq!(watch.observe(Some(CLAUDE_READY)), vec![], "just drawn");
        assert_eq!(watch.observe(Some(CLAUDE_READY)), vec![Step::Paste]);
        assert!(!watch.in_flight());
        assert_eq!(watch.observe(Some(CLAUDE_READY)), vec![], "pasted once");
    }

    #[test]
    fn a_manager_mid_turn_gets_the_batch_after_a_while() {
        let mut watch = Watch::new(true);
        for i in 0..PATIENCE {
            let busy = format!("✻ Working… ({i}s)");
            assert_eq!(watch.observe(Some(&busy)), vec![], "look {i}");
        }
        assert_eq!(watch.observe(Some("✻ Working… (99s)")), vec![Step::Paste]);
    }

    #[test]
    fn a_quoted_question_is_not_a_prompt() {
        // The conversation quotes the question (a comment on this file): no
        // answers on offer, nothing blocks.
        let quoted = "> 14 | Do you trust the files in this folder?\n  Comment: reword it\n  ? for shortcuts";
        assert_eq!(blocking_prompt(quoted), None);
        // Far above the bottom of the screen, even a whole prompt is history.
        let mut capture = String::from(CLAUDE_TRUST);
        for i in 0..PROMPT_LINES {
            capture.push_str(&format!("\nline {i}"));
        }
        assert_eq!(blocking_prompt(&capture), None);
    }

    #[test]
    fn nothing_is_typed_into_a_trust_prompt_and_the_hold_is_recorded_once() {
        let mut watch = Watch::new(true);
        assert_eq!(watch.observe(Some(CLAUDE_TRUST)), vec![blocked()]);
        assert_eq!(watch.observe(Some(CLAUDE_TRUST)), vec![], "recorded once");
        assert!(watch.in_flight());
        // Answered: on its way again, and typed once the screen settles.
        assert_eq!(
            watch.observe(Some(CLAUDE_READY)),
            vec![Step::Record {
                state: "awaiting",
                reason: None
            }]
        );
        assert_eq!(watch.observe(Some(CLAUDE_READY)), vec![Step::Paste]);
    }

    #[test]
    fn a_manager_started_with_the_batch_is_only_watched() {
        let mut watch = Watch::new(false);
        assert!(!watch.in_flight());
        assert_eq!(watch.observe(Some(CLAUDE_READY)), vec![]);
        assert_eq!(watch.observe(Some(CLAUDE_TRUST)), vec![blocked()]);
        assert_eq!(
            watch.observe(Some(CLAUDE_READY)),
            vec![Step::Record {
                state: "awaiting",
                reason: None
            }]
        );
    }

    #[test]
    fn a_session_gone_is_recorded_exited_and_ends_the_watch() {
        let mut watch = Watch::new(false);
        watch.observe(Some(CLAUDE_READY));
        assert_eq!(
            watch.observe(None),
            vec![
                Step::Record {
                    state: "exited",
                    reason: None
                },
                Step::Stop
            ]
        );
    }

    #[test]
    fn a_session_gone_at_the_trust_prompt_says_so() {
        let mut watch = Watch::new(false);
        watch.observe(Some(CLAUDE_TRUST));
        assert_eq!(
            watch.observe(None),
            vec![
                Step::Record {
                    state: "exited",
                    reason: Some(EXITED_AT_TRUST_PROMPT_REASON.to_string())
                },
                Step::Stop
            ]
        );
    }
}
