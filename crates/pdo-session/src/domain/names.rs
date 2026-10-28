//! The names of PDO's tmux sessions: how each kind is spelled, and how a name
//! is read back into its kind (the orphan sweep judges a session by its name).

use std::path::{Path, PathBuf};

use pdo_extension_api::{SessionKind, SessionRef};
use pdo_kernel::Actor;

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

/// The library authoring assistant session of the local operator (#594,
/// ADR-0051 — it replaced the per-pipeline name of #302 / ADR-0048): the one
/// session of a daemon whose callers are all the local operator. Since CORE-61
/// every other Actor has its own ([`libassist_session_name_of`]).
///
/// **The `-shared` suffix is load-bearing, not decoration.** [`parse_session_name`]
/// strips the `libassist-` prefix and rejects an *empty* remainder, so a bare
/// `pdo-libassist` would not parse — and an unparseable `pdo-*` name is killed by
/// the orphan sweep as `UnrecognisedName` within one sweep interval. Which template
/// is open travels in the daemon's *focus* (`PUT /sessions/libassist/focus`), never
/// in the session name.
pub const LIBASSIST_SESSION_NAME: &str = "pdo-libassist-shared";

/// Session naming convention for the library pipeline authoring assistant of
/// the local operator. See [`LIBASSIST_SESSION_NAME`] for why the name is not
/// bare.
pub fn libassist_session_name() -> &'static str {
    LIBASSIST_SESSION_NAME
}

/// The library assistant session of `actor` (CORE-61): one per Actor, so
/// `POST /sessions/libassist` is create-if-absent **per Actor** (a second open
/// re-attaches the same session, with its whole conversation) and two Actors
/// never share a conversation.
///
/// The local operator keeps [`LIBASSIST_SESSION_NAME`]: a daemon without an
/// extension that identifies its callers behaves as before. Any other Actor's
/// name ends with a stable digest of the Actor, which tmux accepts whatever the
/// extension's ids hold (`.` and `:` are not allowed in a tmux session name).
pub fn libassist_session_name_of(actor: &Actor) -> String {
    match actor {
        Actor::LocalOperator => LIBASSIST_SESSION_NAME.to_string(),
        other => {
            let key = serde_json::to_string(other).unwrap_or_default();
            format!("pdo-libassist-a{:016x}", fnv1a(key.as_bytes()))
        }
    }
}

/// The prefix of every Session hébergée's name (ADR-0091): the sweep keeps a
/// name with it only while a live session of that name is recorded, and kills
/// any other.
pub const HOSTED_SESSION_PREFIX: &str = "pdo-hosted-";

/// The name of the Session hébergée `suffix` (a random id the session context
/// draws): the extension's key never enters it, so it holds only what tmux
/// accepts.
pub fn hosted_session_name(suffix: &str) -> String {
    format!("{HOSTED_SESSION_PREFIX}{suffix}")
}

/// FNV-1a, 64 bits: a digest that stays the same from one build to the next,
/// unlike the standard library's hasher.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// The session `name`, as the extensions see it ([`SessionRef`]); `None` for a
/// name PDO does not give.
pub fn session_ref(name: &str) -> Option<SessionRef> {
    Some(match parse_session_name(name)? {
        ParsedSession::NodeRun {
            run_id,
            node_id,
            iter,
        } => SessionRef::new(name, SessionKind::Node)
            .in_run(run_id)
            .of_node(node_id, iter),
        ParsedSession::Manager { run_id } => {
            SessionRef::new(name, SessionKind::Manager).in_run(run_id)
        }
        ParsedSession::Shell { run_id } => SessionRef::new(name, SessionKind::Shell).in_run(run_id),
        ParsedSession::LibAssist => SessionRef::new(name, SessionKind::LibraryAssistant),
        ParsedSession::Hosted => SessionRef::new(name, SessionKind::Hosted),
    })
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
    /// A library pipeline authoring assistant (#302 / ADR-0048, one per Actor
    /// since CORE-61, [`libassist_session_name_of`]).
    ///
    /// A **unit** variant: the name says whose assistant it is, never which
    /// template is being edited — that is the *focus* of its Actor; putting
    /// the template back in the name would re-create the per-pipeline session
    /// it replaced.
    /// Owns no Run; reaped by the explicit `DELETE` on leaving every edit view,
    /// with the sweep's idle arm as the backstop.
    LibAssist,
    /// A Session hébergée (ADR-0091, [`hosted_session_name`]). Owns no Run:
    /// its record, not its name, says whether it should be alive.
    Hosted,
}

impl ParsedSession {
    /// The Run the session belongs to; `None` for the library assistant.
    pub fn run_id(&self) -> Option<&str> {
        match self {
            ParsedSession::NodeRun { run_id, .. }
            | ParsedSession::Manager { run_id }
            | ParsedSession::Shell { run_id } => Some(run_id),
            ParsedSession::LibAssist | ParsedSession::Hosted => None,
        }
    }
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
    // The suffix names whose assistant it is (CORE-61), never a template, and it
    // must be **non-empty** — that rejection is why the local operator's is named
    // `pdo-libassist-shared` and not `pdo-libassist`.
    if let Some(suffix) = rest.strip_prefix("libassist-") {
        if !suffix.is_empty() {
            return Some(ParsedSession::LibAssist);
        }
        return None;
    }

    // ADR-0091: a Session hébergée, parsed before the `-iter-` split like the
    // other names without a Run. Its suffix must be non-empty.
    if let Some(suffix) = rest.strip_prefix("hosted-") {
        if !suffix.is_empty() {
            return Some(ParsedSession::Hosted);
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
    fn each_actor_has_its_own_assistant_and_the_local_operator_keeps_the_shared_one() {
        let alice = Actor::Identified {
            namespace: "test".into(),
            id: "alice@example.org".into(),
        };
        let bob = Actor::Identified {
            namespace: "test".into(),
            id: "bob".into(),
        };
        assert_eq!(
            libassist_session_name_of(&Actor::LocalOperator),
            LIBASSIST_SESSION_NAME
        );
        let of_alice = libassist_session_name_of(&alice);
        assert_eq!(of_alice, libassist_session_name_of(&alice.clone()));
        assert_ne!(of_alice, libassist_session_name_of(&bob));
        assert_ne!(of_alice, LIBASSIST_SESSION_NAME);
        assert!(
            !of_alice.contains('.') && !of_alice.contains(':') && !of_alice.contains('@'),
            "tmux takes it: {of_alice}"
        );
        assert_eq!(
            parse_session_name(&of_alice),
            Some(ParsedSession::LibAssist)
        );
    }

    #[test]
    fn a_session_name_reads_as_the_session_the_extensions_see() {
        let run_id = "20260731-153057-553fcb3";
        let node = session_ref(&node_session_name(run_id, "worker", 2)).unwrap();
        assert_eq!(node.kind, SessionKind::Node);
        assert_eq!(node.run_id.as_deref(), Some(run_id));
        assert_eq!(node.node_id.as_deref(), Some("worker"));
        assert_eq!(node.iter, Some(2));
        assert_eq!(node.id, node_session_name(run_id, "worker", 2));
        let manager = session_ref(&manager_session_name(run_id)).unwrap();
        assert_eq!(manager.kind, SessionKind::Manager);
        assert_eq!(manager.run_id.as_deref(), Some(run_id));
        assert_eq!(manager.node_id, None);
        let shell = session_ref(&shell_session_name(run_id)).unwrap();
        assert_eq!(shell.kind, SessionKind::Shell);
        let assistant = session_ref(LIBASSIST_SESSION_NAME).unwrap();
        assert_eq!(assistant.kind, SessionKind::LibraryAssistant);
        assert_eq!(assistant.run_id, None);
        assert_eq!(session_ref("pdo-"), None);
    }

    #[test]
    fn a_hosted_session_name_reads_back_as_a_hosted_session_without_a_run() {
        let name = hosted_session_name("1a2b3c4d5e6f");
        assert_eq!(name, "pdo-hosted-1a2b3c4d5e6f");
        assert!(name.starts_with(HOSTED_SESSION_PREFIX));
        assert_eq!(parse_session_name(&name), Some(ParsedSession::Hosted));
        assert_eq!(ParsedSession::Hosted.run_id(), None);
        assert_eq!(parse_session_name("pdo-hosted-"), None);
        assert_eq!(
            session_ref(&name),
            Some(SessionRef::new(&name, SessionKind::Hosted))
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
