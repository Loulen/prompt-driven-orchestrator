//! A node's prompt: the runtime preamble ([`preamble`]) over what the node
//! reads and writes ([`inputs`]), a `script` node's environment
//! ([`script_env`]), and the Pipeline Manager's own prompt ([`manager`]).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use pdo_pipeline::domain::pipeline::{NodeDef, PipelineDef, PortType};

mod inputs;
mod manager;
mod preamble;
mod script_env;
mod sections;

pub use inputs::*;
pub use manager::*;
pub use preamble::build_preamble;
pub use script_env::*;

/// The image extensions a Run's attachments are told apart by.
pub const ALLOWED_IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "svg", "bmp"];

pub struct InputResolution {
    pub port_name: String,
    /// The upstream node the input comes from.
    pub source_node: String,
    /// A single wire is a one-element list; a `repeated`/pooled input is one path
    /// per COMPLETED source iteration (empty when none completed). Never a raw
    /// `iter-*` glob, which could not exclude a failed iter.
    pub paths: Vec<PathBuf>,
    pub repeated: bool,
    /// Sourced from the Start node's user prompt (`_input/output.md`); the entry
    /// node's prompt label adapts to `prompt_required`.
    pub from_start: bool,
}

pub struct OutputDeclaration {
    pub port_name: String,
    pub path: PathBuf,
    pub port_type: PortType,
}

#[derive(Debug, Clone)]
pub struct ForEachContext {
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
pub struct SecondaryRepoContext {
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
pub fn secondary_repo_contexts(
    repo_root: &Path,
    run_id: &str,
    pins: &[crate::domain::state::RepoPin],
) -> Vec<SecondaryRepoContext> {
    pins.iter()
        .map(|pin| SecondaryRepoContext {
            alias: pin.alias.clone(),
            abs_path: pdo_workspace::adapters::git::worktrees::secondary_snapshot_path(
                repo_root, run_id, &pin.alias,
            )
            .to_string_lossy()
            .to_string(),
            sha: pin.sha.clone(),
            read_only: pin.read_only,
        })
        .collect()
}

pub struct AugmentContext<'a> {
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
    /// For isolated / merge nodes: the per-iteration sub-worktree the
    /// agent must edit in. Set to `None` for nodes that run directly in the
    /// pipeline worktree (non-isolated, switch, loop, etc.).
    pub source_worktree_dir: Option<&'a Path>,
    /// For a NON-isolated agent/script (#654 / ADR-0060): the Run's shared
    /// worktree it works in. Mutually exclusive with `source_worktree_dir` — the
    /// two render the same "Source code edits" section from the two isolations —
    /// and `None` for anything that spawns no session.
    pub shared_worktree_dir: Option<&'a Path>,
    pub input_images: Vec<String>,
    /// Non-image files uploaded alongside the prompt (#779), discovered in
    /// `_input/` at spawn like `input_images`. Entry nodes only.
    pub input_files: Vec<String>,
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
    /// The upstream nodes with no completed iteration yet
    /// ([`crate::domain::input_resolution::absent_sources`]): their inputs are
    /// named absent in the preamble instead of pointing at a file that does not
    /// exist (ADR-0086). Precomputed by the daemon, like `source_iters`.
    pub absent_sources: std::collections::HashSet<String>,
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
    /// The text the extensions place at the very top of the preamble (API
    /// 0.8), before `# PDO Runtime Preamble`; `None` when they give none.
    pub extension_preamble: Option<&'a str>,
}

pub fn build_full_prompt(ctx: &AugmentContext<'_>, role_prompt: &str) -> String {
    let preamble = build_preamble(ctx);
    match toggle_invocations(ctx.node.interactive, ctx.node.orchestrator) {
        Some(lines) => format!("{preamble}---\n\n{role_prompt}\n\n---\n\n{lines}"),
        None => format!("{preamble}---\n\n{role_prompt}"),
    }
}

/// The invocation lines PDO files AFTER the role prompt for the node's toggles
/// (#588 / ADR-0069 §2): `/pdo-interactive` when `interactive` is on,
/// `/pdo-orchestrate` when `orchestrator` is on, in that **spawn order**, one per
/// line, and nothing else — no prose (the seeded skill carries the conduct).
/// Rendered to the session, never editable, never in the author's textarea; the
/// inspector mirrors this exact text under the prompt (`NodeInspector`), byte
/// for byte, so the toggle and the text cannot drift. `None` when no toggle is
/// on: the prompt is then byte-identical to a plain node's.
pub fn toggle_invocations(interactive: bool, orchestrator: bool) -> Option<String> {
    let mut lines: Vec<&str> = Vec::new();
    if interactive {
        lines.push("/pdo-interactive");
    }
    if orchestrator {
        lines.push("/pdo-orchestrate");
    }
    if lines.is_empty() {
        return None;
    }
    let mut out = lines.join("\n");
    out.push('\n');
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdo_pipeline::domain::pipeline::{
        EdgeDef, EdgeEndpoint, EdgeSource, NodeType, Port, PortType,
    };

    fn sample_pipeline() -> PipelineDef {
        PipelineDef {
            name: "test-pipe".into(),
            version: Some("1.0".into()),
            variables: HashMap::new(),
            nodes: vec![NodeDef {
                skills: Vec::new(),
                isolated_worktree: None,
                id: "planner".into(),
                name: "planner".into(),
                node_type: NodeType::Agent,
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
                orchestrator: false,
                on_demand: false,
            }],
            edges: vec![],
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: true,
            grid_size: None,
            wait_for_started_nodes: false,
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
            shared_worktree_dir: None,
            input_images: Vec::new(),
            input_files: Vec::new(),
            start_prompt_present: false,
            source_iters: HashMap::new(),
            absent_sources: Default::default(),
            repeated_iters: HashMap::new(),
            secondary_repos: Vec::new(),
            reused_sub_worktree: false,
            interrupted_git_ops: &[],
            partial_outputs: &[],
            extension_preamble: None,
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
        assert!(surviving_partial_outputs(node, artifacts, 1, None).is_empty());

        // A prior attempt wrote its output.md.
        let out = artifacts.join("planner/iter-1/plan/output.md");
        std::fs::create_dir_all(out.parent().unwrap()).unwrap();
        std::fs::write(&out, "partial work\n").unwrap();
        assert_eq!(
            surviving_partial_outputs(node, artifacts, 1, None),
            vec![out.clone()]
        );

        // An empty file is not partial work.
        let empty = artifacts.join("planner/iter-2/plan/output.md");
        std::fs::create_dir_all(empty.parent().unwrap()).unwrap();
        std::fs::write(&empty, "   \n").unwrap();
        assert!(surviving_partial_outputs(node, artifacts, 2, None).is_empty());

        // #796: written during this run → partial output; older than the run
        // (inherited through git from a previous run) → not this run's work.
        let run_start = |s: &str| {
            Some(
                chrono::DateTime::parse_from_rfc3339(s)
                    .unwrap()
                    .with_timezone(&chrono::Utc),
            )
        };
        assert_eq!(
            surviving_partial_outputs(node, artifacts, 1, run_start("2000-01-01T00:00:00Z")),
            vec![out]
        );
        assert!(
            surviving_partial_outputs(node, artifacts, 1, run_start("2999-01-01T00:00:00Z"))
                .is_empty()
        );
    }

    /// #796 (FP1c): a file git tracks in the run worktree was checked out with
    /// it — a previous run's report on the source branch, NOT an interrupted
    /// attempt of this run — even though the checkout postdates the run's start.
    /// An untracked file written next to it is still this run's partial work; an
    /// image dir whose every entry is tracked is inherited as a whole.
    #[test]
    fn surviving_partial_outputs_ignores_git_tracked_files() {
        let pipeline = sample_pipeline();
        let node = &pipeline.nodes[0]; // planner → markdown output `plan`
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(repo)
                .output()
                .unwrap();
            assert!(out.status.success(), "git {args:?}: {out:?}");
        };
        git(&["init", "-q"]);
        let artifacts = repo.join(".pdo/artifacts");
        let out = artifacts.join("planner/iter-1/plan/output.md");
        std::fs::create_dir_all(out.parent().unwrap()).unwrap();
        std::fs::write(&out, "another ticket's Pass report\n").unwrap();
        git(&["add", "-f", ".pdo/artifacts"]);
        git(&[
            "-c",
            "user.email=t@t",
            "-c",
            "user.name=t",
            "commit",
            "-q",
            "-m",
            "stale",
        ]);

        // Run started BEFORE the checkout wrote the file (the real ordering):
        // the mtime gate lets it through, git does not.
        let run_start = Some(
            chrono::DateTime::parse_from_rfc3339("2000-01-01T00:00:00Z")
                .unwrap()
                .with_timezone(&chrono::Utc),
        );
        assert!(surviving_partial_outputs(node, &artifacts, 1, run_start).is_empty());
        assert!(surviving_partial_outputs(node, &artifacts, 1, None).is_empty());

        // Iter 2, untracked, is handed over.
        let out2 = artifacts.join("planner/iter-2/plan/output.md");
        std::fs::create_dir_all(out2.parent().unwrap()).unwrap();
        std::fs::write(&out2, "this run's partial work\n").unwrap();
        assert_eq!(
            surviving_partial_outputs(node, &artifacts, 2, run_start),
            vec![out2]
        );

        // A dir port: every entry tracked → inherited; one fresh entry → kept.
        let tracked: std::collections::HashSet<PathBuf> = [out.clone()].into_iter().collect();
        assert!(dir_only_tracked(out.parent().unwrap(), &tracked));
        std::fs::write(out.parent().unwrap().join("fresh.md"), "x").unwrap();
        assert!(!dir_only_tracked(out.parent().unwrap(), &tracked));
        std::fs::remove_file(out.parent().unwrap().join("fresh.md")).unwrap();

        // FP1d: an interrupted attempt of THIS run overwrote the tracked path
        // (same port path every run). Still named by `ls-files`, but modified:
        // it is this run's partial work and is handed over on the re-spawn.
        std::fs::write(&out, "this run's partial work over the stale report\n").unwrap();
        assert_eq!(
            surviving_partial_outputs(node, &artifacts, 1, run_start),
            vec![out.clone()]
        );
        assert_eq!(
            surviving_partial_outputs(node, &artifacts, 1, None),
            vec![out]
        );
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
            skills: Vec::new(),
            isolated_worktree: None,
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
            orchestrator: false,
            on_demand: false,
        });
        pipeline.edges.push(EdgeDef {
            source: EdgeSource::single("planner", "plan"),
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
        // #588 / ADR-0069: the declared wait is a capability of every node
        // session; orchestration prose left the preamble for the seeded skill.
        assert!(preamble.contains("pdo wait-user --message"));
        assert!(!preamble.contains("pdo run create"));
        assert!(!preamble.contains("pdo page mount"));
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
            skills: Vec::new(),
            isolated_worktree: None,
            id: "implementer".into(),
            name: "implementer".into(),
            node_type: NodeType::Agent,
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
            orchestrator: false,
            on_demand: false,
        });
        pipeline.edges.push(EdgeDef {
            source: EdgeSource::single("planner", "plan"),
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
            source: EdgeSource::single("planner", "plan"),
            target: EdgeEndpoint {
                node: "implementer".into(),
                port: "plan".into(),
            },
            ..Default::default()
        });
        pipeline.nodes.push(NodeDef {
            skills: Vec::new(),
            isolated_worktree: None,
            id: "implementer".into(),
            name: "implementer".into(),
            node_type: NodeType::Agent,
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
            orchestrator: false,
            on_demand: false,
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
            skills: Vec::new(),
            isolated_worktree: None,
            id: "implementer".into(),
            name: "implementer".into(),
            node_type: NodeType::Agent,
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
            orchestrator: false,
            on_demand: false,
        });
        pipeline.edges.push(EdgeDef {
            source: EdgeSource::single("planner", "plan"),
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
            skills: Vec::new(),
            isolated_worktree: None,
            id: "implementer".into(),
            name: "implementer".into(),
            node_type: NodeType::Agent,
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
            orchestrator: false,
            on_demand: false,
        });
        pipeline.edges.push(EdgeDef {
            source: EdgeSource::single("planner", "plan"),
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
    fn interactive_and_plain_nodes_share_the_same_completion_section() {
        // #588 / ADR-0069 §2: the base preamble is identical for every agentic
        // node — no interactive block, no orchestration prose; the toggle
        // skills carry the conduct.
        let mut interactive_pipeline = sample_pipeline();
        interactive_pipeline.nodes[0].interactive = true;
        let plain_pipeline = sample_pipeline();
        let vars = HashMap::new();
        let interactive = build_preamble(&sample_ctx(
            &interactive_pipeline,
            &interactive_pipeline.nodes[0],
            &vars,
        ));
        let plain = build_preamble(&sample_ctx(
            &plain_pipeline,
            &plain_pipeline.nodes[0],
            &vars,
        ));
        assert_eq!(interactive, plain);

        let completion = plain
            .split("## Completion")
            .nth(1)
            .expect("a Completion section");
        assert!(completion.contains("pdo complete"));
        assert!(completion.contains("pdo skip --reason"));
        assert!(completion.contains("pdo wait-user --message"));
        assert!(!plain.contains("once the user says they are done"));
        assert!(!plain.contains("Mark ready for completion"));
        assert!(!plain.contains("## Orchestration"));
        assert!(!plain.contains("pdo run create"));
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

    /// API 0.8: the extensions' text opens the prompt, before anything of
    /// PDO's; without one, the prompt is unchanged.
    #[test]
    fn the_extension_preamble_comes_first() {
        let pipeline = sample_pipeline();
        let node = &pipeline.nodes[0];
        let vars = HashMap::new();
        let plain = build_full_prompt(&sample_ctx(&pipeline, node, &vars), "Plan.");
        assert!(plain.starts_with("# PDO Runtime Preamble"), "{plain}");

        let mut ctx = sample_ctx(&pipeline, node, &vars);
        ctx.extension_preamble = Some("Ligne de fixture\n");
        let full = build_full_prompt(&ctx, "Plan.");
        assert!(
            full.starts_with("Ligne de fixture\n\n# PDO Runtime Preamble"),
            "{full}"
        );
        assert_eq!(full.strip_prefix("Ligne de fixture\n\n").unwrap(), plain);

        ctx.extension_preamble = Some("  \n");
        assert_eq!(
            build_full_prompt(&ctx, "Plan."),
            plain,
            "a blank text is none"
        );
    }

    #[test]
    fn toggles_file_their_invocation_lines_after_the_role_prompt() {
        // #588 / ADR-0069 §2: the toggles add ONLY the invocation lines, in
        // spawn order (interactive, then orchestrate), after the role prompt —
        // never in the author's textarea, never any prose. No toggle ⇒
        // byte-identical to a plain prompt.
        let vars = HashMap::new();
        let role = "You are a planner. Plan well.";

        let mut both = sample_pipeline();
        both.nodes[0].interactive = true;
        both.nodes[0].orchestrator = true;
        let full = build_full_prompt(&sample_ctx(&both, &both.nodes[0], &vars), role);
        let after_role = full.split(role).nth(1).expect("role prompt present");
        assert_eq!(
            after_role,
            "\n\n---\n\n/pdo-interactive\n/pdo-orchestrate\n"
        );
        assert!(!full.starts_with("/pdo-"));

        let mut orchestrate_only = sample_pipeline();
        orchestrate_only.nodes[0].orchestrator = true;
        let full = build_full_prompt(
            &sample_ctx(&orchestrate_only, &orchestrate_only.nodes[0], &vars),
            role,
        );
        assert!(full.ends_with("\n\n---\n\n/pdo-orchestrate\n"));
        assert!(!full.contains("/pdo-interactive"));
        assert!(!full.contains("## Orchestration"));

        let mut interactive_only = sample_pipeline();
        interactive_only.nodes[0].interactive = true;
        let full = build_full_prompt(
            &sample_ctx(&interactive_only, &interactive_only.nodes[0], &vars),
            role,
        );
        assert!(full.ends_with("\n\n---\n\n/pdo-interactive\n"));

        let plain_pipeline = sample_pipeline();
        let plain = sample_ctx(&plain_pipeline, &plain_pipeline.nodes[0], &vars);
        let bare = build_full_prompt(&plain, "You are a worker. Work.");
        assert!(bare.ends_with("You are a worker. Work."));
        assert!(!bare.contains("/pdo-orchestrate") && !bare.contains("/pdo-interactive"));

        assert_eq!(toggle_invocations(false, false), None);
        assert_eq!(
            toggle_invocations(true, true).as_deref(),
            Some("/pdo-interactive\n/pdo-orchestrate\n")
        );
    }
    #[test]
    fn multi_input_resolution_from_two_upstream_nodes() {
        let pipeline = PipelineDef {
            name: "multi-input".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                NodeDef {
                    skills: Vec::new(),
                    isolated_worktree: None,
                    id: "planner".into(),
                    name: "planner".into(),
                    node_type: NodeType::Agent,
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
                    orchestrator: false,
                    on_demand: false,
                },
                NodeDef {
                    skills: Vec::new(),
                    isolated_worktree: None,
                    id: "researcher".into(),
                    name: "researcher".into(),
                    node_type: NodeType::Agent,
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
                    orchestrator: false,
                    on_demand: false,
                },
                NodeDef {
                    skills: Vec::new(),
                    isolated_worktree: None,
                    id: "implementer".into(),
                    name: "implementer".into(),
                    node_type: NodeType::Agent,
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
                    orchestrator: false,
                    on_demand: false,
                },
            ],
            edges: vec![
                EdgeDef {
                    source: EdgeSource::single("planner", "plan"),
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
                    source: EdgeSource::single("researcher", "context"),
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
            grid_size: None,
            wait_for_started_nodes: false,
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
                skills: Vec::new(),
                isolated_worktree: None,
                id: "reviewer".into(),
                name: "reviewer".into(),
                node_type: NodeType::Agent,
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
                            pdo_pipeline::domain::pipeline::FrontmatterFieldDecl {
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
                orchestrator: false,
                on_demand: false,
            }],
            edges: vec![],
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: true,
            grid_size: None,
            wait_for_started_nodes: false,
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
                        pdo_pipeline::domain::pipeline::FrontmatterFieldDecl {
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
        pipeline.nodes[0].node_type = NodeType::Switch;
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
            "release_node_completion",
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
        let url = pdo_sandbox::domain::container::daemon_url(6193, true);
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
        let url = pdo_sandbox::domain::container::daemon_url(6193, false);
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
            .split("### 9. cleanup_run")
            .nth(1)
            .expect("preamble should contain the cleanup_run section")
            .split("### 10.")
            .next()
            .expect("cleanup_run section should be delimited by section 10");
        assert!(
            preamble.contains("### 10. rename_run"),
            "renumbering drifted: section 10 should be rename_run"
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
                skills: Vec::new(),
                isolated_worktree: None,
                id: "designer".into(),
                name: "designer".into(),
                node_type: NodeType::Agent,
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
                orchestrator: false,
                on_demand: false,
            }],
            edges: vec![],
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: true,
            grid_size: None,
            wait_for_started_nodes: false,
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
                skills: Vec::new(),
                isolated_worktree: None,
                id: "gallery".into(),
                name: "gallery".into(),
                node_type: NodeType::Agent,
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
                orchestrator: false,
                on_demand: false,
            }],
            edges: vec![],
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: true,
            grid_size: None,
            wait_for_started_nodes: false,
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
                skills: Vec::new(),
                isolated_worktree: None,
                id: "node".into(),
                name: "node".into(),
                node_type: NodeType::Agent,
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
                orchestrator: false,
                on_demand: false,
            }],
            edges: vec![],
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: true,
            grid_size: None,
            wait_for_started_nodes: false,
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
    fn discover_input_files_skips_prompt_and_images() {
        // #779: every non-image regular file but `output.md`, sorted.
        let tmp = tempfile::tempdir().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        let input_dir = artifacts_dir.join("_input");
        std::fs::create_dir_all(&input_dir).unwrap();
        std::fs::write(input_dir.join("output.md"), "text prompt").unwrap();
        std::fs::write(input_dir.join("screenshot.PNG"), [0x89]).unwrap();
        std::fs::write(input_dir.join("SPEC-779.md"), "# spec").unwrap();
        std::fs::write(input_dir.join("fixtures.json"), "{}").unwrap();
        std::fs::write(input_dir.join("noext"), "raw").unwrap();

        let files = discover_input_files(&artifacts_dir);
        assert_eq!(files, vec!["SPEC-779.md", "fixtures.json", "noext"]);
        assert!(discover_input_files(tmp.path()).is_empty(), "no _input dir");
    }

    #[test]
    fn preamble_includes_file_section_when_files_present() {
        let pipeline = sample_pipeline();
        let node = &pipeline.nodes[0];
        let vars = HashMap::new();
        let mut ctx = sample_ctx(&pipeline, node, &vars);
        ctx.input_files = vec!["SPEC-779.md".into(), "fixtures.json".into()];

        let preamble = build_preamble(&ctx);
        assert!(preamble.contains("## Input Files"), "{preamble}");
        assert!(preamble.contains("passed to the entry node"), "{preamble}");
        assert!(preamble.contains("_input/SPEC-779.md"));
        assert!(preamble.contains("_input/fixtures.json"));
        assert!(!preamble.contains("## Input Images"));

        ctx.input_files.clear();
        assert!(!build_preamble(&ctx).contains("Input Files"));
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

    // --- #654 / ADR-0060: one "Source code edits" section per isolation, and NO
    // git instruction in either. The Node edits files; the runtime delivers. ---

    /// A NON-isolated NodeRun gets the section too — pre-#654 it got nothing —
    /// and it names the shared worktree plus the delivery contract, without ever
    /// asking for a git command.
    #[test]
    fn preamble_tells_a_shared_worktree_node_that_pdo_delivers_what_it_leaves() {
        let pipeline = sample_pipeline();
        let node = &pipeline.nodes[0];
        let vars = HashMap::new();
        let mut ctx = sample_ctx(&pipeline, node, &vars);
        ctx.shared_worktree_dir = Some(Path::new("/repo/.pdo/runs/r/worktree"));

        let preamble = build_preamble(&ctx);
        assert!(preamble.contains("## Source code edits"), "{preamble}");
        assert!(
            preamble.contains("run's shared worktree")
                && preamble.contains("/repo/.pdo/runs/r/worktree"),
            "it names where the NodeRun works: {preamble}"
        );
        assert!(
            preamble.contains("do not have to run any git command"),
            "the delivery contract replaces every git instruction: {preamble}"
        );
        assert!(
            preamble.contains(".gitignore"),
            "…and says what stays out: {preamble}"
        );
        assert!(
            !preamble.contains("dedicated git worktree"),
            "the two isolations never render together: {preamble}"
        );
    }

    /// The isolated section keeps its "edit here, not elsewhere" rule and states
    /// the same contract — commits kept, the rest committed, then merged back.
    #[test]
    fn preamble_tells_an_isolated_node_its_work_is_committed_then_merged_back() {
        let pipeline = sample_pipeline();
        let node = &pipeline.nodes[0];
        let vars = HashMap::new();
        let mut ctx = sample_ctx(&pipeline, node, &vars);
        ctx.source_worktree_dir = Some(Path::new(SUB_WT));

        let preamble = build_preamble(&ctx);
        assert!(preamble.contains("dedicated git worktree"), "{preamble}");
        assert!(
            preamble.contains("do not have to run any git command"),
            "no git instruction is conditioned on anything any more: {preamble}"
        );
        assert!(
            preamble.contains("keeps whatever you committed yourself"),
            "the node's own commits survive, and it is told so: {preamble}"
        );
        assert!(
            !preamble.contains("run's shared worktree"),
            "the two isolations never render together: {preamble}"
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
}
