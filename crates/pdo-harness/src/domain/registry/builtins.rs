//! The harnesses PDO ships compiled in: the embedded floor (ADR-0045, ADR-0046).

use super::*;

/// The `claude` descriptor: the legacy launch, expressed as data.
///
/// The launch template reproduces the pre-#550 `build_agent_tail` **byte for
/// byte** once its holes are empty — that is the #550 gate, pinned by the goldens
/// in [`crate::domain::argv`]. The resume template reproduces the pre-#550
/// `build_resume_script` tail; its `{resume}` hole is the identity-or-blind
/// selector the resume seam computes (ADR-0045 keeps that choice in code). Its
/// `{model}` hole stays empty on a Node's resume (the conversation keeps its
/// model); a Session hébergée relaunched on another model fills it.
pub fn claude() -> HarnessDescriptor {
    HarnessDescriptor {
        name: CLAUDE.to_string(),
        binary: "claude".to_string(),
        launch: [
            "exec",
            "claude",
            "--dangerously-skip-permissions",
            "--model {model}",
            "--effort {effort}",
            "--settings {settings}",
            "--session-id {session_id}",
            "{prompt}",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect(),
        resume: [
            "exec",
            "claude",
            "--dangerously-skip-permissions",
            "{resume}",
            "--model {model}",
            "--effort {effort}",
            "--settings {settings}",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect(),
        // #614: the resume verbs, now a property of the descriptor rather than
        // constants in the resume seam. `--resume '<id>'` re-enters by identity
        // (#473); `--continue` blind-continues a pre-#473 row or the manager.
        resume_by_id: "--resume".to_string(),
        resume_blind: "--continue".to_string(),
        // AC #4: the CCR suppression now comes from the descriptor for an agent
        // tail (byte-identical to the wrapper's old hard-coded export — see the
        // `harness_env` handling in `pdo_session::domain::launch::wrap_with_env`).
        env: vec![(
            "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC".to_string(),
            "1".to_string(),
        )],
    }
}

/// The `opencode` descriptor (measured on 1.18.18).
///
/// `--auto` is its `--dangerously-skip-permissions`; the prompt is a `--prompt`
/// argument; the model is `--model provider/model`. There is **no** effort hole
/// (no launch-time effort axis) and **no** session-id hole (identity can't be
/// pinned), so the effort picker greys and attribution falls back to the working
/// dir. Resume is a blind `--continue` (opencode is resident and continues the
/// cwd's latest session); it carries no env block.
pub fn opencode() -> HarnessDescriptor {
    HarnessDescriptor {
        name: OPENCODE.to_string(),
        binary: "opencode".to_string(),
        launch: [
            "exec",
            "opencode",
            "--auto",
            "--model {model}",
            "--prompt {prompt}",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect(),
        resume: ["exec", "opencode", "--auto", "{resume}"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        // opencode cannot pin a session identity, so it only ever blind-continues
        // — `resume_by_id` is empty and `{resume}` always renders `--continue`
        // (byte-identical to the pre-#614 literal tail).
        resume_by_id: String::new(),
        resume_blind: "--continue".to_string(),
        env: vec![],
    }
}

/// The `copilot` descriptor — GitHub Copilot CLI as PDO's second first-party
/// harness (#614, ADR-0045). Embedded (the third arm of the floor), not a disk
/// descriptor: the picker's provenance is decided by name, so a `copilot` declared
/// on disk would read "From descriptors" and contradict the support table.
///
/// Its capabilities are declared in [`crate::domain::probes`].
///
/// The launch uses copilot's **interactive** mode with the prompt auto-executed
/// (`-i {prompt}`), so the harness is **resident after the turn** — an attachable
/// session the user can take over, PDO's core contract (CONTEXT.md § "Harnais
/// agentique", ADR-0012). The non-interactive `-p` is deliberately *not* used: it
/// exits at turn end, making the harness ineligible (a session death indistinguishable
/// from a failure, ADR-0032). This is the #615-iter-2 correction of the launch that
/// originally shipped a **positional** prompt (`"{prompt}"`): copilot 1.0.80 reserves
/// the positional slot for **subcommands**, so a positional prompt is refused
/// (`error: too many arguments`) and the node hangs `running` forever — the way to
/// enter interactive mode *with* a prompt is `-i`, measured on the installed binary.
/// Interactive mode is also what writes the `events.jsonl` journal every copilot
/// capability reads. The tokens:
/// - `--allow-all` grants full autonomy — no tool, path or URL permission dialog
///   (AC "aucun dialogue de permission d'outil, de chemin ou d'URL");
/// - `--no-ask-user` disables the question tool, so the node never stalls asking;
/// - the model hole is **absent when unset**, so a node with no explicit model
///   launches on copilot's own automatic model selector (AC), the class of
///   dead-end #561 measured on `opencode` never arises;
/// - `--session-id {session_id}` pins the session identity at launch (measured on
///   the installed binary — the caller-provided id the SDK documents, requested
///   for the CLI in github/copilot-cli#167), so resume re-enters **this** node's
///   session by identity, never a blind `--continue` (AC);
/// - `-i {prompt}` enters interactive mode and auto-executes the augmented prompt.
///
/// **Prerequisite (not PDO code):** interactive copilot gates a first turn behind a
/// one-time "trust this folder" dialog (`-p` bypasses it, interactive does not).
/// Trust **cascades** to subdirectories, so trusting the target repo root once covers
/// every node sub-worktree beneath it — the "dossier de travail approuvé" prerequisite
/// (CONTEXT.md § "Harnais agentique"), documented like auth, never staged by PDO.
///
/// `COPILOT_AUTO_UPDATE=false` freezes the binary: PDO decides when the target
/// moves, not the provider (AC).
pub fn copilot() -> HarnessDescriptor {
    HarnessDescriptor {
        name: COPILOT.to_string(),
        binary: "copilot".to_string(),
        launch: [
            "exec",
            "copilot",
            "--allow-all",
            "--no-ask-user",
            "--model {model}",
            "--session-id {session_id}",
            "-i {prompt}",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect(),
        resume: [
            "exec",
            "copilot",
            "--allow-all",
            "--no-ask-user",
            "{resume}",
            "--model {model}",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect(),
        // Resume is by identity (`--resume '<id>'`) or not at all: `resume_blind`
        // is empty, so a resume with no recorded id renders no resume flag rather
        // than a blind continue (AC "jamais par un continue aveugle").
        resume_by_id: "--resume".to_string(),
        resume_blind: String::new(),
        // PDO owns when the harness moves (AC): disable copilot's start-of-session
        // plugin auto-update.
        env: vec![("COPILOT_AUTO_UPDATE".to_string(), "false".to_string())],
    }
}

/// The `pi` descriptor — pi coding agent as PDO's third first-party harness (#705,
/// story #702; measured on 0.85.1). Embedded, for the same reason as `copilot`: the
/// picker decides provenance by name, and the support table publishes its row.
///
/// Its capabilities are declared in [`crate::domain::probes`] — all **explicitly
/// absent** in this ticket (cost, transcript, end of turn, usage-limit anchor,
/// context usage, staging): the descriptor lands first, the instrumentation follows
/// (#706/#707). The support table says so rather than implying it.
///
/// The launch enters pi's **resident TUI** with the prompt auto-executed as a
/// positional message (`pi [options] [messages...]`), so the harness stays attachable
/// after the turn — PDO's core contract (CONTEXT.md § "Harnais agentique", ADR-0012).
/// `-p` is deliberately *not* used: it exits at turn end (ineligible, ADR-0032). The
/// tokens:
/// - `-a` (`--approve`) trusts the project-local files (`AGENTS.md`, `.pi/`
///   extensions) for this run — no trust dialog on first turn, the "dossier de
///   travail approuvé" prerequisite met by argv instead of a staged home;
/// - `--model {model}` is `provider/model` (an alias like `~anthropic/...-latest`
///   included), absent when unset so pi picks its own default;
/// - `--thinking {effort}` is pi's effort axis (`off … max`, read off `--help`);
/// - `-e {settings}` is the **turn-end extension** slot (CONTEXT.md § "Extension de
///   fin de tour"): the `{settings}` hole carries the "injected settings file" meaning
///   it has for `claude`, here an extension file loaded by `-e`. It stays **empty in
///   #705** — the extension the daemon writes arrives with #707 — so the token drops
///   and nothing claude-shaped is ever handed to pi (see
///   [`crate::domain::probes::settings_hole_takes_claude_file`]);
/// - `--session-id {session_id}` is the **imposed session identity**: pi creates the
///   session under that exact id if missing and reopens it otherwise (measured on
///   `--help`: "Use exact project session ID, creating it if missing"), so two
///   concurrent nodes in one worktree never share a session file, and resume re-enters
///   *this* node's conversation by identity.
///
/// Resume uses the **same** flag — `--session-id '<id>'` is created-or-resumed — and
/// pi never blind-continues (`--continue` would reopen the cwd's latest session, the
/// collision the imposed identity removes): `resume_blind` is empty, so a row with no
/// identity renders no resume flag at all. The effort is re-posed from the start
/// event (`--thinking {effort}`), as `claude`'s is (#424).
///
/// Env: `PI_SKIP_VERSION_CHECK=1` (PDO decides when the target moves, not a startup
/// banner) and `PI_TELEMETRY=0` (install telemetry off inside a PDO node).
///
/// **Prerequisites (not PDO code):** provider auth done on the host (`pi auth`, or
/// the provider env var), and the `pi` binary on the *user's* PATH (ADR-0055).
pub fn pi() -> HarnessDescriptor {
    HarnessDescriptor {
        name: PI.to_string(),
        binary: "pi".to_string(),
        launch: [
            "exec",
            "pi",
            "-a",
            "--model {model}",
            "--thinking {effort}",
            "-e {settings}",
            "--session-id {session_id}",
            "{prompt}",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect(),
        resume: [
            "exec",
            "pi",
            "-a",
            "{resume}",
            "--model {model}",
            "--thinking {effort}",
            "-e {settings}",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect(),
        // Same flag at launch and resume: `--session-id` is created-or-resumed.
        // Never a blind continue (imposed identity, CONTEXT.md § "Harnais agentique").
        resume_by_id: "--session-id".to_string(),
        resume_blind: String::new(),
        env: vec![
            ("PI_SKIP_VERSION_CHECK".to_string(), "1".to_string()),
            ("PI_TELEMETRY".to_string(), "0".to_string()),
        ],
    }
}

/// The embedded floor: the harnesses PDO ships compiled in, in precedence-neutral
/// declaration order.
pub fn embedded_floor() -> Vec<HarnessDescriptor> {
    vec![claude(), opencode(), copilot(), pi()]
}
