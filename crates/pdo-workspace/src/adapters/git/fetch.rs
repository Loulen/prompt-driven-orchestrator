//! Fetch every remote of a repository, read-only and non-interactive (#802,
//! ADR-0070 §1).

use std::collections::hash_map::DefaultHasher;
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use crate::domain::branches::{classify_fetch_error, FetchError, FetchErrorKind};

use super::branches::git_remote_names;
use super::repository::git_common_dir;

/// How long a fetch may take before it is abandoned (spec #801: "de l'ordre de vingt
/// secondes"). The form fetches on open, so an unreachable remote must not keep a
/// spinner alive for git's own (unbounded) patience.
const FETCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

/// `FETCH_HEAD` as it stood before a fetch attempt, so a failed attempt can put it
/// back exactly as it was (#802).
///
/// git opens `FETCH_HEAD` for writing at the START of a fetch, so a fetch that reaches
/// no remote leaves it truncated and freshly dated — which would wipe out the "dernier
/// fetch réussi" the unknown state is supposed to report, and re-date it to the moment
/// of the failure. Restoring is not a mutation of the repo: it leaves it byte- and
/// mtime-identical to before PDO asked.
struct FetchHeadSnapshot {
    path: PathBuf,
    /// `None` when the file did not exist at all (a repo that never fetched).
    before: Option<(Vec<u8>, std::time::SystemTime)>,
}

impl FetchHeadSnapshot {
    fn take(repo: &Path) -> Option<Self> {
        let path = git_common_dir(repo)?.join("FETCH_HEAD");
        let before = std::fs::metadata(&path)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|modified| Some((std::fs::read(&path).ok()?, modified)));
        Some(Self { path, before })
    }

    /// Put `FETCH_HEAD` back. Best-effort throughout: a repo we cannot write to is a
    /// reason to leave it alone, never a reason to fail a read-only fetch.
    fn restore(&self) {
        match &self.before {
            Some((bytes, modified)) => {
                if std::fs::write(&self.path, bytes).is_ok() {
                    if let Ok(file) = std::fs::OpenOptions::new().write(true).open(&self.path) {
                        let _ = file.set_modified(*modified);
                    }
                }
            }
            // Nothing was there before; the failed attempt's empty husk is the only
            // trace, and `last_fetch_at` already reads it as "never".
            None => {
                let _ = std::fs::remove_file(&self.path);
            }
        }
    }
}

/// A fetch in flight: its repo, and a digest of the environment it runs with,
/// so two callers with different credentials never share one outcome (the
/// digest keeps no value of the environment in memory).
pub type FlightKey = (PathBuf, u64);

/// The outcome slot of a fetch in flight.
type Flight = tokio::sync::watch::Receiver<Option<Option<FetchError>>>;

/// The in-flight fetch of each repo and environment, so concurrent callers
/// **join** the running one instead of starting a second (spec #801). The value
/// is the outcome slot the leader publishes into; followers clone the receiver
/// and await it.
pub fn fetch_in_flight() -> &'static std::sync::Mutex<std::collections::HashMap<FlightKey, Flight>>
{
    static FLIGHTS: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<FlightKey, Flight>>,
    > = std::sync::OnceLock::new();
    FLIGHTS.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// The lock of each repo's fetches: two flights of one repo (two Actors, two
/// credentials) never run at once, since both write its refs and `FETCH_HEAD`.
fn repo_lock(repo: &Path) -> std::sync::Arc<tokio::sync::Mutex<()>> {
    static LOCKS: std::sync::OnceLock<
        std::sync::Mutex<
            std::collections::HashMap<PathBuf, std::sync::Arc<tokio::sync::Mutex<()>>>,
        >,
    > = std::sync::OnceLock::new();
    LOCKS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .entry(repo.to_path_buf())
        .or_default()
        .clone()
}

/// The flight `repo` fetched with `env` joins.
fn flight_key(repo: &Path, env: &BTreeMap<String, String>) -> FlightKey {
    let mut hasher = DefaultHasher::new();
    env.hash(&mut hasher);
    (repo.to_path_buf(), hasher.finish())
}

/// Fetch every remote of `repo` with `env` added to git's environment (the
/// credentials an extension provides, never in an argv), pruning refs that
/// vanished server-side. Returns
/// `None` on success, or the named reason it did not happen (#802, ADR-0070 §1).
///
/// Read-only and **non-interactive by construction**: `GIT_TERMINAL_PROMPT=0` plus a
/// batch-mode ssh, so a missing credential fails with git's own message instead of
/// hanging on a prompt nobody can see (the precedent of `clone::clone_shallow`). Bounded by
/// `FETCH_TIMEOUT`. Never pushes, merges or rebases — that is the whole of ADR-0070 §2.
///
/// Only ONE fetch per repo and environment runs at a time: the form fetches on open
/// and on repo change, and the sync popover refetches, so three overlapping asks on
/// the same repo are routine. The second and third await the first's outcome. A
/// fetch of the same repo with another environment waits for it, then runs.
pub async fn fetch_all_remotes(repo: &Path, env: &BTreeMap<String, String>) -> Option<FetchError> {
    enum Role {
        Leader(tokio::sync::watch::Sender<Option<Option<FetchError>>>),
        Follower(tokio::sync::watch::Receiver<Option<Option<FetchError>>>),
    }
    let key = flight_key(repo, env);
    let role = {
        let mut map = fetch_in_flight().lock().unwrap_or_else(|p| p.into_inner());
        match map.get(&key) {
            Some(rx) => Role::Follower(rx.clone()),
            None => {
                let (tx, rx) = tokio::sync::watch::channel(None);
                map.insert(key.clone(), rx);
                Role::Leader(tx)
            }
        }
    };

    let mut rx = match role {
        Role::Follower(rx) => rx,
        Role::Leader(tx) => {
            let outcome = {
                let lock = repo_lock(repo);
                let _one_at_a_time = lock.lock().await;
                run_git_fetch(repo, env).await
            };
            // Leave the map BEFORE publishing, so a caller arriving after the result
            // starts a fresh fetch rather than joining a finished one. Followers that
            // already cloned the receiver still read the value: a watch receiver keeps
            // the last value after the sender is dropped.
            fetch_in_flight()
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .remove(&key);
            let _ = tx.send(Some(outcome.clone()));
            return outcome;
        }
    };

    loop {
        if rx.borrow().is_some() {
            break;
        }
        if rx.changed().await.is_err() {
            break;
        }
    }
    let published = rx.borrow().clone();
    published.unwrap_or(Some(FetchError {
        kind: FetchErrorKind::Failed,
        message: "the in-flight fetch ended without a result".into(),
    }))
}

/// The fetch itself, with no in-flight bookkeeping. Split out so the join logic above
/// stays readable, and so tests can drive a real `git fetch` against a bare origin.
pub async fn run_git_fetch(repo: &Path, env: &BTreeMap<String, String>) -> Option<FetchError> {
    match git_remote_names(repo) {
        // A repo with no remote is a legitimate case, not a failure to report as one:
        // named so the popover can say "no remote" instead of quoting git at the user.
        Ok(remotes) if remotes.is_empty() => {
            return Some(FetchError {
                kind: FetchErrorKind::NoRemote,
                message: "this repository has no remote".into(),
            });
        }
        Ok(_) => {}
        Err(e) => {
            return Some(FetchError {
                kind: FetchErrorKind::Failed,
                message: e,
            });
        }
    }

    // Taken BEFORE the attempt: git truncates FETCH_HEAD on its way in, so by the time
    // we know the fetch failed the previous date is already gone.
    let snapshot = FetchHeadSnapshot::take(repo);
    let failed = |error: FetchError| {
        if let Some(snapshot) = &snapshot {
            snapshot.restore();
        }
        Some(error)
    };

    // Non-interactive, the operator's own `GIT_SSH_COMMAND` kept with the batch
    // flag added: replacing it would break the very setups that need a custom
    // command, in the name of not prompting.
    let mut cmd = super::clone::network_git(&["fetch", "--all", "--prune", "--quiet"], env);
    cmd.current_dir(repo);

    let spawned = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => {
            return failed(FetchError {
                kind: FetchErrorKind::Failed,
                message: format!("cannot run git: {e}"),
            });
        }
    };

    match tokio::time::timeout(FETCH_TIMEOUT, spawned.wait_with_output()).await {
        Ok(Ok(output)) if output.status.success() => None,
        Ok(Ok(output)) => {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let message = if stderr.is_empty() {
                format!("git fetch exited with {}", output.status)
            } else {
                stderr.clone()
            };
            failed(FetchError {
                kind: classify_fetch_error(&stderr),
                message,
            })
        }
        Ok(Err(e)) => failed(FetchError {
            kind: FetchErrorKind::Failed,
            message: format!("git fetch failed: {e}"),
        }),
        Err(_) => failed(FetchError {
            kind: FetchErrorKind::Timeout,
            message: format!(
                "git fetch did not finish within {}s",
                FETCH_TIMEOUT.as_secs()
            ),
        }),
    }
}
