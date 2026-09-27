//! One `agent()` call becomes one node.

use oxc_ast::ast::{CallExpression, Expression};

use crate::domain::pipeline::{NodeDef, NodeType, Port, ViewPosition};

use super::*;

impl Importer {
    pub(super) fn emit_agent(&mut self, call: &CallExpression, cursor: &mut Cursor) -> String {
        let opts = call.arguments.get(1).and_then(|a| a.as_expression());
        let label = opts.and_then(|o| object_prop(o, "label"));
        let label_static = label.and_then(static_prefix_of);
        let model = opts
            .and_then(|o| object_prop(o, "model"))
            .and_then(string_literal_value);
        // Kept as the raw expression: the warning below needs `display_name`,
        // which is only computed further down.
        let effort_expr = opts.and_then(|o| object_prop(o, "effort"));
        let effort = effort_expr.and_then(string_literal_value);
        let seed = label_static
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| format!("node-{}", self.ordinal));
        let id = self.unique_id(&seed);
        self.ordinal += 1;

        let display_name = label_static
            .as_deref()
            .map(clean_name)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| id.clone());

        // A computed effort (`{effort: lvl}`) can't be resolved without executing
        // the script, so it IS lost — announce it rather than dropping it in
        // silence (ADR-0001).
        if effort_expr.is_some() && effort.is_none() {
            self.warn(format!(
                "nœud '{display_name}': `effort` calculé (non littéral) — niveau d'effort non importé, à repositionner dans l'inspecteur (#424)"
            ));
        }

        let prompt_arg = call.arguments.first().and_then(|a| a.as_expression());
        let (prompt_body, has_static, refs) = match prompt_arg {
            Some(e) => render_prompt(e),
            None => (String::new(), false, Vec::new()),
        };
        let prompt_body = if has_static {
            prompt_body
        } else {
            self.warn(format!(
                "nœud '{display_name}': prompt construit dynamiquement (helper nu / identifiant) — placeholder annoté à rédiger (N3, hors périmètre v1 : résolution one-hop du helper)"
            ));
            if prompt_body.is_empty() {
                "⟨TODO: prompt construit dynamiquement — rédige le rôle de ce nœud⟩".to_string()
            } else {
                prompt_body
            }
        };

        // Output port frontmatter from `opts.schema` (const ref or inline object).
        let frontmatter = opts.and_then(|o| object_prop(o, "schema")).and_then(|s| {
            let s = s.without_parentheses();
            match s {
                Expression::Identifier(id) => self.consts.schemas.get(id.name.as_str()).cloned(),
                Expression::ObjectExpression(obj) => Some(object_to_frontmatter(obj)),
                _ => None,
            }
        });
        let frontmatter = frontmatter.filter(|m| !m.is_empty());

        let mut out_port = Port {
            name: "out".into(),
            repeated: false,
            side: None,
            port_type: crate::domain::pipeline::PortType::Markdown,
            frontmatter: None,
            when: None,
            description: None,
            instructions: None,
            required: false,
        };
        out_port.frontmatter = frontmatter;

        // Workflow import stays claude-only (ADR-0016). Only materialise an entry
        // when at least one is present, so a plain node keeps an empty map.
        let mut harnesses = std::collections::BTreeMap::new();
        if model.is_some() || effort.is_some() {
            harnesses.insert(
                pdo_harness::registry::CLAUDE.to_string(),
                pdo_harness::resolver::HarnessEntry { model, effort },
            );
        }

        // #653 / ADR-0060: an imported agent role is an isolated `agent`, full
        // stop. The pre-#653 importer sniffed the prompt and the label for
        // mutation keywords to pick between non-isolated and isolated; a
        // guess about what a foreign prompt will touch is exactly the thing the
        // explicit isolation line replaces.
        let node = NodeDef {
            id: id.clone(),
            name: display_name,
            node_type: NodeType::Agent,
            isolated_worktree: Some(true),
            inputs: vec![plain_port("in")],
            outputs: vec![out_port],
            interactive: false,
            view: Some(ViewPosition {
                x: 320.0,
                y: (self.ordinal as f64) * 140.0,
            }),
            max_iter: None,
            over: None,
            pin_harness: None,
            harnesses,
            agent_choice: None,
            skills: Vec::new(),
            auto_fail: None,
            orchestrator: false,
        };
        self.nodes.push(node);
        self.prompts.insert(id.clone(), prompt_body);

        for p in cursor.iter() {
            self.add_edge(&p.node, &p.port, &id, "in", p.when.clone(), p.is_else);
        }

        // Best-effort provenance edges from interpolation holes (ADR-0011): a
        // top-level arg (`bugReport`) => an edge from `start`; a var bound to an
        // upstream `agent()` => an edge from that node. Deduped per (source,target)
        // pair so we never contradict a chain/loop edge.
        for r in &refs {
            let root = r.split(['.', '[']).next().unwrap_or(r).trim().to_string();
            if root.is_empty() {
                continue;
            }
            if self.consts.from_args.contains(&root) || root == "args" {
                self.add_provenance_edge("start", "user_prompt", &id);
            } else if let Some(src) = self.agent_vars.get(&root).cloned() {
                self.add_provenance_edge(&src, "out", &id);
            }
        }

        *cursor = vec![Pending {
            node: id.clone(),
            port: "out".into(),
            when: None,
            is_else: false,
        }];
        id
    }
}
