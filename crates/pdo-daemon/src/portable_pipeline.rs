use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use crate::agent_choice::AgentChoice;
use crate::pipeline::{self, PipelineDef};

pub(crate) const VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PortablePipelineDocument {
    pdo_pipeline: u32,
    pipeline: PipelineDef,
    #[serde(default)]
    prompts: BTreeMap<String, String>,
}

#[derive(Debug)]
pub(crate) struct InterpretedDocument {
    pub pipeline: PipelineDef,
    pub prompts: HashMap<String, String>,
}

fn make_portable(mut pipeline: PipelineDef) -> PipelineDef {
    for node in &mut pipeline.nodes {
        if matches!(node.agent_choice, Some(AgentChoice::Profile { .. })) {
            node.agent_choice = Some(AgentChoice::Inherit);
        }
    }
    pipeline
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
    serde_yaml::to_string(&PortablePipelineDocument {
        pdo_pipeline: VERSION,
        pipeline: canonical,
        prompts: prompts
            .iter()
            .map(|(id, prompt)| (id.clone(), prompt.clone()))
            .collect(),
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

    let mut document: PortablePipelineDocument =
        serde_yaml::from_value(value).map_err(|e| format!("invalid pipeline document: {e}"))?;
    document.pipeline = make_portable(document.pipeline);

    let canonical = serde_yaml::to_string(&document.pipeline)
        .map_err(|e| format!("pipeline: failed to validate: {e}"))?;
    let parsed = pipeline::parse_pipeline(&canonical)
        .map_err(|e| format!("pipeline: invalid definition: {e}"))?;

    let known_nodes = parsed
        .pipeline
        .nodes
        .iter()
        .map(|node| node.id.as_str())
        .collect::<std::collections::HashSet<_>>();
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
    if let Some(orphan) = document
        .prompts
        .keys()
        .find(|node_id| !known_nodes.contains(node_id.as_str()))
    {
        return Err(format!("prompts.{orphan}: node does not exist"));
    }

    Ok(InterpretedDocument {
        pipeline: parsed.pipeline,
        prompts: document.prompts.into_iter().collect(),
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
    fn unknown_version_is_rejected_with_the_document_path() {
        let error = super::interpret("pdo_pipeline: 4\npipeline: {}\nprompts: {}\n").unwrap_err();
        assert!(error.contains("pdo_pipeline: 4"), "{error}");
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
}
