//! The routes no `pdo` command calls, and why.
//!
//! - [`EXCLUDED`]: for good. Each entry says why a command line has nothing to
//!   do with these routes (a stream for the browser, a terminal, a presence, a
//!   resource the UI draws with, a shell's own tools). A new entry is a
//!   decision, read in review.
//! - [`PENDING`]: **provisional exclusions, to cover later**: gestures that
//!   deserve a command not written yet, listed by family with the date they
//!   were listed. The family's commands remove their entries one by one; the
//!   test fails on an entry whose route a command calls.

use crate::routes::Route;

/// Routes excluded for good, with the reason.
pub struct Excluded {
    pub reason: &'static str,
    pub routes: &'static [(&'static str, &'static str)],
}

/// Routes whose command is still to write: a provisional exclusion.
pub struct Pending {
    /// The family of commands that will cover them.
    pub family: &'static str,
    /// When they were listed (`YYYY-MM-DD`).
    pub since: &'static str,
    pub routes: &'static [(&'static str, &'static str)],
}

/// Whether one entry of `excluded` or `pending` names `route`.
pub fn names(excluded: &[Excluded], pending: &[Pending], route: &Route) -> bool {
    let matches = |(m, p): &(&str, &str)| *m == route.0 && *p == route.1;
    excluded.iter().any(|e| e.routes.iter().any(matches))
        || pending.iter().any(|p| p.routes.iter().any(matches))
}

pub const EXCLUDED: &[Excluded] = &[
    Excluded {
        reason: "a WebSocket for the browser: the UI's live events, a terminal's \
                 keystrokes, the shared draft of the pipeline editor. A command reads a \
                 state (`pdo run status`, `pdo pipeline show`) or waits on one \
                 (`pdo run wait`)",
        routes: &[
            ("GET", "/ws"),
            ("GET", "/sessions/{session_id}/pty"),
            ("GET", "/pipelines/{pipeline_id}/draft"),
        ],
    },
    Excluded {
        reason: "drives a terminal the browser shows: opening one on a session, the \
                 manager or a shell, reading its pane, typing into it, its zoom. From a \
                 shell, attach to the tmux session itself",
        routes: &[
            ("POST", "/sessions/{session_id}/attach"),
            ("POST", "/sessions/{run_id}/manager/attach"),
            ("POST", "/sessions/{run_id}/shell"),
            ("GET", "/runs/{run_id}/nodes/{node_id}/pane"),
            ("POST", "/runs/{run_id}/nodes/{node_id}/terminal-text"),
            ("GET", "/sessions/{session_id}/terminal-zoom"),
            ("PUT", "/sessions/{session_id}/terminal-zoom"),
        ],
    },
    Excluded {
        reason: "a presence: who has a Run open in a browser. A command opens nothing",
        routes: &[("GET", "/runs/presences")],
    },
    Excluded {
        reason: "files served to the browser: a mounted page and the live artifacts of \
                 a session. They are on disk, in the directory `pdo page list` shows \
                 and in `PDO_LIVE_DIR`",
        routes: &[
            ("GET", "/pages/"),
            ("GET", "/pages/{name}"),
            ("GET", "/pages/{name}/"),
            ("GET", "/pages/{name}/{*path}"),
            ("GET", "/sessions/{session_id}/live"),
            ("GET", "/sessions/{session_id}/live/{*path}"),
        ],
    },
    Excluded {
        reason: "what the UI draws its screens with: the catalogue of Ressources an \
                 extension's role editor lists. A command prints ids",
        routes: &[("GET", "/resources")],
    },
    Excluded {
        reason: "the question a UI asks before showing a gesture. A command asks by \
                 making the gesture, and prints the refusal",
        routes: &[("POST", "/authorize")],
    },
    Excluded {
        reason: "the companion of the pipeline editor open in a browser: the editor \
                 opens it, sends its focus as a heartbeat and closes it; the assistant \
                 reads the draft and saves it from its own session",
        routes: &[
            ("POST", "/sessions/libassist"),
            ("DELETE", "/sessions/libassist"),
            ("GET", "/sessions/libassist/focus"),
            ("PUT", "/sessions/libassist/focus"),
            ("POST", "/sessions/libassist/save"),
            ("GET", "/sessions/libassist/draft"),
        ],
    },
    Excluded {
        reason: "the Prometheus exposition, read by a scraper",
        routes: &[("GET", "/metrics")],
    },
    Excluded {
        reason: "the browser's stand-in for a shell: browsing folders, a repository's \
                 branches, remote and validity, fetch, fast-forward, clone, create, the \
                 recent list of the launch form. From a shell, `ls` and `git`; a \
                 repository joins a Projet with `pdo project member add`",
        routes: &[
            ("GET", "/fs/browse"),
            ("GET", "/repos/branches"),
            ("POST", "/repos/fetch"),
            ("POST", "/repos/fast-forward"),
            ("GET", "/repos/validate"),
            ("GET", "/repos/remote"),
            ("POST", "/repos/create"),
            ("GET", "/repos/clone-target"),
            ("GET", "/repos/clones"),
            ("POST", "/repos/clones"),
            ("GET", "/repos/clones/{id}"),
            ("DELETE", "/repos/clones/{id}"),
            ("GET", "/repos/recent"),
        ],
    },
];

/// Empty: every route has its command or its reason above. A gesture whose
/// command is still to write goes here, dated, under the family that will
/// cover it.
pub const PENDING: &[Pending] = &[];
