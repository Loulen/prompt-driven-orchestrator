//! Where a harness's transcripts live on disk (#473, #585, #615, #707): the
//! filesystem side of the transcript capability.
//!
//! The domain declares *how* a harness's store is laid out
//! ([`TranscriptResolution`], [`SubagentLayout`]); this adapter is the one place
//! that reads the directories. A harness that declares no resolution resolves
//! nothing, never `claude`'s `<uuid>.jsonl` path (ADR-0051).

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::domain::claude_transcript::encode_working_dir;
use crate::domain::pi_session;
use crate::domain::probes::{resolved, SubagentLayout, TranscriptResolution};

/// Resolve `harness`'s transcript file, dispatched on its declared
/// [`TranscriptResolution`] (ADR-0051). A data-declared harness resolves `None`.
///
/// - `claude`: by the pinned session id when the node recorded one
///   (`<uuid>.jsonl`, #473), else the legacy newest-mtime pick for a pre-#473 row;
/// - `copilot`: `<store>/<session-id>/events.jsonl`, by the identity PDO imposed,
///   ignoring the working directory (#615) — `None` without an id;
/// - `pi`: `<store>/<encoded-cwd>/*_<session-id>.jsonl` (#707) — `None` without
///   an id (pi never blind-continues).
pub fn resolve_transcript(
    harness: &str,
    projects_root: &Path,
    working_dir: &Path,
    session_id: Option<&str>,
) -> Option<PathBuf> {
    match resolved(harness).transcript_resolution()? {
        TranscriptResolution::ClaudeJsonl => match session_id {
            Some(sid) => session_jsonl_by_id(projects_root, working_dir, sid),
            None => find_session_jsonl(projects_root, working_dir),
        },
        TranscriptResolution::CopilotEventsJsonl => {
            let sid = session_id.filter(|s| !s.is_empty())?;
            Some(projects_root.join(sid).join("events.jsonl"))
        }
        TranscriptResolution::PiJsonlById => {
            pi_session_by_id(projects_root, working_dir, session_id?)
        }
    }
}

/// `harness`'s subagent transcripts for one main session, as `(file_stem,
/// transcript_text)` pairs, dispatched on its declared [`SubagentLayout`]
/// (#585). Every harness without one answers an empty `Vec` (a value, not a
/// silently-skipped branch); `stats_performance` applies its own declared-group
/// labelling to whatever this returns.
pub fn subagent_transcripts(
    harness: &str,
    project_root: &Path,
    working_dir: &Path,
    session_id: &str,
) -> Vec<(String, String)> {
    let mut out = Vec::new();
    match resolved(harness).subagent_layout() {
        Some(SubagentLayout::ClaudeSessionSubagentsDir) => {
            let dir = project_root
                .join(encode_working_dir(working_dir))
                .join(session_id)
                .join("subagents");
            collect_jsonl_stems(&dir, &mut out);
        }
        None => {}
    }
    out
}

/// Resolve a node's transcript by its **pinned session id** (#473): Claude Code
/// names the transcript file `<session_id>.jsonl` under the encoded-cwd project
/// dir, so an exact-name lookup returns *this node's own* transcript regardless of
/// how many other `.jsonl` files share the dir.
///
/// This is the fix for the manager-vs-node collision: a non-isolated
/// node's cwd is the Run worktree, which is also the manager's cwd (and every
/// sibling non-CM node's), so one CC project dir holds several `.jsonl` and
/// [`find_session_jsonl`]'s newest-mtime pick returns whichever was touched last —
/// usually the manager's. Resolving by the id PDO pinned at spawn
/// (`claude --session-id <uuid>`) is immune to that.
///
/// `projects_root` is the same #408 seam as [`find_session_jsonl`] (staging root
/// for a live sandboxed Run, `~/.claude/projects/` otherwise); the cwd encoding
/// stays the single source of truth (#373). Returns `None` when the file does not
/// exist yet (a session that has not written its transcript), which the sweep
/// treats as "no signal".
/// `pub(crate)` since #613/ADR-0051: part of **`claude`'s** transcript resolution
/// ([`TranscriptResolution::ClaudeJsonl`]); a consumer dispatches through
/// [`resolve_transcript`].
pub fn session_jsonl_by_id(
    projects_root: &Path,
    working_dir: &Path,
    session_id: &str,
) -> Option<PathBuf> {
    let encoded = encode_working_dir(working_dir);
    let path = projects_root
        .join(encoded)
        .join(format!("{session_id}.jsonl"));
    path.is_file().then_some(path)
}

/// Find the most recently modified `.jsonl` file for `working_dir` under the
/// given Claude Code `projects/` root.
///
/// **Legacy fallback since #473.** The turn-end probe now resolves by pinned
/// session id ([`session_jsonl_by_id`]) whenever the node recorded one; this
/// newest-mtime resolution survives only for a node started before #473 (no id in
/// its `NodeStarted`), where it is exactly the pre-#473 behaviour — including its
/// known collision with the manager's transcript, which no historical node can now
/// avoid but every new node does.
///
/// `projects_root` is the seam that lets a sandboxed Run's transcripts be read
/// from its staged home while it is live (#408): the caller resolves it via
/// `pdo_sandbox::adapters::staging::stores::transcripts_root` (staging for a live sandboxed Run,
/// `~/.claude/projects/` otherwise). The cwd encoding stays here, the single
/// source of truth (#373) — the seam only swaps the base root.
pub fn find_session_jsonl(projects_root: &Path, working_dir: &Path) -> Option<PathBuf> {
    let encoded = encode_working_dir(working_dir);
    newest_jsonl_in(&projects_root.join(encoded))
}

fn newest_jsonl_in(dir: &Path) -> Option<PathBuf> {
    if !dir.is_dir() {
        return None;
    }

    let mut newest: Option<(PathBuf, SystemTime)> = None;

    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let mtime = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .unwrap_or(SystemTime::UNIX_EPOCH);

        match &newest {
            Some((_, best_time)) if mtime <= *best_time => {}
            _ => newest = Some((path, mtime)),
        }
    }

    newest.map(|(p, _)| p)
}

/// Resolve the session file of the identity PDO imposed: the one `*_<session_id>.jsonl`
/// in `<store_root>/<pi_session::session_dir_name(working_dir)>/`. `None` when the directory or
/// the file does not exist yet (a node that has not written a first entry), or when
/// `session_id` is empty. Should pi ever leave two files for one id (it does not:
/// `--session-id` is created-or-resumed), the newest by mtime wins.
pub fn pi_session_by_id(
    store_root: &Path,
    working_dir: &Path,
    session_id: &str,
) -> Option<PathBuf> {
    if session_id.is_empty() {
        return None;
    }
    let dir = store_root.join(pi_session::session_dir_name(working_dir));
    let suffix = format!("_{session_id}.jsonl");
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in std::fs::read_dir(&dir).ok()?.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.ends_with(&suffix) {
            continue;
        }
        let mtime = entry
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(std::time::UNIX_EPOCH);
        if best.as_ref().is_none_or(|(t, _)| mtime > *t) {
            best = Some((mtime, path));
        }
    }
    best.map(|(_, p)| p)
}

/// Recurse `dir`, pairing every `*.jsonl` file's stem with its text — the raw
/// discovery step behind [`subagent_transcripts`]. No grouping
/// heuristic lives here: labelling a stem as a declared group or falling back to
/// "Unidentified subagent" is `stats_performance`'s own concern, not a
/// harness capability.
fn collect_jsonl_stems(dir: &Path, out: &mut Vec<(String, String)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl_stems(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if let Ok(text) = std::fs::read_to_string(&path) {
                out.push((stem.to_string(), text));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    use crate::domain::registry::{CLAUDE, COPILOT, OPENCODE, PI};

    const TURN_ENDED: &str = include_str!("../../tests/fixtures/pi-0.85.1/turn_ended.jsonl");
    const IN_TOOL_CALL: &str = include_str!("../../tests/fixtures/pi-0.85.1/in_tool_call.jsonl");

    // --- find_session_jsonl (filesystem) ---
    //
    // Since #408 `find_session_jsonl` takes the `projects/` root as a param (the
    // observability seam), so these tests inject a tempdir root directly — no HOME
    // swap, no crate-wide HOME lock, fully hermetic.

    #[test]
    fn find_jsonl_returns_newest_file() {
        let tmp = tempfile::tempdir().unwrap();
        let projects = tmp.path().join(".claude").join("projects");

        let encoded = encode_working_dir(Path::new("/home/user/project"));
        let projects_dir = projects.join(&encoded);
        std::fs::create_dir_all(&projects_dir).unwrap();

        let old_file = projects_dir.join("old-session.jsonl");
        std::fs::write(&old_file, "old").unwrap();
        filetime::set_file_mtime(
            &old_file,
            filetime::FileTime::from_system_time(SystemTime::now() - Duration::from_secs(300)),
        )
        .unwrap();

        let new_file = projects_dir.join("new-session.jsonl");
        std::fs::write(&new_file, "new").unwrap();

        let result = find_session_jsonl(&projects, Path::new("/home/user/project"));

        assert!(result.is_some());
        assert_eq!(result.unwrap().file_name().unwrap(), "new-session.jsonl");
    }

    #[test]
    fn find_jsonl_no_dir_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        let projects = tmp.path().join(".claude").join("projects");
        assert!(find_session_jsonl(&projects, Path::new("/nonexistent/dir")).is_none());
    }

    #[test]
    fn find_jsonl_ignores_non_jsonl_files() {
        let tmp = tempfile::tempdir().unwrap();
        let projects = tmp.path().join(".claude").join("projects");

        let encoded = encode_working_dir(Path::new("/tmp/testdir"));
        let projects_dir = projects.join(&encoded);
        std::fs::create_dir_all(&projects_dir).unwrap();

        std::fs::write(projects_dir.join("notes.txt"), "not jsonl").unwrap();
        std::fs::write(projects_dir.join("data.json"), "not jsonl either").unwrap();

        assert!(find_session_jsonl(&projects, Path::new("/tmp/testdir")).is_none());
    }

    #[test]
    fn find_jsonl_resolves_a_real_pdo_node_dir() {
        // #373 regression: a representative PDO node working dir (absolute,
        // carries `.pdo`) must resolve to the transcript CC actually writes —
        // i.e. under the leading-dash, `--pdo` name. Pre-fix this looked up
        // `home-...-.pdo-...` and found nothing, so the mtime probe was dead.
        let tmp = tempfile::tempdir().unwrap();
        let projects = tmp.path().join(".claude").join("projects");

        let node_dir = Path::new(
            "/home/user/code/pdo/.pdo/runs/20260623-100032-9b8331b/nodes/gzpYZA2m/iter-1",
        );

        // The transcript dir CC writes: leading `-`, `.pdo` → `--pdo`.
        let cc_name = projects
            .join("-home-user-code-pdo--pdo-runs-20260623-100032-9b8331b-nodes-gzpYZA2m-iter-1");
        std::fs::create_dir_all(&cc_name).unwrap();
        std::fs::write(cc_name.join("session.jsonl"), "{}").unwrap();

        // The encoder now produces exactly that name …
        assert_eq!(projects.join(encode_working_dir(node_dir)), cc_name);
        // … so the probe resolves the transcript.
        let found = find_session_jsonl(&projects, node_dir);
        assert!(
            found.is_some(),
            "find_session_jsonl must resolve a real PDO node transcript after the #373 fix"
        );
        assert_eq!(found.unwrap().file_name().unwrap(), "session.jsonl");
    }

    // --- session_jsonl_by_id (#473: resolve by pinned identity, not mtime) ---

    #[test]
    fn session_jsonl_by_id_resolves_the_exact_named_file() {
        let tmp = tempfile::tempdir().unwrap();
        let projects = tmp.path().join(".claude").join("projects");
        let wd = Path::new("/home/user/project");
        let dir = projects.join(encode_working_dir(wd));
        std::fs::create_dir_all(&dir).unwrap();

        let sid = "11111111-2222-3333-4444-555555555555";
        std::fs::write(dir.join(format!("{sid}.jsonl")), "{}").unwrap();

        let found = session_jsonl_by_id(&projects, wd, sid).expect("must resolve the pinned id");
        assert_eq!(found.file_name().unwrap(), format!("{sid}.jsonl").as_str());
    }

    #[test]
    fn session_jsonl_by_id_missing_file_is_none() {
        let tmp = tempfile::tempdir().unwrap();
        let projects = tmp.path().join(".claude").join("projects");
        // The dir may not even exist yet (session hasn't written a transcript).
        assert!(
            session_jsonl_by_id(&projects, Path::new("/home/user/project"), "no-such-id").is_none()
        );
    }

    /// **The #473 bug, as a red/green contrast.** A single shared CC project dir
    /// (the Run worktree, shared by the manager and a non-CM node) holds two
    /// transcripts: the node's pinned `<uuid>.jsonl` (older) and the manager's
    /// (newer). `find_session_jsonl` — the pre-#473 resolution — returns the
    /// manager's (newest mtime); `session_jsonl_by_id` returns the node's own.
    #[test]
    fn session_jsonl_by_id_ignores_a_newer_sibling_transcript() {
        let tmp = tempfile::tempdir().unwrap();
        let projects = tmp.path().join(".claude").join("projects");
        // A representative shared worktree cwd.
        let wd = Path::new("/home/u/.pdo/runs/20260101-120000-abc/worktree");
        let dir = projects.join(encode_working_dir(wd));
        std::fs::create_dir_all(&dir).unwrap();

        let node_sid = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
        let node_file = dir.join(format!("{node_sid}.jsonl"));
        std::fs::write(&node_file, "node").unwrap();
        filetime::set_file_mtime(
            &node_file,
            filetime::FileTime::from_system_time(SystemTime::now() - Duration::from_secs(600)),
        )
        .unwrap();

        // The manager's transcript, in the SAME dir, touched more recently.
        let manager_file = dir.join("00000000-0000-0000-0000-000000000000.jsonl");
        std::fs::write(&manager_file, "manager").unwrap();

        // Pre-#473: newest-mtime pick returns the MANAGER's file — the bug.
        assert_eq!(
            find_session_jsonl(&projects, wd)
                .unwrap()
                .file_name()
                .unwrap(),
            "00000000-0000-0000-0000-000000000000.jsonl",
            "the legacy resolution picks the newest sibling (the manager) — this is the bug"
        );
        // #473: identity resolution returns the NODE's own transcript.
        assert_eq!(
            session_jsonl_by_id(&projects, wd, node_sid)
                .unwrap()
                .file_name()
                .unwrap(),
            format!("{node_sid}.jsonl").as_str(),
            "resolving by pinned id must return this node's transcript, not the newest sibling"
        );
    }

    #[test]
    fn resolve_by_id_globs_the_imposed_identity_inside_the_cwd_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let store = tmp.path().join("sessions");
        let wd = Path::new("/work/shared-worktree");
        let dir = store.join(pi_session::session_dir_name(wd));
        std::fs::create_dir_all(&dir).unwrap();
        // Two nodes in the same worktree: two files, one directory (#473 has no
        // equivalent here — the identity is in the file name).
        let a = dir.join("2026-09-05T15-07-33-921Z_sid-a.jsonl");
        let b = dir.join("2026-09-05T15-07-34-000Z_sid-b.jsonl");
        std::fs::write(&a, TURN_ENDED).unwrap();
        std::fs::write(&b, IN_TOOL_CALL).unwrap();
        std::fs::write(dir.join("notes.txt"), "not a session").unwrap();
        assert_eq!(pi_session_by_id(&store, wd, "sid-a"), Some(a));
        assert_eq!(pi_session_by_id(&store, wd, "sid-b"), Some(b));
        // Unknown id, empty id, unknown cwd: none — never a newest-mtime guess.
        assert_eq!(pi_session_by_id(&store, wd, "sid-c"), None);
        assert_eq!(pi_session_by_id(&store, wd, ""), None);
        assert_eq!(
            pi_session_by_id(&store, Path::new("/elsewhere"), "sid-a"),
            None
        );
        // A prefix match is not an identity match.
        assert_eq!(pi_session_by_id(&store, wd, "a"), None);
    }

    #[test]
    fn pi_resolves_its_session_by_identity_inside_the_cwd_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let store = tmp.path().join("sessions");
        let wd = Path::new("/shared/wt");
        let dir = store.join(pi_session::session_dir_name(wd));
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("2026-09-05T15-00-00-000Z_sid-a.jsonl");
        let b = dir.join("2026-09-05T15-00-01-000Z_sid-b.jsonl");
        std::fs::write(&a, "").unwrap();
        std::fs::write(&b, "").unwrap();
        assert_eq!(resolve_transcript(PI, &store, wd, Some("sid-a")), Some(a));
        assert_eq!(resolve_transcript(PI, &store, wd, Some("sid-b")), Some(b));
        // No pinned identity ⇒ nothing to resolve (pi never blind-continues).
        assert!(resolve_transcript(PI, &store, wd, None).is_none());
        // And never claude's `<uuid>.jsonl` under an encoded cwd.
        assert!(resolve_transcript(PI, &store, wd, Some("missing")).is_none());
    }

    #[test]
    fn copilot_resolves_its_journal_by_session_identity_ignoring_cwd() {
        // #615 AC: resolved from the imposed session identity, no cwd encoding — so
        // two nodes sharing a worktree get distinct journals.
        let resolve =
            |sid| resolve_transcript(COPILOT, Path::new("/store"), Path::new("/shared/wt"), sid);
        let a = resolve(Some("sid-a")).unwrap();
        let b = resolve(Some("sid-b")).unwrap();
        assert_eq!(a, PathBuf::from("/store/sid-a/events.jsonl"));
        assert_eq!(b, PathBuf::from("/store/sid-b/events.jsonl"));
        assert_ne!(a, b, "distinct sessions ⇒ distinct journals, same worktree");
        // No pinned identity ⇒ no journal (copilot never blind-continues).
        assert!(resolve(None).is_none());
    }

    #[test]
    fn subagent_transcripts_dispatches_only_claude_to_its_directory_convention() {
        let tmp = tempfile::tempdir().unwrap();
        let project_root = tmp.path().join("projects");
        let working_dir = Path::new("/home/user/project");
        let encoded = encode_working_dir(working_dir);
        let subagents_dir = project_root.join(&encoded).join("sid-1").join("subagents");
        std::fs::create_dir_all(&subagents_dir).unwrap();
        std::fs::write(
            subagents_dir.join("reviewer.jsonl"),
            r#"{"type":"assistant","message":{"id":"m2","model":"claude-opus-4-8","usage":{"input_tokens":10,"output_tokens":5}}}"#,
        )
        .unwrap();

        // `claude` discovers the file, stem verbatim (grouping is the caller's
        // job, not this dispatch's — see the trait method's doc comment).
        let claude_found = subagent_transcripts(CLAUDE, &project_root, working_dir, "sid-1");
        assert_eq!(claude_found.len(), 1);
        assert_eq!(claude_found[0].0, "reviewer");

        // `copilot` has no nested-subagent convention: the SAME directory
        // existing on disk is never picked up for it — a motivated absence
        // (the dispatch answers empty), never a silently-skipped branch.
        let copilot_found = subagent_transcripts(COPILOT, &project_root, working_dir, "sid-1");
        assert!(copilot_found.is_empty());

        // A data-declared harness: empty too, from the trait default.
        assert!(subagent_transcripts(OPENCODE, &project_root, working_dir, "sid-1").is_empty());
    }

    #[test]
    fn a_data_declared_harness_resolves_no_transcript() {
        for name in [OPENCODE, "my-custom-harness"] {
            assert!(
                resolve_transcript(name, Path::new("/proj"), Path::new("/wd"), Some("abc"))
                    .is_none(),
                "{name}: no transcript resolution leaks from claude"
            );
        }
    }
}
