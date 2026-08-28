//! Pure argv-template renderer for agentic harnesses (ADR-0045).
//!
//! A harness declares its launch and resume tails as a **template of argv
//! tokens**. This module turns such a template plus a set of hole values into the
//! single tail string a tmux session runs — with ONE rule: **a token that
//! references a hole whose value is empty is dropped in its entirety**. That rule
//! is the whole reason the `claude` tail stays byte-identical to the legacy launch
//! when no model / effort / settings / session identity is posed (the #550 gate).
//!
//! **Pure by contract — an AC, not advice.** No `$HOME`, no disk, no clock. Don't
//! shell-quote here: the caller hands in already-quoted values (`'opus'`, or
//! `"$(cat '/path')"` for the prompt) so the quoting discipline stays in
//! [`crate::tmux_session_manager`], where the shell semantics live.

/// The values substituted into a descriptor's argv holes.
///
/// Each field is **already shell-quoted by the caller**. An EMPTY string means
/// "no value": every token referencing that hole is dropped.
///
/// `resume` is deliberately NOT one of ADR-0045's five holes: it is the resume
/// *selector* (`--resume '<id>'` by identity, or a blind `--continue`), and the
/// identity-or-blind choice reads the node's event-log row, not the harness. The
/// hole-drop rule cannot express "emit X *when* the hole is empty", so the caller
/// computes the selector and hands it in as one opaque, never-empty value.
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
    /// An unknown placeholder resolves to the empty string, so its token drops —
    /// the same "absent" behaviour as an empty known hole. A typo'd placeholder is
    /// caught by the byte-for-byte descriptor goldens, not here.
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
/// A token is dropped **entirely** if any `{hole}` it contains resolves to empty.
/// Survivors join on a single space: an absent optional flag must leave no double
/// space and no trailing space, or the no-op `claude` tail stops being
/// byte-identical to the legacy literal.
pub(crate) fn render(tokens: &[String], holes: &Holes) -> String {
    tokens
        .iter()
        .filter_map(|tok| render_token(tok, holes))
        .collect::<Vec<_>>()
        .join(" ")
}

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
        // Don't treat an unclosed `{` as a placeholder: it is literal text.
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
            return None;
        }
        out.push_str(value);
        rest = &rest[close + 1..];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness_registry;

    fn prompt_hole(path: &str) -> String {
        format!("\"$(cat '{path}')\"")
    }

    // THE GATE (#550). These goldens were captured from the PRE-refactor
    // `build_agent_tail` / `build_resume_script` literals; the builders were then
    // rewritten to route through this renderer. Don't regenerate them from current
    // output — a golden written after the refactor would prove nothing.

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

    // #614/#615. Don't launch copilot with a positional prompt: that slot is for
    // subcommands, copilot refuses it, and the node hangs. `-i {prompt}` enters
    // interactive mode with the prompt auto-executed, so the harness stays resident.

    #[test]
    fn copilot_launch_no_holes_uses_the_automatic_selector_and_no_session_pin() {
        let d = harness_registry::resolve("copilot").unwrap();
        let holes = Holes {
            prompt: prompt_hole("/tmp/p.md"),
            ..Default::default()
        };
        assert_eq!(
            render(&d.launch, &holes),
            "exec copilot --allow-all --no-ask-user -i \"$(cat '/tmp/p.md')\""
        );
    }

    #[test]
    fn copilot_launch_fills_every_hole() {
        // Copilot has no launch-time effort axis: an effort value must leak no token.
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
             -i \"$(cat '/tmp/p.md')\""
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
            "exec copilot --allow-all --no-ask-user --model 'gpt-5.2' -i \"$(cat '/tmp/p.md')\""
        );
    }

    #[test]
    fn copilot_resume_by_identity() {
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
        // copilot's `resume_blind` is empty on purpose: never a blind `--continue`.
        let d = harness_registry::resolve("copilot").unwrap();
        assert_eq!(
            render(&d.resume, &Holes::default()),
            "exec copilot --allow-all --no-ask-user"
        );
    }

    #[test]
    fn opencode_resume_stays_a_blind_continue_byte_for_byte() {
        // opencode cannot pin a session identity, so it only ever blind-continues.
        let d = harness_registry::resolve("opencode").unwrap();
        let holes = Holes {
            resume: "--continue".to_string(),
            ..Default::default()
        };
        assert_eq!(render(&d.resume, &holes), "exec opencode --auto --continue");
    }

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
