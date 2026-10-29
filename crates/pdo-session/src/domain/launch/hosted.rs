//! The script a Session hébergée runs (ADR-0091): none of a Run's identity,
//! only `PDO_SESSION_ID`, `PDO_DAEMON_URL` and `PDO_LIVE_DIR`, then the
//! extensions' variables, then the agent or the shell.

use std::path::Path;

use super::quote::{sh_quote_arg, sh_single_quote};
use super::tails::{build_agent_tail, build_resume_tail, forced_ccr_env};
use super::wrap::{extension_exports, host_daemon_url};

/// The shell of a Session hébergée: the run shell's respawn loop, so an EOF
/// (a closed terminal) never ends the session; only « terminate » does.
const SHELL_LOOP: &str = "while true; do bash -i; sleep 0.2; done";

/// What a Session hébergée runs after its exports.
pub enum HostedTail<'a> {
    Agent {
        harness: &'a pdo_harness::registry::HarnessDescriptor,
        /// The file the agent reads first (the extensions' text, then the
        /// initial prompt), in the session's state folder; `None` when there
        /// is nothing to read.
        primer: Option<&'a Path>,
        model: Option<&'a str>,
        effort: Option<&'a str>,
        /// The conversation id pinned at launch, for a harness that pins one.
        session_id: Option<&'a str>,
    },
    /// An agent back into the conversation pinned at its last launch.
    Resume {
        harness: &'a pdo_harness::registry::HarnessDescriptor,
        model: Option<&'a str>,
        effort: Option<&'a str>,
        session_id: &'a str,
    },
    Shell,
}

/// The script of the Session hébergée `session`: the session `PATH` (when
/// known), no `PDO_RUN_ID`/`PDO_NODE_ID`/`PDO_NODE_ITER`, `PDO_SESSION_ID`,
/// `PDO_DAEMON_URL`, `PDO_LIVE_DIR` (`live_dir`, in its state folder), the
/// harness's variables (the `claude` CCR suppression for a shell), the
/// extensions' variables (never over one of the core's), then the tail.
/// `tmux_cmd_override` replaces an agent's tail (the per-daemon test seam),
/// never a shell's.
pub fn build_hosted_script(
    session: &str,
    daemon_port: u16,
    session_path: &str,
    live_dir: &Path,
    tail: HostedTail<'_>,
    tmux_cmd_override: Option<&str>,
    extension_env: &[(String, String)],
) -> String {
    let (tail_cmd, harness_env) = match tail {
        HostedTail::Shell => (SHELL_LOOP.to_string(), forced_ccr_env()),
        HostedTail::Agent {
            harness,
            primer,
            model,
            effort,
            session_id,
        } => {
            let cmd = match tmux_cmd_override {
                Some(cmd) => cmd.to_string(),
                None => build_agent_tail(harness, primer, model, effort, None, session_id),
            };
            (cmd, harness.env.clone())
        }
        HostedTail::Resume {
            harness,
            model,
            effort,
            session_id,
        } => {
            let cmd = match tmux_cmd_override {
                Some(cmd) => cmd.to_string(),
                None => build_resume_tail(harness, model, effort, session_id),
            };
            (cmd, harness.env.clone())
        }
    };
    let live_dir = live_dir.to_string_lossy().into_owned();
    let core_env = [("PDO_LIVE_DIR".to_string(), live_dir.clone())];
    let path_export = if session_path.is_empty() {
        String::new()
    } else {
        format!("export PATH={} && ", sh_single_quote(session_path))
    };
    let harness_exports: String = harness_env
        .iter()
        .map(|(k, v)| format!("export {k}={} && ", sh_quote_arg(v)))
        .collect();
    let extension_exports: String = extension_exports(extension_env, &[&harness_env, &core_env])
        .iter()
        .map(|(k, v)| format!("export {k}={} && ", sh_single_quote(v)))
        .collect();
    // Never a Run's identity, even one the daemon inherited (a daemon started
    // from a Node's session): `pdo complete` must not reach that Node.
    let inner = format!(
        "{path_export}unset PDO_RUN_ID PDO_NODE_ID PDO_NODE_ITER && \
         export PDO_SESSION_ID={session_q} && \
         export PDO_DAEMON_URL={daemon_url_q} && \
         export PDO_LIVE_DIR={live_dir_q} && \
         {harness_exports}{extension_exports}{tail_cmd}",
        session_q = sh_single_quote(session),
        daemon_url_q = sh_single_quote(&host_daemon_url(daemon_port)),
        live_dir_q = sh_single_quote(&live_dir),
    );
    format!("exec bash -c {}", sh_single_quote(&inner))
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
    fn an_agent_carries_its_session_and_the_daemon_never_a_run() {
        let claude = pdo_harness::registry::claude();
        let script = build_hosted_script(
            "pdo-hosted-abc",
            6172,
            "/usr/bin:/bin",
            Path::new("/state/live"),
            HostedTail::Agent {
                harness: &claude,
                primer: Some(Path::new("/state/primer.md")),
                model: Some("opus"),
                effort: None,
                session_id: Some("uuid-1"),
            },
            None,
            &env(&[("FIXTURE_HELLO", "monde"), ("PDO_SESSION_ID", "stolen")]),
        );
        assert!(script.contains("PDO_SESSION_ID="), "{script}");
        assert!(script.contains("pdo-hosted-abc"), "{script}");
        assert!(script.contains("http://localhost:6172"), "{script}");
        assert!(
            script.contains("unset PDO_RUN_ID PDO_NODE_ID PDO_NODE_ITER"),
            "{script}"
        );
        assert!(!script.contains("export PDO_RUN_ID"), "{script}");
        assert!(!script.contains("export PDO_NODE_ID"), "{script}");
        assert!(script.contains("FIXTURE_HELLO"), "{script}");
        assert!(
            !script.contains("stolen"),
            "the core's variable wins: {script}"
        );
        assert!(script.contains("/state/primer.md"), "{script}");
        assert!(script.contains("export PDO_LIVE_DIR="), "{script}");
        assert!(script.contains("/state/live"), "{script}");
        assert!(script.contains("--model"), "{script}");
        assert!(script.contains("uuid-1"), "{script}");
        assert!(script.contains("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC"));
    }

    #[test]
    fn an_agent_without_anything_to_read_gets_no_prompt() {
        let claude = pdo_harness::registry::claude();
        let script = build_hosted_script(
            "pdo-hosted-abc",
            6172,
            "",
            Path::new("/state/live"),
            HostedTail::Agent {
                harness: &claude,
                primer: None,
                model: None,
                effort: None,
                session_id: None,
            },
            None,
            &[],
        );
        assert!(!script.contains("cat "), "{script}");
        assert!(!script.contains("export PATH"), "{script}");
    }

    #[test]
    fn a_shell_ignores_the_test_seam_and_respawns() {
        let script = build_hosted_script(
            "pdo-hosted-sh",
            6172,
            "",
            Path::new("/state/live"),
            HostedTail::Shell,
            Some("exec sleep 600"),
            &env(&[("PDO_TOKEN", "t")]),
        );
        assert!(script.contains("bash -i"), "{script}");
        assert!(!script.contains("sleep 600"), "{script}");
        assert!(script.contains("PDO_TOKEN"), "{script}");
        assert!(script.contains("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC"));
    }

    #[test]
    fn a_resumed_agent_re_enters_its_conversation_by_identity_with_its_model_and_effort() {
        let claude = pdo_harness::registry::claude();
        let script = build_hosted_script(
            "pdo-hosted-r",
            6172,
            "",
            Path::new("/state/live"),
            HostedTail::Resume {
                harness: &claude,
                model: Some("opus"),
                effort: Some("high"),
                session_id: "uuid-1",
            },
            None,
            &env(&[("PDO_LIVE_DIR", "/elsewhere")]),
        );
        assert!(script.contains("--resume"), "{script}");
        assert!(script.contains("uuid-1"), "{script}");
        assert!(!script.contains("--continue"), "{script}");
        assert!(!script.contains("--session-id"), "{script}");
        assert!(script.contains("--model"), "{script}");
        assert!(script.contains("--effort"), "{script}");
        assert!(script.contains("PDO_SESSION_ID="), "{script}");
        assert!(
            !script.contains("/elsewhere"),
            "the core's live folder wins: {script}"
        );
    }

    #[test]
    fn an_agent_tail_is_replaced_by_the_test_seam() {
        let claude = pdo_harness::registry::claude();
        let script = build_hosted_script(
            "pdo-hosted-a",
            6172,
            "",
            Path::new("/state/live"),
            HostedTail::Agent {
                harness: &claude,
                primer: None,
                model: None,
                effort: None,
                session_id: None,
            },
            Some("exec sleep 600"),
            &[],
        );
        assert!(script.contains("exec sleep 600"), "{script}");
        assert!(!script.contains("claude"), "{script}");
    }
}
