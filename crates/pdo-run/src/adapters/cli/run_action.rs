//! The `pdo run` subcommands (#721, #588): create a Run, wait for the children
//! of this node.

use clap::Subcommand;

#[derive(Subcommand, Debug)]
// `Create` carries every creation flag, `Wait` two — a clap enum is parsed once
// per process, so the size skew buys nothing to box away (it is already boxed
// behind `Commands::Run`).
#[allow(clippy::large_enum_variant)]
pub enum RunAction {
    /// Create a Run — the thin CLI client of the daemon's `POST /runs`
    /// (issue #721, ADR-0064).
    ///
    /// Identity travels with the session, never in the body: run from a node
    /// session (the `PDO_RUN_ID` / `PDO_NODE_ID` env vars the session is
    /// wrapped with), the created Run is mechanically linked to that Run + node
    /// (`parent_run_id` / `parent_node_id`); run from a plain terminal, it is a
    /// root Run. The child's project defaults to the parent's — `--target-repo`
    /// overrides it explicitly. Daemon refusals (unknown pipeline, a refused
    /// session claim, …) print the daemon's own `error` sentence and exit
    /// non-zero.
    Create {
        /// The pipeline to run, resolved by the daemon against its pipeline stores.
        pipeline: String,
        /// Prompt for the Run's start node. A prompt-optional pipeline accepts
        /// the empty default.
        #[arg(short, long, default_value = "", conflicts_with = "input_file")]
        input: String,
        /// Read the prompt from a file instead of `--input` (#779) — e.g. the
        /// spec an upstream node wrote: `--input-file "$PDO_INPUT_SPEC"`.
        #[arg(long, value_name = "PATH")]
        input_file: Option<std::path::PathBuf>,
        /// Attach an image (#779, repeatable): sent in the multipart `images`
        /// field, written to the child's `_input/`, listed under
        /// `## Input Images` in its entry node's preamble.
        #[arg(long, value_name = "PATH")]
        image: Vec<std::path::PathBuf>,
        /// Attach any file (#779, repeatable): sent in the multipart `files`
        /// field, written to the child's `_input/`, listed under
        /// `## Input Files`. Any extension; one size budget per run
        /// (`max_attachments_mb` in Settings); duplicate names are refused.
        #[arg(long, value_name = "PATH")]
        file: Vec<std::path::PathBuf>,
        /// Run variables as a JSON object: `--variables '{"pool":"a"}'`.
        #[arg(long)]
        variables: Option<String>,
        /// Skills selected at the Run tier (#669, ADR-0062): comma-separated
        /// ids (`--skills a,b`) or a JSON list of `{id, name}` objects.
        #[arg(long)]
        skills: Option<String>,
        /// Agentic profile at the Run tier (ADR-0057) as JSON —
        /// `{"mode":"profile","profile_id":"…"}` or
        /// `{"mode":"custom","harness":"…","model":"…","effort":"…"}`.
        #[arg(long)]
        agent_choice: Option<String>,
        /// Agentic harness (ADR-0046): free text, validated at spawn.
        #[arg(long)]
        harness: Option<String>,
        /// Sandbox isolation: `off`, or the name of a staging profile.
        #[arg(long)]
        sandbox: Option<String>,
        /// Primary target repo (path or URL, like the UI's create modal).
        /// Overrides the parent Run's project — ADR-0033 keeps the boundary.
        #[arg(long)]
        target_repo: Option<String>,
        /// Secondary repos as a JSON list of `{repo, base_branch?, read_only?}`
        /// (ADR-0042).
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
        /// The Run's `auto_fail` preference (ADR-0049).
        #[arg(long)]
        auto_fail: Option<bool>,
        /// Run-level provisioning rules as JSON (ADR-0061).
        #[arg(long)]
        provisioning: Option<String>,
    },
    /// Block until a child run of this node becomes terminal (#588, ADR-0069):
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
}
