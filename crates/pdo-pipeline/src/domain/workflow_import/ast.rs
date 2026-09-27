//! Reading the AST: calls, bindings, where the agents are, guarded exits.

use oxc_ast::ast::{CallExpression, Expression, Statement};

use super::*;

pub(super) fn binding_name<'a>(id: &'a oxc_ast::ast::BindingPattern<'a>) -> Option<&'a str> {
    match id {
        oxc_ast::ast::BindingPattern::BindingIdentifier(b) => Some(b.name.as_str()),
        _ => None,
    }
}

/// Strip parentheses and `await` wrappers.
pub(super) fn unwrap_expr<'a>(mut expr: &'a Expression<'a>) -> &'a Expression<'a> {
    loop {
        match expr {
            Expression::ParenthesizedExpression(p) => expr = &p.expression,
            Expression::AwaitExpression(a) => expr = &a.argument,
            _ => break,
        }
    }
    expr
}

/// If `expr` is (or wraps, via `.then()` chains) a call to `agent(...)`, return
/// the agent call.
pub(super) fn as_agent_call<'a>(expr: &'a Expression<'a>) -> Option<&'a CallExpression<'a>> {
    let e = unwrap_expr(expr);
    if let Expression::CallExpression(c) = e {
        if c.callee.is_specific_id("agent") {
            return Some(c);
        }
        // agent(...).then(...) / .catch(...)
        if let Expression::StaticMemberExpression(m) = c.callee.without_parentheses() {
            return as_agent_call(&m.object);
        }
    }
    None
}

pub(super) fn as_named_call<'a>(
    expr: &'a Expression<'a>,
    name: &str,
) -> Option<&'a CallExpression<'a>> {
    let e = unwrap_expr(expr);
    if let Expression::CallExpression(c) = e {
        if c.callee.is_specific_id(name) {
            return Some(c);
        }
    }
    None
}

pub(super) fn stmt_has_agent(stmt: &Statement, depth: u32) -> bool {
    if depth > MAX_DEPTH {
        return false;
    }
    match stmt {
        Statement::ExpressionStatement(es) => expr_has_agent(&es.expression, depth + 1),
        Statement::VariableDeclaration(d) => d.declarations.iter().any(|dc| {
            dc.init
                .as_ref()
                .is_some_and(|e| expr_has_agent(e, depth + 1))
        }),
        Statement::BlockStatement(b) => b.body.iter().any(|s| stmt_has_agent(s, depth + 1)),
        Statement::IfStatement(i) => {
            stmt_has_agent(&i.consequent, depth + 1)
                || i.alternate
                    .as_ref()
                    .is_some_and(|a| stmt_has_agent(a, depth + 1))
        }
        Statement::ForStatement(f) => stmt_has_agent(&f.body, depth + 1),
        Statement::WhileStatement(w) => stmt_has_agent(&w.body, depth + 1),
        Statement::DoWhileStatement(w) => stmt_has_agent(&w.body, depth + 1),
        Statement::TryStatement(t) => {
            t.block.body.iter().any(|s| stmt_has_agent(s, depth + 1))
                || t.finalizer
                    .as_ref()
                    .is_some_and(|f| f.body.iter().any(|s| stmt_has_agent(s, depth + 1)))
        }
        Statement::ReturnStatement(r) => r
            .argument
            .as_ref()
            .is_some_and(|e| expr_has_agent(e, depth + 1)),
        _ => false,
    }
}

pub(super) fn if_has_agent(i: &oxc_ast::ast::IfStatement) -> bool {
    stmt_has_agent(&i.consequent, 0) || i.alternate.as_ref().is_some_and(|a| stmt_has_agent(a, 0))
}

pub(super) fn expr_has_agent(expr: &Expression, depth: u32) -> bool {
    if depth > MAX_DEPTH {
        return false;
    }
    let mut found = Vec::new();
    collect_agents_in_expr(expr, &mut found, depth);
    !found.is_empty()
}

/// Collect every `agent(...)` call reachable from `expr` (in source order),
/// without descending into an agent call's own arguments.
pub(super) fn collect_agents_in_expr<'a>(
    expr: &'a Expression<'a>,
    out: &mut Vec<&'a CallExpression<'a>>,
    depth: u32,
) {
    if depth > MAX_DEPTH {
        return;
    }
    match expr.without_parentheses() {
        Expression::AwaitExpression(a) => collect_agents_in_expr(&a.argument, out, depth + 1),
        Expression::CallExpression(c) => {
            if c.callee.is_specific_id("agent") {
                out.push(c);
                return;
            }
            // Recurse into callee (for `agent(...).then(...)`) and arguments.
            collect_agents_in_expr(&c.callee, out, depth + 1);
            for a in &c.arguments {
                if let Some(e) = a.as_expression() {
                    collect_agents_in_expr(e, out, depth + 1);
                }
            }
        }
        Expression::StaticMemberExpression(m) => collect_agents_in_expr(&m.object, out, depth + 1),
        Expression::ComputedMemberExpression(m) => {
            collect_agents_in_expr(&m.object, out, depth + 1)
        }
        Expression::ArrowFunctionExpression(f) => {
            for s in &f.body.statements {
                collect_agents_in_stmt(s, out, depth + 1);
            }
        }
        Expression::FunctionExpression(f) => {
            if let Some(b) = &f.body {
                for s in &b.statements {
                    collect_agents_in_stmt(s, out, depth + 1);
                }
            }
        }
        Expression::ArrayExpression(arr) => {
            for el in &arr.elements {
                if let Some(e) = el.as_expression() {
                    collect_agents_in_expr(e, out, depth + 1);
                }
            }
        }
        Expression::BinaryExpression(b) => {
            collect_agents_in_expr(&b.left, out, depth + 1);
            collect_agents_in_expr(&b.right, out, depth + 1);
        }
        Expression::LogicalExpression(l) => {
            collect_agents_in_expr(&l.left, out, depth + 1);
            collect_agents_in_expr(&l.right, out, depth + 1);
        }
        Expression::ConditionalExpression(c) => {
            collect_agents_in_expr(&c.test, out, depth + 1);
            collect_agents_in_expr(&c.consequent, out, depth + 1);
            collect_agents_in_expr(&c.alternate, out, depth + 1);
        }
        _ => {}
    }
}

pub(super) fn collect_agents_in_stmt<'a>(
    stmt: &'a Statement<'a>,
    out: &mut Vec<&'a CallExpression<'a>>,
    depth: u32,
) {
    if depth > MAX_DEPTH {
        return;
    }
    match stmt {
        Statement::ExpressionStatement(es) => {
            collect_agents_in_expr(&es.expression, out, depth + 1)
        }
        Statement::ReturnStatement(r) => {
            if let Some(e) = &r.argument {
                collect_agents_in_expr(e, out, depth + 1);
            }
        }
        Statement::VariableDeclaration(d) => {
            for dc in &d.declarations {
                if let Some(e) = &dc.init {
                    collect_agents_in_expr(e, out, depth + 1);
                }
            }
        }
        Statement::BlockStatement(b) => {
            for s in &b.body {
                collect_agents_in_stmt(s, out, depth + 1);
            }
        }
        Statement::IfStatement(i) => {
            collect_agents_in_stmt(&i.consequent, out, depth + 1);
            if let Some(a) = &i.alternate {
                collect_agents_in_stmt(a, out, depth + 1);
            }
        }
        _ => {}
    }
}

#[derive(PartialEq, Eq)]
pub(super) enum GuardExit {
    Return,
    Break,
    None,
}

/// Does the `if` consequent guard a `return` or a `break`? Scans the direct
/// statements of the consequent (a block or a single statement).
pub(super) fn guarded_exit(consequent: &Statement) -> GuardExit {
    pub(super) fn scan(stmt: &Statement, depth: u32) -> GuardExit {
        if depth > MAX_DEPTH {
            return GuardExit::None;
        }
        match stmt {
            Statement::ReturnStatement(_) => GuardExit::Return,
            Statement::BreakStatement(_) => GuardExit::Break,
            Statement::BlockStatement(b) => {
                for s in &b.body {
                    let e = scan(s, depth + 1);
                    if e != GuardExit::None {
                        return e;
                    }
                }
                GuardExit::None
            }
            _ => GuardExit::None,
        }
    }
    scan(consequent, 0)
}

pub(super) fn expr_mentions_ident(expr: &Expression, ident: &str, depth: u32) -> bool {
    if depth > MAX_DEPTH {
        return false;
    }
    match expr.without_parentheses() {
        Expression::Identifier(id) => id.name.as_str() == ident,
        Expression::StaticMemberExpression(m) => expr_mentions_ident(&m.object, ident, depth + 1),
        Expression::ComputedMemberExpression(m) => {
            expr_mentions_ident(&m.object, ident, depth + 1)
                || expr_mentions_ident(&m.expression, ident, depth + 1)
        }
        Expression::BinaryExpression(b) => {
            expr_mentions_ident(&b.left, ident, depth + 1)
                || expr_mentions_ident(&b.right, ident, depth + 1)
        }
        Expression::LogicalExpression(l) => {
            expr_mentions_ident(&l.left, ident, depth + 1)
                || expr_mentions_ident(&l.right, ident, depth + 1)
        }
        Expression::ConditionalExpression(c) => {
            expr_mentions_ident(&c.test, ident, depth + 1)
                || expr_mentions_ident(&c.consequent, ident, depth + 1)
                || expr_mentions_ident(&c.alternate, ident, depth + 1)
        }
        Expression::UnaryExpression(u) => expr_mentions_ident(&u.argument, ident, depth + 1),
        Expression::CallExpression(c) => {
            expr_mentions_ident(&c.callee, ident, depth + 1)
                || c.arguments
                    .iter()
                    .filter_map(|a| a.as_expression())
                    .any(|e| expr_mentions_ident(e, ident, depth + 1))
        }
        _ => false,
    }
}
