//! `POST /nodes/parse`: one node's YAML becomes a canvas-instantiable spec,
//! through the same coercion rules as a whole pipeline.

use crate::domain::library::LibraryEntry;
use crate::domain::pipeline;
use crate::domain::yaml_json::yaml_value_to_json;

/// Node-library `LibraryEntry` fields plus the two identity/layout keys the canvas
/// regenerates (`id`) or re-centres (`view`). Any *other* root key is surfaced as
/// an ignored-field warning (ADR-0001: no silent loss). A field missing from this
/// list is reported as ignored even when it parses fine — a false loss report.
/// Keep it in sync with `LibraryEntry`.
const KNOWN_NODE_KEYS: &[&str] = &[
    "name",
    "type",
    "inputs",
    "outputs",
    "interactive",
    "model",
    "effort",
    "max_iter",
    "branches",
    "prompt",
    // The harness axis. `model`/`effort` above stay accepted — a pasted flat pair
    // is folded into `harnesses.claude` by `normalize_node_value`.
    "pin_harness",
    "harnesses",
    // Carried by `parse_node_spec` but not a `LibraryEntry` field.
    "agent_choice",
    // #653/ADR-0060: where the node works. `normalize_node_value` stamps the
    // type's default onto every `agent`/`script`, so this key is present on
    // virtually every pasted node spec — announcing it as an ignored field would
    // be a warning about a loss that never happens.
    "isolated_worktree",
    // ADR-0086: the « on demand » option.
    "on_demand",
    // Accepted-and-dropped, not a semantic loss → no warning.
    "id",
    "view",
];

/// Pure core of `POST /nodes/parse`, split from the axum handler so it is
/// unit-testable without a router/AppState. `Err(msg)` is the verbatim 400 message.
///
/// The single source of truth for node coercion (serde structs +
/// `pipeline::normalize_node_value`) — the front carries no YAML parser. It never
/// touches the node library and never calls `parse_pipeline`, which requires a
/// top-level `name:` AND a `nodes:` list.
///
/// Hard failures: invalid YAML, non-mapping root, a whole pipeline pasted by
/// mistake, missing `name`, retired `for-each`. Soft losses ride back in
/// `warnings[]` with the node still produced.
pub fn parse_node_spec(yaml: &str) -> Result<serde_json::Value, String> {
    let mut value: serde_yaml::Value =
        serde_yaml::from_str(yaml).map_err(|e| format!("invalid YAML: {e}"))?;

    let Some(map) = value.as_mapping() else {
        return Err(
            "expected a single node definition (a YAML mapping), got a non-mapping document"
                .to_string(),
        );
    };

    // Reject a whole pipeline here, or serde reports a cryptic "missing field
    // `type`" instead.
    if map.contains_key(serde_yaml::Value::String("nodes".into()))
        || map.contains_key(serde_yaml::Value::String("edges".into()))
    {
        return Err(
            "this looks like a whole pipeline, not a node — paste a single node's YAML \
             (import a full pipeline from the pipeline library instead)"
                .to_string(),
        );
    }

    if map.get(serde_yaml::Value::String("name".into())).is_none() {
        return Err("missing required field: name".to_string());
    }

    // 5. Normalize the node `type` in place — shared with `parse_pipeline`:
    //    default/coerce to non-isolated (warnings), refuse retired `for-each`.
    let mut warnings: Vec<String> = pipeline::normalize_node_value(&mut value)
        .map_err(|e| format!("{e}"))?
        .into_iter()
        .map(|d| d.message)
        .collect();

    // Surface every root key serde will silently ignore (ADR-0001).
    if let Some(m) = value.as_mapping() {
        for key in m.keys() {
            if let Some(k) = key.as_str() {
                if !KNOWN_NODE_KEYS.contains(&k) {
                    warnings.push(format!("unknown field '{k}' (ignored)"));
                }
            }
        }
    }

    // Read the harness axis off the value BEFORE the deserialize below consumes it.
    // `normalize_node_value` folded any flat `model:`/`effort:` into
    // `harnesses.claude`, so the claude entry is the source for `spec.model`/
    // `spec.effort`.
    let claude = value
        .get("harnesses")
        .and_then(|h| h.get(pdo_harness::registry::CLAUDE));
    let claude_model = claude
        .and_then(|c| c.get("model"))
        .and_then(|v| v.as_str())
        .map(String::from);
    let claude_effort = claude
        .and_then(|c| c.get("effort"))
        .and_then(|v| v.as_str())
        .map(String::from);
    let pin_harness = value
        .get("pin_harness")
        .and_then(|v| v.as_str())
        .map(String::from);
    let harnesses = value
        .get("harnesses")
        .map(yaml_value_to_json)
        .unwrap_or(serde_json::Value::Null);
    // Carried verbatim, not interpreted. Not a `LibraryEntry` field, so it must be
    // read off the raw value here or the deserialize below silently drops it.
    let agent_choice = value
        .get("agent_choice")
        .map(yaml_value_to_json)
        .unwrap_or(serde_json::Value::Null);
    // 8. Deserialize the (normalized) map into a LibraryEntry. `prompt` defaults
    //    to empty (a structural node carries none); unknown fields are ignored.
    //    #655: `isolated_worktree` is a LibraryEntry field, so the pasted node's
    //    workspace rides through the same door as the rest of it — and
    //    `normalize_node_value` above already stamped the type default onto a
    //    silent `agent`/`script`, so what comes out is never a guess.
    let mut entry: LibraryEntry = serde_yaml::from_value(value).map_err(|e| format!("{e}"))?;
    entry.stamp_isolation();

    Ok(serde_json::json!({
        "spec": {
            "name": entry.name,
            "type": entry.node_type,
            "inputs": entry.inputs,
            "outputs": entry.outputs,
            "interactive": entry.interactive,
            // Claude-aware view, derived from the folded harnesses map.
            "model": claude_model,
            "effort": claude_effort,
            "pin_harness": pin_harness,
            "harnesses": harnesses,
            "agent_choice": agent_choice,
            // #653: where the node works.
            "isolated_worktree": entry.isolated_worktree,
            // ADR-0086: the node waits for the user's ▶.
            "on_demand": entry.on_demand,
            "max_iter": entry.max_iter,
            "branches": entry.branches,
        },
        "prompt": entry.prompt,
        "warnings": warnings,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- POST /nodes/parse: single-node YAML → canvas-instantiable spec.
    // The core (`parse_node_spec`) is a pure fn, so most cases test it directly;
    // two router oneshots confirm the route is actually mounted (the FP's
    // headline "404 blocking" risk) and maps Ok/Err to 200/400. ---

    #[test]
    fn parse_node_spec_happy_path_library_shape() {
        // Exactly what the front export emits (LibraryEntry shape) → no warnings.
        let body = parse_node_spec(
            "name: Writer\ntype: agent\nisolated_worktree: false\noutputs:\n  - name: adr\nprompt: |\n  Write an ADR.\n",
        )
        .unwrap();
        assert_eq!(body["spec"]["type"], "agent");
        assert_eq!(body["spec"]["name"], "Writer");
        assert_eq!(body["spec"]["outputs"][0]["name"], "adr");
        // #653: the pasted node's opt-out of isolation rides through — dropping
        // it would silently fork a sub-worktree on instantiation.
        assert_eq!(body["spec"]["isolated_worktree"], false);
        assert_eq!(body["prompt"], "Write an ADR.\n");
        assert_eq!(body["warnings"].as_array().unwrap().len(), 0);
    }

    /// ADR-0086: a pasted on-demand node stays on demand, without an
    /// ignored-field warning; a structural one is refused.
    #[test]
    fn parse_node_spec_carries_on_demand() {
        let body = parse_node_spec("name: Review\ntype: agent\non_demand: true\n").unwrap();
        assert_eq!(body["spec"]["on_demand"], true);
        assert_eq!(body["warnings"].as_array().unwrap().len(), 0);
        let plain = parse_node_spec("name: Work\ntype: script\n").unwrap();
        assert_eq!(plain["spec"]["on_demand"], false);
        assert!(parse_node_spec("name: E\ntype: end\non_demand: true\n").is_err());
    }

    /// #653 / ADR-0060: a node pasted with no isolation line gets its type's
    /// default stamped and states it in the spec — the canvas never has to
    /// re-derive it. A type that carries no isolation reports `null`.
    #[test]
    fn parse_node_spec_states_the_isolation_for_every_node() {
        for (yaml, expected) in [
            ("name: n\ntype: agent\n", serde_json::json!(true)),
            ("name: n\ntype: script\n", serde_json::json!(false)),
            (
                "name: n\ntype: agent\nisolated_worktree: false\n",
                serde_json::json!(false),
            ),
            ("name: n\ntype: start\n", serde_json::Value::Null),
            // #655: a stray line on a type that carries no isolation is dropped,
            // not round-tripped.
            (
                "name: n\ntype: start\nisolated_worktree: false\n",
                serde_json::Value::Null,
            ),
        ] {
            let body = parse_node_spec(yaml).unwrap();
            assert_eq!(body["spec"]["isolated_worktree"], expected, "{yaml}");
            assert_eq!(
                body["warnings"].as_array().unwrap().len(),
                0,
                "the stamped default is not a loss to warn about: {yaml}"
            );
        }
    }

    #[test]
    fn parse_node_spec_keeps_an_intermediate_output() {
        let body = parse_node_spec(
            "name: Writer\ntype: agent\noutputs:\n  - name: adr\n  - name: draft\n    intermediate: true\n",
        )
        .unwrap();
        assert!(body["warnings"].as_array().unwrap().is_empty(), "{body}");
        assert_eq!(body["spec"]["outputs"][1]["intermediate"], true);
        // Omitted when false, like in the pipeline YAML.
        assert!(body["spec"]["outputs"][0].get("intermediate").is_none());
    }

    #[test]
    fn parse_node_spec_missing_name_is_hard_error() {
        let err = parse_node_spec("type: agent\n").unwrap_err();
        assert!(err.contains("name"), "{err}");
    }

    #[test]
    fn parse_node_spec_invalid_yaml_is_hard_error() {
        let err = parse_node_spec("name: [unterminated\n").unwrap_err();
        assert!(err.contains("invalid YAML"), "{err}");
    }

    #[test]
    fn parse_node_spec_non_mapping_root_is_hard_error() {
        let err = parse_node_spec("- just a list item\n").unwrap_err();
        assert!(err.contains("mapping"), "{err}");
    }

    #[test]
    fn parse_node_spec_whole_pipeline_is_hard_error() {
        for yaml in ["name: p\nnodes: []\n", "name: p\nedges: []\n"] {
            let err = parse_node_spec(yaml).unwrap_err();
            assert!(err.contains("whole pipeline"), "{err}");
        }
    }

    #[test]
    fn parse_node_spec_foreach_is_hard_error() {
        for t in ["for-each", "foreach"] {
            let err = parse_node_spec(&format!("name: n\ntype: {t}\n")).unwrap_err();
            assert!(
                err.contains("removed") && err.contains("pdo migrate"),
                "{err}"
            );
        }
    }

    #[test]
    fn parse_node_spec_missing_type_defaults_agent_with_warning() {
        let body = parse_node_spec("name: n\n").unwrap();
        assert_eq!(body["spec"]["type"], "agent");
        let ws = body["warnings"].as_array().unwrap();
        assert!(
            ws.iter()
                .any(|m| m.as_str().unwrap().contains("missing 'type'")),
            "{ws:?}"
        );
    }

    #[test]
    fn parse_node_spec_unknown_type_coerces_agent_with_warning() {
        let body = parse_node_spec("name: n\ntype: bogus\n").unwrap();
        assert_eq!(body["spec"]["type"], "agent");
        let ws = body["warnings"].as_array().unwrap();
        assert!(
            ws.iter()
                .any(|m| m.as_str().unwrap().contains("unknown node type 'bogus'")),
            "{ws:?}"
        );
    }

    #[test]
    fn parse_node_spec_legacy_types_kept_verbatim() {
        // Legacy switch/loop are NOT coerced to `agent` (silent semantic loss is
        // forbidden, ADR-0001/#268) — imported as-is (a soft warning is fine).
        for t in ["switch", "loop"] {
            let body = parse_node_spec(&format!("name: n\ntype: {t}\n")).unwrap();
            assert_eq!(body["spec"]["type"], t, "legacy {t} must survive");
        }
    }

    #[test]
    fn parse_node_spec_unknown_field_warns_but_still_parses() {
        let body =
            parse_node_spec("name: n\ntype: agent\nisolated_worktree: false\nwidget: 3\n").unwrap();
        assert_eq!(body["spec"]["type"], "agent");
        let ws = body["warnings"].as_array().unwrap();
        assert!(
            ws.iter()
                .any(|m| m.as_str().unwrap().contains("unknown field 'widget'")),
            "{ws:?}"
        );
    }

    #[test]
    fn parse_node_spec_drops_id_and_view_without_warning() {
        // id is regenerated on add, view re-centred — dropping them is not a loss.
        let body = parse_node_spec(
            "name: n\ntype: agent\nisolated_worktree: false\nid: keepme\nview:\n  x: 1\n  y: 2\n",
        )
        .unwrap();
        assert_eq!(body["warnings"].as_array().unwrap().len(), 0);
        assert!(body["spec"].get("id").is_none());
    }

    #[test]
    fn parse_node_spec_model_round_trips() {
        let body = parse_node_spec("name: n\ntype: agent\nisolated_worktree: true\nmodel: opus\n")
            .unwrap();
        assert_eq!(body["spec"]["model"], "opus");
        // A node on the account default omits `model:` → null in the spec.
        let plain = parse_node_spec("name: n\ntype: agent\nisolated_worktree: false\n").unwrap();
        assert!(plain["spec"]["model"].is_null());
    }

    #[test]
    fn parse_node_spec_effort_round_trips() {
        // The only test that catches the `json!` spec in `parse_node_spec`
        // dropping the effort — the macro compiles perfectly without the key, so
        // *Add node from YAML…* would silently create an effort-less node.
        let body = parse_node_spec(
            "name: n\ntype: agent\nisolated_worktree: true\nmodel: opus\neffort: low\n",
        )
        .unwrap();
        assert_eq!(body["spec"]["effort"], "low");
        assert_eq!(body["spec"]["model"], "opus");
        // …and `effort` is a KNOWN node key: a YAML that parses fine must NOT be
        // announced as an ignored field (ADR-0001/#268 forbids the silent loss —
        // and forbids reporting one that did not happen).
        assert_eq!(
            body["warnings"].as_array().unwrap().len(),
            0,
            "effort must not warn: {:?}",
            body["warnings"]
        );
        // A node on the account default omits `effort:` → null in the spec.
        let plain = parse_node_spec("name: n\ntype: agent\nisolated_worktree: false\n").unwrap();
        assert!(plain["spec"]["effort"].is_null());
    }

    #[test]
    fn parse_node_spec_unknown_effort_level_passes_through() {
        // The wire is free-text. A level the curated UI set does not know
        // must reach the spec verbatim, not be dropped or coerced (the picker
        // renders it in a dedicated pass-through segment).
        let body =
            parse_node_spec("name: n\ntype: agent\nisolated_worktree: false\neffort: turbo\n")
                .unwrap();
        assert_eq!(body["spec"]["effort"], "turbo");
        assert_eq!(body["warnings"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn parse_node_spec_prompt_absent_defaults_empty() {
        let body = parse_node_spec("name: n\ntype: agent\nisolated_worktree: false\n").unwrap();
        assert_eq!(body["prompt"], "");
    }
}
