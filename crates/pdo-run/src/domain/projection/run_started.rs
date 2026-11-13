//! `RunStarted`: the Run's frozen snapshot (graph, sandbox, provisioning,
//! harness, skills, repositories…) lands in the projection.

use tracing::warn;

use super::support::entry_node_ids;
use crate::domain::event::{EdgeInfo, Event, NodeDefInfo};
use crate::domain::state::*;

pub(super) fn apply_run_started(state: &mut RunState, event: &Event) {
    state.started_at = Some(event.ts.clone());
    state.status = RunStatus::Running;
    if let Some(ref payload) = event.payload {
        if let Some(name) = payload.get("pipeline_name").and_then(|v| v.as_str()) {
            state.pipeline_name = name.to_string();
        }
        if let Some(run_name) = payload.get("name").and_then(|v| v.as_str()) {
            if !run_name.is_empty() {
                state.name = Some(run_name.to_string());
            }
        }
        // ADR-0086: written only when set, like the node options.
        state.wait_for_started_nodes = payload
            .get("wait_for_started_nodes")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if let Some(input) = payload.get("input").and_then(|v| v.as_str()) {
            state.input = Some(input.to_string());
        }
        if let Some(variables) = payload.get("variables").and_then(|v| v.as_object()) {
            state.variables = variables
                .iter()
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect();
        }
        if let Some(edges) = payload.get("edges") {
            if let Ok(parsed) = serde_json::from_value::<Vec<EdgeInfo>>(edges.clone()) {
                state.edges = parsed;
            }
        }
        if let Some(node_defs) = payload.get("node_defs") {
            if let Ok(parsed) = serde_json::from_value::<Vec<NodeDefInfo>>(node_defs.clone()) {
                state.node_defs = parsed;
            }
            if let Some(raw) = payload.get("provisioning_rules") {
                match serde_json::from_value::<Vec<pdo_workspace::domain::provisioning::ScopedRules>>(
                    raw.clone(),
                ) {
                    Ok(rules) => state.provisioning_rules = rules,
                    Err(e) => {
                        warn!("run_started carries unreadable provisioning_rules ({raw}): {e}")
                    }
                }
            }
        }
        if let Some(tr) = payload.get("target_repo").and_then(|v| v.as_str()) {
            state.target_repo = Some(tr.to_string());
        }
        // #465 (ADR-0042): the secondary repos, frozen (repo/alias/sha) at
        // Run start. Absent → the mono-repo default (empty), which is what
        // every historical run and every mono-repo create carries. A present-
        // but-unreadable value degrades to empty with a `warn!` (LOUD, never a
        // panic — this runs before the transition guard): losing the read-only
        // context is a soft failure, not a reason to fail the Run replay.
        match payload.get("target_repos") {
            None => {}
            Some(raw) => match serde_json::from_value::<Vec<RepoPin>>(raw.clone()) {
                Ok(pins) => state.target_repos = pins,
                Err(e) => warn!(
                    "run_started carries an unreadable `target_repos` value \
                     ({raw}): {e}; projecting an empty secondary list (the Run \
                     runs mono-repo)."
                ),
            },
        }
        // #407: isolation mode, projected once and never mutated. Absent
        // (or malformed) → the `Off` default (host path). Never panics —
        // this applier runs before the transition guard.
        match payload.get("sandbox") {
            None => {}
            // #426/#432: the two pre-rename tokens are mapped to `Off` + a
            // `warn!` BEFORE `parse` sees them — `parse` is syntactic now and
            // would happily read `copy` as a profile name, turning a Run that
            // predates the rename into a hard `RunFailed`. Only the payload
            // arm keeps this compatibility; the tiers a user can still type
            // (instance default, Trigger) fail loud by design (ADR-0031 §7).
            Some(raw)
                if raw
                    .as_str()
                    .is_some_and(|s| LEGACY_SANDBOX_TOKENS.contains(&s.trim())) =>
            {
                warn!(
                    "run_started carries the pre-#426 `sandbox` token ({raw}); \
                     projecting `off` (host path). `copy`/`pure` were renamed to \
                     `full`/`minimal` in #426, without alias."
                )
            }
            Some(raw) => match serde_json::from_value::<SandboxMode>(raw.clone()) {
                Ok(sandbox) => state.sandbox = sandbox,
                // A blank / non-string value. The degradation goes toward LESS
                // isolation (`Off` → host bash in `run-shell`, `cleanup_run`
                // skipping `merge_back`/`teardown`, cost reading the wrong
                // transcripts root). Silent is not an option: ADR-0030 pt 4
                // forbids an unlogged host fallback.
                Err(_) => warn!(
                    "run_started carries an unreadable `sandbox` value ({raw}); \
                     projecting `off` (host path)."
                ),
            },
        }
        // #432 (ADR-0031 §6): the sibling FROZEN entry list. Written together
        // with `sandbox` or not at all, so an absent key on a `sandbox` payload
        // can only mean "created by a pre-profiles daemon" — which the prep
        // resolves (virtual default) or fails (user profile), per its own
        // decision table. A present-but-unreadable value is kept verbatim in
        // `sandbox_entries_raw_error` so the prep can name it: re-resolving
        // would silently change what the already-spawned nodes saw.
        match payload.get("sandbox_entries") {
            None => {}
            Some(raw) => match serde_json::from_value::<Vec<String>>(raw.clone()) {
                Ok(entries) => state.sandbox_entries = Some(entries),
                Err(_) => {
                    state.sandbox_entries_raw_error = Some(raw.to_string());
                    warn!(
                        "run_started carries an unreadable `sandbox_entries` value \
                         ({raw}); the sandbox prep will fail this Run loud rather \
                         than re-resolve a different list."
                    );
                }
            },
        }
        // #468 (ADR-0031 §8): the sibling FROZEN env. Absent means "no profile
        // env" — indistinguishable from an empty map by construction, so unlike
        // `sandbox_entries` there is no legacy arm to re-resolve. A present-but-
        // unreadable value is kept verbatim so the prep can name it.
        //
        // The `warn!` names the KEY and the shape, never the values: a client token
        // in the systemd journal is an incident, and the journal outlives the Run.
        // `raw` here is the whole malformed value, which is why the arm below is the
        // one exception — it fires only on a payload that is NOT a string map, i.e.
        // one that cannot be carrying the user's values in the first place.
        match payload.get("sandbox_env") {
            None => {}
            Some(raw) => {
                match serde_json::from_value::<std::collections::BTreeMap<String, String>>(
                    raw.clone(),
                ) {
                    Ok(env) => state.sandbox_env = Some(env),
                    Err(_) => {
                        state.sandbox_env_raw_error = Some(raw.to_string());
                        warn!(
                            "run_started carries an unreadable `sandbox_env` value \
                         (not a map of strings); the sandbox prep will fail this Run \
                         loud rather than pose a different environment than the \
                         nodes that already launched saw."
                        );
                    }
                }
            }
        }
        // #467 (ADR-0031 §9): the third sibling, the FROZEN image source. Same shape as
        // the env — absent means "the profile posed none", so the instance-wide setting
        // decides and there is no legacy arm to re-resolve. A present-but-unreadable
        // value is kept verbatim so the prep can name it: degrading to "no image source"
        // would silently start the container in a DIFFERENT image than the nodes that
        // already launched ran in. The `warn!` may carry the raw value — an image ref is
        // not a secret, unlike an env value, and this arm only fires on a payload that is
        // not a valid image source in the first place.
        match payload.get("sandbox_image") {
            None => {}
            Some(raw) => {
                match serde_json::from_value::<pdo_sandbox::domain::image::ProfileImage>(
                    raw.clone(),
                ) {
                    Ok(image) => state.sandbox_image = Some(image),
                    Err(e) => {
                        state.sandbox_image_raw_error = Some(raw.to_string());
                        warn!(
                            "run_started carries an unreadable `sandbox_image` value \
                             ({raw}): {e}; the sandbox prep will fail this Run loud \
                             rather than start it in a different image than the nodes \
                             that already launched ran in."
                        );
                    }
                }
            }
        }
        if let Some(sb) = payload.get("source_branch").and_then(|v| v.as_str()) {
            state.source_branch = Some(sb.to_string());
        }
        // #804 (ADR-0070 §1): the reason a Trigger fire could not refresh the
        // remotes before cutting. Read field by field rather than through a
        // typed `from_value`, so a payload whose `kind` this binary does not
        // know still projects its prose — the Run must stay readable, and this
        // key drives a sentence, nothing else. An entry with no `message` has
        // nothing to say and is dropped rather than rendered empty.
        if let Some(raw) = payload.get("source_fetch_error") {
            let message = raw
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .trim()
                .to_string();
            if !message.is_empty() {
                state.source_fetch_error = Some(SourceFetchError {
                    kind: raw
                        .get("kind")
                        .and_then(|v| v.as_str())
                        .unwrap_or("failed")
                        .to_string(),
                    message,
                });
            }
        }
        // #417: the FROZEN fork point, projected the same way as `source_branch`.
        // The create chokepoint writes this key for every new Run (a resolvable
        // `source_ref`); an absent key (every pre-#417 Run) leaves `None`, and the
        // LOC/diff base then falls back to `source_branch`, then `HEAD`.
        if let Some(fs) = payload.get("fork_sha").and_then(|v| v.as_str()) {
            state.fork_sha = Some(fs.to_string());
        }
        // #551 (ADR-0046): the FROZEN Run harness, projected the same way as
        // `source_branch`. The create chokepoint only writes this key for a
        // non-empty choice, so an absent key (every historical Run, every Run
        // with no explicit harness) leaves `None` — the free nodes then resolve
        // through the instance default and the `claude` floor. A blank string
        // never reaches here (normalised away at the freeze), so it cannot win a
        // tier (#347).
        if let Some(h) = payload.get("harness").and_then(|v| v.as_str()) {
            if !h.is_empty() {
                state.harness = Some(h.to_string());
            }
        }
        // #563 (ADR-0057): the FROZEN Run `AgentChoice`, projected the same
        // way as `harness` — the create chokepoint only writes this key for
        // an explicit (non-`Inherit`) choice, so an absent key (every
        // historical Run, every Run with no explicit choice) leaves `None`
        // and the legacy `harness` above still decides the Run tier.
        if let Some(v) = payload.get("agent_choice") {
            if let Ok(choice) =
                serde_json::from_value::<pdo_harness::agent_choice::AgentChoice>(v.clone())
            {
                state.agent_choice = Some(choice);
            }
        }
        // #669 (ADR-0062): the FROZEN Run-tier skills, written by the create
        // chokepoint only when non-empty; absent ⇒ none (historical Runs).
        if let Some(v) = payload.get("skills") {
            if let Ok(skills) =
                serde_json::from_value::<Vec<pdo_skill::domain::selection::SkillRef>>(v.clone())
            {
                state.skills = skills;
            }
        }
        // ADR-0049: the Run's frozen `auto_fail` tier. Absent key ⇒ `None`
        // (the Run stated no preference — every historical Run).
        if let Some(af) = payload.get("auto_fail").and_then(|v| v.as_bool()) {
            state.auto_fail = Some(af);
        }
        if let Some(tb) = payload.get("triggered_by").and_then(|v| v.as_str()) {
            state.triggered_by = Some(tb.to_string());
        }
        // API 0.8: who created the Run. An absent or unreadable key ⇒ unknown.
        state.created_by = payload
            .get("created_by")
            .and_then(|v| serde_json::from_value(v.clone()).ok());
        state.trigger_editor = payload
            .get("trigger_editor")
            .and_then(|v| serde_json::from_value(v.clone()).ok());
        // The Provenance an extension gave its Run; absent ⇒ none (every Run
        // not created by an extension with one, every historical Run).
        state.provenance = payload
            .get("provenance")
            .and_then(|v| serde_json::from_value(v.clone()).ok());
        // ADR-0064: the mechanical parent provenance the daemon froze at
        // create. Absent keys ⇒ a root run (every historical Run).
        if let Some(pr) = payload.get("parent_run_id").and_then(|v| v.as_str()) {
            state.parent_run_id = Some(pr.to_string());
        }
        if let Some(pn) = payload.get("parent_node_id").and_then(|v| v.as_str()) {
            state.parent_node_id = Some(pn.to_string());
        }
        if let Some(pid) = payload.get("pipeline_id").and_then(|v| v.as_str()) {
            state.pipeline_id = Some(pid.to_string());
        }
        // ADR-0088: the version the Run launched; absent on a Run older than
        // the versions, or not launched from the registry.
        state.pipeline_version = payload
            .get("pipeline_version")
            .and_then(|v| serde_json::from_value(v.clone()).ok());
        // ADR-0087: the Projet recorded at launch; `null` (or, before the
        // migration, absent) ⇒ in no Projet.
        state.project_id = payload
            .get("project_id")
            .and_then(|v| v.as_str())
            .map(str::to_string);

        let string_list = |key: &str| -> Vec<String> {
            payload
                .get(key)
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default()
        };
        let input_images = string_list("image_filenames");
        let input_files = string_list("file_filenames");

        state.start_node = Some(StartNodeInfo {
            input_path: "_input/output.md".to_string(),
            started_at: event.ts.clone(),
            target_node_ids: entry_node_ids(&state.edges, &state.node_defs),
            input_images,
            input_files,
        });

        if let Some(end_def) = state.node_defs.iter().find(|n| n.node_type == "end") {
            state.end_node = Some(EndNodeInfo {
                id: end_def.id.clone(),
                ports: end_def
                    .inputs
                    .iter()
                    .map(|port| EndPortStatus {
                        port_name: port.name.clone(),
                        status: "pending".to_string(),
                        reason: None,
                        fired_at: None,
                    })
                    .collect(),
            });
        }
    }
}
