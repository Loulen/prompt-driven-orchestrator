//! The importer's bookkeeping: ids, warnings, blocks and statements.

use oxc_ast::ast::{Declaration, Statement};

use crate::domain::pipeline::{Diagnostic, Severity};

use super::*;

impl Importer {
    pub(super) fn new(consts: Consts) -> Self {
        Importer {
            consts,
            nodes: Vec::new(),
            edges: Vec::new(),
            loops: Vec::new(),
            prompts: HashMap::new(),
            warnings: Vec::new(),
            used_ids: HashSet::new(),
            ordinal: 0,
            loop_ordinal: 0,
            loop_stack: Vec::new(),
            agent_vars: HashMap::new(),
        }
    }

    pub(super) fn warn(&mut self, message: impl Into<String>) {
        self.warnings.push(Diagnostic {
            severity: Severity::Warning,
            message: message.into(),
        });
    }

    pub(super) fn unique_id(&mut self, seed: &str) -> String {
        let base = if seed.is_empty() { "node" } else { seed };
        let mut id = deterministic_id(base);
        let mut n = 2u32;
        while self.used_ids.contains(&id) {
            id = deterministic_id(&format!("{base}-{n}"));
            n += 1;
        }
        self.used_ids.insert(id.clone());
        id
    }

    pub(super) fn walk_block(&mut self, stmts: &[Statement], cursor: &mut Cursor, depth: u32) {
        if depth > MAX_DEPTH {
            return;
        }
        for stmt in stmts {
            self.walk_stmt(stmt, cursor, depth);
        }
    }

    pub(super) fn walk_stmt(&mut self, stmt: &Statement, cursor: &mut Cursor, depth: u32) {
        if depth > MAX_DEPTH || self.nodes.len() >= MAX_NODES {
            return;
        }
        match stmt {
            Statement::ExportNamedDeclaration(e) => {
                if let Some(Declaration::VariableDeclaration(decl)) = &e.declaration {
                    self.walk_var_decl(decl, cursor, depth);
                }
            }
            Statement::VariableDeclaration(decl) => self.walk_var_decl(decl, cursor, depth),
            Statement::ExpressionStatement(es) => {
                self.walk_value_expr(&es.expression, None, cursor, depth);
            }
            Statement::ForStatement(f) => {
                let bound = resolve_for_bound(f.test.as_ref(), &self.consts);
                self.walk_loop(&f.body, bound, cursor, depth);
            }
            Statement::WhileStatement(w) => {
                let bound = resolve_test_bound(Some(&w.test), &self.consts);
                self.walk_loop(&w.body, bound, cursor, depth);
            }
            Statement::DoWhileStatement(w) => {
                let bound = resolve_test_bound(Some(&w.test), &self.consts);
                self.walk_loop(&w.body, bound, cursor, depth);
            }
            Statement::IfStatement(i) => self.walk_if(i, cursor, depth),
            Statement::TryStatement(t) => {
                self.warn("`try/finally` (guaranteed cleanup) has no structural equivalent — its body is flattened and the cleanup guarantee is lost (idiom outside the v1 subset)");
                self.walk_block(&t.block.body, cursor, depth + 1);
                if let Some(f) = &t.finalizer {
                    self.walk_block(&f.body, cursor, depth + 1);
                }
            }
            Statement::BlockStatement(b) => self.walk_block(&b.body, cursor, depth + 1),
            Statement::ReturnStatement(_) => {
                // A bare top-level return is the workflow's return value — no node.
            }
            _ => {}
        }
    }
}
