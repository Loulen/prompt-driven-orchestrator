//! The names of PDO's tmux sessions: how each kind is spelled, and how a name
//! is read back into its kind (the orphan sweep judges a session by its name).

use std::path::{Path, PathBuf};

/// Compute the per-daemon tmux socket name (`tmux -L <name>`) for a daemon
/// listening on `daemon_port`.
///
/// Each daemon scopes its tmux state to a private socket so that orphan
/// sweeps and `list` calls only see *its own* sessions. Two daemons running
/// on different ports therefore can't observe — or kill — each other's
/// sessions, even when both run as the same user on the same host.
///
/// This eliminates the failure mode where a sub-claude transitively spawns
/// its own `pdo daemon` (e.g. for an end-to-end test from a Tester
/// node): the new daemon's boot-time orphan sweep runs against an empty
/// event log and would otherwise call `tmux kill-session` on every
/// `pdo-*` session it finds on the system-default socket — collapsing
/// the parent daemon's running pipelines.
pub fn tmux_socket_name(daemon_port: u16) -> String {
    format!("pdo-{daemon_port}")
}

/// Session naming convention for NodeRuns.
pub fn node_session_name(run_id: &str, node_id: &str, iter: i64) -> String {
    format!("pdo-{run_id}-{node_id}-iter-{iter}")
}

/// Session naming convention for the Pipeline Manager.
pub fn manager_session_name(run_id: &str) -> String {
    format!("pdo-mgr-{run_id}")
}

/// Session naming convention for an ad-hoc run shell (#316, ADR-0021).
///
/// One fixed name per Run so `POST /sessions/{run_id}/shell` is create-if-absent
/// (a second click re-attaches the same session). Parsed back out by
/// [`parse_session_name`] via the `shell-` prefix branch, mirroring `mgr-`.
pub fn shell_session_name(run_id: &str) -> String {
    format!("pdo-shell-{run_id}")
}

/// The one library authoring assistant session, for the whole daemon (#594,
/// ADR-0051 — it replaces the per-pipeline name of #302 / ADR-0048).
///
/// **The `-shared` suffix is load-bearing, not decoration.** [`parse_session_name`]
/// strips the `libassist-` prefix and rejects an *empty* remainder, so a bare
/// `pdo-libassist` would not parse — and an unparseable `pdo-*` name is killed by
/// the orphan sweep as `UnrecognisedName` within one sweep interval. Which template
/// is open travels in the daemon's *focus* (`PUT /sessions/libassist/focus`), never
/// in the session name.
pub const LIBASSIST_SESSION_NAME: &str = "pdo-libassist-shared";

/// Session naming convention for the library pipeline authoring assistant.
///
/// One name for the whole daemon, so `POST /sessions/libassist` is
/// create-if-absent (a second open re-attaches the same session, with its whole
/// conversation). See [`LIBASSIST_SESSION_NAME`] for why the name is not bare.
pub fn libassist_session_name() -> &'static str {
    LIBASSIST_SESSION_NAME
}

/// Parsed components of a `pdo-*` session name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedSession {
    NodeRun {
        run_id: String,
        node_id: String,
        iter: i64,
    },
    Manager {
        run_id: String,
    },
    /// Ad-hoc run shell `pdo-shell-<run_id>` (#316 / ADR-0021).
    Shell {
        run_id: String,
    },
    /// The library pipeline authoring assistant [`LIBASSIST_SESSION_NAME`]
    /// (#302 / ADR-0048, made a singleton by #594 / ADR-0051).
    ///
    /// A **unit** variant: one assistant per daemon. Which template is being
    /// edited is the daemon's *focus*, not the session's identity — putting it
    /// back in the name would re-create the per-pipeline session it replaced.
    /// Owns no Run; reaped by the explicit `DELETE` on leaving every edit view,
    /// with the sweep's idle arm as the backstop.
    LibAssist,
}

/// Parse a session name like `pdo-<run_id>-<node_id>-iter-<N>` or
/// `pdo-mgr-<run_id>`. Returns `None` for unrecognised formats.
pub fn parse_session_name(name: &str) -> Option<ParsedSession> {
    let rest = name.strip_prefix("pdo-")?;

    if let Some(run_id) = rest.strip_prefix("mgr-") {
        if !run_id.is_empty() {
            return Some(ParsedSession::Manager {
                run_id: run_id.to_string(),
            });
        }
        return None;
    }

    // #316: `pdo-shell-<run_id>` — parsed BEFORE the `-iter-` split (a shell name
    // has no `-iter-` suffix, so it would otherwise return None and be killed as
    // "unrecognised" by the orphan sweep).
    if let Some(run_id) = rest.strip_prefix("shell-") {
        if !run_id.is_empty() {
            return Some(ParsedSession::Shell {
                run_id: run_id.to_string(),
            });
        }
        return None;
    }

    // #302: the library assistant — parsed BEFORE the `-iter-` split, like
    // `shell-` / `mgr-`, for the same reason.
    //
    // The suffix is no longer read (#594: one shared session), but it must still be
    // **non-empty** — that rejection is why the singleton is named
    // `pdo-libassist-shared` and not `pdo-libassist`.
    if let Some(suffix) = rest.strip_prefix("libassist-") {
        if !suffix.is_empty() {
            return Some(ParsedSession::LibAssist);
        }
        return None;
    }

    // run_id contains dashes (e.g. 20260506-143000-a3f1b2c), so we split on
    // the last "-iter-" to isolate the iter suffix first.
    let iter_sep = rest.rfind("-iter-")?;
    let before_iter = &rest[..iter_sep];
    let iter_str = &rest[iter_sep + 6..];
    let iter: i64 = iter_str.parse().ok()?;

    // run_id format: YYYYMMDD-HHMMSS-<7hex> = 23 chars.
    // After that comes "-" then node_id.
    let bytes = before_iter.as_bytes();
    const RUN_ID_LEN: usize = 23; // 8 + 1 + 6 + 1 + 7
    if bytes.len() <= RUN_ID_LEN
        || bytes[8] != b'-'
        || bytes[15] != b'-'
        || bytes[RUN_ID_LEN] != b'-'
    {
        return None;
    }

    let run_id = &before_iter[..RUN_ID_LEN];
    let node_id = &before_iter[RUN_ID_LEN + 1..];

    if node_id.is_empty() {
        return None;
    }

    Some(ParsedSession::NodeRun {
        run_id: run_id.to_string(),
        node_id: node_id.to_string(),
        iter,
    })
}

/// Where a NodeRun's files live: its own sub-worktree when `isolated`, the Run's
/// shared worktree otherwise (#653, ADR-0060).
///
/// Pre-#653 this keyed off the node *type* string (`code-mutating` ⇒ its own
/// tree). Isolation is now a per-node choice that the type no longer implies, so
/// callers pass the answer — frozen from `NodeStarted` for a live iteration,
/// read off the document otherwise.
pub fn working_dir_for_node(
    repo_root: &Path,
    run_id: &str,
    node_id: &str,
    iter: i64,
    isolated: bool,
) -> PathBuf {
    if isolated {
        repo_root
            .join(".pdo")
            .join("runs")
            .join(run_id)
            .join("nodes")
            .join(node_id)
            .join(format!("iter-{iter}"))
    } else {
        repo_root
            .join(".pdo")
            .join("runs")
            .join(run_id)
            .join("worktree")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_node_session() {
        let name = "pdo-20260506-143000-a3f1b2c-solo-iter-1";
        let parsed = parse_session_name(name).unwrap();
        assert_eq!(
            parsed,
            ParsedSession::NodeRun {
                run_id: "20260506-143000-a3f1b2c".into(),
                node_id: "solo".into(),
                iter: 1,
            }
        );
    }

    #[test]
    fn parse_node_session_with_dashed_node_id() {
        let name = "pdo-20260506-143000-a3f1b2c-impl-worker-iter-3";
        let parsed = parse_session_name(name).unwrap();
        assert_eq!(
            parsed,
            ParsedSession::NodeRun {
                run_id: "20260506-143000-a3f1b2c".into(),
                node_id: "impl-worker".into(),
                iter: 3,
            }
        );
    }

    #[test]
    fn parse_manager_session() {
        let name = "pdo-mgr-20260506-143000-a3f1b2c";
        let parsed = parse_session_name(name).unwrap();
        assert_eq!(
            parsed,
            ParsedSession::Manager {
                run_id: "20260506-143000-a3f1b2c".into(),
            }
        );
    }

    #[test]
    fn parse_shell_session() {
        // #316: `pdo-shell-<run_id>` parses to a Shell variant, even though the
        // run_id itself contains dashes and no `-iter-` suffix.
        let name = "pdo-shell-20260506-143000-a3f1b2c";
        let parsed = parse_session_name(name).unwrap();
        assert_eq!(
            parsed,
            ParsedSession::Shell {
                run_id: "20260506-143000-a3f1b2c".into(),
            }
        );
    }

    #[test]
    fn parse_libassist_session() {
        // #594: the shared assistant's name parses to the unit LibAssist variant.
        assert_eq!(
            parse_session_name(LIBASSIST_SESSION_NAME),
            Some(ParsedSession::LibAssist)
        );
        // Pre-#594 per-pipeline names still parse — a daemon upgraded under a live
        // tmux server must be able to judge (and reap) the sessions it inherits,
        // not read them as unrecognised.
        assert_eq!(
            parse_session_name("pdo-libassist-feature-with-review"),
            Some(ParsedSession::LibAssist)
        );
    }

    /// **The trap the singleton name exists to avoid** (#594). The parser rejects
    /// an empty suffix, so a bare `pdo-libassist` is unrecognised — and an
    /// unrecognised `pdo-*` name is killed by the very next sweep. That is why the
    /// name is `pdo-libassist-shared`; pinned here so nobody "simplifies" it.
    #[test]
    fn a_bare_libassist_name_would_not_parse() {
        assert!(parse_session_name("pdo-libassist").is_none());
        assert!(parse_session_name("pdo-libassist-").is_none());
    }

    #[test]
    fn parse_garbage_returns_none() {
        assert!(parse_session_name("foo-bar").is_none());
        assert!(parse_session_name("pdo-").is_none());
        assert!(parse_session_name("pdo-mgr-").is_none());
        assert!(parse_session_name("pdo-shell-").is_none());
        assert!(parse_session_name("pdo-libassist-").is_none());
    }

    /// Formatters and parser pinned as a **round trip**, not independently
    /// against literals (which is all `parse_*_session` / the naming tests do).
    /// Without this, a brand-new session kind whose formatter lands with no
    /// matching parser branch is silently reaped within 60 s, leaving one `info!`
    /// line as the only trace. This turns that into a red test.
    #[test]
    fn session_name_formatters_round_trip_through_the_parser() {
        let run_id = "20260731-153057-553fcb3";

        assert_eq!(
            parse_session_name(&node_session_name(run_id, "impl-worker", 7)),
            Some(ParsedSession::NodeRun {
                run_id: run_id.into(),
                node_id: "impl-worker".into(),
                iter: 7,
            })
        );
        assert_eq!(
            parse_session_name(&manager_session_name(run_id)),
            Some(ParsedSession::Manager {
                run_id: run_id.into()
            })
        );
        assert_eq!(
            parse_session_name(&shell_session_name(run_id)),
            Some(ParsedSession::Shell {
                run_id: run_id.into()
            })
        );
        // #302: the libassist formatter must round-trip too — else the sweep reaps
        // it as unrecognised within 60 s (the exact failure this test exists for).
        assert_eq!(
            parse_session_name(libassist_session_name()),
            Some(ParsedSession::LibAssist)
        );
    }

    #[test]
    fn shell_session_name_format() {
        assert_eq!(
            shell_session_name("20260506-143000-a3f1b2c"),
            "pdo-shell-20260506-143000-a3f1b2c"
        );
    }

    #[test]
    fn libassist_session_name_format() {
        // The `-shared` suffix is not cosmetic: see `a_bare_libassist_name_would_not_parse`.
        assert_eq!(libassist_session_name(), "pdo-libassist-shared");
    }

    #[test]
    fn node_session_name_format() {
        assert_eq!(
            node_session_name("20260506-143000-a3f1b2c", "solo", 1),
            "pdo-20260506-143000-a3f1b2c-solo-iter-1"
        );
    }

    #[test]
    fn manager_session_name_format() {
        assert_eq!(
            manager_session_name("20260506-143000-a3f1b2c"),
            "pdo-mgr-20260506-143000-a3f1b2c"
        );
    }
}
