//! Liveness sweep policy for the nodes of a live Run.
//!
//! **Session death is the only verdict of death (#469, ADR-0032).** For an agent
//! node the death of the agent *is* the death of the tmux session by
//! construction: `tmux_session_manager::wrap_with_env` emits
//! `exec bash -c '<exports> && <tail>'` and `build_agent_tail` emits
//! `exec claude …`, so the `claude` process **is** the pane leader of the
//! session's only window. It exits → the pane dies → the session dies →
//! `session_alive == false`. Same in a sandbox: the pane carries the
//! `docker exec` client, which returns as soon as claude exits inside the
//! container. `remain-on-exit` is never armed.
//!
//! That fact is why there is no idle threshold here any more. A transcript-mtime
//! proxy adds nothing to death detection — what it catches *beyond*
//! [`Detection::SessionDied`] is exclusively the agent that is **alive but
//! silent**, i.e. "wedged" or "not progressing", and a `docker build` or a
//! `cargo test --workspace` is indistinguishable from either. Measured on a real
//! 679-record transcript of a healthy node: five silent gaps of 155 s, 185 s,
//! 214 s, 270 s and 291 s. The old 120 s threshold was not mis-calibrated, it was
//! structurally incapable, and one false positive cost a whole Run.
//!
//! What the sweep still does, beyond death:
//! - **usage-limit menu** (#290): observability only, the node stays `Running`;
//! - **turn-end auto-completion** (#469 §2): opt-in, and keyed on a *constated
//!   end of turn* ([`parse_turn_state`]) — never on a duration.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::event_log::{self, EventKind, NodeStatus};
use crate::outputs_validator;
use crate::pipeline;

/// How often the background sweep wakes up. Surfaced by `GET /stale/health`
/// (#251) and mirrors `trigger_scheduler::TICK_INTERVAL_SECS`.
///
/// Since #469 this is the sweep's *only* time constant on the liveness path:
/// there is no idle age that "counts as stale" for it to be compared against.
pub const STALE_TICK_INTERVAL_SECS: u64 = 30;

/// How many trailing bytes of a node's Claude Code transcript the turn-end probe
/// reads (#469 §2). Never the whole file: a long node's `.jsonl` runs to
/// megabytes and the sweep visits every live node every
/// [`STALE_TICK_INTERVAL_SECS`].
///
/// A window that clips mid-record is a *designed-for* case, not a bug:
/// [`parse_turn_state`] skips every unparseable line and answers
/// [`TurnState::Unknown`] when nothing substantial survives — and `Unknown`
/// behaves as "at work", so a clipped read can only ever be conservative.
pub const TRANSCRIPT_TAIL_BYTES: u64 = 256 * 1024;

/// Anti-bounce window before a constated end of turn may be acted on (#469 §2).
///
/// The transcript mtime survives the removal of the idle threshold in **this one
/// role and no other**: it is not an oracle for "is the agent alive", it only
/// keeps the sweep from racing a write still in flight (a final `assistant`
/// record whose successor is being appended as we read). A clock-skewed *future*
/// mtime counts as not-quiet — conservative by construction.
pub const TURN_END_QUIET_PERIOD: Duration = Duration::from_secs(60);

/// Env seam for the turn-end auto-completion setting (#469 §4, ADR-0015).
/// Middle tier of `stored → env → default(false)`; the resolver is
/// [`autocomplete_turn_end_with`].
pub const AUTOCOMPLETE_TURN_END_ENV: &str = "PDO_AUTOCOMPLETE_TURN_END";

/// Built-in default for turn-end auto-completion: **off**.
///
/// Load-bearing (ADR-0012, autonomy is earned): a terminal action the runtime
/// initiates on its own must be opted into. With it off, the sweep performs
/// exactly one `session_exists` per live node and reads no transcript at all —
/// strictly cheaper than the pre-#469 path, which paid a `read_dir` plus an
/// outputs validation per node per tick.
pub const AUTOCOMPLETE_TURN_END_DEFAULT: bool = false;

/// Parse a stored/env boolean flag. `None` for anything unrecognised, so a typo
/// falls through to the next precedence tier instead of silently meaning `false`.
pub fn parse_bool_setting(raw: &str) -> Option<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

/// The `env` tier of the turn-end auto-completion setting (#469 §4).
pub fn env_autocomplete_turn_end() -> Option<bool> {
    std::env::var(AUTOCOMPLETE_TURN_END_ENV)
        .ok()
        .as_deref()
        .and_then(parse_bool_setting)
}

/// Resolve turn-end auto-completion: `stored → env → default(false)` (#469 §4,
/// ADR-0015).
///
/// `stored` is the raw `instance_config.autocomplete_turn_end` column: `Some(0)`
/// is a stored **off** and wins over the env, exactly like a stored `0` would for
/// any other knob; only SQL `NULL` (`None`) falls through.
pub fn autocomplete_turn_end_with(stored: Option<i64>) -> bool {
    match stored {
        Some(v) => v != 0,
        None => env_autocomplete_turn_end().unwrap_or(AUTOCOMPLETE_TURN_END_DEFAULT),
    }
}

/// On-screen anchors for Claude Code's usage-limit interactive menu (#290).
///
/// The menu wording is NOT officially documented and DRIFTS across CC versions
/// (corroborated by anthropics/claude-code#28484 + a direct capture 2026-06-30).
/// These are the substrings observed most stable; match is case-insensitive after
/// ANSI-stripping + whitespace-collapsing. Detection is best-effort /
/// observability-only (#290 Slice 1): a miss is the status quo (no regression), a
/// false positive is one harmless informational event. UPDATE THIS LIST when CC
/// wording changes.
const USAGE_LIMIT_ANCHORS: &[&str] = &[
    "stop and wait for limit to reset",
    "stop and wait for the limit to reset",
];

/// Strip ANSI/CSI escape sequences from a tmux pane capture (which is taken with
/// `-e`, so it contains escapes). Char-safe: preserves multi-byte UTF-8 (e.g. the
/// menu's `❯`). Best-effort — handles CSI (`ESC [ … final @-~`) and drops a lone
/// escape's next char; good enough for pane text (mostly SGR colour codes).
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                while let Some(&nc) = chars.peek() {
                    chars.next();
                    if matches!(nc, '\u{40}'..='\u{7e}') {
                        break; // CSI final byte
                    }
                }
            } else {
                chars.next();
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// True if the captured pane shows Claude Code's usage-limit interactive menu.
/// `pane` is raw tmux capture (may contain ANSI). Observability-only (#290): the
/// caller flags the node but never changes its fate.
///
/// `pub(crate)` since #613/ADR-0051: this is **`claude`'s** usage-limit
/// implementation ([`crate::harness_probes::HarnessProbes::detect_usage_limit`]),
/// not a generic matcher — a consumer dispatches through
/// [`crate::harness_probes::usage_limit_shown`], never calling this directly.
pub(crate) fn detect_usage_limit(pane: &str) -> bool {
    // Whitespace is collapsed so a line-wrap / padding can't split an anchor.
    let stripped = strip_ansi(pane).to_ascii_lowercase();
    let norm = stripped.split_whitespace().collect::<Vec<_>>().join(" ");
    USAGE_LIMIT_ANCHORS.iter().any(|a| norm.contains(a))
}

/// What the sweep concluded about one live node.
///
/// There is deliberately **no `Stale` variant** (#469, ADR-0032): a verdict that
/// meant "alive but idle past N seconds" produced false positives that were
/// terminal *and* irrecoverable — the node latched out of the probe set, its
/// later `node_done` was refused by the completion guard, and
/// `reconcile_run_level_stall` failed the whole Run within the same sweep. The
/// `NodeStale` **event** and `NodeStatus::Stale` survive for historical Runs (the
/// log is append-only), but nothing in the daemon emits them any more.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Detection {
    /// The node's tmux session is gone. The **only** verdict of death.
    SessionDied,
    /// The agent is alive and has visibly finished its turn with valid outputs,
    /// and turn-end auto-completion is enabled (#469 §2). Produced by
    /// [`assess_node`], never by [`decide`]: it needs two probes past liveness.
    TurnEnded,
    /// Nothing to do. Includes every "alive but not progressing" shape —
    /// mid-tool-call, wedged on an interactive prompt, API retries exhausted.
    Ok,
}

/// Pure liveness decision: session alive or not (#469 §1).
///
/// This *is* the whole of it now. `session_alive == false` is the single
/// authority on death (see the module docs for why the double `exec` makes the
/// agent's exit and the session's death the same event); everything else the
/// sweep does hangs off [`Detection::Ok`] in [`assess_node`].
pub fn decide(session_alive: bool) -> Detection {
    if session_alive {
        Detection::Ok
    } else {
        Detection::SessionDied
    }
}

/// Where an agent's transcript says it is, right now (#469 §2).
///
/// Exactly one variant is actionable ([`Self::TurnEnded`]); the other three all
/// mean "leave it alone", for three different reasons worth keeping distinct in
/// logs and tests.
///
/// A *substantial* record is one carrying a `message` object whose `role` is
/// `assistant` or `user`. That definition is load-bearing: a naive "look at the
/// last line" reads one of Claude Code's trailing metadata records
/// (`last-prompt`, `ai-title`, `mode`, `permission-mode` — none of which even
/// carry a `timestamp`) and concludes nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnState {
    /// A `tool_use` block has no matching `tool_result`: the agent is *inside* a
    /// tool call and alive, however long the silence has lasted. This is the
    /// state a `docker build` or a `cargo test --workspace` sits in, and the one
    /// the old mtime threshold could not tell from death.
    InToolCall,
    /// The last substantial record is a `user` message (a prompt, or a
    /// `tool_result`): the assistant still owes a reply. **This is the
    /// API-retries-exhausted shape (#251)** — it must never be completed.
    AwaitingAssistant,
    /// The last substantial record is an `assistant` message and no `tool_use` is
    /// pending: the turn is over. The only actionable state.
    TurnEnded,
    /// No transcript, nothing parseable, or a single record overrunning the read
    /// window (a large `tool_result`). Behaves as "at work": with the signal
    /// absent nothing is touched. Fail-safe by construction.
    Unknown,
}

/// Role of a substantial transcript record. Private: only the last one matters
/// to [`parse_turn_state`], and only to distinguish two of its four answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecordRole {
    Assistant,
    User,
}

/// Classify the tail of a Claude Code `.jsonl` transcript into a [`TurnState`]
/// (#469 §2). Pure — the caller does the read ([`read_transcript_tail`]).
///
/// One forward pass over the lines:
/// 1. every `tool_use` block seen in an `assistant` message opens an id;
/// 2. every `tool_result` block seen in a `user` message closes its
///    `tool_use_id`;
/// 3. an id still open at the end ⇒ [`TurnState::InToolCall`], **checked first**
///    — the record that opened it is itself an `assistant` message, so testing
///    the last role first would misread a pending tool call as a finished turn;
/// 4. otherwise the last substantial role decides.
///
/// Unparseable lines are skipped, which is what makes a byte-clipped tail safe:
/// the leading partial record is simply not there. A `tool_result` whose
/// `tool_use` fell outside the window closes nothing (harmless); a `tool_use`
/// whose `tool_result` fell outside stays open (conservative).
///
/// The JSONL layout is not a documented contract — same caution as the #290 pane
/// anchors — though `tool_use` / `tool_result` blocks and their `id`s are its
/// most stable part, far ahead of a menu's wording.
///
/// `pub(crate)` since #613/ADR-0051: this is **`claude`'s** end-of-turn parser
/// ([`crate::harness_probes::HarnessProbes::classify_turn_ended`]) — a consumer
/// dispatches through [`crate::harness_probes::turn_ended`], never reading another
/// harness's store with it.
pub(crate) fn parse_turn_state(tail: &str) -> TurnState {
    let mut open_tool_uses: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut last_role: Option<RecordRole> = None;

    for line in tail.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(record) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let Some(message) = record.get("message") else {
            continue;
        };
        let role = match message.get("role").and_then(|r| r.as_str()) {
            Some("assistant") => RecordRole::Assistant,
            Some("user") => RecordRole::User,
            _ => continue,
        };
        last_role = Some(role);

        // `content` is an array of blocks in the modern format and a bare string
        // in the older one; only the array can carry tool blocks.
        if let Some(blocks) = message.get("content").and_then(|c| c.as_array()) {
            for block in blocks {
                match block.get("type").and_then(|t| t.as_str()) {
                    Some("tool_use") => {
                        if let Some(id) = block.get("id").and_then(|i| i.as_str()) {
                            open_tool_uses.insert(id.to_string());
                        }
                    }
                    Some("tool_result") => {
                        if let Some(id) = block.get("tool_use_id").and_then(|i| i.as_str()) {
                            open_tool_uses.remove(id);
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    if !open_tool_uses.is_empty() {
        return TurnState::InToolCall;
    }
    match last_role {
        Some(RecordRole::Assistant) => TurnState::TurnEnded,
        Some(RecordRole::User) => TurnState::AwaitingAssistant,
        None => TurnState::Unknown,
    }
}

/// Whether the transcript has been quiet long enough for a constated end of turn
/// to be acted on (#469 §2 anti-bounce). See [`TURN_END_QUIET_PERIOD`] for why
/// the mtime survives *only* in this role.
fn quiet_long_enough(mtime: SystemTime, now: SystemTime) -> bool {
    now.duration_since(mtime).unwrap_or(Duration::ZERO) >= TURN_END_QUIET_PERIOD
}

/// Encode a working directory path exactly as Claude Code names its
/// `~/.claude/projects/` directory: every non-`[A-Za-z0-9]` char maps to `-`
/// (case preserved, runs NOT collapsed). So a leading `/` becomes a leading `-`
/// and `.pdo`/`.claude` become `--pdo`/`--claude`.
///
/// Example: `/home/user/project` → `-home-user-project`; a PDO node dir like
/// `/home/u/.pdo/runs/X/worktree` → `-home-u--pdo-runs-X-worktree`.
///
/// This is the single source of truth for the CC project-dir encoding;
/// [`crate::run_cost::cc_project_dirname`] delegates here.
///
/// Don't "clean up" the encoding (strip the leading `/`, keep `.` intact): that
/// resolved `None` for *every* PDO node, silently killing the transcript probe
/// (#373).
pub fn encode_working_dir(dir: &Path) -> String {
    dir.to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
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
/// ([`crate::harness_probes::HarnessProbes::resolve_transcript`]); a consumer
/// dispatches through [`crate::harness_probes::resolve_transcript`].
pub(crate) fn session_jsonl_by_id(
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
/// [`crate::sandbox_run::transcripts_root`] (staging for a live sandboxed Run,
/// `~/.claude/projects/` otherwise). The cwd encoding stays here, the single
/// source of truth (#373) — the seam only swaps the base root.
pub(crate) fn find_session_jsonl(projects_root: &Path, working_dir: &Path) -> Option<PathBuf> {
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

/// The trailing slice of a node's transcript plus the mtime of that same file
/// (#469 §2).
///
/// One probe returning both, deliberately: the anti-bounce
/// ([`TURN_END_QUIET_PERIOD`]) must be evaluated against the *file we just
/// tailed*. A second `jsonl_mtime()` probe could resolve a different newest
/// `.jsonl` between the two calls and let a stale quiet-check greenlight a fresh
/// tail. It is also what makes "setting off ⇒ zero transcript I/O" provable: one
/// method, never called.
#[derive(Debug, Clone)]
pub struct TranscriptTail {
    /// Last [`TRANSCRIPT_TAIL_BYTES`] bytes, lossily decoded. May begin
    /// mid-record — [`parse_turn_state`] is built for that.
    pub text: String,
    /// mtime of the tailed file. The anti-bounce clock, **not** an activity
    /// oracle: Claude Code writes untimestamped metadata records that bump it
    /// without any agent activity behind them.
    pub mtime: SystemTime,
}

/// Read the last [`TRANSCRIPT_TAIL_BYTES`] of `path` together with its mtime.
///
/// `None` on any I/O failure (absent file, unreadable) — which
/// [`assess_node`] treats as "no signal", i.e. leave the node alone. Seeks rather
/// than reading the file whole, so a multi-megabyte transcript costs one
/// bounded read per live node per sweep.
pub fn read_transcript_tail(path: &Path) -> Option<TranscriptTail> {
    let mut file = std::fs::File::open(path).ok()?;
    let meta = file.metadata().ok()?;
    let mtime = meta.modified().ok()?;
    let len = meta.len();
    let start = len.saturating_sub(TRANSCRIPT_TAIL_BYTES);
    if start > 0 {
        file.seek(SeekFrom::Start(start)).ok()?;
    }
    let mut buf = Vec::with_capacity((len - start) as usize);
    file.take(TRANSCRIPT_TAIL_BYTES)
        .read_to_end(&mut buf)
        .ok()?;
    Some(TranscriptTail {
        // Lossy: the seek can land mid-codepoint, and a replacement char only
        // ever breaks the JSON of the leading partial record, which is skipped.
        text: String::from_utf8_lossy(&buf).into_owned(),
        mtime,
    })
}

/// Validate outputs for a node using the pipeline definition.
///
/// Since #469 this is consulted **only** behind a constated
/// [`TurnState::TurnEnded`] — never behind a duration. It is the second of the
/// two independent guards on auto-completion: it is what stops an agent that
/// ended its turn to *ask a question* from being completed, since its outputs
/// are still incomplete.
pub fn validate_outputs(
    pipeline_path: &Path,
    node_id: &str,
    iter: i64,
    artifacts_dir: &Path,
) -> bool {
    let yaml = match std::fs::read_to_string(pipeline_path) {
        Ok(y) => y,
        Err(_) => return false,
    };
    let pipeline_def = match pipeline::parse_pipeline(&yaml) {
        Ok(p) => p.pipeline,
        Err(_) => return false,
    };

    outputs_validator::validate(&pipeline_def, node_id, iter, artifacts_dir).is_ok()
}

/// Build events for a detection result.
///
/// Only [`Detection::SessionDied`] has an event of its own here.
/// [`Detection::TurnEnded`] deliberately produces **none** (#469 §3): appending a
/// `NodeAutoCompleted` straight into the log was the defect of the old design —
/// on an isolated node it would record a `Completed` whose commit
/// stayed on the `pdo/sub-…` branch, with the downstream receiving nothing. Its
/// terminal event is appended by the *shared node-completion body*, past the
/// forgotten-run refusal, the completion guard and
/// `commit_and_merge_sub_worktree_inner`.
pub fn detection_events(
    detection: &Detection,
    run_id: &str,
    node_id: &str,
    iter: i64,
) -> Vec<event_log::Event> {
    // The session-died cause names the dead tmux session (#213 AC1) so the
    // incident is self-explanatory in the UI/log. Since résilience (ADR-0049)
    // this is a `NodeInterrupted`, not a `NodeFailed`: "la session est morte,
    // pas le travail" — the run parks `AwaitingUser` (derived in `finalize`),
    // never `Failed`, and a human resumes or restarts it.
    let (kind, reason) = match detection {
        Detection::Ok | Detection::TurnEnded => return vec![],
        Detection::SessionDied => {
            let session = crate::tmux_session_manager::node_session_name(run_id, node_id, iter);
            (
                EventKind::NodeInterrupted,
                format!("session_died: tmux session {session} no longer exists"),
            )
        }
    };

    vec![event_log::Event {
        id: None,
        run_id: run_id.to_string(),
        ts: event_log::now_iso(),
        kind,
        node_id: Some(node_id.to_string()),
        iter: Some(iter),
        payload: Some(serde_json::json!({ "reason": reason })),
    }]
}

/// Injected I/O for [`assess_node`]. The impure sweep layer ([`crate::lib`])
/// implements this against tmux + the filesystem for one running node; unit
/// tests supply a fake so the whole probe → gate → decide → events →
/// diagnostics → dedup pipeline runs without a daemon.
///
/// Every method is a side-effect-free *read*: the reap/spawn side effects stay
/// in the sweep, keyed off [`Assessment::detection`].
pub trait NodeProbes {
    /// Is the node's tmux session still alive? The one probe on the default path
    /// — see [`decide`].
    fn session_alive(&self) -> bool;

    /// Trailing slice + mtime of *this node's* Claude Code transcript, or `None`
    /// when nothing resolves (#469 §2).
    ///
    /// #473: the implementation resolves by the session id PDO pinned at spawn
    /// ([`session_jsonl_by_id`]) — the transcript named `<uuid>.jsonl`, this node's
    /// own — and only falls back to the newest-mtime pick ([`find_session_jsonl`])
    /// for a pre-#473 node with no recorded id. Without that, a
    /// non-isolated node shares its cwd (the Run worktree) with the
    /// manager's `claude`, so the newest `.jsonl` was usually the manager's and the
    /// turn-end verdict was read off the wrong conversation.
    ///
    /// Called **only** when turn-end auto-completion is enabled. With the setting
    /// off this is never invoked, which is the whole of "unchecked ⇒ no transcript
    /// read" and is asserted directly through this seam.
    fn transcript_tail(&self) -> Option<TranscriptTail>;

    /// Do the node's declared outputs validate against the pipeline? The second
    /// of the two independent auto-completion guards, consulted **only** behind a
    /// constated [`TurnState::TurnEnded`] — so a healthy node never pays for it.
    fn outputs_valid(&self) -> bool;

    /// Best-effort capture of the node's tmux pane, for the usage-limit menu
    /// probe (#290). Only called on the `Ok` path (an alive node).
    fn capture_pane(&self) -> Option<String>;

    /// Best-effort session-death forensics (#234). Gathered lazily — only when
    /// the session is found dead — so no tmux/proc I/O runs on a healthy node.
    fn session_death_diagnostics(&self) -> SessionDeathDiagnostics;
}

/// Outcome of [`assess_node`]: the raw detection plus the events to append and
/// the observability the sweep needs. The impure caller appends
/// [`Self::events`] and runs any reap/spawn side effects keyed off
/// [`Self::detection`].
///
/// Not `PartialEq`/`Eq`: [`event_log::Event`]'s payload is a
/// `serde_json::Value` (no `Eq`), and callers/tests inspect the fields
/// individually rather than compare a whole `Assessment`.
#[derive(Debug, Clone)]
pub struct Assessment {
    /// The verdict the sweep drives its side effects off: `SessionDied` reaps,
    /// `TurnEnded` runs the shared node-completion body (#469 §3), `Ok` does
    /// nothing.
    pub detection: Detection,
    /// Events to append. A `SessionDied` failure already carries its
    /// diagnostics; a usage-limit menu carries a (deduped) `NodeBlockedOnLimit`.
    /// Empty for a nominal `Ok` node — and for `TurnEnded`, whose terminal event
    /// belongs to the shared completion body, not to the sweep.
    pub events: Vec<event_log::Event>,
    /// True when the node is alive but its pane shows Claude Code's usage-limit
    /// menu — feeds the per-sweep `blocked_on_limit` gauge (#290). Set on every
    /// sweep the menu is visible, independent of event dedup.
    pub blocked_on_limit: bool,
    /// The session-death forensics gathered on the `SessionDied` path (`None`
    /// otherwise), surfaced so the sweep can log the structured fields (#234)
    /// without re-running the probe or re-parsing the event payload.
    pub session_death_diagnostics: Option<SessionDeathDiagnostics>,
}

/// True when an event of `kind` for `(node_id, iter)` already exists in
/// `prior_events` — the rising-edge de-dup key for the informational
/// `NodeBlockedOnLimit` marker. Pure over the event log snapshot the sweep
/// already loaded, so a held condition emits one event, not one per ~30 s sweep
/// tick, and the dedup survives a daemon restart.
fn episode_has_event(
    prior_events: &[event_log::Event],
    kind: &EventKind,
    node_id: &str,
    iter: i64,
) -> bool {
    prior_events
        .iter()
        .any(|e| &e.kind == kind && e.node_id.as_deref() == Some(node_id) && e.iter == Some(iter))
}

fn informational_event(
    kind: EventKind,
    run_id: &str,
    node_id: &str,
    iter: i64,
    payload: serde_json::Value,
) -> event_log::Event {
    event_log::Event {
        id: None,
        run_id: run_id.to_string(),
        ts: event_log::now_iso(),
        kind,
        node_id: Some(node_id.to_string()),
        iter: Some(iter),
        payload: Some(payload),
    }
}

/// The liveness-sweep policy for a single running node, with all I/O injected
/// via `probes`. This is the one place the whole pipeline lives, so
/// [`crate::lib`]'s sweep is reduced to a loop that builds a [`NodeProbes`]
/// adapter, calls this, appends [`Assessment::events`], and runs the
/// reap / complete side effects keyed off [`Assessment::detection`].
///
/// `autocomplete_turn_end` is the resolved instance setting
/// ([`autocomplete_turn_end_with`]), read once per sweep by the caller. When it
/// is `false` this function **short-circuits at the head of the `Ok` path**:
/// neither [`NodeProbes::transcript_tail`] nor [`NodeProbes::outputs_valid`] is
/// invoked, so the default path costs one `session_exists` plus the #290 pane
/// capture and nothing else.
///
/// The two guards on `TurnEnded` are independent and both mandatory:
/// [`TurnState::TurnEnded`] (the agent visibly finished) **and** valid outputs
/// (it finished *the work*, not just its turn — an agent that stopped to ask a
/// question fails the second). The anti-bounce
/// ([`TURN_END_QUIET_PERIOD`]) sits in front of both so a write in flight is
/// never raced.
///
/// Note on #290: a node wedged on the usage-limit menu cannot be auto-completed
/// here even though it is silent — the limit is hit while *requesting* the next
/// assistant message, so its last substantial record is a `user`/`tool_result`
/// and [`parse_turn_state`] answers [`TurnState::AwaitingAssistant`]. "Blocked"
/// is never "finished", by construction rather than by a third guard.
///
/// `prior_events` is the run's event-log snapshot, used purely for the
/// rising-edge de-dup of `NodeBlockedOnLimit` (see [`episode_has_event`]).
// The probe set + policy toggles are all distinct facts a sweep tick needs; the
// same posture as the four `tmux_session_manager` builders that allow this lint.
#[allow(clippy::too_many_arguments)]
pub fn assess_node(
    probes: &impl NodeProbes,
    prior_events: &[event_log::Event],
    run_id: &str,
    node_id: &str,
    iter: i64,
    now: SystemTime,
    autocomplete_turn_end: bool,
    harness: &str,
) -> Assessment {
    // #613/ADR-0051: the two probes are dispatch points, not presence guards. The
    // gate reads the resolved harness's declared capabilities, and the detection
    // itself is dispatched to that harness's implementation — `claude`'s JSONL
    // parser / pane anchor for `claude`, a data-declared harness's own (or nothing).
    let caps = crate::harness_probes::capabilities(harness);
    let detection = decide(probes.session_alive());

    if detection == Detection::SessionDied {
        let mut events = detection_events(&detection, run_id, node_id, iter);
        let mut diag = probes.session_death_diagnostics();
        // #615/ADR-0052: a harness that exits 0 on a hard failure (`copilot`) leaves
        // the verdict in its journal, not its exit code. Only such a harness pays a
        // tail read on death (gated on `exit_code_is_verdict` being false) — so
        // `claude`, whose death is its own signal, keeps short-circuiting every
        // probe. When the tail trails on a hard error, name it in the death
        // diagnostics, so the `NodeFailed` payload says WHY, not just "session died".
        if !crate::harness_probes::exit_code_is_verdict(harness) && diag.harness_error.is_none() {
            diag.harness_error = probes
                .transcript_tail()
                .and_then(|tail| crate::harness_probes::hard_error(harness, &tail.text));
        }
        attach_diagnostics(&mut events, &diag);
        return Assessment {
            detection,
            events,
            blocked_on_limit: false,
            session_death_diagnostics: Some(diag),
        };
    }

    // Alive, but maybe wedged on Claude Code's usage-limit menu (#290):
    // observability only — the node keeps running. The gauge counts every sweep
    // the menu is visible; the event is emitted once per (node, iter) episode.
    //
    // #553: the menu anchor is proper to a harness. Gate the probe on the
    // capability — a harness without it never has its pane captured for a menu
    // whose wording belongs to another harness (ANDed first, so no pane I/O runs).
    let blocked_on_limit = caps.usage_limit
        && probes
            .capture_pane()
            .is_some_and(|pane| crate::harness_probes::usage_limit_shown(harness, &pane));
    let events = if blocked_on_limit
        && !episode_has_event(prior_events, &EventKind::NodeBlockedOnLimit, node_id, iter)
    {
        vec![informational_event(
            EventKind::NodeBlockedOnLimit,
            run_id,
            node_id,
            iter,
            serde_json::json!({ "signal": "usage_limit_menu" }),
        )]
    } else {
        vec![]
    };

    // #469 §2: turn-end auto-completion. Opt-in, and the ONLY path here that can
    // end a live node's iteration. `claude --dangerously-skip-permissions
    // "<prompt>"` does not exit at the end of a turn — it stays in the REPL — so
    // an agent that finished without calling `pdo complete` is alive and
    // motionless, and this is its positive signature.
    //
    // #553: the turn-end substrate is a capability. Gate the probe on it — a
    // harness without it is never auto-completed on an invented heuristic (its
    // store is not the claude JSONL `parse_turn_state` reads). ANDed BEFORE the
    // transcript read, so an un-instrumented harness pays no transcript I/O.
    let turn_ended = autocomplete_turn_end
        && caps.turn_end
        && probes.transcript_tail().is_some_and(|tail| {
            quiet_long_enough(tail.mtime, now)
                && crate::harness_probes::turn_ended(harness, &tail.text)
        })
        && probes.outputs_valid();

    Assessment {
        detection: if turn_ended {
            Detection::TurnEnded
        } else {
            detection
        },
        events,
        blocked_on_limit,
        session_death_diagnostics: None,
    }
}

/// Best-effort diagnostic context captured the moment a node's session is found
/// dead (#234). Without this the daemon records only the *symptom*
/// (`session_died: tmux session … no longer exists`) and every occurrence
/// becomes a from-scratch forensic investigation; with it the operator can tell
/// "one session died" from "the whole tmux server collapsed" on first sight.
///
/// Every field is best-effort: a `None` (or `0` for [`Self::correlated_deaths`])
/// means the probe could not run or found nothing, never a confirmed negative
/// — the impure sweep layer ([`crate::lib`]) does the tmux/proc I/O, this struct
/// only carries the result and shapes it into the payload.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionDeathDiagnostics {
    /// `tmux -L <socket> ls` result: `Some(false)` means the whole server is
    /// gone (every session under the socket died at once — the #234 root cause),
    /// `Some(true)` means only this one session vanished, `None` = probe failed.
    pub tmux_server_alive: Option<bool>,
    /// `MemAvailable` from `/proc/meminfo` at detection time, in KiB.
    pub mem_available_kb: Option<u64>,
    /// `SwapFree` from `/proc/meminfo` at detection time, in KiB.
    pub swap_free_kb: Option<u64>,
    /// How many *other* running nodes in the same run were also found
    /// session-dead in this sweep. A non-zero count points at a server-wide
    /// collapse (multiple runs dying ~ms apart) rather than an isolated death.
    pub correlated_deaths: usize,
    /// The **hard error** the node's harness journal carries (#615, ADR-0052), if
    /// any. Set for a harness that **exits 0 on a hard failure** (`copilot`): the
    /// exit code is not a verdict, so PDO reads the failure off the journal and
    /// names it here, in the `NodeFailed` payload, instead of reporting only the
    /// symptom ("session died"). `None` for `claude` (whose death is its own
    /// signal) and for any harness whose journal shows no trailing error.
    pub harness_error: Option<String>,
}

impl SessionDeathDiagnostics {
    /// Shape the diagnostics into the JSON object attached to the `NodeFailed`
    /// payload alongside `reason`. Pure — no I/O. `None` fields serialise to
    /// `null` so a missing probe is distinguishable from a real value.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "tmux_server_alive": self.tmux_server_alive,
            "mem_available_kb": self.mem_available_kb,
            "swap_free_kb": self.swap_free_kb,
            "correlated_deaths": self.correlated_deaths,
            "harness_error": self.harness_error,
        })
    }
}

/// Fold session-death diagnostics into the `NodeFailed` event(s) built by
/// [`detection_events`], adding a `diagnostics` object alongside `reason`.
///
/// Pure: the impure sweep gathers the diagnostics (tmux/proc reads) then calls
/// this. A no-op for any event whose payload is missing or not a JSON object,
/// so the non-`SessionDied` detections (which carry no diagnostics) are never
/// touched even if this is mistakenly called on them.
pub fn attach_diagnostics(events: &mut [event_log::Event], diag: &SessionDeathDiagnostics) {
    for event in events.iter_mut() {
        if let Some(obj) = event.payload.as_mut().and_then(|p| p.as_object_mut()) {
            obj.insert("diagnostics".to_string(), diag.to_json());
        }
    }
}

/// Parse the contents of `/proc/meminfo`, returning `(MemAvailable, SwapFree)`
/// in KiB. Either is `None` when its line is absent or unparseable. Pure, so
/// the impure sweep layer only performs the file read.
pub fn parse_meminfo(contents: &str) -> (Option<u64>, Option<u64>) {
    // Lines look like `MemAvailable:    1234 kB`; take the first numeric token
    // after the `<key>:` prefix. The trailing `:` keeps `MemAvailable` from
    // matching `MemFree`/`MemTotal` and `SwapFree` from matching `SwapTotal`.
    let field = |key: &str| -> Option<u64> {
        contents.lines().find_map(|line| {
            let rest = line.strip_prefix(key)?;
            rest.split_whitespace().next()?.parse().ok()
        })
    };
    (field("MemAvailable:"), field("SwapFree:"))
}

/// Count how many of `running` (other than `self_node`) are session-dead,
/// according to `is_dead`. Pure given the predicate so the counting logic is
/// testable without tmux; the impure sweep passes a closure backed by
/// `tmux_session_manager::session_exists`.
pub fn count_correlated_deaths(
    running: &[(String, i64)],
    self_node: (&str, i64),
    is_dead: impl Fn(&str, i64) -> bool,
) -> usize {
    running
        .iter()
        .filter(|(id, it)| (id.as_str(), *it) != self_node)
        .filter(|(id, it)| is_dead(id, *it))
        .count()
}

pub fn running_nodes(run_state: &event_log::RunState) -> Vec<(String, i64)> {
    run_state
        .nodes
        .iter()
        .filter(|(_, ns)| ns.status == NodeStatus::Running)
        .map(|(id, ns)| (id.clone(), ns.iter))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    // --- detect_usage_limit / strip_ansi (#290) ---

    #[test]
    fn detects_usage_limit_menu_with_ansi() {
        // A realistic capture: the selected line carries SGR colour codes and the
        // `❯` cursor glyph, wrapped by the surrounding menu text.
        let pane = "What do you want to do?\n\x1b[2m❯\x1b[0m 1. Stop and \
                    wait for limit to reset\n  2. Switch to usage credits\n";
        assert!(detect_usage_limit(pane));
    }

    #[test]
    fn detects_the_the_variant() {
        assert!(detect_usage_limit(
            "…please wait for the limit to reset. Stop and wait for the limit to reset\n"
        ));
    }

    #[test]
    fn case_insensitive() {
        assert!(detect_usage_limit("STOP AND WAIT FOR LIMIT TO RESET"));
    }

    #[test]
    fn wrapped_anchor_still_matches() {
        // The anchor split by a newline + padding (as a narrow pane would wrap it)
        // must still match — proves the whitespace-collapse normalisation.
        assert!(detect_usage_limit(
            "stop and wait\n   for   limit\nto reset"
        ));
    }

    #[test]
    fn normal_running_pane_is_not_flagged() {
        let pane = "\x1b[2m✻\x1b[0m Thinking… (esc to interrupt)\n\
                    ● Running: cargo test -p pdo-daemon\n";
        assert!(!detect_usage_limit(pane));
    }

    #[test]
    fn empty_pane_is_not_flagged() {
        assert!(!detect_usage_limit(""));
    }

    #[test]
    fn strip_ansi_preserves_unicode() {
        assert_eq!(strip_ansi("\x1b[1m❯\x1b[0m x"), "❯ x");
    }

    // --- encode_working_dir (#373: matches Claude Code's real scheme) ---

    #[test]
    fn encode_basic_path_keeps_leading_dash() {
        // Every non-alphanumeric maps to `-`, so a leading `/` becomes `-`.
        assert_eq!(
            encode_working_dir(Path::new("/home/user/project")),
            "-home-user-project"
        );
    }

    #[test]
    fn encode_root() {
        assert_eq!(encode_working_dir(Path::new("/")), "-");
    }

    #[test]
    fn encode_deeply_nested() {
        assert_eq!(encode_working_dir(Path::new("/a/b/c/d/e")), "-a-b-c-d-e");
    }

    #[test]
    fn encode_maps_dot_to_dash() {
        // #373 root cause: a real PDO node dir carries `.pdo` (→ `--pdo`) and a
        // leading `-`. Before the fix this produced `home-...-.pdo-...` and
        // resolved nothing under ~/.claude/projects.
        assert_eq!(
            encode_working_dir(Path::new("/home/u/.pdo/runs/X/worktree")),
            "-home-u--pdo-runs-X-worktree"
        );
    }

    #[test]
    fn encode_matches_cc_project_dirname() {
        // The two encoders are unified on one implementation (#373): they must
        // never drift again.
        for dir in [
            "/home/llenoir/Documents/perso/Maestro/.pdo/runs/2026-abc/nodes/n1/iter-1",
            "/home/u/.claude",
            "/tmp/x.y.z",
        ] {
            assert_eq!(
                encode_working_dir(Path::new(dir)),
                crate::run_cost::cc_project_dirname(Path::new(dir)),
                "encode_working_dir and cc_project_dirname must agree for {dir}"
            );
        }
    }

    // --- decide (pure logic) — liveness, and nothing else (#469 §1) ---

    #[test]
    fn dead_session_returns_session_died() {
        assert_eq!(decide(false), Detection::SessionDied);
    }

    #[test]
    fn live_session_returns_ok() {
        assert_eq!(decide(true), Detection::Ok);
    }

    #[test]
    fn decide_never_produces_turn_ended() {
        // `TurnEnded` needs two probes past liveness, so it is `assess_node`'s to
        // produce — never the pure liveness verdict's. Guards against someone
        // "simplifying" the two into one and reintroducing a duration.
        for alive in [true, false] {
            assert_ne!(decide(alive), Detection::TurnEnded);
        }
    }

    // --- parse_turn_state (#469 §2). The fixtures are cut from the REAL
    // transcript of the node this issue was opened about (`XBG5Cxkn`, 679
    // records); free text is clipped, every field the parser reads is verbatim.

    /// The real tail: … assistant `tool_use` → user `tool_result` → assistant
    /// `text` → two `system` records → four untimestamped metadata records.
    const FIXTURE_TURN_ENDED: &str = include_str!("../tests/fixtures/turn_state/turn_ended.jsonl");
    /// The same transcript cut on the `docker build -q` whose `tool_result`
    /// landed **214 s** later — one of five over-threshold gaps in this healthy
    /// node, and the measured cause of the false `node_stale`.
    const FIXTURE_IN_TOOL_CALL: &str =
        include_str!("../tests/fixtures/turn_state/in_tool_call.jsonl");
    /// Cut on a `user`/`tool_result`: the assistant still owes a reply.
    const FIXTURE_AWAITING_ASSISTANT: &str =
        include_str!("../tests/fixtures/turn_state/awaiting_assistant.jsonl");
    /// Only Claude Code's untimestamped metadata records (`last-prompt`,
    /// `ai-title`, `mode`, `permission-mode`).
    const FIXTURE_METADATA_ONLY: &str =
        include_str!("../tests/fixtures/turn_state/metadata_only.jsonl");

    #[test]
    fn real_transcript_tail_is_turn_ended() {
        // AC4. The node this issue is about HAD finished: its last substantial
        // record is an assistant `text` message and no tool call is pending.
        assert_eq!(parse_turn_state(FIXTURE_TURN_ENDED), TurnState::TurnEnded);
    }

    #[test]
    fn pending_docker_build_is_in_tool_call() {
        // AC5, the fixture that forbids "two writers on one worktree": mid a
        // 214 s `docker build` the agent is *inside* a tool call, so neither a
        // duration nor already-valid outputs may complete it.
        assert_eq!(
            parse_turn_state(FIXTURE_IN_TOOL_CALL),
            TurnState::InToolCall
        );
    }

    #[test]
    fn trailing_tool_result_is_awaiting_assistant() {
        // AC6 — the API-retries-exhausted shape (#251).
        assert_eq!(
            parse_turn_state(FIXTURE_AWAITING_ASSISTANT),
            TurnState::AwaitingAssistant
        );
    }

    #[test]
    fn metadata_only_tail_is_unknown_not_turn_ended() {
        // AC7. These records carry no `message`, so a naive "read the last line"
        // would see `permission-mode` and conclude nothing — hence the
        // *substantial record* definition. `Unknown` behaves as "at work".
        assert_eq!(parse_turn_state(FIXTURE_METADATA_ONLY), TurnState::Unknown);
    }

    #[test]
    fn empty_tail_is_unknown() {
        assert_eq!(parse_turn_state(""), TurnState::Unknown);
        assert_eq!(parse_turn_state("\n\n  \n"), TurnState::Unknown);
    }

    #[test]
    fn a_clipped_leading_record_is_skipped_not_guessed() {
        // The read window can land mid-record. The leading partial line must be
        // dropped, and the verdict stays that of the records which do survive.
        assert_eq!(
            parse_turn_state(&FIXTURE_TURN_ENDED[40..]),
            TurnState::TurnEnded
        );
    }

    #[test]
    fn a_single_unparseable_record_is_unknown() {
        // One oversized `tool_result` overrunning the whole window: nothing
        // substantial survives → `Unknown` → left alone. Fail-safe.
        assert_eq!(
            parse_turn_state(r#"{"type":"user","message":{"role":"user","conte"#),
            TurnState::Unknown
        );
    }

    #[test]
    fn pending_tool_use_beats_a_later_assistant_message() {
        // Ordering guard: the record that OPENS a tool call is itself an
        // `assistant` message, so testing the last role before the pending-call
        // set would read a live tool call as a finished turn.
        let tail = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"sleep 300"}}]}}"#;
        assert_eq!(parse_turn_state(tail), TurnState::InToolCall);
    }

    #[test]
    fn a_matched_tool_call_does_not_hold_the_turn_open() {
        let tail = concat!(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{}}]}}"#,
            "\n",
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]}}"#,
            "\n",
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"done"}]}}"#,
            "\n"
        );
        assert_eq!(parse_turn_state(tail), TurnState::TurnEnded);
    }

    #[test]
    fn an_orphan_tool_result_closes_nothing_and_is_harmless() {
        // Its `tool_use` fell outside the read window. Removing an id that was
        // never inserted is a no-op, and the last role still decides.
        let tail = concat!(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"gone","content":"ok"}]}}"#,
            "\n",
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"done"}]}}"#,
            "\n"
        );
        assert_eq!(parse_turn_state(tail), TurnState::TurnEnded);
    }

    #[test]
    fn a_string_content_assistant_message_still_ends_the_turn() {
        // The older format carries `content` as a bare string: no tool blocks to
        // read, but the role still counts.
        let tail = r#"{"type":"assistant","message":{"role":"assistant","content":"all done"}}"#;
        assert_eq!(parse_turn_state(tail), TurnState::TurnEnded);
    }

    #[test]
    fn system_records_are_not_substantial() {
        // A `system` record has no `message`, so it never decides; a `thinking`
        // block is an assistant message and legitimately does.
        let tail = concat!(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"thinking","thinking":"hmm"}]}}"#,
            "\n",
            r#"{"type":"system","timestamp":"2026-07-29T09:10:43.884Z","hasOutput":false}"#,
            "\n"
        );
        assert_eq!(parse_turn_state(tail), TurnState::TurnEnded);
    }

    // --- the turn-end anti-bounce (#469 §2) ---

    #[test]
    fn quiet_window_boundaries() {
        let now = SystemTime::now();
        assert!(!quiet_long_enough(now, now), "a write just landed");
        assert!(!quiet_long_enough(
            now - (TURN_END_QUIET_PERIOD - Duration::from_secs(1)),
            now
        ));
        assert!(quiet_long_enough(now - TURN_END_QUIET_PERIOD, now));
        assert!(quiet_long_enough(now - Duration::from_secs(3600), now));
    }

    #[test]
    fn a_future_mtime_is_never_quiet() {
        // Clock skew must not greenlight a completion.
        let now = SystemTime::now();
        assert!(!quiet_long_enough(now + Duration::from_secs(600), now));
    }

    // --- the setting resolver (#469 §4, ADR-0015 stored → env → default) ---

    /// ADR-0012: a terminal action the runtime initiates is earned, not given, so
    /// the built-in default must stay OFF. A compile-time guard — flipping the
    /// constant fails the build rather than a test run.
    const _: () = assert!(!AUTOCOMPLETE_TURN_END_DEFAULT);

    #[test]
    fn stored_wins_over_env_in_both_directions() {
        // A stored 0 is a stored OFF and wins, exactly like a stored 1 wins —
        // that is what makes the checkbox authoritative over the env var.
        assert!(autocomplete_turn_end_with(Some(1)));
        assert!(!autocomplete_turn_end_with(Some(0)));
        // Any non-zero is truthy: the column is written 0/1, but a hand-edited DB
        // must not become a third state.
        assert!(autocomplete_turn_end_with(Some(7)));
    }

    #[test]
    fn bool_setting_parses_the_usual_spellings_and_rejects_junk() {
        for on in ["1", "true", "TRUE", " yes ", "on"] {
            assert_eq!(parse_bool_setting(on), Some(true), "{on:?}");
        }
        for off in ["0", "false", "No", "off"] {
            assert_eq!(parse_bool_setting(off), Some(false), "{off:?}");
        }
        // Junk yields None so it falls through to the next tier instead of
        // silently meaning `false`.
        for junk in ["", "maybe", "2x", "oui"] {
            assert_eq!(parse_bool_setting(junk), None, "{junk:?}");
        }
    }

    // --- read_transcript_tail (filesystem) ---

    #[test]
    fn transcript_tail_reads_the_last_bytes_and_the_mtime() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("session.jsonl");
        // Comfortably larger than the window, so the read must seek.
        let filler = "x".repeat(TRANSCRIPT_TAIL_BYTES as usize);
        std::fs::write(&path, format!("{filler}TAIL-MARKER\n")).unwrap();
        let back_dated = SystemTime::now() - Duration::from_secs(300);
        filetime::set_file_mtime(&path, filetime::FileTime::from_system_time(back_dated)).unwrap();

        let tail = read_transcript_tail(&path).expect("tail must read");
        assert!(
            tail.text.ends_with("TAIL-MARKER\n"),
            "must read the END of the file"
        );
        assert!(
            tail.text.len() as u64 <= TRANSCRIPT_TAIL_BYTES,
            "must never exceed the window: {} bytes",
            tail.text.len()
        );
        assert!(
            tail.mtime <= SystemTime::now() - Duration::from_secs(299),
            "mtime must come from the same stat as the read"
        );
    }

    #[test]
    fn transcript_tail_reads_a_short_file_whole() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("session.jsonl");
        std::fs::write(&path, "{}\n").unwrap();
        assert_eq!(read_transcript_tail(&path).unwrap().text, "{}\n");
    }

    #[test]
    fn transcript_tail_of_a_missing_file_is_none() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(read_transcript_tail(&tmp.path().join("nope.jsonl")).is_none());
    }

    // --- detection_events ---

    #[test]
    fn events_ok_is_empty() {
        assert!(detection_events(&Detection::Ok, "r", "n", 1).is_empty());
    }

    #[test]
    fn events_turn_ended_is_empty() {
        // #469 §3: the terminal `NodeAutoCompleted` belongs to the SHARED
        // node-completion body (which merges the sub-worktree first), never to a
        // bare append from the sweep.
        assert!(detection_events(&Detection::TurnEnded, "r", "n", 1).is_empty());
    }

    #[test]
    fn events_session_died() {
        let events = detection_events(&Detection::SessionDied, "run1", "node1", 1);
        assert_eq!(events.len(), 1);
        // ADR-0049: session death is an infra incident → `NodeInterrupted`, not
        // `NodeFailed`.
        assert_eq!(events[0].kind, EventKind::NodeInterrupted);
        assert_eq!(events[0].node_id.as_deref(), Some("node1"));
        let payload = events[0].payload.as_ref().unwrap();
        // #213 AC1: the failure cause must name the dead tmux session so an
        // operator inspecting the run can tell exactly which session vanished.
        let reason = payload["reason"].as_str().unwrap();
        assert!(
            reason.contains("pdo-run1-node1-iter-1"),
            "session-died cause {reason:?} must name the dead session"
        );
    }

    #[test]
    fn no_detection_can_emit_node_stale() {
        // AC2 (pure half): `NodeStale` has no producer left. The variant itself
        // stays — the log is append-only and historical Runs carry it — but
        // nothing in the daemon writes one.
        for detection in [Detection::Ok, Detection::TurnEnded, Detection::SessionDied] {
            for event in detection_events(&detection, "r", "n", 1) {
                assert_ne!(
                    event.kind,
                    EventKind::NodeStale,
                    "no detection may emit NodeStale any more (#469 §1)"
                );
                assert_ne!(
                    event.kind,
                    EventKind::NodeAutoCompleted,
                    "auto-completion goes through the shared completion body (#469 §3)"
                );
            }
        }
    }

    // --- #234 session-death diagnostics ---

    #[test]
    fn diagnostics_to_json_carries_all_fields() {
        let diag = SessionDeathDiagnostics {
            tmux_server_alive: Some(false),
            mem_available_kb: Some(123),
            swap_free_kb: Some(456),
            correlated_deaths: 2,
            harness_error: Some("model failure after retries".to_string()),
        };
        let json = diag.to_json();
        assert_eq!(json["tmux_server_alive"], serde_json::json!(false));
        assert_eq!(json["mem_available_kb"], serde_json::json!(123));
        assert_eq!(json["swap_free_kb"], serde_json::json!(456));
        assert_eq!(json["correlated_deaths"], serde_json::json!(2));
        assert_eq!(
            json["harness_error"],
            serde_json::json!("model failure after retries")
        );
    }

    #[test]
    fn diagnostics_to_json_none_probes_serialize_to_null() {
        // A failed probe must be distinguishable from a real value, so `None`
        // fields serialise to JSON `null` rather than being dropped.
        let json = SessionDeathDiagnostics::default().to_json();
        assert!(json["tmux_server_alive"].is_null());
        assert!(json["mem_available_kb"].is_null());
        assert!(json["swap_free_kb"].is_null());
        assert_eq!(json["correlated_deaths"], serde_json::json!(0));
    }

    #[test]
    fn attach_diagnostics_enriches_session_died_payload_alongside_reason() {
        let mut events = detection_events(&Detection::SessionDied, "run1", "node1", 1);
        let diag = SessionDeathDiagnostics {
            tmux_server_alive: Some(false),
            mem_available_kb: Some(2048),
            swap_free_kb: Some(0),
            correlated_deaths: 1,
            harness_error: None,
        };
        attach_diagnostics(&mut events, &diag);

        let payload = events[0].payload.as_ref().unwrap();
        // The original symptom is preserved …
        assert!(payload["reason"].as_str().unwrap().contains("session_died"));
        // … and the diagnostics sit alongside it.
        assert_eq!(
            payload["diagnostics"]["tmux_server_alive"],
            serde_json::json!(false)
        );
        assert_eq!(
            payload["diagnostics"]["correlated_deaths"],
            serde_json::json!(1)
        );
        assert_eq!(
            payload["diagnostics"]["mem_available_kb"],
            serde_json::json!(2048)
        );
    }

    #[test]
    fn attach_diagnostics_is_noop_on_empty_events() {
        // Detection::Ok yields no events — attaching must not panic.
        let mut events = detection_events(&Detection::Ok, "run1", "node1", 1);
        attach_diagnostics(&mut events, &SessionDeathDiagnostics::default());
        assert!(events.is_empty());
    }

    #[test]
    fn parse_meminfo_extracts_available_and_swap() {
        let contents = "\
MemTotal:       16384000 kB
MemFree:          512000 kB
MemAvailable:    8192000 kB
SwapTotal:       2048000 kB
SwapFree:         204800 kB
";
        let (mem, swap) = parse_meminfo(contents);
        assert_eq!(mem, Some(8192000));
        assert_eq!(swap, Some(204800));
    }

    #[test]
    fn parse_meminfo_missing_fields_return_none() {
        // No MemAvailable / SwapFree lines (e.g. an ancient kernel) → None,
        // not a wrong value picked up from a similarly-named line.
        let contents = "MemTotal:  16384000 kB\nMemFree:  512000 kB\nSwapTotal:  2048000 kB\n";
        assert_eq!(parse_meminfo(contents), (None, None));
    }

    #[test]
    fn parse_meminfo_ignores_malformed_values() {
        let contents = "MemAvailable:  notanumber kB\nSwapFree:\n";
        assert_eq!(parse_meminfo(contents), (None, None));
    }

    #[test]
    fn count_correlated_deaths_excludes_self_and_counts_dead_peers() {
        let running = vec![
            ("a".to_string(), 1),
            ("b".to_string(), 1),
            ("c".to_string(), 2),
        ];
        // Self ("a", 1) is excluded even though the predicate would call it
        // dead; "b" is dead, "c" is alive → exactly one correlated death.
        let dead = |id: &str, _it: i64| id != "c";
        assert_eq!(count_correlated_deaths(&running, ("a", 1), dead), 1);
    }

    #[test]
    fn count_correlated_deaths_zero_when_peers_alive() {
        let running = vec![("a".to_string(), 1), ("b".to_string(), 1)];
        assert_eq!(count_correlated_deaths(&running, ("a", 1), |_, _| false), 0);
    }

    #[test]
    fn count_correlated_deaths_distinguishes_iter() {
        // Same node id, different iter, must be treated as a distinct peer and
        // counted — not collapsed onto self.
        let running = vec![("a".to_string(), 1), ("a".to_string(), 2)];
        assert_eq!(count_correlated_deaths(&running, ("a", 1), |_, _| true), 1);
    }

    // --- running_nodes ---

    #[test]
    fn running_nodes_filters_correctly() {
        use crate::event_log::{project, Event, EventKind};

        fn make_event(kind: EventKind, node_id: Option<&str>, iter: Option<i64>) -> Event {
            let payload = if kind == EventKind::RunStarted {
                Some(serde_json::json!({ "pipeline_name": "test" }))
            } else {
                None
            };
            Event {
                id: None,
                run_id: "test-run".to_string(),
                ts: event_log::now_iso(),
                kind,
                node_id: node_id.map(String::from),
                iter,
                payload,
            }
        }

        let events = vec![
            make_event(EventKind::RunStarted, None, None),
            make_event(EventKind::NodeStarted, Some("worker"), Some(1)),
            make_event(EventKind::NodeStarted, Some("planner"), Some(1)),
            make_event(EventKind::NodeCompleted, Some("planner"), Some(1)),
        ];

        let state = project(&events).unwrap();
        let running = running_nodes(&state);
        assert_eq!(running.len(), 1);
        assert_eq!(running[0].0, "worker");
        assert_eq!(running[0].1, 1);
    }

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

        let node_dir =
            Path::new("/home/llenoir/Documents/perso/Maestro/.pdo/runs/20260623-100032-9b8331b/nodes/gzpYZA2m/iter-1");

        // The transcript dir CC writes: leading `-`, `.pdo` → `--pdo`.
        let cc_name = projects
            .join("-home-llenoir-Documents-perso-Maestro--pdo-runs-20260623-100032-9b8331b-nodes-gzpYZA2m-iter-1");
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

    // --- validate_outputs (integration with outputs_validator) ---

    #[test]
    fn validate_outputs_with_no_declared_outputs() {
        let tmp = tempfile::tempdir().unwrap();
        let pipeline_path = tmp.path().join("pipeline.yaml");
        std::fs::write(
            &pipeline_path,
            "name: test\nnodes:\n  - id: start\n    name: Start\n    type: start\n    inputs: []\n    outputs:\n      - name: user_prompt\n  - id: worker\n    name: Worker\n    type: agent\n    isolated_worktree: false\n    inputs:\n      - name: task\n    outputs: []\n  - id: end\n    name: End\n    type: end\n    inputs:\n      - name: result\n    outputs: []\nedges:\n  - source: { node: start, port: user_prompt }\n    target: { node: worker, port: task }\n",
        )
        .unwrap();

        let artifacts_dir = tmp.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();

        assert!(validate_outputs(
            &pipeline_path,
            "worker",
            1,
            &artifacts_dir
        ));
    }

    #[test]
    fn validate_outputs_with_missing_artifact() {
        let tmp = tempfile::tempdir().unwrap();
        let pipeline_path = tmp.path().join("pipeline.yaml");
        std::fs::write(
            &pipeline_path,
            "name: test\nnodes:\n  - id: start\n    name: Start\n    type: start\n    inputs: []\n    outputs:\n      - name: user_prompt\n  - id: worker\n    name: Worker\n    type: agent\n    isolated_worktree: false\n    inputs:\n      - name: task\n    outputs:\n      - name: report\n  - id: end\n    name: End\n    type: end\n    inputs:\n      - name: result\n    outputs: []\nedges:\n  - source: { node: start, port: user_prompt }\n    target: { node: worker, port: task }\n",
        )
        .unwrap();

        let artifacts_dir = tmp.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();

        assert!(!validate_outputs(
            &pipeline_path,
            "worker",
            1,
            &artifacts_dir
        ));
    }

    // --- assess_node (#469: the whole sweep policy, with fake I/O) ---

    /// A fully controllable [`NodeProbes`] fake. The two counters are the point:
    /// they prove the *short-circuits*, i.e. that a healthy node with the setting
    /// off pays for no transcript read and no outputs validation at all.
    struct FakeProbes {
        session_alive: bool,
        tail: Option<TranscriptTail>,
        outputs_valid: bool,
        pane: Option<String>,
        diagnostics: SessionDeathDiagnostics,
        tail_calls: std::cell::Cell<usize>,
        validate_calls: std::cell::Cell<usize>,
    }

    impl FakeProbes {
        fn alive() -> Self {
            Self {
                session_alive: true,
                tail: None,
                outputs_valid: false,
                pane: None,
                diagnostics: SessionDeathDiagnostics::default(),
                tail_calls: std::cell::Cell::new(0),
                validate_calls: std::cell::Cell::new(0),
            }
        }

        /// Alive, transcript = `text` last written `quiet` ago, outputs `valid`.
        fn with_tail(text: &str, quiet: Duration, valid: bool) -> Self {
            Self {
                tail: Some(TranscriptTail {
                    text: text.to_string(),
                    mtime: SystemTime::now() - quiet,
                }),
                outputs_valid: valid,
                ..Self::alive()
            }
        }

        /// The shape this issue exists for: an agent that finished its turn,
        /// wrote valid outputs, and never called `pdo complete`.
        fn finished_turn() -> Self {
            Self::with_tail(
                FIXTURE_TURN_ENDED,
                TURN_END_QUIET_PERIOD + Duration::from_secs(5),
                true,
            )
        }
    }

    impl NodeProbes for FakeProbes {
        fn session_alive(&self) -> bool {
            self.session_alive
        }
        fn transcript_tail(&self) -> Option<TranscriptTail> {
            self.tail_calls.set(self.tail_calls.get() + 1);
            self.tail.clone()
        }
        fn outputs_valid(&self) -> bool {
            self.validate_calls.set(self.validate_calls.get() + 1);
            self.outputs_valid
        }
        fn capture_pane(&self) -> Option<String> {
            self.pane.clone()
        }
        fn session_death_diagnostics(&self) -> SessionDeathDiagnostics {
            self.diagnostics.clone()
        }
    }

    fn assess(probes: &FakeProbes, autocomplete: bool) -> Assessment {
        // The existing suite is about the `claude` sweep, which has both
        // capabilities and whose JSONL parser the `FakeProbes` fixtures feed;
        // #613's capability gating is exercised by the dedicated tests below on a
        // data-declared harness (`opencode`, which has neither capability).
        assess_node(
            probes,
            &[],
            "run1",
            "worker",
            1,
            SystemTime::now(),
            autocomplete,
            crate::harness_registry::CLAUDE,
        )
    }

    // --- session death: the only verdict of death ---

    #[test]
    fn assess_dead_session_fails_with_diagnostics() {
        let probes = FakeProbes {
            session_alive: false,
            outputs_valid: true, // must be ignored: a dead session wins
            diagnostics: SessionDeathDiagnostics {
                tmux_server_alive: Some(false),
                correlated_deaths: 2,
                ..Default::default()
            },
            ..FakeProbes::finished_turn()
        };
        let a = assess(&probes, true);
        assert_eq!(a.detection, Detection::SessionDied);
        assert_eq!(a.events.len(), 1);
        // ADR-0049: session death → `NodeInterrupted`, not `NodeFailed`.
        assert_eq!(a.events[0].kind, EventKind::NodeInterrupted);
        // #234: diagnostics folded into the failure payload AND surfaced for the
        // sweep's structured log.
        assert_eq!(
            a.events[0].payload.as_ref().unwrap()["diagnostics"]["correlated_deaths"],
            serde_json::json!(2)
        );
        assert_eq!(a.session_death_diagnostics.unwrap().correlated_deaths, 2);
        assert!(!a.blocked_on_limit);
    }

    #[test]
    fn assess_dead_session_probes_no_transcript() {
        // Death short-circuits everything: no tail read, no outputs validation,
        // no pane capture. `claude`'s exit IS its verdict, so no journal is read.
        let probes = FakeProbes {
            session_alive: false,
            ..FakeProbes::finished_turn()
        };
        assert_eq!(assess(&probes, true).detection, Detection::SessionDied);
        assert_eq!(probes.tail_calls.get(), 0);
        assert_eq!(probes.validate_calls.get(), 0);
    }

    // --- #615: copilot's journal is the verdict, not its exit code (ADR-0052) ---

    const COPILOT_HARD_ERROR: &str = concat!(
        r#"{"type":"assistant.turn_start","data":{"turnId":"0"}}"#,
        "\n",
        r#"{"type":"session.error","data":{"errorType":"query","message":"Failed to get response from the AI model; retried 5 times"}}"#,
        "\n"
    );
    const COPILOT_TURN_ENDED: &str = concat!(
        r#"{"type":"assistant.turn_start","data":{"turnId":"0"}}"#,
        "\n",
        r#"{"type":"assistant.turn_end","data":{"turnId":"0"}}"#,
        "\n"
    );

    #[test]
    fn assess_dead_copilot_session_names_the_journal_error() {
        // AC (#615): a hard error the harness EXITED 0 on is recognised from the
        // journal, not the exit code. The session died; PDO reads the tail copilot
        // left and names the failure in the diagnostics.
        let probes = FakeProbes {
            session_alive: false,
            ..FakeProbes::with_tail(COPILOT_HARD_ERROR, Duration::from_secs(1), false)
        };
        let a = assess_harness(&probes, true, crate::harness_registry::COPILOT);
        assert_eq!(a.detection, Detection::SessionDied);
        let err = a
            .session_death_diagnostics
            .as_ref()
            .unwrap()
            .harness_error
            .as_deref()
            .expect("the journal error is named");
        assert!(err.contains("Failed to get response from the AI model"));
        // And it rides in the NodeInterrupted payload alongside the symptom.
        assert_eq!(
            a.events[0].payload.as_ref().unwrap()["diagnostics"]["harness_error"],
            serde_json::json!(err)
        );
    }

    #[test]
    fn assess_copilot_errored_turn_is_not_auto_completed() {
        // A copilot node whose journal trails on a hard error must NOT be auto-
        // completed as a finished turn — even with the setting on and outputs valid.
        let probes = FakeProbes::with_tail(
            COPILOT_HARD_ERROR,
            TURN_END_QUIET_PERIOD + Duration::from_secs(5),
            true,
        );
        let a = assess_harness(&probes, true, crate::harness_registry::COPILOT);
        assert_ne!(
            a.detection,
            Detection::TurnEnded,
            "an errored turn is not ended"
        );
    }

    #[test]
    fn assess_copilot_finished_turn_is_auto_completed() {
        // The positive control: a real copilot turn-end (its own event shape) with
        // valid outputs auto-completes — dispatched to copilot's journal parser.
        let probes = FakeProbes::with_tail(
            COPILOT_TURN_ENDED,
            TURN_END_QUIET_PERIOD + Duration::from_secs(5),
            true,
        );
        let a = assess_harness(&probes, true, crate::harness_registry::COPILOT);
        assert_eq!(a.detection, Detection::TurnEnded);
    }

    // --- setting OFF: the default path is one liveness probe (#469 §4, AC8) ---

    #[test]
    fn assess_with_setting_off_never_reads_the_transcript() {
        // AC8, at the I/O seam: the node HAS finished its turn with valid
        // outputs, and with the box unchecked the sweep does not even look.
        let probes = FakeProbes::finished_turn();
        let a = assess(&probes, false);
        assert_eq!(a.detection, Detection::Ok);
        assert!(a.events.is_empty());
        assert_eq!(
            probes.tail_calls.get(),
            0,
            "transcript_tail must NOT be called when the setting is off"
        );
        assert_eq!(
            probes.validate_calls.get(),
            0,
            "outputs must NOT be validated when the setting is off"
        );
    }

    #[test]
    fn assess_healthy_node_is_silent() {
        let probes = FakeProbes {
            pane: Some("● Running: cargo test".to_string()),
            ..FakeProbes::alive()
        };
        let a = assess(&probes, true);
        assert_eq!(a.detection, Detection::Ok);
        assert!(!a.blocked_on_limit);
        assert!(a.events.is_empty());
    }

    // --- setting ON: the two independent guards (#469 §2) ---

    #[test]
    fn assess_finished_turn_with_valid_outputs_is_turn_ended() {
        let probes = FakeProbes::finished_turn();
        let a = assess(&probes, true);
        assert_eq!(a.detection, Detection::TurnEnded);
        // The sweep, not `assess_node`, owns the terminal event (#469 §3).
        assert!(
            a.events.is_empty(),
            "TurnEnded must emit no event of its own"
        );
        assert_eq!(probes.validate_calls.get(), 1);
    }

    #[test]
    fn assess_mid_tool_call_is_never_completed_even_with_valid_outputs() {
        // The core regression guard, and the reason a duration cannot be the
        // signal: this transcript has been silent through a 214 s `docker build`
        // and its outputs already validate. Completing it would put a second
        // writer on the node's worktree.
        let probes = FakeProbes::with_tail(FIXTURE_IN_TOOL_CALL, Duration::from_secs(3600), true);
        assert_eq!(assess(&probes, true).detection, Detection::Ok);
        assert_eq!(
            probes.validate_calls.get(),
            0,
            "a pending tool call must short-circuit before the outputs guard"
        );
    }

    #[test]
    fn assess_awaiting_assistant_is_never_completed() {
        // #251: API retries exhausted mid-turn. Silent, alive, outputs valid —
        // and still not finished.
        let probes =
            FakeProbes::with_tail(FIXTURE_AWAITING_ASSISTANT, Duration::from_secs(3600), true);
        assert_eq!(assess(&probes, true).detection, Detection::Ok);
    }

    #[test]
    fn assess_unknown_turn_state_is_never_completed() {
        // Signal absent ⇒ touch nothing. Fail-safe by construction.
        let probes = FakeProbes::with_tail(FIXTURE_METADATA_ONLY, Duration::from_secs(3600), true);
        assert_eq!(assess(&probes, true).detection, Detection::Ok);
    }

    #[test]
    fn assess_finished_turn_with_incomplete_outputs_is_not_completed() {
        // The second guard on its own: an agent that ends its turn to ask a
        // question has an ended turn and unfinished work.
        let probes = FakeProbes::with_tail(
            FIXTURE_TURN_ENDED,
            TURN_END_QUIET_PERIOD + Duration::from_secs(5),
            false,
        );
        assert_eq!(assess(&probes, true).detection, Detection::Ok);
        assert_eq!(probes.validate_calls.get(), 1, "the outputs guard did run");
    }

    #[test]
    fn assess_respects_the_anti_bounce_window() {
        // A turn that ended one second ago is not acted on: the successor record
        // may be landing as we read.
        let probes = FakeProbes::with_tail(FIXTURE_TURN_ENDED, Duration::from_secs(1), true);
        assert_eq!(assess(&probes, true).detection, Detection::Ok);
        assert_eq!(
            probes.validate_calls.get(),
            0,
            "the anti-bounce must short-circuit before the outputs guard"
        );
    }

    #[test]
    fn assess_no_transcript_is_ok() {
        // A `script` node (ADR-0017) has no `claude`, hence no transcript at all
        // — it self-signals and must never be touched here.
        let probes = FakeProbes {
            tail: None,
            outputs_valid: true,
            ..FakeProbes::alive()
        };
        assert_eq!(assess(&probes, true).detection, Detection::Ok);
        assert_eq!(probes.tail_calls.get(), 1);
        assert_eq!(probes.validate_calls.get(), 0);
    }

    // --- usage-limit menu (#290), unchanged by #469 ---

    #[test]
    fn assess_usage_limit_menu_flags_blocked_and_emits_once() {
        let probes = FakeProbes {
            pane: Some("❯ 1. Stop and wait for limit to reset".to_string()),
            ..FakeProbes::alive()
        };
        let a = assess(&probes, true);
        assert_eq!(a.detection, Detection::Ok);
        assert!(
            a.blocked_on_limit,
            "usage-limit menu must set the gauge flag"
        );
        assert_eq!(a.events.len(), 1);
        assert_eq!(a.events[0].kind, EventKind::NodeBlockedOnLimit);
    }

    #[test]
    fn assess_usage_limit_gauge_set_but_event_deduped() {
        // On a subsequent sweep the menu is still up: the gauge still counts it,
        // but the event is not re-emitted (rising-edge dedup).
        let probes = FakeProbes {
            pane: Some("Stop and wait for limit to reset".to_string()),
            ..FakeProbes::alive()
        };
        let prior = vec![event_log::Event {
            id: None,
            run_id: "run1".to_string(),
            ts: event_log::now_iso(),
            kind: EventKind::NodeBlockedOnLimit,
            node_id: Some("worker".to_string()),
            iter: Some(1),
            payload: None,
        }];
        let a = assess_node(
            &probes,
            &prior,
            "run1",
            "worker",
            1,
            SystemTime::now(),
            true,
            crate::harness_registry::CLAUDE,
        );
        assert!(
            a.blocked_on_limit,
            "gauge counts every sweep the menu is up"
        );
        assert!(
            a.events.is_empty(),
            "the blocked event is emitted only once"
        );
    }

    #[test]
    fn a_blocked_node_that_finished_its_turn_still_reports_the_menu() {
        // Turn-end and the #290 marker are orthogonal: flipping the detection to
        // `TurnEnded` must not swallow the informational event or the gauge.
        let probes = FakeProbes {
            pane: Some("Stop and wait for limit to reset".to_string()),
            ..FakeProbes::finished_turn()
        };
        let a = assess(&probes, true);
        assert_eq!(a.detection, Detection::TurnEnded);
        assert!(a.blocked_on_limit);
        assert_eq!(a.events.len(), 1);
        assert_eq!(a.events[0].kind, EventKind::NodeBlockedOnLimit);
    }

    // --- #553/#613: capability gating — a data-declared harness runs no probe ---

    fn assess_harness(probes: &FakeProbes, autocomplete: bool, harness: &str) -> Assessment {
        assess_node(
            probes,
            &[],
            "run1",
            "worker",
            1,
            SystemTime::now(),
            autocomplete,
            harness,
        )
    }

    /// A data-declared harness the sweep carries no code for: neither capability,
    /// no transcript resolution, no pane anchor. `opencode` is the embedded example.
    const DATA_DECLARED: &str = crate::harness_registry::OPENCODE;

    #[test]
    fn a_harness_without_the_turn_end_capability_is_never_auto_completed() {
        // The node HAS finished its turn with valid outputs and the setting is ON —
        // but its harness has no turn-end substrate, so the sweep must not complete
        // it, and must not even read a transcript (the substrate is not claude's).
        let probes = FakeProbes::finished_turn();
        let a = assess_harness(&probes, true, DATA_DECLARED);
        assert_eq!(
            a.detection,
            Detection::Ok,
            "no auto-completion without the capability"
        );
        assert_eq!(
            probes.tail_calls.get(),
            0,
            "no transcript read for an un-instrumented harness"
        );
        assert_eq!(probes.validate_calls.get(), 0);
    }

    #[test]
    fn a_harness_with_the_turn_end_capability_still_auto_completes() {
        // The control: with the capability present (and the setting on) the same
        // finished turn IS completed — the gate is the capability, nothing else.
        let probes = FakeProbes::finished_turn();
        let a = assess_harness(&probes, true, crate::harness_registry::CLAUDE);
        assert_eq!(a.detection, Detection::TurnEnded);
    }

    #[test]
    fn a_harness_without_the_usage_limit_capability_is_never_flagged_blocked() {
        // The pane shows what WOULD be a usage-limit menu, but this harness has no
        // such anchor — so the probe short-circuits and the node is not flagged.
        let probes = FakeProbes {
            pane: Some("❯ 1. Stop and wait for limit to reset".to_string()),
            ..FakeProbes::alive()
        };
        let a = assess_harness(&probes, true, DATA_DECLARED);
        assert_eq!(a.detection, Detection::Ok);
        assert!(
            !a.blocked_on_limit,
            "no menu probe runs without the usage-limit capability"
        );
        assert!(a.events.is_empty());
    }
}
