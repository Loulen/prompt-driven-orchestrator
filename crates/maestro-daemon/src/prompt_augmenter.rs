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
        if edge.target.node != ctx.node.id {
            continue;
        }

        let target_port = ctx.node.inputs.iter().find(|p| p.name == edge.target.port);
        let repeated = target_port.is_some_and(|p| p.repeated);

        let source_node = &edge.source.node;
        let is_start = ctx
            .pipeline
            .nodes
            .iter()
            .any(|n| n.id == *source_node && n.node_type == crate::pipeline::NodeType::Start);

        let path = if is_start {
            ctx.artifacts_dir.join("_input.md")
        } else if repeated {
            ctx.artifacts_dir
                .join(source_node)
                .join("iter-*")
                .join(format!("{}.md", edge.source.port))
        } else {
            ctx.artifacts_dir
                .join(source_node)
                .join(format!("iter-{}", ctx.iter))
                .join(format!("{}.md", edge.source.port))
        };

        inputs.push(InputResolution {
            port_name: edge.target.port.clone(),
            path,
            repeated,
        });
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
        preamble.push('\n');
    }

    // CLI commands
    preamble.push_str("## Completion\n\n");
    if ctx.node.interactive {
        preamble.push_str(
            "This is an **interactive** node. Do NOT call `maestro complete`.\n\
             The user will attach to this terminal session, interact with you,\n\
             and click **\"Mark complete\"** in the Maestro UI when done.\n\
             Write your outputs to the paths listed above before the user marks complete.\n\n\
             If you cannot complete the task, signal failure:\n\
             ```\n\
             maestro fail --reason \"<description of the problem>\"\n\
             ```\n\n",
        );
    } else {
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
    }

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

pub fn build_manager_preamble(run_id: &str, daemon_url: &str) -> String {
    format!(
        r#"# Pipeline Manager Runtime Preamble

You manage **run `{run_id}`**.

- Daemon base URL: `{daemon_url}`
- Run state: `curl {daemon_url}/runs/{run_id}`
- Event log: `curl {daemon_url}/runs/{run_id}/events`
- Node pane: `curl {daemon_url}/runs/{run_id}/nodes/<node-id>/pane?iter=<N>`
- Node IO: `curl {daemon_url}/runs/{run_id}/nodes/<node-id>/io?iter=<N>`
- Artifact: `curl '{daemon_url}/runs/{run_id}/artifact?path=<relative-path>'`

## Available commands

All commands are issued via `POST {daemon_url}/runs/{run_id}/commands` with a JSON body.

### 1. extend_cycle

Increment the iteration ceiling for a cycle and re-evaluate outgoing conditions.

```bash
curl -X POST {daemon_url}/runs/{run_id}/commands \
  -H 'Content-Type: application/json' \
  -d '{{"kind":"extend_cycle","node_id":"<node-id>","additional_iter":<N>}}'
```

### 2. resume_run

Re-run the scheduler from the current state. Use after a manual conflict resolution or after extending a cycle on a halted run.

```bash
curl -X POST {daemon_url}/runs/{run_id}/commands \
  -H 'Content-Type: application/json' \
  -d '{{"kind":"resume_run"}}'
```

### 3. kill_node

Kill a running NodeRun's tmux session and emit `node_failed`.

```bash
curl -X POST {daemon_url}/runs/{run_id}/commands \
  -H 'Content-Type: application/json' \
  -d '{{"kind":"kill_node","node_id":"<node-id>","iter":<N>}}'
```

### 4. restart_node

Kill a NodeRun and re-spawn it fresh (same iter, new session).

```bash
curl -X POST {daemon_url}/runs/{run_id}/commands \
  -H 'Content-Type: application/json' \
  -d '{{"kind":"restart_node","node_id":"<node-id>","iter":<N>}}'
```

### 5. mark_node_done

Force-complete a NodeRun (typically an interactive node the user has finished with).

```bash
curl -X POST {daemon_url}/runs/{run_id}/commands \
  -H 'Content-Type: application/json' \
  -d '{{"kind":"mark_node_done","node_id":"<node-id>","iter":<N>}}'
```

### 6. inject_artifact

Write an artifact directly into the Blackboard.

```bash
curl -X POST {daemon_url}/runs/{run_id}/commands \
  -H 'Content-Type: application/json' \
  -d '{{"kind":"inject_artifact","path":"<node-id>/iter-<N>/<port>.md","content":"<markdown content>"}}'
```

### 7. cleanup_run

Archive the run: remove worktrees, branches, and artifacts from disk. Events are preserved.

```bash
curl -X POST {daemon_url}/runs/{run_id}/commands \
  -H 'Content-Type: application/json' \
  -d '{{"kind":"cleanup_run"}}'
```

---

"#
    )
}

pub fn build_manager_prompt(run_id: &str, daemon_url: &str, role_prompt: &str) -> String {
    let preamble = build_manager_preamble(run_id, daemon_url);
    format!("{preamble}{role_prompt}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::{EdgeDef, EdgeEndpoint, NodeType, Port};

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
                    frontmatter: None,
                    when: None,
                }],
                outputs: vec![Port {
                    name: "plan".into(),
                    repeated: false,
                    side: None,
                    frontmatter: None,
                    when: None,
                }],
                interactive: false,
                view: None,
                max_iter: None,
            }],
            edges: vec![],
            auto_merge_resolver: true,
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
            name: "implementer".into(),
            node_type: NodeType::CodeMutating,
            inputs: vec![Port {
                name: "plan".into(),
                repeated: false,
                side: None,
                frontmatter: None,
                when: None,
            }],
            outputs: vec![Port {
                name: "summary".into(),
                repeated: false,
                side: None,
                frontmatter: None,
                when: None,
            }],
            interactive: false,
            view: None,
            max_iter: None,
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
    fn interactive_node_preamble_omits_maestro_complete_instruction() {
        let mut pipeline = sample_pipeline();
        pipeline.nodes[0].interactive = true;
        let node = &pipeline.nodes[0];
        let vars = HashMap::new();
        let ctx = sample_ctx(&pipeline, node, &vars);

        let preamble = build_preamble(&ctx);
        assert!(
            !preamble.contains("signal completion by running"),
            "interactive node should not instruct to run maestro complete"
        );
        assert!(preamble.contains("Do NOT call `maestro complete`"));
        assert!(preamble.contains("Mark complete"));
        assert!(preamble.contains("maestro fail --reason"));
    }

    #[test]
    fn non_interactive_node_preamble_includes_maestro_complete() {
        let pipeline = sample_pipeline();
        let node = &pipeline.nodes[0];
        assert!(!node.interactive);
        let vars = HashMap::new();
        let ctx = sample_ctx(&pipeline, node, &vars);

        let preamble = build_preamble(&ctx);
        assert!(preamble.contains("maestro complete"));
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
                        frontmatter: None,
                        when: None,
                    }],
                    interactive: false,
                    view: None,
                    max_iter: None,
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
                        frontmatter: None,
                        when: None,
                    }],
                    interactive: false,
                    view: None,
                    max_iter: None,
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
                            frontmatter: None,
                            when: None,
                        },
                        Port {
                            name: "context".into(),
                            repeated: false,
                            side: None,
                            frontmatter: None,
                            when: None,
                        },
                    ],
                    outputs: vec![Port {
                        name: "summary".into(),
                        repeated: false,
                        side: None,
                        frontmatter: None,
                        when: None,
                    }],
                    interactive: false,
                    view: None,
                    max_iter: None,
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
                },
            ],
            auto_merge_resolver: true,
        };

        let node = &pipeline.nodes[2]; // implementer
        let vars = HashMap::new();
        let ctx = sample_ctx(&pipeline, node, &vars);

        let inputs = resolve_input_paths(&ctx);
        assert_eq!(inputs.len(), 2);

        let plan_input = inputs.iter().find(|i| i.port_name == "plan").unwrap();
        assert_eq!(
            plan_input.path,
            PathBuf::from("/repo/.maestro/artifacts/planner/iter-1/plan.md")
        );

        let ctx_input = inputs.iter().find(|i| i.port_name == "context").unwrap();
        assert_eq!(
            ctx_input.path,
            PathBuf::from("/repo/.maestro/artifacts/researcher/iter-1/context.md")
        );

        let preamble = build_preamble(&ctx);
        assert!(preamble.contains("`plan`"));
        assert!(preamble.contains("`context`"));
        assert!(preamble.contains("planner/iter-1/plan.md"));
        assert!(preamble.contains("researcher/iter-1/context.md"));
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
                    frontmatter: None,
                    when: None,
                }],
                outputs: vec![Port {
                    name: "review".into(),
                    repeated: false,
                    side: None,
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
                }],
                interactive: false,
                view: None,
                max_iter: None,
            }],
            edges: vec![],
            auto_merge_resolver: true,
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
        let preamble = build_manager_preamble("20260507-120000-abc1234", "http://localhost:5172");
        assert!(preamble.contains("20260507-120000-abc1234"));
        assert!(preamble.contains("http://localhost:5172"));
    }

    #[test]
    fn manager_preamble_contains_all_seven_commands() {
        let preamble = build_manager_preamble("run-1", "http://localhost:5172");
        for cmd in [
            "extend_cycle",
            "resume_run",
            "kill_node",
            "restart_node",
            "mark_node_done",
            "inject_artifact",
            "cleanup_run",
        ] {
            assert!(
                preamble.contains(cmd),
                "preamble should contain command: {cmd}"
            );
        }
    }

    #[test]
    fn manager_preamble_contains_curl_examples() {
        let preamble = build_manager_preamble("run-1", "http://localhost:5172");
        assert!(preamble.contains("curl -X POST"));
        assert!(preamble.contains("Content-Type: application/json"));
    }

    #[test]
    fn manager_prompt_combines_preamble_and_role() {
        let prompt = build_manager_prompt(
            "run-1",
            "http://localhost:5172",
            "You are the Pipeline Manager.",
        );
        assert!(prompt.contains("# Pipeline Manager Runtime Preamble"));
        assert!(prompt.contains("You are the Pipeline Manager."));
    }
}
