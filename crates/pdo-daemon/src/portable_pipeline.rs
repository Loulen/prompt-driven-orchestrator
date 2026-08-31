use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};
use tracing::info;

use crate::agent_choice::AgentChoice;
use crate::pipeline::{self, PipelineDef};

pub(crate) const VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PortablePipelineDocument {
    pdo_pipeline: u32,
    pipeline: serde_yaml::Value,
    #[serde(default)]
    prompts: BTreeMap<String, String>,
}

#[derive(Debug)]
pub(crate) struct InterpretedDocument {
    pub pipeline: PipelineDef,
    pub prompts: HashMap<String, String>,
    /// Non-fatal diagnostics — today, prompts dropped because they name a node
    /// the document does not define.
    pub warnings: Vec<String>,
}

fn make_portable(mut pipeline: PipelineDef) -> PipelineDef {
    for node in &mut pipeline.nodes {
        if matches!(node.agent_choice, Some(AgentChoice::Profile { .. })) {
            node.agent_choice = Some(AgentChoice::Inherit);
        }
    }
    pipeline
}

fn sort_mappings(value: &mut serde_yaml::Value) {
    match value {
        serde_yaml::Value::Sequence(values) => {
            for value in values {
                sort_mappings(value);
            }
        }
        serde_yaml::Value::Mapping(mapping) => {
            let mut entries = std::mem::take(mapping).into_iter().collect::<Vec<_>>();
            for (key, value) in &mut entries {
                sort_mappings(key);
                sort_mappings(value);
            }
            entries.sort_by_cached_key(|(key, _)| serde_yaml::to_string(key).unwrap_or_default());
            mapping.extend(entries);
        }
        _ => {}
    }
}

fn ordered_pipeline_value(pipeline: PipelineDef) -> Result<serde_yaml::Value, String> {
    let mut value = serde_yaml::to_value(pipeline)
        .map_err(|e| format!("failed to serialize portable pipeline: {e}"))?;
    sort_mappings(&mut value);
    Ok(value)
}

pub(crate) fn export(
    pipeline: &PipelineDef,
    prompts: &HashMap<String, String>,
) -> Result<String, String> {
    let portable = make_portable(pipeline.clone());
    let yaml = serde_yaml::to_string(&portable)
        .map_err(|e| format!("failed to validate portable pipeline: {e}"))?;
    let canonical = pipeline::parse_pipeline(&yaml)
        .map_err(|e| format!("failed to validate portable pipeline: {e}"))?
        .pipeline;
    // The exporter's contract has to be the importer's: the prompts come
    // straight off a sidecar dir that can hold leftovers of deleted nodes, and
    // emitting one produced a document PDO itself refuses to import.
    let (live_prompts, orphans) = pipeline::split_live_prompts(&canonical, prompts);
    if !orphans.is_empty() {
        info!(
            "portable export of '{}': dropped {} prompt(s) with no node: {}",
            canonical.name,
            orphans.len(),
            orphans.join(", ")
        );
    }
    serde_yaml::to_string(&PortablePipelineDocument {
        pdo_pipeline: VERSION,
        pipeline: ordered_pipeline_value(canonical)?,
        prompts: live_prompts.into_iter().collect(),
    })
    .map_err(|e| format!("failed to serialize portable pipeline document: {e}"))
}

pub(crate) fn interpret(source: &str) -> Result<InterpretedDocument, String> {
    let value: serde_yaml::Value = serde_yaml::from_str(source).map_err(|e| {
        if let Some(location) = e.location() {
            format!(
                "truncated or invalid pipeline document at line {}: {e}",
                location.line()
            )
        } else {
            format!("truncated or invalid pipeline document: {e}")
        }
    })?;
    let version = value
        .as_mapping()
        .and_then(|map| map.get(serde_yaml::Value::String("pdo_pipeline".into())))
        .and_then(serde_yaml::Value::as_u64)
        .ok_or_else(|| "pdo_pipeline: missing or invalid version".to_string())?;
    if version != u64::from(VERSION) {
        return Err(format!(
            "pdo_pipeline: {version} is not supported (expected {VERSION})"
        ));
    }

    let document: PortablePipelineDocument = serde_yaml::from_value(value).map_err(|e| {
        if e.to_string().starts_with("missing field") {
            format!(
                "truncated or incomplete pipeline document at line {}: {e}",
                source.lines().count().max(1)
            )
        } else {
            format!("invalid pipeline document: {e}")
        }
    })?;
    let portable = serde_yaml::from_value(document.pipeline).map_err(|e| {
        if e.to_string().starts_with("missing field") {
            format!(
                "truncated or incomplete pipeline document at line {}: pipeline: {e}",
                source.lines().count().max(1)
            )
        } else {
            format!("pipeline: invalid definition: {e}")
        }
    })?;
    let portable = make_portable(portable);

    let canonical = serde_yaml::to_string(&portable)
        .map_err(|e| format!("pipeline: failed to validate: {e}"))?;
    let parsed = pipeline::parse_pipeline(&canonical)
        .map_err(|e| format!("pipeline: invalid definition: {e}"))?;
    let dangling = pipeline::dangling_edge_references(&parsed.pipeline);
    if !dangling.is_empty() {
        return Err(format!("pipeline.edges: {}", dangling.join("; ")));
    }

    // A key that cannot be a filename stays fatal: it is an attempt to write
    // outside the sidecar dir, not a leftover.
    if let Some(unsafe_id) = document.prompts.keys().find(|node_id| {
        node_id.is_empty()
            || *node_id == "."
            || *node_id == ".."
            || node_id.contains('/')
            || node_id.contains('\\')
    }) {
        return Err(format!(
            "prompts.{unsafe_id}: node id cannot be used as a prompt filename"
        ));
    }

    // A prompt naming no node, on the other hand, is a droppable leftover
    // Rejecting the whole document over one made pipelines exported by
    // older versions un-importable, on the machine least able to fix them.
    let all_prompts = document.prompts.into_iter().collect::<HashMap<_, _>>();
    let (prompts, orphans) = pipeline::split_live_prompts(&parsed.pipeline, &all_prompts);
    let warnings = orphans
        .iter()
        .map(|node_id| format!("prompts.{node_id}: no such node in the document — prompt ignored"))
        .collect();

    Ok(InterpretedDocument {
        pipeline: parsed.pipeline,
        prompts,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::agent_choice::AgentChoice;
    use crate::pipeline::{NodeDef, NodeType, PipelineDef, Port};

    fn node(id: &str, name: &str, node_type: NodeType) -> NodeDef {
        NodeDef {
            id: id.into(),
            name: name.into(),
            node_type,
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
        }
    }

    fn pipeline() -> PipelineDef {
        let port = |name: &str| Port {
            name: name.into(),
            repeated: false,
            side: None,
            port_type: Default::default(),
            frontmatter: None,
            when: None,
            description: None,
            instructions: None,
            required: false,
        };
        let mut start = node("start", "Start", NodeType::Start);
        start.outputs.push(port("user_prompt"));
        let mut worker = node("worker", "Worker", NodeType::DocOnly);
        worker.agent_choice = Some(AgentChoice::Profile {
            profile_id: "local-reviewer".into(),
        });
        let mut end = node("end", "End", NodeType::End);
        end.inputs.push(port("result"));
        PipelineDef {
            name: "Portable".into(),
            version: Some("1.0".into()),
            variables: HashMap::new(),
            nodes: vec![start, worker, end],
            edges: vec![],
            loops: vec![],
            notes: vec![],
            prompt_required: true,
        }
    }

    #[test]
    fn export_import_is_stable_and_profiles_become_inherit() {
        let mut prompts = HashMap::new();
        prompts.insert("worker".into(), "Review carefully.".into());

        let first = super::export(&pipeline(), &prompts).unwrap();
        let imported = super::interpret(&first).unwrap();
        let second = super::export(&imported.pipeline, &imported.prompts).unwrap();

        assert_eq!(first, second);
        assert_eq!(
            imported
                .pipeline
                .nodes
                .iter()
                .find(|node| node.id == "worker")
                .unwrap()
                .agent_choice,
            Some(AgentChoice::Inherit)
        );
        assert_eq!(imported.prompts, prompts);
    }

    #[test]
    fn export_orders_variables_deterministically() {
        let mut pipeline = pipeline();
        for name in ["zulu", "echo", "hotel", "alpha", "yankee", "bravo"] {
            pipeline.variables.insert(
                name.into(),
                crate::pipeline::VariableDef {
                    var_type: crate::pipeline::VariableType::String,
                    default: serde_yaml::Value::String(name.into()),
                },
            );
        }

        let document = super::export(&pipeline, &HashMap::new()).unwrap();
        let positions = ["alpha:", "bravo:", "echo:", "hotel:", "yankee:", "zulu:"]
            .map(|name| document.find(name).unwrap());

        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn export_orders_nested_pipeline_maps_deterministically() {
        let mut pipeline = pipeline();
        let frontmatter = ["zulu", "echo", "hotel", "alpha", "yankee", "bravo"]
            .into_iter()
            .map(|name| {
                (
                    format!("field_{name}"),
                    crate::pipeline::FrontmatterFieldDecl {
                        field_type: "string".into(),
                        allowed: None,
                    },
                )
            })
            .collect();
        pipeline.nodes[0].outputs[0].frontmatter = Some(frontmatter);

        let document = super::export(&pipeline, &HashMap::new()).unwrap();
        let positions = [
            "field_alpha:",
            "field_bravo:",
            "field_echo:",
            "field_hotel:",
            "field_yankee:",
            "field_zulu:",
        ]
        .map(|name| document.find(name).unwrap());

        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn unknown_version_is_rejected_with_the_document_path() {
        let error = super::interpret("pdo_pipeline: 4\npipeline: {}\nprompts: {}\n").unwrap_err();
        assert!(error.contains("pdo_pipeline: 4"), "{error}");
    }

    #[test]
    fn truncated_document_is_rejected_with_its_last_line() {
        let source = "pdo_pipeline: 1\npipeline:\n  version: '1.0'\n";

        let error = super::interpret(source).unwrap_err();

        assert!(error.contains("truncated or incomplete"), "{error}");
        assert!(error.contains("line 3"), "{error}");
    }

    #[test]
    fn unsafe_prompt_filename_is_rejected() {
        let source = super::export(&pipeline(), &HashMap::new())
            .unwrap()
            .replace("prompts: {}", "prompts:\n  ../../outside: injected");

        assert!(super::interpret(&source)
            .unwrap_err()
            .contains("cannot be used as a prompt filename"));
    }

    #[test]
    fn dangling_edge_reference_is_rejected_with_the_document_path() {
        let source = super::export(&pipeline(), &HashMap::new())
            .unwrap()
            .replace(
                "  edges: []",
                "  edges:\n  - source:\n      node: start\n      port: user_prompt\n    target:\n      node: ghost\n      port: input",
            );

        let error = super::interpret(&source).unwrap_err();

        assert!(error.contains("pipeline.edges"), "{error}");
        assert!(error.contains("non-existent node 'ghost'"), "{error}");
    }

    /// The export is the last place the "keys ⊆ nodes" invariant can be
    /// restored before the document leaves the machine that can still fix it.
    #[test]
    fn export_drops_prompts_of_nodes_that_no_longer_exist() {
        let mut prompts = HashMap::new();
        prompts.insert("worker".into(), "Review carefully.".into());
        prompts.insert("FBKE6BhH".into(), "Prompt of a deleted node.".into());

        let document = super::export(&pipeline(), &prompts).unwrap();

        assert!(!document.contains("FBKE6BhH"), "{document}");
        assert!(document.contains("Review carefully."), "{document}");
        let imported = super::interpret(&document).unwrap();
        assert_eq!(imported.prompts.len(), 1);
        assert!(imported.warnings.is_empty());
    }

    /// A document produced by an older PDO still carries the orphan. It
    /// is a leftover, not a corruption: import it and say what was dropped.
    #[test]
    fn orphan_prompt_is_dropped_with_a_warning_instead_of_rejecting_the_document() {
        let source = super::export(&pipeline(), &HashMap::new())
            .unwrap()
            .replace(
                "prompts: {}",
                "prompts:\n  FBKE6BhH: Prompt of a deleted node.",
            );

        let imported = super::interpret(&source).unwrap();

        assert!(imported.prompts.is_empty());
        assert_eq!(imported.warnings.len(), 1);
        assert!(
            imported.warnings[0].contains("prompts.FBKE6BhH"),
            "{:?}",
            imported.warnings
        );
        assert_eq!(imported.pipeline.nodes.len(), 3);
    }
}
