//! The branches a Run can be cut from, and their freshness against the remote
//! (#571, #802, ADR-0070 §1): what `GET /repos/branches` and `POST /repos/fetch`
//! answer, and the pure readings of git's plumbing output behind them.

use std::collections::HashSet;

use pdo_kernel::extension_host::GitCredentials;
use serde::Serialize;
use ts_rs::TS;

use super::remote::remote_host;

/// A branch offered as a Run source: a local branch or a remote-tracking ref. `name`
/// is the exact string the client posts back as `source_branch`, so a remote-tracking
/// ref keeps its `origin/` prefix. NEVER re-derive locality by string surgery: a
/// *local* branch may legitimately be named `origin/x`, so `kind` — classified from
/// the full refname — is the only authoritative signal.
///
/// #802/ADR-0070: the entry also carries what makes freshness *visible* at launch —
/// the upstream branch, the écart amont (`ahead`/`behind`) **as of the last fetch**,
/// and the tip commit's date and subject. Every extra field is optional on purpose:
/// a branch with no upstream has no écart (not a zero one), and a repo git cannot
/// read a date from is not an error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, rename = "BranchRef")]
pub struct BranchEntry {
    pub name: String,
    pub kind: BranchKind,
    /// The tracking branch's short name (`origin/main`), when this local branch has
    /// one. Always `None` for a remote-tracking ref — it *is* the upstream.
    pub upstream: Option<String>,
    /// Commits this branch has that its upstream does not, as of the last fetch.
    /// `None` when there is no upstream, or when the upstream ref is gone.
    pub ahead: Option<u32>,
    /// Commits the upstream has that this branch does not, as of the last fetch.
    pub behind: Option<u32>,
    /// Tip commit date, ISO-8601 with offset (`git`'s `iso-strict`).
    pub last_commit_at: Option<String>,
    pub last_commit_subject: Option<String>,
    /// #803: this branch is the repo's checked-out one. Decides one sentence in
    /// the fast-forward popover — advancing HEAD walks the working tree forward,
    /// advancing any other branch is a pure ref move that changes no file.
    /// Always `false` for a remote-tracking ref.
    pub head: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum BranchKind {
    Local,
    Remote,
}

/// What `GET /repos/branches` and `POST /repos/fetch` both answer (#802).
///
/// One shape for the two verbs, so a refetch replaces the list in place instead of
/// teaching the client a second format. `last_fetch_at` is **derived from the repo
/// itself** (the mtime of `FETCH_HEAD`), never stored by PDO: a fetch anyone ran —
/// from a terminal, from an IDE — dates the numbers just as well as ours.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct BranchList {
    pub branches: Vec<BranchEntry>,
    /// RFC-3339 timestamp of the last successful fetch, or `None` when the repo has
    /// never fetched (fresh `git init`, or a clone that never refreshed).
    pub last_fetch_at: Option<String>,
    /// Only ever set by the fetch verb, and only when the fetch failed. The list
    /// beside it is still the truth on disk — a failed fetch degrades the écart to
    /// "unknown, as of `last_fetch_at`", it never empties the list (ADR-0070 §1).
    pub fetch_error: Option<FetchError>,
}

/// Why a fetch did not happen, named rather than raw (#802). The client picks its
/// wording from `kind`; `message` carries git's own stderr for the detail line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum FetchErrorKind {
    /// The repo has no remote at all — a legitimate, purely local repo.
    NoRemote,
    /// Exceeded `FETCH_TIMEOUT`.
    Timeout,
    /// Credentials missing or refused. Never a prompt: the fetch runs
    /// non-interactive, so this is what a missing key looks like.
    Auth,
    /// Host unreachable / unresolvable.
    Network,
    /// Anything else git refused to do.
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, rename = "BranchFetchError")]
pub struct FetchError {
    pub kind: FetchErrorKind,
    pub message: String,
}

impl FetchError {
    /// The same failure, its message led by whose credentials the host of
    /// `url` refused, when it refused them (`kind` is
    /// [`FetchErrorKind::Auth`]): the ones provided for an Actor, or the
    /// daemon's.
    pub fn naming_credentials(mut self, url: &str, credentials: &GitCredentials) -> Self {
        if self.kind == FetchErrorKind::Auth {
            let refused = credentials.refused_by(&remote_host(url));
            self.message = format!("{refused}: {}", self.message);
        }
        self
    }
}

/// Strip the `<remote>/` prefix off a remote-tracking short name, yielding the
/// bare branch name used for the dedup test. Falls back to the input unchanged
/// when no configured remote prefixes it.
pub fn strip_remote_prefix(short: &str, remotes: &[String]) -> String {
    for r in remotes {
        if let Some(rest) = short.strip_prefix(&format!("{r}/")) {
            return rest.to_string();
        }
    }
    short.to_string()
}

/// Read `git for-each-ref` over `refs/heads` and `refs/remotes` into the list a
/// Run can be cut from. `stdout` has one line per ref, eight tab-separated
/// fields: refname, short name, symref, upstream, upstream track, HEAD marker,
/// committer date, subject (last, since it may itself hold a tab).
///
/// A remote-tracking ref whose bare name already exists as a local branch is
/// dropped unconditionally, on the convention that the local branch is what the
/// user manipulates. Remote-vs-remote twins (`origin/foo` + `upstream/foo`) are
/// BOTH kept: two remotes may point at different SHAs, so merging them would lie.
///
/// Locals before remotes via an EXPLICIT partition — never lean on
/// `refs/heads` < `refs/remotes` lexical luck, since the first entry may become
/// the client's default and a local `main` must win.
pub fn parse_branch_refs(stdout: &str, remotes: &[String]) -> Vec<BranchEntry> {
    let mut locals: Vec<BranchEntry> = Vec::new();
    let mut remote_pairs: Vec<(String, BranchEntry)> = Vec::new();
    let mut local_names: HashSet<String> = HashSet::new();

    for line in stdout.lines() {
        let mut fields = line.splitn(8, '\t');
        let refname = fields.next().unwrap_or("");
        let short = fields.next().unwrap_or("");
        let symref = fields.next().unwrap_or("");
        let upstream = fields.next().unwrap_or("");
        let track = fields.next().unwrap_or("");
        // `%(HEAD)` is `*` for the checked-out branch, a space otherwise — git's
        // own marker, so a detached HEAD marks nothing and no branch claims it.
        let head = fields.next().unwrap_or("").trim() == "*";
        let date = fields.next().unwrap_or("");
        let subject = fields.next().unwrap_or("");
        if !symref.is_empty() {
            continue;
        }
        let some = |s: &str| (!s.is_empty()).then(|| s.to_string());
        // Classify by the FULL refname prefix, never the short name: a local branch
        // literally named `origin/ambig` lives under refs/heads/ and must list local.
        if refname.starts_with("refs/heads/") {
            let (ahead, behind) = parse_upstream_track(track, !upstream.is_empty());
            local_names.insert(short.to_string());
            locals.push(BranchEntry {
                name: short.to_string(),
                kind: BranchKind::Local,
                upstream: some(upstream),
                ahead,
                behind,
                last_commit_at: some(date),
                last_commit_subject: some(subject),
                head,
            });
        } else if refname.starts_with("refs/remotes/") {
            let bare = strip_remote_prefix(short, remotes);
            remote_pairs.push((
                bare,
                BranchEntry {
                    name: short.to_string(),
                    kind: BranchKind::Remote,
                    // A remote-tracking ref IS the upstream; it has no écart of its own.
                    upstream: None,
                    ahead: None,
                    behind: None,
                    last_commit_at: some(date),
                    last_commit_subject: some(subject),
                    // A remote-tracking ref is never anyone's HEAD.
                    head: false,
                },
            ));
        }
    }

    let mut remotes_out: Vec<BranchEntry> = remote_pairs
        .into_iter()
        .filter(|(bare, _entry)| !local_names.contains(bare))
        .map(|(_bare, entry)| entry)
        .collect();

    locals.sort_by(|a, b| a.name.cmp(&b.name));
    remotes_out.sort_by(|a, b| a.name.cmp(&b.name));

    let mut out = locals;
    out.extend(remotes_out);
    out
}

/// Read `git`'s `%(upstream:track)` column into the écart amont (#802).
///
/// git prints `[ahead 2]`, `[behind 3]`, `[ahead 2, behind 3]`, `[gone]`, or an
/// EMPTY string when the branch is exactly level with its upstream. That last case
/// is why `has_upstream` is a separate argument: empty-and-tracking means `0↑ 0↓`
/// ("up to date"), empty-and-not-tracking means *there is no écart to speak of*, and
/// collapsing the two would paint a ✓ on every branch that tracks nothing.
///
/// `[gone]` — the upstream was pruned away — keeps `upstream` set but leaves the
/// counts unknown; there is nothing left to count against.
pub fn parse_upstream_track(track: &str, has_upstream: bool) -> (Option<u32>, Option<u32>) {
    if !has_upstream {
        return (None, None);
    }
    let inner = track.trim().trim_start_matches('[').trim_end_matches(']');
    if inner.is_empty() {
        return (Some(0), Some(0));
    }
    if inner == "gone" {
        return (None, None);
    }
    let mut ahead = Some(0);
    let mut behind = Some(0);
    for part in inner.split(',') {
        let part = part.trim();
        if let Some(n) = part.strip_prefix("ahead ") {
            ahead = n.trim().parse().ok();
        } else if let Some(n) = part.strip_prefix("behind ") {
            behind = n.trim().parse().ok();
        }
    }
    (ahead, behind)
}

/// Classify git's fetch stderr into a named reason (#802).
///
/// Matching on git's prose is unavoidable — `git fetch` exits 128 for everything —
/// but the *consequence* of a mis-classification is only a slightly wrong sentence in
/// the popover: the stderr line itself is always shown, and no branch of the UI
/// behaves differently per kind. Hence substring matching rather than an exhaustive
/// parser that would rot faster.
pub fn classify_fetch_error(stderr: &str) -> FetchErrorKind {
    let s = stderr.to_lowercase();
    let network = [
        "could not resolve host",
        "could not resolve hostname",
        "temporary failure in name resolution",
        "connection refused",
        "connection timed out",
        "network is unreachable",
        "no route to host",
        "operation timed out",
        "failed to connect",
    ];
    let auth = [
        "permission denied",
        "authentication failed",
        "could not read username",
        "could not read password",
        "terminal prompts disabled",
        "access denied",
        "invalid username or password",
        "publickey",
    ];
    if network.iter().any(|m| s.contains(m)) {
        return FetchErrorKind::Network;
    }
    if auth.iter().any(|m| s.contains(m)) {
        return FetchErrorKind::Auth;
    }
    FetchErrorKind::Failed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_upstream_track_reads_each_shape_git_prints() {
        assert_eq!(parse_upstream_track("", true), (Some(0), Some(0)));
        assert_eq!(parse_upstream_track("[ahead 2]", true), (Some(2), Some(0)));
        assert_eq!(parse_upstream_track("[behind 3]", true), (Some(0), Some(3)));
        assert_eq!(
            parse_upstream_track("[ahead 2, behind 3]", true),
            (Some(2), Some(3))
        );
        // Pruned upstream: still tracking something, but nothing left to count.
        assert_eq!(parse_upstream_track("[gone]", true), (None, None));
        // No upstream at all — the empty column must NOT read as "level".
        assert_eq!(parse_upstream_track("", false), (None, None));
    }

    #[test]
    fn a_refused_fetch_names_whose_credentials() {
        let refused = FetchError {
            kind: FetchErrorKind::Auth,
            message: "fatal: Authentication failed".into(),
        };
        let provided =
            GitCredentials::provided(Default::default(), "Alice", Some("a token".into()));
        assert_eq!(
            refused
                .clone()
                .naming_credentials("https://tok@git.example.com/a.git", &provided)
                .message,
            "The credentials provided for Alice (a token) were refused for git.example.com: fatal: Authentication failed"
        );
        assert!(refused
            .naming_credentials("git@h:a.git", &GitCredentials::daemon())
            .message
            .starts_with("The daemon's git credentials were refused for h: "));
        let offline = FetchError {
            kind: FetchErrorKind::Network,
            message: "offline".into(),
        };
        assert_eq!(
            offline.clone().naming_credentials("https://h/a", &provided),
            offline
        );
    }

    #[test]
    fn classify_fetch_error_names_the_common_refusals() {
        assert_eq!(
            classify_fetch_error("ssh: Could not resolve hostname github.com"),
            FetchErrorKind::Network
        );
        assert_eq!(
            classify_fetch_error("git@github.com: Permission denied (publickey)."),
            FetchErrorKind::Auth
        );
        assert_eq!(
            classify_fetch_error("fatal: couldn't find remote ref"),
            FetchErrorKind::Failed
        );
    }

    fn line(fields: &[&str]) -> String {
        fields.join("\t")
    }

    #[test]
    fn locals_come_first_a_remote_twin_is_dropped_and_symrefs_skipped() {
        let remotes = vec!["origin".to_string()];
        let stdout = [
            line(&[
                "refs/heads/main",
                "main",
                "",
                "origin/main",
                "[behind 2]",
                "*",
                "2026-01-01T00:00:00+00:00",
                "tip",
            ]),
            line(&[
                "refs/heads/origin/ambig",
                "origin/ambig",
                "",
                "",
                "",
                " ",
                "",
                "",
            ]),
            line(&[
                "refs/remotes/origin/HEAD",
                "origin",
                "refs/remotes/origin/main",
                "",
                "",
                " ",
                "",
                "",
            ]),
            line(&[
                "refs/remotes/origin/main",
                "origin/main",
                "",
                "",
                "",
                " ",
                "",
                "",
            ]),
            line(&[
                "refs/remotes/origin/feature",
                "origin/feature",
                "",
                "",
                "",
                " ",
                "",
                "a\ttabbed subject",
            ]),
        ]
        .join("\n");
        let branches = parse_branch_refs(&stdout, &remotes);
        let names: Vec<_> = branches.iter().map(|b| (b.name.as_str(), b.kind)).collect();
        assert_eq!(
            names,
            vec![
                ("main", BranchKind::Local),
                ("origin/ambig", BranchKind::Local),
                ("origin/feature", BranchKind::Remote),
            ]
        );
        assert_eq!((branches[0].ahead, branches[0].behind), (Some(0), Some(2)));
        assert!(branches[0].head);
        assert_eq!(branches[0].last_commit_subject.as_deref(), Some("tip"));
        assert_eq!(branches[1].upstream, None);
        assert_eq!(
            branches[2].last_commit_subject.as_deref(),
            Some("a\ttabbed subject")
        );
    }

    #[test]
    fn the_remote_prefix_is_stripped_by_the_longest_remote_first() {
        let remotes = vec!["origin/nested".to_string(), "origin".to_string()];
        assert_eq!(strip_remote_prefix("origin/nested/x", &remotes), "x");
        assert_eq!(strip_remote_prefix("origin/y", &remotes), "y");
        assert_eq!(strip_remote_prefix("other/z", &remotes), "other/z");
    }
}
