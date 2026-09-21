//! Structured Run diff (#748, ADR-0067): a `git diff` between two refs turned into
//! files → hunks → lines, so the Diff tab (and later the Review page) render
//! from data instead of re-parsing the raw patch in the browser.
//!
//! This is the server-side successor of the frontend's `parseUnifiedDiff.ts`:
//! same contract (best-effort, never fails on a chunk it does not fully
//! understand), same path ladder (`rename from/to` → `---`/`+++` → `Binary files`
//! → `diff --git` header), same counting rule (`+`/`-` lines inside hunks; a
//! binary file counts as a file with 0/0).
//!
//! The raw-patch endpoints (`GET /runs/<id>/diff`, `…/nodes/<node>/diff`) are
//! untouched; this module only adds a second reading of the same `git diff`.

use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Serialize, Debug, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FileStatus {
    Added,
    Deleted,
    Modified,
    Renamed,
    Copied,
}

#[derive(Serialize, Debug, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LineKind {
    Context,
    Add,
    Del,
}

/// One line of a hunk. `old_no`/`new_no` are the 1-based line numbers on each
/// side — `None` on the side the line does not exist on (`Add` has no `old_no`,
/// `Del` has no `new_no`).
#[derive(Serialize, Debug, PartialEq, Eq, Clone)]
pub(crate) struct DiffLine {
    pub kind: LineKind,
    /// The line text WITHOUT its leading `+`/`-`/space marker.
    pub content: String,
    pub old_no: Option<u32>,
    pub new_no: Option<u32>,
}

#[derive(Serialize, Debug, PartialEq, Eq, Clone)]
pub(crate) struct Hunk {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    /// The function context git prints after the second `@@` (may be empty).
    pub header: String,
    pub lines: Vec<DiffLine>,
}

#[derive(Serialize, Debug, PartialEq, Eq, Clone)]
pub(crate) struct FileDiff {
    /// Source path, `None` for a pure addition.
    pub old_path: Option<String>,
    /// Destination path, `None` for a pure deletion.
    pub new_path: Option<String>,
    pub status: FileStatus,
    pub binary: bool,
    pub additions: u32,
    pub deletions: u32,
    pub hunks: Vec<Hunk>,
}

/// The whole answer of `GET /runs/<id>/diff/structured`.
#[derive(Serialize, Debug, Clone)]
pub(crate) struct StructuredDiff {
    /// The source ref as asked (or defaulted), verbatim.
    pub from_ref: String,
    /// The destination ref as asked (or defaulted), verbatim.
    pub to_ref: String,
    /// `from_ref` resolved to a commit SHA (best-effort, `None` if unresolvable).
    pub from_sha: Option<String>,
    /// `to_ref` resolved to a commit SHA.
    pub to_sha: Option<String>,
    /// `true` when the pair was compared with the three-dot (merge-base) form —
    /// the fork → tip default, identical to the LOC stat.
    pub three_dot: bool,
    pub files: Vec<FileDiff>,
    pub additions: u32,
    pub deletions: u32,
    pub files_changed: u32,
}

#[derive(Debug)]
pub(crate) enum GitError {
    /// `git` could not be spawned at all.
    Spawn(std::io::Error),
    /// `git` ran and failed; `stderr` verbatim.
    Failed { stderr: String },
}

impl GitError {
    /// The failure means one of the refs (or the repo) does not exist — the
    /// caller answers 404, not 500.
    pub(crate) fn is_unknown_revision(&self) -> bool {
        match self {
            GitError::Spawn(_) => false,
            GitError::Failed { stderr } => {
                stderr.contains("unknown revision")
                    || stderr.contains("not a git repository")
                    || stderr.contains("bad revision")
                    || stderr.contains("Not a valid object name")
                    || stderr.contains("invalid object name")
            }
        }
    }
}

impl std::fmt::Display for GitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GitError::Spawn(e) => write!(f, "git diff failed: {e}"),
            GitError::Failed { stderr } => write!(f, "git diff failed: {stderr}"),
        }
    }
}

/// A ref is safe to hand to `git` as a positional argument: non-empty, no
/// leading `-` (option injection), no whitespace or control characters.
pub(crate) fn is_safe_ref(r: &str) -> bool {
    !r.is_empty() && !r.starts_with('-') && !r.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// A repo-relative path is safe for `git show <ref>:<path>`: non-empty, no NUL,
/// no leading `-`, no absolute or parent-escaping components.
pub(crate) fn is_safe_path(p: &str) -> bool {
    !p.is_empty()
        && !p.starts_with('-')
        && !p.starts_with('/')
        && !p.contains('\0')
        && !p.split('/').any(|seg| seg == "..")
}

fn run_git(repo: &Path, args: &[&str]) -> Result<Vec<u8>, GitError> {
    run_git_env(repo, args, &[])
}

fn run_git_env(repo: &Path, args: &[&str], envs: &[(&str, &Path)]) -> Result<Vec<u8>, GitError> {
    let out = std::process::Command::new("git")
        .args(args)
        .envs(envs.iter().map(|(k, v)| (*k, *v)))
        .current_dir(repo)
        .output()
        .map_err(GitError::Spawn)?;
    if !out.status.success() {
        return Err(GitError::Failed {
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        });
    }
    Ok(out.stdout)
}

/// Resolve a ref to its commit SHA, `None` when it does not resolve.
pub(crate) fn rev_parse(repo: &Path, r: &str) -> Option<String> {
    let spec = format!("{r}^{{commit}}");
    run_git(repo, &["rev-parse", "--verify", "--quiet", &spec])
        .ok()
        .map(|b| String::from_utf8_lossy(&b).trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Resolve a ref to its commit SHA, else — a snapshot tree of the `worktree` ref
/// (#835) — to its tree id. `None` when it is neither.
pub(crate) fn rev_parse_object(repo: &Path, r: &str) -> Option<String> {
    rev_parse(repo, r).or_else(|| {
        let spec = format!("{r}^{{tree}}");
        run_git(repo, &["rev-parse", "--verify", "--quiet", &spec])
            .ok()
            .map(|b| String::from_utf8_lossy(&b).trim().to_string())
            .filter(|s| !s.is_empty())
    })
}

/// Snapshot the working tree of `worktree` as a **tree object** (#835): the
/// tracked files as they are on disk plus the untracked, non-ignored ones,
/// `.pdo/` left out like everywhere else. Returns the tree id.
///
/// Works on a *copy* of the worktree's index (`GIT_INDEX_FILE`), so the node's
/// own staging area is never touched; `git add -A` onto that copy then
/// `write-tree` writes only the blobs that are new to the object store (a
/// dangling tree `git gc` reclaims later). Reading the Run's diff thus never
/// commits, stages or otherwise moves anything the node can observe.
pub(crate) fn snapshot_worktree(worktree: &Path) -> Result<String, GitError> {
    let index = run_git(worktree, &["rev-parse", "--git-path", "index"])?;
    let index = String::from_utf8_lossy(&index).trim().to_string();
    let index = {
        let p = PathBuf::from(&index);
        if p.is_absolute() {
            p
        } else {
            worktree.join(p)
        }
    };
    let tmp = std::env::temp_dir().join(format!(
        "pdo-snapshot-{}-{}.index",
        std::process::id(),
        SNAPSHOT_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    // A worktree with nothing ever staged has no index file yet: start empty.
    if index.exists() {
        std::fs::copy(&index, &tmp).map_err(GitError::Spawn)?;
    }
    let env: &[(&str, &Path)] = &[("GIT_INDEX_FILE", tmp.as_path())];
    // Stage everything, then put `.pdo/` back to its `HEAD` state. Not a
    // `:(exclude).pdo/` pathspec: naming `.pdo` in the pathspec makes `git add`
    // treat it as explicitly requested, and it aborts ("paths are ignored by one
    // of your .gitignore files") when the target repo gitignores `.pdo/` — the
    // documented setup (ADR-0060). `git reset -- .pdo` on the copied index is a
    // no-op when nothing under `.pdo` was staged or is tracked.
    let result = run_git_env(
        worktree,
        &["-c", "core.quotepath=false", "add", "-A", "--", "."],
        env,
    )
    .and_then(|_| run_git_env(worktree, &["reset", "-q", "--", ".pdo"], env))
    .and_then(|_| run_git_env(worktree, &["write-tree"], env))
    .map(|b| String::from_utf8_lossy(&b).trim().to_string());
    let _ = std::fs::remove_file(&tmp);
    result
}

static SNAPSHOT_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The merge-base of `from_ref` and the worktree's `HEAD` — the left side of the
/// fork → worktree default (#835), so the source branch moving after the fork
/// never shows up as the Run's work (the three-dot semantics of [`compute`]).
pub(crate) fn merge_base_with_head(worktree: &Path, from_ref: &str) -> Result<String, GitError> {
    run_git(worktree, &["merge-base", from_ref, "HEAD"])
        .map(|b| String::from_utf8_lossy(&b).trim().to_string())
}

/// The structured diff of `from_ref` → the Run's working tree (#835): the
/// worktree is snapshotted as a tree ([`snapshot_worktree`]) and compared
/// two-dot against `from_ref` — or, when `three_dot`, against the merge-base
/// of `from_ref` and the worktree's `HEAD`, mirroring the fork → tip default.
/// `to_ref` is what the caller asked for (the `worktree` id), reported verbatim;
/// `to_sha` is the snapshot tree id.
pub(crate) fn compute_worktree(
    worktree: &Path,
    from_ref: &str,
    to_ref: &str,
    three_dot: bool,
) -> Result<StructuredDiff, GitError> {
    let snapshot = snapshot_worktree(worktree)?;
    let base = if three_dot {
        merge_base_with_head(worktree, from_ref)?
    } else {
        from_ref.to_string()
    };
    let mut d = compute(worktree, &base, &snapshot, false)?;
    d.from_ref = from_ref.to_string();
    d.from_sha = rev_parse(worktree, from_ref);
    d.to_ref = to_ref.to_string();
    d.to_sha = Some(snapshot);
    d.three_dot = three_dot;
    Ok(d)
}

/// Compute the structured diff of `from_ref` → `to_ref` in `repo`.
///
/// `three_dot` selects `from...to` (merge-base as the left side — the Run's
/// fork → tip default, mirroring `compute_run_loc`) versus a plain `from to`
/// (an arbitrary pair of Run refs, ADR-0067 §1). `.pdo/` is excluded in both
/// forms, like the raw endpoints and the LOC stat, so counted = shown.
pub(crate) fn compute(
    repo: &Path,
    from_ref: &str,
    to_ref: &str,
    three_dot: bool,
) -> Result<StructuredDiff, GitError> {
    let range = format!("{from_ref}...{to_ref}");
    let mut args: Vec<&str> = vec![
        "-c",
        "core.quotepath=false",
        "diff",
        "--no-color",
        "--no-ext-diff",
        "--find-renames",
    ];
    if three_dot {
        args.push(&range);
    } else {
        args.push(from_ref);
        args.push(to_ref);
    }
    args.extend(["--", ".", ":(exclude).pdo/"]);
    let stdout = run_git(repo, &args)?;
    let patch = String::from_utf8_lossy(&stdout);
    let files = parse_patch(&patch);
    let additions = files.iter().map(|f| f.additions).sum();
    let deletions = files.iter().map(|f| f.deletions).sum();
    let files_changed = files.len() as u32;
    Ok(StructuredDiff {
        from_ref: from_ref.to_string(),
        to_ref: to_ref.to_string(),
        from_sha: rev_parse_object(repo, from_ref),
        to_sha: rev_parse_object(repo, to_ref),
        three_dot,
        files,
        additions,
        deletions,
        files_changed,
    })
}

/// The parsed `git diff <from> <to> -- <path>` of **one** path, two-dot, for the
/// re-map of a review comment's anchor (#752, ADR-0067 §5): the caller walks its
/// hunks to follow a line from `from` to `to`. Renames are followed so a moved
/// file still maps. `Ok(vec![])` = identical content at both refs.
pub(crate) fn compute_path(
    repo: &Path,
    from_ref: &str,
    to_ref: &str,
    path: &str,
) -> Result<Vec<FileDiff>, GitError> {
    let stdout = run_git(
        repo,
        &[
            "-c",
            "core.quotepath=false",
            "diff",
            "--no-color",
            "--no-ext-diff",
            "--find-renames",
            "-U0",
            from_ref,
            to_ref,
            "--",
            path,
        ],
    )?;
    Ok(parse_patch(&String::from_utf8_lossy(&stdout)))
}

/// `git show <ref>:<path>` — the full content of one file at one ref, for the
/// context expansion of the Review page. Returns the raw bytes (a binary file
/// is a legitimate answer; the handler picks the content type).
pub(crate) fn file_at_ref(repo: &Path, r: &str, path: &str) -> Result<Vec<u8>, GitError> {
    let spec = format!("{r}:{path}");
    run_git(repo, &["-c", "core.quotepath=false", "show", &spec])
}

/// Parse a full unified `git diff` into per-file sections, in patch order.
/// `[]` for empty/whitespace input. Never fails: a chunk it cannot understand
/// still yields a `FileDiff` with empty paths.
pub(crate) fn parse_patch(raw: &str) -> Vec<FileDiff> {
    if raw.trim().is_empty() {
        return Vec::new();
    }
    let mut chunks: Vec<Vec<&str>> = Vec::new();
    for line in raw.split('\n') {
        if line.starts_with("diff --git ") {
            chunks.push(vec![line]);
        } else if let Some(cur) = chunks.last_mut() {
            cur.push(line);
        }
        // Preamble before the first `diff --git` is dropped.
    }
    chunks.iter().map(|c| parse_chunk(c)).collect()
}

fn parse_chunk(lines: &[&str]) -> FileDiff {
    let mut rename_from: Option<String> = None;
    let mut rename_to: Option<String> = None;
    let mut copy_from: Option<String> = None;
    let mut copy_to: Option<String> = None;
    // `Some(None)` = marker seen and it was /dev/null.
    let mut minus_path: Option<Option<String>> = None;
    let mut plus_path: Option<Option<String>> = None;
    let mut binary_old: Option<String> = None;
    let mut binary_new: Option<String> = None;
    let mut binary = false;
    let mut new_file_mode = false;
    let mut deleted_file_mode = false;
    let mut hunks: Vec<Hunk> = Vec::new();
    let mut additions = 0u32;
    let mut deletions = 0u32;
    // Running line counters for the current hunk.
    let mut old_no = 0u32;
    let mut new_no = 0u32;
    let mut in_hunk = false;

    for (i, line) in lines.iter().enumerate() {
        if i == 0 {
            continue; // the `diff --git` header, used only as a last-resort path
        }
        if in_hunk {
            if let Some(rest) = line.strip_prefix('+') {
                if let Some(h) = hunks.last_mut() {
                    h.lines.push(DiffLine {
                        kind: LineKind::Add,
                        content: rest.to_string(),
                        old_no: None,
                        new_no: Some(new_no),
                    });
                }
                new_no += 1;
                additions += 1;
                continue;
            }
            if let Some(rest) = line.strip_prefix('-') {
                if let Some(h) = hunks.last_mut() {
                    h.lines.push(DiffLine {
                        kind: LineKind::Del,
                        content: rest.to_string(),
                        old_no: Some(old_no),
                        new_no: None,
                    });
                }
                old_no += 1;
                deletions += 1;
                continue;
            }
            if let Some(rest) = line.strip_prefix(' ') {
                if let Some(h) = hunks.last_mut() {
                    h.lines.push(DiffLine {
                        kind: LineKind::Context,
                        content: rest.to_string(),
                        old_no: Some(old_no),
                        new_no: Some(new_no),
                    });
                }
                old_no += 1;
                new_no += 1;
                continue;
            }
            if line.starts_with('\\') {
                // `\ No newline at end of file` — not a line of either side.
                continue;
            }
            if line.is_empty() {
                // A trailing empty string from the final `\n` split; git never
                // emits an empty hunk line (context lines carry a leading space).
                continue;
            }
            // Anything else ends the hunk (a new `@@` is handled below).
            in_hunk = false;
        }

        if let Some(h) = parse_hunk_header(line) {
            old_no = h.old_start;
            new_no = h.new_start;
            hunks.push(h);
            in_hunk = true;
        } else if let Some(p) = line.strip_prefix("rename from ") {
            rename_from = decode_path(p);
        } else if let Some(p) = line.strip_prefix("rename to ") {
            rename_to = decode_path(p);
        } else if let Some(p) = line.strip_prefix("copy from ") {
            copy_from = decode_path(p);
        } else if let Some(p) = line.strip_prefix("copy to ") {
            copy_to = decode_path(p);
        } else if line.starts_with("new file mode ") {
            new_file_mode = true;
        } else if line.starts_with("deleted file mode ") {
            deleted_file_mode = true;
        } else if let Some(rest) = line.strip_prefix("--- ") {
            minus_path = Some(marker_path(rest, "a/"));
        } else if let Some(rest) = line.strip_prefix("+++ ") {
            plus_path = Some(marker_path(rest, "b/"));
        } else if *line == "GIT binary patch" {
            binary = true;
        } else if let Some(rest) = line.strip_prefix("Binary files ") {
            binary = true;
            if let Some(mid) = rest.strip_suffix(" differ") {
                if let Some((a, b)) = mid.split_once(" and ") {
                    binary_old = strip_prefix_or_null(a, "a/");
                    binary_new = strip_prefix_or_null(b, "b/");
                }
            }
        }
        // `index`, `similarity index`, mode lines: ignored.
    }

    // Path ladder: rename/copy → ---/+++ → Binary files → `diff --git` header.
    let (old_path, new_path) = if rename_from.is_some() || rename_to.is_some() {
        (rename_from.clone(), rename_to.clone())
    } else if copy_from.is_some() || copy_to.is_some() {
        (copy_from.clone(), copy_to.clone())
    } else if minus_path.is_some() || plus_path.is_some() {
        (minus_path.flatten(), plus_path.flatten())
    } else if binary_old.is_some() || binary_new.is_some() {
        (binary_old, binary_new)
    } else {
        let p = header_path(lines[0]);
        (p.clone(), p)
    };

    let is_rename = rename_from.is_some() || rename_to.is_some();
    let is_copy = !is_rename && (copy_from.is_some() || copy_to.is_some());
    let status = if is_rename {
        FileStatus::Renamed
    } else if is_copy {
        FileStatus::Copied
    } else if new_file_mode || old_path.is_none() {
        FileStatus::Added
    } else if deleted_file_mode || new_path.is_none() {
        FileStatus::Deleted
    } else {
        FileStatus::Modified
    };

    if binary {
        additions = 0;
        deletions = 0;
        hunks.clear();
    }

    FileDiff {
        old_path,
        new_path,
        status,
        binary,
        additions,
        deletions,
        hunks,
    }
}

/// `@@ -a[,b] +c[,d] @@[ header]`.
fn parse_hunk_header(line: &str) -> Option<Hunk> {
    let rest = line.strip_prefix("@@ -")?;
    let (ranges, header) = rest.split_once(" @@")?;
    let (old, new) = ranges.split_once(" +")?;
    let (old_start, old_lines) = parse_range(old)?;
    let (new_start, new_lines) = parse_range(new)?;
    Some(Hunk {
        old_start,
        old_lines,
        new_start,
        new_lines,
        header: header.strip_prefix(' ').unwrap_or(header).to_string(),
        lines: Vec::new(),
    })
}

fn parse_range(s: &str) -> Option<(u32, u32)> {
    match s.split_once(',') {
        Some((a, b)) => Some((a.parse().ok()?, b.parse().ok()?)),
        None => Some((s.parse().ok()?, 1)),
    }
}

/// The path of a `--- `/`+++ ` marker: strip git's trailing tab (paths with
/// spaces), C-unquote, `/dev/null` → `None`, drop the `a/`/`b/` prefix.
fn marker_path(rest: &str, prefix: &str) -> Option<String> {
    let rest = rest.strip_suffix('\t').unwrap_or(rest);
    let path = decode_path(rest)?;
    if path == "/dev/null" {
        return None;
    }
    Some(
        path.strip_prefix(prefix)
            .map(str::to_string)
            .unwrap_or(path),
    )
}

fn strip_prefix_or_null(token: &str, prefix: &str) -> Option<String> {
    let path = decode_path(token)?;
    if path == "/dev/null" {
        return None;
    }
    Some(
        path.strip_prefix(prefix)
            .map(str::to_string)
            .unwrap_or(path),
    )
}

/// Last resort for a chunk with neither markers nor rename lines (a mode-only
/// change): `diff --git a/X b/X` where both halves name the same path.
fn header_path(header: &str) -> Option<String> {
    let rest = header.strip_prefix("diff --git ")?;
    let bytes = rest.as_bytes();
    if bytes.len() % 2 == 0 {
        return None;
    }
    let mid = bytes.len() / 2;
    if bytes[mid] != b' ' {
        return None;
    }
    let a = &rest[..mid];
    let b = &rest[mid + 1..];
    let a = a.strip_prefix("a/")?;
    let b = b.strip_prefix("b/")?;
    if a == b {
        Some(a.to_string())
    } else {
        None
    }
}

/// C-unquote a git path token when it is wrapped in double quotes (the
/// `core.quotepath` output, or a path with control characters), else verbatim.
/// `None` only for an empty token.
fn decode_path(token: &str) -> Option<String> {
    if token.is_empty() {
        return None;
    }
    if token.len() >= 2 && token.starts_with('"') && token.ends_with('"') {
        return Some(c_unquote(&token[1..token.len() - 1]));
    }
    Some(token.to_string())
}

/// Octal escapes are raw UTF-8 bytes, so the bytes are collected first and
/// decoded once — a char-by-char replace would mojibake multi-byte codepoints.
fn c_unquote(inner: &str) -> String {
    let mut bytes: Vec<u8> = Vec::with_capacity(inner.len());
    let src = inner.as_bytes();
    let mut i = 0;
    while i < src.len() {
        let c = src[i];
        if c != b'\\' {
            bytes.push(c);
            i += 1;
            continue;
        }
        let Some(&n) = src.get(i + 1) else {
            break; // trailing lone backslash: dropped
        };
        if (b'0'..=b'7').contains(&n) {
            let mut j = i + 1;
            let mut oct = 0u32;
            let mut digits = 0;
            while j < src.len() && digits < 3 && (b'0'..=b'7').contains(&src[j]) {
                oct = oct * 8 + u32::from(src[j] - b'0');
                j += 1;
                digits += 1;
            }
            bytes.push((oct & 0xff) as u8);
            i = j;
            continue;
        }
        let mapped = match n {
            b'a' => Some(0x07),
            b'b' => Some(0x08),
            b'f' => Some(0x0c),
            b'n' => Some(0x0a),
            b'r' => Some(0x0d),
            b't' => Some(0x09),
            b'v' => Some(0x0b),
            b'"' => Some(0x22),
            b'\\' => Some(0x5c),
            _ => None,
        };
        match mapped {
            Some(b) => bytes.push(b),
            None => bytes.push(n),
        }
        i += 2;
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

#[cfg(test)]
mod tests {
    //! Migrated from `frontend/src/lib/parseUnifiedDiff.test.ts` (#748): the
    //! parser moved server-side, its cases came with it.
    use super::*;

    fn one(raw: &str) -> FileDiff {
        let files = parse_patch(raw);
        assert_eq!(files.len(), 1, "expected one file: {files:?}");
        files.into_iter().next().unwrap()
    }

    #[test]
    fn empty_and_whitespace_yield_no_files() {
        assert!(parse_patch("").is_empty());
        assert!(parse_patch("  \n\n\t").is_empty());
    }

    #[test]
    fn modified_file_with_counts_and_line_numbers() {
        let raw = "diff --git a/src/a.rs b/src/a.rs\nindex 111..222 100644\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1,3 +1,3 @@ fn ctx\n ctx\n-old a\n+new a\n ctx2\n";
        let f = one(raw);
        assert_eq!(f.old_path.as_deref(), Some("src/a.rs"));
        assert_eq!(f.new_path.as_deref(), Some("src/a.rs"));
        assert_eq!(f.status, FileStatus::Modified);
        assert!(!f.binary);
        assert_eq!((f.additions, f.deletions), (1, 1));
        assert_eq!(f.hunks.len(), 1);
        let h = &f.hunks[0];
        assert_eq!(
            (h.old_start, h.old_lines, h.new_start, h.new_lines),
            (1, 3, 1, 3)
        );
        assert_eq!(h.header, "fn ctx");
        assert_eq!(
            h.lines,
            vec![
                DiffLine {
                    kind: LineKind::Context,
                    content: "ctx".into(),
                    old_no: Some(1),
                    new_no: Some(1)
                },
                DiffLine {
                    kind: LineKind::Del,
                    content: "old a".into(),
                    old_no: Some(2),
                    new_no: None
                },
                DiffLine {
                    kind: LineKind::Add,
                    content: "new a".into(),
                    old_no: None,
                    new_no: Some(2)
                },
                DiffLine {
                    kind: LineKind::Context,
                    content: "ctx2".into(),
                    old_no: Some(3),
                    new_no: Some(3)
                },
            ]
        );
    }

    #[test]
    fn multiple_files_preserve_order() {
        let raw = [
            "diff --git a/src/a.rs b/src/a.rs",
            "--- a/src/a.rs",
            "+++ b/src/a.rs",
            "@@ -1 +1 @@",
            "-old a",
            "+new a",
            "diff --git a/src/b.rs b/src/b.rs",
            "new file mode 100644",
            "index 0000000..333",
            "--- /dev/null",
            "+++ b/src/b.rs",
            "@@ -0,0 +1,2 @@",
            "+line 1",
            "+line 2",
            "",
        ]
        .join("\n");
        let files = parse_patch(&raw);
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].new_path.as_deref(), Some("src/a.rs"));
        assert_eq!(files[1].new_path.as_deref(), Some("src/b.rs"));
        assert_eq!(files[1].status, FileStatus::Added);
        assert!(files[1].old_path.is_none());
        assert_eq!((files[1].additions, files[1].deletions), (2, 0));
        // A one-line range `@@ -0,0 +1,2 @@` numbers the new side from 1.
        assert_eq!(files[1].hunks[0].lines[0].new_no, Some(1));
        assert_eq!(files[1].hunks[0].lines[1].new_no, Some(2));
    }

    #[test]
    fn deletion_has_no_new_path() {
        let raw = "diff --git a/gone.rs b/gone.rs\ndeleted file mode 100644\n--- a/gone.rs\n+++ /dev/null\n@@ -1,2 +0,0 @@\n-a\n-b\n";
        let f = one(raw);
        assert_eq!(f.status, FileStatus::Deleted);
        assert_eq!(f.old_path.as_deref(), Some("gone.rs"));
        assert!(f.new_path.is_none());
        assert_eq!((f.additions, f.deletions), (0, 2));
    }

    #[test]
    fn pure_rename_without_hunks() {
        let raw = "diff --git a/old.rs b/new.rs\nsimilarity index 100%\nrename from old.rs\nrename to new.rs\n";
        let f = one(raw);
        assert_eq!(f.status, FileStatus::Renamed);
        assert_eq!(f.old_path.as_deref(), Some("old.rs"));
        assert_eq!(f.new_path.as_deref(), Some("new.rs"));
        assert_eq!((f.additions, f.deletions), (0, 0));
        assert!(f.hunks.is_empty());
    }

    #[test]
    fn rename_with_edits_keeps_counts() {
        let raw = "diff --git a/old.rs b/new.rs\nsimilarity index 90%\nrename from old.rs\nrename to new.rs\n--- a/old.rs\n+++ b/new.rs\n@@ -1 +1 @@\n-x\n+y\n";
        let f = one(raw);
        assert_eq!(f.status, FileStatus::Renamed);
        assert_eq!(f.new_path.as_deref(), Some("new.rs"));
        assert_eq!((f.additions, f.deletions), (1, 1));
    }

    #[test]
    fn copy_is_recognised() {
        let raw = "diff --git a/a.rs b/b.rs\nsimilarity index 100%\ncopy from a.rs\ncopy to b.rs\n";
        let f = one(raw);
        assert_eq!(f.status, FileStatus::Copied);
        assert_eq!(f.old_path.as_deref(), Some("a.rs"));
        assert_eq!(f.new_path.as_deref(), Some("b.rs"));
    }

    #[test]
    fn modified_binary_is_binary_with_zero_counts() {
        let raw = "diff --git a/img.png b/img.png\nindex 111..222 100644\nBinary files a/img.png and b/img.png differ\n";
        let f = one(raw);
        assert!(f.binary);
        assert_eq!(f.status, FileStatus::Modified);
        assert_eq!(f.new_path.as_deref(), Some("img.png"));
        assert_eq!((f.additions, f.deletions), (0, 0));
    }

    #[test]
    fn new_binary_is_added() {
        let raw = "diff --git a/img.png b/img.png\nnew file mode 100644\nindex 0000000..222\nBinary files /dev/null and b/img.png differ\n";
        let f = one(raw);
        assert!(f.binary);
        assert_eq!(f.status, FileStatus::Added);
        assert!(f.old_path.is_none());
        assert_eq!(f.new_path.as_deref(), Some("img.png"));
    }

    #[test]
    fn git_binary_patch_marker_is_binary() {
        let raw = "diff --git a/x.bin b/x.bin\nindex 111..222 100644\nGIT binary patch\nliteral 3\nKcmZQzWMKEfU|<7;00000\n\n";
        let f = one(raw);
        assert!(f.binary);
        assert_eq!(f.new_path.as_deref(), Some("x.bin"));
        assert!(f.hunks.is_empty());
    }

    #[test]
    fn path_with_spaces_uses_trailing_tab() {
        let raw = "diff --git a/my file.txt b/my file.txt\n--- a/my file.txt\t\n+++ b/my file.txt\t\n@@ -1 +1 @@\n-a\n+b\n";
        let f = one(raw);
        assert_eq!(f.new_path.as_deref(), Some("my file.txt"));
    }

    #[test]
    fn c_quoted_unicode_path_is_decoded() {
        // "é" = \303\251 in git's C-quoting.
        let raw = "diff --git \"a/caf\\303\\251.txt\" \"b/caf\\303\\251.txt\"\n--- \"a/caf\\303\\251.txt\"\n+++ \"b/caf\\303\\251.txt\"\n@@ -1 +1 @@\n-a\n+b\n";
        let f = one(raw);
        assert_eq!(f.new_path.as_deref(), Some("café.txt"));
    }

    #[test]
    fn no_newline_marker_is_not_counted() {
        let raw = "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1 +1 @@\n-a\n\\ No newline at end of file\n+b\n\\ No newline at end of file\n";
        let f = one(raw);
        assert_eq!((f.additions, f.deletions), (1, 1));
        assert_eq!(f.hunks[0].lines.len(), 2);
    }

    #[test]
    fn malformed_chunk_never_panics() {
        let f = one("diff --git nonsense\n@@ garbage @@\n+x\n");
        assert!(f.old_path.is_none() && f.new_path.is_none());
        // `+x` outside a recognised hunk is not counted.
        assert_eq!(f.additions, 0);
    }

    #[test]
    fn mode_only_change_takes_path_from_header() {
        let raw = "diff --git a/run.sh b/run.sh\nold mode 100644\nnew mode 100755\n";
        let f = one(raw);
        assert_eq!(f.new_path.as_deref(), Some("run.sh"));
        assert_eq!(f.status, FileStatus::Modified);
    }

    #[test]
    fn two_hunks_restart_numbering() {
        let raw = "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1,2 +1,2 @@\n a\n-b\n+B\n@@ -10,2 +10,3 @@\n j\n+K\n k\n";
        let f = one(raw);
        assert_eq!(f.hunks.len(), 2);
        assert_eq!(f.hunks[1].lines[0].old_no, Some(10));
        assert_eq!(f.hunks[1].lines[1].new_no, Some(11));
        assert_eq!(f.hunks[1].lines[2].old_no, Some(11));
        assert_eq!(f.hunks[1].lines[2].new_no, Some(12));
    }

    #[test]
    fn ref_and_path_safety() {
        assert!(is_safe_ref("pdo/run-abc"));
        assert!(is_safe_ref("a1b2c3^{commit}"));
        assert!(!is_safe_ref(""));
        assert!(!is_safe_ref("--output=/tmp/x"));
        assert!(!is_safe_ref("a b"));
        assert!(is_safe_path("src/lib.rs"));
        assert!(is_safe_path("my file.txt"));
        assert!(!is_safe_path("../etc/passwd"));
        assert!(!is_safe_path("/etc/passwd"));
        assert!(!is_safe_path("-x"));
        assert!(!is_safe_path(""));
    }

    #[test]
    fn compute_over_a_real_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(repo)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        git(&["init", "-q", "-b", "main"]);
        git(&["config", "user.email", "t@t"]);
        git(&["config", "user.name", "t"]);
        git(&["config", "commit.gpgsign", "false"]);
        std::fs::write(repo.join("a.txt"), "one\ntwo\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "-q", "-m", "c0"]);
        let c0 = git(&["rev-parse", "HEAD"]);
        git(&["checkout", "-q", "-b", "work"]);
        std::fs::write(repo.join("a.txt"), "one\nTWO\nthree\n").unwrap();
        std::fs::write(repo.join("b.txt"), "new\n").unwrap();
        std::fs::create_dir_all(repo.join(".pdo")).unwrap();
        std::fs::write(repo.join(".pdo/art.txt"), "blackboard\n").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "c1"]);
        let c1 = git(&["rev-parse", "HEAD"]);
        // Advance main after the fork: three-dot must not show it as a deletion.
        git(&["checkout", "-q", "main"]);
        std::fs::write(repo.join("main_only.txt"), "m\n").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "c2"]);
        let c2 = git(&["rev-parse", "HEAD"]);

        let d = compute(repo, "main", "work", true).unwrap();
        assert!(d.three_dot);
        // `from_sha` is the ref AS RESOLVED (main's tip), while the diff itself is
        // taken from the merge-base (c0) — the three-dot contract.
        assert_eq!(d.from_sha.as_deref(), Some(c2.as_str()));
        assert_eq!(d.to_sha.as_deref(), Some(c1.as_str()));
        assert_eq!(d.files_changed, 2, "{d:?}");
        let paths: Vec<_> = d
            .files
            .iter()
            .map(|f| f.new_path.clone().unwrap())
            .collect();
        assert_eq!(paths, vec!["a.txt", "b.txt"]);
        assert_eq!((d.additions, d.deletions), (3, 1));
        assert!(!paths.iter().any(|p| p.contains("main_only")));
        assert!(!paths.iter().any(|p| p.contains(".pdo")));

        // Two-dot on an explicit pair.
        let d2 = compute(repo, &c0, &c1, false).unwrap();
        assert!(!d2.three_dot);
        assert_eq!(d2.files_changed, 2);

        // Unknown ref → a 404-able error.
        let err = compute(repo, "main", "nope", true).unwrap_err();
        assert!(err.is_unknown_revision(), "{err}");

        // File at ref.
        let content = file_at_ref(repo, "work", "b.txt").unwrap();
        assert_eq!(content, b"new\n");
        let missing = file_at_ref(repo, "main", "b.txt").unwrap_err();
        assert!(missing.is_unknown_revision() || matches!(missing, GitError::Failed { .. }));
    }

    #[test]
    fn compute_worktree_sees_uncommitted_and_untracked_without_touching_the_index() {
        // #835: on a dirty tree, the worktree diff shows the modified tracked
        // file and the untracked one (not `.pdo/`, not the ignored file), the
        // node's own index stays as it was, and `git status` is unchanged.
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(repo)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        git(&["init", "-q", "-b", "main"]);
        git(&["config", "user.email", "t@t"]);
        git(&["config", "user.name", "t"]);
        git(&["config", "commit.gpgsign", "false"]);
        std::fs::write(repo.join("a.txt"), "one\ntwo\n").unwrap();
        std::fs::write(repo.join(".gitignore"), "ignored.txt\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "-q", "-m", "c0"]);
        let c0 = git(&["rev-parse", "HEAD"]);
        git(&["checkout", "-q", "-b", "work"]);
        // One commit on the run branch.
        std::fs::write(repo.join("committed.txt"), "c\n").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "c1"]);
        // Advance main after the fork: three-dot must not show it.
        git(&["checkout", "-q", "main"]);
        std::fs::write(repo.join("main_only.txt"), "m\n").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "c2"]);
        git(&["checkout", "-q", "work"]);
        // Then a dirty tree on top of the run branch.
        std::fs::write(repo.join("a.txt"), "one\nTWO\nthree\n").unwrap();
        std::fs::write(repo.join("untracked.txt"), "new\n").unwrap();
        std::fs::write(repo.join("ignored.txt"), "nope\n").unwrap();
        std::fs::create_dir_all(repo.join(".pdo")).unwrap();
        std::fs::write(repo.join(".pdo/art.txt"), "blackboard\n").unwrap();
        let status_before = git(&["status", "--porcelain"]);

        let d = compute_worktree(repo, "main", "worktree", true).unwrap();
        assert!(d.three_dot);
        assert_eq!(d.from_ref, "main");
        assert_eq!(d.to_ref, "worktree");
        let mut paths: Vec<_> = d
            .files
            .iter()
            .map(|f| f.new_path.clone().unwrap())
            .collect();
        paths.sort();
        assert_eq!(
            paths,
            vec!["a.txt", "committed.txt", "untracked.txt"],
            "{d:?}"
        );
        assert_eq!((d.additions, d.deletions), (4, 1));

        // The snapshot is a tree the object store knows: file-at-ref and a
        // two-dot diff against it work like for any ref (comments re-map, #752).
        let snapshot = d.to_sha.clone().unwrap();
        assert_eq!(
            rev_parse_object(repo, &snapshot).as_deref(),
            Some(snapshot.as_str())
        );
        assert!(rev_parse(repo, &snapshot).is_none(), "a tree, not a commit");
        assert_eq!(
            file_at_ref(repo, &snapshot, "untracked.txt").unwrap(),
            b"new\n"
        );
        let only_dirty = compute_path(repo, "HEAD", &snapshot, "a.txt").unwrap();
        assert_eq!(only_dirty.len(), 1);

        // Nothing moved for the node: same status, same index, same HEAD.
        assert_eq!(git(&["status", "--porcelain"]), status_before);
        assert_eq!(git(&["diff", "--cached", "--name-only"]), "");
        assert_ne!(git(&["rev-parse", "HEAD"]), c0);

        // Two-dot from an explicit ref: main's advance shows up as a deletion.
        let d2 = compute_worktree(repo, "main", "worktree", false).unwrap();
        assert!(!d2.three_dot);
        assert!(d2
            .files
            .iter()
            .any(|f| f.old_path.as_deref() == Some("main_only.txt")));

        // A clean tree snapshots to HEAD's own tree: fork → worktree == fork → tip.
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "c3"]);
        let clean = compute_worktree(repo, "main", "worktree", true).unwrap();
        assert_eq!(clean.to_sha, rev_parse_object(repo, "HEAD^{tree}"));
        assert_eq!(
            clean.files_changed,
            compute(repo, "main", "work", true).unwrap().files_changed
        );
    }

    #[test]
    fn snapshot_worktree_works_when_the_target_repo_gitignores_the_pdo_directory() {
        // #835 FP finding: `git add -A -- . ':(exclude).pdo/'` aborts when
        // `.pdo/` is gitignored as a directory — the documented target-repo
        // setup (ADR-0060). The snapshot must succeed, leave `.pdo/` out, and
        // keep a `.pdo` path that *is* tracked at HEAD exactly as HEAD has it.
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(repo)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        git(&["init", "-q", "-b", "main"]);
        git(&["config", "user.email", "t@t"]);
        git(&["config", "user.name", "t"]);
        git(&["config", "commit.gpgsign", "false"]);
        std::fs::write(repo.join("a.txt"), "one\n").unwrap();
        std::fs::write(repo.join(".gitignore"), ".pdo/\ntarget/\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "-q", "-m", "c0"]);
        std::fs::write(repo.join("a.txt"), "one\nTWO\n").unwrap();
        std::fs::write(repo.join("untracked.txt"), "new\n").unwrap();
        std::fs::create_dir_all(repo.join(".pdo/runs")).unwrap();
        std::fs::write(repo.join(".pdo/runs/art.txt"), "blackboard\n").unwrap();
        let status_before = git(&["status", "--porcelain"]);

        let d = compute_worktree(repo, "main", "worktree", true).unwrap();
        let mut paths: Vec<_> = d
            .files
            .iter()
            .map(|f| f.new_path.clone().unwrap())
            .collect();
        paths.sort();
        assert_eq!(paths, vec!["a.txt", "untracked.txt"], "{d:?}");
        assert_eq!(git(&["status", "--porcelain"]), status_before);

        // Same rule when `.pdo` is *not* ignored: it stays out of the snapshot.
        std::fs::write(repo.join(".gitignore"), "target/\n").unwrap();
        let d = compute_worktree(repo, "main", "worktree", true).unwrap();
        let mut paths: Vec<_> = d
            .files
            .iter()
            .map(|f| f.new_path.clone().unwrap())
            .collect();
        paths.sort();
        assert_eq!(paths, vec![".gitignore", "a.txt", "untracked.txt"], "{d:?}");

        // A `.pdo` path tracked at HEAD is snapshotted as HEAD has it — neither
        // dropped nor updated from the disk.
        std::fs::write(repo.join(".pdo/pipeline.yaml"), "v1\n").unwrap();
        git(&["add", "-f", ".pdo/pipeline.yaml"]);
        git(&["commit", "-q", "-m", "c1"]);
        std::fs::write(repo.join(".pdo/pipeline.yaml"), "v2\n").unwrap();
        let snapshot = snapshot_worktree(repo).unwrap();
        assert_eq!(
            file_at_ref(repo, &snapshot, ".pdo/pipeline.yaml").unwrap(),
            b"v1\n"
        );
        assert!(file_at_ref(repo, &snapshot, ".pdo/runs/art.txt").is_err());
    }
}
