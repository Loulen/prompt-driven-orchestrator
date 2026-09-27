//! Walking declarations, loops and conditionals.

use oxc_ast::ast::{Expression, Statement};

use crate::domain::pipeline::{LoopKind, LoopRegion};

use super::*;

impl Importer {
    pub(super) fn walk_var_decl(
        &mut self,
        decl: &oxc_ast::ast::VariableDeclaration,
        cursor: &mut Cursor,
        depth: u32,
    ) {
        for d in &decl.declarations {
            let Some(init) = &d.init else { continue };
            let bind = binding_name(&d.id).map(|s| s.to_string());
            self.walk_value_expr(init, bind.as_deref(), cursor, depth);
        }
    }

    /// Handle an expression in value position (a statement's expression, or a
    /// declarator initializer). `bind` is the JS variable it's assigned to, if any.
    pub(super) fn walk_value_expr(
        &mut self,
        expr: &Expression,
        bind: Option<&str>,
        cursor: &mut Cursor,
        depth: u32,
    ) {
        let inner = unwrap_expr(expr);
        if let Some(call) = as_agent_call(inner) {
            let id = self.emit_agent(call, cursor);
            if let Some(name) = bind {
                self.agent_vars.insert(name.to_string(), id);
            }
            return;
        }
        if let Some(call) = as_named_call(inner, "pipeline") {
            self.handle_pipeline(call, cursor, depth);
            return;
        }
        if let Some(call) = as_named_call(inner, "parallel") {
            self.handle_parallel(call, cursor, depth);
        }
        // Anything else (log/phase/assignment/plain data) contributes no node.
    }

    pub(super) fn walk_loop(
        &mut self,
        body: &Statement,
        bound: MaxIter,
        cursor: &mut Cursor,
        depth: u32,
    ) {
        if !stmt_has_agent(body, 0) {
            // Plumbing loop (double-decode of `args`, `JSON.parse` retry, …) — not
            // a pipeline loop. The anti-phantom-`bounded` guard (ADR-0016).
            return;
        }
        if !self.loop_stack.is_empty() {
            // Nested loop: ADR-0011 mandates flat iteration. Flatten the body inline
            // (so its agents still appear) but emit no nested region.
            self.warn("boucle imbriquée non supportée (ADR-0011 : itération plate) — le corps est aplati sans région dédiée ; à recâbler à la main");
            self.walk_stmt_loop_body(body, cursor, depth + 1);
            return;
        }

        let entry_before = self.nodes.len();
        self.loop_stack.push(LoopFrame {
            break_when: None,
            break_source: None,
        });
        self.walk_stmt_loop_body(body, cursor, depth + 1);
        let frame = self.loop_stack.pop().expect("loop frame pushed above");

        let members: Vec<String> = self.nodes[entry_before..]
            .iter()
            .map(|n| n.id.clone())
            .collect();
        if members.is_empty() {
            return;
        }
        let entry = members[0].clone();

        let max_iter = match bound {
            MaxIter::Value(v) => Some(serde_yaml::Value::Number(serde_yaml::Number::from(v))),
            MaxIter::Unresolved => {
                self.warn(format!(
                    "borne de boucle non résolue en littéral — défaut max_iter={DEFAULT_MAX_ITER} appliqué (ADR-0011)"
                ));
                Some(serde_yaml::Value::Number(serde_yaml::Number::from(
                    DEFAULT_MAX_ITER,
                )))
            }
        };

        let region_id = {
            self.loop_ordinal += 1;
            let seed = format!("loop-{}", self.loop_ordinal);
            self.unique_id(&seed)
        };
        self.loops.push(LoopRegion {
            id: region_id,
            kind: LoopKind::Bounded,
            members,
            max_iter,
            over: None,
        });

        // Exit / back-edge wiring (mirrors `pipeline_migrator::dissolve_loops`).
        let exit_source = frame.break_source.unwrap_or_else(|| {
            self.nodes
                .last()
                .map(|n| n.id.clone())
                .unwrap_or_else(|| entry.clone())
        });
        if let Some(when) = frame.break_when {
            // Continuation back-edge: loop again while the break guard is false.
            self.add_edge(&exit_source, "out", &entry, "in", None, true);
            *cursor = vec![Pending {
                node: exit_source,
                port: "out".into(),
                when: Some(when),
                is_else: false,
            }];
        } else {
            // Unconditional loop body: leave from the last member, no guard.
            *cursor = vec![Pending {
                node: exit_source,
                port: "out".into(),
                when: None,
                is_else: false,
            }];
        }
    }

    /// Walk a loop body statement, routing a top-level `if (...) break` to the
    /// current loop frame rather than emitting an exit-to-end edge.
    pub(super) fn walk_stmt_loop_body(
        &mut self,
        body: &Statement,
        cursor: &mut Cursor,
        depth: u32,
    ) {
        match body {
            Statement::BlockStatement(b) => {
                for s in &b.body {
                    self.walk_loop_inner(s, cursor, depth);
                }
            }
            other => self.walk_loop_inner(other, cursor, depth),
        }
    }

    pub(super) fn walk_loop_inner(&mut self, stmt: &Statement, cursor: &mut Cursor, depth: u32) {
        if let Statement::IfStatement(i) = stmt {
            let exit = guarded_exit(&i.consequent);
            if exit == GuardExit::Break && !if_has_agent(i) {
                if let Some(frame) = self.loop_stack.last_mut() {
                    if frame.break_when.is_none() {
                        if let Some(pred) = extract_predicate(&i.test) {
                            frame.break_when = Some(pred.when_value());
                            let src = pred
                                .object
                                .as_ref()
                                .and_then(|o| self.agent_vars.get(o).cloned());
                            frame.break_source = src;
                        }
                    }
                }
                return;
            }
        }
        self.walk_stmt(stmt, cursor, depth);
    }

    pub(super) fn walk_if(
        &mut self,
        i: &oxc_ast::ast::IfStatement,
        cursor: &mut Cursor,
        depth: u32,
    ) {
        let exit = guarded_exit(&i.consequent);
        if exit == GuardExit::Return && !if_has_agent(i) {
            match extract_predicate(&i.test) {
                Some(pred) => {
                    let when = pred.when_value();
                    // Exit-to-end edge from each current source, guarded by the
                    // predicate; the fall-through continuation becomes `else`.
                    let sources = cursor.clone();
                    for p in &sources {
                        self.add_edge(&p.node, &p.port, "end", "result", Some(when.clone()), false);
                    }
                    for p in cursor.iter_mut() {
                        p.when = None;
                        p.is_else = true;
                    }
                }
                None => {
                    self.warn("garde `if (...) return` dont le prédicat n'est pas mappable sur un champ de frontmatter / iter / variable — ignorée (routing conditionnel non matérialisé)");
                }
            }
            return;
        }
        // `if` guarding a spawn, or with no recognizable exit: flatten both
        // branches inline (best-effort) so their agents still appear.
        if if_has_agent(i) {
            self.walk_stmt(&i.consequent, cursor, depth + 1);
            if let Some(alt) = &i.alternate {
                self.walk_stmt(alt, cursor, depth + 1);
            }
        }
    }
}
