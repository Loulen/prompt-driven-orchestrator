//! Reading a repository: is it one, which refs does it have, how far apart
//! are two of them.

use std::path::{Path, PathBuf};

pub fn validate_target_repo(path: &str) -> Result<PathBuf, String> {
    let p = PathBuf::from(path);
    if !p.is_absolute() {
        return Err("target_repo must be an absolute path".into());
    }
    if !p.is_dir() {
        return Err(format!(
            "target_repo does not exist or is not a directory: {path}"
        ));
    }
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .current_dir(&p)
        .output();
    match output {
        Ok(o) if o.status.success() => Ok(p),
        _ => Err(format!("target_repo is not a git repository: {path}")),
    }
}

/// Accept exactly a local branch OR a remote-tracking ref as a Run source, probing
/// each namespace with an exact-ref lookup.
///
/// Don't go back to `git branch --list <b>`: it is blind to remote-tracking refs AND
/// treats its argument as a glob (`mai*` matches `main`). `show-ref --verify` takes a
/// full refname literally. Deliberately NOT widened to any commit-ish — `rev-parse`
/// would accept SHAs and tags, and the contract is a *branch*.
pub fn validate_source_branch(repo: &Path, branch: &str) -> Result<(), String> {
    for fullref in [
        format!("refs/heads/{branch}"),
        format!("refs/remotes/{branch}"),
    ] {
        let output = std::process::Command::new("git")
            .args(["show-ref", "--verify", "--quiet", &fullref])
            .current_dir(repo)
            .output();
        match output {
            Ok(o) if o.status.success() => return Ok(()),
            // Ref absent in this namespace: try the next before giving up.
            Ok(_) => {}
            Err(e) => return Err(format!("failed to run git: {e}")),
        }
    }
    Err(format!(
        "branch '{branch}' does not exist in {} \
         (looked for a local branch refs/heads/{branch} and a \
         remote-tracking ref refs/remotes/{branch})",
        repo.display()
    ))
}

/// The repo's common git dir — where `FETCH_HEAD` lives. `--git-common-dir` (not
/// `--git-dir`) because a linked worktree has its own git dir but shares the common
/// one, and PDO's own Runs live in linked worktrees: reading the per-worktree dir
/// would report "never fetched" for every Run worktree.
pub fn git_common_dir(repo: &Path) -> Option<PathBuf> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--git-common-dir"])
        .current_dir(repo)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if raw.is_empty() {
        return None;
    }
    let path = PathBuf::from(&raw);
    Some(if path.is_absolute() {
        path
    } else {
        repo.join(path)
    })
}

/// When this repo last fetched successfully, read off `FETCH_HEAD`'s mtime (#802).
///
/// Derived, never stored (ADR-0070 §1): git rewrites `FETCH_HEAD` on every successful
/// fetch, whoever ran it — a `git fetch` in a terminal dates the écart just as well as
/// one of ours, and a PDO-owned timestamp would claim otherwise.
///
/// An EMPTY `FETCH_HEAD` reads as "never" rather than "just now": a fetch that reached
/// no remote truncates the file to zero bytes and re-dates it, so its mtime dates a
/// FAILURE. Trusting it would print "fetched 2 s ago" over numbers nobody refreshed —
/// the precise lie this feature exists to remove. (PDO's own fetch restores the file
/// when it fails, see `run_git_fetch`; this guard covers the failed fetches run
/// outside PDO, from a terminal or an IDE.)
pub fn last_fetch_at(repo: &Path) -> Option<String> {
    let meta = std::fs::metadata(git_common_dir(repo)?.join("FETCH_HEAD")).ok()?;
    if meta.len() == 0 {
        return None;
    }
    Some(chrono::DateTime::<chrono::Utc>::from(meta.modified().ok()?).to_rfc3339())
}

/// One line of `git worktree list --porcelain`: which checkout holds which branch.
///
/// Parsed rather than `git branch --show-current`-ed per worktree: the porcelain
/// format is stable, names every worktree of the repo in one call, and reports a
/// detached HEAD as the ABSENCE of a `branch` line (so a detached checkout never
/// claims a branch it does not hold).
pub fn worktree_holding_branch(repo: &Path, branch: &str) -> Option<PathBuf> {
    let output = std::process::Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(repo)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let wanted = format!("refs/heads/{branch}");
    let mut current: Option<&str> = None;
    for line in stdout.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            current = Some(path);
        } else if let Some(r) = line.strip_prefix("branch ") {
            if r == wanted {
                return current.map(PathBuf::from);
            }
        }
    }
    None
}

/// The modified TRACKED files of a checkout, as `git status --porcelain` reports
/// them (`XY path`). Untracked files are excluded on purpose: they survive a
/// fast-forward untouched, so refusing over them would refuse the commonest tree
/// there is (#803, spec #801).
pub fn dirty_tracked_files(checkout: &Path) -> Vec<String> {
    let output = std::process::Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=no"])
        .current_dir(checkout)
        .output();
    match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .lines()
            .map(|l| l.trim_end().to_string())
            .filter(|l| !l.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

/// `git rev-list --left-right --count a...b` → `(ahead, behind)`, or `None` when
/// either side does not resolve.
pub fn ahead_behind(repo: &Path, branch: &str, upstream: &str) -> Option<(u32, u32)> {
    let range = format!("refs/heads/{branch}...refs/remotes/{upstream}");
    let output = std::process::Command::new("git")
        .args(["rev-list", "--left-right", "--count", &range])
        .current_dir(repo)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut parts = stdout.split_whitespace();
    let ahead = parts.next()?.parse().ok()?;
    let behind = parts.next()?.parse().ok()?;
    Some((ahead, behind))
}

/// The tracking branch of a local branch, short form (`origin/main`), or `None`.
pub fn upstream_of(repo: &Path, branch: &str) -> Option<String> {
    let spec = format!("{branch}@{{upstream}}");
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "--symbolic-full-name", &spec])
        .current_dir(repo)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!name.is_empty()).then_some(name)
}

/// Short SHA + subject of a ref, for the "a1b2c3d → e4f5a6b · subject" line.
pub fn tip_summary(repo: &Path, r: &str) -> (String, Option<String>) {
    let output = std::process::Command::new("git")
        .args(["log", "-1", "--format=%h%x09%s", r])
        .current_dir(repo)
        .output();
    match output {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            let line = stdout.trim_end_matches('\n');
            let mut fields = line.splitn(2, '\t');
            let sha = fields.next().unwrap_or("").to_string();
            let subject = fields
                .next()
                .map(|s| s.to_string())
                .filter(|s| !s.is_empty());
            (sha, subject)
        }
        _ => (String::new(), None),
    }
}

/// Short SHA of a ref (`--short` leaves git to pick a unambiguous length).
pub fn rev_parse_short(repo: &Path, r: &str) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--short", r])
        .current_dir(repo)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!sha.is_empty()).then_some(sha)
}

/// Whether two paths name the same checkout. Canonicalised on BOTH sides — the
/// target repo is stored verbatim (ADR-0033) while `git worktree list` prints
/// resolved paths, so a `/tmp` that is a symlink to `/private/tmp` (macOS) would
/// otherwise make our own checkout look like somebody else's and refuse every
/// fast-forward on the platform.
pub fn same_checkout(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// `git rev-list --count from..to` — commits `to` has that `from` does not.
pub fn count_commits(repo: &Path, from: &str, to: &str) -> Option<u32> {
    let range = format!("{from}..{to}");
    let output = std::process::Command::new("git")
        .args(["rev-list", "--count", &range])
        .current_dir(repo)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout).trim().parse().ok()
}

pub fn ref_exists(repo: &Path, fullref: &str) -> bool {
    std::process::Command::new("git")
        .args(["show-ref", "--verify", "--quiet", fullref])
        .current_dir(repo)
        .output()
        .is_ok_and(|o| o.status.success())
}

/// The commit a ref names (`<ref>^{commit}`), or `None` when it names none.
pub fn resolve_commit(repo: &Path, r: &str) -> Option<String> {
    let spec = format!("{r}^{{commit}}");
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--verify", "--quiet", &spec])
        .current_dir(repo)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!sha.is_empty()).then_some(sha)
}

/// `<common git dir>/info/exclude` of the repository `dir` belongs to (a linked
/// worktree resolves to the shared common dir). `None` outside a work tree.
pub fn info_exclude_path(dir: &Path) -> anyhow::Result<Option<PathBuf>> {
    use anyhow::Context;
    let output = std::process::Command::new("git")
        .args([
            "rev-parse",
            "--is-inside-work-tree",
            "--git-path",
            "info/exclude",
        ])
        .current_dir(dir)
        .output()
        .with_context(|| format!("locate Git metadata for {}", dir.display()))?;
    if !output.status.success() {
        return Ok(None);
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut lines = text.lines();
    if lines.next() != Some("true") {
        return Ok(None);
    }
    let Some(path) = lines.next().filter(|l| !l.is_empty()) else {
        return Ok(None);
    };
    let path = PathBuf::from(path);
    Ok(Some(if path.is_absolute() {
        path
    } else {
        dir.join(path)
    }))
}

/// Whether any path under `rel` is tracked in `worktree`'s index.
pub fn is_tracked(worktree: &Path, rel: &str) -> anyhow::Result<bool> {
    use anyhow::Context;
    let output = std::process::Command::new("git")
        .args(["ls-files", "-z", "--", rel])
        .current_dir(worktree)
        .output()
        .with_context(|| format!("git ls-files {rel} in {}", worktree.display()))?;
    Ok(output.status.success() && !output.stdout.is_empty())
}
