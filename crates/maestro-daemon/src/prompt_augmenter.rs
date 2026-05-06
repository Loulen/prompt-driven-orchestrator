use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::pipeline::{NodeDef, PipelineDef};

pub struct InputResolution {
    pub port_name: String,
    pub path: PathBuf,
    pub repeated: bool,
}

pub struct OutputDeclaration {
    pub port_name: String,
    pub path: PathBuf,
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
}

pub fn resolve_input_paths(ctx: &AugmentContext<'_>) -> Vec<InputResolution> {
    let mut inputs = Vec::new();

    for edge in &ctx.pipeline.edges {
        if edge.target.node == ctx.node.id {
            let target_port = ctx.node.inputs.iter().find(|p| p.name == edge.target.port);
            let repeated = target_port.is_some_and(|p| p.repeated);

            let path = if repeated {
                ctx.artifacts_dir
                    .join(&edge.source.node)
                    .join("iter-*")
                    .join(format!("{}.md", edge.source.port))
            } else {
                ctx.artifacts_dir
                    .join(&edge.source.node)
                    .join(format!("iter-{}", ctx.iter))
                    .join(format!("{}.md", edge.source.port))
            };

            inputs.push(InputResolution {
                port_name: edge.target.port.clone(),
                path,
                repeated,
            });
        }
    }

    if inputs.is_empty() && ctx.node.inputs.iter().any(|p| p.name == "task") {
        inputs.push(InputResolution {
            port_name: "task".into(),
            path: ctx.artifacts_dir.join("_input.md"),
            repeated: false,
        });
    }

    inputs
}

pub fn resolve_output_paths(ctx: &AugmentContext<'_>) -> Vec<OutputDeclaration> {
    ctx.node
        .outputs
        .iter()
        .map(|port| OutputDeclaration {
            port_name: port.name.clone(),
            path: ctx
                .artifacts_dir
                .join(&ctx.node.id)
                .join(format!("iter-{}", ctx.iter))
                .join(format!("{}.md", port.name)),
        })
        .collect()
}

pub fn build_preamble(ctx: &AugmentContext<'_>) -> String {
    let inputs = resolve_input_paths(ctx);
    let outputs = resolve_output_paths(ctx);

    let mut preamble = String::new();

    preamble.push_str("# Maestro Runtime Preamble\n\n");
    preamble.push_str(&format!(
        "You are node `{}` in pipeline `{}`, iteration {}.\n\n",
        ctx.node.id, ctx.pipeline.name, ctx.iter
    ));

    // Inputs
    preamble.push_str("## Inputs\n\n");
    if inputs.is_empty() {
        preamble.push_str("No inputs.\n\n");
    } else {
        for input in &inputs {
            if input.repeated {
                preamble.push_str(&format!(
                    "- `{}` (accumulated): read all files matching `{}`\n",
                    input.port_name,
                    input.path.display()
                ));
            } else {
                preamble.push_str(&format!(
                    "- `{}`: read `{}`\n",
                    input.port_name,
                    input.path.display()
                ));
            }
        }
        preamble.push('\n');
    }

    // Outputs
    preamble.push_str("## Outputs\n\n");
    if outputs.is_empty() {
        preamble.push_str("No outputs declared.\n\n");
    } else {
        for output in &outputs {
            preamble.push_str(&format!(
                "- `{}`: write to `{}`\n",
                output.port_name,
                output.path.display()
            ));
        }
        preamble.push('\n');
    }

    // CLI commands
    preamble.push_str("## Completion\n\n");
    preamble.push_str(
        "When you are done, signal completion by running:\n\
         ```\n\
         maestro complete\n\
         ```\n\n\
         If you cannot complete the task, signal failure:\n\
         ```\n\
         maestro fail --reason \"<description of the problem>\"\n\
         ```\n\n",
    );

    // Variables
    if !ctx.variables.is_empty() {
        preamble.push_str("## Pipeline Variables\n\n");
        for (name, value) in ctx.variables {
            let val_str = serde_yaml::to_string(value).unwrap_or_else(|_| format!("{value:?}"));
            preamble.push_str(&format!("- `${name}` = {}\n", val_str.trim()));
        }
        preamble.push('\n');
    }

    preamble
}

pub fn build_full_prompt(ctx: &AugmentContext<'_>, role_prompt: &str) -> String {
    let preamble = build_preamble(ctx);
    format!("{preamble}---\n\n{role_prompt}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::{EdgeDef, EdgeTarget, NodeType, Port};

    fn sample_pipeline() -> PipelineDef {
        PipelineDef {
            name: "test-pipe".into(),
            version: Some("1.0".into()),
            variables: HashMap::new(),
            nodes: vec![NodeDef {
                id: "planner".into(),
                node_type: NodeType::DocOnly,
                prompt_file: Some("prompts/planner.md".into()),
                inputs: vec![Port {
                    name: "task".into(),
                    repeated: false,
                }],
                outputs: vec![Port {
                    name: "plan".into(),
                    repeated: false,
                }],
                interactive: false,
            }],
            edges: vec![],
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
            artifacts_dir: Path::new("/repo/.maestro/artifacts"),
            variables,
            daemon_url: "http://localhost:5172",
        }
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
            inputs[0].path,
            PathBuf::from("/repo/.maestro/artifacts/_input.md")
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
            PathBuf::from("/repo/.maestro/artifacts/planner/iter-1/plan.md")
        );
    }

    #[test]
    fn cli_commands_listed_in_preamble() {
        let pipeline = sample_pipeline();
        let node = &pipeline.nodes[0];
        let vars = HashMap::new();
        let ctx = sample_ctx(&pipeline, node, &vars);

        let preamble = build_preamble(&ctx);
        assert!(preamble.contains("maestro complete"));
        assert!(preamble.contains("maestro fail --reason"));
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
            node_type: NodeType::CodeMutating,
            prompt_file: Some("prompts/impl.md".into()),
            inputs: vec![Port {
                name: "plan".into(),
                repeated: false,
            }],
            outputs: vec![Port {
                name: "summary".into(),
                repeated: false,
            }],
            interactive: false,
        });
        pipeline.edges.push(EdgeDef {
            source: EdgeTarget {
                node: "planner".into(),
                port: "plan".into(),
            },
            target: EdgeTarget {
                node: "implementer".into(),
                port: "plan".into(),
            },
            when: None,
        });

        let node = &pipeline.nodes[1]; // implementer
        let vars = HashMap::new();
        let ctx = sample_ctx(&pipeline, node, &vars);

        let inputs = resolve_input_paths(&ctx);
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].port_name, "plan");
        assert_eq!(
            inputs[0].path,
            PathBuf::from("/repo/.maestro/artifacts/planner/iter-1/plan.md")
        );
    }

    #[test]
    fn full_prompt_combines_preamble_and_role() {
        let pipeline = sample_pipeline();
        let node = &pipeline.nodes[0];
        let vars = HashMap::new();
        let ctx = sample_ctx(&pipeline, node, &vars);

        let full = build_full_prompt(&ctx, "You are a planner. Plan well.");
        assert!(full.contains("# Maestro Runtime Preamble"));
        assert!(full.contains("You are a planner. Plan well."));
        assert!(full.contains("---"));
    }
}
