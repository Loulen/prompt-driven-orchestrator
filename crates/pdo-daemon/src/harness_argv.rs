//! Pure argv-template renderer for agentic harnesses (ADR-0045).
//!
//! A harness declares its launch and resume tails as a **template of argv
//! tokens**. This module turns such a template plus a set of hole values into the
//! single tail string a tmux session runs — with ONE rule: **a token that
//! references a hole whose value is empty is dropped in its entirety**. That rule
//! is the whole reason the `claude` tail stays byte-identical to the legacy launch
//! when no model / effort / settings / session identity is posed (the #550 gate).
//!
//! **Pure by contract — an AC, not advice.** No `$HOME`, no disk, no clock. The
//! caller shell-quotes each hole value before handing it in (`'opus'`, or
//! `"$(cat '/path')"` for the prompt), so the shell-quoting discipline stays in
//! [`crate::tmux_session_manager`] where the shell semantics already live. This
//! module only substitutes placeholders and joins the survivors, which is exactly
//! what makes it testable without a fixture.

/// The values substituted into a descriptor's argv holes.
///
/// Each field is **already shell-quoted by the caller**. An EMPTY string means
/// "no value": every token that references that hole is dropped, so the rendered
/// tail collapses to exactly what it would be if the hole did not appear in the
/// template at all.
///
/// `prompt`, `model`, `effort`, `session_id` and `settings` are the five holes of
/// ADR-0045. `resume` is a sixth, and it is deliberately NOT one of them: it is
/// the resume *selector* (`--resume '<id>'` by identity, or a blind `--continue`),
/// which ADR-0045 keeps in **code** because the identity-or-blind choice reads the
/// node's event-log row, not the harness. The pure hole-drop rule cannot express
/// "emit X *when* the hole is empty", so the selector is computed by the caller
/// and handed in here as one opaque, never-empty value.
#[derive(Debug, Default, Clone)]
pub(crate) struct Holes {
    pub prompt: String,
    pub model: String,
    pub effort: String,
    pub session_id: String,
    pub settings: String,
    pub resume: String,
}

impl Holes {
    /// Resolve a placeholder name to its (already-quoted) value.
    ///
    /// An unknown placeholder resolves to the empty string, so its token drops —
    /// the same "absent" behaviour as an empty known hole. Every embedded
    /// descriptor is pinned by a byte-for-byte golden, which turns a typo'd
    /// placeholder into a red test rather than a silent drop.
    fn resolve(&self, name: &str) -> &str {
        match name {
            "prompt" => &self.prompt,
            "model" => &self.model,
            "effort" => &self.effort,
            "session_id" => &self.session_id,
            "settings" => &self.settings,
            "resume" => &self.resume,
            _ => "",
        }
    }
}

/// Render a descriptor's argv-token template into a single tail string.
///
/// Each token may contain zero or more `{hole}` placeholders. A token is dropped
/// **entirely** if any placeholder it contains resolves to an empty value;
/// otherwise every placeholder is substituted and the token is kept. The
/// survivors are joined by a single space — so an absent optional flag leaves no
/// double space and no trailing space, which is precisely what keeps the no-op
/// `claude` tail byte-identical to the hand-written legacy literal.
pub(crate) fn render(tokens: &[String], holes: &Holes) -> String {
    tokens
        .iter()
        .filter_map(|tok| render_token(tok, holes))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Render one token, or `None` when it references an empty hole (⇒ the token is
/// dropped by [`render`]).
fn render_token(token: &str, holes: &Holes) -> Option<String> {
    let mut out = String::with_capacity(token.len());
    let mut rest = token;
    loop {
        let open = match rest.find('{') {
            None => {
                out.push_str(rest);
                return Some(out);
            }
            Some(i) => i,
        };
        // A `{` with no closing `}` is literal text, not a placeholder — keep it
        // and stop scanning (none of our descriptors do this, but be lenient).
        let rel_close = match rest[open + 1..].find('}') {
            None => {
                out.push_str(rest);
                return Some(out);
            }
            Some(i) => i,
        };
        let close = open + 1 + rel_close;
        out.push_str(&rest[..open]);
        let value = holes.resolve(&rest[open + 1..close]);
        if value.is_empty() {
            return None; // one empty hole drops the whole token
        }
        out.push_str(value);
        rest = &rest[close + 1..];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness_registry;

    /// The prompt hole as the caller builds it: `"$(cat '<path>')"`.
    fn prompt_hole(path: &str) -> String {
        format!("\"$(cat '{path}')\"")
    }

    // -----------------------------------------------------------------------
    // THE GATE (#550): the `claude` launch/resume tails, byte for byte.
    //
    // These goldens were captured from the pre-refactor `build_agent_tail` /
    // `build_resume_script` literals (single space before the prompt cat, leading
    // space on `--effort` etc.) and then the tail builders were rewritten to route
    // through this renderer. A golden written *after* the refactor would prove
    // nothing — so it is the literal here that is authoritative.
    // -----------------------------------------------------------------------

    #[test]
    fn claude_launch_no_holes_is_byte_identical_to_the_legacy_tail() {
        let d = harness_registry::resolve("claude").unwrap();
        let holes = Holes {
            prompt: prompt_hole("/tmp/test-prompt.md"),
            ..Default::default()
        };
        assert_eq!(
            render(&d.launch, &holes),
            "exec claude --dangerously-skip-permissions \"$(cat '/tmp/test-prompt.md')\""
        );
    }

    #[test]
    fn claude_resume_no_holes_is_byte_identical_to_the_legacy_tail() {
        let d = harness_registry::resolve("claude").unwrap();
        // The blind-continue selector, as the resume seam computes it when the row
        // carries no pinned id (pre-#473 / opencode).
        let holes = Holes {
            resume: "--continue".to_string(),
            ..Default::default()
        };
        assert_eq!(
            render(&d.resume, &holes),
            "exec claude --dangerously-skip-permissions --continue"
        );
    }

    #[test]
    fn claude_launch_fills_every_hole_in_order() {
        let d = harness_registry::resolve("claude").unwrap();
        let holes = Holes {
            prompt: prompt_hole("/tmp/p.md"),
            model: "'opus'".to_string(),
            effort: "'low'".to_string(),
            settings: "'/tmp/s.json'".to_string(),
            session_id: "'abc-123'".to_string(),
            ..Default::default()
        };
        assert_eq!(
            render(&d.launch, &holes),
            "exec claude --dangerously-skip-permissions --model 'opus' --effort 'low' \
             --settings '/tmp/s.json' --session-id 'abc-123' \"$(cat '/tmp/p.md')\""
        );
    }

    #[test]
    fn claude_launch_model_only_hugs_the_base_flag() {
        // #296/#424: an absent optional flag leaves no double space and no gap.
        let d = harness_registry::resolve("claude").unwrap();
        let holes = Holes {
            prompt: prompt_hole("/tmp/p.md"),
            model: "'opus'".to_string(),
            ..Default::default()
        };
        assert_eq!(
            render(&d.launch, &holes),
            "exec claude --dangerously-skip-permissions --model 'opus' \"$(cat '/tmp/p.md')\""
        );
    }

    #[test]
    fn claude_launch_effort_only_leaves_no_gap_where_the_model_would_be() {
        let d = harness_registry::resolve("claude").unwrap();
        let holes = Holes {
            prompt: prompt_hole("/tmp/p.md"),
            effort: "'xhigh'".to_string(),
            ..Default::default()
        };
        assert_eq!(
            render(&d.launch, &holes),
            "exec claude --dangerously-skip-permissions --effort 'xhigh' \"$(cat '/tmp/p.md')\""
        );
    }

    #[test]
    fn claude_resume_by_identity() {
        let d = harness_registry::resolve("claude").unwrap();
        let holes = Holes {
            resume: "--resume 'abc-123'".to_string(),
            ..Default::default()
        };
        assert_eq!(
            render(&d.resume, &holes),
            "exec claude --dangerously-skip-permissions --resume 'abc-123'"
        );
    }

    #[test]
    fn claude_resume_carries_effort_and_settings() {
        let d = harness_registry::resolve("claude").unwrap();
        let holes = Holes {
            resume: "--continue".to_string(),
            effort: "'low'".to_string(),
            settings: "'/tmp/s.json'".to_string(),
            ..Default::default()
        };
        assert_eq!(
            render(&d.resume, &holes),
            "exec claude --dangerously-skip-permissions --continue --effort 'low' \
             --settings '/tmp/s.json'"
        );
    }

    // -----------------------------------------------------------------------
    // #614: the `copilot` launch/resume tails, byte for byte — every hole full
    // and every hole empty (AC "tous trous pleins et tous trous vides"). The tail
    // is what a resident tmux pane runs; goldens pin it so a flag typo is a red
    // test, never a broken launch discovered only against a live binary.
    // -----------------------------------------------------------------------

    #[test]
    fn copilot_launch_no_holes_uses_the_automatic_selector_and_no_session_pin() {
        // A node with no model / session id: `--model` and `--session-id` drop, so
        // copilot launches on its automatic model selector (AC), fully autonomous,
        // question tool off, resident after the turn.
        let d = harness_registry::resolve("copilot").unwrap();
        let holes = Holes {
            prompt: prompt_hole("/tmp/p.md"),
            ..Default::default()
        };
        assert_eq!(
            render(&d.launch, &holes),
            "exec copilot --allow-all --no-ask-user \"$(cat '/tmp/p.md')\""
        );
    }

    #[test]
    fn copilot_launch_fills_every_hole() {
        // Every hole present: model pins the selector, session id pins identity.
        // `--effort` has no place in copilot's template, so an effort value never
        // leaks a token (copilot has no launch-time effort axis).
        let d = harness_registry::resolve("copilot").unwrap();
        let holes = Holes {
            prompt: prompt_hole("/tmp/p.md"),
            model: "'gpt-5.2'".to_string(),
            session_id: "'abc-123'".to_string(),
            effort: "'high'".to_string(),
            ..Default::default()
        };
        assert_eq!(
            render(&d.launch, &holes),
            "exec copilot --allow-all --no-ask-user --model 'gpt-5.2' --session-id 'abc-123' \
             \"$(cat '/tmp/p.md')\""
        );
    }

    #[test]
    fn copilot_launch_model_only_still_pins_no_session() {
        let d = harness_registry::resolve("copilot").unwrap();
        let holes = Holes {
            prompt: prompt_hole("/tmp/p.md"),
            model: "'gpt-5.2'".to_string(),
            ..Default::default()
        };
        assert_eq!(
            render(&d.launch, &holes),
            "exec copilot --allow-all --no-ask-user --model 'gpt-5.2' \"$(cat '/tmp/p.md')\""
        );
    }

    #[test]
    fn copilot_resume_by_identity() {
        // The resume seam fills `{resume}` with `--resume '<id>'` (its verb is the
        // descriptor's `resume_by_id`), so copilot re-enters THIS node's session.
        let d = harness_registry::resolve("copilot").unwrap();
        let holes = Holes {
            resume: "--resume 'abc-123'".to_string(),
            ..Default::default()
        };
        assert_eq!(
            render(&d.resume, &holes),
            "exec copilot --allow-all --no-ask-user --resume 'abc-123'"
        );
    }

    #[test]
    fn copilot_resume_with_no_identity_renders_no_resume_flag() {
        // copilot's `resume_blind` is empty, so a resume with no recorded id leaves
        // the `{resume}` hole empty and the token drops — never a blind `--continue`
        // (AC "jamais par un continue aveugle").
        let d = harness_registry::resolve("copilot").unwrap();
        assert_eq!(
            render(&d.resume, &Holes::default()),
            "exec copilot --allow-all --no-ask-user"
        );
    }

    #[test]
    fn opencode_resume_stays_a_blind_continue_byte_for_byte() {
        // The #614 verb-as-property refactor keeps opencode's resume byte-identical:
        // `resume_blind` fills `{resume}` with `--continue` (opencode cannot pin an
        // identity, so it only ever blind-continues).
        let d = harness_registry::resolve("opencode").unwrap();
        let holes = Holes {
            resume: "--continue".to_string(),
            ..Default::default()
        };
        assert_eq!(render(&d.resume, &holes), "exec opencode --auto --continue");
    }

    // -----------------------------------------------------------------------
    // The rendering rule itself, independent of any descriptor.
    // -----------------------------------------------------------------------

    #[test]
    fn a_token_with_one_empty_hole_is_dropped_whole() {
        let tokens = vec!["--model {model}".to_string()];
        assert_eq!(render(&tokens, &Holes::default()), "");
    }

    #[test]
    fn a_token_with_a_filled_hole_survives() {
        let tokens = vec!["--model {model}".to_string()];
        let holes = Holes {
            model: "'opus'".to_string(),
            ..Default::default()
        };
        assert_eq!(render(&tokens, &holes), "--model 'opus'");
    }

    #[test]
    fn a_literal_token_with_no_hole_always_survives() {
        let tokens = vec!["--auto".to_string()];
        assert_eq!(render(&tokens, &Holes::default()), "--auto");
    }

    #[test]
    fn survivors_are_joined_by_a_single_space_only() {
        let tokens = vec![
            "a".to_string(),
            "--x {model}".to_string(), // dropped
            "b".to_string(),
        ];
        assert_eq!(render(&tokens, &Holes::default()), "a b");
    }

    #[test]
    fn an_unknown_placeholder_drops_its_token() {
        let tokens = vec!["--x {nope}".to_string(), "keep".to_string()];
        assert_eq!(render(&tokens, &Holes::default()), "keep");
    }
}
