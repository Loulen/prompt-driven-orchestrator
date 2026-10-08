//! The pipeline document: its model, the one parse every reader goes
//! through, and the rules it checks on itself.
//!
//! Each notion lives in its own file; this module gathers them under one
//! path, `pipeline::…`, the name every reader of a pipeline already uses.

mod definition;
mod diagnostic;
mod edge;
mod node;
mod normalize;
mod parse;
mod prompts;
mod references;

pub use definition::{
    GridSize, LoopKind, LoopRegion, Note, PipelineDef, VariableDef, VariableType,
};
pub use diagnostic::{Diagnostic, ParseError, ParseResult, Severity};
pub use edge::{EdgeAnchor, EdgeDef, EdgeEndpoint, EdgeRouteMode, EdgeSource, EdgeWaypoint};
pub use node::{
    is_image_file, FrontmatterFieldDecl, NodeDef, NodeType, Port, PortSide, PortType, ViewPosition,
    IMAGE_EXTENSIONS,
};
pub use normalize::{
    fold_flat_model_effort_into_harnesses, normalize_node_value, ISOLATED_WORKTREE_KEY, SKILLS_KEY,
    VALID_NODE_TYPES,
};
pub use parse::parse_pipeline;
pub use prompts::{canonical_prompt_path, split_live_prompts};
pub use references::{dangling_edge_references, resolve_switch_upstream_schema};

#[cfg(test)]
mod tests {
    use super::parse::KNOWN_TOP_LEVEL_KEYS;
    use super::*;
    use pretty_assertions::assert_eq;
    use std::collections::HashMap;

    const START_END_NODES: &str = r#"
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: end
    name: End
    type: end
    inputs:
      - name: result"#;

    fn with_start_end(yaml: &str) -> String {
        if yaml.contains("type: start") && yaml.contains("type: end") {
            return yaml.to_string();
        }
        let replacement = format!("nodes:{START_END_NODES}");
        if yaml.contains("nodes: []") {
            yaml.replacen("nodes: []", &replacement, 1)
        } else {
            yaml.replacen("nodes:", &replacement, 1)
        }
    }

    const VALID_MINIMAL: &str = r#"
name: test-pipeline
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: end
    name: End
    type: end
    inputs:
      - name: result
  - id: ab12cd34
    name: planner
    type: agent
    isolated_worktree: false
    inputs:
      - name: task
    outputs:
      - name: plan
"#;

    #[test]
    fn parses_valid_minimal_pipeline() {
        let result = parse_pipeline(VALID_MINIMAL).unwrap();
        assert_eq!(result.pipeline.name, "test-pipeline");
        assert_eq!(result.pipeline.version.as_deref(), Some("1.0"));
        assert_eq!(result.pipeline.nodes.len(), 3);

        let node = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab12cd34")
            .unwrap();
        assert_eq!(node.name, "planner");
        assert_eq!(node.node_type, NodeType::Agent);
        assert_eq!(node.inputs.len(), 1);
        assert_eq!(node.inputs[0].name, "task");
        assert_eq!(node.outputs.len(), 1);
        assert_eq!(node.outputs[0].name, "plan");

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn fails_on_missing_node_name() {
        let yaml = with_start_end(
            r#"
name: no-name
nodes:
  - id: ab12cd34
    type: agent
    isolated_worktree: false
    inputs:
      - name: in
    outputs:
      - name: out
"#,
        );
        let err = parse_pipeline(&yaml).unwrap_err();
        assert!(matches!(err, ParseError::InvalidYaml(_)));
    }

    #[test]
    fn ignores_deprecated_prompt_file_field() {
        let yaml = with_start_end(
            r#"
name: old-style
nodes:
  - id: ab12cd34
    name: worker
    type: agent
    isolated_worktree: false
    prompt_file: prompts/worker.md
    inputs:
      - name: in
    outputs:
      - name: out
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn errors_on_invalid_yaml() {
        let yaml = "{{not: valid: yaml:::";
        let err = parse_pipeline(yaml).unwrap_err();
        assert!(matches!(err, ParseError::InvalidYaml(_)));
    }

    #[test]
    fn errors_on_missing_pipeline_name() {
        let yaml = r#"
version: "1.0"
nodes: []
"#;
        let err = parse_pipeline(yaml).unwrap_err();
        assert!(matches!(err, ParseError::MissingField(_)));
    }

    #[test]
    fn rejects_pipeline_without_start_node() {
        let yaml = r#"
name: no-start
nodes:
  - id: end
    name: End
    type: end
    inputs:
      - name: result
  - id: ab12cd34
    name: worker
    type: agent
    isolated_worktree: false
    inputs: []
    outputs: []
"#;
        let err = parse_pipeline(yaml).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("start"), "error should mention start: {msg}");
    }

    #[test]
    fn rejects_pipeline_without_end_node() {
        let yaml = r#"
name: no-end
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: ab12cd34
    name: worker
    type: agent
    isolated_worktree: false
    inputs: []
    outputs: []
"#;
        let err = parse_pipeline(yaml).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("end"), "error should mention end: {msg}");
    }

    #[test]
    fn rejects_start_node_with_inputs() {
        let yaml = r#"
name: bad-start
nodes:
  - id: start
    name: Start
    type: start
    inputs:
      - name: oops
    outputs:
      - name: user_prompt
  - id: end
    name: End
    type: end
    inputs:
      - name: result
"#;
        let err = parse_pipeline(yaml).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("zero inputs"),
            "error should mention zero inputs: {msg}"
        );
    }

    #[test]
    fn rejects_start_node_with_wrong_output_port() {
        let yaml = r#"
name: bad-start-port
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: wrong_name
  - id: end
    name: End
    type: end
    inputs:
      - name: result
"#;
        let err = parse_pipeline(yaml).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("user_prompt"),
            "error should mention user_prompt: {msg}"
        );
    }

    #[test]
    fn rejects_end_node_with_outputs() {
        let yaml = r#"
name: bad-end
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: end
    name: End
    type: end
    inputs:
      - name: result
    outputs:
      - name: oops
"#;
        let err = parse_pipeline(yaml).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("zero outputs"),
            "error should mention zero outputs: {msg}"
        );
    }

    #[test]
    fn rejects_end_node_with_wrong_input_port() {
        let yaml = r#"
name: bad-end-port
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: end
    name: End
    type: end
    inputs:
      - name: wrong_name
"#;
        let err = parse_pipeline(yaml).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("result"), "error should mention result: {msg}");
    }

    #[test]
    fn rejects_legacy_halt_target_syntax() {
        let yaml = with_start_end(
            r#"
name: with-halt
nodes:
  - id: ab12cd34
    name: reviewer
    type: agent
    isolated_worktree: false
    outputs:
      - name: review
edges:
  - source: { node: ab12cd34, port: review }
    target: { halt: { message: "Blocked" } }
"#,
        );
        let err = parse_pipeline(&yaml).unwrap_err();
        assert!(matches!(err, ParseError::InvalidYaml(_)));
    }

    #[test]
    fn parses_edge_with_reason() {
        let yaml = with_start_end(
            r#"
name: with-reason
nodes:
  - id: ab12cd34
    name: reviewer
    type: agent
    isolated_worktree: false
    outputs:
      - name: review
edges:
  - source: { node: ab12cd34, port: review }
    target: { node: end, port: result }
    reason: "Blocked after {iter} iterations"
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let edge = &result.pipeline.edges[0];
        assert_eq!(
            edge.reason.as_deref(),
            Some("Blocked after {iter} iterations")
        );
        assert_eq!(edge.target.node, "end");
        assert_eq!(edge.target.port, "result");
    }

    #[test]
    fn parses_edge_with_when_clause_and_else_marker() {
        let yaml = with_start_end(
            r#"
name: conditional-edges
nodes:
  - id: ab000001
    name: reviewer
    type: agent
    isolated_worktree: false
    outputs:
      - name: review
  - id: ab000002
    name: implementer
    type: agent
    isolated_worktree: true
    inputs:
      - name: review
    outputs:
      - name: code
  - id: ab000003
    name: archiver
    type: agent
    isolated_worktree: false
    inputs:
      - name: review
    outputs:
      - name: note
edges:
  - source: { node: ab000001, port: review }
    target: { node: ab000002, port: review }
    when:
      verdict: { eq: FAIL }
  - source: { node: ab000001, port: review }
    target: { node: ab000003, port: review }
    else: true
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let guarded = &result.pipeline.edges[0];
        assert!(guarded.when.is_some(), "first edge should carry when:");
        assert!(!guarded.is_else, "first edge is not an else edge");

        let fallback = &result.pipeline.edges[1];
        assert!(fallback.when.is_none(), "else edge carries no when:");
        assert!(fallback.is_else, "second edge should be an else edge");

        let serialized = serde_yaml::to_string(&result.pipeline).unwrap();
        let reparsed = parse_pipeline(&serialized).unwrap();
        assert!(reparsed.pipeline.edges[0].when.is_some());
        assert!(reparsed.pipeline.edges[1].is_else);
    }

    #[test]
    fn parses_interactive_node() {
        let yaml = with_start_end(
            r#"
name: interactive-pipe
nodes:
  - id: ab000001
    name: griller
    type: agent
    isolated_worktree: false
    interactive: true
    inputs:
      - name: task
    outputs:
      - name: brief
  - id: ab000002
    name: worker
    type: agent
    isolated_worktree: true
    inputs:
      - name: brief
    outputs:
      - name: summary
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let griller = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab000001")
            .unwrap();
        assert!(griller.interactive);
        let worker = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab000002")
            .unwrap();
        assert!(!worker.interactive);
    }

    #[test]
    fn interactive_defaults_to_false() {
        let result = parse_pipeline(VALID_MINIMAL).unwrap();
        let planner = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab12cd34")
            .unwrap();
        assert!(!planner.interactive);
    }

    #[test]
    fn parses_typed_variables_explicit_form() {
        let yaml = with_start_end(
            r#"
name: typed-vars
variables:
  max_iter:
    type: int
    default: 5
  mode:
    type: string
    default: strict
  verbose:
    type: bool
    default: true
nodes: []
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let vars = &result.pipeline.variables;
        assert_eq!(vars.len(), 3);
        assert_eq!(vars["max_iter"].var_type, VariableType::Int);
        assert_eq!(vars["mode"].var_type, VariableType::String);
        assert_eq!(vars["verbose"].var_type, VariableType::Bool);
    }

    #[test]
    fn parses_variables_inferred_type_from_value() {
        let yaml = with_start_end(
            r#"
name: inferred-vars
variables:
  max_iter: 5
  threshold: 0.8
  mode: strict
  verbose: true
  tags: [a, b, c]
nodes: []
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let vars = &result.pipeline.variables;
        assert_eq!(vars["max_iter"].var_type, VariableType::Int);
        assert_eq!(vars["threshold"].var_type, VariableType::Float);
        assert_eq!(vars["mode"].var_type, VariableType::String);
        assert_eq!(vars["verbose"].var_type, VariableType::Bool);
        assert_eq!(vars["tags"].var_type, VariableType::List);
    }

    #[test]
    fn variable_defaults_extracts_values() {
        let yaml = with_start_end(
            r#"
name: defaults-test
variables:
  max_iter: 5
  threshold: 0.8
nodes: []
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let defaults = result.pipeline.variable_defaults();
        assert_eq!(
            defaults["max_iter"],
            serde_yaml::Value::Number(serde_yaml::Number::from(5))
        );
    }

    #[test]
    fn parses_pipeline_with_edges_and_variables() {
        let yaml = with_start_end(
            r#"
name: full-pipeline
version: "2.0"
variables:
  max_iter: 5
  threshold: 0.8
nodes:
  - id: ab000001
    name: planner
    type: agent
    isolated_worktree: false
    inputs:
      - name: task
    outputs:
      - name: plan
  - id: ab000002
    name: implementer
    type: agent
    isolated_worktree: true
    inputs:
      - name: plan
    outputs:
      - name: summary
edges:
  - source: { node: ab000001, port: plan }
    target: { node: ab000002, port: plan }
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        assert_eq!(result.pipeline.edges.len(), 1);
        assert_eq!(result.pipeline.variables.len(), 2);
        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn dangling_edge_references_names_the_edge_and_the_port() {
        // A source-port typo is a dangling reference; the message names both the
        // edge and the missing port so a launch refusal is actionable.
        let yaml = with_start_end(
            r#"
name: dangling
nodes:
  - id: ab000001
    name: planner
    type: agent
    isolated_worktree: false
    outputs:
      - name: plan
  - id: ab000002
    name: implementer
    type: agent
    isolated_worktree: false
edges:
  - source: { node: ab000001, port: plaan }
    target: { node: ab000002, port: spec }
"#,
        );
        let pipeline = parse_pipeline(&yaml).unwrap().pipeline;
        let errors = dangling_edge_references(&pipeline);
        assert_eq!(
            errors.len(),
            1,
            "exactly one dangling reference: {errors:?}"
        );
        assert!(
            errors[0].contains("ab000001") && errors[0].contains("'plaan'"),
            "error must name the edge endpoint and the missing port; got: {}",
            errors[0]
        );
        assert!(
            errors[0].contains("ab000002") && errors[0].contains("spec"),
            "error must identify the edge (both endpoints); got: {}",
            errors[0]
        );
    }

    #[test]
    fn dangling_edge_references_flags_nonexistent_nodes_but_not_emergent_inputs() {
        let yaml = with_start_end(
            r#"
name: dangling-node
nodes:
  - id: ab000001
    name: planner
    type: agent
    isolated_worktree: false
    outputs:
      - name: plan
  - id: ab000002
    name: implementer
    type: agent
    isolated_worktree: false
edges:
  - source: { node: ab000001, port: plan }
    target: { node: ghost, port: plan }
  - source: { node: ab000001, port: plan }
    target: { node: ab000002, port: anything }
"#,
        );
        let pipeline = parse_pipeline(&yaml).unwrap().pipeline;
        let errors = dangling_edge_references(&pipeline);
        assert_eq!(
            errors.len(),
            1,
            "only the ghost-node edge is dangling — an emergent input on a \
             regular node is valid by construction (#149); got: {errors:?}"
        );
        assert!(errors[0].contains("non-existent node 'ghost'"));
    }

    #[test]
    fn dangling_edge_references_empty_on_well_formed_pipeline() {
        let yaml = r#"
name: ok
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: ab000001
    name: planner
    type: agent
    isolated_worktree: false
    outputs:
      - name: plan
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: ab000001, port: task }
  - source: { node: ab000001, port: plan }
    target: { node: end, port: result }
"#;
        let pipeline = parse_pipeline(yaml).unwrap().pipeline;
        assert!(dangling_edge_references(&pipeline).is_empty());
    }

    #[test]
    fn dangling_references_flag_unknown_loop_member() {
        // A region member naming a missing node makes the region silently no-op at
        // runtime — same class as a dangling edge.
        let yaml = r#"
name: orphan-region
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: ab000001
    name: worker
    type: agent
    isolated_worktree: false
    outputs:
      - name: out
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: ab000001, port: task }
  - source: { node: ab000001, port: out }
    target: { node: end, port: result }
loops:
  - id: per-item
    kind: collection
    over: items
    members: [worker]
"#;
        let result = parse_pipeline(yaml).unwrap();
        let errors = dangling_edge_references(&result.pipeline);
        assert_eq!(errors.len(), 1);
        assert!(
            errors[0].contains("loop region 'per-item'")
                && errors[0].contains("non-existent node 'worker'"),
            "unexpected message: {}",
            errors[0]
        );
        assert!(result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("loop region 'per-item'")));
    }

    #[test]
    fn warns_on_edge_to_nonexistent_node() {
        let yaml = with_start_end(
            r#"
name: bad-edge
nodes:
  - id: ab000001
    name: planner
    type: agent
    isolated_worktree: false
    outputs:
      - name: plan
edges:
  - source: { node: ab000001, port: plan }
    target: { node: ghost, port: plan }
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let warnings: Vec<&str> = result
            .diagnostics
            .iter()
            .map(|d| d.message.as_str())
            .collect();
        assert!(warnings
            .iter()
            .any(|w| w.contains("non-existent node 'ghost'")));
    }

    #[test]
    fn warns_on_source_port_typo_but_not_emergent_target() {
        // Outputs stay declared, so a source-port typo is still a warning. Inputs
        // on a regular (agent / script) node are emergent (#149): the
        // input is derived from the edge and named after the target port, so any
        // target port is valid by construction — no false-positive "target port
        // not found" diagnostic (regression guard for run-minimal / edit-and-save).
        let yaml = with_start_end(
            r#"
name: bad-port
nodes:
  - id: ab000001
    name: planner
    type: agent
    isolated_worktree: false
    outputs:
      - name: plan
  - id: ab000002
    name: implementer
    type: agent
    isolated_worktree: false
edges:
  - source: { node: ab000001, port: plaan }
    target: { node: ab000002, port: anything }
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let warnings: Vec<&str> = result
            .diagnostics
            .iter()
            .map(|d| d.message.as_str())
            .collect();
        assert!(warnings
            .iter()
            .any(|w| w.contains("source port 'plaan' not found")));
        assert!(
            !warnings.iter().any(|w| w.contains("target port")),
            "emergent input on a regular node must not warn on target port; got: {warnings:?}"
        );
    }

    // ── Multi-output edges (ADR-0073 / #843) ────────────────────────────────
    //
    // An edge carries one or more output ports of the SAME source node and stays
    // ONE edge for the runtime. The YAML is additive: the single-port form is
    // unchanged and is the one re-emitted whenever a single port is carried.

    const TWO_OUTPUT_PIPELINE: &str = r#"
name: multi-port
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: ab000001
    name: design
    type: agent
    isolated_worktree: false
    outputs:
      - name: out
      - name: spec
  - id: ab000002
    name: orchestrator
    type: agent
    isolated_worktree: false
    outputs:
      - name: out
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: ab000001, port: brief }
  - source: { node: ab000001, ports: [out, spec] }
    target: { node: ab000002, port: out }
  - source: { node: ab000002, port: out }
    target: { node: end, port: result }
"#;

    #[test]
    fn parses_the_single_port_source_form() {
        let result = parse_pipeline(TWO_OUTPUT_PIPELINE).unwrap();
        let edge = &result.pipeline.edges[0];
        assert_eq!(edge.source.ports, vec!["user_prompt".to_string()]);
        assert_eq!(edge.source.port(), "user_prompt");
    }

    #[test]
    fn parses_the_multi_port_source_form() {
        let result = parse_pipeline(TWO_OUTPUT_PIPELINE).unwrap();
        let edge = &result.pipeline.edges[1];
        assert_eq!(
            edge.source.ports,
            vec!["out".to_string(), "spec".to_string()]
        );
        // The primary port is the first carried one: what every legacy
        // single-port reading of the edge means.
        assert_eq!(edge.source.port(), "out");
        assert!(edge.source.carries("spec"));
        assert!(!edge.source.carries("notes"));
    }

    #[test]
    fn a_single_port_source_re_emits_the_unchanged_form() {
        // THE backward-compatibility contract: re-serializing an existing edge
        // must produce `port:`, never `ports: [..]`, or every pipeline in the
        // wild would be rewritten on its first save.
        let source = EdgeSource::single("ab000001", "out");
        let yaml = serde_yaml::to_string(&source).unwrap();
        assert!(yaml.contains("port: out"), "got: {yaml}");
        assert!(!yaml.contains("ports:"), "got: {yaml}");
    }

    #[test]
    fn a_multi_port_source_emits_the_additive_form() {
        let source = EdgeSource {
            node: "ab000001".into(),
            ports: vec!["out".into(), "spec".into()],
        };
        let yaml = serde_yaml::to_string(&source).unwrap();
        assert!(yaml.contains("ports:"), "got: {yaml}");
        let round: EdgeSource = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(round, source);
    }

    #[test]
    fn an_edge_source_that_names_no_output_is_refused() {
        // An edge that carries nothing has no meaning: accepting it would let a
        // pipeline reach the scheduler with an edge that can never resolve.
        let err = serde_yaml::from_str::<EdgeSource>(
            "node: ab000001
",
        )
        .unwrap_err();
        assert!(err.to_string().contains("names no output"), "got: {err}");

        let err = serde_yaml::from_str::<EdgeSource>(
            "node: ab000001
ports: []
",
        )
        .unwrap_err();
        assert!(err.to_string().contains("empty"), "got: {err}");
    }

    #[test]
    fn an_edge_source_naming_both_forms_is_refused() {
        // Reading one and dropping the other would silently lose a carried port
        // on the next save.
        let err = serde_yaml::from_str::<EdgeSource>(
            "node: ab000001
port: out
ports: [out, spec]
",
        )
        .unwrap_err();
        assert!(err.to_string().contains("both"), "got: {err}");
    }

    #[test]
    fn warns_on_a_typo_in_any_carried_port_not_just_the_first() {
        // A typo in the second port of a `ports:` list is the same dangling
        // reference as one in the first; ignoring it would stall the run.
        let yaml = TWO_OUTPUT_PIPELINE.replace("ports: [out, spec]", "ports: [out, speec]");
        let result = parse_pipeline(&yaml).unwrap();
        let warnings: Vec<&str> = result
            .diagnostics
            .iter()
            .map(|d| d.message.as_str())
            .collect();
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("source port 'speec' not found")),
            "got: {warnings:?}"
        );
        assert!(
            !warnings.iter().any(|w| w.contains("'out' not found")),
            "the carried port that DOES exist must not warn; got: {warnings:?}"
        );
    }

    #[test]
    fn emergent_inputs_are_one_per_carried_port() {
        let result = parse_pipeline(TWO_OUTPUT_PIPELINE).unwrap();
        let multi = &result.pipeline.edges[1];
        assert_eq!(
            multi.emergent_inputs(),
            vec![
                ("out".to_string(), "out".to_string()),
                ("spec".to_string(), "spec".to_string()),
            ]
        );
        // A single-port edge keeps the target's declared input name — what a
        // structural handle (End's `result`) and every pre-#843 file mean.
        let to_end = &result.pipeline.edges[2];
        assert_eq!(
            to_end.emergent_inputs(),
            vec![("out".to_string(), "result".to_string())]
        );
    }

    #[test]
    fn the_source_label_names_every_carried_port() {
        let result = parse_pipeline(TWO_OUTPUT_PIPELINE).unwrap();
        assert_eq!(result.pipeline.edges[1].source.label(), "ab000001.out+spec");
        assert_eq!(result.pipeline.edges[2].source.label(), "ab000002.out");
    }

    #[test]
    fn warns_on_structural_target_port_typo() {
        // Structural nodes (here: the End node's `result` input) keep their
        // declared, required ports — a target-port typo on them is still a warning.
        let yaml = r#"
name: bad-structural-target
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: ab000001
    name: only
    type: agent
    isolated_worktree: false
    outputs:
      - name: out
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: ab000001, port: user_prompt }
  - source: { node: ab000001, port: out }
    target: { node: end, port: resullt }
"#;
        let result = parse_pipeline(yaml).unwrap();
        let warnings: Vec<&str> = result
            .diagnostics
            .iter()
            .map(|d| d.message.as_str())
            .collect();
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("target port 'resullt' not found")),
            "structural End-node target typo should warn; got: {warnings:?}"
        );
        // The canonical emergent Start->only edge must NOT warn (the #149 regression).
        assert!(
            !warnings.iter().any(|w| w.contains("user_prompt")),
            "emergent Start->only edge must not warn; got: {warnings:?}"
        );
    }

    #[test]
    fn no_warning_on_cycle_in_topology() {
        let yaml = with_start_end(
            r#"
name: cycle
nodes:
  - id: ab000001
    name: implementer
    type: agent
    isolated_worktree: false
    inputs:
      - name: review
    outputs:
      - name: code
  - id: ab000002
    name: reviewer
    type: agent
    isolated_worktree: false
    inputs:
      - name: code
    outputs:
      - name: review
edges:
  - source: { node: ab000001, port: code }
    target: { node: ab000002, port: code }
  - source: { node: ab000002, port: review }
    target: { node: ab000001, port: review }
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        assert!(
            result.diagnostics.is_empty(),
            "cycle should not produce warnings, got: {:?}",
            result
                .diagnostics
                .iter()
                .map(|d| &d.message)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn parses_bounded_loop_region_block() {
        // A loop is a named entry of the `loops:` block, no longer a node.
        let yaml = with_start_end(
            r#"
name: with-region
nodes:
  - id: ab000001
    name: implementer
    type: agent
    isolated_worktree: true
    outputs:
      - name: code
  - id: ab000002
    name: reviewer
    type: agent
    isolated_worktree: false
    outputs:
      - name: review
loops:
  - id: review_loop
    kind: bounded
    members: [ab000001, ab000002]
    max_iter: 3
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        assert_eq!(result.pipeline.loops.len(), 1);
        let region = &result.pipeline.loops[0];
        assert_eq!(region.id, "review_loop");
        assert_eq!(region.kind, LoopKind::Bounded);
        assert_eq!(region.members, vec!["ab000001", "ab000002"]);
        assert_eq!(
            region.max_iter,
            Some(serde_yaml::Value::Number(serde_yaml::Number::from(3)))
        );
    }

    #[test]
    fn parses_collection_loop_region_block_and_round_trips() {
        // A collection region carries no `max_iter` — the lap count is the
        // collection size. The `over` driver and `kind` must round-trip.
        let yaml = with_start_end(
            r#"
name: with-collection
nodes:
  - id: ab000001
    name: triage
    type: agent
    isolated_worktree: false
    outputs:
      - name: plan
        frontmatter:
          issues:
            type: list
  - id: ab000002
    name: fixer
    type: agent
    isolated_worktree: true
    outputs:
      - name: fix
loops:
  - id: per-issue
    kind: collection
    over: issues
    members: [ab000002]
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        assert_eq!(result.pipeline.loops.len(), 1);
        let region = &result.pipeline.loops[0];
        assert_eq!(region.id, "per-issue");
        assert_eq!(region.kind, LoopKind::Collection);
        assert_eq!(region.over.as_deref(), Some("issues"));
        assert_eq!(region.members, vec!["ab000002"]);
        assert_eq!(region.max_iter, None);

        let serialized = serde_yaml::to_string(&result.pipeline).unwrap();
        let reparsed = parse_pipeline(&serialized).unwrap();
        let r = &reparsed.pipeline.loops[0];
        assert_eq!(r.kind, LoopKind::Collection);
        assert_eq!(r.over.as_deref(), Some("issues"));
    }

    #[test]
    fn parses_nodes_with_view_positions() {
        let yaml = with_start_end(
            r#"
name: with-view
nodes:
  - id: ab12cd34
    name: planner
    type: agent
    isolated_worktree: false
    view: { x: 100, y: 200 }
    outputs:
      - name: plan
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let node = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab12cd34")
            .unwrap();
        let view = node.view.as_ref().unwrap();
        assert_eq!(view.x, 100.0);
        assert_eq!(view.y, 200.0);
    }

    #[test]
    fn accepts_edge_with_when_clause() {
        // ADR-0011 supersedes #45: conditional routing lives on the edge again.
        let yaml = with_start_end(
            r#"
name: conditional
nodes:
  - id: ab000001
    name: reviewer
    type: agent
    isolated_worktree: false
    outputs:
      - name: review
  - id: ab000002
    name: implementer
    type: agent
    isolated_worktree: true
    inputs:
      - name: review
    outputs:
      - name: code
edges:
  - source: { node: ab000001, port: review }
    target: { node: ab000002, port: review }
    when:
      iter: { lt: 5 }
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        assert!(result.pipeline.edges[0].when.is_some());
    }

    #[test]
    fn parses_repeated_flag_on_edge() {
        // `repeated` is an edge property, not a declared input port: it marks an
        // edge whose source artifact accumulates across iterations.
        let yaml = with_start_end(
            r#"
name: repeated-edge
nodes:
  - id: ab000001
    name: reviewer
    type: agent
    isolated_worktree: false
    outputs:
      - name: review
  - id: ab000002
    name: implementer
    type: agent
    isolated_worktree: true
    outputs:
      - name: code
edges:
  - source: { node: ab000001, port: review }
    target: { node: ab000002, port: reviews }
    repeated: true
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        assert!(result.pipeline.edges[0].repeated);
        let serialized = serde_yaml::to_string(&result.pipeline).unwrap();
        let reparsed: PipelineDef = serde_yaml::from_str(&serialized).unwrap();
        assert!(reparsed.edges[0].repeated);
    }

    #[test]
    fn parses_manual_edge_routing_and_round_trips() {
        // Manual routing persists in the pipeline file so a shared workflow carries
        // its arrows; the daemon must re-serialize it without drift.
        let yaml = with_start_end(
            r#"
name: routed-edge
nodes:
  - id: ab000001
    name: reviewer
    type: agent
    isolated_worktree: false
    outputs:
      - name: review
  - id: ab000002
    name: implementer
    type: agent
    isolated_worktree: true
    outputs:
      - name: code
edges:
  - source: { node: ab000001, port: review }
    target: { node: ab000002, port: review }
    mode: manual
    waypoints:
      - { x: 120, y: 40 }
      - { x: 120, y: 220 }
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let edge = &result.pipeline.edges[0];
        assert_eq!(edge.mode, Some(EdgeRouteMode::Manual));
        let wp = edge.waypoints.as_ref().expect("waypoints parsed");
        assert_eq!(wp.len(), 2);
        assert_eq!(wp[0].x, 120.0);
        assert_eq!(wp[0].y, 40.0);
        assert_eq!(wp[1].y, 220.0);

        let serialized = serde_yaml::to_string(&result.pipeline).unwrap();
        let reparsed: PipelineDef = serde_yaml::from_str(&serialized).unwrap();
        let redge = &reparsed.edges[0];
        assert_eq!(redge.mode, Some(EdgeRouteMode::Manual));
        assert_eq!(redge.waypoints.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn parses_edge_target_side_and_round_trips() {
        // The incoming-edge anchor side is layout, but persisted so a shared
        // workflow keeps its arrow arrival sides.
        let yaml = with_start_end(
            r#"
name: anchored-edge
nodes:
  - id: ab000001
    name: planner
    type: agent
    isolated_worktree: false
    outputs:
      - name: plan
  - id: ab000002
    name: implementer
    type: agent
    isolated_worktree: true
    outputs:
      - name: code
edges:
  - source: { node: ab000001, port: plan }
    target: { node: ab000002, port: plan }
    target_side: top
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        assert_eq!(result.pipeline.edges[0].target_side, Some(PortSide::Top));

        let serialized = serde_yaml::to_string(&result.pipeline).unwrap();
        let reparsed: PipelineDef = serde_yaml::from_str(&serialized).unwrap();
        assert_eq!(reparsed.edges[0].target_side, Some(PortSide::Top));
    }

    #[test]
    fn edge_target_side_defaults_to_none() {
        // An edge without an explicit anchor side parses with `target_side: None`
        // (legacy left anchoring is the canvas default, never written to file).
        let yaml = with_start_end(
            r#"
name: unanchored-edge
nodes:
  - id: ab000001
    name: planner
    type: agent
    isolated_worktree: false
    outputs:
      - name: plan
  - id: ab000002
    name: implementer
    type: agent
    isolated_worktree: true
    outputs:
      - name: code
edges:
  - source: { node: ab000001, port: plan }
    target: { node: ab000002, port: plan }
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        assert_eq!(result.pipeline.edges[0].target_side, None);
        // Absent ⇒ never serialized (clean file, round-trips by absence).
        let serialized = serde_yaml::to_string(&result.pipeline).unwrap();
        assert!(!serialized.contains("target_side"));
        // Same for the two #844 anchors: an edge drawn before per-edge anchoring
        // existed round-trips byte for byte.
        assert_eq!(result.pipeline.edges[0].source_anchor, None);
        assert_eq!(result.pipeline.edges[0].target_anchor, None);
        assert!(!serialized.contains("source_anchor"));
        assert!(!serialized.contains("target_anchor"));
    }

    #[test]
    fn parses_edge_anchors_and_round_trips() {
        // #844: WHERE on a card's border the wire leaves and lands. Layout, like
        // `target_side`, but persisted so a shared workflow keeps its wiring.
        let yaml = with_start_end(
            r#"
name: wired-edge
nodes:
  - id: ab000001
    name: planner
    type: agent
    isolated_worktree: false
    outputs:
      - name: plan
  - id: ab000002
    name: implementer
    type: agent
    isolated_worktree: true
    outputs:
      - name: code
edges:
  - source: { node: ab000001, port: plan }
    target: { node: ab000002, port: plan }
    target_side: top
    source_anchor: { side: bottom, offset: 80 }
    target_anchor: { side: top, offset: 36 }
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let edge = &result.pipeline.edges[0];
        let src = edge.source_anchor.expect("source anchor parsed");
        assert_eq!(src.side, PortSide::Bottom);
        assert_eq!(src.offset, 80.0);
        let tgt = edge.target_anchor.expect("target anchor parsed");
        assert_eq!(tgt.side, PortSide::Top);
        assert_eq!(tgt.offset, 36.0);

        let serialized = serde_yaml::to_string(&result.pipeline).unwrap();
        let reparsed: PipelineDef = serde_yaml::from_str(&serialized).unwrap();
        assert_eq!(reparsed.edges[0].source_anchor, Some(src));
        assert_eq!(reparsed.edges[0].target_anchor, Some(tgt));
    }

    #[test]
    fn parses_edge_label_layout_and_round_trips() {
        // #845: the label toggle, both label positions and the draw order are
        // layout — parsed, kept and re-written verbatim, so a pipeline decorated
        // on one machine renders identically on the next.
        let yaml = with_start_end(
            r#"
name: decorated-edge
nodes:
  - id: ab000001
    name: planner
    type: agent
    isolated_worktree: false
    outputs:
      - name: plan
      - name: notes
  - id: ab000002
    name: implementer
    type: agent
    isolated_worktree: true
    outputs:
      - name: code
edges:
  - source: { node: ab000001, ports: [plan, notes] }
    target: { node: ab000002, port: plan }
    show_output_labels: false
    output_label_pos:
      plan: { x: 12, y: 34 }
      notes: { x: 56, y: 78 }
    condition_label_pos: { x: 90, y: 11 }
    below_nodes: true
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let edge = &result.pipeline.edges[0];
        assert_eq!(edge.show_output_labels, Some(false));
        assert_eq!(
            edge.output_label_pos.as_ref().unwrap()["plan"],
            EdgeWaypoint { x: 12.0, y: 34.0 }
        );
        assert_eq!(
            edge.condition_label_pos,
            Some(EdgeWaypoint { x: 90.0, y: 11.0 })
        );
        assert!(edge.below_nodes);

        let serialized = serde_yaml::to_string(&result.pipeline).unwrap();
        let reparsed: PipelineDef = serde_yaml::from_str(&serialized).unwrap();
        let redge = &reparsed.edges[0];
        assert_eq!(redge.show_output_labels, Some(false));
        assert_eq!(redge.output_label_pos.as_ref().unwrap().len(), 2);
        assert_eq!(
            redge.condition_label_pos,
            Some(EdgeWaypoint { x: 90.0, y: 11.0 })
        );
        assert!(redge.below_nodes);
    }

    #[test]
    fn edge_label_layout_defaults_to_absent() {
        // An un-decorated edge carries none of the four, and writes none of them:
        // the derived defaults (labels on iff the source declares >= 2 outputs,
        // drawn above the cards, labels at their computed spots) round-trip by
        // ABSENCE, so opening and saving a pipeline nobody re-decorated is a
        // no-op on the file.
        let yaml = with_start_end(
            r#"
name: plain-edge
nodes:
  - id: ab000001
    name: planner
    type: agent
    isolated_worktree: false
    outputs:
      - name: plan
  - id: ab000002
    name: implementer
    type: agent
    isolated_worktree: true
    outputs:
      - name: code
edges:
  - source: { node: ab000001, port: plan }
    target: { node: ab000002, port: plan }
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let edge = &result.pipeline.edges[0];
        assert_eq!(edge.show_output_labels, None);
        assert_eq!(edge.output_label_pos, None);
        assert_eq!(edge.condition_label_pos, None);
        assert!(!edge.below_nodes);

        let serialized = serde_yaml::to_string(&result.pipeline).unwrap();
        for key in [
            "show_output_labels",
            "output_label_pos",
            "condition_label_pos",
            "below_nodes",
        ] {
            assert!(!serialized.contains(key), "{key} must not be written");
        }
    }

    #[test]
    fn edge_routing_defaults_to_none() {
        // An edge without explicit routing parses with no mode and no waypoints
        // (auto routing is recomputed deterministically, never persisted).
        let yaml = with_start_end(
            r#"
name: auto-edge
nodes:
  - id: ab000001
    name: planner
    type: agent
    isolated_worktree: false
    outputs:
      - name: plan
  - id: ab000002
    name: implementer
    type: agent
    isolated_worktree: true
    outputs:
      - name: code
edges:
  - source: { node: ab000001, port: plan }
    target: { node: ab000002, port: plan }
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        assert_eq!(result.pipeline.edges[0].mode, None);
        assert!(result.pipeline.edges[0].waypoints.is_none());
    }

    #[test]
    fn edge_repeated_defaults_to_false() {
        let yaml = with_start_end(
            r#"
name: plain-edge
nodes:
  - id: ab000001
    name: planner
    type: agent
    isolated_worktree: false
    outputs:
      - name: plan
  - id: ab000002
    name: implementer
    type: agent
    isolated_worktree: true
    outputs:
      - name: code
edges:
  - source: { node: ab000001, port: plan }
    target: { node: ab000002, port: plan }
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        assert!(!result.pipeline.edges[0].repeated);
    }

    #[test]
    fn parses_multiple_nodes_with_multiple_ports() {
        let yaml = with_start_end(
            r#"
name: multi-port
nodes:
  - id: ab000001
    name: planner
    type: agent
    isolated_worktree: false
    inputs:
      - name: task
    outputs:
      - name: plan
      - name: task_list
  - id: ab000002
    name: implementer
    type: agent
    isolated_worktree: true
    inputs:
      - name: plan
      - name: task_list
    outputs:
      - name: summary
  - id: ab000003
    name: reviewer
    type: agent
    isolated_worktree: false
    inputs:
      - name: summary
    outputs:
      - name: review
edges:
  - source: { node: ab000001, port: plan }
    target: { node: ab000002, port: plan }
  - source: { node: ab000001, port: task_list }
    target: { node: ab000002, port: task_list }
  - source: { node: ab000002, port: summary }
    target: { node: ab000003, port: summary }
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        assert_eq!(result.pipeline.edges.len(), 3);
        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn parses_output_port_with_frontmatter_schema() {
        let yaml = with_start_end(
            r#"
name: with-schema
nodes:
  - id: ab12cd34
    name: reviewer
    type: agent
    isolated_worktree: false
    inputs:
      - name: code
    outputs:
      - name: review
        frontmatter:
          verdict:
            type: enum
            allowed: [PASS, FAIL]
          score:
            type: int
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let node = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab12cd34")
            .unwrap();
        let port = &node.outputs[0];
        assert_eq!(port.name, "review");
        let schema = port.frontmatter.as_ref().unwrap();
        assert_eq!(schema.len(), 2);
        assert_eq!(schema["verdict"].field_type, "enum");
        assert_eq!(
            schema["verdict"].allowed,
            Some(vec!["PASS".into(), "FAIL".into()])
        );
        assert_eq!(schema["score"].field_type, "int");
        assert!(schema["score"].allowed.is_none());
    }

    #[test]
    fn port_side_defaults_left_for_inputs_right_for_outputs() {
        let result = parse_pipeline(VALID_MINIMAL).unwrap();
        let node = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab12cd34")
            .unwrap();
        assert_eq!(node.inputs[0].side, Some(PortSide::Left));
        assert_eq!(node.outputs[0].side, Some(PortSide::Right));
    }

    #[test]
    fn parses_explicit_port_side_all_four_values() {
        let yaml = with_start_end(
            r#"
name: sides-test
nodes:
  - id: ab12cd34
    name: worker
    type: agent
    isolated_worktree: false
    inputs:
      - name: left-in
        side: left
      - name: right-in
        side: right
      - name: top-in
        side: top
      - name: bottom-in
        side: bottom
    outputs:
      - name: left-out
        side: left
      - name: right-out
        side: right
      - name: top-out
        side: top
      - name: bottom-out
        side: bottom
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let node = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab12cd34")
            .unwrap();

        assert_eq!(node.inputs[0].side, Some(PortSide::Left));
        assert_eq!(node.inputs[1].side, Some(PortSide::Right));
        assert_eq!(node.inputs[2].side, Some(PortSide::Top));
        assert_eq!(node.inputs[3].side, Some(PortSide::Bottom));

        assert_eq!(node.outputs[0].side, Some(PortSide::Left));
        assert_eq!(node.outputs[1].side, Some(PortSide::Right));
        assert_eq!(node.outputs[2].side, Some(PortSide::Top));
        assert_eq!(node.outputs[3].side, Some(PortSide::Bottom));
    }

    #[test]
    fn port_side_omitted_gets_contextual_default() {
        let yaml = with_start_end(
            r#"
name: defaults-test
nodes:
  - id: ab12cd34
    name: worker
    type: agent
    isolated_worktree: false
    inputs:
      - name: a
      - name: b
        side: top
    outputs:
      - name: x
      - name: y
        side: bottom
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let node = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab12cd34")
            .unwrap();

        assert_eq!(node.inputs[0].side, Some(PortSide::Left));
        assert_eq!(node.inputs[1].side, Some(PortSide::Top));
        assert_eq!(node.outputs[0].side, Some(PortSide::Right));
        assert_eq!(node.outputs[1].side, Some(PortSide::Bottom));
    }

    #[test]
    fn output_port_without_frontmatter_has_none() {
        let result = parse_pipeline(VALID_MINIMAL).unwrap();
        let planner = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab12cd34")
            .unwrap();
        let port = &planner.outputs[0];
        assert!(port.frontmatter.is_none());
    }

    #[test]
    fn canonical_prompt_path_for_template() {
        let pp = std::path::Path::new("/pipelines/review-loop.yaml");
        let path = canonical_prompt_path(pp, "ab12cd34");
        assert_eq!(
            path.to_str().unwrap(),
            "/pipelines/review-loop.prompts/ab12cd34.md"
        );
    }

    #[test]
    fn canonical_prompt_path_for_run() {
        let pp = std::path::Path::new("/runs/run-1/pipeline.yaml");
        let path = canonical_prompt_path(pp, "ab12cd34");
        assert_eq!(
            path.to_str().unwrap(),
            "/runs/run-1/pipeline.prompts/ab12cd34.md"
        );
    }

    #[test]
    fn legacy_merge_node_parses_as_an_isolated_agent_with_its_ports() {
        // ADR-0082: `merge` is retired with no alias and no migrator. It takes
        // the generic unknown-type path (like `doc-only` / `code-mutating`,
        // #653): an isolated `agent`, the ordinary warning, ports untouched so
        // edges and prompts naming `branches` / `merged` keep working.
        let yaml = with_start_end(
            r#"
name: merge-test
nodes:
  - id: ab000001
    name: merge-point
    type: merge
    inputs:
      - name: branches
        repeated: true
    outputs:
      - name: merged
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let mg = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab000001")
            .unwrap();
        assert_eq!(mg.node_type, NodeType::Agent);
        assert!(mg.is_isolated());
        assert_eq!(mg.isolated_worktree, Some(true));
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.severity == Severity::Warning
                    && d.message
                        == "node 'ab000001': unknown node type 'merge', defaulting to 'agent'"),
            "{:?}",
            result.diagnostics
        );
        assert_eq!(mg.inputs.len(), 1);
        assert_eq!(mg.inputs[0].name, "branches");
        assert!(mg.inputs[0].repeated);
        assert_eq!(mg.outputs.len(), 1);
        assert_eq!(mg.outputs[0].name, "merged");
    }

    // --- Script node tests (#248 / ADR-0017) ---

    /// The shipped `disk-janitor` pipeline (#480, #128 Track A) must always parse
    /// cleanly: the `reap` node stays a `script` node (a silent degrade to
    /// non-isolated would spend an LLM turn and lose determinism) and no diagnostic
    /// is error-severity. `serializer_round_trip` only asserts *if* a pipeline is
    /// valid (it `continue`s past a non-200 GET); this pins that it *is*.
    #[test]
    fn shipped_disk_janitor_pipeline_is_valid() {
        let yaml = include_str!("../../../../../.pdo/pipelines/disk-janitor.yaml");
        let result = parse_pipeline(yaml).expect("disk-janitor.yaml must parse");

        let reap = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "reap")
            .expect("disk-janitor.yaml must have a `reap` node");
        assert_eq!(
            reap.node_type,
            NodeType::Script,
            "the janitor's reap node must stay a `script` node (deterministic, no LLM turn)"
        );

        assert!(
            result
                .pipeline
                .nodes
                .iter()
                .any(|n| n.node_type == NodeType::Start),
            "must have a start node"
        );
        assert!(
            result
                .pipeline
                .nodes
                .iter()
                .any(|n| n.node_type == NodeType::End),
            "must have an end node"
        );
        assert!(
            !result
                .diagnostics
                .iter()
                .any(|d| d.severity == Severity::Error),
            "shipped pipeline must have no error-severity diagnostics: {:?}",
            result.diagnostics
        );
    }

    #[test]
    fn parses_script_node() {
        let yaml = with_start_end(
            r#"
name: script-test
nodes:
  - id: ab000001
    name: notify
    type: script
    outputs:
      - name: out
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let node = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab000001")
            .unwrap();
        assert_eq!(node.node_type, NodeType::Script);
        assert_eq!(node.outputs.len(), 1);
        assert_eq!(node.outputs[0].name, "out");
    }

    #[test]
    fn script_node_is_not_rewritten_to_agent() {
        // ⚠️ silent-failure guard: `script` must be in `valid_types`, else
        // `parse_pipeline` degrades the node to non-isolated with a diagnostic.
        let yaml = with_start_end(
            r#"
name: script-not-rewritten
nodes:
  - id: ab000001
    name: notify
    type: script
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let node = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab000001")
            .unwrap();
        assert_eq!(
            node.node_type,
            NodeType::Script,
            "must not degrade to non-isolated"
        );
        assert!(
            !result
                .diagnostics
                .iter()
                .any(|d| d.message.contains("ab000001")),
            "no rewrite diagnostic for a valid script node: {:?}",
            result.diagnostics
        );
    }

    #[test]
    fn script_node_has_emergent_inputs() {
        assert!(NodeType::Script.has_emergent_inputs());
    }

    /// #669 / ADR-0062: a `script` node carrying `skills` is refused at parse with
    /// a message naming the node and the fix — a skill on a node that launches no
    /// agent would be a setting that silently does nothing.
    #[test]
    fn script_node_with_skills_is_refused_at_parse() {
        let yaml = r#"
name: script-skills
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: sh
    name: Shell
    type: script
    skills:
      - id: 11111111-1111-1111-1111-111111111111
        name: tdd
    outputs:
      - name: out
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: sh, port: in }
  - source: { node: sh, port: out }
    target: { node: end, port: result }
"#;
        let err = parse_pipeline(yaml).unwrap_err();
        let message = err.to_string();
        assert!(matches!(err, ParseError::InvalidField(_)), "{message}");
        assert!(message.contains("node 'sh'"), "{message}");
        assert!(message.contains("`script`"), "{message}");
        assert!(message.contains("`skills`"), "{message}");
        // An empty list or a null is not a selection: tolerated, not refused.
        let tolerated = yaml.replace(
            "    skills:\n      - id: 11111111-1111-1111-1111-111111111111\n        name: tdd\n",
            "    skills: []\n",
        );
        assert!(parse_pipeline(&tolerated).is_ok());
    }

    /// #669: an `agent` node's `skills` (ids + labels) round-trip through parse
    /// and re-serialization; an absent key parses to an empty list and is omitted
    /// on the way back out.
    #[test]
    fn agent_node_skills_round_trip_and_absent_is_omitted() {
        let yaml = r#"
id: a1
name: Worker
type: agent
skills:
  - id: 11111111-1111-1111-1111-111111111111
    name: tdd
  - id: 22222222-2222-2222-2222-222222222222
    name: grilling
"#;
        let node: NodeDef = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(node.skills.len(), 2);
        assert_eq!(node.skills[0].name, "tdd");
        let round = serde_yaml::to_string(&node).unwrap();
        assert!(round.contains("skills:"), "{round}");
        let back: NodeDef = serde_yaml::from_str(&round).unwrap();
        assert_eq!(back.skills, node.skills);

        let bare: NodeDef = serde_yaml::from_str("id: a2\nname: X\ntype: agent\n").unwrap();
        assert!(bare.skills.is_empty());
        let round = serde_yaml::to_string(&bare).unwrap();
        assert!(!round.contains("skills"), "{round}");
    }

    #[test]
    fn script_node_roundtrips_via_serde() {
        let node = NodeDef {
            isolated_worktree: None,
            id: "s1".into(),
            name: "notify".into(),
            node_type: NodeType::Script,
            inputs: vec![],
            outputs: vec![Port {
                name: "out".into(),
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
            skills: Vec::new(),
            auto_fail: None,
            orchestrator: false,
            on_demand: false,
        };
        let yaml = serde_yaml::to_string(&node).unwrap();
        assert!(yaml.contains("type: script"), "serializes to kebab: {yaml}");
        let back: NodeDef = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(back.node_type, NodeType::Script);
    }

    /// A node with no `agent_choice:` key parses to `None` and re-serializes
    /// without the key — the byte-identical no-migration contract (ADR-0057).
    #[test]
    fn node_without_agent_choice_key_parses_to_none_and_omits_it_on_reserialize() {
        let yaml = with_start_end(
            r#"
name: legacy-pipeline
nodes:
  - id: ab000001
    name: planner
    type: agent
    isolated_worktree: false
    inputs:
      - name: task
    outputs:
      - name: plan
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let node = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab000001")
            .unwrap();
        assert_eq!(node.agent_choice, None);
        let round = serde_yaml::to_string(node).unwrap();
        assert!(
            !round.contains("agent_choice"),
            "absent agent_choice must not appear on reserialize: {round}"
        );
    }

    /// `agent_choice: {mode: profile, ...}` parses into
    /// [`pdo_harness::agent_choice::AgentChoice::Profile`] and round-trips unchanged.
    #[test]
    fn node_agent_choice_profile_reference_round_trips() {
        let yaml = with_start_end(
            r#"
name: profile-pipeline
nodes:
  - id: ab000001
    name: planner
    type: agent
    isolated_worktree: false
    inputs:
      - name: task
    outputs:
      - name: plan
    agent_choice:
      mode: profile
      profile_id: reviewer
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let node = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab000001")
            .unwrap();
        assert_eq!(
            node.agent_choice,
            Some(pdo_harness::agent_choice::AgentChoice::Profile {
                profile_id: "reviewer".into()
            })
        );
        let round = serde_yaml::to_string(node).unwrap();
        let back: NodeDef = serde_yaml::from_str(&round).unwrap();
        assert_eq!(back.agent_choice, node.agent_choice);
    }

    /// `agent_choice: {mode: custom, ...}` parses atomically — no field is read
    /// from `harnesses`/`pin_harness` even when both are present (ADR-0057 ¶1).
    #[test]
    fn node_agent_choice_custom_round_trips_and_coexists_with_legacy_fields() {
        let yaml = with_start_end(
            r#"
name: custom-pipeline
nodes:
  - id: ab000001
    name: planner
    type: agent
    isolated_worktree: false
    inputs:
      - name: task
    outputs:
      - name: plan
    pin_harness: claude
    harnesses:
      claude:
        model: sonnet
    agent_choice:
      mode: custom
      harness: opencode
      model: gpt-5
      effort: high
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let node = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab000001")
            .unwrap();
        assert_eq!(
            node.agent_choice,
            Some(pdo_harness::agent_choice::AgentChoice::Custom {
                harness: "opencode".into(),
                model: Some("gpt-5".into()),
                effort: Some("high".into()),
            })
        );
        // Legacy fields are still parsed and preserved verbatim — Custom wins
        // at resolution time (`pdo_harness::agent_choice`), it does not erase them.
        assert_eq!(node.pin_harness.as_deref(), Some("claude"));
        assert!(node.harnesses.contains_key("claude"));
    }

    #[test]
    fn parses_switch_node_with_when_on_outputs() {
        let yaml = with_start_end(
            r#"
name: switch-test
nodes:
  - id: reviewer
    name: Reviewer
    type: agent
    isolated_worktree: false
    outputs:
      - name: review
        frontmatter:
          verdict:
            type: enum
            allowed: [PASS, APPROVED, FAIL]
  - id: ab000001
    name: verdict-switch
    type: switch
    inputs:
      - name: in
    outputs:
      - name: pass
        when:
          verdict: { in: [PASS, APPROVED] }
      - name: default
edges:
  - source: { node: reviewer, port: review }
    target: { node: ab000001, port: in }
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let sw = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab000001")
            .unwrap();
        assert_eq!(sw.node_type, NodeType::Switch);
        assert_eq!(sw.inputs.len(), 1);
        assert_eq!(sw.inputs[0].name, "in");
        assert_eq!(sw.outputs.len(), 2);
        assert_eq!(sw.outputs[0].name, "pass");
        assert!(sw.outputs[0].when.is_some());
        assert_eq!(sw.outputs[1].name, "default");
        assert!(sw.outputs[1].when.is_none());
    }

    #[test]
    fn switch_node_auto_appends_default_if_missing() {
        let yaml = with_start_end(
            r#"
name: switch-no-default
nodes:
  - id: reviewer
    name: Reviewer
    type: agent
    isolated_worktree: false
    outputs:
      - name: review
        frontmatter:
          verdict:
            type: enum
            allowed: [PASS, FAIL]
  - id: ab000001
    name: verdict-switch
    type: switch
    inputs:
      - name: in
    outputs:
      - name: pass
        when:
          verdict: { in: [PASS] }
edges:
  - source: { node: reviewer, port: review }
    target: { node: ab000001, port: in }
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let sw = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab000001")
            .unwrap();
        assert_eq!(sw.outputs.len(), 2);
        assert_eq!(sw.outputs[1].name, "default");
        assert!(sw.outputs[1].when.is_none());
        assert_eq!(sw.outputs[1].side, Some(PortSide::Right));
    }

    #[test]
    fn switch_node_explicit_default_not_duplicated() {
        let yaml = with_start_end(
            r#"
name: switch-explicit-default
nodes:
  - id: reviewer
    name: Reviewer
    type: agent
    isolated_worktree: false
    outputs:
      - name: review
        frontmatter:
          verdict:
            type: enum
            allowed: [PASS, FAIL]
  - id: ab000001
    name: verdict-switch
    type: switch
    inputs:
      - name: in
    outputs:
      - name: pass
        when:
          verdict: { in: [PASS] }
      - name: default
        side: bottom
edges:
  - source: { node: reviewer, port: review }
    target: { node: ab000001, port: in }
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let sw = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab000001")
            .unwrap();
        assert_eq!(sw.outputs.len(), 2);
        let default_port = sw.outputs.iter().find(|p| p.name == "default").unwrap();
        assert_eq!(default_port.side, Some(PortSide::Bottom));
    }

    #[test]
    fn switch_node_rejects_wrong_input_name() {
        let yaml = with_start_end(
            r#"
name: bad-switch
nodes:
  - id: ab000001
    name: bad-switch
    type: switch
    inputs:
      - name: data
    outputs:
      - name: default
"#,
        );
        let err = parse_pipeline(&yaml).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("exactly one input named 'in'"),
            "error should mention input 'in': {msg}"
        );
    }

    #[test]
    fn switch_node_rejects_multiple_inputs() {
        let yaml = with_start_end(
            r#"
name: bad-switch
nodes:
  - id: ab000001
    name: bad-switch
    type: switch
    inputs:
      - name: in
      - name: extra
    outputs:
      - name: default
"#,
        );
        let err = parse_pipeline(&yaml).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("exactly one input named 'in'"),
            "error should mention input constraint: {msg}"
        );
    }

    #[test]
    fn parses_loop_node_with_max_iter() {
        let yaml = with_start_end(
            r#"
name: loop-test
nodes:
  - id: ab000001
    name: review-loop
    type: loop
    max_iter: 5
    inputs:
      - name: in
      - name: break
    outputs:
      - name: body
      - name: done
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let lp = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab000001")
            .unwrap();
        assert_eq!(lp.node_type, NodeType::Loop);
        assert_eq!(lp.inputs.len(), 2);
        assert_eq!(lp.outputs.len(), 2);
        let max_iter = lp.max_iter.as_ref().unwrap();
        assert_eq!(max_iter.as_u64(), Some(5));
    }

    #[test]
    fn parses_loop_node_with_variable_max_iter() {
        let yaml = with_start_end(
            r#"
name: loop-var-test
variables:
  max_iter_review: 3
nodes:
  - id: ab000001
    name: review-loop
    type: loop
    max_iter: "$max_iter_review"
    inputs:
      - name: in
      - name: break
    outputs:
      - name: body
      - name: done
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let lp = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab000001")
            .unwrap();
        let max_iter = lp.max_iter.as_ref().unwrap();
        assert_eq!(max_iter.as_str(), Some("$max_iter_review"));
    }

    #[test]
    fn loop_node_rejects_missing_max_iter() {
        let yaml = with_start_end(
            r#"
name: bad-loop
nodes:
  - id: ab000001
    name: bad-loop
    type: loop
    inputs:
      - name: in
      - name: break
    outputs:
      - name: body
      - name: done
"#,
        );
        let err = parse_pipeline(&yaml).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("must declare 'max_iter'"),
            "error should mention max_iter: {msg}"
        );
    }

    #[test]
    fn loop_node_rejects_missing_break_input() {
        let yaml = with_start_end(
            r#"
name: bad-loop
nodes:
  - id: ab000001
    name: bad-loop
    type: loop
    max_iter: 3
    inputs:
      - name: in
    outputs:
      - name: body
      - name: done
"#,
        );
        let err = parse_pipeline(&yaml).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("must have input 'break'"),
            "error should mention break: {msg}"
        );
    }

    #[test]
    fn loop_node_rejects_missing_body_output() {
        let yaml = with_start_end(
            r#"
name: bad-loop
nodes:
  - id: ab000001
    name: bad-loop
    type: loop
    max_iter: 3
    inputs:
      - name: in
      - name: break
    outputs:
      - name: done
"#,
        );
        let err = parse_pipeline(&yaml).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("must have output 'body'"),
            "error should mention body: {msg}"
        );
    }

    #[test]
    fn round_trip_loop_switch_pipeline() {
        let yaml = r#"
name: review-loop
version: "1.0"
variables:
  max_iter_review: 3
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: ab000001
    name: review-loop
    type: loop
    max_iter: "$max_iter_review"
    inputs:
      - name: in
      - name: break
    outputs:
      - name: body
      - name: done
  - id: ab000002
    name: implementer
    type: agent
    isolated_worktree: true
    inputs:
      - name: task
    outputs:
      - name: code
  - id: ab000003
    name: reviewer
    type: agent
    isolated_worktree: false
    inputs:
      - name: code
    outputs:
      - name: review
        frontmatter:
          verdict:
            type: enum
            allowed: [PASS, FAIL, APPROVED]
  - id: ab000004
    name: verdict-switch
    type: switch
    inputs:
      - name: in
    outputs:
      - name: pass
        when:
          verdict: { in: [PASS, APPROVED] }
      - name: default
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: ab000001, port: in }
  - source: { node: ab000001, port: body }
    target: { node: ab000002, port: task }
  - source: { node: ab000002, port: code }
    target: { node: ab000003, port: code }
  - source: { node: ab000003, port: review }
    target: { node: ab000004, port: in }
  - source: { node: ab000004, port: pass }
    target: { node: ab000001, port: break }
  - source: { node: ab000001, port: done }
    target: { node: end, port: result }
"#;
        let result = parse_pipeline(yaml).unwrap();
        assert_eq!(result.pipeline.name, "review-loop");
        assert_eq!(result.pipeline.nodes.len(), 6);
        assert_eq!(result.pipeline.edges.len(), 6);

        let loop_node = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.node_type == NodeType::Loop)
            .unwrap();
        assert_eq!(
            loop_node.max_iter.as_ref().unwrap().as_str(),
            Some("$max_iter_review")
        );

        let switch_node = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.node_type == NodeType::Switch)
            .unwrap();
        assert_eq!(switch_node.outputs.len(), 2);

        let serialized = serde_yaml::to_string(&result.pipeline).unwrap();
        let result2 = parse_pipeline(&serialized).unwrap();
        assert_eq!(result2.pipeline.name, result.pipeline.name);
        assert_eq!(result2.pipeline.nodes.len(), result.pipeline.nodes.len());
        assert_eq!(result2.pipeline.edges.len(), result.pipeline.edges.len());
    }

    #[test]
    fn parse_materializes_a_region_for_an_undeclared_cycle() {
        // ADR-0011 (b): a cycle is never accidentally unbounded, whatever route it
        // arrived by — hand-written file, imported workflow, or a mere reopen.
        let yaml = with_start_end(
            r#"
name: cyclic
nodes:
  - id: ab000001
    name: implementer
    type: agent
    isolated_worktree: true
    outputs:
      - name: code
  - id: ab000002
    name: reviewer
    type: agent
    isolated_worktree: false
    outputs:
      - name: review
edges:
  - source: { node: ab000001, port: code }
    target: { node: ab000002, port: code }
  - source: { node: ab000002, port: review }
    target: { node: ab000001, port: task }
    else: true
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        assert_eq!(result.pipeline.loops.len(), 1);
        let region = &result.pipeline.loops[0];
        assert_eq!(region.kind, LoopKind::Bounded);
        let mut members = region.members.clone();
        members.sort();
        assert_eq!(members, vec!["ab000001", "ab000002"]);
        assert_eq!(
            region.id,
            crate::domain::loops::generated_region_id(&region.members)
        );
        // Derived, never invented: re-parsing the same bytes yields the same id, so
        // the region a live run counts laps against keeps its identity.
        assert_eq!(
            parse_pipeline(&yaml).unwrap().pipeline.loops[0].id,
            region.id
        );
        assert!(
            result.diagnostics.is_empty(),
            "a plain cycle must parse clean: {:?}",
            result.diagnostics
        );
    }

    #[test]
    fn parse_keeps_a_declared_region_verbatim() {
        // No-regression half of #396: a pipeline that already carries its `loops:`
        // block keeps that entry — its own id, its own bound — and gains no
        // duplicate region beside it.
        let yaml = with_start_end(
            r#"
name: cyclic-declared
nodes:
  - id: ab000001
    name: implementer
    type: agent
    isolated_worktree: true
    outputs:
      - name: code
  - id: ab000002
    name: reviewer
    type: agent
    isolated_worktree: false
    outputs:
      - name: review
edges:
  - source: { node: ab000001, port: code }
    target: { node: ab000002, port: code }
  - source: { node: ab000002, port: review }
    target: { node: ab000001, port: task }
    else: true
loops:
  - id: my-loop
    kind: bounded
    members: [ab000001, ab000002]
    max_iter: 2
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        assert_eq!(result.pipeline.loops.len(), 1);
        assert_eq!(result.pipeline.loops[0].id, "my-loop");
        assert_eq!(
            crate::domain::loops::resolve_region_max_iter(
                &result.pipeline.loops[0],
                &HashMap::new()
            ),
            2
        );
    }

    #[test]
    fn parse_diagnoses_legacy_control_nodes_instead_of_wrapping_them() {
        // A `type: loop` node keeps its own bound and scheduler path, so no region
        // is derived over its cycle — that would run one loop on two counters and
        // show DEFAULT_MAX_ITER where the engine uses 3.
        let result = parse_pipeline(crate::domain::migration::LEGACY_REVIEW_LOOP_YAML).unwrap();
        assert!(
            result.pipeline.loops.is_empty(),
            "no region over a legacy loop node: {:?}",
            result.pipeline.loops
        );
        let messages: Vec<&str> = result
            .diagnostics
            .iter()
            .map(|d| d.message.as_str())
            .collect();
        assert!(
            messages.iter().any(|m| m.contains("pdo migrate")
                && m.contains("loop 'qdtXejYS'")
                && m.contains("switch 'tmkRzihO'")),
            "the diagnostic must name every legacy node and the remedy: {messages:?}"
        );
    }

    #[test]
    fn parse_fixture_review_loop_yaml() {
        // The seed fixture is post-ADR-0011: conditional edges plus a named bounded
        // region, no legacy control node.
        let yaml = include_str!("../../../../../.pdo/pipelines/review-loop.yaml");
        let result = parse_pipeline(yaml).unwrap();
        assert_eq!(result.pipeline.name, "review-loop");
        assert!(!result
            .pipeline
            .nodes
            .iter()
            .any(|n| matches!(n.node_type, NodeType::Loop | NodeType::Switch)));
        assert!(result
            .pipeline
            .edges
            .iter()
            .any(|e| e.when.is_some() || e.is_else));
        assert_eq!(result.pipeline.loops.len(), 1);
        assert_eq!(result.pipeline.loops[0].kind, LoopKind::Bounded);
    }

    #[test]
    fn existing_node_types_still_parse() {
        let result = parse_pipeline(VALID_MINIMAL).unwrap();
        let types: Vec<&NodeType> = result.pipeline.nodes.iter().map(|n| &n.node_type).collect();
        assert!(types.contains(&&NodeType::Start));
        assert!(types.contains(&&NodeType::End));
        assert!(types.contains(&&NodeType::Agent));
    }

    #[test]
    fn resolve_switch_upstream_schema_returns_schema_when_connected() {
        let yaml = with_start_end(
            r#"
name: typed-switch
nodes:
  - id: reviewer
    name: Reviewer
    type: agent
    isolated_worktree: false
    inputs:
      - name: code
    outputs:
      - name: review
        frontmatter:
          verdict:
            type: enum
            allowed: [PASS, FAIL]
          score:
            type: int
  - id: gate
    name: Gate
    type: switch
    inputs:
      - name: in
    outputs:
      - name: pass
        when:
          verdict: { eq: PASS }
      - name: default
edges:
  - source: { node: reviewer, port: review }
    target: { node: gate, port: in }
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let schema = resolve_switch_upstream_schema(&result.pipeline, "gate");
        assert!(schema.is_some());
        let schema = schema.unwrap();
        assert_eq!(schema.len(), 2);
        assert_eq!(schema["verdict"].field_type, "enum");
        assert_eq!(
            schema["verdict"].allowed,
            Some(vec!["PASS".into(), "FAIL".into()])
        );
        assert_eq!(schema["score"].field_type, "int");
    }

    #[test]
    fn resolve_switch_upstream_schema_returns_none_when_no_edge() {
        let yaml = with_start_end(
            r#"
name: unconnected-switch
nodes:
  - id: gate
    name: Gate
    type: switch
    inputs:
      - name: in
    outputs:
      - name: default
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let schema = resolve_switch_upstream_schema(&result.pipeline, "gate");
        assert!(schema.is_none());
    }

    #[test]
    fn resolve_switch_upstream_schema_returns_none_when_upstream_has_no_schema() {
        let yaml = with_start_end(
            r#"
name: untyped-switch
nodes:
  - id: reviewer
    name: Reviewer
    type: agent
    isolated_worktree: false
    inputs:
      - name: code
    outputs:
      - name: review
  - id: gate
    name: Gate
    type: switch
    inputs:
      - name: in
    outputs:
      - name: default
edges:
  - source: { node: reviewer, port: review }
    target: { node: gate, port: in }
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let schema = resolve_switch_upstream_schema(&result.pipeline, "gate");
        assert!(schema.is_none());
    }

    #[test]
    fn rejects_switch_when_field_not_in_upstream_schema() {
        let yaml = with_start_end(
            r#"
name: bad-switch-when
nodes:
  - id: reviewer
    name: Reviewer
    type: agent
    isolated_worktree: false
    inputs:
      - name: code
    outputs:
      - name: review
        frontmatter:
          verdict:
            type: enum
            allowed: [PASS, FAIL]
  - id: gate
    name: Gate
    type: switch
    inputs:
      - name: in
    outputs:
      - name: pass
        when:
          nonexistent_field: { eq: PASS }
      - name: default
edges:
  - source: { node: reviewer, port: review }
    target: { node: gate, port: in }
"#,
        );
        let err = parse_pipeline(&yaml).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("nonexistent_field") && msg.contains("not found in upstream schema"),
            "error should mention undeclared field: {msg}"
        );
    }

    #[test]
    fn rejects_switch_when_field_with_no_upstream_schema() {
        let yaml = with_start_end(
            r#"
name: untyped-upstream
nodes:
  - id: reviewer
    name: Reviewer
    type: agent
    isolated_worktree: false
    inputs:
      - name: code
    outputs:
      - name: review
  - id: gate
    name: Gate
    type: switch
    inputs:
      - name: in
    outputs:
      - name: pass
        when:
          verdict: { eq: PASS }
      - name: default
edges:
  - source: { node: reviewer, port: review }
    target: { node: gate, port: in }
"#,
        );
        let err = parse_pipeline(&yaml).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("verdict") && msg.contains("not found in upstream schema"),
            "error should mention missing schema: {msg}"
        );
    }

    #[test]
    fn rejects_switch_when_field_with_no_upstream_edge() {
        let yaml = with_start_end(
            r#"
name: no-edge
nodes:
  - id: gate
    name: Gate
    type: switch
    inputs:
      - name: in
    outputs:
      - name: pass
        when:
          verdict: { eq: PASS }
      - name: default
"#,
        );
        let err = parse_pipeline(&yaml).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("verdict") && msg.contains("not found in upstream schema"),
            "error should mention missing upstream: {msg}"
        );
    }

    #[test]
    fn accepts_switch_when_field_matching_upstream_schema() {
        let yaml = with_start_end(
            r#"
name: valid-switch
nodes:
  - id: reviewer
    name: Reviewer
    type: agent
    isolated_worktree: false
    inputs:
      - name: code
    outputs:
      - name: review
        frontmatter:
          verdict:
            type: enum
            allowed: [PASS, FAIL]
          score:
            type: int
  - id: gate
    name: Gate
    type: switch
    inputs:
      - name: in
    outputs:
      - name: pass
        when:
          verdict: { eq: PASS }
          score: { gte: 7 }
      - name: default
edges:
  - source: { node: reviewer, port: review }
    target: { node: gate, port: in }
"#,
        );
        let result = parse_pipeline(&yaml);
        assert!(result.is_ok(), "should accept valid schema fields");
    }

    #[test]
    fn accepts_switch_when_with_variable_ref() {
        let yaml = with_start_end(
            r#"
name: var-switch
variables:
  threshold: 7
nodes:
  - id: gate
    name: Gate
    type: switch
    inputs:
      - name: in
    outputs:
      - name: pass
        when:
          $threshold: { gte: 5 }
      - name: default
"#,
        );
        let result = parse_pipeline(&yaml);
        assert!(result.is_ok(), "should accept $variable references");
    }

    #[test]
    fn rejects_switch_when_with_undeclared_variable_ref() {
        let yaml = with_start_end(
            r#"
name: bad-var-switch
nodes:
  - id: gate
    name: Gate
    type: switch
    inputs:
      - name: in
    outputs:
      - name: pass
        when:
          $nonexistent: { eq: 5 }
      - name: default
"#,
        );
        let err = parse_pipeline(&yaml).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("$nonexistent"),
            "error should mention undeclared variable: {msg}"
        );
    }

    #[test]
    fn accepts_switch_with_no_when_clauses() {
        let yaml = with_start_end(
            r#"
name: empty-switch
nodes:
  - id: gate
    name: Gate
    type: switch
    inputs:
      - name: in
    outputs:
      - name: default
"#,
        );
        let result = parse_pipeline(&yaml);
        assert!(result.is_ok(), "switch with only default should be valid");
    }

    #[test]
    fn resolve_switch_upstream_schema_returns_none_for_non_switch_node() {
        let yaml = with_start_end(
            r#"
name: not-a-switch
nodes:
  - id: planner
    name: Planner
    type: agent
    isolated_worktree: false
    inputs:
      - name: task
    outputs:
      - name: plan
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let schema = resolve_switch_upstream_schema(&result.pipeline, "planner");
        assert!(schema.is_none());
    }

    #[test]
    fn foreach_node_type_is_refused_at_parse() {
        // Not warn+coerce: silently rewriting a fan-out node to non-isolated would
        // run each item's work zero times. The error names the migration path.
        for t in ["for-each", "foreach"] {
            let yaml = format!(
                r#"
name: test
nodes:
  - id: fe1
    name: per-item
    type: {t}
    inputs:
      - name: in
      - name: break
    outputs:
      - name: body
      - name: done
edges: []
"#
            );
            let err = parse_pipeline(&yaml).unwrap_err();
            let msg = err.to_string();
            assert!(
                msg.contains("removed") && msg.contains("pdo migrate"),
                "refusal must name `pdo migrate`, got: {msg}"
            );
        }
    }

    fn node_value(yaml: &str) -> serde_yaml::Value {
        serde_yaml::from_str(yaml).unwrap()
    }

    fn type_str(v: &serde_yaml::Value) -> Option<String> {
        v.as_mapping()
            .and_then(|m| m.get(serde_yaml::Value::String("type".into())))
            .and_then(|t| t.as_str())
            .map(str::to_string)
    }

    #[test]
    fn normalize_node_value_defaults_missing_type() {
        let mut v = node_value("id: n1\nname: X\n");
        let diags = normalize_node_value(&mut v).unwrap();
        assert_eq!(diags.len(), 1);
        assert!(
            diags[0].message.contains("missing 'type'"),
            "{}",
            diags[0].message
        );
        assert_eq!(type_str(&v).as_deref(), Some("agent"));
    }

    #[test]
    fn normalize_node_value_coerces_unknown_type() {
        let mut v = node_value("id: n1\nname: X\ntype: bogus\n");
        let diags = normalize_node_value(&mut v).unwrap();
        assert_eq!(diags.len(), 1);
        assert!(
            diags[0].message.contains("unknown node type 'bogus'"),
            "{}",
            diags[0].message
        );
        assert_eq!(type_str(&v).as_deref(), Some("agent"));
    }

    #[test]
    fn normalize_node_value_leaves_known_types_untouched() {
        for t in VALID_NODE_TYPES {
            let mut v = node_value(&format!("id: n1\nname: X\ntype: {t}\n"));
            let diags = normalize_node_value(&mut v).unwrap();
            assert!(diags.is_empty(), "type {t} should not warn");
            assert_eq!(type_str(&v).as_deref(), Some(*t));
        }
    }

    // -----------------------------------------------------------------------
    // #653 / ADR-0060 — `agent` + explicit isolation
    // -----------------------------------------------------------------------

    fn isolation_of(v: &serde_yaml::Value) -> Option<Option<bool>> {
        let map = v.as_mapping()?;
        Some(
            map.get(serde_yaml::Value::String(ISOLATED_WORKTREE_KEY.into()))
                .map(|value| value.as_bool().expect("isolation must be a bool")),
        )
    }

    #[test]
    fn a_silent_agent_parses_isolated_and_says_so() {
        // The editor default is `true`, and the parsed document STATES it: an
        // author who never touched the control still reads where the node works.
        let mut v = node_value("id: n1\nname: X\ntype: agent\n");
        assert!(normalize_node_value(&mut v).unwrap().is_empty());
        assert_eq!(isolation_of(&v), Some(Some(true)));
    }

    #[test]
    fn a_silent_script_parses_shared_and_says_so() {
        let mut v = node_value("id: n1\nname: X\ntype: script\n");
        assert!(normalize_node_value(&mut v).unwrap().is_empty());
        assert_eq!(isolation_of(&v), Some(Some(false)));
    }

    #[test]
    fn an_explicit_isolation_is_never_overwritten() {
        for (written, expected) in [("false", false), ("true", true)] {
            for node_type in ["agent", "script"] {
                let mut v = node_value(&format!(
                    "id: n1\nname: X\ntype: {node_type}\nisolated_worktree: {written}\n"
                ));
                assert!(normalize_node_value(&mut v).unwrap().is_empty());
                assert_eq!(
                    isolation_of(&v),
                    Some(Some(expected)),
                    "{node_type} wrote {written}"
                );
            }
        }
    }

    #[test]
    fn types_without_isolation_carry_no_line() {
        // Start/End own no worktree — writing a line nobody may edit would
        // only invite someone to edit it. A stray key is dropped rather than
        // round-tripped.
        for node_type in ["start", "end", "switch", "loop"] {
            let mut v = node_value(&format!(
                "id: n1\nname: X\ntype: {node_type}\nisolated_worktree: false\n"
            ));
            normalize_node_value(&mut v).unwrap();
            assert_eq!(isolation_of(&v), Some(None), "type {node_type}");
        }
    }

    #[test]
    fn retired_types_fail_like_any_other_invalid_value() {
        // #653: no alias, no migrator, no bespoke diagnostic. The two retired
        // types take the generic unknown-type path — the message must not name
        // them any differently than `bogus`. ADR-0082: `merge` is retired the
        // same way.
        for retired in ["doc-only", "code-mutating", "merge"] {
            let mut v = node_value(&format!("id: n1\nname: X\ntype: {retired}\n"));
            let diags = normalize_node_value(&mut v).unwrap();
            assert_eq!(diags.len(), 1, "type {retired}");
            assert_eq!(
                diags[0].message,
                format!("node 'n1': unknown node type '{retired}', defaulting to 'agent'")
            );
            assert_eq!(type_str(&v).as_deref(), Some("agent"));
        }
    }

    #[test]
    fn isolation_round_trips_through_the_pipeline_serializer() {
        let yaml = "\
name: iso
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: shared
    name: Shared
    type: agent
    isolated_worktree: false
  - id: forked
    name: Forked
    type: agent
  - id: builder
    name: Builder
    type: script
  - id: sandboxed
    name: Sandboxed
    type: script
    isolated_worktree: true
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges: []
";
        let parsed = parse_pipeline(yaml).unwrap().pipeline;
        let by_id = |id: &str| parsed.nodes.iter().find(|n| n.id == id).unwrap();
        assert_eq!(by_id("shared").isolated_worktree, Some(false));
        assert_eq!(by_id("forked").isolated_worktree, Some(true));
        assert_eq!(by_id("builder").isolated_worktree, Some(false));
        assert_eq!(by_id("sandboxed").isolated_worktree, Some(true));

        // Re-serialize and re-parse: every agent/script still states its choice.
        let out = serde_yaml::to_string(&parsed).unwrap();
        assert_eq!(out.matches("isolated_worktree:").count(), 4, "{out}");
        let again = parse_pipeline(&out).unwrap().pipeline;
        for node in &again.nodes {
            let original = parsed.nodes.iter().find(|n| n.id == node.id).unwrap();
            assert_eq!(
                node.isolated_worktree, original.isolated_worktree,
                "{}",
                node.id
            );
        }
    }

    #[test]
    fn is_isolated_reads_the_line() {
        let yaml = "\
name: iso
nodes:
  - id: shared
    name: Shared
    type: agent
    isolated_worktree: false
  - id: forked
    name: Forked
    type: agent
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges: []
";
        let parsed = parse_pipeline(yaml).unwrap().pipeline;
        let by_id = |id: &str| parsed.nodes.iter().find(|n| n.id == id).unwrap();
        assert!(!by_id("shared").is_isolated());
        assert!(by_id("forked").is_isolated());
        assert!(!by_id("start").is_isolated());
    }

    #[test]
    fn normalize_node_value_refuses_foreach() {
        for t in ["for-each", "foreach"] {
            let mut v = node_value(&format!("id: n1\nname: X\ntype: {t}\n"));
            let err = normalize_node_value(&mut v).unwrap_err().to_string();
            assert!(
                err.contains("removed") && err.contains("pdo migrate"),
                "{err}"
            );
        }
    }

    #[test]
    fn normalize_node_value_ignores_non_mapping() {
        // A stray non-mapping element (e.g. a scalar in the nodes list) is a
        // no-op: no diagnostics, no mutation.
        let mut v = node_value("- a scalar");
        let before = v.clone();
        let diags = normalize_node_value(&mut v).unwrap();
        assert!(diags.is_empty());
        assert_eq!(v, before);
    }

    #[test]
    fn normalize_node_value_matches_parse_pipeline_diagnostic() {
        // The per-node pass must emit exactly the diagnostic the full pipeline
        // parse emits for the same node — one source of truth.
        let mut node = node_value("id: n1\nname: X\ntype: bogus\n");
        let node_diags = normalize_node_value(&mut node).unwrap();
        let yaml = with_start_end(
            r#"
name: t
nodes:
  - id: n1
    name: X
    type: bogus
edges: []
"#,
        );
        let msgs: Vec<String> = parse_pipeline(&yaml)
            .unwrap()
            .diagnostics
            .iter()
            .map(|d| d.message.clone())
            .collect();
        assert!(
            msgs.iter().any(|m| *m == node_diags[0].message),
            "parse_pipeline diagnostics {msgs:?} must include {:?}",
            node_diags[0].message
        );
    }

    #[test]
    fn parses_node_model_and_round_trips() {
        let yaml = with_start_end(
            r#"
name: per-node-model
nodes:
  - id: ab000001
    name: implementer
    type: agent
    isolated_worktree: true
    model: opus
    outputs:
      - name: code
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let node = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab000001")
            .unwrap();
        // A flat `model:` is folded into `harnesses.claude.model` on parse.
        assert_eq!(
            node.harnesses
                .get("claude")
                .and_then(|e| e.model.as_deref()),
            Some("opus")
        );

        let serialized = serde_yaml::to_string(&result.pipeline).unwrap();
        let result2 = parse_pipeline(&serialized).unwrap();
        let node2 = result2
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab000001")
            .unwrap();
        assert_eq!(
            node2
                .harnesses
                .get("claude")
                .and_then(|e| e.model.as_deref()),
            Some("opus")
        );
    }

    #[test]
    fn node_model_defaults_to_none() {
        // A node with no `model:` parses to `None` and is never serialized — the
        // absence gate that keeps an unset node byte-identical to a library twin
        // with no model (the "diverged forever" guard).
        let yaml = with_start_end(
            r#"
name: no-model
nodes:
  - id: ab000001
    name: implementer
    type: agent
    isolated_worktree: true
    outputs:
      - name: code
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let node = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab000001")
            .unwrap();
        assert!(node.harnesses.is_empty());
        // Absent ⇒ never serialized (clean file, round-trips by absence).
        let serialized = serde_yaml::to_string(&result.pipeline).unwrap();
        assert!(!serialized.contains("model:"));
        assert!(!serialized.contains("harnesses:"));
    }

    #[test]
    fn parses_node_effort_and_round_trips() {
        // `effort:` is pass-through, deliberately not an enum: a value the wire
        // does not know must reach the harness verbatim rather than fail the whole
        // pipeline parse.
        let yaml = with_start_end(
            r#"
name: per-node-effort
nodes:
  - id: ab000001
    name: implementer
    type: agent
    isolated_worktree: true
    model: opus
    effort: low
    outputs:
      - name: code
  - id: ab000002
    name: exotic
    type: agent
    isolated_worktree: false
    effort: turbo
    outputs:
      - name: notes
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        // A flat `effort:` is folded into `harnesses.claude.effort` on parse.
        let find = |p: &PipelineDef, id: &str| {
            p.nodes
                .iter()
                .find(|n| n.id == id)
                .and_then(|n| n.harnesses.get("claude").and_then(|e| e.effort.clone()))
        };
        assert_eq!(find(&result.pipeline, "ab000001").as_deref(), Some("low"));
        assert_eq!(
            result
                .pipeline
                .nodes
                .iter()
                .find(|n| n.id == "ab000001")
                .unwrap()
                .harnesses
                .get("claude")
                .and_then(|e| e.model.as_deref()),
            Some("opus")
        );
        // An unknown level parses fine (pass-through, no closed enum).
        assert_eq!(find(&result.pipeline, "ab000002").as_deref(), Some("turbo"));

        let serialized = serde_yaml::to_string(&result.pipeline).unwrap();
        let result2 = parse_pipeline(&serialized).unwrap();
        assert_eq!(find(&result2.pipeline, "ab000001").as_deref(), Some("low"));
        assert_eq!(
            find(&result2.pipeline, "ab000002").as_deref(),
            Some("turbo")
        );
    }

    #[test]
    fn node_effort_defaults_to_none() {
        // A node with no `effort:` parses to `None` and is never serialized — the
        // absence gate that keeps an unset node byte-identical to a library twin
        // with no effort, and the launch command identical to the legacy path.
        let yaml = with_start_end(
            r#"
name: no-effort
nodes:
  - id: ab000001
    name: implementer
    type: agent
    isolated_worktree: true
    outputs:
      - name: code
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let node = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab000001")
            .unwrap();
        assert!(node.harnesses.is_empty());
        // Absent ⇒ never serialized (clean file, round-trips by absence).
        let serialized = serde_yaml::to_string(&result.pipeline).unwrap();
        assert!(!serialized.contains("effort:"));
    }

    #[test]
    fn round_trip_frontmatter_output_port() {
        let yaml = with_start_end(
            r#"
name: frontmatter-rt
nodes:
  - id: reviewer
    name: reviewer
    type: agent
    isolated_worktree: false
    inputs:
      - name: code
    outputs:
      - name: review
        frontmatter:
          verdict:
            type: enum
            allowed: [PASS, FAIL]
          score:
            type: int
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let serialized = serde_yaml::to_string(&result.pipeline).unwrap();
        let result2 = parse_pipeline(&serialized).unwrap();
        let reviewer = result2
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "reviewer")
            .unwrap();
        let out = &reviewer.outputs[0];
        let fm = out.frontmatter.as_ref().unwrap();
        assert_eq!(fm["verdict"].field_type, "enum");
        assert_eq!(
            fm["verdict"].allowed.as_deref(),
            Some(&["PASS".into(), "FAIL".into()][..])
        );
        assert_eq!(fm["score"].field_type, "int");
    }

    #[test]
    fn round_trip_switch_with_when_clauses() {
        let yaml = with_start_end(
            r#"
name: switch-rt
nodes:
  - id: reviewer
    name: reviewer
    type: agent
    isolated_worktree: false
    inputs:
      - name: code
    outputs:
      - name: review
        frontmatter:
          verdict:
            type: enum
            allowed: [PASS, APPROVED, FAIL]
  - id: gate
    name: gate
    type: switch
    inputs:
      - name: in
    outputs:
      - name: pass
        when:
          verdict:
            in: [PASS, APPROVED]
      - name: rework
        when:
          verdict:
            eq: FAIL
edges:
  - source: { node: start, port: user_prompt }
    target: { node: reviewer, port: code }
  - source: { node: reviewer, port: review }
    target: { node: gate, port: in }
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let serialized = serde_yaml::to_string(&result.pipeline).unwrap();
        let result2 = parse_pipeline(&serialized).unwrap();
        let gate = result2
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "gate")
            .unwrap();
        let pass_port = gate.outputs.iter().find(|p| p.name == "pass").unwrap();
        assert!(pass_port.when.is_some());
        let rework_port = gate.outputs.iter().find(|p| p.name == "rework").unwrap();
        assert!(rework_port.when.is_some());
    }

    #[test]
    fn round_trip_multi_field_when_clause() {
        let yaml = with_start_end(
            r#"
name: multi-when-rt
nodes:
  - id: reviewer
    name: reviewer
    type: agent
    isolated_worktree: false
    inputs:
      - name: code
    outputs:
      - name: review
        frontmatter:
          verdict:
            type: enum
            allowed: [PASS, FAIL]
          complexity_score:
            type: int
  - id: gate
    name: gate
    type: switch
    inputs:
      - name: in
    outputs:
      - name: pass
        when:
          verdict:
            eq: PASS
          complexity_score:
            lt: 3
edges:
  - source: { node: start, port: user_prompt }
    target: { node: reviewer, port: code }
  - source: { node: reviewer, port: review }
    target: { node: gate, port: in }
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let serialized = serde_yaml::to_string(&result.pipeline).unwrap();
        let result2 = parse_pipeline(&serialized).unwrap();
        let gate = result2
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "gate")
            .unwrap();
        let pass_port = gate.outputs.iter().find(|p| p.name == "pass").unwrap();
        let when = pass_port.when.as_ref().unwrap();
        assert!(when.as_mapping().unwrap().len() >= 2);
    }

    #[test]
    fn port_type_defaults_to_markdown() {
        let yaml = r#"
name: test
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: worker
    name: Worker
    type: agent
    isolated_worktree: false
    inputs:
      - name: task
    outputs:
      - name: out
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: worker, port: task }
  - source: { node: worker, port: out }
    target: { node: end, port: result }
"#;
        let result = parse_pipeline(yaml).unwrap();
        let worker = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "worker")
            .unwrap();
        assert_eq!(worker.outputs[0].port_type, PortType::Markdown);
    }

    #[test]
    fn port_type_image_deserializes() {
        let yaml = r#"
name: test
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: designer
    name: Designer
    type: agent
    isolated_worktree: false
    inputs:
      - name: task
    outputs:
      - name: screenshot
        port_type: image
      - name: gallery
        port_type: image_list
      - name: report
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: designer, port: task }
  - source: { node: designer, port: report }
    target: { node: end, port: result }
"#;
        let result = parse_pipeline(yaml).unwrap();
        let designer = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "designer")
            .unwrap();
        assert_eq!(designer.outputs[0].name, "screenshot");
        assert_eq!(designer.outputs[0].port_type, PortType::Image);
        assert_eq!(designer.outputs[1].name, "gallery");
        assert_eq!(designer.outputs[1].port_type, PortType::ImageList);
        assert_eq!(designer.outputs[2].name, "report");
        assert_eq!(designer.outputs[2].port_type, PortType::Markdown);
    }

    #[test]
    fn port_type_html_deserializes_and_round_trips() {
        // A `port_type: html` output survives load → save: mapped to PortType::Html
        // (not the markdown default) and emitted back as the bare scalar `html`.
        let yaml = r#"
name: test
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: designer
    name: Designer
    type: agent
    isolated_worktree: false
    inputs:
      - name: task
    outputs:
      - name: report
        port_type: html
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: designer, port: task }
  - source: { node: designer, port: report }
    target: { node: end, port: result }
"#;
        let result = parse_pipeline(yaml).unwrap();
        let designer = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "designer")
            .unwrap();
        assert_eq!(designer.outputs[0].name, "report");
        assert_eq!(designer.outputs[0].port_type, PortType::Html);

        // Enum-level serde round-trip: html <-> "html".
        let rendered = serde_yaml::to_string(&PortType::Html).unwrap();
        assert_eq!(rendered.trim(), "html");
        let back: PortType = serde_yaml::from_str("html").unwrap();
        assert_eq!(back, PortType::Html);
    }

    #[test]
    fn port_type_image_list_deserializes_on_input_port() {
        // The frontend emits `port_type: image_list` on INPUT ports too; the daemon
        // read path must round-trip it to PortType::ImageList, not the markdown
        // default.
        let yaml = r#"
name: test
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: tester
    name: Tester
    type: agent
    isolated_worktree: false
    inputs:
      - name: screens
        port_type: image_list
      - name: notes
    outputs:
      - name: screens-fixed
        port_type: image_list
      - name: result
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: tester, port: notes }
  - source: { node: tester, port: result }
    target: { node: end, port: result }
"#;
        let result = parse_pipeline(yaml).unwrap();
        let tester = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "tester")
            .unwrap();
        assert_eq!(tester.inputs[0].name, "screens");
        assert_eq!(tester.inputs[0].port_type, PortType::ImageList);
        assert_eq!(tester.inputs[1].name, "notes");
        assert_eq!(tester.inputs[1].port_type, PortType::Markdown);
        assert_eq!(tester.outputs[0].name, "screens-fixed");
        assert_eq!(tester.outputs[0].port_type, PortType::ImageList);
        assert_eq!(tester.outputs[1].name, "result");
        assert_eq!(tester.outputs[1].port_type, PortType::Markdown);
    }

    #[test]
    fn prompt_required_defaults_to_true_when_absent() {
        let yaml = with_start_end(
            r#"
name: no-flag
nodes: []
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        assert!(result.pipeline.prompt_required);
    }

    #[test]
    fn prompt_required_false_round_trips() {
        let yaml = with_start_end(
            r#"
name: optional-prompt
prompt_required: false
nodes: []
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        assert!(!result.pipeline.prompt_required);

        let serialized = serde_yaml::to_string(&result.pipeline).unwrap();
        let result2 = parse_pipeline(&serialized).unwrap();
        assert!(!result2.pipeline.prompt_required);
    }

    #[test]
    fn prompt_required_true_is_not_serialized() {
        // The default round-trips by absence, keeping prompt-required pipelines
        // (the common case) clean in YAML — same convention as `loops`.
        let yaml = with_start_end(
            r#"
name: default-flag
nodes: []
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let serialized = serde_yaml::to_string(&result.pipeline).unwrap();
        assert!(!serialized.contains("prompt_required"));
    }

    #[test]
    fn prompt_required_false_emits_no_unknown_field_diagnostic() {
        // prompt_required is a legitimate field; the unknown-field lint must not
        // warn about it.
        let yaml = with_start_end(
            r#"
name: optional-prompt
prompt_required: false
nodes: []
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        assert!(
            result.diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            result.diagnostics
        );
        assert!(!result.pipeline.prompt_required);
    }

    #[test]
    fn grid_size_round_trips_and_absent_is_omitted() {
        // #877 / ADR-0076: the pipeline's own wiring-grid size persists in the
        // file, parses without an unknown-field warning, and an unset pipeline
        // serializes without the key (it follows the reader's global default).
        let yaml = with_start_end(
            r#"
name: gridded
grid_size: L
nodes: []
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(result.pipeline.grid_size, Some(GridSize::L));
        let serialized = serde_yaml::to_string(&result.pipeline).unwrap();
        assert!(serialized.contains("grid_size: L"), "{serialized}");
        assert_eq!(
            parse_pipeline(&serialized).unwrap().pipeline.grid_size,
            Some(GridSize::L)
        );

        let plain = parse_pipeline(&with_start_end("name: plain\nnodes: []\n")).unwrap();
        assert_eq!(plain.pipeline.grid_size, None);
        assert!(!serde_yaml::to_string(&plain.pipeline)
            .unwrap()
            .contains("grid_size"));
    }

    /// ADR-0086: « on demand » is a node option that round-trips, omitted when
    /// false so an untouched pipeline stays byte-identical.
    #[test]
    fn on_demand_round_trips_and_false_is_omitted() {
        let yaml = with_start_end(
            r#"
name: on-demand-pipe
nodes:
  - id: ab000001
    name: reviewer
    type: agent
    isolated_worktree: true
    on_demand: true
  - id: ab000002
    name: report
    type: script
    isolated_worktree: false
    on_demand: true
  - id: ab000003
    name: worker
    type: agent
    isolated_worktree: true
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        let flag = |id: &str| {
            result
                .pipeline
                .nodes
                .iter()
                .find(|n| n.id == id)
                .unwrap()
                .on_demand
        };
        assert!(flag("ab000001"));
        assert!(flag("ab000002"));
        assert!(!flag("ab000003"));

        let serialized = serde_yaml::to_string(&result.pipeline).unwrap();
        assert_eq!(
            serialized.matches("on_demand: true").count(),
            2,
            "{serialized}"
        );
        assert!(!serialized.contains("on_demand: false"), "{serialized}");
        let reparsed = parse_pipeline(&serialized).unwrap();
        assert!(reparsed
            .pipeline
            .nodes
            .iter()
            .any(|n| n.id == "ab000002" && n.on_demand));
    }

    /// ADR-0086: a structural node has nothing to launch — the option is
    /// refused at parse, named, rather than carried in silence.
    #[test]
    fn on_demand_is_refused_on_a_structural_node() {
        for node_type in ["start", "end", "switch", "loop"] {
            let yaml = format!(
                "name: p\nnodes:\n  - id: s\n    name: S\n    type: {node_type}\n    on_demand: true\n"
            );
            let err = parse_pipeline(&yaml).unwrap_err().to_string();
            assert!(err.contains("on_demand"), "{node_type}: {err}");
            assert!(err.contains(node_type), "{node_type}: {err}");
        }
    }

    /// ADR-0086: « Attente des nœuds lancés » is a root option, off by
    /// default and absent from the YAML when off.
    #[test]
    fn wait_for_started_nodes_round_trips_and_false_is_omitted() {
        let yaml = with_start_end("name: waiting\nwait_for_started_nodes: true\nnodes: []\n");
        let result = parse_pipeline(&yaml).unwrap();
        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert!(result.pipeline.wait_for_started_nodes);
        let serialized = serde_yaml::to_string(&result.pipeline).unwrap();
        assert!(
            serialized.contains("wait_for_started_nodes: true"),
            "{serialized}"
        );
        assert!(
            parse_pipeline(&serialized)
                .unwrap()
                .pipeline
                .wait_for_started_nodes
        );

        let plain = parse_pipeline(&with_start_end("name: plain\nnodes: []\n")).unwrap();
        assert!(!plain.pipeline.wait_for_started_nodes);
        assert!(!serde_yaml::to_string(&plain.pipeline)
            .unwrap()
            .contains("wait_for_started_nodes"));
    }

    #[test]
    fn deterministic_output_instructions_are_preserved_with_a_non_blocking_diagnostic() {
        let yaml = VALID_MINIMAL.replace(
            "      - name: user_prompt",
            "      - name: user_prompt\n        instructions: Keep this guidance.",
        );
        let result = parse_pipeline(&yaml).unwrap();

        assert_eq!(
            result.pipeline.nodes[0].outputs[0].instructions.as_deref(),
            Some("Keep this guidance.")
        );
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.severity == Severity::Warning
                && diagnostic.message.contains("deterministic node 'start'")
                && diagnostic.message.contains("output 'user_prompt'")
        }));
    }

    #[test]
    fn truly_unknown_field_still_warns() {
        let yaml = with_start_end(
            r#"
name: with-bogus
bogus_field: 1
nodes: []
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        assert_eq!(result.diagnostics.len(), 1);
        assert_eq!(
            result.diagnostics[0].message,
            "unknown field 'bogus_field' (ignored)"
        );
    }

    #[test]
    fn known_keys_cover_serialized_pipeline() {
        // Drift guard: every top-level key PipelineDef can serialize must be in
        // KNOWN_TOP_LEVEL_KEYS, or the lint false-positives on the next added field.
        // The fixture sets every optional field to a non-default value so all
        // skip_serializing_if fields are actually emitted.
        let yaml = r#"
name: full-fixture
version: "1.0"
variables:
  max_iter_review: 3
prompt_required: false
grid_size: S
wait_for_started_nodes: true
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: ab000001
    name: implementer
    type: agent
    isolated_worktree: true
    inputs:
      - name: task
    outputs:
      - name: code
  - id: ab000002
    name: reviewer
    type: agent
    isolated_worktree: false
    inputs:
      - name: code
    outputs:
      - name: review
        frontmatter:
          verdict:
            type: enum
            allowed: [PASS, FAIL]
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: ab000001, port: task }
  - source: { node: ab000001, port: code }
    target: { node: ab000002, port: code }
  # Conditional routing on the EDGE (ADR-0011) — a legacy `switch` node here
  # would trip the "run `pdo migrate`" diagnostic (#396) and make this fixture
  # lint-dirty for a reason unrelated to what it guards.
  - source: { node: ab000002, port: review }
    target: { node: end, port: result }
    when:
      verdict: { in: [PASS] }
loops:
  - id: review_loop
    kind: bounded
    members: [ab000001, ab000002]
    max_iter: "$max_iter_review"
notes:
  - id: note-1
    content: "remember to bound this loop"
    view:
      x: 120.0
      y: 240.0
"#;
        let result = parse_pipeline(yaml).unwrap();
        assert!(
            result.diagnostics.is_empty(),
            "fixture must lint clean: {:?}",
            result.diagnostics
        );

        let serialized = serde_yaml::to_string(&result.pipeline).unwrap();
        let value: serde_yaml::Value = serde_yaml::from_str(&serialized).unwrap();
        let mapping = value.as_mapping().unwrap();
        for key in mapping.keys() {
            let k = key.as_str().unwrap();
            assert!(
                KNOWN_TOP_LEVEL_KEYS.contains(&k),
                "field '{k}' serialized but missing from KNOWN_TOP_LEVEL_KEYS — add it or the lint will false-positive (cf. #183)"
            );
        }
    }
}
