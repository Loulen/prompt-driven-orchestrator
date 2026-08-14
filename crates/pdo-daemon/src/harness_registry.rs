//! Resolve an agentic-harness name to its descriptor (ADR-0045).
//!
//! A **harness** is the program that runs a NodeRun's agent (`claude`,
//! `opencode`, …). It declares itself with two argv-token templates (launch,
//! resume), an env block, and the binary PDO probes at spawn — never with named
//! per-feature fields (a named field per case becomes a spelling per case; the
//! template covers them without naming, ADR-0045).
//!
//! **This slice is the embedded floor only.** `claude` and `opencode` are compiled
//! in; nothing is seeded on disk and this module never reads `$HOME` (the
//! discipline `run_cost` paid for in #408 — a root goes in, a descriptor comes
//! out). A user-declared *disk tier* that merges over the floor **by name** is
//! #553; [`merge_by_name`] is that seam, present and tested now so the disk tier
//! layers on without rewriting [`resolve`]'s callers — but no caller passes a disk
//! tier in this slice, so the floor is the whole registry.

/// A harness, as PDO launches / resumes / attaches it (ADR-0045).
///
/// The two templates are rendered by [`crate::harness_argv`]; the caller fills
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

    /// Whether the LAUNCH template pins a session identity (`{session_id}`).
    ///
    /// `opencode` cannot (measured: launching with a fresh id answers "Session not
    /// found" and exits 1 — its selector *continues* a session, never creates
    /// one), so PDO never pins one for it and attributes by working dir alone.
    pub fn pins_session_id(&self) -> bool {
        self.launch.iter().any(|t| t.contains("{session_id}"))
    }
}

/// The `claude` harness name — the floor of the precedence chain (ADR-0046).
pub const CLAUDE: &str = "claude";
/// The `opencode` harness name (ADR-0045, measured on 1.18.18).
pub const OPENCODE: &str = "opencode";

/// The `claude` descriptor: the legacy launch, expressed as data.
///
/// The launch template reproduces the pre-#550 `build_agent_tail` **byte for
/// byte** once its holes are empty — that is the #550 gate, pinned by the goldens
/// in [`crate::harness_argv`]. The resume template reproduces the pre-#550
/// `build_resume_script` tail; its `{resume}` hole is the identity-or-blind
/// selector the resume seam computes (ADR-0045 keeps that choice in code).
pub fn claude() -> HarnessDescriptor {
    HarnessDescriptor {
        name: CLAUDE.to_string(),
        binary: "claude".to_string(),
        launch: [
            "exec",
            "claude",
            "--dangerously-skip-permissions",
            "--model {model}",
            "--effort {effort}",
            "--settings {settings}",
            "--session-id {session_id}",
            "{prompt}",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect(),
        resume: [
            "exec",
            "claude",
            "--dangerously-skip-permissions",
            "{resume}",
            "--effort {effort}",
            "--settings {settings}",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect(),
        // AC #4: the CCR suppression now comes from the descriptor for an agent
        // tail (byte-identical to the wrapper's old hard-coded export — see the
        // `harness_env` handling in `tmux_session_manager::wrap_with_env`).
        env: vec![(
            "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC".to_string(),
            "1".to_string(),
        )],
    }
}

/// The `opencode` descriptor (measured on 1.18.18).
///
/// `--auto` is its `--dangerously-skip-permissions`; the prompt is a `--prompt`
/// argument; the model is `--model provider/model`. There is **no** effort hole
/// (no launch-time effort axis) and **no** session-id hole (identity can't be
/// pinned), so the effort picker greys and attribution falls back to the working
/// dir. Resume is a blind `--continue` (opencode is resident and continues the
/// cwd's latest session); it carries no env block.
pub fn opencode() -> HarnessDescriptor {
    HarnessDescriptor {
        name: OPENCODE.to_string(),
        binary: "opencode".to_string(),
        launch: [
            "exec",
            "opencode",
            "--auto",
            "--model {model}",
            "--prompt {prompt}",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect(),
        resume: ["exec", "opencode", "--auto", "--continue"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        env: vec![],
    }
}

/// The embedded floor: the harnesses PDO ships compiled in, in precedence-neutral
/// declaration order.
pub fn embedded_floor() -> Vec<HarnessDescriptor> {
    vec![claude(), opencode()]
}

/// Merge a user-declared disk tier over the embedded floor, **by name**: a disk
/// descriptor replaces the floor's entry of the same name; a floor name absent
/// from disk survives. Pure — the caller (a future slice, #553) reads and parses
/// the disk tier and hands the descriptors in. No caller passes a disk tier in
/// this slice, so `merge_by_name(embedded_floor(), vec![])` is the whole registry.
pub fn merge_by_name(
    floor: Vec<HarnessDescriptor>,
    disk: Vec<HarnessDescriptor>,
) -> Vec<HarnessDescriptor> {
    let mut merged = floor;
    for d in disk {
        match merged.iter_mut().find(|f| f.name == d.name) {
            Some(slot) => *slot = d,
            None => merged.push(d),
        }
    }
    merged
}

/// Resolve a harness name to its descriptor. `None` ⇒ no harness carries that
/// name (an unknown harness — the spawn seam turns this into a fail-fast that
/// names it, never a silent fallback).
///
/// The disk tier (#553) layers on by making this `merge_by_name(embedded_floor(),
/// disk).into_iter().find(...)`; callers don't change.
pub fn resolve(name: &str) -> Option<HarnessDescriptor> {
    embedded_floor().into_iter().find(|d| d.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floor_carries_claude_and_opencode() {
        assert!(resolve(CLAUDE).is_some());
        assert!(resolve(OPENCODE).is_some());
        assert!(resolve("nope").is_none());
    }

    #[test]
    fn claude_pins_identity_has_effort_and_can_resume() {
        let d = claude();
        assert!(d.pins_session_id(), "claude pins --session-id");
        assert!(d.has_effort_hole(), "claude has an effort axis");
        assert!(d.can_resume(), "claude resumes by --resume/--continue");
    }

    #[test]
    fn opencode_has_no_effort_axis_and_no_identity_pin() {
        let d = opencode();
        assert!(
            !d.has_effort_hole(),
            "opencode has no launch-time effort axis (greys the picker)"
        );
        assert!(
            !d.pins_session_id(),
            "opencode cannot pin a session identity"
        );
        assert!(d.can_resume(), "opencode blind-continues");
        assert!(d.env.is_empty(), "opencode carries no CCR env");
    }

    #[test]
    fn merge_by_name_replaces_a_floor_entry_and_keeps_the_rest() {
        let custom_claude = HarnessDescriptor {
            name: CLAUDE.to_string(),
            binary: "my-claude".to_string(),
            launch: vec!["exec".to_string(), "my-claude".to_string()],
            resume: vec![],
            env: vec![],
        };
        let merged = merge_by_name(embedded_floor(), vec![custom_claude.clone()]);
        // claude is replaced by the disk entry…
        let c = merged.iter().find(|d| d.name == CLAUDE).unwrap();
        assert_eq!(c.binary, "my-claude");
        // …and opencode, absent from the disk tier, survives from the floor.
        assert!(merged.iter().any(|d| d.name == OPENCODE));
    }

    #[test]
    fn merge_by_name_appends_an_unknown_disk_harness() {
        let novel = HarnessDescriptor {
            name: "novel".to_string(),
            binary: "novel".to_string(),
            launch: vec!["exec".to_string(), "novel".to_string()],
            resume: vec![],
            env: vec![],
        };
        let merged = merge_by_name(embedded_floor(), vec![novel]);
        assert!(merged.iter().any(|d| d.name == "novel"));
        assert_eq!(merged.len(), 3);
    }
}
