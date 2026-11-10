//! The repositories of a Run at a write boundary (ADR-0033, ADR-0042): the
//! required primary, and the secondaries frozen into pins — shared by the
//! create path and the mid-run edit of `PATCH /runs/{id}/repos`, so the two
//! surfaces cannot drift. Also where a Run's secondary snapshots sit on disk.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use pdo_workspace::adapters::git::repository::validate_target_repo;
use pdo_workspace::adapters::git::worktrees::rev_parse_verified;

use super::request::TargetRepoInput;
use crate::adapters::http::verdicts::repo_edit::RepoEditRefusal;
use crate::domain::state::RepoPin;

/// The message every write boundary returns when no target repo was named.
/// Actionable on purpose: the caller must learn *what* to pass.
pub const TARGET_REPO_REQUIRED: &str =
    "target_repo is required: name the absolute path of the git \
    repository this must work in — the daemon's own working directory is not a default";

/// Resolve a **required** target repo at a write boundary (ADR-0033).
///
/// Trims once and treats the trimmed value as canonical. The single source of the
/// rule for every write surface; the read side (`effective_repo_root`) deliberately
/// does NOT use it and keeps its fallback for historical runs.
pub fn required_target_repo(field: Option<&str>) -> Result<PathBuf, String> {
    match field.map(str::trim) {
        Some(p) if !p.is_empty() => validate_target_repo(p),
        _ => Err(TARGET_REPO_REQUIRED.to_string()),
    }
}

/// Validate and freeze the secondary repos of a multi-repo create (ADR-0042).
///
/// `inputs` is the full `target_repos` list: `[0]` is the primary, `[1..]` the
/// secondaries. Each `sha` is resolved NOW against the local `base_branch` (default
/// `HEAD`, no fetch) and each `alias` disambiguated on basename collision.
///
/// Fails the whole create on the FIRST bad secondary — resolving here, before
/// `RunStarted` and `create_worktree`, keeps the "a bad repo → no Run" guarantee.
pub fn resolve_secondary_pins(
    primary_root: &Path,
    inputs: &[TargetRepoInput],
) -> Result<Vec<RepoPin>, String> {
    if inputs.len() <= 1 {
        return Ok(Vec::new());
    }
    let primary_canon = canonical_or_self(primary_root);

    let mut pins: Vec<RepoPin> = Vec::new();
    let mut seen_canon: HashSet<PathBuf> = HashSet::new();
    let mut used_aliases: HashSet<String> = HashSet::new();
    for input in &inputs[1..] {
        // Both the create path and `patch_run_repos` freeze a secondary through this
        // ONE helper, so the two surfaces cannot drift.
        let pin =
            resolve_one_secondary_pin(input, &primary_canon, &mut seen_canon, &mut used_aliases)
                .map_err(|r| r.message())?;
        pins.push(pin);
    }
    Ok(pins)
}

/// Canonicalize for comparison, falling back to the path itself when it cannot be
/// resolved. Shared by the create and mid-run edit paths so "is this the primary / a
/// dup?" is answered identically on both.
pub fn canonical_or_self(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// Validate and freeze **one** secondary repo into a [`RepoPin`] — the
/// single per-repo validator both `resolve_secondary_pins` and `PATCH /runs/{id}/repos`
/// route through (ADR-0042).
///
/// `seen_canon` and `used_aliases` are the caller's running sets: create seeds them
/// empty, the edit path seeds them from the Run's SURVIVING secondaries **and the
/// snapshot folders present on disk**, so a removed-but-persistent snapshot's alias
/// is never reused. Mutating them here is what keeps a batch collision-free.
///
/// Writes nothing to disk, so the caller decides whether to project the typed refusal
/// or flatten it to a 400 string.
pub fn resolve_one_secondary_pin(
    input: &TargetRepoInput,
    primary_canon: &Path,
    seen_canon: &mut HashSet<PathBuf>,
    used_aliases: &mut HashSet<String>,
) -> Result<RepoPin, RepoEditRefusal> {
    let raw = input.repo.trim();
    let repo = validate_target_repo(raw).map_err(|error| RepoEditRefusal::BadRepo {
        repo: raw.to_string(),
        error,
    })?;
    let repo_display = repo.to_string_lossy().to_string();
    let repo_canon = canonical_or_self(&repo);
    // A repo cannot be its own secondary: a self-snapshot registers a detached
    // worktree inside the repo it snapshots and buys nothing.
    if repo_canon == primary_canon {
        return Err(RepoEditRefusal::SelfReference { repo: repo_display });
    }
    // A repo already listed would produce two snapshots fighting over one alias.
    if !seen_canon.insert(repo_canon) {
        return Err(RepoEditRefusal::RepoAlreadyPinned { repo: repo_display });
    }
    let base = input
        .base_branch
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let rev = base.unwrap_or("HEAD");
    let sha = rev_parse_verified(&repo, rev).map_err(|e| RepoEditRefusal::BadRepo {
        repo: repo_display.clone(),
        error: format!("cannot resolve base branch '{rev}': {e}"),
    })?;
    let alias = disambiguate_alias(&repo, used_aliases);
    Ok(RepoPin {
        repo: repo_display,
        alias,
        sha,
        base_branch: base.map(str::to_string),
        // Both construction paths route through here, so the flag cannot diverge
        // between the two surfaces.
        read_only: input.read_only,
    })
}

/// A filesystem-safe, collision-free directory name for a secondary snapshot. Starts
/// from the repo's basename and appends `-2`, `-3`, … until unused. `used` must
/// accumulate across the call: the snapshot paths must never collide, or one snapshot
/// clobbers another.
pub fn disambiguate_alias(repo: &Path, used: &mut HashSet<String>) -> String {
    let base = repo
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("repo");
    if used.insert(base.to_string()) {
        return base.to_string();
    }
    let mut n = 2;
    loop {
        let candidate = format!("{base}-{n}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
        n += 1;
    }
}

/// The directory that parents a Run's secondary snapshots: `<run_dir>/repos/`.
pub fn secondary_repos_dir(repo_root: &Path, run_id: &str) -> PathBuf {
    repo_root
        .join(".pdo")
        .join("runs")
        .join(run_id)
        .join("repos")
}

/// The snapshot folder names present on disk under `<run_dir>/repos/`.
///
/// The DISK is the source of truth for taken aliases: a removed-but-persistent
/// snapshot (its teardown is deferred to cleanup) still owns its folder name, so a
/// later `add` reusing it makes `git worktree add` fail on the existing directory.
pub fn existing_snapshot_dir_names(repo_root: &Path, run_id: &str) -> Vec<String> {
    std::fs::read_dir(secondary_repos_dir(repo_root, run_id))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter_map(|e| e.file_name().to_str().map(String::from))
        .collect()
}

/// Resolve the **secondary repo** that owns a detached snapshot at `snapshot_dir`,
/// so `remove_secondary_snapshot` can `prune` its registration from inside it. Two
/// roads, git-authoritative first:
///
/// - `git -C <snapshot> rev-parse --path-format=absolute --git-common-dir` returns
///   the secondary's shared `.git`; its parent is the repo root.
/// - Fallback: the `gitdir:` pointer in `<snapshot>/.git` is
///   `<secondary>/.git/worktrees/<name>`; the root is the path before `/.git/`.
///
/// `None` when neither resolves (a snapshot whose secondary was moved/deleted) — the
/// caller then only removes the directory and warns; the registration is already
/// unreachable.
pub fn snapshot_owning_secondary(snapshot_dir: &Path) -> Option<PathBuf> {
    if let Ok(output) = std::process::Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(snapshot_dir)
        .output()
    {
        if output.status.success() {
            let common = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if let Some(root) = Path::new(&common).parent() {
                if !root.as_os_str().is_empty() {
                    return Some(root.to_path_buf());
                }
            }
        }
    }
    let pointer = std::fs::read_to_string(snapshot_dir.join(".git")).ok()?;
    let raw = pointer.lines().next()?.strip_prefix("gitdir:")?.trim();
    let gitdir = Path::new(raw);
    let idx = gitdir
        .iter()
        .position(|c| c == std::ffi::OsStr::new(".git"))?;
    let root: PathBuf = gitdir.iter().take(idx).collect();
    (!root.as_os_str().is_empty()).then_some(root)
}
