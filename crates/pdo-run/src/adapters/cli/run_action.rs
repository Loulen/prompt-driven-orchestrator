//! The `pdo run` subcommands (#721, #588): create a Run, wait for the children
//! of this node, list the Runs, read the state of one, and the gestures of the
//! Runs list and the run view ([`RunGesture`]).

use clap::Subcommand;

use super::run_gesture::RunGesture;
use super::run_read::RunRead;

#[derive(Subcommand, Debug)]
// `Create` carries every creation flag, `Wait` two — a clap enum is parsed once
// per process, so the size skew buys nothing to box away (it is already boxed
// behind `Commands::Run`).
#[allow(clippy::large_enum_variant)]
pub enum RunAction {
    /// Create a Run — the thin CLI client of the daemon's `POST /runs`.
    ///
    /// Identity travels with the session, never in the body: run from a node
    /// session (the `PDO_RUN_ID` / `PDO_NODE_ID` env vars the session is
    /// wrapped with), the created Run is mechanically linked to that Run + node
    /// (`parent_run_id` / `parent_node_id`); run from a plain terminal, it is a
    /// root Run. The child's repository defaults to the parent's — `--target-repo`
    /// overrides it explicitly, within the parent's Project. Daemon refusals (unknown pipeline, a refused
    /// session claim, …) print the daemon's own `error` sentence and exit
    /// non-zero.
    Create {
        /// The pipeline to run, resolved by the daemon against its pipeline stores.
        pipeline: String,
        /// Prompt for the Run's start node. A prompt-optional pipeline accepts
        /// the empty default.
        #[arg(short, long, default_value = "", conflicts_with = "input_file")]
        input: String,
        /// Read the prompt from a file instead of `--input` — e.g. the
        /// spec an upstream node wrote: `--input-file "$PDO_INPUT_SPEC"`.
        #[arg(long, value_name = "PATH")]
        input_file: Option<std::path::PathBuf>,
        /// Attach an image (repeatable): sent in the multipart `images`
        /// field, written to the child's `_input/`, listed under
        /// `## Input Images` in its entry node's preamble.
        #[arg(long, value_name = "PATH")]
        image: Vec<std::path::PathBuf>,
        /// Attach any file (repeatable): sent in the multipart `files`
        /// field, written to the child's `_input/`, listed under
        /// `## Input Files`. Any extension; one size budget per run
        /// (`max_attachments_mb` in Settings); duplicate names are refused.
        #[arg(long, value_name = "PATH")]
        file: Vec<std::path::PathBuf>,
        /// Run variables as a JSON object: `--variables '{"pool":"a"}'`.
        #[arg(long)]
        variables: Option<String>,
        /// Skills selected at the Run tier: comma-separated
        /// ids (`--skills a,b`) or a JSON list of `{id, name}` objects.
        #[arg(long)]
        skills: Option<String>,
        /// Agentic profile at the Run tier as JSON —
        /// `{"mode":"profile","profile_id":"…"}` or
        /// `{"mode":"custom","harness":"…","model":"…","effort":"…"}`.
        #[arg(long)]
        agent_choice: Option<String>,
        /// Agentic harness: free text, validated at spawn.
        #[arg(long)]
        harness: Option<String>,
        /// Sandbox isolation: `off`, or the name of a staging profile.
        #[arg(long)]
        sandbox: Option<String>,
        /// Primary target repo (path or URL, like the UI's create modal).
        /// Overrides the parent Run's repository; from a node session it
        /// must be one of the parent Run's Project (or of no Project when
        /// the parent is in none).
        #[arg(long)]
        target_repo: Option<String>,
        /// The Project the Run is launched in; it must hold the
        /// primary repo. Without it, the Run is launched in the Project
        /// holding the primary repo, and refused when several do. From a
        /// node session, the Run stays in its parent's.
        #[arg(long)]
        project: Option<String>,
        /// The Run's repositories as a JSON list of `{repo, base_branch?,
        /// read_only?}`, the primary first (the same as `--target-repo`), then
        /// the secondaries.
        #[arg(long)]
        target_repos: Option<String>,
        /// Branch the primary repo starts from (default: its local HEAD).
        #[arg(long)]
        source_branch: Option<String>,
        /// Display name for the Run.
        #[arg(long)]
        name: Option<String>,
        /// Whether the manager may auto-name the Run (default: resolved by the
        /// daemon from the presence of `--name`).
        #[arg(long)]
        auto_name: Option<bool>,
        /// The Run's `auto_fail` preference.
        #[arg(long)]
        auto_fail: Option<bool>,
        /// Run-level provisioning rules as JSON.
        #[arg(long)]
        provisioning: Option<String>,
        /// Data for an extension (repeatable): `--ext <id>.<key>=<value>` sets
        /// `extensions.<id>.<key>` of `POST /runs` to the string `<value>`,
        /// which PDO never interprets. The extension `<id>` checks it before
        /// the Run is created (a refusal prints its reason) and receives it
        /// before the first node. An id no installed extension has is refused.
        #[arg(long = "ext", value_name = "ID.KEY=VALUE")]
        ext: Vec<String>,
    },
    /// Block until a child run of this node becomes terminal:
    /// a long-poll on the daemon, never a push into your pane. Prints one JSON
    /// line per child that settled (`run_id`, `name`, `status`, `reason`,
    /// `children_active`) and exits 0; exits **2** with nothing on stdout when
    /// `--timeout` elapses (not a failure — call again); exits 1 when the daemon
    /// is unreachable or you are not in a node session. With no active child at
    /// the call, exits 0 with `{"noop":true,…}`. A child `awaiting_user` does
    /// not wake you: its wait shows on your node and run instead.
    Wait {
        /// Return only once EVERY child of this node is terminal.
        #[arg(long)]
        all: bool,
        /// Give up after this many seconds (exit 2). Without it, waits forever.
        #[arg(long)]
        timeout: Option<u64>,
    },
    /// List the Runs the daemon knows, newest first: one line per Run
    /// (`run_id`, status, pipeline, name). The thin CLI client of `GET /runs`,
    /// sending `PDO_TOKEN` like every command: a daemon that refuses the call
    /// (an extension requiring a token) prints its reason and exits 1.
    List {
        /// Print the daemon's JSON as is.
        #[arg(long)]
        json: bool,
    },
    /// The state of one Run in a single line: `<id> <status> · nodes
    /// <done>/<total> done[, <n> running][, <n> awaiting_user][, <n> failed] ·
    /// children <n> active · $<cost>`. A read, not a wait: the client of the
    /// Run's detail and of its children. Exits 0 when the Run is found, **4**
    /// when the daemon knows no such Run, 1 when the daemon is unreachable.
    Status {
        /// The Run to read.
        run_id: String,
        /// Print one compact JSON object with the line's fields instead.
        #[arg(long)]
        json: bool,
    },
    /// The gestures on a Run (show, cancel, pause, resume, retry, reopen,
    /// archive, forget, rename, its repositories, its Manager, its pipeline…).
    #[command(flatten)]
    Gesture(RunGesture),
    /// What a Run shows (events, costs, artifacts, files, refs, search…).
    #[command(flatten)]
    Read(RunRead),
}
