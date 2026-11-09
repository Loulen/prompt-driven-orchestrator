//! What starts a Run: the nodes Start is wired to, the roots (a work node with
//! no incoming edge at all) and, on the user's ▶, an on-demand node. A
//! pipeline with none of them is a Run that never starts anything (CORE-241).

use super::definition::PipelineDef;
use super::node::{NodeDef, NodeType};

/// Why a Run of this pipeline starts nothing: said once, here, for the
/// editor's lint and for the Run that fails on it.
pub const START_LEADS_NOWHERE: &str =
    "Start is connected to no node, and no node starts on its own, so a Run of this pipeline \
     would start nothing: draw an edge from Start to the node that should run first";

/// [`START_LEADS_NOWHERE`] when a Run of `pipeline` would start nothing, or
/// `None` when something starts it:
/// - an edge out of Start (End included: such a Run ends at once, it does not
///   hang);
/// - a root, a work node with no incoming edge at all, which the scheduler
///   starts with no Start edge (`graph_resolver::ready_nodes`);
/// - an on-demand node, which the user starts with its ▶ (ADR-0086).
///
/// A work node fed only by conditional edges, or by other work nodes, is no
/// root: it waits on a producer that nothing starts. That is the tour's
/// pipeline before CORE-241, implementer ⇄ tester with Start unwired. A
/// pipeline with no Start is the parse's refusal, not this one.
pub fn start_leads_nowhere(pipeline: &PipelineDef) -> Option<&'static str> {
    let start = pipeline
        .nodes
        .iter()
        .find(|n| n.node_type == NodeType::Start)?;
    let start_wired = pipeline.edges.iter().any(|e| e.source.node == start.id);
    let starts_on_its_own =
        |n: &NodeDef| n.on_demand || !pipeline.edges.iter().any(|e| e.target.node == n.id);
    let something_starts = start_wired || work_nodes(pipeline).any(starts_on_its_own);
    (!something_starts).then_some(START_LEADS_NOWHERE)
}

/// The nodes a Run spawns, as the scheduler counts them: not Start, End nor a
/// legacy control node.
fn work_nodes(pipeline: &PipelineDef) -> impl Iterator<Item = &NodeDef> {
    pipeline.nodes.iter().filter(|n| {
        !matches!(
            n.node_type,
            NodeType::Start | NodeType::End | NodeType::Loop | NodeType::Switch
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::pipeline::parse_pipeline;

    const NODES: &str = r#"
name: p
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
  - id: impl
    name: implementer
    type: agent
    inputs:
      - name: in
    outputs:
      - name: out
  - id: tester
    name: tester
    type: agent
    inputs:
      - name: in
    outputs:
      - name: out
"#;

    /// The tour's loop before CORE-241: implementer ⇄ tester, tester → End.
    const THE_LOOP: &str = r#"
  - source: { node: impl, port: out }
    target: { node: tester, port: in }
  - source: { node: tester, port: out }
    target: { node: impl, port: in }
    when: { verdict: { eq: fail } }
  - source: { node: tester, port: out }
    target: { node: end, port: result }
    when: { verdict: { eq: pass } }
"#;

    fn pipeline(nodes: &str, edges: &str) -> PipelineDef {
        parse_pipeline(&format!("{nodes}edges:{edges}"))
            .unwrap()
            .pipeline
    }

    #[test]
    fn a_loop_nothing_starts_says_the_run_would_start_nothing() {
        let p = pipeline(NODES, THE_LOOP);
        assert_eq!(start_leads_nowhere(&p), Some(START_LEADS_NOWHERE));
        assert!(START_LEADS_NOWHERE.starts_with("Start is connected to no node"));
    }

    #[test]
    fn start_wired_to_the_loop_starts_it() {
        let p = pipeline(
            NODES,
            &format!(
                "{THE_LOOP}  - source: {{ node: start, port: user_prompt }}\n    target: {{ node: impl, port: in }}\n"
            ),
        );
        assert_eq!(start_leads_nowhere(&p), None);
    }

    #[test]
    fn a_root_starts_the_run_without_a_start_edge() {
        // implementer has no incoming edge: the scheduler starts it as a root.
        let p = pipeline(
            NODES,
            "\n  - source: { node: impl, port: out }\n    target: { node: tester, port: in }\n",
        );
        assert_eq!(start_leads_nowhere(&p), None);
        assert_eq!(start_leads_nowhere(&pipeline(NODES, " []")), None);
    }

    #[test]
    fn an_on_demand_node_is_started_by_the_user() {
        let nodes = NODES.replace(
            "    name: implementer\n    type: agent\n",
            "    name: implementer\n    type: agent\n    on_demand: true\n",
        );
        assert_ne!(nodes, NODES, "precondition: implementer is on demand");
        assert_eq!(start_leads_nowhere(&pipeline(&nodes, THE_LOOP)), None);
    }

    #[test]
    fn start_and_end_alone_start_nothing_and_start_wired_to_end_ends_at_once() {
        let markers = NODES.split("  - id: impl").next().unwrap();
        assert_eq!(
            start_leads_nowhere(&pipeline(markers, " []")),
            Some(START_LEADS_NOWHERE)
        );
        let to_end = "\n  - source: { node: start, port: user_prompt }\n    target: { node: end, port: result }\n";
        assert_eq!(start_leads_nowhere(&pipeline(markers, to_end)), None);
    }

    #[test]
    fn the_parse_warns_about_it_so_the_canvas_banner_says_it() {
        let warned = |p: &str| {
            parse_pipeline(p)
                .unwrap()
                .diagnostics
                .iter()
                .any(|d| d.message == START_LEADS_NOWHERE)
        };
        assert!(warned(&format!("{NODES}edges:{THE_LOOP}")));
        assert!(!warned(&format!("{NODES}edges: []")));
    }
}
