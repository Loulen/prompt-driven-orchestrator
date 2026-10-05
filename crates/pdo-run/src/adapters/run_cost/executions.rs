//! The Run's frozen executions, read off its `NodeStarted` events, and the
//! transcript files under a Claude project dir.

use super::*;

pub fn collect_jsonl_file(path: &Path, out: &mut Vec<Line>) {
    if let Ok(content) = std::fs::read_to_string(path) {
        out.extend(content.lines().filter_map(parse_line));
    }
}

pub struct Execution {
    pub harness: String,
    pub node_id: Option<String>,
    pub session_id: Option<String>,
    /// The model × effort FROZEN in the startup event (ADR-0046) — the
    /// **requested** values the fold falls back to when the source is mute
    /// (ADR-0065 §1). `None` when the event named nothing.
    pub requested_model: Option<String>,
    pub requested_effort: Option<String>,
    /// Where this execution worked (#653): the store folder of a cwd-keyed harness
    /// (`claude`'s encoded project dir, `pi`'s session folder) derives from it.
    pub working_dir: Option<PathBuf>,
    pub project: Option<PathBuf>,
    pub legacy_claim: bool,
    pub unavailable_reason: Option<String>,
}

/// The Run's **frozen executions** — one per distinct `NodeStarted` identity, with
/// the harness, session id and working directory attribution reads from. Shared by
/// the fold and by the cache key ([`reported_stores_mtime_millis`]), so the two can
/// never disagree about which file belongs to which node.
pub fn collect_executions(
    events: &[crate::domain::event::Event],
    claude_root: &Path,
    repo_root: &Path,
    run_id: &str,
) -> Vec<Execution> {
    let node_defs: Vec<&serde_json::Value> = events
        .iter()
        .find(|event| event.kind == crate::domain::event::EventKind::RunStarted)
        .and_then(|event| event.payload.as_ref())
        .and_then(|payload| payload.get("node_defs"))
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .collect();
    let node_types: BTreeMap<String, String> = node_defs
        .iter()
        .filter_map(|node| {
            Some((
                node.get("id")?.as_str()?.to_string(),
                node.get("node_type")?.as_str()?.to_string(),
            ))
        })
        .collect();
    // #653/ADR-0060: which nodes work in a sub-worktree of their own, from the
    // Run snapshot. Attribution asks this and not the node's type, because the
    // type no longer says where the transcript's `cwd` will be.
    let snapshot_isolation: BTreeMap<String, bool> = node_defs
        .iter()
        .filter_map(|node| {
            let id = node.get("id")?.as_str()?.to_string();
            let isolated = node
                .get("isolated_worktree")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(matches!(
                    node.get("node_type").and_then(serde_json::Value::as_str),
                    Some("agent")
                ));
            Some((id, isolated))
        })
        .collect();
    let node_type = |event: &crate::domain::event::Event| {
        event
            .payload
            .as_ref()
            .and_then(|payload| payload.get("node_type"))
            .and_then(|value| value.as_str())
            .map(str::to_string)
            .or_else(|| {
                event
                    .node_id
                    .as_ref()
                    .and_then(|id| node_types.get(id))
                    .cloned()
            })
    };
    // The NodeRun's FROZEN isolation, else the snapshot's answer for its node.
    let isolated = |event: &crate::domain::event::Event| {
        event
            .payload
            .as_ref()
            .and_then(|payload| payload.get("isolated_worktree"))
            .and_then(serde_json::Value::as_bool)
            .or_else(|| {
                event
                    .node_id
                    .as_ref()
                    .and_then(|id| snapshot_isolation.get(id))
                    .copied()
            })
            .unwrap_or(false)
    };
    let mut executions = Vec::new();
    let mut seen_executions = HashSet::new();
    let mut claimed_legacy_projects = HashSet::new();
    for (order, event) in events.iter().enumerate() {
        if event.kind != crate::domain::event::EventKind::NodeStarted {
            continue;
        }
        let payload = event.payload.as_ref();
        if node_type(event).as_deref() == Some("script") {
            continue;
        }
        let harness = payload
            .and_then(|p| p.get("harness"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(pdo_harness::registry::CLAUDE);
        let session_id = payload
            .and_then(|p| p.get("session_id"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty());
        let requested_model = payload
            .and_then(|p| p.get("model"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let requested_effort = payload
            .and_then(|p| p.get("effort"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let identity = frozen_execution_identity(
            harness,
            session_id,
            event.node_id.as_deref(),
            event.iter,
            event.id,
            order,
        );
        if !seen_executions.insert(identity) {
            continue;
        }
        let is_isolated = isolated(event);
        let working_dir = if is_isolated {
            event.node_id.as_deref().map(|node_id| {
                pdo_workspace::adapters::git::worktrees::sub_worktree_path(
                    repo_root,
                    run_id,
                    node_id,
                    event.iter.unwrap_or(1),
                )
            })
        } else {
            Some(pdo_workspace::adapters::git::worktrees::worktree_dir_for_run(repo_root, run_id))
        };
        let project = working_dir
            .as_ref()
            .map(|working_dir| claude_root.join(cc_project_dirname(working_dir)));
        let legacy_claim = harness == pdo_harness::registry::CLAUDE
            && session_id.is_none()
            && is_isolated
            && project
                .as_ref()
                .is_some_and(|project| claimed_legacy_projects.insert(project.clone()));
        let unavailable_reason = if event.node_id.is_none() {
            Some("missing node identity".to_string())
        } else if harness == pdo_harness::registry::CLAUDE
            && session_id.is_none()
            && is_isolated
            && !legacy_claim
        {
            Some("cost not separable from an earlier start in the same iteration".to_string())
        } else {
            None
        };
        executions.push(Execution {
            harness: harness.to_string(),
            node_id: event.node_id.clone(),
            session_id: session_id.map(str::to_string),
            requested_model,
            requested_effort,
            working_dir,
            project,
            legacy_claim,
            unavailable_reason,
        });
    }
    executions
}

pub fn collect_jsonl_paths(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl_paths(&path, out);
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("jsonl") {
            out.push(path);
        }
    }
}
