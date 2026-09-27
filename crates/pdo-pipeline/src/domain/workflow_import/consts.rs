//! The constants of a workflow: numbers, schemas, what reads `args`.

use oxc_ast::ast::{Declaration, Expression, Statement};

use super::*;

pub(super) fn collect_consts(stmts: &[Statement], consts: &mut Consts, depth: u32) {
    if depth > MAX_DEPTH {
        return;
    }
    for stmt in stmts {
        match stmt {
            Statement::VariableDeclaration(decl) => {
                for d in &decl.declarations {
                    let Some(name) = binding_name(&d.id) else {
                        continue;
                    };
                    let Some(init) = &d.init else { continue };
                    let init = init.without_parentheses();
                    if let Some(n) = resolve_number(init, consts) {
                        consts.numbers.insert(name.to_string(), n);
                    }
                    if let Expression::ObjectExpression(obj) = init {
                        consts
                            .schemas
                            .insert(name.to_string(), object_to_frontmatter(obj));
                    }
                    if expr_mentions_ident(init, "args", 0) {
                        consts.from_args.insert(name.to_string());
                    }
                }
            }
            Statement::ExportNamedDeclaration(e) => {
                if let Some(Declaration::VariableDeclaration(decl)) = &e.declaration {
                    for d in &decl.declarations {
                        if let (Some(name), Some(init)) = (binding_name(&d.id), &d.init) {
                            let init = init.without_parentheses();
                            if let Some(n) = resolve_number(init, consts) {
                                consts.numbers.insert(name.to_string(), n);
                            }
                            if let Expression::ObjectExpression(obj) = init {
                                consts
                                    .schemas
                                    .insert(name.to_string(), object_to_frontmatter(obj));
                            }
                        }
                    }
                }
            }
            // Descend into blocks so consts declared inside `try { … }` etc. still
            // resolve (sandcastle wraps its orchestration in a try/finally).
            Statement::BlockStatement(b) => collect_consts(&b.body, consts, depth + 1),
            Statement::TryStatement(t) => {
                collect_consts(&t.block.body, consts, depth + 1);
                if let Some(f) = &t.finalizer {
                    collect_consts(&f.body, consts, depth + 1);
                }
                if let Some(h) = &t.handler {
                    collect_consts(&h.body.body, consts, depth + 1);
                }
            }
            Statement::ForStatement(f) => {
                if let Statement::BlockStatement(b) = &f.body {
                    collect_consts(&b.body, consts, depth + 1);
                }
            }
            Statement::WhileStatement(w) => {
                if let Statement::BlockStatement(b) = &w.body {
                    collect_consts(&b.body, consts, depth + 1);
                }
            }
            Statement::IfStatement(i) => {
                if let Statement::BlockStatement(b) = &i.consequent {
                    collect_consts(&b.body, consts, depth + 1);
                }
            }
            _ => {}
        }
    }
}
