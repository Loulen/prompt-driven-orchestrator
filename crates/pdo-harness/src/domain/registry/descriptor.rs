//! A harness descriptor: how PDO launches, resumes and attaches it (ADR-0045).

/// A harness, as PDO launches / resumes / attaches it (ADR-0045).
///
/// The two templates are rendered by [`crate::domain::argv`]; the caller fills
/// the holes (`{prompt}`, `{model}`, `{effort}`, `{session_id}`, `{settings}`,
/// and the resume-only `{resume}` selector). The env block is exported before an
/// **agent** tail — that is where `claude`'s `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1`
/// now comes from (AC #4); `script` / `shell` tails get it forced by the wrapper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessDescriptor {
    /// The harness name (`claude`, `opencode`) — the registry key.
    pub name: String,
    /// The program that runs the agent — probed on `PATH` at spawn. Not found ⇒
    /// the spawn fails **fast**, naming the harness, and writes no start event
    /// (ADR-0037).
    pub binary: String,
    /// The launch tail as an argv-token template.
    pub launch: Vec<String>,
    /// The resume tail as an argv-token template. **Empty** ⇒ the harness has no
    /// resume mechanism, so a resume serves the last pane snapshot rather than an
    /// error (AC #9, same branch as a `script` node).
    pub resume: Vec<String>,
    /// The flag that fills the resume template's `{resume}` hole to re-enter a
    /// conversation **by session identity** — `claude`/`copilot`'s `--resume`. The
    /// resume seam prepends it to the node's recorded session id (`--resume '<id>'`).
    /// **Empty** ⇒ the harness cannot resume by identity, so the seam falls back to
    /// [`Self::resume_blind`]. This is the #614 move: the resume *verb* is a property
    /// of the harness, not a constant hard-coded in the resume seam.
    pub resume_by_id: String,
    /// The flag that fills `{resume}` when the node has **no** recorded identity —
    /// `claude`/`opencode`'s blind `--continue` (re-enter the cwd's latest session).
    /// **Empty** ⇒ the harness never blind-continues, so a resume with no identity
    /// renders no resume flag at all (`copilot`: resume is by identity or not at
    /// all — never a blind continue).
    pub resume_blind: String,
    /// Env exported before an agent tail (`K=V`). Rendered by the wrapper.
    pub env: Vec<(String, String)>,
}

impl HarnessDescriptor {
    /// Whether the LAUNCH template carries an `{effort}` hole.
    ///
    /// `opencode` has no effort axis at launch (measured on 1.18.18: the effort
    /// variant is an in-session command, not a flag), so the UI greys its effort
    /// picker off this fact alone — an absence declared by the descriptor's shape,
    /// no extra flag needed (AC #13, ADR-0045).
    pub fn has_effort_hole(&self) -> bool {
        self.launch.iter().any(|t| t.contains("{effort}"))
    }

    /// Whether the harness can re-enter a saved conversation. `false` ⇒ a resume
    /// serves the pane snapshot (AC #9).
    pub fn can_resume(&self) -> bool {
        !self.resume.is_empty()
    }

    /// Whether the templates carry a `{settings}` hole — i.e. this harness accepts
    /// an injected settings file (the turn-end `Stop` hook substrate, #433/ADR-0043).
    ///
    /// #613/ADR-0051 (correctif 8): PDO writes the claude-format settings file
    /// **only** for a harness that has this hole. `opencode` has none (its template
    /// carries no `{settings}` token), so a node on it gets no stray settings file
    /// beside its prompt — "the absence is said, never supplied". Checks both
    /// templates so a harness that resumes but does not launch with settings (or the
    /// reverse) is still handled at the matching seam.
    pub fn has_settings_hole(&self) -> bool {
        self.launch
            .iter()
            .chain(self.resume.iter())
            .any(|t| t.contains("{settings}"))
    }

    /// Whether the LAUNCH template pins a session identity (`{session_id}`).
    ///
    /// `opencode` cannot (measured: launching with a fresh id answers "Session not
    /// found" and exits 1 — its selector *continues* a session, never creates
    /// one), so PDO never pins one for it and attributes by working dir alone.
    pub fn pins_session_id(&self) -> bool {
        self.launch.iter().any(|t| t.contains("{session_id}"))
    }

    /// Whether the LAUNCH template can carry a settings file (`{settings}`), and
    /// therefore whether PDO can arm a hook on this harness (#433, #594).
    ///
    /// Only `claude` does. Without the hole the token is dropped silently at
    /// render, so a caller that depends on a hook — the library assistant's
    /// per-message focus injection (ADR-0051 §3) — must ask *before* assuming its
    /// mechanism is armed, and say so when it is not. Same shape as
    /// [`Self::has_effort_hole`]: an absence declared by the descriptor.
    pub fn can_inject_hooks(&self) -> bool {
        self.launch.iter().any(|t| t.contains("{settings}"))
    }
}

/// The `claude` harness name — the floor of the precedence chain (ADR-0046).
pub const CLAUDE: &str = "claude";
/// The `opencode` harness name (ADR-0045, measured on 1.18.18).
pub const OPENCODE: &str = "opencode";
/// The `copilot` harness name — PDO's second first-party harness (#614).
pub const COPILOT: &str = "copilot";
/// The `pi` harness name — PDO's third first-party harness (#705, story #702).
pub const PI: &str = "pi";

/// The `claude` build PDO's capabilities were last measured against.
pub const CLAUDE_VALIDATED_VERSION: &str = "2.1.246";
/// The `opencode` build PDO's descriptor was last measured against — the release
/// ADR-0045 records: no launch-time effort axis, no pinnable session identity, and
/// a session store PDO deliberately declares no resolution for.
pub const OPENCODE_VALIDATED_VERSION: &str = "1.18.18";
/// The `copilot` build PDO's capabilities were last measured against — the release
/// #615 measured the `-i` interactive launch, the event journal's `assistant.turn_end`,
/// and the `session.usage_checkpoint` reported cost on.
pub const COPILOT_VALIDATED_VERSION: &str = "1.0.80";
/// The `pi` build PDO's descriptor was last measured against (#705). The grilling
/// of #702 measured 0.84.2; the descriptor was validated on 0.85.1, whose `--help`
/// declares the same launch surface (`-a`, `--session-id`, `--thinking`, `-e`,
/// `--list-models`) — no divergence found between the two on the tokens PDO uses.
pub const PI_VALIDATED_VERSION: &str = "0.85.1";

/// The **last validated version** of `name`'s binary — the release PDO's knowledge
/// of that harness was measured against (#617). `None` for a data-declared harness:
/// PDO carries no code for it, so it has validated nothing.
///
/// This is a **documented bound, not a guard**. PDO does not read the installed
/// version here and never refuses to launch on a different one (#612 put version
/// gating out of scope). It exists because the same harness can be installed twice
/// on one machine, months apart, with different event schemas — and an inventory
/// taken on the wrong install is worse than no inventory. The published support
/// table ([`crate::domain::support`]) names it beside each harness's capabilities so
/// a reader knows *which* binary the row describes.
pub fn validated_version(name: &str) -> Option<&'static str> {
    match name {
        CLAUDE => Some(CLAUDE_VALIDATED_VERSION),
        OPENCODE => Some(OPENCODE_VALIDATED_VERSION),
        COPILOT => Some(COPILOT_VALIDATED_VERSION),
        PI => Some(PI_VALIDATED_VERSION),
        _ => None,
    }
}
