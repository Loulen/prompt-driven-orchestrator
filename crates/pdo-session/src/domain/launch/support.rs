//! The script an Agent de support runs: none of a Run's identity, only
//! `PDO_SESSION_ID` and `PDO_DAEMON_URL`, then the harness's and the
//! extensions' variables (the token of its Actor among them), then the agent.

use std::path::Path;

use super::quote::{sh_quote_arg, sh_single_quote};
use super::tails::build_agent_tail;
use super::wrap::{extension_exports, host_daemon_url};

/// What the agent of an Agent de support launches with.
pub struct SupportTail<'a> {
    pub harness: &'a pdo_harness::registry::HarnessDescriptor,
    /// The file holding its first message (the person's first question);
    /// `None`: the agent opens on an empty input.
    pub first_message: Option<&'a Path>,
    pub model: Option<&'a str>,
    pub effort: Option<&'a str>,
}

/// The script of the Agent de support `session`: the session `PATH` (when
/// known), no `PDO_RUN_ID`/`PDO_NODE_ID`/`PDO_NODE_ITER` (even ones the daemon
/// inherited: `pdo complete` must reach no Node), `PDO_SESSION_ID`,
/// `PDO_DAEMON_URL`, the harness's variables, the extensions' variables (never
/// over one of the core's), then the agent. `tmux_cmd_override` replaces the
/// agent (the per-daemon test seam).
pub fn build_support_script(
    session: &str,
    daemon_port: u16,
    session_path: &str,
    tail: SupportTail<'_>,
    tmux_cmd_override: Option<&str>,
    extension_env: &[(String, String)],
) -> String {
    let tail_cmd = match tmux_cmd_override {
        Some(cmd) => cmd.to_string(),
        None => build_agent_tail(
            tail.harness,
            tail.first_message,
            tail.model,
            tail.effort,
            None,
            None,
        ),
    };
    let harness_env = &tail.harness.env;
    let path_export = if session_path.is_empty() {
        String::new()
    } else {
        format!("export PATH={} && ", sh_single_quote(session_path))
    };
    let harness_exports: String = harness_env
        .iter()
        .map(|(k, v)| format!("export {k}={} && ", sh_quote_arg(v)))
        .collect();
    let extension_exports: String = extension_exports(extension_env, &[harness_env])
        .iter()
        .map(|(k, v)| format!("export {k}={} && ", sh_single_quote(v)))
        .collect();
    format!(
        "{path_export}unset PDO_RUN_ID PDO_NODE_ID PDO_NODE_ITER && \
         export PDO_SESSION_ID={session_q} && \
         export PDO_DAEMON_URL={daemon_url_q} && \
         {harness_exports}{extension_exports}{tail_cmd}",
        session_q = sh_single_quote(session),
        daemon_url_q = sh_single_quote(&host_daemon_url(daemon_port)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn the_agent_carries_its_session_its_actor_token_and_never_a_run() {
        let claude = pdo_harness::registry::claude();
        let script = build_support_script(
            "pdo-support-operator",
            6172,
            "/usr/bin:/bin",
            SupportTail {
                harness: &claude,
                first_message: Some(Path::new("/state/question.md")),
                model: Some("opus"),
                effort: Some("high"),
            },
            None,
            &env(&[("PDO_TOKEN", "t"), ("PDO_SESSION_ID", "stolen")]),
        );
        assert!(
            script.starts_with("export PATH='/usr/bin:/bin' && "),
            "{script}"
        );
        assert!(script.contains("unset PDO_RUN_ID PDO_NODE_ID PDO_NODE_ITER"));
        assert!(script.contains("export PDO_SESSION_ID='pdo-support-operator'"));
        assert!(script.contains("export PDO_DAEMON_URL='http://localhost:6172'"));
        assert!(script.contains("export PDO_TOKEN='t'"), "{script}");
        assert!(
            !script.contains("stolen"),
            "a core variable is never overwritten"
        );
        assert!(script.contains("--model 'opus'"), "{script}");
        assert!(script.contains("--effort 'high'"), "{script}");
        assert!(
            script.ends_with("\"$(cat '/state/question.md')\""),
            "the question is the first message: {script}"
        );
    }

    #[test]
    fn without_a_question_the_agent_opens_on_an_empty_input() {
        let claude = pdo_harness::registry::claude();
        let script = build_support_script(
            "pdo-support-operator",
            6172,
            "",
            SupportTail {
                harness: &claude,
                first_message: None,
                model: None,
                effort: None,
            },
            None,
            &[],
        );
        assert!(!script.contains("$(cat"), "{script}");
        assert!(
            script.ends_with("--dangerously-skip-permissions"),
            "{script}"
        );
    }

    #[test]
    fn the_test_seam_replaces_the_agent() {
        let claude = pdo_harness::registry::claude();
        let script = build_support_script(
            "pdo-support-operator",
            6172,
            "",
            SupportTail {
                harness: &claude,
                first_message: None,
                model: None,
                effort: None,
            },
            Some("exec sleep 600"),
            &[],
        );
        assert!(script.ends_with("exec sleep 600"), "{script}");
        assert!(
            !script.contains("export PATH="),
            "no PATH known, none exported"
        );
    }
}
