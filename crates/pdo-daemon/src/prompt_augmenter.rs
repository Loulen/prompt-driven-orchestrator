use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::pipeline::{NodeDef, NodeType, PipelineDef, PortType, IMAGE_EXTENSIONS};

pub(crate) struct InputResolution {
    pub port_name: String,
    /// A single wire is a one-element list; a `repeated`/pooled input is one path
    /// per COMPLETED source iteration (empty when none completed). Never a raw
    /// `iter-*` glob, which could not exclude a failed iter.
    pub paths: Vec<PathBuf>,
    pub repeated: bool,
    /// Sourced from the Start node's user prompt (`_input/output.md`); the entry
    /// node's prompt label adapts to `prompt_required`.
    pub from_start: bool,
}

pub(crate) struct OutputDeclaration {
    pub port_name: String,
    pub path: PathBuf,
    pub port_type: PortType,
}

pub(crate) struct ForEachContext {
    pub current_item: String,
    pub current_iter: i64,
    pub total: i64,
}

/// A secondary repo made visible to a node (ADR-0042/0047), already resolved to
/// its **absolute snapshot path** so this pure module never touches the run-dir
/// path math. That path is identical on host and in the sandbox (invariant D3).
///
/// `read_only` is the ADR-0047 opt-in: `false` (the default) means the node may
/// modify/commit/deliver the repo; `true` restores read-only-context semantics.
pub(crate) struct SecondaryRepoContext {
    pub alias: String,
    pub abs_path: String,
    pub sha: String,
    pub read_only: bool,
}

/// Build the per-node secondary-repo view from the Run's frozen pins.
///
/// The nodes' sub-worktrees do NOT inherit the snapshots (they are siblings under
/// the run dir and `.pdo/` is gitignored), so a node can only reach a secondary
/// by **absolute path** — hence resolving it here. Shared by both spawn sites, so
/// the preamble and the script env can never disagree.
pub(crate) fn secondary_repo_contexts(
    repo_root: &Path,
    run_id: &str,
    pins: &[crate::event_log::RepoPin],
) -> Vec<SecondaryRepoContext> {
    pins.iter()
        .map(|pin| SecondaryRepoContext {
            alias: pin.alias.clone(),
            abs_path: crate::worktree_ops::secondary_snapshot_path(repo_root, run_id, &pin.alias)
                .to_string_lossy()
                .to_string(),
            sha: pin.sha.clone(),
            read_only: pin.read_only,
        })
        .collect()
}

pub(crate) struct AugmentContext<'a> {
    pub pipeline: &'a PipelineDef,
    pub node: &'a NodeDef,
    #[allow(dead_code)]
    pub run_id: &'a str,
    pub iter: i64,
    pub artifacts_dir: &'a Path,
    pub variables: &'a HashMap<String, serde_yaml::Value>,
    #[allow(dead_code)]
    pub daemon_url: &'a str,
    pub foreach_context: Option<ForEachContext>,
    /// For code-mutating / merge nodes: the per-iteration sub-worktree the
    /// agent must edit in. Set to `None` for nodes that run directly in the
    /// pipeline worktree (doc-only, switch, loop, etc.).
    pub source_worktree_dir: Option<&'a Path>,
    pub input_images: Vec<String>,
    /// Whether the Start node's user prompt carries non-whitespace content.
    /// Precomputed by the daemon (which owns the artifacts dir and the read
    /// error) so `build_preamble` stays pure. Only consulted for the
    /// prompt-optional entry-node preamble.
    pub start_prompt_present: bool,
    /// Per upstream source node, the iteration whose artifacts this NodeRun reads
    /// — the source's latest COMPLETED iteration. A source absent from the map
    /// falls back to the consumer's own `iter` (positional), preserving
    /// override/injection flows where nothing has completed yet.
    pub source_iters: HashMap<String, i64>,
    /// Per source node feeding a `repeated` incoming edge, its COMPLETED
    /// iterations (ascending). Precomputed by the daemon: this module is pure and
    /// cannot hold a `RunState`. A source absent from the map (or mapping to an
    /// empty `Vec`) pools nothing — failed iterations stay quarantined, and no raw
    /// `iter-*` glob is ever handed to an agent or script.
    pub repeated_iters: HashMap<String, Vec<i64>>,
    /// Secondary repos visible to this node; empty for a mono-repo Run. Nodes
    /// reach these ONLY by absolute path, so `build_preamble` prints the paths and
    /// `build_script_env` exposes them as `PDO_SECONDARY_REPOS` plus
    /// `PDO_WRITABLE_SECONDARY_REPOS`.
    pub secondary_repos: Vec<SecondaryRepoContext>,
    /// The sub-worktree was reused **in place** at `restart_node`, so a prior
    /// agent's uncommitted work is still there. Always `false` on the start/retry
    /// path, where a reuse is unreachable by construction.
    pub reused_sub_worktree: bool,
    /// Every interrupted git operation the reused sub-worktree carries, in scan
    /// order — `index.lock` FIRST, because its instruction ("remove it before
    /// anything else") depends on that position. Empty means no notice.
    pub interrupted_git_ops: &'a [String],
    /// The partial output a previous INTERRUPTED attempt left on disk for this
    /// iteration (ADR-0049). A same-iter re-spawn never wipes it, so
    /// `build_preamble` surfaces it as input to build on — never a target to
    /// clobber. Empty on a first spawn and on a clean restart.
    pub partial_outputs: &'a [std::path::PathBuf],
}

pub(crate) fn discover_input_images(artifacts_dir: &Path) -> Vec<String> {
    let input_dir = artifacts_dir.join("_input");
    let entries = match std::fs::read_dir(&input_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let mut images = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .unwrap_or_default();
        if crate::ALLOWED_IMAGE_EXTENSIONS.contains(&ext.as_str()) {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                images.push(name.to_string());
            }
        }
    }
    images.sort();
    images
}

/// `Ok(false)` when the file is absent — the expected prompt-optional case, not
/// an error. `Err` only on a genuine I/O failure, so the caller surfaces it
/// instead of silently reporting "no prompt".
pub(crate) fn read_start_prompt_present(artifacts_dir: &Path) -> std::io::Result<bool> {
    match std::fs::read_to_string(crate::blackboard::input_path(artifacts_dir)) {
        Ok(text) => Ok(!text.trim().is_empty()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

pub(crate) fn resolve_input_paths(ctx: &AugmentContext<'_>) -> Vec<InputResolution> {
    // Project over the SINGLE edge-walk: the iteration decision lives in
    // `input_resolution::resolve_consumer_inputs`. The preamble and script-env are
    // pure projections over its result — never a second, independent edge-walk.
    let mut inputs: Vec<InputResolution> = crate::input_resolution::resolve_consumer_inputs(
        ctx.pipeline,
        ctx.artifacts_dir,
        &ctx.node.id,
        ctx.iter,
        &ctx.source_iters,
        &ctx.repeated_iters,
    )
    .into_iter()
    .map(|r| InputResolution {
        port_name: r.port,
        paths: r.paths,
        repeated: r.repeated,
        from_start: r.from_start,
    })
    .collect();

    if inputs.is_empty() && ctx.node.inputs.iter().any(|p| p.name == "task") {
        inputs.push(InputResolution {
            port_name: "task".into(),
            paths: vec![crate::blackboard::input_path(ctx.artifacts_dir)],
            repeated: false,
            from_start: true,
        });
    }

    inputs
}

/// The on-disk path a single output port declares, for one iteration. The single
/// source of truth for output-port path math (shared by [`resolve_output_paths`]
/// and [`surviving_partial_outputs`]).
fn output_port_path(
    node_id: &str,
    artifacts_dir: &Path,
    iter: i64,
    port: &crate::pipeline::Port,
) -> PathBuf {
    match port.port_type {
        PortType::Image | PortType::ImageList => {
            crate::blackboard::port_dir(artifacts_dir, node_id, iter, &port.name)
        }
        PortType::Markdown => {
            crate::blackboard::artifact_path(artifacts_dir, node_id, iter, &port.name)
        }
        // An html port's declared path is its `output.html` file (parallel to
        // markdown's `output.md`), not the port dir.
        PortType::Html => {
            crate::blackboard::artifact_path_html(artifacts_dir, node_id, iter, &port.name)
        }
    }
}

pub(crate) fn resolve_output_paths(ctx: &AugmentContext<'_>) -> Vec<OutputDeclaration> {
    ctx.node
        .outputs
        .iter()
        .map(|port| OutputDeclaration {
            port_name: port.name.clone(),
            path: output_port_path(&ctx.node.id, ctx.artifacts_dir, ctx.iter, port),
            port_type: port.port_type,
        })
        .collect()
}

/// The declared output paths that ALREADY hold content on disk for `iter` — what
/// an interrupted attempt left behind (ADR-0049). A same-iter re-spawn never
/// wipes it, so the fresh agent must be shown it and told to build on it. Empty
/// on a first spawn.
///
/// Does I/O, so the daemon computes it and feeds
/// [`AugmentContext::partial_outputs`]; `build_preamble` itself stays pure.
pub(crate) fn surviving_partial_outputs(
    node: &NodeDef,
    artifacts_dir: &Path,
    iter: i64,
) -> Vec<PathBuf> {
    node.outputs
        .iter()
        .map(|port| output_port_path(&node.id, artifacts_dir, iter, port))
        .filter(|path| path_holds_content(path))
        .collect()
}

/// Does `path` hold real output? A file with non-whitespace bytes, or a directory
/// with at least one entry (an image/html port dir). A missing path, or an empty
/// file/dir, is "no partial output" — the ordinary first-spawn state.
fn path_holds_content(path: &Path) -> bool {
    if path.is_dir() {
        return std::fs::read_dir(path).is_ok_and(|mut e| e.next().is_some());
    }
    std::fs::read_to_string(path).is_ok_and(|s| !s.trim().is_empty())
}

/// Sanitize a port / variable name into an env-var suffix: `my-port.v2` →
/// `MY_PORT_V2`.
fn env_name_suffix(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect()
}

/// The raw string a `script` node's bash reads from `$PDO_VAR_<NAME>`. Scalars
/// are emitted verbatim — NOT via `serde_yaml::to_string`, which would quote
/// bool-/number-looking strings (`"true"` → `'true'`) and leak those quotes into
/// the env value. Sequences and mappings have no scalar form, so they go out as
/// compact JSON a script can parse with `jq`.
fn var_value_to_env_string(value: &serde_yaml::Value) -> String {
    match value {
        serde_yaml::Value::Null => String::new(),
        serde_yaml::Value::Bool(b) => b.to_string(),
        serde_yaml::Value::Number(n) => n.to_string(),
        serde_yaml::Value::String(s) => s.clone(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

/// The `PDO_*` environment catalogue handed to a `script` node's bash
/// (ADR-0017). A script cannot read the prose preamble, so its I/O arrives as
/// environment variables — through the SAME `resolve_input_paths` /
/// `resolve_output_paths` the preamble uses, never a second resolution path.
///
/// - `PDO_ARTIFACTS_DIR` — the Blackboard root.
/// - `PDO_INPUT_<PORT>` — absolute path of each resolved input. A `repeated`
///   input holds one path per COMPLETED source iteration, `\n`-separated (empty
///   when nothing has completed); a single wire holds exactly one path (#353).
/// - `PDO_INPUT_<PORT>_REPEATED=1` — set when that input is a `repeated`/pooled
///   input, so a script knows to `readarray -t files <<< "$PDO_INPUT_<PORT>"`
///   (and to `pdo skip` when the value is empty).
/// - `PDO_OUTPUT_<PORT>` — absolute path the script writes its `output.md` to.
/// - `PDO_VAR_<NAME>` — each resolved pipeline variable (sorted for determinism).
///
/// The base four (`PDO_RUN_ID`/`NODE_ID`/`NODE_ITER`/`DAEMON_URL`) are exported
/// by `tmux_session_manager::wrap_with_env` and are *not* repeated here.
pub(crate) fn build_script_env(ctx: &AugmentContext<'_>) -> Vec<(String, String)> {
    let mut env = Vec::new();

    env.push((
        "PDO_ARTIFACTS_DIR".to_string(),
        ctx.artifacts_dir.to_string_lossy().into_owned(),
    ));

    for input in resolve_input_paths(ctx) {
        let key = format!("PDO_INPUT_{}", env_name_suffix(&input.port_name));
        if input.repeated {
            env.push((format!("{key}_REPEATED"), "1".to_string()));
        }
        // `\n`-separated, not space-separated, so a path containing spaces stays
        // splittable; `sh_single_quote` in `wrap_with_env` preserves the newlines
        // verbatim. A single wire is a one-element list, so its value is unchanged.
        let value = input
            .paths
            .iter()
            .map(|p| p.to_string_lossy())
            .collect::<Vec<_>>()
            .join("\n");
        env.push((key, value));
    }

    for output in resolve_output_paths(ctx) {
        env.push((
            format!("PDO_OUTPUT_{}", env_name_suffix(&output.port_name)),
            output.path.to_string_lossy().into_owned(),
        ));
    }

    // Secondary repos as `alias=abspath` lines, `\n`-separated (same convention
    // as a `repeated` input). Only set when there is at least one, so a mono-repo
    // script's env is byte-identical to what it was before this feature.
    if !ctx.secondary_repos.is_empty() {
        let value = ctx
            .secondary_repos
            .iter()
            .map(|s| format!("{}={}", s.alias, s.abs_path))
            .collect::<Vec<_>>()
            .join("\n");
        env.push(("PDO_SECONDARY_REPOS".to_string(), value));

        // The writable subset, so a delivery script knows which secondaries it may
        // commit without re-deriving read-only. Only set when non-empty — an
        // all-read-only Run leaves it unset, so `${PDO_WRITABLE_SECONDARY_REPOS:-}`
        // is the safe read.
        let writable = ctx
            .secondary_repos
            .iter()
            .filter(|s| !s.read_only)
            .map(|s| format!("{}={}", s.alias, s.abs_path))
            .collect::<Vec<_>>()
            .join("\n");
        if !writable.is_empty() {
            env.push(("PDO_WRITABLE_SECONDARY_REPOS".to_string(), writable));
        }
    }

    // HashMap iteration order is non-deterministic; sort so the emitted script
    // bytes are stable (test determinism, ADR-0002 mechanical determinism).
    let mut vars: Vec<_> = ctx.variables.iter().collect();
    vars.sort_by(|a, b| a.0.cmp(b.0));
    for (name, value) in vars {
        env.push((
            format!("PDO_VAR_{}", env_name_suffix(name)),
            var_value_to_env_string(value),
        ));
    }

    // `env_name_suffix` can collapse distinct names onto one env key (`foo-bar`
    // and `foo_bar` → `PDO_INPUT_FOO_BAR`), and the last export silently wins.
    // The mapping is per spec, but a silent shadow is a footgun: say it loudly so
    // an author can rename (ADR-0004, « jamais de comportement silencieux »).
    let mut seen = std::collections::HashSet::new();
    for (key, _) in &env {
        if !seen.insert(key.as_str()) {
            tracing::warn!(
                "script node {}: env var {key} is set more than once — port/variable \
                 names collide after sanitization; the last value wins",
                ctx.node.id
            );
        }
    }

    env
}

/// Pre-create the directory of every declared output port for a `script` node.
/// Agents create these lazily via the Write tool, but a bash
/// `> "$PDO_OUTPUT_out"` fails on a missing parent. Best-effort: a failure here
/// is not fatal — the script's own redirect surfaces any real problem.
pub(crate) fn precreate_output_dirs(ctx: &AugmentContext<'_>) {
    for output in resolve_output_paths(ctx) {
        let dir = match output.port_type {
            PortType::Image | PortType::ImageList => output.path.clone(),
            // html resolves to a file path like markdown, so pre-create its
            // PARENT directory.
            PortType::Markdown | PortType::Html => output
                .path
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or(output.path),
        };
        if let Err(e) = std::fs::create_dir_all(&dir) {
            tracing::warn!(
                "failed to pre-create output dir {} for script node {}: {e}",
                dir.display(),
                ctx.node.id
            );
        }
    }
}

pub(crate) fn build_preamble(ctx: &AugmentContext<'_>) -> String {
    let inputs = resolve_input_paths(ctx);
    let outputs = resolve_output_paths(ctx);

    let mut preamble = String::new();

    preamble.push_str("# PDO Runtime Preamble\n\n");
    preamble.push_str(&format!(
        "You are node `{}` in pipeline `{}`, iteration {}.\n\n",
        ctx.node.id, ctx.pipeline.name, ctx.iter
    ));

    preamble.push_str("## Inputs\n\n");
    if inputs.is_empty() {
        preamble.push_str("No inputs.\n\n");
    } else {
        for input in &inputs {
            // When the pipeline is prompt-optional and no prompt was supplied,
            // the node must source its own work; a supplied prompt is merely
            // additional info layered on the node's own brief.
            if input.from_start && !ctx.pipeline.prompt_required {
                // `from_start` is always a single path.
                let path = input
                    .paths
                    .first()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                if ctx.start_prompt_present {
                    preamble.push_str(&format!(
                        "- `{}` (additional info): read `{}`. \
                         This is supplementary context — your role prompt below is the primary brief.\n",
                        input.port_name, path
                    ));
                } else {
                    preamble.push_str(&format!(
                        "- `{}`: No prompt was provided for this run. \
                         Source your own work from your role prompt below \
                         (an empty `{}` is expected).\n",
                        input.port_name, path
                    ));
                }
            } else if input.repeated {
                // Enumerate one concrete path per completed source iteration: a
                // raw `iter-*` glob would re-include failed iters. An empty pool
                // gets an explicit line, never an orphan glob (ADR-0004).
                if input.paths.is_empty() {
                    preamble.push_str(&format!(
                        "- `{}` (accumulated): no completed iterations yet — nothing to read.\n",
                        input.port_name
                    ));
                } else {
                    preamble.push_str(&format!(
                        "- `{}` (accumulated): read all of these files:\n",
                        input.port_name
                    ));
                    for path in &input.paths {
                        preamble.push_str(&format!("  - {}\n", path.display()));
                    }
                }
            } else {
                let path = input
                    .paths
                    .first()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                preamble.push_str(&format!("- `{}`: read `{}`\n", input.port_name, path));
            }
        }
        preamble.push('\n');
    }

    if !ctx.input_images.is_empty() {
        preamble.push_str("## Input Images\n\n");
        preamble.push_str(
            "The following images were uploaded alongside the text prompt. \
             Use the Read tool to view them:\n",
        );
        let input_dir = ctx.artifacts_dir.join("_input");
        for filename in &ctx.input_images {
            let img_path = input_dir.join(filename);
            preamble.push_str(&format!("- `{}`\n", img_path.display()));
        }
        preamble.push('\n');
    }

    preamble.push_str("## Outputs\n\n");
    if outputs.is_empty() {
        preamble.push_str("No outputs declared.\n\n");
    } else {
        let ext_list = IMAGE_EXTENSIONS.join(", .");
        for output in &outputs {
            let instructions = matches!(
                ctx.node.node_type,
                NodeType::DocOnly | NodeType::CodeMutating
            )
            .then(|| {
                ctx.node
                    .outputs
                    .iter()
                    .find(|port| port.name == output.port_name)
                    .and_then(|port| port.instructions.as_deref())
            })
            .flatten();
            match output.port_type {
                PortType::Image => {
                    preamble.push_str(&format!(
                        "- `{}` (image): drop exactly one image file in `{}`\n",
                        output.port_name,
                        output.path.display(),
                    ));
                    append_output_instructions(&mut preamble, instructions);
                    preamble.push_str(&format!("  Accepted extensions: .{}\n", ext_list));
                }
                PortType::ImageList => {
                    preamble.push_str(&format!(
                        "- `{}` (image_list): drop one or more image files in `{}`\n",
                        output.port_name,
                        output.path.display(),
                    ));
                    append_output_instructions(&mut preamble, instructions);
                    preamble.push_str(&format!("  Accepted extensions: .{}\n", ext_list));
                }
                PortType::Markdown => {
                    preamble.push_str(&format!(
                        "- `{}`: write to `{}`\n",
                        output.port_name,
                        output.path.display()
                    ));
                    append_output_instructions(&mut preamble, instructions);

                    let schema = ctx
                        .node
                        .outputs
                        .iter()
                        .find(|p| p.name == output.port_name)
                        .and_then(|p| p.frontmatter.as_ref());

                    if let Some(schema) = schema {
                        preamble.push_str("  Required YAML frontmatter:\n");
                        for (field_name, field_decl) in schema {
                            if let Some(ref allowed) = field_decl.allowed {
                                preamble.push_str(&format!(
                                    "  - `{}`: {} (allowed: {})\n",
                                    field_name,
                                    field_decl.field_type,
                                    allowed.join(", ")
                                ));
                            } else {
                                preamble.push_str(&format!(
                                    "  - `{}`: {}\n",
                                    field_name, field_decl.field_type
                                ));
                            }
                        }
                    }
                }
                // No frontmatter block, and the agent must know the page renders
                // offline in a scriptless sandboxed iframe (ADR-0028).
                PortType::Html => {
                    preamble.push_str(&format!(
                        "- `{}` (html): write a single self-contained HTML file to `{}`\n",
                        output.port_name,
                        output.path.display(),
                    ));
                    append_output_instructions(&mut preamble, instructions);
                    preamble.push_str(
                        "  Inline all CSS in a `<style>` tag and use no external network \
                         requests (no CDN links, web fonts, or remote assets) — the artifact is \
                         rendered offline.\n\
                         \x20 **JavaScript will NOT run**: the file is displayed in a sandboxed \
                         iframe with scripts disabled. Do not rely on `<script>`, inline event \
                         handlers, or any interactivity — everything must be conveyed through \
                         static HTML and CSS.\n",
                    );
                }
            }
        }

        fn append_output_instructions(preamble: &mut String, instructions: Option<&str>) {
            if let Some(instructions) = instructions {
                let mut lines = instructions.lines();
                if let Some(first_line) = lines.next() {
                    preamble.push_str(&format!("  Expected content: {first_line}\n"));
                }
                for line in lines {
                    preamble.push_str(&format!("  {line}\n"));
                }
            }
        }
        preamble.push('\n');
    }

    // The partial output an INTERRUPTED attempt left behind (ADR-0049). Empty on
    // a first spawn, so this section only appears on a recovery.
    if !ctx.partial_outputs.is_empty() {
        preamble.push_str("## Partial output from an interrupted attempt\n\n");
        preamble.push_str(
            "A previous attempt at THIS node was interrupted (an infra incident, not a \
             failure — ADR-0049) and its partial output survived on disk. It is provided \
             here as **input**: read it first and continue from it. Do **not** blindly \
             overwrite it or start from scratch — keeping this work is the whole point of \
             the restart.\n\n",
        );
        for path in ctx.partial_outputs {
            preamble.push_str(&format!("- read `{}`\n", path.display()));
        }
        preamble.push('\n');
    }

    // Secondary repositories, injected by ABSOLUTE path because a node's
    // sub-worktree does not inherit the snapshot files. Writable by default
    // (ADR-0047); a `read_only` opt-in makes a tracked-file write trip the
    // `secondary_repo_dirtied` guard (409).
    if !ctx.secondary_repos.is_empty() {
        let (writable, read_only): (Vec<_>, Vec<_>) =
            ctx.secondary_repos.iter().partition(|s| !s.read_only);

        preamble.push_str("## Secondary repositories\n\n");
        preamble.push_str(
            "These repositories are associated with this Run (#465). Read and reach them \
             by **absolute path** — your sub-worktree does not contain them. Each is a \
             worktree pinned to a fixed commit.\n\n",
        );

        if !writable.is_empty() {
            preamble.push_str(
                "**Writable** (ADR-0047) — you **MAY** modify, commit, and deliver these \
                 repositories. `git` works inside them (their `.git` is mounted rw in the \
                 sandbox). PDO does **not** deliver them for you: do it yourself from the \
                 repository's own directory (e.g. `git checkout -b …`, commit, then \
                 `gh pr create` / `git push`), exactly as you would the primary. \
                 Uncommitted changes are lost when the Run is torn down. Their \
                 absolute paths are listed below.\n\n",
            );
            for sec in &writable {
                preamble.push_str(&format!(
                    "- `{}` (writable, pinned @ `{}`): `{}`\n",
                    sec.alias, sec.sha, sec.abs_path
                ));
            }
            preamble.push('\n');
        }

        if !read_only.is_empty() {
            preamble.push_str(
                "**Read-only** — these are read-only **context** only. Do **not** modify, \
                 commit in, or open MRs against them; writing to a tracked file will be \
                 refused (`secondary_repo_dirtied`, 409).\n\n",
            );
            for sec in &read_only {
                preamble.push_str(&format!(
                    "- `{}` (read-only, pinned @ `{}`): `{}`\n",
                    sec.alias, sec.sha, sec.abs_path
                ));
            }
            preamble.push('\n');
        }
    }

    if let Some(sub_wt) = ctx.source_worktree_dir {
        preamble.push_str("## Source code edits\n\n");
        preamble.push_str(&format!(
            "Your working directory `{}` is a **dedicated git worktree** of \
             the project, on its own branch. Make **all** source code edits \
             there — do not `cd` elsewhere to edit files. Read with relative \
             paths or paths under this directory.\n\n\
             The input/output artefact paths above live in the *pipeline \
             worktree* (a different directory, shared with other nodes). \
             Treat those paths as read-only/write-only for artefacts; never \
             edit source code there.\n\n\
             When you run `pdo complete`, your committed changes are \
             automatically merged from this sub-worktree back into the \
             pipeline worktree. Edits made outside this directory will be \
             silently dropped from the merge.\n\n",
            sub_wt.display()
        ));

        // The interrupted-git-op notice goes into the re-spawned node's OWN
        // preamble, not only the response the manager sees. Both parts are
        // conditional, so a fresh cut (the common case) gets neither.
        if ctx.reused_sub_worktree {
            preamble.push_str(
                "> **This worktree was REUSED from a previous attempt.** A prior agent may \
                 have left uncommitted work here. Inspect what is already in the working \
                 directory (`git status`, read the changed files) BEFORE you start over — do \
                 not blindly reset or redo work that is already done.\n\n",
            );
        }
        if !ctx.interrupted_git_ops.is_empty() {
            let listed = ctx
                .interrupted_git_ops
                .iter()
                .map(|op| format!("`{op}`"))
                .collect::<Vec<_>>()
                .join(", ");
            preamble.push_str(&format!(
                "> ⚠ **An interrupted git operation was left in this worktree:** {listed}\n>\n"
            ));
            // Vector order is load-bearing: `index.lock` leads, and its
            // instruction ("remove it before anything else") depends on that.
            for op in ctx.interrupted_git_ops {
                let line = match op.as_str() {
                    "index.lock" => "> - `index.lock`: first confirm no git process is running \
                         here, then remove `.git/index.lock` **before anything else** — the \
                         `--abort` / `--continue` commands below themselves need the index lock \
                         free to run.\n"
                        .to_string(),
                    "MERGE_HEAD" => "> - `MERGE_HEAD`: a merge is in progress. Inspect it (`git \
                         status`, `git diff`) — it may carry conflicts a previous agent already \
                         resolved (work worth keeping). Decide deliberately: finish it (`git \
                         commit`) **or** abandon it (`git merge --abort`). Never `--abort` \
                         blindly.\n"
                        .to_string(),
                    "rebase-merge" | "rebase-apply" => "> - a rebase is in progress. `git \
                         status` to inspect; `git rebase --continue` to finish or `git rebase \
                         --abort` to abandon.\n"
                        .to_string(),
                    other => format!("> - `{other}`: resolve this git state before completing.\n"),
                };
                preamble.push_str(&line);
            }
            preamble.push_str(
                ">\n> Do NOT run `pdo complete` until the worktree is in a clean git state — \
                 otherwise the merge-back may record a merge commit nobody intended, \
                 **silently**.\n\n",
            );
        }
    }

    preamble.push_str("## Completion\n\n");
    if ctx.node.interactive {
        preamble.push_str(
            "This is an **interactive** node. Do NOT call `pdo complete`.\n\
             The user will attach to this terminal session, interact with you,\n\
             and click **\"Mark complete\"** in the PDO UI when done.\n\
             Write your outputs to the paths listed above before the user marks complete.\n\n\
             If you cannot complete the task, signal failure:\n\
             ```\n\
             pdo fail --reason \"<description of the problem>\"\n\
             ```\n\n",
        );
    } else {
        preamble.push_str(
            "When you are done, signal completion by running:\n\
             ```\n\
             pdo complete\n\
             ```\n\n\
             **`pdo complete` can be REFUSED**, and its exit code tells you what to do \
             next (#490):\n\
             - **0** — granted, or a legal duplicate. Nothing more to do.\n\
             - **3** — refused, *and it is still your turn*: your outputs are missing or \
             their frontmatter does not match the declared schema. The node is still \
             running and nothing has failed. Fix what stderr lists, then run \
             `pdo complete` again. **Do NOT run `pdo fail`.**\n\
             - **4** — refused, *and the runtime has already ruled*: the failure is \
             already recorded in the run log. **Do NOT run `pdo fail`** — you would \
             record it a second time, with a wrong reason. Stop and report what \
             happened.\n\
             - **1** — the daemon could not be reached or gave no verdict. This is the \
             only case where signalling failure yourself is right.\n\n\
             If you cannot complete the task, signal failure:\n\
             ```\n\
             pdo fail --reason \"<description of the problem>\"\n\
             ```\n\n\
             If there is legitimately nothing to do — your input/pool is empty \
             through no error (e.g. the eligible items were all claimed before \
             you ran) — record a graceful no-op instead of a failure. This ends \
             the run as `skipped` (not `failed`) and short-circuits downstream:\n\
             ```\n\
             pdo skip --reason \"<why there is nothing to do>\"\n\
             ```\n\n",
        );
    }

    if !ctx.variables.is_empty() {
        preamble.push_str("## Pipeline Variables\n\n");
        for (name, value) in ctx.variables {
            let val_str = serde_yaml::to_string(value).unwrap_or_else(|_| format!("{value:?}"));
            preamble.push_str(&format!("- `${name}` = {}\n", val_str.trim()));
        }
        preamble.push('\n');
    }

    if let Some(ref fe) = ctx.foreach_context {
        preamble.push_str("## Collection Context\n\n");
        preamble.push_str(&format!(
            "This node is running as one lap of a collection fan-out ({} of {}).\n",
            fe.current_iter, fe.total
        ));
        preamble.push_str(&format!("- `current_item`: {}\n", fe.current_item));
        preamble.push_str(&format!("- `current_iter`: {}\n", fe.current_iter));
        preamble.push_str(&format!("- `total`: {}\n\n", fe.total));
    }

    preamble
}

pub(crate) fn build_full_prompt(ctx: &AugmentContext<'_>, role_prompt: &str) -> String {
    let preamble = build_preamble(ctx);
    format!("{preamble}---\n\n{role_prompt}")
}

/// Middle tier of `stored → env → default(true)`; resolved by
/// [`default_auto_name_with`].
pub(crate) const DEFAULT_AUTO_NAME_ENV: &str = "PDO_DEFAULT_AUTO_NAME";

/// Built-in default for Run auto-naming: **on** — a Run created with no name is
/// auto-named by the manager.
pub(crate) const DEFAULT_AUTO_NAME_DEFAULT: bool = true;

/// Reuses the shared boolean parser so a typo falls through to the next tier
/// rather than silently meaning `false`.
pub(crate) fn env_default_auto_name() -> Option<bool> {
    std::env::var(DEFAULT_AUTO_NAME_ENV)
        .ok()
        .as_deref()
        .and_then(crate::stale_detector::parse_bool_setting)
}

/// Resolve the instance default for auto-naming: `stored → env → default(true)`.
///
/// `stored` is the raw `instance_config.default_auto_name` column: `Some(0)` is a
/// stored **off** and wins over the env; only SQL `NULL` falls through.
///
/// Only ever the *default* — the create-run chokepoint consults it solely when
/// the request carries neither an explicit `auto_name` flag nor a `name`.
pub(crate) fn default_auto_name_with(stored: Option<i64>) -> bool {
    match stored {
        Some(v) => v != 0,
        None => env_default_auto_name().unwrap_or(DEFAULT_AUTO_NAME_DEFAULT),
    }
}

/// How the manager should treat run naming, decided by the daemon at spawn.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum RunNameHint {
    /// The user supplied a display name — do not rename.
    UserProvided,
    /// No name, but there is input to summarise — name it now from `_input`.
    DeriveFromInput,
    /// No name and no input; a deterministic placeholder was set. Rename best-effort,
    /// only once enough is known, never by polling.
    Placeholder,
}

pub(crate) fn build_manager_preamble(
    run_id: &str,
    daemon_url: &str,
    name_hint: RunNameHint,
) -> String {
    let auto_name_instruction = match name_hint {
        RunNameHint::UserProvided => String::new(),
        RunNameHint::DeriveFromInput =>
            "\n**No display name was provided for this run.** As your first action, read the user input from the `_input` artifact and issue a `rename_run` command with a short, descriptive name (2–5 words) that captures the intent of the run.\n".to_string(),
        RunNameHint::Placeholder =>
            "\n**This run has a placeholder display name** (`Untitled run …`) because it started without a prompt — its real purpose only becomes clear once its nodes do work. Do **not** rename it as your first action, and do **not** poll or block waiting for nodes. Instead, *when you have enough context to know what this run is actually doing* (typically once nodes have produced output, e.g. when the user later engages you), give it a short, descriptive name (2–5 words) via the `rename_run` command. This is best-effort: if you never gain enough context, leave the placeholder in place.\n".to_string(),
    };

    format!(
        r#"# Pipeline Manager Runtime Preamble

You manage **run `{run_id}`**.
{auto_name_instruction}
- Daemon base URL: `{daemon_url}`
- Run state: `curl {daemon_url}/runs/{run_id}`
- Event log: `curl {daemon_url}/runs/{run_id}/events`
- Node pane: `curl {daemon_url}/runs/{run_id}/nodes/<node-id>/pane?iter=<N>`
- Node IO: `curl {daemon_url}/runs/{run_id}/nodes/<node-id>/io?iter=<N>`
- Artifact: `curl '{daemon_url}/runs/{run_id}/artifact?path=<relative-path>'`

## Why a run is not advancing — read the reason, never `journalctl`

The runtime never fails a run on its own initiative (ADR-0049): any non-advancement — an
infra incident (`Interrupted` node), a runtime give-up (stall, output-validation refusal,
merge conflict), or an `unrouted` region/convergence — parks the run `awaiting_user` and
records **why** in the run state itself. You never need the daemon's logs to know the cause.

On `curl {daemon_url}/runs/{run_id}` a parked run carries:

- **`awaiting_reason_code`** — a stable machine slug you branch on: `session_died`,
  `spawn_aborted`, `boot_recovery` (an infra incident on a node); `run_stalled` (nothing
  schedulable); `unrouted` / `region_exhausted` / `region_ended_unrouted` (routing left no
  live path — route it with `end_region`/`bump_region` or the exit edge); `merge_conflict`,
  `merge_resolution_failed`, `script_validation_failed`, `frontmatter_retry_exhausted`,
  `doc_violated_code_immutability` (a completion give-up); `agent_fail_awaiting` (an agent
  `pdo fail` awaiting your confirmation).
- **`awaiting_reason`** — the same cause in prose, for the human.

`awaiting_reason_code` **absent** on an `awaiting_user` run means the wait is *interactive*
(a node is asking its user a question), not an incident — leave it be. Per node, an
interrupted node carries the same cause in `nodes.<id>.failure_reason`. Recover with the
lever the code points to (`restart_node`, `bump_region`/`end_region`, a fix + reopen); the
targeted commands re-open the run themselves.

## Available commands

All commands are issued via `POST {daemon_url}/runs/{run_id}/commands` with a JSON body.

### 1. bump_region

Grant a bounded loop region N more iterations and re-evaluate. This is THE lever for any node that belongs to a loop region — never `extend_cycle`.

```bash
curl -X POST {daemon_url}/runs/{run_id}/commands \
  -H 'Content-Type: application/json' \
  -d '{{"kind":"bump_region","region_id":"<region-id>","additional_iter":<N>}}'
```

**Finding the `region_id`:** it is a key of `loop_states` in `curl {daemon_url}/runs/{run_id}` (the `loop_node_id` of `loop_iter_started` events). A bounded region has a `loop_states` entry **from lap 1** (#601), so "no entry" means "no such loop", not "first lap" — an absent key is a genuine miss to read the pipeline definition's `loops:` block for, not a first-lap blind spot. An unknown `region_id` is rejected with 400 before anything is recorded.

The response tells you what actually happened: `{{"ok":true,"spawned":[…]}}` when nodes were re-launched, or `{{"ok":true,"noop":true,"reason":"…"}}` when nothing was eligible yet (e.g. the region's current iteration is still running — the extra laps then apply when it finishes).

### 2. end_region

Fire a bounded loop region's completion now (route its exit) instead of running more laps. Same `region_id` discovery and same truthful response body as `bump_region`.

```bash
curl -X POST {daemon_url}/runs/{run_id}/commands \
  -H 'Content-Type: application/json' \
  -d '{{"kind":"end_region","region_id":"<region-id>"}}'
```

### 3. resume_run

Re-run the scheduler from the current state. Use after a manual conflict resolution or after extending a cycle on a halted run.

```bash
curl -X POST {daemon_url}/runs/{run_id}/commands \
  -H 'Content-Type: application/json' \
  -d '{{"kind":"resume_run"}}'
```

### 4. kill_node

Kill a running NodeRun's tmux session and emit `node_failed`.

```bash
curl -X POST {daemon_url}/runs/{run_id}/commands \
  -H 'Content-Type: application/json' \
  -d '{{"kind":"kill_node","node_id":"<node-id>","iter":<N>}}'
```

### 5. restart_node

Kill a NodeRun and re-spawn it on the **same iter** with a new session. On a `code-mutating` or `merge` node the sub-worktree is **reused in place**, so the dead session's uncommitted work is still there.

```bash
curl -X POST {daemon_url}/runs/{run_id}/commands \
  -H 'Content-Type: application/json' \
  -d '{{"kind":"restart_node","node_id":"<node-id>","iter":<N>}}'
```

The response tells you what actually happened; it never blanket-claims success (#489, ADR-0037). `200 {{"ok":true,"spawned":[{{"node_id":"…","iter":N}}],"reused_sub_worktree":<bool>,"base_sha":"<sha>"|null,"interrupted_git_ops":["index.lock",…]|[]}}` when the node was re-spawned. The re-spawned agent is told **directly in its own preamble** what to do about a reused worktree and any interrupted git operation left in it, so you do not need to relay instructions — `reused_sub_worktree` and `interrupted_git_ops` are there for your situational awareness (`interrupted_git_ops` lists every marker found, in order — `index.lock`, `MERGE_HEAD`, `rebase-*` — and is `[]` when the reused worktree's git state was clean). `200 {{"ok":true,"waiting":true,"reason":"…"}}` when the session cap queued it: a `NodeWaiting` **was** recorded and the admission sweep owns it — it will spawn, do not re-issue. Otherwise: `409 {{"error":"<slug>","recoverable":<bool>, …}}` with `restart_refused`, `sandbox_prep_not_ready` or `sub_worktree_occupied`; `400 {{"error":"node_not_found"}}`; `500 {{"error":"spawn_failed","run_failed":<bool>}}`. Discriminate on `error`, never on the status.

Every knowable refusal is raised **before** the session is killed: an error body with `session_killed:false` means nothing was touched, so fix the cause and re-issue. `session_killed:true` means the session is gone and nothing replaced it — that node needs a different lever, not a retry of this one.

### 6. mark_node_done

Force-complete a NodeRun (typically an interactive node the user has finished with).

```bash
curl -X POST {daemon_url}/runs/{run_id}/commands \
  -H 'Content-Type: application/json' \
  -d '{{"kind":"mark_node_done","node_id":"<node-id>","iter":<N>}}'
```

This goes through the same shared completion body as `pdo complete`, so it answers truthfully (#490): `200 {{"ok":true}}` when the node completed, `200 {{"ok":true,"noop":true,"reason":"…"}}` on a legal duplicate, and **`409 {{"error":"<slug>","recoverable":<bool>, …}}`** when the completion is refused — `missing_outputs`, `frontmatter_retry_pending`, `frontmatter_retry_exhausted`, `script_validation_failed`, `completion_rejected`, … Discriminate on `error`, never on the status. `recoverable:false` means the runtime already recorded the terminal event: do not try to record it again.

### 7. inject_artifact

Write an artifact directly into the Blackboard.

```bash
curl -X POST {daemon_url}/runs/{run_id}/commands \
  -H 'Content-Type: application/json' \
  -d '{{"kind":"inject_artifact","path":"<node-id>/iter-<N>/<port>/output.md","content":"<markdown content>"}}'
```

### 8. cleanup_run

Archive the run: remove worktrees, branches, and artifacts from disk. Events are preserved.

```bash
curl -X POST {daemon_url}/runs/{run_id}/commands \
  -H 'Content-Type: application/json' \
  -d '{{"kind":"cleanup_run"}}'
```

**Never call `cleanup_run` on your own initiative.** It is destructive and irreversible: it kills every active node session and removes the run's worktrees, branches, and artifacts from disk. Always check with the user first and wait for explicit confirmation before issuing it — even if you believe the run is stuck or finished.

### 9. rename_run

Set or update the display name of this run.

```bash
curl -X POST {daemon_url}/runs/{run_id}/commands \
  -H 'Content-Type: application/json' \
  -d '{{"kind":"rename_run","name":"<display name>"}}'
```

### 10. start_node

Force-spawn a node now, without waiting for its upstream producers to complete. Use when you deliberately want to start a node ahead of its dependencies. Inputs resolve best-effort: any not-yet-produced upstream artifact resolves to the path where it *will* appear, so the node may run against missing or stale inputs. This is reversible — `restart_node` or `kill_node` it if it ran too early.

```bash
curl -X POST {daemon_url}/runs/{run_id}/commands \
  -H 'Content-Type: application/json' \
  -d '{{"kind":"start_node","node_id":"<node-id>"}}'
```

### 11. extend_cycle (legacy)

Increment the iteration ceiling of a *legacy drawn cycle* (a pipeline without a `loops:` block) and re-evaluate. `node_id` is the node whose **outgoing exit condition** references the `$max_iter`-style variable to bump — never the cycle's head/entry node. For any node that belongs to a bounded loop region this command is rejected with 409 — use `bump_region` instead. An unknown `node_id` is rejected with 400. Same truthful response body as `bump_region`.

```bash
curl -X POST {daemon_url}/runs/{run_id}/commands \
  -H 'Content-Type: application/json' \
  -d '{{"kind":"extend_cycle","node_id":"<node-id>","additional_iter":<N>}}'
```

---

"#
    )
}

pub(crate) fn build_manager_prompt(
    run_id: &str,
    daemon_url: &str,
    role_prompt: &str,
    name_hint: RunNameHint,
) -> String {
    let preamble = build_manager_preamble(run_id, daemon_url, name_hint);
    format!("{preamble}{role_prompt}")
}

/// Runtime preamble for the library pipeline authoring assistant (ADR-0048,
/// reshaped by ADR-0051).
///
/// Prepended to `prompts/builtin/library-assistant.md`. Same discipline as the
/// manager: we own the session prompt, so the endpoints are documented in plain
/// text rather than shipping a custom MCP. The **write-on-save** rule is restated
/// here so it holds even if the static role file is trimmed.
///
/// **It names no pipeline.** One assistant serves every template, so a pipeline
/// id frozen at spawn would be wrong from the second template onwards. Which one
/// is open arrives per message via the `UserPromptSubmit` hook; the instruction to
/// fetch it is kept here in plain text because that hook only exists on a harness
/// exposing `--settings` (ADR-0051 §3).
pub(crate) fn build_library_assistant_preamble(daemon_url: &str) -> String {
    format!(
        r#"# Pipeline Assistant Runtime Preamble

You author PDO pipeline templates in natural language, on the user's behalf. You
are **one shared assistant for the whole daemon**: the user switches between
templates without restarting you, so the template you work on changes over time
and your conversation history does not.

## Which pipeline is open

A line beginning `PDO — pipeline actuellement ouverte` is injected into your
context at **every** user message. It names the pipeline id, its scope, and the
**absolute path** of its YAML file. Work on that file, at that path — never on a
sibling you guessed from the working directory.

If that line is missing (some harnesses cannot inject it), fetch the same fact
yourself **before acting**: `curl -s {daemon_url}/sessions/libassist/focus`. If it
answers no pipeline, ask the user which one — do not pick one.

Your working directory is the repo's templates folder; read the `*.yaml` files in
it for real, in-house examples of the format. It is a source of examples, **not**
the location of the file you are editing: that is the absolute path the focus
gives you, and it may live in another folder entirely.

- Daemon base URL: `{daemon_url}`
- Which pipeline is open: `curl -s {daemon_url}/sessions/libassist/focus`
- List every template: `curl {daemon_url}/library/pipelines`
- Validate a single node's YAML before you save: `curl -X POST {daemon_url}/nodes/parse -H 'Content-Type: application/json' -d '{{"yaml":"<node yaml>"}}'`
- Persist the open template: `curl -X POST {daemon_url}/sessions/libassist/save -H 'Content-Type: application/json' -d '{{"yaml":"<full pipeline yaml>","prompts":{{"<node-id>":"<prompt markdown>"}}}}'`

**Saving takes no id and no scope.** The daemon writes into the file the focus
names, because it is the only party that knows where that is. Do **not** save
through `POST /library/pipelines`: it writes into the *library store*, a different
tree from the `.pdo/pipelines/` an edit tab opens — you would leave a duplicate
there, leave the edited file untouched, and report a save that did not happen.

## Write on save, never on every edit

Do **not** touch files as you reason. When the user asks for a change: describe
what you will change, **show a diff**, and write **only after the user says OK**.
Validate node YAML via `POST /nodes/parse` first; then persist the full template
via `POST /sessions/libassist/save`. The canvas re-reads the template the moment
you save — so send the whole file at once, never a half-edited one, and never
write it with `Write`/`Edit`/`sed` (the canvas would not hear about it).

You drive no Run and issue no run commands: your only durable effect is the YAML
the user reviews. Read first, propose second, write last.

"#
    )
}

/// Runtime preamble + the static role prompt. Mirror of
/// [`build_manager_prompt`].
pub(crate) fn build_library_assistant_prompt(daemon_url: &str, role_prompt: &str) -> String {
    let preamble = build_library_assistant_preamble(daemon_url);
    format!("{preamble}{role_prompt}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::{EdgeDef, EdgeEndpoint, NodeType, Port, PortType};

    fn sample_pipeline() -> PipelineDef {
        PipelineDef {
            name: "test-pipe".into(),
            version: Some("1.0".into()),
            variables: HashMap::new(),
            nodes: vec![NodeDef {
                id: "planner".into(),
                name: "planner".into(),
                node_type: NodeType::DocOnly,
                inputs: vec![Port {
                    name: "task".into(),
                    repeated: false,
                    side: None,
                    port_type: PortType::Markdown,
                    frontmatter: None,
                    when: None,
                    description: None,
                    instructions: None,
                    required: false,
                }],
                outputs: vec![Port {
                    name: "plan".into(),
                    repeated: false,
                    side: None,
                    port_type: PortType::Markdown,
                    frontmatter: None,
                    when: None,
                    description: None,
                    instructions: None,
                    required: false,
                }],
                interactive: false,
                view: None,
                max_iter: None,
                over: None,
                pin_harness: None,
                harnesses: Default::default(),
                agent_choice: None,
                auto_fail: None,
            }],
            edges: vec![],
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: true,
        }
    }

    fn sample_ctx<'a>(
        pipeline: &'a PipelineDef,
        node: &'a NodeDef,
        variables: &'a HashMap<String, serde_yaml::Value>,
    ) -> AugmentContext<'a> {
        AugmentContext {
            pipeline,
            node,
            run_id: "20260101-120000-abc1234",
            iter: 1,
            artifacts_dir: Path::new("/repo/.pdo/artifacts"),
            variables,
            daemon_url: "http://localhost:5172",
            foreach_context: None,
            source_worktree_dir: None,
            input_images: Vec::new(),
            start_prompt_present: false,
            source_iters: HashMap::new(),
            repeated_iters: HashMap::new(),
            secondary_repos: Vec::new(),
            reused_sub_worktree: false,
            interrupted_git_ops: &[],
            partial_outputs: &[],
        }
    }

    /// ADR-0047: the secondary preamble branches per pin. A writable secondary is
    /// invited to write/commit/deliver and never labelled read-only; a read-only
    /// one keeps the "do not modify" wording. The section title no longer lies
    /// ("(read-only)" is gone from the header).
    #[test]
    fn secondary_preamble_branches_writable_vs_read_only() {
        let pipeline = sample_pipeline();
        let node = &pipeline.nodes[0];
        let vars = HashMap::new();
        let mut ctx = sample_ctx(&pipeline, node, &vars);
        ctx.secondary_repos = vec![
            SecondaryRepoContext {
                alias: "sdk".into(),
                abs_path: "/work/sdk".into(),
                sha: "aaaa1111".into(),
                read_only: false,
            },
            SecondaryRepoContext {
                alias: "ref".into(),
                abs_path: "/work/ref".into(),
                sha: "bbbb2222".into(),
                read_only: true,
            },
        ];

        let preamble = build_preamble(&ctx);
        // The header no longer claims a global read-only mode.
        assert!(preamble.contains("## Secondary repositories\n"));
        assert!(!preamble.contains("## Secondary repositories (read-only)"));
        // The writable one is invited to deliver; the read-only one is fenced off.
        assert!(preamble.contains("`sdk` (writable"));
        assert!(preamble.contains("MAY** modify, commit, and deliver"));
        assert!(preamble.contains("`ref` (read-only"));
        assert!(preamble.contains("Do **not** modify"));

        // Env: every secondary in PDO_SECONDARY_REPOS, only the writable one in
        // PDO_WRITABLE_SECONDARY_REPOS.
        let env: HashMap<String, String> = build_script_env(&ctx).into_iter().collect();
        assert_eq!(
            env.get("PDO_SECONDARY_REPOS").map(String::as_str),
            Some("sdk=/work/sdk\nref=/work/ref")
        );
        assert_eq!(
            env.get("PDO_WRITABLE_SECONDARY_REPOS").map(String::as_str),
            Some("sdk=/work/sdk")
        );
    }

    /// ADR-0047: an all-read-only Run behaves like the pre-feature read-only mode —
    /// no writable section, no `PDO_WRITABLE_SECONDARY_REPOS` var at all.
    #[test]
    fn all_read_only_secondaries_emit_no_writable_env() {
        let pipeline = sample_pipeline();
        let node = &pipeline.nodes[0];
        let vars = HashMap::new();
        let mut ctx = sample_ctx(&pipeline, node, &vars);
        ctx.secondary_repos = vec![SecondaryRepoContext {
            alias: "ref".into(),
            abs_path: "/work/ref".into(),
            sha: "bbbb2222".into(),
            read_only: true,
        }];

        let preamble = build_preamble(&ctx);
        assert!(!preamble.contains("MAY** modify"));
        assert!(preamble.contains("`ref` (read-only"));

        let env: HashMap<String, String> = build_script_env(&ctx).into_iter().collect();
        assert!(env.contains_key("PDO_SECONDARY_REPOS"));
        assert!(
            !env.contains_key("PDO_WRITABLE_SECONDARY_REPOS"),
            "no writable secondary ⇒ the writable env var must be unset"
        );
    }

    #[test]
    fn input_port_resolves_to_input_md_for_entry_node() {
        let pipeline = sample_pipeline();
        let node = &pipeline.nodes[0];
        let vars = HashMap::new();
        let ctx = sample_ctx(&pipeline, node, &vars);

        let inputs = resolve_input_paths(&ctx);
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].port_name, "task");
        assert_eq!(
            inputs[0].paths[0],
            PathBuf::from("/repo/.pdo/artifacts/_input/output.md")
        );
        assert!(!inputs[0].repeated);
    }

    #[test]
    fn output_port_path_declaration() {
        let pipeline = sample_pipeline();
        let node = &pipeline.nodes[0];
        let vars = HashMap::new();
        let ctx = sample_ctx(&pipeline, node, &vars);

        let outputs = resolve_output_paths(&ctx);
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].port_name, "plan");
        assert_eq!(
            outputs[0].path,
            PathBuf::from("/repo/.pdo/artifacts/planner/iter-1/plan/output.md")
        );
    }

    /// #599 AC1: a written output artifact is detected as surviving partial work;
    /// a missing or empty one is not — that is the ordinary first-spawn state.
    #[test]
    fn surviving_partial_outputs_sees_written_work_and_ignores_empty() {
        let pipeline = sample_pipeline();
        let node = &pipeline.nodes[0]; // planner → markdown output `plan`
        let tmp = tempfile::tempdir().unwrap();
        let artifacts = tmp.path();

        // Nothing written yet → no partial output (first spawn).
        assert!(surviving_partial_outputs(node, artifacts, 1).is_empty());

        // A prior attempt wrote its output.md.
        let out = artifacts.join("planner/iter-1/plan/output.md");
        std::fs::create_dir_all(out.parent().unwrap()).unwrap();
        std::fs::write(&out, "partial work\n").unwrap();
        assert_eq!(surviving_partial_outputs(node, artifacts, 1), vec![out]);

        // An empty file is not partial work.
        let empty = artifacts.join("planner/iter-2/plan/output.md");
        std::fs::create_dir_all(empty.parent().unwrap()).unwrap();
        std::fs::write(&empty, "   \n").unwrap();
        assert!(surviving_partial_outputs(node, artifacts, 2).is_empty());
    }

    /// #599 AC1: when partial output survives, the preamble surfaces it as input to
    /// build on and tells the fresh agent NOT to overwrite it. Absent otherwise.
    #[test]
    fn partial_output_section_is_rendered_only_when_partial_output_survives() {
        let pipeline = sample_pipeline();
        let node = &pipeline.nodes[0];
        let vars = HashMap::new();

        // No partial output → no section.
        let clean = build_preamble(&sample_ctx(&pipeline, node, &vars));
        assert!(!clean.contains("Partial output from an interrupted attempt"));

        // Partial output present → the section, the path, and the do-not-overwrite.
        let paths = vec![PathBuf::from(
            "/repo/.pdo/artifacts/planner/iter-1/plan/output.md",
        )];
        let mut ctx = sample_ctx(&pipeline, node, &vars);
        ctx.partial_outputs = &paths;
        let preamble = build_preamble(&ctx);
        assert!(preamble.contains("## Partial output from an interrupted attempt"));
        assert!(preamble.contains("planner/iter-1/plan/output.md"));
        assert!(preamble.contains("**not**"));
        assert!(preamble.contains("**input**"));
    }

    #[test]
    fn script_env_catalogue_is_built_from_the_same_resolution() {
        // #248: a script node's I/O arrives as PDO_* env vars derived from the
        // same input/output resolution the prose preamble uses.
        let pipeline = sample_pipeline();
        let node = &pipeline.nodes[0]; // planner: input `task`, output `plan`
        let mut vars = HashMap::new();
        vars.insert(
            "max_iter".into(),
            serde_yaml::Value::Number(serde_yaml::Number::from(5)),
        );
        let ctx = sample_ctx(&pipeline, node, &vars);

        let env: HashMap<String, String> = build_script_env(&ctx).into_iter().collect();

        assert_eq!(
            env.get("PDO_ARTIFACTS_DIR").map(String::as_str),
            Some("/repo/.pdo/artifacts")
        );
        assert_eq!(
            env.get("PDO_INPUT_TASK").map(String::as_str),
            Some("/repo/.pdo/artifacts/_input/output.md")
        );
        assert_eq!(
            env.get("PDO_OUTPUT_PLAN").map(String::as_str),
            Some("/repo/.pdo/artifacts/planner/iter-1/plan/output.md")
        );
        assert_eq!(env.get("PDO_VAR_MAX_ITER").map(String::as_str), Some("5"));
    }

    #[test]
    fn script_env_var_values_are_raw_not_yaml_quoted() {
        // #248 regression: serde_yaml::to_string quotes bool-/number-looking
        // strings ("true" → 'true'); those quotes must NOT leak into the env
        // value a script's bash reads. A script consumes raw bytes, so
        // `[ "$PDO_VAR_FLAG" = "true" ]` must compare against `true`, not `'true'`.
        let pipeline = sample_pipeline();
        let node = &pipeline.nodes[0];
        let mut vars = HashMap::new();
        vars.insert("flag".into(), serde_yaml::Value::String("true".into()));
        vars.insert("port".into(), serde_yaml::Value::String("8080".into()));
        vars.insert(
            "count".into(),
            serde_yaml::Value::Number(serde_yaml::Number::from(3)),
        );
        vars.insert("enabled".into(), serde_yaml::Value::Bool(true));
        vars.insert(
            "plain".into(),
            serde_yaml::Value::String("hello world".into()),
        );
        let ctx = sample_ctx(&pipeline, node, &vars);

        let env: HashMap<String, String> = build_script_env(&ctx).into_iter().collect();
        assert_eq!(env.get("PDO_VAR_FLAG").map(String::as_str), Some("true"));
        assert_eq!(env.get("PDO_VAR_PORT").map(String::as_str), Some("8080"));
        assert_eq!(env.get("PDO_VAR_COUNT").map(String::as_str), Some("3"));
        assert_eq!(env.get("PDO_VAR_ENABLED").map(String::as_str), Some("true"));
        assert_eq!(
            env.get("PDO_VAR_PLAIN").map(String::as_str),
            Some("hello world")
        );
    }

    fn script_with_repeated_laps_pipeline() -> PipelineDef {
        let mut pipeline = sample_pipeline();
        pipeline.nodes.push(NodeDef {
            id: "collector".into(),
            name: "collector".into(),
            node_type: NodeType::Script,
            inputs: vec![],
            outputs: vec![],
            interactive: false,
            view: None,
            max_iter: None,
            over: None,
            pin_harness: None,
            harnesses: Default::default(),
            agent_choice: None,
            auto_fail: None,
        });
        pipeline.edges.push(EdgeDef {
            source: EdgeEndpoint {
                node: "planner".into(),
                port: "plan".into(),
            },
            target: EdgeEndpoint {
                node: "collector".into(),
                port: "laps".into(),
            },
            repeated: true,
            ..Default::default()
        });
        pipeline
    }

    #[test]
    fn script_env_marks_repeated_inputs_as_newline_separated_concrete_paths() {
        // #353: a `repeated` edge → PDO_INPUT_<PORT> is the `\n`-joined list of
        // concrete completed-iteration paths (no `iter-*` glob) + a _REPEATED=1
        // flag so the script `readarray -t files <<< "$PDO_INPUT_<PORT>"`.
        let pipeline = script_with_repeated_laps_pipeline();
        let node = pipeline.nodes.iter().find(|n| n.id == "collector").unwrap();
        let vars = HashMap::new();
        let mut ctx = sample_ctx(&pipeline, node, &vars);
        ctx.repeated_iters = HashMap::from([("planner".to_string(), vec![1, 2])]);

        let env: HashMap<String, String> = build_script_env(&ctx).into_iter().collect();
        assert_eq!(
            env.get("PDO_INPUT_LAPS_REPEATED").map(String::as_str),
            Some("1")
        );
        let laps = env.get("PDO_INPUT_LAPS").expect("PDO_INPUT_LAPS is set");
        assert!(!laps.contains("iter-*"), "no raw glob: {laps:?}");
        assert_eq!(
            laps,
            "/repo/.pdo/artifacts/planner/iter-1/plan/output.md\n\
             /repo/.pdo/artifacts/planner/iter-2/plan/output.md",
            "newline-separated concrete paths (a path may contain spaces)"
        );
    }

    #[test]
    fn script_env_repeated_empty_pool_keeps_flag_and_empties_value() {
        // #353 D3: a repeated source with nothing completed → PDO_INPUT_<PORT>
        // is present but empty, _REPEATED=1 still set, so a script can detect
        // "repeated but empty" and `pdo skip`.
        let pipeline = script_with_repeated_laps_pipeline();
        let node = pipeline.nodes.iter().find(|n| n.id == "collector").unwrap();
        let vars = HashMap::new();
        let ctx = sample_ctx(&pipeline, node, &vars); // repeated_iters empty

        let env: HashMap<String, String> = build_script_env(&ctx).into_iter().collect();
        assert_eq!(
            env.get("PDO_INPUT_LAPS_REPEATED").map(String::as_str),
            Some("1")
        );
        assert_eq!(
            env.get("PDO_INPUT_LAPS").map(String::as_str),
            Some(""),
            "present but empty when nothing has completed"
        );
    }

    #[test]
    fn cli_commands_listed_in_preamble() {
        let pipeline = sample_pipeline();
        let node = &pipeline.nodes[0];
        let vars = HashMap::new();
        let ctx = sample_ctx(&pipeline, node, &vars);

        let preamble = build_preamble(&ctx);
        assert!(preamble.contains("pdo complete"));
        assert!(preamble.contains("pdo fail --reason"));
        // #245: non-interactive nodes learn the graceful no-op primitive.
        assert!(preamble.contains("pdo skip --reason"));
    }

    #[test]
    fn iter_value_injection() {
        let pipeline = sample_pipeline();
        let node = &pipeline.nodes[0];
        let vars = HashMap::new();
        let mut ctx = sample_ctx(&pipeline, node, &vars);
        ctx.iter = 3;

        let preamble = build_preamble(&ctx);
        assert!(preamble.contains("iteration 3"));
    }

    #[test]
    fn variables_included_in_preamble() {
        let pipeline = sample_pipeline();
        let node = &pipeline.nodes[0];
        let mut vars = HashMap::new();
        vars.insert(
            "max_iter".into(),
            serde_yaml::Value::Number(serde_yaml::Number::from(5)),
        );
        let ctx = sample_ctx(&pipeline, node, &vars);

        let preamble = build_preamble(&ctx);
        assert!(preamble.contains("$max_iter"));
        assert!(preamble.contains("5"));
    }

    #[test]
    fn edge_based_input_resolution() {
        let mut pipeline = sample_pipeline();
        pipeline.nodes.push(NodeDef {
            id: "implementer".into(),
            name: "implementer".into(),
            node_type: NodeType::CodeMutating,
            inputs: vec![Port {
                name: "plan".into(),
                repeated: false,
                side: None,
                port_type: PortType::Markdown,
                frontmatter: None,
                when: None,
                description: None,
                instructions: None,
                required: false,
            }],
            outputs: vec![Port {
                name: "summary".into(),
                repeated: false,
                side: None,
                port_type: PortType::Markdown,
                frontmatter: None,
                when: None,
                description: None,
                instructions: None,
                required: false,
            }],
            interactive: false,
            view: None,
            max_iter: None,
            over: None,
            pin_harness: None,
            harnesses: Default::default(),
            agent_choice: None,
            auto_fail: None,
        });
        pipeline.edges.push(EdgeDef {
            source: EdgeEndpoint {
                node: "planner".into(),
                port: "plan".into(),
            },
            target: EdgeEndpoint {
                node: "implementer".into(),
                port: "plan".into(),
            },
            reason: None,
            when: None,
            is_else: false,
            repeated: false,
            ..Default::default()
        });

        let node = &pipeline.nodes[1]; // implementer
        let vars = HashMap::new();
        let ctx = sample_ctx(&pipeline, node, &vars);

        let inputs = resolve_input_paths(&ctx);
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].port_name, "plan");
        assert_eq!(
            inputs[0].paths[0],
            PathBuf::from("/repo/.pdo/artifacts/planner/iter-1/plan/output.md")
        );
    }

    #[test]
    fn input_resolves_to_latest_completed_source_iter_not_consumer_iter() {
        // #194: the planner failed at iter 1 (its plan/ artifact is poison) and
        // completed at iter 2. The implementer spawning at iter 1 must read the
        // planner's iter-2 artifact — resolution follows the source's latest
        // COMPLETED iteration, not the consumer's positional iter.
        let mut pipeline = sample_pipeline();
        pipeline.edges.push(EdgeDef {
            source: EdgeEndpoint {
                node: "planner".into(),
                port: "plan".into(),
            },
            target: EdgeEndpoint {
                node: "implementer".into(),
                port: "plan".into(),
            },
            ..Default::default()
        });
        pipeline.nodes.push(NodeDef {
            id: "implementer".into(),
            name: "implementer".into(),
            node_type: NodeType::CodeMutating,
            inputs: vec![],
            outputs: vec![],
            interactive: false,
            view: None,
            max_iter: None,
            over: None,
            pin_harness: None,
            harnesses: Default::default(),
            agent_choice: None,
            auto_fail: None,
        });

        let node = &pipeline.nodes[1]; // implementer
        let vars = HashMap::new();
        let mut ctx = sample_ctx(&pipeline, node, &vars);
        ctx.source_iters = HashMap::from([("planner".to_string(), 2)]);

        let inputs = resolve_input_paths(&ctx);
        assert_eq!(inputs.len(), 1);
        assert_eq!(
            inputs[0].paths[0],
            PathBuf::from("/repo/.pdo/artifacts/planner/iter-2/plan/output.md")
        );
    }

    #[test]
    fn emergent_input_from_edge_with_no_declared_port() {
        // #149: the implementer declares NO inputs; its input is emergent from
        // the incoming edge. The preamble must still enumerate it.
        let mut pipeline = sample_pipeline();
        pipeline.nodes.push(NodeDef {
            id: "implementer".into(),
            name: "implementer".into(),
            node_type: NodeType::CodeMutating,
            inputs: vec![],
            outputs: vec![],
            interactive: false,
            view: None,
            max_iter: None,
            over: None,
            pin_harness: None,
            harnesses: Default::default(),
            agent_choice: None,
            auto_fail: None,
        });
        pipeline.edges.push(EdgeDef {
            source: EdgeEndpoint {
                node: "planner".into(),
                port: "plan".into(),
            },
            target: EdgeEndpoint {
                node: "implementer".into(),
                port: "plan".into(),
            },
            reason: None,
            when: None,
            is_else: false,
            repeated: false,
            ..Default::default()
        });

        let node = &pipeline.nodes[1];
        let vars = HashMap::new();
        let ctx = sample_ctx(&pipeline, node, &vars);

        let inputs = resolve_input_paths(&ctx);
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].port_name, "plan");
        assert_eq!(
            inputs[0].paths[0],
            PathBuf::from("/repo/.pdo/artifacts/planner/iter-1/plan/output.md")
        );
    }

    fn repeated_edge_pipeline() -> PipelineDef {
        // #149: `repeated` (accumulate across iterations) lives on the EDGE, not
        // on a declared input port. planner → implementer, port `plans`.
        let mut pipeline = sample_pipeline();
        pipeline.nodes.push(NodeDef {
            id: "implementer".into(),
            name: "implementer".into(),
            node_type: NodeType::CodeMutating,
            inputs: vec![],
            outputs: vec![],
            interactive: false,
            view: None,
            max_iter: None,
            over: None,
            pin_harness: None,
            harnesses: Default::default(),
            agent_choice: None,
            auto_fail: None,
        });
        pipeline.edges.push(EdgeDef {
            source: EdgeEndpoint {
                node: "planner".into(),
                port: "plan".into(),
            },
            target: EdgeEndpoint {
                node: "implementer".into(),
                port: "plans".into(),
            },
            reason: None,
            when: None,
            is_else: false,
            repeated: true,
            ..Default::default()
        });
        pipeline
    }

    #[test]
    fn repeated_input_enumerates_concrete_completed_iter_paths() {
        // #353: `repeated` resolves to one concrete path per COMPLETED source
        // iteration (via the projected `repeated_iters` set), NEVER a raw
        // `iter-*` glob. planner completed iters 1 and 3 (iter 2 failed) → the
        // pool is iter-1 + iter-3, and the failed iter-2 is quarantined.
        let pipeline = repeated_edge_pipeline();
        let node = &pipeline.nodes[1];
        let vars = HashMap::new();
        let mut ctx = sample_ctx(&pipeline, node, &vars);
        ctx.repeated_iters = HashMap::from([("planner".to_string(), vec![1, 3])]);

        let inputs = resolve_input_paths(&ctx);
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].port_name, "plans");
        assert!(inputs[0].repeated, "repeated comes from the edge");
        assert_eq!(
            inputs[0].paths,
            vec![
                PathBuf::from("/repo/.pdo/artifacts/planner/iter-1/plan/output.md"),
                PathBuf::from("/repo/.pdo/artifacts/planner/iter-3/plan/output.md"),
            ]
        );
        for p in &inputs[0].paths {
            assert!(
                !p.to_string_lossy().contains("iter-*"),
                "no raw glob survives to the agent"
            );
        }

        let preamble = build_preamble(&ctx);
        assert!(preamble.contains("`plans` (accumulated)"));
        assert!(preamble.contains("planner/iter-1/plan/output.md"));
        assert!(preamble.contains("planner/iter-3/plan/output.md"));
        assert!(
            !preamble.contains("iter-2"),
            "the failed iteration is never enumerated"
        );
    }

    #[test]
    fn repeated_input_with_empty_pool_is_an_explicit_line_not_a_glob() {
        // #353 D6 / ADR-0004: a repeated source with no completed iteration
        // yields an empty path list. The preamble says so explicitly (never an
        // orphan glob), and PDO_INPUT_<PORT> is empty while _REPEATED=1 stays.
        let pipeline = repeated_edge_pipeline();
        let node = &pipeline.nodes[1];
        let vars = HashMap::new();
        let ctx = sample_ctx(&pipeline, node, &vars); // repeated_iters empty

        let inputs = resolve_input_paths(&ctx);
        assert_eq!(inputs.len(), 1);
        assert!(inputs[0].repeated);
        assert!(inputs[0].paths.is_empty(), "empty pool when none completed");

        let preamble = build_preamble(&ctx);
        assert!(preamble.contains("`plans` (accumulated)"));
        assert!(preamble.contains("no completed iterations yet"));
        assert!(!preamble.contains("iter-*"));
    }

    #[test]
    fn interactive_node_preamble_omits_pdo_complete_instruction() {
        let mut pipeline = sample_pipeline();
        pipeline.nodes[0].interactive = true;
        let node = &pipeline.nodes[0];
        let vars = HashMap::new();
        let ctx = sample_ctx(&pipeline, node, &vars);

        let preamble = build_preamble(&ctx);
        assert!(
            !preamble.contains("signal completion by running"),
            "interactive node should not instruct to run pdo complete"
        );
        assert!(preamble.contains("Do NOT call `pdo complete`"));
        assert!(preamble.contains("Mark complete"));
        assert!(preamble.contains("pdo fail --reason"));
    }

    #[test]
    fn non_interactive_node_preamble_includes_pdo_complete() {
        let pipeline = sample_pipeline();
        let node = &pipeline.nodes[0];
        assert!(!node.interactive);
        let vars = HashMap::new();
        let ctx = sample_ctx(&pipeline, node, &vars);

        let preamble = build_preamble(&ctx);
        assert!(preamble.contains("pdo complete"));
    }

    #[test]
    fn full_prompt_combines_preamble_and_role() {
        let pipeline = sample_pipeline();
        let node = &pipeline.nodes[0];
        let vars = HashMap::new();
        let ctx = sample_ctx(&pipeline, node, &vars);

        let full = build_full_prompt(&ctx, "You are a planner. Plan well.");
        assert!(full.contains("# PDO Runtime Preamble"));
        assert!(full.contains("You are a planner. Plan well."));
        assert!(full.contains("---"));
    }

    #[test]
    fn multi_input_resolution_from_two_upstream_nodes() {
        let pipeline = PipelineDef {
            name: "multi-input".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                NodeDef {
                    id: "planner".into(),
                    name: "planner".into(),
                    node_type: NodeType::DocOnly,
                    inputs: vec![],
                    outputs: vec![Port {
                        name: "plan".into(),
                        repeated: false,
                        side: None,
                        port_type: PortType::Markdown,
                        frontmatter: None,
                        when: None,
                        description: None,
                        instructions: None,
                        required: false,
                    }],
                    interactive: false,
                    view: None,
                    max_iter: None,
                    over: None,
                    pin_harness: None,
                    harnesses: Default::default(),
                    agent_choice: None,
                    auto_fail: None,
                },
                NodeDef {
                    id: "researcher".into(),
                    name: "researcher".into(),
                    node_type: NodeType::DocOnly,
                    inputs: vec![],
                    outputs: vec![Port {
                        name: "context".into(),
                        repeated: false,
                        side: None,
                        port_type: PortType::Markdown,
                        frontmatter: None,
                        when: None,
                        description: None,
                        instructions: None,
                        required: false,
                    }],
                    interactive: false,
                    view: None,
                    max_iter: None,
                    over: None,
                    pin_harness: None,
                    harnesses: Default::default(),
                    agent_choice: None,
                    auto_fail: None,
                },
                NodeDef {
                    id: "implementer".into(),
                    name: "implementer".into(),
                    node_type: NodeType::CodeMutating,
                    inputs: vec![
                        Port {
                            name: "plan".into(),
                            repeated: false,
                            side: None,
                            port_type: PortType::Markdown,
                            frontmatter: None,
                            when: None,
                            description: None,
                            instructions: None,
                            required: false,
                        },
                        Port {
                            name: "context".into(),
                            repeated: false,
                            side: None,
                            port_type: PortType::Markdown,
                            frontmatter: None,
                            when: None,
                            description: None,
                            instructions: None,
                            required: false,
                        },
                    ],
                    outputs: vec![Port {
                        name: "summary".into(),
                        repeated: false,
                        side: None,
                        port_type: PortType::Markdown,
                        frontmatter: None,
                        when: None,
                        description: None,
                        instructions: None,
                        required: false,
                    }],
                    interactive: false,
                    view: None,
                    max_iter: None,
                    over: None,
                    pin_harness: None,
                    harnesses: Default::default(),
                    agent_choice: None,
                    auto_fail: None,
                },
            ],
            edges: vec![
                EdgeDef {
                    source: EdgeEndpoint {
                        node: "planner".into(),
                        port: "plan".into(),
                    },
                    target: EdgeEndpoint {
                        node: "implementer".into(),
                        port: "plan".into(),
                    },
                    reason: None,
                    when: None,
                    is_else: false,
                    repeated: false,
                    ..Default::default()
                },
                EdgeDef {
                    source: EdgeEndpoint {
                        node: "researcher".into(),
                        port: "context".into(),
                    },
                    target: EdgeEndpoint {
                        node: "implementer".into(),
                        port: "context".into(),
                    },
                    reason: None,
                    when: None,
                    is_else: false,
                    repeated: false,
                    ..Default::default()
                },
            ],
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: true,
        };

        let node = &pipeline.nodes[2]; // implementer
        let vars = HashMap::new();
        let ctx = sample_ctx(&pipeline, node, &vars);

        let inputs = resolve_input_paths(&ctx);
        assert_eq!(inputs.len(), 2);

        let plan_input = inputs.iter().find(|i| i.port_name == "plan").unwrap();
        assert_eq!(
            plan_input.paths[0],
            PathBuf::from("/repo/.pdo/artifacts/planner/iter-1/plan/output.md")
        );

        let ctx_input = inputs.iter().find(|i| i.port_name == "context").unwrap();
        assert_eq!(
            ctx_input.paths[0],
            PathBuf::from("/repo/.pdo/artifacts/researcher/iter-1/context/output.md")
        );

        let preamble = build_preamble(&ctx);
        assert!(preamble.contains("`plan`"));
        assert!(preamble.contains("`context`"));
        assert!(preamble.contains("planner/iter-1/plan/output.md"));
        assert!(preamble.contains("researcher/iter-1/context/output.md"));
    }

    #[test]
    fn frontmatter_schema_injected_in_output_section() {
        let pipeline = PipelineDef {
            name: "review-pipe".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![NodeDef {
                id: "reviewer".into(),
                name: "reviewer".into(),
                node_type: NodeType::DocOnly,
                inputs: vec![Port {
                    name: "code".into(),
                    repeated: false,
                    side: None,
                    port_type: PortType::Markdown,
                    frontmatter: None,
                    when: None,
                    description: None,
                    instructions: None,
                    required: false,
                }],
                outputs: vec![Port {
                    name: "review".into(),
                    repeated: false,
                    side: None,
                    port_type: PortType::Markdown,
                    frontmatter: Some(
                        [(
                            "verdict".into(),
                            crate::pipeline::FrontmatterFieldDecl {
                                field_type: "enum".into(),
                                allowed: Some(vec!["PASS".into(), "FAIL".into()]),
                            },
                        )]
                        .into_iter()
                        .collect(),
                    ),
                    when: None,
                    description: None,
                    instructions: None,
                    required: false,
                }],
                interactive: false,
                view: None,
                max_iter: None,
                over: None,
                pin_harness: None,
                harnesses: Default::default(),
                agent_choice: None,
                auto_fail: None,
            }],
            edges: vec![],
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: true,
        };

        let node = &pipeline.nodes[0];
        let vars = HashMap::new();
        let ctx = sample_ctx(&pipeline, node, &vars);

        let preamble = build_preamble(&ctx);
        assert!(
            preamble.contains("Required YAML frontmatter"),
            "preamble should mention frontmatter schema"
        );
        assert!(
            preamble.contains("`verdict`"),
            "preamble should mention the verdict field"
        );
        assert!(
            preamble.contains("PASS"),
            "preamble should list allowed values"
        );
        assert!(
            preamble.contains("FAIL"),
            "preamble should list allowed values"
        );
    }

    #[test]
    fn output_without_frontmatter_schema_no_schema_section() {
        let pipeline = sample_pipeline();
        let node = &pipeline.nodes[0];
        let vars = HashMap::new();
        let ctx = sample_ctx(&pipeline, node, &vars);

        let preamble = build_preamble(&ctx);
        assert!(
            !preamble.contains("Required YAML frontmatter"),
            "port without schema should not mention frontmatter requirements"
        );
    }

    #[test]
    fn output_instructions_are_scoped_ordered_and_literal_for_every_artifact_type() {
        let cases = [
            (PortType::Markdown, "Required YAML frontmatter:"),
            (PortType::Image, "Accepted extensions:"),
            (PortType::ImageList, "Accepted extensions:"),
            (PortType::Html, "Inline all CSS"),
        ];

        for (port_type, format_constraint) in cases {
            let mut pipeline = sample_pipeline();
            let output = &mut pipeline.nodes[0].outputs[0];
            output.port_type = port_type;
            output.instructions = Some("Keep ${topic} literal.\nSecond line.".into());
            if port_type == PortType::Markdown {
                output.frontmatter = Some(
                    [(
                        "issue_link".into(),
                        crate::pipeline::FrontmatterFieldDecl {
                            field_type: "string".into(),
                            allowed: None,
                        },
                    )]
                    .into_iter()
                    .collect(),
                );
            }

            let node = &pipeline.nodes[0];
            let vars = HashMap::new();
            let preamble = build_preamble(&sample_ctx(&pipeline, node, &vars));
            let output_position = preamble.find("`plan`").expect("output declaration");
            let instructions_position = preamble
                .find("Expected content: Keep ${topic} literal.\n  Second line.")
                .expect("literal multiline instructions");
            let constraint_position = preamble
                .find(format_constraint)
                .expect("artifact format constraint");
            assert!(
                output_position < instructions_position
                    && instructions_position < constraint_position,
                "instructions must stay under their output and before its format constraint: {preamble}"
            );
        }

        let pipeline = sample_pipeline();
        let node = &pipeline.nodes[0];
        let vars = HashMap::new();
        assert!(!build_preamble(&sample_ctx(&pipeline, node, &vars)).contains("Expected content:"));
    }

    #[test]
    fn deterministic_nodes_do_not_inject_preserved_output_instructions() {
        let mut pipeline = sample_pipeline();
        pipeline.nodes[0].node_type = NodeType::Merge;
        pipeline.nodes[0].outputs[0].instructions = Some("This must remain metadata only.".into());

        let node = &pipeline.nodes[0];
        let vars = HashMap::new();
        let preamble = build_preamble(&sample_ctx(&pipeline, node, &vars));

        assert!(!preamble.contains("This must remain metadata only."));
        assert!(!preamble.contains("Expected content:"));
    }

    #[test]
    fn variables_substitution_in_preamble() {
        let pipeline = sample_pipeline();
        let node = &pipeline.nodes[0];
        let mut vars = HashMap::new();
        vars.insert(
            "max_iter_review".into(),
            serde_yaml::Value::Number(serde_yaml::Number::from(10)),
        );
        vars.insert("mode".into(), serde_yaml::Value::String("strict".into()));
        let ctx = sample_ctx(&pipeline, node, &vars);

        let preamble = build_preamble(&ctx);
        assert!(preamble.contains("$max_iter_review"));
        assert!(preamble.contains("10"));
        assert!(preamble.contains("$mode"));
        assert!(preamble.contains("strict"));
    }

    // --- Manager preamble tests (issue #10) ---

    #[test]
    fn manager_preamble_contains_run_id_and_daemon_url() {
        let preamble = build_manager_preamble(
            "20260507-120000-abc1234",
            "http://localhost:5172",
            RunNameHint::UserProvided,
        );
        assert!(preamble.contains("20260507-120000-abc1234"));
        assert!(preamble.contains("http://localhost:5172"));
    }

    #[test]
    fn manager_preamble_contains_all_commands() {
        let preamble =
            build_manager_preamble("run-1", "http://localhost:5172", RunNameHint::UserProvided);
        for cmd in [
            "bump_region",
            "end_region",
            "extend_cycle",
            "resume_run",
            "kill_node",
            "restart_node",
            "mark_node_done",
            "inject_artifact",
            "cleanup_run",
            "rename_run",
            "start_node",
        ] {
            assert!(
                preamble.contains(cmd),
                "preamble should contain command: {cmd}"
            );
        }
    }

    #[test]
    fn manager_preamble_contains_curl_examples() {
        let preamble =
            build_manager_preamble("run-1", "http://localhost:5172", RunNameHint::UserProvided);
        assert!(preamble.contains("curl -X POST"));
        assert!(preamble.contains("Content-Type: application/json"));
    }

    // #447: the preamble takes the URL as a *parameter* — these tests pin the
    // composition with the shared resolver, i.e. what the daemon actually hands it
    // on each path. Asserting the ABSENCE of the wrong host is the load-bearing
    // half: the bug was not a missing URL, it was a plausible wrong one that the
    // manager obeyed and then reported the daemon dead.

    #[test]
    fn manager_preamble_sandboxed_carries_the_container_side_url_only() {
        let url = crate::sandbox_container::daemon_url(6193, true);
        let preamble = build_manager_preamble("run-1", &url, RunNameHint::DeriveFromInput);
        assert!(
            preamble.contains("Daemon base URL: `http://host.docker.internal:6193`"),
            "a sandboxed manager must be told the gateway URL: {preamble}"
        );
        assert!(
            !preamble.contains("localhost:6193"),
            "no `curl` line may keep the host-only URL — inside the container it \
             resolves to the container itself and every command silently fails"
        );
        // The very command the bug swallowed: the first-action rename.
        assert!(preamble.contains("http://host.docker.internal:6193/runs/run-1/commands"));
    }

    #[test]
    fn manager_preamble_host_path_is_unchanged() {
        let url = crate::sandbox_container::daemon_url(6193, false);
        let preamble = build_manager_preamble("run-1", &url, RunNameHint::DeriveFromInput);
        assert!(preamble.contains("Daemon base URL: `http://localhost:6193`"));
        assert!(
            !preamble.contains("host.docker.internal"),
            "non-regression: an `off` Run's manager must never see the gateway host"
        );
    }

    #[test]
    fn manager_prompt_combines_preamble_and_role() {
        let prompt = build_manager_prompt(
            "run-1",
            "http://localhost:5172",
            "You are the Pipeline Manager.",
            RunNameHint::UserProvided,
        );
        assert!(prompt.contains("# Pipeline Manager Runtime Preamble"));
        assert!(prompt.contains("You are the Pipeline Manager."));
    }

    #[test]
    fn manager_preamble_derive_from_input_keeps_existing_instruction() {
        let preamble = build_manager_preamble(
            "run-1",
            "http://localhost:5172",
            RunNameHint::DeriveFromInput,
        );
        assert!(preamble.contains("No display name was provided"));
        assert!(preamble.contains("rename_run"));
    }

    #[test]
    fn manager_preamble_placeholder_is_best_effort_no_poll() {
        let preamble =
            build_manager_preamble("run-1", "http://localhost:5172", RunNameHint::Placeholder);
        assert!(preamble.contains("placeholder display name"));
        assert!(preamble.contains("rename_run"));
        assert!(preamble.contains("best-effort"));
        assert!(preamble.contains("do **not** poll"));
        // The placeholder rename must NOT be front-loaded as the manager's first action.
        assert!(!preamble.contains("As your first action"));
    }

    #[test]
    fn manager_preamble_user_provided_has_no_naming_instruction() {
        let preamble =
            build_manager_preamble("run-1", "http://localhost:5172", RunNameHint::UserProvided);
        assert!(!preamble.contains("No display name was provided"));
        assert!(!preamble.contains("placeholder display name"));
    }

    #[test]
    fn manager_preamble_cleanup_run_has_self_initiative_guardrail() {
        let preamble =
            build_manager_preamble("run-1", "http://localhost:5172", RunNameHint::UserProvided);
        let section = preamble
            .split("### 8. cleanup_run")
            .nth(1)
            .expect("preamble should contain the cleanup_run section")
            .split("### 9.")
            .next()
            .expect("cleanup_run section should be delimited by section 9");
        assert!(
            preamble.contains("### 9. rename_run"),
            "renumbering drifted: section 9 should be rename_run"
        );
        assert!(
            section.contains("Never call `cleanup_run` on your own initiative"),
            "cleanup_run section should carry the self-initiative guardrail, got: {section}"
        );
    }

    #[test]
    fn manager_preamble_region_commands_lead_and_extend_cycle_is_legacy() {
        // ADR-0025 / #327: bump_region/end_region are the primary loop levers
        // (sections 1-2, with the region_id discovery recipe + lap-1 caveat);
        // extend_cycle is demoted to a legacy section with explicit target
        // semantics.
        let preamble =
            build_manager_preamble("run-1", "http://localhost:5172", RunNameHint::UserProvided);
        assert!(preamble.contains("### 1. bump_region"));
        assert!(preamble.contains("### 2. end_region"));
        assert!(preamble.contains("extend_cycle (legacy)"));
        let bump = preamble.find("### 1. bump_region").unwrap();
        let legacy = preamble.find("extend_cycle (legacy)").unwrap();
        assert!(bump < legacy, "bump_region must come before extend_cycle");
        // region_id discovery recipe + first-lap caveat
        assert!(preamble.contains("loop_states"));
        assert!(preamble.contains("first lap"));
        // legacy target semantics: exit-condition node, never the head
        assert!(preamble.contains("never the cycle's head/entry node"));
        assert!(preamble.contains("rejected with 409"));
        // truthful response contract (ADR-0025) is documented
        assert!(preamble.contains("\"noop\":true"));
        assert!(preamble.contains("\"spawned\""));
    }

    // --- image port type preamble tests ---

    #[test]
    fn image_port_preamble_says_drop_exactly_one() {
        let pipeline = PipelineDef {
            name: "img-pipe".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![NodeDef {
                id: "designer".into(),
                name: "designer".into(),
                node_type: NodeType::DocOnly,
                inputs: vec![],
                outputs: vec![Port {
                    name: "screenshot".into(),
                    repeated: false,
                    side: None,
                    port_type: PortType::Image,
                    frontmatter: None,
                    when: None,
                    description: None,
                    instructions: None,
                    required: false,
                }],
                interactive: false,
                view: None,
                max_iter: None,
                over: None,
                pin_harness: None,
                harnesses: Default::default(),
                agent_choice: None,
                auto_fail: None,
            }],
            edges: vec![],
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: true,
        };
        let node = &pipeline.nodes[0];
        let vars = HashMap::new();
        let ctx = sample_ctx(&pipeline, node, &vars);
        let preamble = build_preamble(&ctx);
        assert!(
            preamble.contains("(image)"),
            "preamble should label port as image"
        );
        assert!(
            preamble.contains("exactly one image file"),
            "preamble should say exactly one"
        );
        assert!(preamble.contains(".png"), "preamble should list extensions");
        assert!(
            !preamble.contains("output.md"),
            "image port should not reference output.md"
        );
    }

    #[test]
    fn image_list_port_preamble_says_one_or_more() {
        let pipeline = PipelineDef {
            name: "gallery-pipe".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![NodeDef {
                id: "gallery".into(),
                name: "gallery".into(),
                node_type: NodeType::DocOnly,
                inputs: vec![],
                outputs: vec![Port {
                    name: "photos".into(),
                    repeated: false,
                    side: None,
                    port_type: PortType::ImageList,
                    frontmatter: None,
                    when: None,
                    description: None,
                    instructions: None,
                    required: false,
                }],
                interactive: false,
                view: None,
                max_iter: None,
                over: None,
                pin_harness: None,
                harnesses: Default::default(),
                agent_choice: None,
                auto_fail: None,
            }],
            edges: vec![],
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: true,
        };
        let node = &pipeline.nodes[0];
        let vars = HashMap::new();
        let ctx = sample_ctx(&pipeline, node, &vars);
        let preamble = build_preamble(&ctx);
        assert!(
            preamble.contains("(image_list)"),
            "preamble should label port as image_list"
        );
        assert!(
            preamble.contains("one or more image files"),
            "preamble should say one or more"
        );
    }

    #[test]
    fn image_port_output_path_is_directory_not_file() {
        let pipeline = PipelineDef {
            name: "test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![NodeDef {
                id: "node".into(),
                name: "node".into(),
                node_type: NodeType::DocOnly,
                inputs: vec![],
                outputs: vec![Port {
                    name: "img".into(),
                    repeated: false,
                    side: None,
                    port_type: PortType::Image,
                    frontmatter: None,
                    when: None,
                    description: None,
                    instructions: None,
                    required: false,
                }],
                interactive: false,
                view: None,
                max_iter: None,
                over: None,
                pin_harness: None,
                harnesses: Default::default(),
                agent_choice: None,
                auto_fail: None,
            }],
            edges: vec![],
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: true,
        };
        let node = &pipeline.nodes[0];
        let vars = HashMap::new();
        let ctx = sample_ctx(&pipeline, node, &vars);
        let outputs = resolve_output_paths(&ctx);
        assert_eq!(outputs.len(), 1);
        assert!(
            !outputs[0].path.to_string_lossy().ends_with("output.md"),
            "image port path should be a directory, not output.md"
        );
        assert!(outputs[0].path.to_string_lossy().ends_with("/img"));
    }

    #[test]
    fn discover_input_images_finds_image_files() {
        let tmp = tempfile::tempdir().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        let input_dir = artifacts_dir.join("_input");
        std::fs::create_dir_all(&input_dir).unwrap();
        std::fs::write(input_dir.join("output.md"), "text prompt").unwrap();
        std::fs::write(input_dir.join("screenshot.png"), [0x89]).unwrap();
        std::fs::write(input_dir.join("diagram.jpg"), [0xFF]).unwrap();
        std::fs::write(input_dir.join("notes.txt"), "not an image").unwrap();

        let images = discover_input_images(&artifacts_dir);
        assert_eq!(images, vec!["diagram.jpg", "screenshot.png"]);
    }

    #[test]
    fn discover_input_images_returns_empty_when_no_images() {
        let tmp = tempfile::tempdir().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        let input_dir = artifacts_dir.join("_input");
        std::fs::create_dir_all(&input_dir).unwrap();
        std::fs::write(input_dir.join("output.md"), "text only").unwrap();

        let images = discover_input_images(&artifacts_dir);
        assert!(images.is_empty());
    }

    #[test]
    fn discover_input_images_returns_empty_when_no_input_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();

        let images = discover_input_images(&artifacts_dir);
        assert!(images.is_empty());
    }

    #[test]
    fn preamble_includes_image_section_when_images_present() {
        let pipeline = sample_pipeline();
        let node = &pipeline.nodes[0];
        let vars = HashMap::new();
        let mut ctx = sample_ctx(&pipeline, node, &vars);
        ctx.input_images = vec!["screenshot.png".into(), "diagram.jpg".into()];

        let preamble = build_preamble(&ctx);
        assert!(
            preamble.contains("## Input Images"),
            "preamble should contain Input Images section"
        );
        assert!(preamble.contains("screenshot.png"));
        assert!(preamble.contains("diagram.jpg"));
        assert!(preamble.contains("_input/screenshot.png"));
    }

    #[test]
    fn preamble_omits_image_section_when_no_images() {
        let pipeline = sample_pipeline();
        let node = &pipeline.nodes[0];
        let vars = HashMap::new();
        let ctx = sample_ctx(&pipeline, node, &vars);

        let preamble = build_preamble(&ctx);
        assert!(
            !preamble.contains("Input Images"),
            "preamble should not contain Input Images section when no images"
        );
    }

    // --- #516: the interrupted-git-op notice is ROUTED into the re-spawned node's
    // own preamble. Pure string-in → string-out, exercised directly. ---

    const SUB_WT: &str = "/repo/.pdo/runs/r/nodes/impl-1/iter-1";

    /// **THE #516 case.** A reused worktree carrying BOTH an `index.lock` and a
    /// `MERGE_HEAD` gets a notice that names both markers, gives the differentiated
    /// instruction for each (remove the lock first; inspect-then-finish-or-abort the
    /// merge), and warns that `pdo complete` on a dirty git state records a merge
    /// nobody intended, silently.
    #[test]
    fn preamble_routes_every_interrupted_git_op_with_differentiated_advice() {
        let pipeline = sample_pipeline();
        let node = &pipeline.nodes[0];
        let vars = HashMap::new();
        let mut ctx = sample_ctx(&pipeline, node, &vars);
        ctx.source_worktree_dir = Some(Path::new(SUB_WT));
        ctx.reused_sub_worktree = true;
        let ops = vec!["index.lock".to_string(), "MERGE_HEAD".to_string()];
        ctx.interrupted_git_ops = &ops;

        let preamble = build_preamble(&ctx);

        // The reuse notice.
        assert!(
            preamble.contains("REUSED from a previous attempt"),
            "{preamble}"
        );
        // Both markers surface — neither is masked by the other.
        assert!(preamble.contains("index.lock"), "{preamble}");
        assert!(preamble.contains("MERGE_HEAD"), "{preamble}");
        // Differentiated instructions.
        assert!(
            preamble.contains("remove `.git/index.lock`"),
            "index.lock must get the remove-first instruction: {preamble}"
        );
        assert!(
            preamble.contains("git merge --abort"),
            "MERGE_HEAD must get the finish-or-abort instruction: {preamble}"
        );
        // The load-bearing warning: a silent merge commit is the whole bug.
        assert!(
            preamble.contains("nobody intended") && preamble.contains("**silently**"),
            "the silent-merge warning is the point of #516: {preamble}"
        );
        // Scan order: `index.lock` is mentioned before `MERGE_HEAD`.
        assert!(
            preamble.find("index.lock").unwrap() < preamble.find("MERGE_HEAD").unwrap(),
            "index.lock must lead (remove-first depends on it): {preamble}"
        );
    }

    /// **A/B negative control.** A fresh cut — `reused_sub_worktree=false`,
    /// `interrupted_git_ops=[]` — gets NEITHER notice. Proves the notice is not
    /// unconditional prose that just happens to be true in the positive test.
    #[test]
    fn preamble_omits_both_notices_on_a_fresh_cut() {
        let pipeline = sample_pipeline();
        let node = &pipeline.nodes[0];
        let vars = HashMap::new();
        let mut ctx = sample_ctx(&pipeline, node, &vars);
        ctx.source_worktree_dir = Some(Path::new(SUB_WT));
        ctx.reused_sub_worktree = false;
        ctx.interrupted_git_ops = &[];

        let preamble = build_preamble(&ctx);

        // The base "Source code edits" section is still there…
        assert!(preamble.contains("## Source code edits"), "{preamble}");
        // …but neither the reuse notice nor the interrupted-op notice.
        assert!(
            !preamble.contains("REUSED from a previous attempt"),
            "{preamble}"
        );
        assert!(
            !preamble.contains("interrupted git operation"),
            "{preamble}"
        );
    }

    /// A reused worktree with a CLEAN git state gets the "inspect what is here"
    /// notice but NOT the interrupted-op part.
    #[test]
    fn preamble_reuse_notice_without_ops_when_git_state_is_clean() {
        let pipeline = sample_pipeline();
        let node = &pipeline.nodes[0];
        let vars = HashMap::new();
        let mut ctx = sample_ctx(&pipeline, node, &vars);
        ctx.source_worktree_dir = Some(Path::new(SUB_WT));
        ctx.reused_sub_worktree = true;
        ctx.interrupted_git_ops = &[];

        let preamble = build_preamble(&ctx);
        assert!(
            preamble.contains("REUSED from a previous attempt"),
            "{preamble}"
        );
        assert!(
            !preamble.contains("interrupted git operation"),
            "no ops means no interrupted-op notice: {preamble}"
        );
    }

    // --- entry-node preamble adapts to prompt_required (#158, #274) ---
    //
    // These are now pure string-in -> string-out: each mutates the precomputed
    // `ctx.start_prompt_present` bool, no temp dir or FS access (#274). The matrix
    // {prompt_required} x {start_prompt_present} collapses to 3 distinct phrasings
    // because the bool is dead when a prompt is required.

    #[test]
    fn entry_preamble_tells_node_to_source_own_work_when_prompt_optional_and_empty() {
        let mut pipeline = sample_pipeline();
        pipeline.prompt_required = false;
        let node = &pipeline.nodes[0];
        let vars = HashMap::new();
        let mut ctx = sample_ctx(&pipeline, node, &vars);
        ctx.start_prompt_present = false;

        let preamble = build_preamble(&ctx);
        assert!(
            preamble.contains("No prompt was provided"),
            "entry node with no prompt should be told to source its own work; got:\n{preamble}"
        );
    }

    #[test]
    fn entry_preamble_labels_input_as_additional_info_when_prompt_optional_and_present() {
        let mut pipeline = sample_pipeline();
        pipeline.prompt_required = false;
        let node = &pipeline.nodes[0];
        let vars = HashMap::new();
        let mut ctx = sample_ctx(&pipeline, node, &vars);
        ctx.start_prompt_present = true;

        let preamble = build_preamble(&ctx);
        assert!(
            preamble.contains("additional info"),
            "a prompt on a prompt-optional pipeline should be labelled additional info; got:\n{preamble}"
        );
        assert!(
            !preamble.contains("No prompt was provided"),
            "must not claim the prompt is missing when input is present"
        );
    }

    #[test]
    fn entry_preamble_unchanged_for_prompt_required_pipeline() {
        // The default (prompt-required) keeps the plain input label — neither the
        // "additional info" nor the "source your own work" wording leaks in, and
        // the precomputed bool must not change that (it is dead for this branch).
        let pipeline = sample_pipeline();
        assert!(pipeline.prompt_required);
        let node = &pipeline.nodes[0];
        let vars = HashMap::new();

        for present in [false, true] {
            let mut ctx = sample_ctx(&pipeline, node, &vars);
            ctx.start_prompt_present = present;

            let preamble = build_preamble(&ctx);
            assert!(
                !preamble.contains("No prompt was provided"),
                "start_prompt_present={present} must not leak into a prompt-required pipeline"
            );
            assert!(
                !preamble.contains("additional info"),
                "start_prompt_present={present} must not leak into a prompt-required pipeline"
            );
            assert!(preamble.contains("## Inputs"));
        }
    }

    // --- read_start_prompt_present reader (#274) ---

    #[test]
    fn read_start_prompt_present_true_when_content() {
        let tmp = tempfile::tempdir().unwrap();
        let input_dir = tmp.path().join("_input");
        std::fs::create_dir_all(&input_dir).unwrap();
        std::fs::write(input_dir.join("output.md"), "do the task").unwrap();

        assert!(read_start_prompt_present(tmp.path()).unwrap());
    }

    #[test]
    fn read_start_prompt_present_false_when_whitespace_only() {
        let tmp = tempfile::tempdir().unwrap();
        let input_dir = tmp.path().join("_input");
        std::fs::create_dir_all(&input_dir).unwrap();
        std::fs::write(input_dir.join("output.md"), "   \n").unwrap();

        assert!(!read_start_prompt_present(tmp.path()).unwrap());
    }

    #[test]
    fn read_start_prompt_present_false_when_file_absent() {
        // A missing `_input/output.md` is the expected prompt-optional case, not
        // an I/O error: NotFound maps to Ok(false), never Err.
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path()).unwrap();

        assert!(!read_start_prompt_present(tmp.path()).unwrap());
    }

    /// The primer names **one** write path, and names the other as a trap.
    ///
    /// On a harness with no `--settings` hole the hook never arms (ADR-0051 §3),
    /// so this text is the only thing standing between the assistant and the bug
    /// it used to be instructed into: `POST /library/pipelines` reads `scope` in
    /// the library store's vocabulary, so a `repo` template saved there became a
    /// duplicate in another tree while the edited file never moved.
    #[test]
    fn library_assistant_preamble_points_the_save_at_the_focus() {
        let p = build_library_assistant_preamble("http://localhost:1234");

        assert!(
            p.contains("/sessions/libassist/save"),
            "the primer names the focus-driven save endpoint:\n{p}"
        );
        assert!(
            !p.contains("-X POST http://localhost:1234/library/pipelines"),
            "and never offers the library store as a way to save:\n{p}"
        );
        assert!(
            p.contains("Do **not** save\nthrough `POST /library/pipelines`"),
            "the trap is named outright, so a model that knows the old endpoint is \
             warned off it:\n{p}"
        );
    }
}
