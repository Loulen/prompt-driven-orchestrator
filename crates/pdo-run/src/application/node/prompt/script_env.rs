//! A `script` node's I/O (ADR-0017): its `PDO_*` environment, and its output
//! directories created before the body runs.

use pdo_pipeline::domain::pipeline::PortType;

use super::*;

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
///   when nothing has completed); a single wire holds exactly one path (#353),
///   or nothing when its upstream has produced nothing in this Run (ADR-0086).
/// - `PDO_INPUT_<PORT>_REPEATED=1` — set when that input is a `repeated`/pooled
///   input, so a script knows to `readarray -t files <<< "$PDO_INPUT_<PORT>"`
///   (and to `pdo skip` when the value is empty).
/// - `PDO_OUTPUT_<PORT>` — absolute path the script writes its `output.md` to.
/// - `PDO_LIVE_DIR` — the folder the script publishes its live artifacts in
///   (ADR-0089), set only when the node declares one or may publish freely.
///   Nothing checks it at the end.
/// - `PDO_ITEM` / `PDO_ITEM_TOTAL` — on a lap of a collection fan-out only: the
///   lap's item (a scalar verbatim, a mapping or a list as YAML) and the number
///   of laps.
/// - `PDO_VAR_<NAME>` — each resolved pipeline variable (sorted for determinism).
///
/// The base four (`PDO_RUN_ID`/`NODE_ID`/`NODE_ITER`/`DAEMON_URL`) are exported
/// by `pdo_session::domain::launch::wrap_with_env` and are *not* repeated here.
pub fn build_script_env(ctx: &AugmentContext<'_>) -> Vec<(String, String)> {
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
        // An input with nothing to read (a branch not taken, an upstream that
        // has produced nothing yet) never blanks a same-named input that has.
        if value.is_empty() && env.iter().any(|(k, _)| *k == key) {
            continue;
        }
        env.push((key, value));
    }

    for output in resolve_output_paths(ctx) {
        env.push((
            format!("PDO_OUTPUT_{}", env_name_suffix(&output.port_name)),
            output.path.to_string_lossy().into_owned(),
        ));
    }

    // A lap of a collection fan-out: the item it works on and how many laps the
    // fan-out has (its own index is `PDO_NODE_ITER`) — what an agent lap reads
    // under `## Collection Context`.
    if let Some(fe) = &ctx.foreach_context {
        env.push(("PDO_ITEM".to_string(), fe.current_item.clone()));
        env.push(("PDO_ITEM_TOTAL".to_string(), fe.total.to_string()));
    }

    if !ctx.node.live_artifacts.is_empty() || ctx.node.free_live_artifacts {
        env.push((
            "PDO_LIVE_DIR".to_string(),
            crate::domain::blackboard::live_dir(ctx.artifacts_dir, &ctx.node.id, ctx.iter)
                .to_string_lossy()
                .into_owned(),
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
pub fn precreate_output_dirs(ctx: &AugmentContext<'_>) {
    for output in resolve_output_paths(ctx) {
        let dir = match output.port_type {
            PortType::Image | PortType::ImageList | PortType::Files => output.path.clone(),
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
    // ADR-0089: `> "$PDO_LIVE_DIR/progress.md"` needs the folder too.
    if !ctx.node.live_artifacts.is_empty() || ctx.node.free_live_artifacts {
        let dir = crate::domain::blackboard::live_dir(ctx.artifacts_dir, &ctx.node.id, ctx.iter);
        if let Err(e) = std::fs::create_dir_all(&dir) {
            tracing::warn!(
                "failed to pre-create live dir {} for script node {}: {e}",
                dir.display(),
                ctx.node.id
            );
        }
    }
}
