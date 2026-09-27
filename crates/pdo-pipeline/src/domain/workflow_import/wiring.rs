//! Edges, `pipeline()` and `parallel()`, and the finished document.

use oxc_ast::ast::{CallExpression, Expression};

use crate::domain::pipeline::{
    EdgeDef, EdgeEndpoint, LoopKind, LoopRegion, NodeDef, NodeType, PipelineDef, Port, ViewPosition,
};

use super::*;

impl Importer {
    pub(super) fn handle_pipeline(
        &mut self,
        call: &CallExpression,
        cursor: &mut Cursor,
        _depth: u32,
    ) {
        // pipeline(items, stage1, stage2, …): each stage is a callback whose body
        // contains the per-item agent(s). Collection region over `items`.
        let over = call
            .arguments
            .first()
            .and_then(|a| a.as_expression())
            .and_then(|e| match unwrap_expr(e) {
                Expression::Identifier(id) => Some(id.name.as_str().to_string()),
                _ => None,
            })
            .unwrap_or_else(|| "items".to_string());

        let member_start = self.nodes.len();
        for stage in call.arguments.iter().skip(1) {
            let Some(expr) = stage.as_expression() else {
                continue;
            };
            let mut agents: Vec<&CallExpression> = Vec::new();
            collect_agents_in_expr(expr, &mut agents, 0);
            for a in agents {
                self.emit_agent(a, cursor);
            }
        }
        let members: Vec<String> = self.nodes[member_start..]
            .iter()
            .map(|n| n.id.clone())
            .collect();
        if members.is_empty() {
            self.warn("`pipeline(...)` sans `agent()` détectable dans les stages — aucune région collection créée");
            return;
        }
        self.warn(format!(
            "`pipeline(...)` -> région collection (fan-out par item) ; `over: {over}` inféré best-effort — vérifie le champ liste sur le canvas"
        ));
        self.loop_ordinal += 1;
        let seed = format!("collection-{}", self.loop_ordinal);
        let region_id = self.unique_id(&seed);
        self.loops.push(LoopRegion {
            id: region_id,
            kind: LoopKind::Collection,
            members,
            max_iter: None,
            over: Some(over),
        });
    }

    pub(super) fn handle_parallel(
        &mut self,
        call: &CallExpression,
        cursor: &mut Cursor,
        _depth: u32,
    ) {
        // parallel([thunk, …]) / parallel(arr.map(x => () => agent(…))): fan the
        // upstream out to every agent found; downstream fans in from all of them.
        let entry_sources = cursor.clone();
        let mut agents: Vec<&CallExpression> = Vec::new();
        if let Some(first) = call.arguments.first().and_then(|a| a.as_expression()) {
            collect_agents_in_expr(first, &mut agents, 0);
        }
        if agents.is_empty() {
            return;
        }
        let mut new_cursor: Cursor = Vec::new();
        let mut any_isolated = false;
        for a in agents {
            // Each sibling is entered from the same upstream sources (fan-out).
            let mut branch_cursor = entry_sources.clone();
            let id = self.emit_agent(a, &mut branch_cursor);
            if self.nodes.last().map(|n| n.is_isolated()) == Some(true) {
                any_isolated = true;
            }
            new_cursor.push(Pending {
                node: id,
                port: "out".into(),
                when: None,
                is_else: false,
            });
        }
        if any_isolated {
            self.warn("`parallel(...)` avec des nœuds isolés — envisage un nœud Merge en aval (lint info-only ADR-0006, pas d'auto-insertion)");
        }
        *cursor = new_cursor;
    }

    pub(super) fn add_edge(
        &mut self,
        src_node: &str,
        src_port: &str,
        tgt_node: &str,
        tgt_port: &str,
        when: Option<serde_yaml::Value>,
        is_else: bool,
    ) {
        let dup = self.edges.iter().any(|e| {
            e.source.node == src_node
                && e.source.port() == src_port
                && e.target.node == tgt_node
                && e.target.port == tgt_port
                && e.when == when
                && e.is_else == is_else
        });
        if dup {
            return;
        }
        self.edges.push(EdgeDef {
            source: crate::domain::pipeline::EdgeSource::single(src_node, src_port),
            target: EdgeEndpoint {
                node: tgt_node.to_string(),
                port: tgt_port.to_string(),
            },
            reason: None,
            when,
            is_else,
            repeated: false,
            mode: None,
            waypoints: None,
            target_side: None,
            source_anchor: None,
            target_anchor: None,
            show_output_labels: None,
            output_label_pos: None,
            condition_label_pos: None,
            below_nodes: false,
        });
    }

    /// Add a provenance edge only when no edge already connects the two nodes (in
    /// either port/guard form) — avoids contradicting a chain/loop edge.
    pub(super) fn add_provenance_edge(&mut self, src_node: &str, src_port: &str, tgt_node: &str) {
        if src_node == tgt_node {
            return;
        }
        let exists = self
            .edges
            .iter()
            .any(|e| e.source.node == src_node && e.target.node == tgt_node);
        if exists {
            return;
        }
        self.add_edge(src_node, src_port, tgt_node, "in", None, false);
    }

    /// Connect any dangling cursor endpoints to `end`, so the pipeline is complete.
    pub(super) fn finish(&mut self, cursor: &mut Cursor) {
        let pending = std::mem::take(cursor);
        if self.nodes.is_empty() {
            // No agents at all — a straight start -> end draft.
            self.add_edge("start", "user_prompt", "end", "result", None, false);
            return;
        }
        for p in pending {
            self.add_edge(&p.node, &p.port, "end", "result", p.when, p.is_else);
        }
    }

    pub(super) fn build_pipeline(&self, name: &str) -> PipelineDef {
        let mut nodes = Vec::with_capacity(self.nodes.len() + 2);
        nodes.push(start_node());
        nodes.extend(self.nodes.iter().cloned());
        nodes.push(end_node(self.nodes.len()));

        PipelineDef {
            name: name.to_string(),
            version: Some("1.0".into()),
            variables: HashMap::new(),
            nodes,
            edges: self.edges.clone(),
            loops: self.loops.clone(),
            notes: Vec::new(),
            prompt_required: true,
            grid_size: None,
        }
    }
}

pub(super) fn plain_port(name: &str) -> Port {
    Port {
        name: name.to_string(),
        repeated: false,
        side: None,
        port_type: crate::domain::pipeline::PortType::Markdown,
        frontmatter: None,
        when: None,
        description: None,
        instructions: None,
        required: false,
    }
}

pub(super) fn start_node() -> NodeDef {
    NodeDef {
        id: "start".into(),
        name: "Start".into(),
        node_type: NodeType::Start,
        isolated_worktree: None,
        inputs: vec![],
        outputs: vec![plain_port("user_prompt")],
        interactive: false,
        view: Some(ViewPosition { x: 320.0, y: 0.0 }),
        max_iter: None,
        over: None,
        pin_harness: None,
        harnesses: Default::default(),
        agent_choice: None,
        skills: Vec::new(),
        auto_fail: None,
        orchestrator: false,
    }
}

pub(super) fn end_node(agent_count: usize) -> NodeDef {
    NodeDef {
        id: "end".into(),
        name: "End".into(),
        node_type: NodeType::End,
        isolated_worktree: None,
        inputs: vec![plain_port("result")],
        outputs: vec![],
        interactive: false,
        view: Some(ViewPosition {
            x: 320.0,
            y: (agent_count as f64 + 2.0) * 140.0,
        }),
        max_iter: None,
        over: None,
        pin_harness: None,
        harnesses: Default::default(),
        agent_choice: None,
        skills: Vec::new(),
        auto_fail: None,
        orchestrator: false,
    }
}
